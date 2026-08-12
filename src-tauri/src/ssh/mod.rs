use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver as StdReceiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::connection_log;
use crate::sftp::SftpAppState;
use crate::ssh_algorithms::{
    self, ConnectAttemptError, NegotiationProfile, NegotiationProfileCache,
};
use crate::terminal_transfer::{TerminalTransferControl, TerminalTransferRuntime};
use crate::tunnel::TunnelState;
use encoding_rs::Encoding;
use russh::keys::{check_known_hosts_path, HashAlg, PublicKey};
use russh::Pty;
use russh::{client, ChannelMsg, Disconnect};

use serialport::{
    available_ports, ClearBuffer, DataBits, FlowControl, Parity, SerialPortType, StopBits,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender,
};
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use zeroize::Zeroize;

pub(crate) mod auth;
pub(crate) mod channel_state;
pub(crate) mod supervisor;

#[cfg(unix)]
fn ensure_private_key_permissions(path: &PathBuf) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_key_permissions(_path: &PathBuf) -> Result<(), String> {
    Ok(())
}

#[derive(Clone, Default)]
pub struct SshAppState {
    negotiation_profiles: NegotiationProfileCache,
}

impl SshAppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn preferred_profile_for_endpoint(&self, host: &str, port: u16) -> NegotiationProfile {
        self.negotiation_profiles
            .preferred_profile_for_endpoint(host, port)
    }

    pub fn remember_successful_profile(&self, host: &str, port: u16, profile: NegotiationProfile) {
        self.negotiation_profiles
            .remember_successful_profile(host, port, profile);
    }
}

pub type SharedSshSession = Arc<AsyncMutex<client::Handle<ClientHandler>>>;
pub type SharedSshSessionSlot = Arc<Mutex<Option<SharedSshSession>>>;
type SharedChannelLifecycle = Arc<Mutex<channel_state::ChannelLifecycle>>;
const SSH_INPUT_QUEUE_CAPACITY: usize = 256;
const SERIAL_READ_TIMEOUT: Duration = Duration::from_millis(100);
const SERIAL_MIN_BAUD_RATE: u32 = 50;
const SERIAL_COMMAND_QUEUE_CAPACITY: usize = 256;
const SERIAL_STATUS_INTERVAL: Duration = Duration::from_millis(250);
const SERIAL_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) enum SerialControlRequest {
    WriteRaw(Vec<u8>),
    SendFile(String),
    SetDtr(bool),
    SetRts(bool),
    SetBreak(bool),
    Clear(String),
    StartCapture { path: String, append: bool },
    StopCapture,
    GetStatus,
}

#[derive(Debug)]
enum SerialCommand {
    Write(Vec<u8>),
    Control {
        request: SerialControlRequest,
        respond_to: oneshot::Sender<Result<SerialControlResponse, String>>,
    },
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SerialStatus {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_rate: u64,
    tx_rate: u64,
    cts: Option<bool>,
    dsr: Option<bool>,
    ri: Option<bool>,
    dcd: Option<bool>,
    capturing: bool,
    sending_file: bool,
}

#[derive(Debug)]
pub(crate) enum SerialControlResponse {
    Unit,
    Status(SerialStatus),
}

#[derive(Clone)]
struct SerialSharedState {
    stop: Arc<AtomicBool>,
    rx_bytes: Arc<AtomicU64>,
    tx_bytes: Arc<AtomicU64>,
    capture: Arc<Mutex<Option<File>>>,
    file_send_active: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct TerminalRuntimeHandle {
    pub tx: Sender<Vec<u8>>,
    pub window_size_tx: UnboundedSender<(u32, u32)>,
    pub close_tx: UnboundedSender<()>,
    pub shared_session: SharedSshSessionSlot,
    pub(crate) transfer_control_tx: Option<UnboundedSender<TerminalTransferControl>>,
    pub(crate) transfer_owned: Arc<AtomicBool>,
    channel_lifecycle: SharedChannelLifecycle,
    input_encoding: Option<String>,
    input_line_ending: Option<String>,
    serial_command_tx: Option<SyncSender<SerialCommand>>,
    serial_file_send_active: Option<Arc<AtomicBool>>,
}

pub type SessionIoReceiver = Receiver<Vec<u8>>;
pub type SessionResizeReceiver = tokio::sync::mpsc::UnboundedReceiver<(u32, u32)>;
pub type SessionCloseReceiver = tokio::sync::mpsc::UnboundedReceiver<()>;

pub(crate) fn create_terminal_runtime_channels() -> (
    TerminalRuntimeHandle,
    SessionIoReceiver,
    SessionResizeReceiver,
    SessionCloseReceiver,
) {
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (window_size_tx, window_size_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let shared_session = Arc::new(Mutex::new(None));
    let channel_lifecycle = Arc::new(Mutex::new(channel_state::ChannelLifecycle::default()));

    (
        TerminalRuntimeHandle {
            tx,
            window_size_tx,
            close_tx,
            shared_session,
            transfer_control_tx: None,
            transfer_owned: Arc::new(AtomicBool::new(false)),
            channel_lifecycle,
            input_encoding: None,
            input_line_ending: None,
            serial_command_tx: None,
            serial_file_send_active: None,
        },
        rx,
        window_size_rx,
        close_rx,
    )
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct SshConfig {
    pub(crate) protocol: Option<String>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: Option<String>,
    pub(crate) private_key_path: Option<String>,
    pub(crate) passphrase: Option<String>,
    pub(crate) connect_timeout: Option<u64>,
    pub(crate) keep_alive_interval: Option<u64>,
    pub(crate) term_type: Option<String>,
    pub(crate) encoding: Option<String>,
    pub(crate) login_script: Option<String>,
    pub(crate) jump_host: Option<String>,
    pub(crate) jump_port: Option<u16>,
    pub(crate) jump_username: Option<String>,
    pub(crate) jump_auth_type: Option<String>,
    pub(crate) jump_password: Option<String>,
    pub(crate) jump_private_key_path: Option<String>,
    pub(crate) jump_passphrase: Option<String>,
    pub(crate) serial_path: Option<String>,
    pub(crate) baud_rate: Option<u32>,
    pub(crate) data_bits: Option<u8>,
    pub(crate) stop_bits: Option<String>,
    pub(crate) parity: Option<String>,
    pub(crate) flow_control: Option<String>,
    pub(crate) serial_line_ending: Option<String>,
    pub(crate) serial_device_id: Option<String>,
    pub(crate) serial_connect_delay_ms: Option<u64>,
    pub(crate) serial_line_delay_ms: Option<u64>,
    pub(crate) local_profile: Option<String>,
    pub(crate) local_working_directory: Option<String>,
    pub(crate) initial_cols: Option<u16>,
    pub(crate) initial_rows: Option<u16>,
}

impl Drop for SshConfig {
    fn drop(&mut self) {
        if let Some(ref mut p) = self.password {
            p.zeroize();
        }
        if let Some(ref mut p) = self.passphrase {
            p.zeroize();
        }
        if let Some(ref mut p) = self.jump_password {
            p.zeroize();
        }
        if let Some(ref mut p) = self.jump_passphrase {
            p.zeroize();
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortOption {
    path: String,
    label: String,
    stable_id: String,
    vid: Option<u16>,
    pid: Option<u16>,
    serial_number: Option<String>,
}

fn normalized_protocol(value: Option<&str>) -> &'static str {
    match value.unwrap_or("ssh").trim().to_ascii_lowercase().as_str() {
        "telnet" => "telnet",
        "serial" => "serial",
        "local" => "local",
        _ => "ssh",
    }
}

pub(crate) fn is_local_protocol(config: &SshConfig) -> bool {
    normalized_protocol(config.protocol.as_deref()) == "local"
}

fn socket_address(host: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("地址解析失败: {}", e))?
        .next()
        .ok_or_else(|| "未解析到可用地址".to_string())
}

fn serial_port_stable_id(path: &str, port_type: &SerialPortType) -> String {
    match port_type {
        SerialPortType::UsbPort(info) => {
            if let Some(serial_number) = info
                .serial_number
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                format!("usb:{:04x}:{:04x}:{}", info.vid, info.pid, serial_number)
            } else {
                format!("usb:{:04x}:{:04x}:{}", info.vid, info.pid, path)
            }
        }
        SerialPortType::BluetoothPort => format!("bluetooth:{}", path),
        SerialPortType::PciPort => format!("pci:{}", path),
        SerialPortType::Unknown => format!("path:{}", path),
    }
}

fn configured_serial_port_path(config: &SshConfig) -> Result<String, String> {
    config
        .serial_path
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "串口设备路径不能为空".to_string())
}

fn serial_port_path(config: &SshConfig) -> Result<String, String> {
    let configured_path = configured_serial_port_path(config)?;
    let Some(stable_id) = config
        .serial_device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(configured_path);
    };

    let ports = match available_ports() {
        Ok(ports) => ports,
        Err(_) => return Ok(configured_path),
    };
    Ok(ports
        .into_iter()
        .find(|port| serial_port_stable_id(&port.port_name, &port.port_type) == stable_id)
        .map(|port| port.port_name)
        .unwrap_or(configured_path))
}

fn serial_baud_rate(config: &SshConfig) -> Result<u32, String> {
    let value = config.baud_rate.unwrap_or(9600);
    if value >= SERIAL_MIN_BAUD_RATE {
        Ok(value)
    } else {
        Err(format!("波特率不能低于 {}", SERIAL_MIN_BAUD_RATE))
    }
}

fn serial_data_bits(config: &SshConfig) -> Result<DataBits, String> {
    match config.data_bits.unwrap_or(8) {
        5 => Ok(DataBits::Five),
        6 => Ok(DataBits::Six),
        7 => Ok(DataBits::Seven),
        8 => Ok(DataBits::Eight),
        value => Err(format!("不支持的数据位: {}", value)),
    }
}

fn serial_stop_bits(config: &SshConfig) -> Result<StopBits, String> {
    match config.stop_bits.as_deref().unwrap_or("1").trim() {
        "1" => Ok(StopBits::One),
        "2" => Ok(StopBits::Two),
        value => Err(format!("不支持的停止位: {}", value)),
    }
}

fn serial_parity(config: &SshConfig) -> Result<Parity, String> {
    match config
        .parity
        .as_deref()
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => Ok(Parity::None),
        "odd" => Ok(Parity::Odd),
        "even" => Ok(Parity::Even),
        value => Err(format!("不支持的校验位: {}", value)),
    }
}

fn serial_flow_control(config: &SshConfig) -> Result<FlowControl, String> {
    match config
        .flow_control
        .as_deref()
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => Ok(FlowControl::None),
        "software" => Ok(FlowControl::Software),
        "hardware" => Ok(FlowControl::Hardware),
        value => Err(format!("不支持的流控方式: {}", value)),
    }
}

fn serial_text_encoding(value: Option<&str>) -> Result<Option<String>, String> {
    let label = value.unwrap_or("UTF-8").trim();
    if label.is_empty() || label.eq_ignore_ascii_case("UTF-8") || label.eq_ignore_ascii_case("UTF8")
    {
        return Ok(None);
    }
    Encoding::for_label(label.as_bytes())
        .map(|_| Some(label.to_string()))
        .ok_or_else(|| format!("不支持的串口字符编码: {}", label))
}

fn serial_line_ending(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("cr").trim().to_ascii_lowercase().as_str() {
        "cr" => Ok("\r"),
        "lf" => Ok("\n"),
        "crlf" => Ok("\r\n"),
        value => Err(format!("不支持的串口行尾: {}", value)),
    }
}

fn serial_login_line_ending(value: Option<&str>) -> Result<&'static str, String> {
    match value {
        Some(value) => serial_line_ending(Some(value)),
        None => Ok("\r\n"),
    }
}

fn normalize_serial_line_endings(value: &str, line_ending: &str) -> String {
    let mut result = String::with_capacity(value.len() + line_ending.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push_str(line_ending);
            }
            '\n' => result.push_str(line_ending),
            _ => result.push(ch),
        }
    }
    result
}

fn encode_text(value: &str, encoding: Option<&str>) -> Result<Vec<u8>, String> {
    let Some(label) = encoding else {
        return Ok(value.as_bytes().to_vec());
    };
    let codec = Encoding::for_label(label.as_bytes())
        .ok_or_else(|| format!("不支持的串口字符编码: {}", label))?;
    let (encoded, _, had_errors) = codec.encode(value);
    if had_errors {
        return Err(format!("文本包含无法使用 {} 编码的字符", label));
    }
    Ok(encoded.into_owned())
}

fn encode_runtime_input(handle: &TerminalRuntimeHandle, value: &str) -> Result<Vec<u8>, String> {
    let normalized = match handle.input_line_ending.as_deref() {
        Some(line_ending) => normalize_serial_line_endings(value, line_ending),
        None => value.to_string(),
    };
    encode_text(&normalized, handle.input_encoding.as_deref())
}

