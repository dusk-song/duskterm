use std::io::{Cursor, Read, Write};
use std::future::Future;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::sftp::SftpAppState;
use crate::ssh_algorithms::{self, ConnectAttemptError, NegotiationProfile, NegotiationProfileCache};
use crate::terminal_transfer::{TerminalTransferProbe, ZmodemDetector};
use crate::tunnel::TunnelState;
use async_trait::async_trait;
use russh::{client, ChannelMsg, Disconnect};
use russh::Pty;
use russh_keys::{check_known_hosts_path, load_secret_key};
use ssh_key::HashAlg;

use serialport::{available_ports, DataBits, FlowControl, Parity, StopBits};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{channel, unbounded_channel, Receiver, Sender, UnboundedSender};
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use zeroize::Zeroize;

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

    pub fn remember_successful_profile(
        &self,
        host: &str,
        port: u16,
        profile: NegotiationProfile,
    ) {
        self.negotiation_profiles
            .remember_successful_profile(host, port, profile);
    }
}

pub type SharedSshSession = Arc<AsyncMutex<client::Handle<ClientHandler>>>;
pub type SharedSshSessionSlot = Arc<Mutex<Option<SharedSshSession>>>;
const SSH_INPUT_QUEUE_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct TerminalRuntimeHandle {
    pub tx: Sender<Vec<u8>>,
    pub window_size_tx: UnboundedSender<(u32, u32)>,
    pub close_tx: UnboundedSender<()>,
    pub shared_session: SharedSshSessionSlot,
}

pub type SessionIoReceiver = Receiver<Vec<u8>>;
pub type SessionResizeReceiver = tokio::sync::mpsc::UnboundedReceiver<(u32, u32)>;
pub type SessionCloseReceiver = tokio::sync::mpsc::UnboundedReceiver<()>;

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
pub struct SerialPortOption {
    path: String,
    label: String,
}

fn normalized_protocol(value: Option<&str>) -> &'static str {
    match value.unwrap_or("ssh").trim().to_ascii_lowercase().as_str() {
        "telnet" => "telnet",
        "serial" => "serial",
        _ => "ssh",
    }
}

fn socket_address(host: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("地址解析失败: {}", e))?
        .next()
        .ok_or_else(|| "未解析到可用地址".to_string())
}

fn serial_port_path(config: &SshConfig) -> Result<String, String> {
    config
        .serial_path
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "串口设备路径不能为空".to_string())
}