fn validate_serial_config(config: &SshConfig) -> Result<(), String> {
    configured_serial_port_path(config)?;
    serial_baud_rate(config)?;
    serial_data_bits(config)?;
    serial_stop_bits(config)?;
    serial_parity(config)?;
    serial_flow_control(config)?;
    serial_text_encoding(config.encoding.as_deref())?;
    serial_line_ending(config.serial_line_ending.as_deref())?;
    if config.serial_connect_delay_ms.unwrap_or(0) > 60_000 {
        return Err("串口连接后延迟不能超过 60000 毫秒".to_string());
    }
    if config.serial_line_delay_ms.unwrap_or(0) > 60_000 {
        return Err("串口脚本行延迟不能超过 60000 毫秒".to_string());
    }
    Ok(())
}

fn serial_runtime_input_options(
    config: &SshConfig,
) -> Result<(Option<String>, Option<String>), String> {
    validate_serial_config(config)?;
    Ok((
        serial_text_encoding(config.encoding.as_deref())?,
        Some(serial_line_ending(config.serial_line_ending.as_deref())?.to_string()),
    ))
}

fn build_serial_port(config: &SshConfig) -> Result<Box<dyn serialport::SerialPort>, String> {
    validate_serial_config(config)?;
    let path = serial_port_path(config)?;
    serialport::new(path, serial_baud_rate(config)?)
        .data_bits(serial_data_bits(config)?)
        .stop_bits(serial_stop_bits(config)?)
        .parity(serial_parity(config)?)
        .flow_control(serial_flow_control(config)?)
        .timeout(SERIAL_READ_TIMEOUT)
        .open()
        .map_err(|e| format!("串口打开失败: {}", e))
}

#[cfg(test)]
mod serial_tests {
    use super::*;

    fn serial_config() -> SshConfig {
        SshConfig {
            protocol: Some("serial".to_string()),
            host: String::new(),
            port: 0,
            username: String::new(),
            password: None,
            private_key_path: None,
            passphrase: None,
            connect_timeout: None,
            keep_alive_interval: None,
            term_type: None,
            encoding: Some("UTF-8".to_string()),
            login_script: None,
            jump_host: None,
            jump_port: None,
            jump_username: None,
            jump_auth_type: None,
            jump_password: None,
            jump_private_key_path: None,
            jump_passphrase: None,
            serial_path: Some("COM1".to_string()),
            baud_rate: Some(9600),
            data_bits: Some(8),
            stop_bits: Some("1".to_string()),
            parity: Some("none".to_string()),
            flow_control: Some("none".to_string()),
            serial_line_ending: Some("cr".to_string()),
            serial_device_id: None,
            serial_connect_delay_ms: Some(0),
            serial_line_delay_ms: Some(0),
            local_profile: None,
            local_working_directory: None,
            initial_cols: None,
            initial_rows: None,
        }
    }

    #[test]
    fn validates_serial_baud_rate_range() {
        let mut config = serial_config();
        config.baud_rate = Some(SERIAL_MIN_BAUD_RATE);
        assert_eq!(serial_baud_rate(&config).unwrap(), SERIAL_MIN_BAUD_RATE);
        config.baud_rate = Some(SERIAL_MIN_BAUD_RATE - 1);
        assert!(serial_baud_rate(&config).is_err());
        config.baud_rate = Some(4_000_000);
        assert_eq!(serial_baud_rate(&config).unwrap(), 4_000_000);
    }

    #[test]
    fn normalizes_mixed_line_endings() {
        assert_eq!(
            normalize_serial_line_endings("one\rtwo\nthree\r\nfour", "\r\n"),
            "one\r\ntwo\r\nthree\r\nfour"
        );
        assert_eq!(serial_line_ending(None).unwrap(), "\r");
        assert_eq!(serial_login_line_ending(None).unwrap(), "\r\n");
    }

    #[test]
    fn encodes_serial_input_with_configured_encoding_and_line_ending() {
        let (mut handle, _rx, _resize_rx, _close_rx) = create_terminal_runtime_channels();
        handle.input_encoding = Some("GBK".to_string());
        handle.input_line_ending = Some("\r\n".to_string());

        let bytes = encode_runtime_input(&handle, "中文\r").unwrap();
        let (decoded, _, had_errors) = encoding_rs::GBK.decode(&bytes);

        assert!(!had_errors);
        assert_eq!(decoded, "中文\r\n");
    }

    #[test]
    fn rejects_missing_path_and_unknown_encoding() {
        let mut config = serial_config();
        config.serial_path = Some("  ".to_string());
        assert!(validate_serial_config(&config).is_err());

        config.serial_path = Some("COM1".to_string());
        config.encoding = Some("not-an-encoding".to_string());
        assert!(validate_serial_config(&config).is_err());
    }
}

fn emit_session_error(app_handle: &AppHandle, session_id: &str, error: impl Into<String>) {
    let _ = app_handle.emit(&format!("ssh-error-{}", session_id), error.into());
}

fn emit_session_closed(app_handle: &AppHandle, session_id: &str, reason: impl Into<String>) {
    let _ = app_handle.emit(&format!("ssh-closed-{}", session_id), reason.into());
}

fn terminate_channel(
    app_handle: &AppHandle,
    session_id: &str,
    lifecycle: &SharedChannelLifecycle,
    cause: channel_state::TerminalCause,
) -> bool {
    let reason = cause.reason();
    let first = lifecycle.lock().unwrap().terminate(cause);
    if first {
        connection_log::append(
            session_id,
            format!("channel terminal state reason={}", reason),
        );
        emit_session_closed(app_handle, session_id, reason);
    }
    first
}

fn default_terminal_modes() -> Vec<(Pty, u32)> {
    vec![
        (Pty::VINTR, 3),
        (Pty::VQUIT, 28),
        (Pty::VERASE, 127),
        (Pty::VKILL, 21),
        (Pty::VEOF, 4),
        (Pty::VSTART, 17),
        (Pty::VSTOP, 19),
        (Pty::VSUSP, 26),
        (Pty::ICRNL, 1),
        (Pty::IXON, 1),
        (Pty::IUTF8, 1),
        (Pty::ISIG, 1),
        (Pty::ICANON, 1),
        (Pty::ECHO, 1),
        (Pty::ECHOE, 1),
        (Pty::ECHOK, 1),
        (Pty::IEXTEN, 1),
        (Pty::OPOST, 1),
        (Pty::ONLCR, 1),
        (Pty::CS8, 1),
        (Pty::TTY_OP_ISPEED, 38400),
        (Pty::TTY_OP_OSPEED, 38400),
    ]
}

fn cleanup_session_state(
    shared_session: &SharedSshSessionSlot,
    sftp_state: &SftpAppState,
    session_id: &str,
) {
    let mut slot = shared_session.lock().unwrap();
    slot.take();
    drop(slot);
    crate::sftp::cleanup_session_state(sftp_state, session_id);
}

fn fail_session_connect(
    app_handle: &AppHandle,
    shared_session: &SharedSshSessionSlot,
    sftp_state: &SftpAppState,
    session_id: &str,
    error: impl Into<String>,
) {
    emit_session_error(app_handle, session_id, error.into());
    cleanup_session_state(shared_session, sftp_state, session_id);
}

fn maybe_send_login_script<W: Write>(
    writer: &mut W,
    login_script: Option<&String>,
    line_ending: &[u8],
) -> Result<(), String> {
    if let Some(script) = login_script {
        let trimmed = script.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        writer
            .write_all(trimmed.as_bytes())
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
        writer
            .write_all(line_ending)
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
        writer
            .flush()
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
    }
    Ok(())
}

fn wait_serial_delay(
    delay: Duration,
    close_rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        if close_rx.try_recv().is_ok() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(20)));
    }
}

fn maybe_send_serial_login_script<W: Write>(
    app_handle: &AppHandle,
    session_id: &str,
    writer: &mut W,
    login_script: Option<&String>,
    encoding: Option<&str>,
    line_ending: &str,
    line_delay: Duration,
    close_rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
) -> Result<bool, String> {
    if let Some(script) = login_script {
        let trimmed = script.trim_matches(|ch| ch == '\r' || ch == '\n');
        if trimmed.is_empty() {
            return Ok(false);
        }
        let lines = trimmed.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if close_rx.try_recv().is_ok() {
                return Ok(true);
            }
            let payload = encode_text(
                &format!("{}{}", line.trim_end_matches('\r'), line_ending),
                encoding,
            )?;
            writer
                .write_all(&payload)
                .map_err(|e| format!("发送登录脚本失败: {}", e))?;
            let _ = app_handle.emit(&format!("serial-data-sent-{}", session_id), payload);
            if !line_delay.is_zero()
                && index + 1 < lines.len()
                && wait_serial_delay(line_delay, close_rx)
            {
                return Ok(true);
            }
        }
        writer
            .flush()
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
    }
    Ok(false)
}

fn normalize_telnet_term_type(value: Option<&str>) -> String {
    let trimmed = value.unwrap_or("").trim();
    let valid = trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("network") || !valid {
        return "xterm-256color".to_string();
    }
    trimmed.to_string()
}

#[derive(Clone, Debug)]
struct TelnetNegotiator {
    term_type: String,
    cols: u32,
    rows: u32,
    naws_enabled: bool,
}

impl TelnetNegotiator {
    const IAC: u8 = 255;
    const DONT: u8 = 254;
    const DO: u8 = 253;
    const WONT: u8 = 252;
    const WILL: u8 = 251;
    const SB: u8 = 250;
    const SE: u8 = 240;

    const OPT_ECHO: u8 = 1;
    const OPT_SGA: u8 = 3;
    const OPT_TTYPE: u8 = 24;
    const OPT_NAWS: u8 = 31;
    const OPT_LINEMODE: u8 = 34;

    const TTYPE_IS: u8 = 0;
    const TTYPE_SEND: u8 = 1;

    fn new(term_type: String, cols: u32, rows: u32) -> Self {
        Self {
            term_type,
            cols: cols.clamp(1, u16::MAX as u32),
            rows: rows.clamp(1, u16::MAX as u32),
            naws_enabled: false,
        }
    }

    fn set_window_size(&mut self, cols: u32, rows: u32) -> Vec<u8> {
        self.cols = cols.clamp(1, u16::MAX as u32);
        self.rows = rows.clamp(1, u16::MAX as u32);
        if self.naws_enabled {
            self.naws_response()
        } else {
            Vec::new()
        }
    }

    fn push_command(response: &mut Vec<u8>, command: u8, option: u8) {
        response.extend_from_slice(&[Self::IAC, command, option]);
    }

    fn push_subnegotiation_byte(response: &mut Vec<u8>, value: u8) {
        response.push(value);
        if value == Self::IAC {
            response.push(Self::IAC);
        }
    }

    fn naws_response(&self) -> Vec<u8> {
        let cols = self.cols as u16;
        let rows = self.rows as u16;
        let mut response = vec![Self::IAC, Self::SB, Self::OPT_NAWS];
        for value in [(cols >> 8) as u8, cols as u8, (rows >> 8) as u8, rows as u8] {
            Self::push_subnegotiation_byte(&mut response, value);
        }
        response.extend_from_slice(&[Self::IAC, Self::SE]);
        response
    }

    fn ttype_response(&self) -> Vec<u8> {
        let mut response = vec![Self::IAC, Self::SB, Self::OPT_TTYPE, Self::TTYPE_IS];
        response.extend_from_slice(self.term_type.as_bytes());
        response.extend_from_slice(&[Self::IAC, Self::SE]);
        response
    }

    fn handle_option(&mut self, command: u8, option: u8, responses: &mut Vec<u8>) {
        match command {
            Self::DO => match option {
                Self::OPT_TTYPE | Self::OPT_SGA => {
                    Self::push_command(responses, Self::WILL, option);
                }
                Self::OPT_NAWS => {
                    self.naws_enabled = true;
                    Self::push_command(responses, Self::WILL, option);
                    responses.extend_from_slice(&self.naws_response());
                }
                _ => Self::push_command(responses, Self::WONT, option),
            },
            Self::DONT if option == Self::OPT_NAWS => self.naws_enabled = false,
            Self::DONT => {}
            Self::WILL => match option {
                Self::OPT_ECHO | Self::OPT_SGA => {
                    Self::push_command(responses, Self::DO, option);
                }
                Self::OPT_LINEMODE => {
                    Self::push_command(responses, Self::DONT, option);
                }
                _ => Self::push_command(responses, Self::DONT, option),
            },
            Self::WONT => {}
            _ => {}
        }
    }

    fn handle_subnegotiation(&mut self, payload: &[u8], responses: &mut Vec<u8>) {
        if payload.len() >= 2 && payload[0] == Self::OPT_TTYPE && payload[1] == Self::TTYPE_SEND {
            responses.extend_from_slice(&self.ttype_response());
        }
    }

    fn parse(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut data = Vec::new();
        let mut responses = Vec::new();
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index] != Self::IAC {
                data.push(bytes[index]);
                index += 1;
                continue;
            }

            if index + 1 >= bytes.len() {
                return (data, responses, bytes[index..].to_vec());
            }

            let command = bytes[index + 1];
            match command {
                Self::IAC => {
                    data.push(Self::IAC);
                    index += 2;
                }
                Self::DO | Self::DONT | Self::WILL | Self::WONT => {
                    if index + 2 >= bytes.len() {
                        return (data, responses, bytes[index..].to_vec());
                    }
                    self.handle_option(command, bytes[index + 2], &mut responses);
                    index += 3;
                }
                Self::SB => {
                    let payload_start = index + 2;
                    let mut end = payload_start;
                    let mut found = false;
                    while end < bytes.len() {
                        if bytes[end] == Self::IAC {
                            if end + 1 >= bytes.len() {
                                return (data, responses, bytes[index..].to_vec());
                            }
                            if bytes[end + 1] == Self::SE {
                                found = true;
                                break;
                            }
                            end += 2;
                            continue;
                        }
                        end += 1;
                    }
                    if !found {
                        return (data, responses, bytes[index..].to_vec());
                    }
                    self.handle_subnegotiation(&bytes[payload_start..end], &mut responses);
                    index = end + 2;
                }
                _ => {
                    index += 2;
                }
            }
        }

        (data, responses, Vec::new())
    }
}

#[cfg(test)]
mod telnet_tests {
    use super::{normalize_telnet_term_type, TelnetNegotiator};

    #[test]
    fn telnet_accepts_remote_echo() {
        let mut negotiator = TelnetNegotiator::new("xterm-256color".to_string(), 80, 24);
        let (_data, responses, remainder) = negotiator.parse(&[
            TelnetNegotiator::IAC,
            TelnetNegotiator::WILL,
            TelnetNegotiator::OPT_ECHO,
        ]);

        assert!(remainder.is_empty());
        assert_eq!(
            responses,
            vec![
                TelnetNegotiator::IAC,
                TelnetNegotiator::DO,
                TelnetNegotiator::OPT_ECHO,
            ]
        );
    }

    #[test]
    fn telnet_reports_configured_terminal_type() {
        let term_type = normalize_telnet_term_type(Some("network"));
        let mut negotiator = TelnetNegotiator::new(term_type, 80, 24);
        let (_data, responses, remainder) = negotiator.parse(&[
            TelnetNegotiator::IAC,
            TelnetNegotiator::DO,
            TelnetNegotiator::OPT_TTYPE,
            TelnetNegotiator::IAC,
            TelnetNegotiator::SB,
            TelnetNegotiator::OPT_TTYPE,
            TelnetNegotiator::TTYPE_SEND,
            TelnetNegotiator::IAC,
            TelnetNegotiator::SE,
        ]);

        let mut expected = vec![
            TelnetNegotiator::IAC,
            TelnetNegotiator::WILL,
            TelnetNegotiator::OPT_TTYPE,
            TelnetNegotiator::IAC,
            TelnetNegotiator::SB,
            TelnetNegotiator::OPT_TTYPE,
            TelnetNegotiator::TTYPE_IS,
        ];
        expected.extend_from_slice(b"xterm-256color");
        expected.extend_from_slice(&[TelnetNegotiator::IAC, TelnetNegotiator::SE]);

        assert!(remainder.is_empty());
        assert_eq!(responses, expected);
    }

    #[test]
    fn telnet_sends_naws_and_updates_on_resize() {
        let mut negotiator = TelnetNegotiator::new("xterm-256color".to_string(), 80, 24);
        let (_data, responses, remainder) = negotiator.parse(&[
            TelnetNegotiator::IAC,
            TelnetNegotiator::DO,
            TelnetNegotiator::OPT_NAWS,
        ]);

        assert!(remainder.is_empty());
        assert_eq!(
            responses,
            vec![
                TelnetNegotiator::IAC,
                TelnetNegotiator::WILL,
                TelnetNegotiator::OPT_NAWS,
                TelnetNegotiator::IAC,
                TelnetNegotiator::SB,
                TelnetNegotiator::OPT_NAWS,
                0,
                80,
                0,
                24,
                TelnetNegotiator::IAC,
                TelnetNegotiator::SE,
            ]
        );

        assert_eq!(
            negotiator.set_window_size(100, 40),
            vec![
                TelnetNegotiator::IAC,
                TelnetNegotiator::SB,
                TelnetNegotiator::OPT_NAWS,
                0,
                100,
                0,
                40,
                TelnetNegotiator::IAC,
                TelnetNegotiator::SE,
            ]
        );
    }

    #[test]
    fn telnet_keeps_partial_iac_as_remainder() {
        let mut negotiator = TelnetNegotiator::new("xterm-256color".to_string(), 80, 24);
        let (data, responses, remainder) = negotiator.parse(&[b'a', TelnetNegotiator::IAC]);

        assert_eq!(data, b"a");
        assert!(responses.is_empty());
        assert_eq!(remainder, vec![TelnetNegotiator::IAC]);

        let mut combined = remainder;
        combined.extend_from_slice(&[TelnetNegotiator::WILL, TelnetNegotiator::OPT_ECHO]);
        let (_data, responses, remainder) = negotiator.parse(&combined);

        assert!(remainder.is_empty());
        assert_eq!(
            responses,
            vec![
                TelnetNegotiator::IAC,
                TelnetNegotiator::DO,
                TelnetNegotiator::OPT_ECHO,
            ]
        );
    }
}

fn spawn_telnet_session(
    app_handle: AppHandle,
    session_id: String,
    config: SshConfig,
    mut rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    thread::spawn(move || {
        let outcome = (|| -> Result<(), String> {
            let address = socket_address(&config.host, config.port)?;
            let timeout = Duration::from_secs(config.connect_timeout.unwrap_or(10).clamp(1, 120));
            let mut stream = TcpStream::connect_timeout(&address, timeout)
                .map_err(|e| format!("Telnet 连接失败: {}", e))?;
            stream
                .set_read_timeout(Some(Duration::from_millis(120)))
                .map_err(|e| format!("Telnet 读超时设置失败: {}", e))?;
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| format!("Telnet 写超时设置失败: {}", e))?;

            let _ = app_handle.emit(&format!("ssh-connected-{}", session_id), ());

            let login_script = config.login_script.clone();
            let username = config.username.trim().to_string();
            let password = config.password.clone().unwrap_or_default();
            let keepalive_interval = config.keep_alive_interval.unwrap_or(0);
            let term_type = normalize_telnet_term_type(config.term_type.as_deref());
            let mut last_keepalive = Instant::now();
            let mut sent_username = username.is_empty();
            let mut sent_password = password.is_empty();
            let mut script_sent = false;
            let mut prompt_cache = String::new();
            let mut carry = Vec::new();
            let mut read_buf = [0u8; 4096];
            let mut negotiator = TelnetNegotiator::new(term_type, 80, 24);

            loop {
                if close_rx.try_recv().is_ok() {
                    let _ = stream.shutdown(Shutdown::Both);
                    break;
                }

                while let Ok(data) = rx.try_recv() {
                    if !data.is_empty() {
                        stream
                            .write_all(&data)
                            .map_err(|e| format!("Telnet 写入失败: {}", e))?;
                        stream
                            .flush()
                            .map_err(|e| format!("Telnet 写入失败: {}", e))?;
                        last_keepalive = Instant::now();
                    }
                }

                while let Ok((cols, rows)) = resize_rx.try_recv() {
                    let response = negotiator.set_window_size(cols, rows);
                    if !response.is_empty() {
                        let _ = stream.write_all(&response);
                        let _ = stream.flush();
                        last_keepalive = Instant::now();
                    }
                }

                if keepalive_interval > 0
                    && last_keepalive.elapsed() >= Duration::from_secs(keepalive_interval)
                {
                    let _ = stream.write_all(&[255, 241]);
                    let _ = stream.flush();
                    last_keepalive = Instant::now();
                }

                match stream.read(&mut read_buf) {
                    Ok(0) => break,
                    Ok(read) => {
                        let mut combined = Vec::with_capacity(carry.len() + read);
                        combined.extend_from_slice(&carry);
                        combined.extend_from_slice(&read_buf[..read]);
                        let (terminal_bytes, responses, remainder) = negotiator.parse(&combined);
                        carry = remainder;

                        if !responses.is_empty() {
                            let _ = stream.write_all(&responses);
                            let _ = stream.flush();
                            last_keepalive = Instant::now();
                        }

                        if !terminal_bytes.is_empty() {
                            let _ = app_handle
                                .emit(&format!("ssh-data-{}", session_id), terminal_bytes.clone());
                            last_keepalive = Instant::now();

                            let text = String::from_utf8_lossy(&terminal_bytes).to_lowercase();
                            if !text.trim().is_empty() {
                                prompt_cache.push_str(&text);
                                if prompt_cache.len() > 800 {
                                    let start = prompt_cache.len().saturating_sub(800);
                                    prompt_cache = prompt_cache[start..].to_string();
                                }
                            }

                            if !sent_username
                                && (prompt_cache.contains("login:")
                                    || prompt_cache.contains("username:"))
                            {
                                stream
                                    .write_all(format!("{}\r\n", username).as_bytes())
                                    .map_err(|e| format!("Telnet 发送用户名失败: {}", e))?;
                                stream
                                    .flush()
                                    .map_err(|e| format!("Telnet 发送用户名失败: {}", e))?;
                                sent_username = true;
                            }

                            if !sent_password && prompt_cache.contains("password:") {
                                stream
                                    .write_all(format!("{}\r\n", password).as_bytes())
                                    .map_err(|e| format!("Telnet 发送密码失败: {}", e))?;
                                stream
                                    .flush()
                                    .map_err(|e| format!("Telnet 发送密码失败: {}", e))?;
                                sent_password = true;
                            }

                            if !script_sent && sent_username && sent_password {
                                maybe_send_login_script(
                                    &mut stream,
                                    login_script.as_ref(),
                                    b"\r\n",
                                )?;
                                script_sent = true;
                            }
                        }
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        if !script_sent && username.is_empty() && password.is_empty() {
                            maybe_send_login_script(&mut stream, login_script.as_ref(), b"\r\n")?;
                            script_sent = true;
                        }
                    }
                    Err(error) => return Err(format!("Telnet 读取失败: {}", error)),
                }
            }

            Ok(())
        })();

        if let Err(error) = outcome {
            emit_session_error(&app_handle, &session_id, error);
        }
        emit_session_closed(&app_handle, &session_id, "session closed");
    });
}

fn current_serial_status(
    port: &mut dyn serialport::SerialPort,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_rate: u64,
    tx_rate: u64,
    capturing: bool,
    sending_file: bool,
) -> SerialStatus {
    SerialStatus {
        rx_bytes,
        tx_bytes,
        rx_rate,
        tx_rate,
        cts: port.read_clear_to_send().ok(),
        dsr: port.read_data_set_ready().ok(),
        ri: port.read_ring_indicator().ok(),
        dcd: port.read_carrier_detect().ok(),
        capturing,
        sending_file,
    }
}

fn write_serial_payload(
    app_handle: &AppHandle,
    session_id: &str,
    port: &mut dyn serialport::SerialPort,
    tx_bytes: &AtomicU64,
    payload: Vec<u8>,
) -> Result<(), String> {
    if payload.is_empty() {
        return Ok(());
    }
    port.write_all(&payload)
        .map_err(|error| format!("串口写入失败: {}", error))?;
    tx_bytes.fetch_add(payload.len() as u64, Ordering::Relaxed);
    let _ = app_handle.emit(&format!("serial-data-sent-{}", session_id), payload);
    Ok(())
}