fn serial_baud_rate(config: &SshConfig) -> u32 {
    config.baud_rate.unwrap_or(9600)
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

fn build_serial_port(config: &SshConfig) -> Result<Box<dyn serialport::SerialPort>, String> {
    let path = serial_port_path(config)?;
    let timeout = config.connect_timeout.unwrap_or(10).clamp(1, 120);
    serialport::new(path, serial_baud_rate(config))
        .data_bits(serial_data_bits(config)?)
        .stop_bits(serial_stop_bits(config)?)
        .parity(serial_parity(config)?)
        .flow_control(serial_flow_control(config)?)
        .timeout(Duration::from_millis((timeout * 100).clamp(100, 2000)))
        .open()
        .map_err(|e| format!("串口打开失败: {}", e))
}

fn emit_session_error(app_handle: &AppHandle, session_id: &str, error: impl Into<String>) {
    let _ = app_handle.emit(&format!("ssh-error-{}", session_id), error.into());
}

fn emit_session_closed(app_handle: &AppHandle, session_id: &str, reason: impl Into<String>) {
    let _ = app_handle.emit(&format!("ssh-closed-{}", session_id), reason.into());
}

fn handle_terminal_transfer_probe(
    app_handle: &AppHandle,
    session_id: &str,
    detector: &mut ZmodemDetector,
    data: &[u8],
) -> Option<Vec<u8>> {
    match detector.inspect(data) {
        TerminalTransferProbe::TerminalData(data) => Some(data),
        TerminalTransferProbe::Detected(request, data) => {
            let _ = app_handle.emit(
                &format!("terminal-transfer-request-{}", session_id),
                request,
            );
            Some(data)
        }
    }
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

fn maybe_send_serial_login_script<W: Write>(
    app_handle: &AppHandle,
    session_id: &str,
    writer: &mut W,
    login_script: Option<&String>,
) -> Result<(), String> {
    if let Some(script) = login_script {
        let trimmed = script.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let mut payload = trimmed.as_bytes().to_vec();
        payload.extend_from_slice(b"\r\n");
        writer
            .write_all(&payload)
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
        writer
            .flush()
            .map_err(|e| format!("发送登录脚本失败: {}", e))?;
        let _ = app_handle.emit(&format!("serial-data-sent-{}", session_id), payload);
    }
    Ok(())
}

fn normalize_telnet_term_type(value: Option<&str>) -> String {
    let trimmed = value.unwrap_or("").trim();
    let valid = trimmed.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
    });
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
        for value in [
            (cols >> 8) as u8,
            cols as u8,
            (rows >> 8) as u8,
            rows as u8,
        ] {
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
        if payload.len() >= 2
            && payload[0] == Self::OPT_TTYPE
            && payload[1] == Self::TTYPE_SEND
        {
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

fn spawn_serial_session(
    app_handle: AppHandle,
    session_id: String,
    config: SshConfig,
    mut rx: SessionIoReceiver,
    mut close_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    thread::spawn(move || {
        let outcome = (|| -> Result<(), String> {
            let mut port = build_serial_port(&config)?;
            let _ = app_handle.emit(&format!("ssh-connected-{}", session_id), ());

            maybe_send_serial_login_script(
                &app_handle,
                &session_id,
                &mut port,
                config.login_script.as_ref(),
            )?;

            let mut read_buf = [0u8; 4096];
            loop {
                if close_rx.try_recv().is_ok() {
                    break;
                }

                while let Ok(data) = rx.try_recv() {
                    if !data.is_empty() {
                        port.write_all(&data)
                            .map_err(|e| format!("串口写入失败: {}", e))?;
                        port.flush().map_err(|e| format!("串口写入失败: {}", e))?;
                        let _ = app_handle.emit(&format!("serial-data-sent-{}", session_id), data);
                    }
                }

                match port.read(&mut read_buf) {
                    Ok(0) => {}
                    Ok(read) => {
                        let _ = app_handle.emit(
                            &format!("ssh-data-{}", session_id),
                            read_buf[..read].to_vec(),
                        );
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(error) => return Err(format!("串口读取失败: {}", error)),
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
    jump_session: Option<client::Handle<ClientHandler>>,
}

impl SharedTunnelSshConnection {
    pub fn shared_session_slot(&self) -> SharedSshSessionSlot {
        self.shared_session_slot.clone()
    }

    pub async fn disconnect(self) {
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

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match check_known_hosts_path(
            &self.host,
            self.port,
            server_public_key,
            &self.known_hosts_path,
        ) {
            Ok(true) => Ok(true),
            Ok(false) => {
                let (tx, rx) = oneshot::channel::<bool>();
                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    *pending = Some(tx);
                }

                // In 0.48, fingerprint takes hash_alg. And algo name needs Named trait or use algorithm() from ssh_key
                let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
                let algo = server_public_key.algorithm().to_string();

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
                    let _ = append_known_host(
                        &self.host,
                        self.port,
                        server_public_key,
                        &self.known_hosts_path,
                    );
                    Ok(true)
                } else {
                    let _ = self.app_handle.emit(
                        &format!("ssh-error-{}", self.session_id),
                        "Host key not trusted. Connection cancelled.".to_string(),
                    );
                    Ok(false)
                }
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
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(target) = self.remote_forward_target.clone() else {
            let mut stream = channel.into_stream();
            let _ = stream.shutdown().await;
            return Ok(());
        };

        tokio::spawn(async move {
            match tokio::net::TcpStream::connect((target.target_host.as_str(), target.target_port))
                .await
            {
                Ok(mut local_stream) => {
                    let mut remote_stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut remote_stream, &mut local_stream)
                        .await;
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

#[async_trait]
#[cfg(test)]
impl client::Handler for TestClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
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

fn append_known_host(
    host: &str,
    port: u16,
    key: &ssh_key::PublicKey,
    path: &PathBuf,
) -> Result<(), String> {
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

    russh_sftp::client::SftpSession::new(channel.into_stream())
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

async fn authenticate_session<H>(
    session: &mut client::Handle<H>,
    username: String,
    private_key_path: Option<String>,
    password: Option<String>,
    mut passphrase: Option<String>,
) -> Result<bool, String>
where
    H: client::Handler + Send + 'static,
    H::Error: std::fmt::Display,
{
    if let Some(p) = passphrase.as_ref() {
        if p.trim().is_empty() {
            passphrase = None;
        }
    }

    let result = if let Some(key_path) =
        private_key_path.and_then(|p| if p.trim().is_empty() { None } else { Some(p) })
    {
        let key_path_buf = PathBuf::from(&key_path);
        ensure_private_key_permissions(&key_path_buf)
            .map_err(|e| format!("Private key permissions must be 0600: {}", e))?;

        let key_pair = load_secret_key(key_path, passphrase.as_deref()).map_err(|e| {
            if passphrase.is_none() {
                "Private key is encrypted; passphrase required".to_string()
            } else {
                format!("Failed to load private key: {}", e)
            }
        })?;

        let key_alg = key_pair.algorithm();
        ssh_algorithms::validate_private_key_algorithm(&key_alg)?;

        session
            .authenticate_publickey(username, Arc::new(key_pair))
            .await
            .map_err(|e| format!("Authentication failed: {}", e))?
    } else if let Some(password) = password {
        let mut password = password;
        if password.trim().is_empty() {
            return Err("Empty password not allowed".to_string());
        }
        let auth = session
            .authenticate_password(username, password.clone())
            .await
            .map_err(|e| format!("Authentication failed: {}", e));
        password.zeroize();
        auth?
    } else {
        return Err("No authentication method provided".to_string());
    };

    if let Some(mut p) = passphrase {
        p.zeroize();
    }

    Ok(result)
}

pub async fn connect_shared_ssh_runtime(
    app_handle: AppHandle,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    mut config: SshConfig,
    remote_forward_target: Option<RemoteForwardTarget>,
) -> Result<SharedTunnelSshConnection, String> {
    let known_hosts_path = app_known_hosts_path()
        .map_err(|error| format!("Failed to initialize known_hosts: {}", error))?;

    let mut jump_session = if let Some(jump) = extract_jump_host(&config) {
        let jump_known_hosts_path = app_known_hosts_path()
            .map_err(|error| format!("Failed to initialize known_hosts: {}", error))?;
        let jump_session_id = format!("{}::jump", session_id);
        let jump_host = jump.host.clone();
        let jump_port = jump.port;

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

        let jump_auth_ok = authenticate_session(
            &mut jump_handle,
            jump.username,
            jump.private_key_path,
            jump.password,
            jump.passphrase,
        )
        .await
        .map_err(|error| format!("Jump host authentication failed: {}", error))?;

        if !jump_auth_ok {
            return Err("Jump host authentication failed".to_string());
        }

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

            let client_config = build_client_config(config.keep_alive_interval, profile);
            match connect_stream_handle(client_config, stream, handler, config.connect_timeout).await
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

        connect_with_profile_retry(
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
        .await?
    };

    let auth_ok = authenticate_session(
        &mut session,
        config.username.clone(),
        config.private_key_path.clone(),
        config.password.clone(),
        config.passphrase.take(),
    )
    .await?;

    if !auth_ok {
        return Err("Authentication failed".to_string());
    }

    let shared_session: SharedSshSession = Arc::new(AsyncMutex::new(session));
    let shared_session_slot: SharedSshSessionSlot = Arc::new(Mutex::new(Some(shared_session.clone())));

    Ok(SharedTunnelSshConnection {
        shared_session,
        shared_session_slot,
        jump_session,
    })
}

async fn run_ssh_session_task(
    app_handle: AppHandle,
    sftp_state: SftpAppState,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
    shared_session_slot: SharedSshSessionSlot,
    mut rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: SessionCloseReceiver,
) {
    let term_type = config.term_type.clone();
    let login_script = config.login_script.clone();

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
            fail_session_connect(
                &app_handle,
                &shared_session_slot,
                &sftp_state,
                &session_id,
                error,
            );
            return;
        }
    };

    {
        let mut slot = shared_session_slot.lock().unwrap();
        *slot = Some(connection.shared_session.clone());
    }

    let mut channel = {
        let session = connection.shared_session.lock().await;
        match session.channel_open_session().await {
            Ok(channel) => channel,
            Err(error) => {
                fail_session_connect(
                    &app_handle,
                    &shared_session_slot,
                    &sftp_state,
                    &session_id,
                    format!("Channel open failed: {}", error),
                );
                return;
            }
        }
    };

    let term = term_type.as_deref().unwrap_or("xterm-256color");
    let terminal_modes = default_terminal_modes();
    if let Err(error) = channel.request_pty(true, term, 80, 24, 0, 0, &terminal_modes).await {
        fail_session_connect(
            &app_handle,
            &shared_session_slot,
            &sftp_state,
            &session_id,
            format!("PTY request failed: {}", error),
        );
        return;
    }

    if let Err(error) = channel.request_shell(true).await {
        fail_session_connect(
            &app_handle,
            &shared_session_slot,
            &sftp_state,
            &session_id,
            format!("Shell request failed: {}", error),
        );
        return;
    }

    if let Some(script) = &login_script {
        if !script.is_empty() {
            let _ = channel.data(script.as_bytes()).await;
            if !script.ends_with('\n') {
                let _ = channel.data("\n".as_bytes()).await;
            }
        }
    }

    let _ = app_handle.emit(&format!("ssh-connected-{}", session_id), ());
    let mut zmodem_detector = ZmodemDetector::new();

    loop {
        tokio::select! {
            Some(data) = rx.recv() => {
                if data.len() > 64 * 1024 {
                    let _ = app_handle.emit(
                        &format!("ssh-error-{}", session_id),
                        "Input too large; dropped to protect server".to_string(),
                    );
                    continue;
                }

                let reader = Cursor::new(data);
                if let Err(error) = channel.data(reader).await {
                    let _ = app_handle.emit(
                        &format!("ssh-error-{}", session_id),
                        format!("Write failed: {}", error),
                    );
                    break;
                }
            }
            Some((cols, rows)) = resize_rx.recv() => {
                let _ = channel.window_change(cols, rows, 0, 0).await;
            }
            Some(msg) = channel.wait() => {
                match msg {
                    ChannelMsg::Data { data } => {
                        if let Some(terminal_data) = handle_terminal_transfer_probe(
                            &app_handle,
                            &session_id,
                            &mut zmodem_detector,
                            data.as_ref(),
                        ) {
                            let _ = app_handle.emit(&format!("ssh-data-{}", session_id), terminal_data);
                        }
                    }
                    ChannelMsg::ExtendedData { data, .. } => {
                        let _ = app_handle.emit(&format!("ssh-data-{}", session_id), data.to_vec());
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        emit_session_closed(
                            &app_handle,
                            &session_id,
                            format!("remote sent exit status {}", exit_status),
                        );
                        break;
                    }
                    ChannelMsg::ExitSignal {
                        signal_name,
                        core_dumped,
                        error_message,
                        ..
                    } => {
                        emit_session_closed(
                            &app_handle,
                            &session_id,
                            format!(
                                "remote sent exit signal {:?}; core dumped: {}; {}",
                                signal_name,
                                core_dumped,
                                error_message
                            ),
                        );
                        break;
                    }
                    ChannelMsg::Eof => {
                        emit_session_closed(&app_handle, &session_id, "remote sent EOF");
                        break;
                    }
                    ChannelMsg::Close => {
                        emit_session_closed(&app_handle, &session_id, "remote sent channel close");
                        break;
                    }
                    _ => {}
                }
            }
            Some(_) = close_rx.recv() => {
                let _ = channel.close().await;
                {
                    let session = connection.shared_session.lock().await;
                    let _ = session.disconnect(Disconnect::ByApplication, "", "English").await;
                }
                emit_session_closed(&app_handle, &session_id, "closed by application");
                break;
            }
            else => break,
        }
    }

    connection.disconnect().await;
    cleanup_session_state(&shared_session_slot, &sftp_state, &session_id);
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

    // Channels for communication with the SSH task
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let shared_session_slot: SharedSshSessionSlot = Arc::new(Mutex::new(None));
    let runtime_handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: shared_session_slot.clone(),
    };

    match normalized_protocol(config.protocol.as_deref()) {
        "telnet" => {
            spawn_telnet_session(app_handle, session_id_clone, config, rx, resize_rx, close_rx);
            return Ok(runtime_handle);
        }
        "serial" => {
            spawn_serial_session(app_handle, session_id_clone, config, rx, close_rx);
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
            rx,
            resize_rx,
            close_rx,
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
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let shared_session_slot: SharedSshSessionSlot = Arc::new(Mutex::new(None));
    let handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: shared_session_slot.clone(),
    };

    let task = match normalized_protocol(config.protocol.as_deref()) {
        "telnet" => {
            spawn_telnet_session(app_handle, session_id, config, rx, resize_rx, close_rx);
            None
        }
        "serial" => {
            spawn_serial_session(app_handle, session_id, config, rx, close_rx);
            None
        }
        _ => Some(tokio::spawn(run_ssh_session_task(
            app_handle,
            sftp_state,
            pending_hostkey,
            session_id,
            config,
            shared_session_slot,
            rx,
            resize_rx,
            close_rx,
        ))),
    };

    Ok(crate::session::state::ManagedSshRuntime {
        handle,
        task,
    })
}

async fn run_shared_shell_channel_task(
    app_handle: AppHandle,
    channel_id: String,
    shared_session: SharedSshSession,
    term_type: Option<String>,
    login_script: Option<String>,
    mut rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: SessionCloseReceiver,
    ready_tx: oneshot::Sender<Result<(), String>>,
) {
    let mut ready_tx = Some(ready_tx);
    let mut channel = {
        let session = shared_session.lock().await;
        match session.channel_open_session().await {
            Ok(channel) => channel,
            Err(error) => {
                let message = format!("Channel open failed: {}", error);
                if let Some(tx) = ready_tx.take() { let _ = tx.send(Err(message.clone())); }
                let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
                return;
            }
        }
    };
    let terminal_modes = default_terminal_modes();
    if let Err(error) = channel.request_pty(true, term_type.as_deref().unwrap_or("xterm-256color"), 80, 24, 0, 0, &terminal_modes).await {
        let message = format!("PTY request failed: {}", error);
        if let Some(tx) = ready_tx.take() { let _ = tx.send(Err(message.clone())); }
        let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
        return;
    }
    if let Err(error) = channel.request_shell(true).await {
        let message = format!("Shell request failed: {}", error);
        if let Some(tx) = ready_tx.take() { let _ = tx.send(Err(message.clone())); }
        let _ = app_handle.emit(&format!("ssh-error-{}", channel_id), message);
        return;
    }
    if let Some(script) = login_script.filter(|script| !script.is_empty()) {
        let _ = channel.data(script.as_bytes()).await;
        if !script.ends_with('\n') { let _ = channel.data("\n".as_bytes()).await; }
    }
    if let Some(tx) = ready_tx.take() { let _ = tx.send(Ok(())); }
    let _ = app_handle.emit(&format!("ssh-connected-{}", channel_id), ());
    let mut zmodem_detector = ZmodemDetector::new();
    loop {
        tokio::select! {
            Some(data) = rx.recv() => {
                if data.len() > 64 * 1024 { continue; }
                if channel.data(Cursor::new(data)).await.is_err() { break; }
            }
            Some((cols, rows)) = resize_rx.recv() => { let _ = channel.window_change(cols, rows, 0, 0).await; }
            Some(msg) = channel.wait() => match msg {
                ChannelMsg::Data { data } => {
                    if let Some(terminal_data) = handle_terminal_transfer_probe(&app_handle, &channel_id, &mut zmodem_detector, data.as_ref()) {
                        let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), terminal_data);
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => { let _ = app_handle.emit(&format!("ssh-data-{}", channel_id), data.to_vec()); }
                ChannelMsg::ExitStatus { .. } | ChannelMsg::ExitSignal { .. } | ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            },
            Some(_) = close_rx.recv() => { let _ = channel.close().await; break; }
            else => break,
        }
    }
    emit_session_closed(&app_handle, &channel_id, "shell channel closed");
}

pub async fn open_shared_shell_channel_runtime(
    app_handle: AppHandle,
    root_handle: &TerminalRuntimeHandle,
    channel_id: String,
    term_type: Option<String>,
    login_script: Option<String>,
) -> Result<crate::session::state::ManagedSshRuntime, String> {
    let shared_session = root_handle.shared_session.lock().unwrap().clone()
        .ok_or_else(|| "Root SSH transport is not ready".to_string())?;
    let (tx, rx) = channel::<Vec<u8>>(SSH_INPUT_QUEUE_CAPACITY);
    let (resize_tx, resize_rx) = unbounded_channel::<(u32, u32)>();
    let (close_tx, close_rx) = unbounded_channel::<()>();
    let handle = TerminalRuntimeHandle {
        tx,
        window_size_tx: resize_tx,
        close_tx,
        shared_session: root_handle.shared_session.clone(),
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(run_shared_shell_channel_task(app_handle, channel_id, shared_session, term_type, login_script, rx, resize_rx, close_rx, ready_tx));
    match ready_rx.await {
        Ok(Ok(())) => Ok(crate::session::state::ManagedSshRuntime { handle, task: Some(task) }),
        Ok(Err(error)) => { let _ = task.await; Err(error) }
        Err(_) => { let _ = task.await; Err("Shell channel task ended before becoming ready".to_string()) }
    }
}

#[tauri::command]
pub async fn test_ssh_connection(config: SshConfig) -> Result<String, String> {
    let address = socket_address(&config.host, config.port)?;
    let timeout = Duration::from_secs(config.connect_timeout.unwrap_or(10).clamp(1, 120));

    match normalized_protocol(config.protocol.as_deref()) {
        "telnet" => {
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
            let label = match port.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    let mut parts = Vec::new();
                    if let Some(manufacturer) = info.manufacturer {
                        if !manufacturer.trim().is_empty() {
                            parts.push(manufacturer.trim().to_string());
                        }
                    }
                    if let Some(product) = info.product {
                        if !product.trim().is_empty() {
                            parts.push(product.trim().to_string());
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
            SerialPortOption { path, label }
        })
        .collect())
}

#[allow(dead_code)]
pub fn write_ssh_legacy(handle: &TerminalRuntimeHandle, data: String) -> Result<(), String> {
    handle
        .tx
        .try_send(data.into_bytes())
        .map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                "SSH input queue is full; dropped to protect memory".to_string()
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                "SSH session input channel is closed".to_string()
            }
        })
}

pub fn write_ssh_runtime(handle: &TerminalRuntimeHandle, data: String) -> Result<(), String> {
    handle
        .tx
        .try_send(data.into_bytes())
        .map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                "SSH input queue is full; dropped to protect memory".to_string()
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                "SSH session input channel is closed".to_string()
            }
        })
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

#[tauri::command]
pub async fn open_ssh_shell_channel(app_handle: AppHandle, supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>, root_session_id: String, channel_id: String, term_type: Option<String>, login_script: Option<String>) -> Result<(), String> {
    supervisor.open_shell_channel(app_handle, root_session_id, channel_id, term_type, login_script).await
}

#[tauri::command]
pub async fn write_ssh_shell_channel(supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>, root_session_id: String, channel_id: String, data: String) -> Result<(), String> {
    supervisor.write_shell_channel(root_session_id, channel_id, data).await
}

#[tauri::command]
pub async fn resize_ssh_shell_channel(supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>, root_session_id: String, channel_id: String, cols: u32, rows: u32) -> Result<(), String> {
    supervisor.resize_shell_channel(root_session_id, channel_id, cols, rows).await
}

#[tauri::command]
pub async fn close_ssh_shell_channel(supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>, root_session_id: String, channel_id: String) -> Result<(), String> {
    supervisor.close_shell_channel(root_session_id, channel_id).await
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