fn handle_serial_control(
    app_handle: &AppHandle,
    session_id: &str,
    port: &mut dyn serialport::SerialPort,
    request: SerialControlRequest,
    shared: &SerialSharedState,
) -> Result<SerialControlResponse, String> {
    match request {
        SerialControlRequest::WriteRaw(payload) => {
            write_serial_payload(app_handle, session_id, port, &shared.tx_bytes, payload)?;
            Ok(SerialControlResponse::Unit)
        }
        SerialControlRequest::SendFile(path) => {
            let file =
                File::open(&path).map_err(|error| format!("打开待发送文件失败: {}", error))?;
            let mut reader = BufReader::new(file);
            let mut buffer = vec![0u8; 1024];
            loop {
                if shared.stop.load(Ordering::Acquire) {
                    return Err("串口文件发送已取消".to_string());
                }
                let read = reader
                    .read(&mut buffer)
                    .map_err(|error| format!("读取待发送文件失败: {}", error))?;
                if read == 0 {
                    break;
                }
                write_serial_payload(
                    app_handle,
                    session_id,
                    port,
                    &shared.tx_bytes,
                    buffer[..read].to_vec(),
                )?;
                while port.bytes_to_write().unwrap_or(0) > 8192 {
                    if shared.stop.load(Ordering::Acquire) {
                        return Err("串口文件发送已取消".to_string());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
            Ok(SerialControlResponse::Unit)
        }
        SerialControlRequest::SetDtr(level) => port
            .write_data_terminal_ready(level)
            .map(|_| SerialControlResponse::Unit)
            .map_err(|error| format!("设置 DTR 失败: {}", error)),
        SerialControlRequest::SetRts(level) => port
            .write_request_to_send(level)
            .map(|_| SerialControlResponse::Unit)
            .map_err(|error| format!("设置 RTS 失败: {}", error)),
        SerialControlRequest::SetBreak(enabled) => {
            let result = if enabled {
                port.set_break()
            } else {
                port.clear_break()
            };
            result
                .map(|_| SerialControlResponse::Unit)
                .map_err(|error| format!("设置 BREAK 失败: {}", error))
        }
        SerialControlRequest::Clear(target) => {
            let target = match target.trim().to_ascii_lowercase().as_str() {
                "input" | "rx" => ClearBuffer::Input,
                "output" | "tx" => ClearBuffer::Output,
                "all" => ClearBuffer::All,
                _ => return Err("清理目标必须为 input、output 或 all".to_string()),
            };
            port.clear(target)
                .map(|_| SerialControlResponse::Unit)
                .map_err(|error| format!("清理串口缓冲区失败: {}", error))
        }
        SerialControlRequest::StartCapture { path, append } => {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&path)
                .map_err(|error| format!("打开串口抓取文件失败: {}", error))?;
            *shared.capture.lock().unwrap() = Some(file);
            Ok(SerialControlResponse::Unit)
        }
        SerialControlRequest::StopCapture => {
            *shared.capture.lock().unwrap() = None;
            Ok(SerialControlResponse::Unit)
        }
        SerialControlRequest::GetStatus => {
            Ok(SerialControlResponse::Status(current_serial_status(
                port,
                shared.rx_bytes.load(Ordering::Relaxed),
                shared.tx_bytes.load(Ordering::Relaxed),
                0,
                0,
                shared.capture.lock().unwrap().is_some(),
                shared.file_send_active.load(Ordering::Acquire),
            )))
        }
    }
}

fn spawn_serial_session(
    app_handle: AppHandle,
    session_id: String,
    config: SshConfig,
    command_rx: StdReceiver<SerialCommand>,
    mut close_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    channel_lifecycle: SharedChannelLifecycle,
    file_send_active: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let outcome = (|| -> Result<(), String> {
            let mut read_port = build_serial_port(&config)?;
            let mut write_port = read_port
                .try_clone()
                .map_err(|error| format!("复制串口句柄失败: {}", error))?;
            let encoding = serial_text_encoding(config.encoding.as_deref())?;
            let line_ending = serial_login_line_ending(config.serial_line_ending.as_deref())?;
            let connect_delay = Duration::from_millis(config.serial_connect_delay_ms.unwrap_or(0));
            let line_delay = Duration::from_millis(config.serial_line_delay_ms.unwrap_or(0));
            let shared = SerialSharedState {
                stop: Arc::new(AtomicBool::new(false)),
                rx_bytes: Arc::new(AtomicU64::new(0)),
                tx_bytes: Arc::new(AtomicU64::new(0)),
                capture: Arc::new(Mutex::new(None::<File>)),
                file_send_active,
            };
            let writer_error = Arc::new(Mutex::new(None::<String>));

            let _ = app_handle.emit(&format!("ssh-connected-{}", session_id), ());
            if !connect_delay.is_zero() && wait_serial_delay(connect_delay, &mut close_rx) {
                return Ok(());
            }
            if maybe_send_serial_login_script(
                &app_handle,
                &session_id,
                &mut write_port,
                config.login_script.as_ref(),
                encoding.as_deref(),
                line_ending,
                line_delay,
                &mut close_rx,
            )? {
                return Ok(());
            }

            let writer_app_handle = app_handle.clone();
            let writer_session_id = session_id.clone();
            let writer_shared = shared.clone();
            let writer_error_slot = writer_error.clone();
            let writer = thread::spawn(move || {
                while !writer_shared.stop.load(Ordering::Acquire) {
                    let command = match command_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(command) => command,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    match command {
                        SerialCommand::Write(payload) => {
                            if let Err(error) = write_serial_payload(
                                &writer_app_handle,
                                &writer_session_id,
                                write_port.as_mut(),
                                &writer_shared.tx_bytes,
                                payload,
                            ) {
                                *writer_error_slot.lock().unwrap() = Some(error);
                                writer_shared.stop.store(true, Ordering::Release);
                            }
                        }
                        SerialCommand::Control {
                            request,
                            respond_to,
                        } => {
                            let background_operation =
                                matches!(&request, SerialControlRequest::SendFile(_));
                            let result = handle_serial_control(
                                &writer_app_handle,
                                &writer_session_id,
                                write_port.as_mut(),
                                request,
                                &writer_shared,
                            );
                            if background_operation {
                                writer_shared
                                    .file_send_active
                                    .store(false, Ordering::Release);
                                if let Err(error) = &result {
                                    let _ = writer_app_handle.emit(
                                        &format!("serial-operation-error-{}", writer_session_id),
                                        error.clone(),
                                    );
                                }
                            }
                            let _ = respond_to.send(result);
                        }
                    }
                }
            });

            let mut read_buf = [0u8; 4096];
            let mut last_status_at = Instant::now();
            let mut last_rx_bytes = 0u64;
            let mut last_tx_bytes = 0u64;
            let mut read_error = None;
            while !shared.stop.load(Ordering::Acquire) {
                if close_rx.try_recv().is_ok() {
                    break;
                }

                match read_port.read(&mut read_buf) {
                    Ok(0) => {}
                    Ok(read) => {
                        shared.rx_bytes.fetch_add(read as u64, Ordering::Relaxed);
                        let capture_error = {
                            let mut capture_guard = shared.capture.lock().unwrap();
                            let result = capture_guard
                                .as_mut()
                                .map(|file| file.write_all(&read_buf[..read]));
                            match result {
                                Some(Err(error)) => {
                                    *capture_guard = None;
                                    Some(format!("写入串口抓取文件失败，已停止抓取: {}", error))
                                }
                                _ => None,
                            }
                        };
                        if let Some(error) = capture_error {
                            let _ = app_handle
                                .emit(&format!("serial-operation-error-{}", session_id), error);
                        }
                        let _ = app_handle.emit(
                            &format!("ssh-data-{}", session_id),
                            read_buf[..read].to_vec(),
                        );
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(error) => {
                        read_error = Some(format!("串口读取失败: {}", error));
                        break;
                    }
                }

                if last_status_at.elapsed() >= SERIAL_STATUS_INTERVAL {
                    let current_rx = shared.rx_bytes.load(Ordering::Relaxed);
                    let current_tx = shared.tx_bytes.load(Ordering::Relaxed);
                    let elapsed_ms = last_status_at.elapsed().as_millis().max(1) as u64;
                    let status = current_serial_status(
                        read_port.as_mut(),
                        current_rx,
                        current_tx,
                        (current_rx.saturating_sub(last_rx_bytes) * 1000) / elapsed_ms,
                        (current_tx.saturating_sub(last_tx_bytes) * 1000) / elapsed_ms,
                        shared.capture.lock().unwrap().is_some(),
                        shared.file_send_active.load(Ordering::Acquire),
                    );
                    let _ = app_handle.emit(&format!("serial-status-{}", session_id), status);
                    last_rx_bytes = current_rx;
                    last_tx_bytes = current_tx;
                    last_status_at = Instant::now();
                }
            }

            shared.stop.store(true, Ordering::Release);
            let _ = writer.join();
            if let Some(error) = read_error {
                return Err(error);
            }
            if let Some(error) = writer_error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(())
        })();

        match outcome {
            Ok(()) => {
                terminate_channel(
                    &app_handle,
                    &session_id,
                    &channel_lifecycle,
                    channel_state::TerminalCause::ApplicationClosed,
                );
            }
            Err(error) => {
                emit_session_error(&app_handle, &session_id, error.clone());
                let cause = channel_state::TerminalCause::SerialError(error);
                let reason = cause.reason();
                if channel_lifecycle.lock().unwrap().terminate(cause) {
                    connection_log::append(
                        &session_id,
                        format!("channel terminal state reason={}", reason),
                    );
                }
            }
        }
    })
}

#[derive(Clone, Debug)]
struct JumpHostConfig {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    private_key_path: Option<String>,
    passphrase: Option<String>,
    connect_timeout: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteForwardTarget {
    pub target_host: String,
    pub target_port: u16,
}

pub struct SharedTunnelSshConnection {
    pub(crate) shared_session: SharedSshSession,
    pub(crate) shared_session_slot: SharedSshSessionSlot,
    jump_session: Option<Arc<client::Handle<ClientHandler>>>,
    keepalive: Option<supervisor::KeepaliveTask>,
}

impl SharedTunnelSshConnection {
    pub fn shared_session_slot(&self) -> SharedSshSessionSlot {
        self.shared_session_slot.clone()
    }

    pub async fn disconnect(mut self) {
        if let Some(keepalive) = self.keepalive.take() {
            keepalive.stop("SSH connection disconnecting").await;
        }
        {
            let session = self.shared_session.lock().await;
            let _ = session
                .disconnect(Disconnect::ByApplication, "", "English")
                .await;
        }

        if let Some(jump_handle) = self.jump_session {
            let _ = jump_handle
                .disconnect(Disconnect::ByApplication, "", "English")
                .await;
        }

        let mut slot = self.shared_session_slot.lock().unwrap();
        slot.take();
    }
}

pub(crate) struct ClientHandler {
    app_handle: AppHandle,
    session_id: String,
    host: String,
    port: u16,
    known_hosts_path: PathBuf,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    remote_forward_target: Option<RemoteForwardTarget>,
}

#[cfg(test)]
#[allow(dead_code)]
struct TestClientHandler {
    host: String,
    port: u16,
    known_hosts_path: PathBuf,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        connection_log::append(
            &self.session_id,
            format!(
                "SSH algorithms negotiated kex={} host_key={} cipher={} client_mac={} server_mac={} client_compression={:?} server_compression={:?} strict_kex={}",
                names.kex.as_ref(),
                names.key,
                names.cipher.as_ref(),
                names.client_mac.as_ref(),
                names.server_mac.as_ref(),
                names.client_compression,
                names.server_compression,
                names.strict_kex(),
            ),
        );
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match check_known_hosts_path(
            &self.host,
            self.port,
            server_public_key,
            &self.known_hosts_path,
        ) {
            Ok(true) => {
                connection_log::append(
                    &self.session_id,
                    format!(
                        "server host key matched known_hosts endpoint={}:{}",
                        self.host, self.port
                    ),
                );
                Ok(true)
            }
            Ok(false) => {
                let (tx, rx) = oneshot::channel::<bool>();
                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    *pending = Some(tx);
                }

                // In 0.48, fingerprint takes hash_alg. And algo name needs Named trait or use algorithm() from ssh_key
                let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
                let algo = server_public_key.algorithm().to_string();
                connection_log::append(
                    &self.session_id,
                    format!(
                        "unknown server host key endpoint={}:{} algorithm={} fingerprint={}",
                        self.host, self.port, algo, fingerprint
                    ),
                );

                let _ = self.app_handle.emit(
                    "ssh-hostkey-request",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "fingerprint": fingerprint,
                        "algorithm": algo,
                    }),
                );

                let decision = tokio::time::timeout(Duration::from_secs(30), rx).await;
                let accepted = match decision {
                    Ok(Ok(v)) => v,
                    _ => false,
                };

                // No learning for now

                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    pending.take();
                }

                if accepted {
                    connection_log::append(
                        &self.session_id,
                        "unknown server host key accepted by user",
                    );
                    // Manual known_hosts appending
                    if let Err(e) = append_known_host(
                        &self.host,
                        self.port,
                        server_public_key,
                        &self.known_hosts_path,
                    ) {
                        let _ = self.app_handle.emit(
                            &format!("ssh-error-{}", self.session_id),
                            format!("Failed to save host key: {}", e),
                        );
                        // Even if save fails, we connected because user accepted.
                        // But maybe we should warn? For now let's proceed but log.
                    }
                    Ok(true)
                } else {
                    connection_log::append(
                        &self.session_id,
                        "unknown server host key rejected or prompt timed out",
                    );
                    let _ = self.app_handle.emit(
                        &format!("ssh-error-{}", self.session_id),
                        "Host key not trusted. Connection cancelled.".to_string(),
                    );
                    Ok(false)
                }
            }
            Err(e) => {
                // Key mismatch (e.g. JumpServer proxy presents different key).
                // Prompt user with warning instead of rejecting outright.
                let (tx, rx) = oneshot::channel::<bool>();
                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    *pending = Some(tx);
                }

                let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
                let algo = server_public_key.algorithm().to_string();
                connection_log::append(
                    &self.session_id,
                    format!("server host key mismatch endpoint={}:{} algorithm={} fingerprint={} error={}", self.host, self.port, algo, fingerprint, e),
                );

                let _ = self.app_handle.emit(
                    "ssh-hostkey-request",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "fingerprint": fingerprint,
                        "algorithm": algo,
                        "warning": format!("主机密钥与已保存的不匹配！可能是代理/堡垒机或中间人攻击。\n原始错误: {}", e),
                    }),
                );

                let decision = tokio::time::timeout(Duration::from_secs(30), rx).await;
                let accepted = match decision {
                    Ok(Ok(v)) => v,
                    _ => false,
                };

                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    pending.take();
                }

                if accepted {
                    connection_log::append(
                        &self.session_id,
                        "mismatched server host key accepted by user",
                    );
                    let _ = append_known_host(
                        &self.host,
                        self.port,
                        server_public_key,
                        &self.known_hosts_path,
                    );
                    Ok(true)
                } else {
                    connection_log::append(
                        &self.session_id,
                        "mismatched server host key rejected or prompt timed out",
                    );
                    let _ = self.app_handle.emit(
                        &format!("ssh-error-{}", self.session_id),
                        "Host key not trusted. Connection cancelled.".to_string(),
                    );
                    Ok(false)
                }
            }
        }
    }

    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        match reason {
            client::DisconnectReason::ReceivedDisconnect(info) => {
                connection_log::append(
                    &self.session_id,
                    format!(
                        "ssh transport received disconnect endpoint={}:{} reason={:?} message={} language={}",
                        self.host,
                        self.port,
                        info.reason_code,
                        info.message,
                        info.lang_tag
                    ),
                );
                Ok(())
            }
            client::DisconnectReason::Error(error) => {
                connection_log::append(
                    &self.session_id,
                    format!(
                        "ssh transport ended with error endpoint={}:{} error={:?} display={}",
                        self.host, self.port, error, error
                    ),
                );
                Err(error)
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(target) = self.remote_forward_target.clone() else {
            return Ok(());
        };

        reply.accept().await;

        tokio::spawn(async move {
            match tokio::net::TcpStream::connect((target.target_host.as_str(), target.target_port))
                .await
            {
                Ok(mut local_stream) => {
                    let mut remote_stream = channel.into_stream();
                    let _ =
                        tokio::io::copy_bidirectional(&mut remote_stream, &mut local_stream).await;
                    let _ = remote_stream.shutdown().await;
                    let _ = local_stream.shutdown().await;
                }
                Err(_) => {
                    let mut remote_stream = channel.into_stream();
                    let _ = remote_stream.shutdown().await;
                }
            }
        });

        Ok(())
    }
}

#[cfg(test)]
impl client::Handler for TestClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match check_known_hosts_path(
            &self.host,
            self.port,
            server_public_key,
            &self.known_hosts_path,
        ) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(_) => Ok(false),
        }
    }
}

fn append_known_host(host: &str, port: u16, key: &PublicKey, path: &PathBuf) -> Result<(), String> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;

    let host_spec = if port == 22 {
        host.to_string()
    } else {
        format!("[{}]:{}", host, port)
    };

    // ssh_key::to_openssh() returns "algo base64"
    let key_string = key.to_openssh().map_err(|e| e.to_string())?;

    writeln!(file, "{} {}", host_spec, key_string).map_err(|e| e.to_string())?;
    Ok(())
}

fn app_known_hosts_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let app_dir = home.join(".duskterm");
    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    let known_hosts = app_dir.join("known_hosts");
    if !known_hosts.exists() {
        std::fs::write(&known_hosts, "").map_err(|e| e.to_string())?;
    }
    let _ = ensure_private_key_permissions(&known_hosts);
    Ok(known_hosts)
}

fn build_client_config(
    keep_alive_interval: Option<u64>,
    profile: NegotiationProfile,
) -> Arc<client::Config> {
    Arc::new(ssh_algorithms::build_client_config(
        keep_alive_interval,
        profile,
    ))
}

pub async fn open_sftp_subsystem_for_session(
    shared_session_slot: &SharedSshSessionSlot,
) -> Result<russh_sftp::client::SftpSession, String> {
    let shared_session = shared_session_slot
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "SSH session not ready for SFTP reuse".to_string())?;

    let channel = {
        let session = shared_session.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|e| format!("Failed to open shared SSH channel: {}", e))?
    };

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to request shared sftp subsystem: {}", e))?;

    russh_sftp::client::SftpSession::new_with_config(
        channel.into_stream(),
        crate::sftp::sftp_client_config(),
    )
    .await
    .map_err(|e| format!("Failed to init shared SFTP session: {}", e))
}

fn sanitize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn extract_jump_host(config: &SshConfig) -> Option<JumpHostConfig> {
    let host = sanitize_optional(config.jump_host.clone())?;
    let username = sanitize_optional(config.jump_username.clone())?;
    let auth_type =
        sanitize_optional(config.jump_auth_type.clone()).unwrap_or_else(|| "password".to_string());
    Some(JumpHostConfig {
        host,
        port: config.jump_port.unwrap_or(22),
        username,
        password: if auth_type == "key" {
            None
        } else {
            sanitize_optional(config.jump_password.clone())
        },
        private_key_path: if auth_type == "key" {
            sanitize_optional(config.jump_private_key_path.clone())
        } else {
            None
        },
        passphrase: if auth_type == "key" {
            sanitize_optional(config.jump_passphrase.clone())
        } else {
            None
        },
        connect_timeout: config.connect_timeout,
    })
}

async fn connect_handle<H, A>(
    config: Arc<client::Config>,
    addrs: A,
    handler: H,
    timeout_secs: Option<u64>,
) -> Result<client::Handle<H>, ConnectAttemptError>
where
    H: client::Handler<Error = russh::Error> + Send + 'static,
    A: tokio::net::ToSocketAddrs,
{
    let connect_fut = client::connect(config, addrs, handler);
    if let Some(timeout) = timeout_secs {
        match tokio::time::timeout(Duration::from_secs(timeout), connect_fut).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(ConnectAttemptError::Timeout),
        }
    } else {
        connect_fut.await.map_err(ConnectAttemptError::from)
    }
}

async fn connect_stream_handle<H, R>(
    config: Arc<client::Config>,
    stream: R,
    handler: H,
    timeout_secs: Option<u64>,
) -> Result<client::Handle<H>, ConnectAttemptError>
where
    H: client::Handler<Error = russh::Error> + Send + 'static,
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let connect_fut = client::connect_stream(config, stream, handler);
    if let Some(timeout) = timeout_secs {
        match tokio::time::timeout(Duration::from_secs(timeout), connect_fut).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(ConnectAttemptError::Timeout),
        }
    } else {
        connect_fut.await.map_err(ConnectAttemptError::from)
    }
}

async fn connect_with_profile_retry<H, F, Fut>(
    app_handle: &AppHandle,
    host: &str,
    port: u16,
    keep_alive_interval: Option<u64>,
    mut attempt: F,
) -> Result<client::Handle<H>, String>
where
    H: client::Handler<Error = russh::Error> + Send + 'static,
    F: FnMut(Arc<client::Config>, NegotiationProfile) -> Fut,
    Fut: Future<Output = Result<client::Handle<H>, ConnectAttemptError>>,
{
    let mut profile = app_handle
        .state::<SshAppState>()
        .preferred_profile_for_endpoint(host, port);

    loop {
        let client_config = build_client_config(keep_alive_interval, profile);
        match attempt(client_config, profile).await {
            Ok(handle) => {
                app_handle
                    .state::<SshAppState>()
                    .remember_successful_profile(host, port, profile);
                return Ok(handle);
            }
            Err(error) if ssh_algorithms::should_retry_with_legacy(profile, &error) => {
                profile = NegotiationProfile::LegacyRsaSha1;
            }
            Err(error) => return Err(format!("Connection failed: {}", error)),
        }
    }
}

pub async fn connect_shared_ssh_runtime(
    app_handle: AppHandle,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    mut config: SshConfig,
    remote_forward_target: Option<RemoteForwardTarget>,
) -> Result<SharedTunnelSshConnection, String> {
    let keepalive_interval =
        ssh_algorithms::effective_keepalive_interval(config.keep_alive_interval);
    connection_log::append(
        &session_id,
        format!(
            "initializing known_hosts for endpoint={}:{}",
            config.host, config.port
        ),
    );
    let known_hosts_path = app_known_hosts_path()
        .map_err(|error| format!("Failed to initialize known_hosts: {}", error))?;

    let mut jump_session = if let Some(jump) = extract_jump_host(&config) {
        let jump_known_hosts_path = app_known_hosts_path()
            .map_err(|error| format!("Failed to initialize known_hosts: {}", error))?;
        let jump_session_id = format!("{}::jump", session_id);
        let jump_host = jump.host.clone();
        let jump_port = jump.port;
        connection_log::append(
            &session_id,
            format!(
                "connecting configured jump host endpoint={}:{} user={}",
                jump_host, jump_port, jump.username
            ),
        );

        let mut jump_handle = connect_with_profile_retry(
            &app_handle,
            &jump_host,
            jump_port,
            config.keep_alive_interval,
            |client_config, _profile| {
                let connect_host = jump_host.clone();
                let jump_handler = ClientHandler {
                    app_handle: app_handle.clone(),
                    session_id: jump_session_id.clone(),
                    host: jump_host.clone(),
                    port: jump_port,
                    known_hosts_path: jump_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
                    remote_forward_target: None,
                };
                async move {
                    connect_handle(
                        client_config,
                        (connect_host.as_str(), jump_port),
                        jump_handler,
                        jump.connect_timeout,
                    )
                    .await
                }
            },
        )
        .await
        .map_err(|error| format!("Jump host connection failed: {}", error))?;
        connection_log::append(
            &session_id,
            "jump host SSH transport connected; authenticating",
        );

        auth::authenticate_session(
            &session_id,
            &mut jump_handle,
            jump.username,
            jump.private_key_path,
            jump.password,
            jump.passphrase,
        )
        .await
        .map_err(|error| format!("Jump host authentication failed: {}", error))?;
        connection_log::append(&session_id, "jump host authentication accepted");

        Some(jump_handle)
    } else {
        None
    };

    let mut session = if let Some(jump_handle) = jump_session.as_mut() {
        let target_host = config.host.clone();
        let target_port = config.port;
        let session_known_hosts_path = known_hosts_path.clone();
        let remote_forward_target = remote_forward_target.clone();
        let mut profile = app_handle
            .state::<SshAppState>()
            .preferred_profile_for_endpoint(&target_host, target_port);

        loop {
            connection_log::append(
                &session_id,
                format!(
                    "opening direct-tcpip through jump host target={}:{}",
                    target_host, target_port
                ),
            );
            let handler = ClientHandler {
                app_handle: app_handle.clone(),
                session_id: session_id.clone(),
                host: target_host.clone(),
                port: target_port,
                known_hosts_path: session_known_hosts_path.clone(),
                pending_hostkey: pending_hostkey.clone(),
                remote_forward_target: remote_forward_target.clone(),
            };
            let stream = jump_handle
                .channel_open_direct_tcpip(target_host.clone(), target_port as u32, "127.0.0.1", 0)
                .await
                .map_err(|error| format!("Jump tunnel open failed: {}", error))?
                .into_stream();
            connection_log::append(
                &session_id,
                "direct-tcpip channel opened; starting target SSH transport",
            );

            let client_config = build_client_config(config.keep_alive_interval, profile);
            match connect_stream_handle(client_config, stream, handler, config.connect_timeout)
                .await
            {
                Ok(session) => {
                    app_handle
                        .state::<SshAppState>()
                        .remember_successful_profile(&target_host, target_port, profile);
                    break session;
                }
                Err(error) if ssh_algorithms::should_retry_with_legacy(profile, &error) => {
                    profile = NegotiationProfile::LegacyRsaSha1;
                }
                Err(error) => {
                    return Err(format!(
                        "Target connection through jump host failed: {}",
                        error
                    ));
                }
            }
        }
    } else {
        let target_host = config.host.clone();
        let target_port = config.port;
        let session_known_hosts_path = known_hosts_path.clone();
        let remote_forward_target = remote_forward_target.clone();

        connection_log::append(
            &session_id,
            format!(
                "connecting direct SSH transport endpoint={}:{}",
                target_host, target_port
            ),
        );
        let connected = connect_with_profile_retry(
            &app_handle,
            &target_host,
            target_port,
            config.keep_alive_interval,
            |client_config, _profile| {
                let connect_host = target_host.clone();
                let handler = ClientHandler {
                    app_handle: app_handle.clone(),
                    session_id: session_id.clone(),
                    host: target_host.clone(),
                    port: target_port,
                    known_hosts_path: session_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
                    remote_forward_target: remote_forward_target.clone(),
                };
                async move {
                    connect_handle(
                        client_config,
                        (connect_host.as_str(), target_port),
                        handler,
                        config.connect_timeout,
                    )
                    .await
                }
            },
        )
        .await?;
        connection_log::append(&session_id, "direct SSH transport connected");
        connected
    };

    connection_log::append(
        &session_id,
        format!("authenticating SSH user={}", config.username),
    );
    auth::authenticate_session(
        &session_id,
        &mut session,
        config.username.clone(),
        config.private_key_path.clone(),
        config.password.clone(),
        config.passphrase.take(),
    )
    .await?;
    connection_log::append(&session_id, "SSH authentication accepted");

    let shared_session: SharedSshSession = Arc::new(AsyncMutex::new(session));
    let shared_session_slot: SharedSshSessionSlot =
        Arc::new(Mutex::new(Some(shared_session.clone())));
    let jump_session = jump_session.map(Arc::new);
    let keepalive = supervisor::spawn_locked_keepalive_task(
        session_id,
        keepalive_interval,
        shared_session.clone(),
        jump_session.clone(),
    );

    Ok(SharedTunnelSshConnection {
        shared_session,
        shared_session_slot,
        jump_session,
        keepalive,
    })
}

async fn run_ssh_session_task(
    app_handle: AppHandle,
    sftp_state: SftpAppState,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
    shared_session_slot: SharedSshSessionSlot,
    channel_lifecycle: SharedChannelLifecycle,
    mut rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: SessionCloseReceiver,
    mut transfer_rx: UnboundedReceiver<TerminalTransferControl>,
    transfer_owned: Arc<AtomicBool>,
) {
    let started_at = Instant::now();
    let term_type = config.term_type.clone();
    let login_script = config.login_script.clone();
    connection_log::append(
        &session_id,
        format!(
            "ssh connect start host={}:{} user={} term={} keepalive_requested={:?} keepalive_effective_secs={} jump_host={}",
            config.host,
            config.port,
            config.username,
            term_type.as_deref().unwrap_or("xterm-256color"),
            config.keep_alive_interval,
            ssh_algorithms::effective_keepalive_interval(config.keep_alive_interval),
            config.jump_host.as_deref().map(|_| "configured").unwrap_or("none")
        ),
    );

    let connection = match connect_shared_ssh_runtime(
        app_handle.clone(),
        pending_hostkey,
        session_id.clone(),
        config,
        None,
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            connection_log::append(
                &session_id,
                format!(
                    "ssh connect failed elapsed_ms={} error={}",
                    started_at.elapsed().as_millis(),
                    error
                ),
            );
            fail_session_connect(
                &app_handle,
                &shared_session_slot,
                &sftp_state,
                &session_id,
                error,
            );
            terminate_channel(
                &app_handle,
                &session_id,
                &channel_lifecycle,
                channel_state::TerminalCause::TransportError("connection failed".to_string()),
            );
            return;
        }
    };
    connection_log::append(
        &session_id,
        format!(
            "ssh transport authenticated elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );

    {
        let mut slot = shared_session_slot.lock().unwrap();
        *slot = Some(connection.shared_session.clone());
    }

    let mut channel = {
        connection_log::append(&session_id, "opening session channel");
        let session = connection.shared_session.lock().await;
        match session.channel_open_session().await {
            Ok(channel) => channel,
            Err(error) => {
                connection_log::append(
                    &session_id,
                    format!("session channel open failed error={}", error),
                );
                fail_session_connect(
                    &app_handle,
                    &shared_session_slot,
                    &sftp_state,
                    &session_id,
                    format!("Channel open failed: {}", error),
                );
                terminate_channel(
                    &app_handle,
                    &session_id,
                    &channel_lifecycle,
                    channel_state::TerminalCause::TransportError(error.to_string()),
                );
                return;
            }
        }
    };
    connection_log::append(
        &session_id,
        format!("session channel opened channel_id={:?}", channel.id()),
    );

    let term = term_type.as_deref().unwrap_or("xterm-256color");
    let terminal_modes = default_terminal_modes();
    connection_log::append(
        &session_id,
        format!(
            "requesting pty term={} cols=80 rows=24 modes={}",
            term,
            terminal_modes.len()
        ),
    );
    if let Err(error) = channel
        .request_pty(true, term, 80, 24, 0, 0, &terminal_modes)
        .await
    {
        connection_log::append(&session_id, format!("pty request failed error={}", error));
        fail_session_connect(
            &app_handle,
            &shared_session_slot,
            &sftp_state,
            &session_id,
            format!("PTY request failed: {}", error),
        );
        terminate_channel(
            &app_handle,
            &session_id,
            &channel_lifecycle,
            channel_state::TerminalCause::TransportError(error.to_string()),
        );
        return;
    }
    connection_log::append(&session_id, "pty request accepted");

    connection_log::append(&session_id, "requesting interactive shell");
    if let Err(error) = channel.request_shell(true).await {
        connection_log::append(&session_id, format!("shell request failed error={}", error));
        fail_session_connect(
            &app_handle,
            &shared_session_slot,
            &sftp_state,
            &session_id,
            format!("Shell request failed: {}", error),
        );
        terminate_channel(
            &app_handle,
            &session_id,
            &channel_lifecycle,
            channel_state::TerminalCause::TransportError(error.to_string()),
        );
        return;
    }
    connection_log::append(&session_id, "interactive shell accepted");

    if let Some(script) = &login_script {
        if !script.is_empty() {
            connection_log::append(
                &session_id,
                format!(
                    "sending login script bytes={} content=redacted",
                    script.len()
                ),
            );
            let _ = channel.data(script.as_bytes()).await;
            if !script.ends_with('\n') {
                let _ = channel.data("\n".as_bytes()).await;
            }
        }
    }

    let _ = app_handle.emit(&format!("ssh-connected-{}", session_id), ());
    connection_log::append(
        &session_id,
        format!(
            "ssh connected event emitted elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );
    let mut transfer_runtime =
        TerminalTransferRuntime::new(session_id.clone(), None, transfer_owned.clone());
    let mut transfer_tick = tokio::time::interval(Duration::from_millis(50));
    transfer_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent_packets = 0u64;
    let mut sent_bytes = 0u64;
    let mut received_packets = 0u64;
    let mut received_bytes = 0u64;

    loop {
        tokio::select! {
            Some(data) = rx.recv() => {
                if transfer_owned.load(Ordering::Acquire) {
                    continue;
                }
                if data.len() > 64 * 1024 {
                    let _ = app_handle.emit(
                        &format!("ssh-error-{}", session_id),
                        "Input too large; dropped to protect server".to_string(),
                    );
                    continue;
                }

                let write_len = data.len();
                let write_kind = connection_log::describe_payload(&data);
                sent_packets = sent_packets.saturating_add(1);
                sent_bytes = sent_bytes.saturating_add(write_len as u64);
                if let Err(error) = channel.data_bytes(data).await {
                    connection_log::append(&session_id, format!("channel write failed bytes={} kind={} error={}", write_len, write_kind, error));
                    let _ = app_handle.emit(
                        &format!("ssh-error-{}", session_id),
                        format!("Write failed: {}", error),
                    );
                    terminate_channel(
                        &app_handle,
                        &session_id,
                        &channel_lifecycle,
                        channel_state::TerminalCause::TransportError(error.to_string()),
                    );
                    break;
                }
            }
            Some((cols, rows)) = resize_rx.recv() => {
                connection_log::append(&session_id, format!("channel window change cols={} rows={}", cols, rows));
                if let Err(error) = channel.window_change(cols, rows, 0, 0).await {
                    connection_log::append(&session_id, format!("channel window change failed error={}", error));
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        received_packets = received_packets.saturating_add(1);
                        received_bytes = received_bytes.saturating_add(data.len() as u64);
                        match transfer_runtime.handle_remote(&app_handle, &channel, data.as_ref()).await {
                            Ok(chunks) => {
                                for terminal_data in chunks {
                                    let _ = app_handle.emit(&format!("ssh-data-{}", session_id), terminal_data);
                                }
                            }
                            Err(error) => {
                                connection_log::append(&session_id, format!("ZMODEM runtime error: {}", error));
                            }
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, ext }) => {
                        received_packets = received_packets.saturating_add(1);
                        received_bytes = received_bytes.saturating_add(data.len() as u64);
                        connection_log::append(&session_id, format!("channel extended data received type={} bytes={}", ext, data.len()));
                        let _ = app_handle.emit(&format!("ssh-data-{}", session_id), data.to_vec());
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        connection_log::append(&session_id, format!("remote exit status={}", exit_status));
                        terminate_channel(
                            &app_handle,
                            &session_id,
                            &channel_lifecycle,
                            channel_state::TerminalCause::ExitStatus(exit_status),
                        );
                        break;
                    }
                    Some(ChannelMsg::ExitSignal {
                        signal_name,
                        core_dumped,
                        error_message,
                        ..
                    }) => {
                        connection_log::append(
                            &session_id,
                            format!("remote exit signal={:?} core_dumped={} message={}", signal_name, core_dumped, error_message),
                        );
                        terminate_channel(
                            &app_handle,
                            &session_id,
                            &channel_lifecycle,
                            channel_state::TerminalCause::ExitSignal(format!("{:?}", signal_name)),
                        );
                        break;
                    }
                    Some(ChannelMsg::Eof) => {
                        connection_log::append(&session_id, "remote sent channel EOF");
                        terminate_channel(&app_handle, &session_id, &channel_lifecycle, channel_state::TerminalCause::RemoteEof);
                        break;
                    }
                    Some(ChannelMsg::Close) => {
                        connection_log::append(&session_id, "remote sent channel close");
                        terminate_channel(&app_handle, &session_id, &channel_lifecycle, channel_state::TerminalCause::RemoteClose);
                        break;
                    }
                    Some(other) => {
                        connection_log::append(&session_id, format!("channel message {:?}", other));
                    }
                    None => {
                        connection_log::append(&session_id, "channel message stream ended without close details");
                        terminate_channel(&app_handle, &session_id, &channel_lifecycle, channel_state::TerminalCause::StreamEnded);
                        break;
                    }
                }
            }
            Some(control) = transfer_rx.recv() => {
                transfer_runtime.handle_control(&app_handle, &channel, control).await;
            }
            _ = transfer_tick.tick() => {
                match transfer_runtime.on_tick(&app_handle, &channel).await {
                    Ok(chunks) => {
                        for terminal_data in chunks {
                            let _ = app_handle.emit(&format!("ssh-data-{}", session_id), terminal_data);
                        }
                    }
                    Err(error) => {
                        connection_log::append(&session_id, format!("ZMODEM timer error: {}", error));
                    }
                }
            }
            Some(_) = close_rx.recv() => {
                connection_log::append(&session_id, "close requested by application");
                let _ = channel.close().await;
                {
                    let session = connection.shared_session.lock().await;
                    let _ = session.disconnect(Disconnect::ByApplication, "", "English").await;
                }
                terminate_channel(&app_handle, &session_id, &channel_lifecycle, channel_state::TerminalCause::ApplicationClosed);
                break;
            }
            else => {
                terminate_channel(&app_handle, &session_id, &channel_lifecycle, channel_state::TerminalCause::StreamEnded);
                break;
            },
        }
    }

    let terminal_tail = transfer_runtime.flush_terminal_data();
    transfer_runtime.shutdown(&app_handle).await;
    if !terminal_tail.is_empty() {
        let _ = app_handle.emit(&format!("ssh-data-{}", session_id), terminal_tail);
    }

    connection_log::append(
        &session_id,
        format!(
            "ssh session cleanup elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );
    connection_log::append(
        &session_id,
        format!(
            "traffic summary sent_packets={} sent_bytes={} received_packets={} received_bytes={}",
            sent_packets, sent_bytes, received_packets, received_bytes
        ),
    );
    connection.disconnect().await;
    cleanup_session_state(&shared_session_slot, &sftp_state, &session_id);
    connection_log::append(&session_id, "ssh session task ended");
}

#[allow(dead_code)]
pub async fn connect_ssh_legacy(
    app_handle: AppHandle,
    sftp_state: SftpAppState,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
) -> Result<TerminalRuntimeHandle, String> {
    let session_id_clone = session_id.clone();
    let protocol = normalized_protocol(config.protocol.as_deref());
    let supports_terminal_transfer = protocol == "ssh";
    let (input_encoding, input_line_ending) = if protocol == "serial" {
        serial_runtime_input_options(&config)?
    } else {
        (None, None)
    };

    // Channels for communication with the SSH task
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let (serial_command_tx, serial_command_rx) =
        sync_channel::<SerialCommand>(SERIAL_COMMAND_QUEUE_CAPACITY);
    let serial_file_send_active = Arc::new(AtomicBool::new(false));
    let (transfer_control_tx, transfer_rx) = unbounded_channel::<TerminalTransferControl>();
    let transfer_owned = Arc::new(AtomicBool::new(false));
    let shared_session_slot: SharedSshSessionSlot = Arc::new(Mutex::new(None));
    let channel_lifecycle = Arc::new(Mutex::new(channel_state::ChannelLifecycle::default()));
    let runtime_handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: shared_session_slot.clone(),
        transfer_control_tx: supports_terminal_transfer.then_some(transfer_control_tx),
        transfer_owned: transfer_owned.clone(),
        channel_lifecycle: channel_lifecycle.clone(),
        input_encoding,
        input_line_ending,
        serial_command_tx: (protocol == "serial").then_some(serial_command_tx),
        serial_file_send_active: (protocol == "serial").then_some(serial_file_send_active.clone()),
    };

    match protocol {
        "telnet" => {
            spawn_telnet_session(
                app_handle,
                session_id_clone,
                config,
                rx,
                resize_rx,
                close_rx,
            );
            return Ok(runtime_handle);
        }
        "serial" => {
            drop(spawn_serial_session(
                app_handle,
                session_id_clone,
                config,
                serial_command_rx,
                close_rx,
                channel_lifecycle,
                serial_file_send_active,
            ));
            return Ok(runtime_handle);
        }
        _ => {}
    }

    // Spawn a thread that owns a Tokio runtime for async SSH
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build();

        let runtime = match runtime {
            Ok(rt) => rt,
            Err(e) => {
                fail_session_connect(
                    &app_handle,
                    &shared_session_slot,
                    &sftp_state,
                    &session_id_clone,
                    format!("Failed to start Tokio runtime: {}", e),
                );
                return;
            }
        };

        runtime.block_on(run_ssh_session_task(
            app_handle,
            sftp_state,
            pending_hostkey,
            session_id_clone,
            config,
            shared_session_slot,
            channel_lifecycle,
            rx,
            resize_rx,
            close_rx,
            transfer_rx,
            transfer_owned,
        ));
    });

    Ok(runtime_handle)
}

pub async fn connect_ssh_runtime(
    app_handle: AppHandle,
    sftp_state: SftpAppState,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
) -> Result<crate::session::state::ManagedSshRuntime, String> {
    let protocol = normalized_protocol(config.protocol.as_deref());
    let supports_terminal_transfer = protocol == "ssh";
    let (input_encoding, input_line_ending) = if protocol == "serial" {
        serial_runtime_input_options(&config)?
    } else {
        (None, None)
    };
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let (serial_command_tx, serial_command_rx) =
        sync_channel::<SerialCommand>(SERIAL_COMMAND_QUEUE_CAPACITY);
    let serial_file_send_active = Arc::new(AtomicBool::new(false));
    let (transfer_control_tx, transfer_rx) = unbounded_channel::<TerminalTransferControl>();
    let transfer_owned = Arc::new(AtomicBool::new(false));
    let shared_session_slot: SharedSshSessionSlot = Arc::new(Mutex::new(None));
    let channel_lifecycle = Arc::new(Mutex::new(channel_state::ChannelLifecycle::default()));
    let handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: shared_session_slot.clone(),
        transfer_control_tx: supports_terminal_transfer.then_some(transfer_control_tx),
        transfer_owned: transfer_owned.clone(),
        channel_lifecycle: channel_lifecycle.clone(),
        input_encoding,
        input_line_ending,
        serial_command_tx: (protocol == "serial").then_some(serial_command_tx),
        serial_file_send_active: (protocol == "serial").then_some(serial_file_send_active.clone()),
    };

    let task = match protocol {
        "telnet" => {
            spawn_telnet_session(app_handle, session_id, config, rx, resize_rx, close_rx);
            None
        }
        "serial" => Some(spawn_serial_session(
            app_handle,
            session_id,
            config,
            serial_command_rx,
            close_rx,
            channel_lifecycle,
            serial_file_send_active,
        )),
        _ => Some(tokio::spawn(run_ssh_session_task(
            app_handle,
            sftp_state,
            pending_hostkey,
            session_id,
            config,
            shared_session_slot,
            channel_lifecycle,
            rx,
            resize_rx,
            close_rx,
            transfer_rx,
            transfer_owned,
        ))),
    };

    Ok(crate::session::state::ManagedSshRuntime { handle, task })
}

async fn run_shared_shell_channel_task(
    app_handle: AppHandle,
    workspace_session_id: String,
    channel_id: String,
    shared_session: SharedSshSession,
    term_type: Option<String>,
    login_script: Option<String>,
    mut rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: SessionCloseReceiver,
    mut transfer_rx: UnboundedReceiver<TerminalTransferControl>,
    transfer_owned: Arc<AtomicBool>,
    channel_lifecycle: SharedChannelLifecycle,
    ready_tx: oneshot::Sender<Result<(), String>>,
) {
    let started_at = Instant::now();
    connection_log::append(&channel_id, "shared shell channel task start");
    let mut ready_tx = Some(ready_tx);
    let mut channel = {
        let session = shared_session.lock().await;
        match session.channel_open_session().await {
            Ok(channel) => channel,
            Err(error) => {
                let message = format!("Channel open failed: {}", error);
                connection_log::append(&channel_id, &message);
                if let Some(tx) = ready_tx.take() {
                    let _ = tx.send(Err(message.clone()));
                }
                let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
                return;
            }
        }
    };
    connection_log::append(
        &channel_id,
        format!(
            "shared session channel opened channel_id={:?}",
            channel.id()
        ),
    );
    let terminal_modes = default_terminal_modes();
    connection_log::append(
        &channel_id,
        format!(
            "requesting shared channel pty term={}",
            term_type.as_deref().unwrap_or("xterm-256color")
        ),
    );
    if let Err(error) = channel
        .request_pty(
            true,
            term_type.as_deref().unwrap_or("xterm-256color"),
            80,
            24,
            0,
            0,
            &terminal_modes,
        )
        .await
    {
        let message = format!("PTY request failed: {}", error);
        connection_log::append(&channel_id, &message);
        if let Some(tx) = ready_tx.take() {
            let _ = tx.send(Err(message.clone()));
        }
        let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
        return;
    }
    connection_log::append(&channel_id, "shared channel pty accepted");
    connection_log::append(&channel_id, "requesting shared interactive shell");
    if let Err(error) = channel.request_shell(true).await {
        let message = format!("Shell request failed: {}", error);
        connection_log::append(&channel_id, &message);
        if let Some(tx) = ready_tx.take() {
            let _ = tx.send(Err(message.clone()));
        }
        let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
        return;
    }
    connection_log::append(&channel_id, "shared interactive shell accepted");
    if let Some(script) = login_script.filter(|script| !script.is_empty()) {
        connection_log::append(
            &channel_id,
            format!(
                "sending shared channel login script bytes={} content=redacted",
                script.len()
            ),
        );
        let _ = channel.data(script.as_bytes()).await;
        if !script.ends_with('\n') {
            let _ = channel.data("\n".as_bytes()).await;
        }
    }
    if let Some(tx) = ready_tx.take() {
        let _ = tx.send(Ok(()));
    }
    let _ = app_handle.emit(&format!("ssh-connected-{}", channel_id), ());
    connection_log::append(
        &channel_id,
        format!(
            "shared shell connected elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );
    let mut transfer_runtime = TerminalTransferRuntime::new(
        workspace_session_id,
        Some(channel_id.clone()),
        transfer_owned.clone(),
    );
    let mut transfer_tick = tokio::time::interval(Duration::from_millis(50));
    transfer_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent_packets = 0u64;
    let mut sent_bytes = 0u64;
    let mut received_packets = 0u64;
    let mut received_bytes = 0u64;
    loop {
        tokio::select! {
            Some(data) = rx.recv() => {
                if transfer_owned.load(Ordering::Acquire) {
                    continue;
                }
                if data.len() > 64 * 1024 { continue; }
                let write_len = data.len();
                let write_kind = connection_log::describe_payload(&data);
                sent_packets = sent_packets.saturating_add(1);
                sent_bytes = sent_bytes.saturating_add(write_len as u64);
                if let Err(error) = channel.data_bytes(data).await {
                    connection_log::append(&channel_id, format!("shared channel write failed bytes={} kind={} error={}", write_len, write_kind, error));
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::TransportError(error.to_string()));
                    break;
                }
            }
            Some((cols, rows)) = resize_rx.recv() => {
                connection_log::append(&channel_id, format!("shared channel window change cols={} rows={}", cols, rows));
                let _ = channel.window_change(cols, rows, 0, 0).await;
            }
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    received_packets = received_packets.saturating_add(1);
                    received_bytes = received_bytes.saturating_add(data.len() as u64);
                    match transfer_runtime.handle_remote(&app_handle, &channel, data.as_ref()).await {
                        Ok(chunks) => {
                            for terminal_data in chunks {
                                let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), terminal_data);
                            }
                        }
                        Err(error) => {
                            connection_log::append(&channel_id, format!("ZMODEM runtime error: {}", error));
                        }
                    }
                }
                Some(ChannelMsg::ExtendedData { data, ext }) => {
                    received_packets = received_packets.saturating_add(1);
                    received_bytes = received_bytes.saturating_add(data.len() as u64);
                    connection_log::append(&channel_id, format!("shared channel extended data type={} bytes={}", ext, data.len()));
                    let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), data.to_vec());
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    connection_log::append(&channel_id, format!("shared channel remote exit status={}", exit_status));
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::ExitStatus(exit_status));
                    break;
                }
                Some(ChannelMsg::ExitSignal { signal_name, core_dumped, error_message, .. }) => {
                    connection_log::append(&channel_id, format!("shared channel remote exit signal={:?} core_dumped={} message={}", signal_name, core_dumped, error_message));
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::ExitSignal(format!("{:?}", signal_name)));
                    break;
                }
                Some(ChannelMsg::Eof) => {
                    connection_log::append(&channel_id, "shared channel remote EOF");
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::RemoteEof);
                    break;
                }
                Some(ChannelMsg::Close) => {
                    connection_log::append(&channel_id, "shared channel remote close");
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::RemoteClose);
                    break;
                }
                Some(other) => connection_log::append(&channel_id, format!("shared channel message {:?}", other)),
                None => {
                    connection_log::append(&channel_id, "shared channel message stream ended without close details");
                    terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::StreamEnded);
                    break;
                }
            },
            Some(control) = transfer_rx.recv() => {
                transfer_runtime.handle_control(&app_handle, &channel, control).await;
            }
            _ = transfer_tick.tick() => {
                match transfer_runtime.on_tick(&app_handle, &channel).await {
                    Ok(chunks) => {
                        for terminal_data in chunks {
                            let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), terminal_data);
                        }
                    }
                    Err(error) => {
                        connection_log::append(&channel_id, format!("ZMODEM timer error: {}", error));
                    }
                }
            }
            Some(_) = close_rx.recv() => {
                connection_log::append(&channel_id, "shared channel close requested by application");
                let _ = channel.close().await;
                terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::ApplicationClosed);
                break;
            }
            else => {
                terminate_channel(&app_handle, &channel_id, &channel_lifecycle, channel_state::TerminalCause::StreamEnded);
                break;
            },
        }
    }
    let terminal_tail = transfer_runtime.flush_terminal_data();
    transfer_runtime.shutdown(&app_handle).await;
    if !terminal_tail.is_empty() {
        let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), terminal_tail);
    }
    connection_log::append(
        &channel_id,
        format!(
            "shared shell channel task ended elapsed_ms={}",
            started_at.elapsed().as_millis()
        ),
    );
    connection_log::append(
        &channel_id,
        format!(
            "shared traffic summary sent_packets={} sent_bytes={} received_packets={} received_bytes={}",
            sent_packets, sent_bytes, received_packets, received_bytes
        ),
    );
}

pub async fn open_shared_shell_channel_runtime(
    app_handle: AppHandle,
    root_handle: &TerminalRuntimeHandle,
    workspace_session_id: String,
    channel_id: String,
    term_type: Option<String>,
    login_script: Option<String>,
) -> Result<crate::session::state::ManagedSshRuntime, String> {
    let shared_session = root_handle
        .shared_session
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Root SSH transport is not ready".to_string())?;
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let (transfer_control_tx, transfer_rx) = unbounded_channel::<TerminalTransferControl>();
    let transfer_owned = Arc::new(AtomicBool::new(false));
    let channel_lifecycle = Arc::new(Mutex::new(channel_state::ChannelLifecycle::default()));
    let handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: root_handle.shared_session.clone(),
        transfer_control_tx: Some(transfer_control_tx),
        transfer_owned: transfer_owned.clone(),
        channel_lifecycle: channel_lifecycle.clone(),
        input_encoding: None,
        input_line_ending: None,
        serial_command_tx: None,
        serial_file_send_active: None,
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(run_shared_shell_channel_task(
        app_handle,
        workspace_session_id,
        channel_id,
        shared_session,
        term_type,
        login_script,
        rx,
        resize_rx,
        close_rx,
        transfer_rx,
        transfer_owned,
        channel_lifecycle,
        ready_tx,
    ));
    match ready_rx.await {
        Ok(Ok(())) => Ok(crate::session::state::ManagedSshRuntime {
            handle,
            task: Some(task),
        }),
        Ok(Err(error)) => {
            let _ = task.await;
            Err(error)
        }
        Err(_) => {
            let _ = task.await;
            Err("Shell channel task ended before becoming ready".to_string())
        }
    }
}

#[tauri::command]
pub async fn test_ssh_connection(config: SshConfig) -> Result<String, String> {
    let timeout = Duration::from_secs(config.connect_timeout.unwrap_or(10).clamp(1, 120));

    match normalized_protocol(config.protocol.as_deref()) {
        "telnet" => {
            let address = socket_address(&config.host, config.port)?;
            let stream = TcpStream::connect_timeout(&address, timeout)
                .map_err(|e| format!("Telnet 连接失败: {}", e))?;
            let _ = stream.shutdown(Shutdown::Both);
            return Ok("Telnet 端口连通性正常".to_string());
        }
        "serial" => {
            let _port = build_serial_port(&config)?;
            return Ok("串口打开成功，参数有效。".to_string());
        }
        _ => {}
    }

    // SSH: just test TCP port reachability (like telnet/nc)
    let address = socket_address(&config.host, config.port)?;
    let stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|e| format!("端口不可达 ({}:{})\n{}", config.host, config.port, e))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(format!("{}:{} 端口连通性正常", config.host, config.port))
}

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<SerialPortOption>, String> {
    let ports = available_ports().map_err(|e| format!("读取串口列表失败: {}", e))?;
    Ok(ports
        .into_iter()
        .map(|port| {
            let path = port.port_name;
            let stable_id = serial_port_stable_id(&path, &port.port_type);
            let mut vid = None;
            let mut pid = None;
            let mut serial_number = None;
            let label = match &port.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    vid = Some(info.vid);
                    pid = Some(info.pid);
                    serial_number = info.serial_number.clone();
                    let mut parts = Vec::new();
                    if let Some(manufacturer) = info.manufacturer.as_deref() {
                        if !manufacturer.trim().is_empty() {
                            parts.push(manufacturer.trim().to_string());
                        }
                    }
                    if let Some(product) = info.product.as_deref() {
                        if !product.trim().is_empty() {
                            parts.push(product.trim().to_string());
                        }
                    }
                    parts.push(format!("VID:{:04X} PID:{:04X}", info.vid, info.pid));
                    if let Some(serial) = info.serial_number.as_deref() {
                        if !serial.trim().is_empty() {
                            parts.push(format!("SN:{}", serial.trim()));
                        }
                    }
                    if parts.is_empty() {
                        path.clone()
                    } else {
                        format!("{} ({})", path, parts.join(" / "))
                    }
                }
                serialport::SerialPortType::BluetoothPort => format!("{} (Bluetooth)", path),
                serialport::SerialPortType::PciPort => format!("{} (PCI)", path),
                _ => path.clone(),
            };
            SerialPortOption {
                path,
                label,
                stable_id,
                vid,
                pid,
                serial_number,
            }
        })
        .collect())
}

#[allow(dead_code)]
pub fn write_ssh_legacy(handle: &TerminalRuntimeHandle, data: String) -> Result<(), String> {
    write_ssh_runtime(handle, data)
}

pub fn write_ssh_runtime(handle: &TerminalRuntimeHandle, data: String) -> Result<(), String> {
    if handle.transfer_owned.load(Ordering::Acquire) {
        return Err("ZMODEM 传输期间终端输入已暂停".to_string());
    }
    {
        let lifecycle = handle.channel_lifecycle.lock().unwrap();
        if !lifecycle.can_write() {
            let reason = lifecycle
                .cause()
                .map(channel_state::TerminalCause::reason)
                .unwrap_or_else(|| "unknown terminal state".to_string());
            return Err(format!("Terminal channel is closed: {}", reason));
        }
    }

    let bytes = encode_runtime_input(handle, &data)?;
    if let Some(serial_tx) = handle.serial_command_tx.as_ref() {
        if handle
            .serial_file_send_active
            .as_ref()
            .is_some_and(|active| active.load(Ordering::Acquire))
        {
            return Err("串口文件发送期间不能插入普通数据".to_string());
        }
        return serial_tx
            .try_send(SerialCommand::Write(bytes))
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => {
                    "串口发送队列已满，已拒绝继续写入以保护内存".to_string()
                }
                std::sync::mpsc::TrySendError::Disconnected(_) => "串口写入通道已关闭".to_string(),
            });
    }
    handle.tx.try_send(bytes).map_err(|error| match error {
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            "Terminal input queue is full; dropped to protect memory".to_string()
        }
        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
            "Terminal session input channel is closed".to_string()
        }
    })
}

pub(crate) async fn control_serial_runtime(
    handle: &TerminalRuntimeHandle,
    request: SerialControlRequest,
) -> Result<SerialControlResponse, String> {
    let tx = handle
        .serial_command_tx
        .as_ref()
        .ok_or_else(|| "当前会话不是串口会话".to_string())?;
    let is_file_send = matches!(&request, SerialControlRequest::SendFile(_));
    let file_send_active = handle
        .serial_file_send_active
        .as_ref()
        .ok_or_else(|| "当前会话不是串口会话".to_string())?;
    if is_file_send {
        file_send_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "已有串口文件正在发送".to_string())?;
    } else if file_send_active.load(Ordering::Acquire) {
        return Err("串口文件发送期间暂不接受其他写入或控制操作".to_string());
    }
    let (respond_to, response_rx) = oneshot::channel();
    if let Err(error) = tx.try_send(SerialCommand::Control {
        request,
        respond_to,
    }) {
        if is_file_send {
            file_send_active.store(false, Ordering::Release);
        }
        return Err(match error {
            std::sync::mpsc::TrySendError::Full(_) => "串口控制队列已满".to_string(),
            std::sync::mpsc::TrySendError::Disconnected(_) => "串口控制通道已关闭".to_string(),
        });
    }
    if is_file_send {
        return Ok(SerialControlResponse::Unit);
    }
    tokio::time::timeout(SERIAL_CONTROL_TIMEOUT, response_rx)
        .await
        .map_err(|_| "串口控制操作超时".to_string())?
        .map_err(|_| "串口控制任务未返回结果".to_string())?
}

#[allow(dead_code)]
pub fn resize_ssh_legacy(
    handle: &TerminalRuntimeHandle,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let _ = handle.window_size_tx.send((cols, rows));
    Ok(())
}

pub fn resize_ssh_runtime(
    handle: &TerminalRuntimeHandle,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let _ = handle.window_size_tx.send((cols, rows));
    Ok(())
}

#[allow(dead_code)]
pub fn disconnect_ssh_legacy(
    handle: Option<TerminalRuntimeHandle>,
    sftp_state: &SftpAppState,
    session_id: String,
) -> Result<(), String> {
    if let Some(handle) = handle {
        let _ = handle.close_tx.send(());
        cleanup_session_state(&handle.shared_session, sftp_state, &session_id);
    } else {
        crate::sftp::cleanup_session_state(sftp_state, &session_id);
    }
    Ok(())
}

pub async fn disconnect_ssh_runtime(
    runtime: Option<crate::session::state::ManagedSshRuntime>,
    sftp_state: &SftpAppState,
    session_id: String,
) -> Result<(), String> {
    if let Some(mut runtime) = runtime {
        let _ = runtime.handle.close_tx.send(());
        if let Some(task) = runtime.task.take() {
            let _ = task.await;
        }
        cleanup_session_state(&runtime.handle.shared_session, sftp_state, &session_id);
    } else {
        crate::sftp::cleanup_session_state(sftp_state, &session_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn connect_ssh(
    app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SshAppState>,
    sftp_state: tauri::State<'_, SftpAppState>,
    id: String,
    config: SshConfig,
) -> Result<String, String> {
    supervisor
        .connect(app_handle, sftp_state.inner().clone(), id, config)
        .await
}

#[tauri::command]
pub async fn confirm_hostkey(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SshAppState>,
    session_id: String,
    accept: bool,
) -> Result<(), String> {
    supervisor.confirm_hostkey(session_id, accept).await
}

#[tauri::command]
pub async fn write_ssh(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SshAppState>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    supervisor.write_terminal(session_id, data).await
}

#[tauri::command]
pub async fn resize_ssh(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SshAppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    supervisor.resize_terminal(session_id, cols, rows).await
}

async fn run_serial_unit_control(
    supervisor: &crate::session::supervisor::SessionSupervisor,
    session_id: String,
    request: SerialControlRequest,
) -> Result<(), String> {
    match supervisor.control_serial(session_id, request).await? {
        SerialControlResponse::Unit => Ok(()),
        SerialControlResponse::Status(_) => Err("串口控制返回了意外的状态结果".to_string()),
    }
}

#[tauri::command]
pub async fn serial_write_bytes(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::WriteRaw(data),
    )
    .await
}

#[tauri::command]
pub async fn serial_write_text(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    text: String,
    encoding: Option<String>,
    line_ending: Option<String>,
) -> Result<(), String> {
    let suffix = match line_ending
        .as_deref()
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => "",
        "cr" => "\r",
        "lf" => "\n",
        "crlf" => "\r\n",
        _ => return Err("发送行尾必须为 none、cr、lf 或 crlf".to_string()),
    };
    let codec = serial_text_encoding(encoding.as_deref())?;
    let payload = encode_text(&format!("{}{}", text, suffix), codec.as_deref())?;
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::WriteRaw(payload),
    )
    .await
}

#[tauri::command]
pub async fn serial_send_file(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::SendFile(path),
    )
    .await
}

#[tauri::command]
pub async fn serial_set_control_line(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    line: String,
    enabled: bool,
) -> Result<(), String> {
    let request = match line.trim().to_ascii_lowercase().as_str() {
        "dtr" => SerialControlRequest::SetDtr(enabled),
        "rts" => SerialControlRequest::SetRts(enabled),
        "break" => SerialControlRequest::SetBreak(enabled),
        _ => return Err("控制线必须为 DTR、RTS 或 BREAK".to_string()),
    };
    run_serial_unit_control(supervisor.inner(), session_id, request).await
}

#[tauri::command]
pub async fn serial_clear_buffer(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    target: String,
) -> Result<(), String> {
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::Clear(target),
    )
    .await
}

#[tauri::command]
pub async fn serial_start_capture(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    path: String,
    append: Option<bool>,
) -> Result<(), String> {
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::StartCapture {
            path,
            append: append.unwrap_or(false),
        },
    )
    .await
}

#[tauri::command]
pub async fn serial_stop_capture(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
) -> Result<(), String> {
    run_serial_unit_control(
        supervisor.inner(),
        session_id,
        SerialControlRequest::StopCapture,
    )
    .await
}

#[tauri::command]
pub async fn serial_get_status(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
) -> Result<SerialStatus, String> {
    match supervisor
        .control_serial(session_id, SerialControlRequest::GetStatus)
        .await?
    {
        SerialControlResponse::Status(status) => Ok(status),
        SerialControlResponse::Unit => Err("串口状态查询未返回状态".to_string()),
    }
}

#[tauri::command]
pub async fn open_ssh_shell_channel(
    app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    root_session_id: String,
    channel_id: String,
    term_type: Option<String>,
    login_script: Option<String>,
) -> Result<(), String> {
    supervisor
        .open_shell_channel(
            app_handle,
            root_session_id,
            channel_id,
            term_type,
            login_script,
        )
        .await
}

#[tauri::command]
pub async fn write_ssh_shell_channel(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    root_session_id: String,
    channel_id: String,
    data: String,
) -> Result<(), String> {
    supervisor
        .write_shell_channel(root_session_id, channel_id, data)
        .await
}

#[tauri::command]
pub async fn resize_ssh_shell_channel(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    root_session_id: String,
    channel_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    supervisor
        .resize_shell_channel(root_session_id, channel_id, cols, rows)
        .await
}

#[tauri::command]
pub async fn close_ssh_shell_channel(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    root_session_id: String,
    channel_id: String,
) -> Result<(), String> {
    supervisor
        .close_shell_channel(root_session_id, channel_id)
        .await
}

#[tauri::command]
pub async fn disconnect_ssh(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SshAppState>,
    sftp_state: tauri::State<'_, SftpAppState>,
    tunnel_state: tauri::State<'_, TunnelState>,
    session_id: String,
) -> Result<(), String> {
    supervisor
        .disconnect(
            sftp_state.inner().clone(),
            tunnel_state.inner().clone(),
            session_id,
        )
        .await
}
