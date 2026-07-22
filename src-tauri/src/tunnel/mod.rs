use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Semaphore};

use crate::{
    session::state::TunnelRuntimeState,
    ssh::{self, RemoteForwardTarget, SharedSshSessionSlot},
    storage::{self, SharedStorageState},
};

const MAX_TUNNELS: usize = 12;
const MAX_TUNNEL_CONNECTIONS_PER_TUNNEL: usize = 64;
const SAFE_LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TunnelRequest {
    pub id: Option<String>,
    pub name: Option<String>,
    pub config_id: Option<String>,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub private_key_path: Option<String>,
    pub mode: String,
    pub listen_host: Option<String>,
    pub listen_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub server_alive_interval: Option<u64>,
    pub allow_public_bind: Option<bool>,
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub id: String,
    pub name: String,
    pub config_id: Option<String>,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub mode: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub status: String,
    pub pid: Option<u32>,
    pub started_at: u64,
    pub command_preview: String,
}

#[derive(Clone)]
pub struct TunnelState {
    tunnels: Arc<Mutex<HashMap<String, TunnelInfo>>>,
}

impl TunnelState {
    pub fn new() -> Self {
        Self {
            tunnels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_map<T>(
        &self,
        f: impl FnOnce(&mut HashMap<String, TunnelInfo>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut map = self
            .tunnels
            .lock()
            .map_err(|_| "Failed to access tunnel state".to_string())?;
        f(&mut map)
    }

    fn ensure_can_register(&self, info: &TunnelInfo) -> Result<(), String> {
        self.with_map(|map| {
            if map.len() >= MAX_TUNNELS {
                return Err(format!(
                    "隧道数量超限（最多 {} 条），请先停止不使用的隧道",
                    MAX_TUNNELS
                ));
            }
            if map.contains_key(&info.id) {
                return Err("隧道ID已存在，请重试".to_string());
            }
            if map
                .values()
                .any(|item| item.command_preview == info.command_preview)
            {
                return Err("检测到重复隧道规则，已阻止重复启动".to_string());
            }
            Ok(())
        })
    }

    fn register(&self, info: TunnelInfo) -> Result<(), String> {
        self.with_map(|map| {
            map.insert(info.id.clone(), info);
            Ok(())
        })
    }

    fn remove(&self, id: &str) -> Result<Option<TunnelInfo>, String> {
        self.with_map(|map| Ok(map.remove(id)))
    }

    pub fn list(&self) -> Result<Vec<TunnelInfo>, String> {
        self.with_map(|map| {
            let mut list = map.values().cloned().collect::<Vec<_>>();
            list.sort_by_key(|item| item.started_at);
            Ok(list)
        })
    }

    pub fn session_id_for_tunnel(&self, tunnel_id: &str) -> Result<Option<String>, String> {
        self.with_map(|map| Ok(map.get(tunnel_id).and_then(|item| item.session_id.clone())))
    }

    pub fn tunnel_ids_for_session(&self, session_id: &str) -> Result<Vec<String>, String> {
        self.with_map(|map| {
            Ok(map
                .iter()
                .filter_map(|(id, item)| {
                    if item.session_id.as_deref() == Some(session_id) {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect())
        })
    }

    pub fn tunnel_ids(&self) -> Result<Vec<String>, String> {
        self.with_map(|map| Ok(map.keys().cloned().collect()))
    }
}

struct LocalTunnelResources {
    shared_session_slot: SharedSshSessionSlot,
    owned_connection: Option<ssh::SharedTunnelSshConnection>,
}

struct SavedTunnelLaunch {
    request: TunnelRequest,
    ssh_config: ssh::SshConfig,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn is_safe_loopback_host(host: &str) -> bool {
    let lowered = host.trim().to_ascii_lowercase();
    SAFE_LOOPBACK_HOSTS.contains(&lowered.as_str())
}

fn normalized_listen_host(req: &TunnelRequest) -> String {
    normalize_non_empty(req.listen_host.clone()).unwrap_or_else(|| "127.0.0.1".to_string())
}

fn allow_public_bind(req: &TunnelRequest) -> bool {
    req.allow_public_bind.unwrap_or(false)
}

fn validate_request(req: &TunnelRequest) -> Result<(), String> {
    if req.host.trim().is_empty() {
        return Err("SSH 主机不能为空".to_string());
    }
    if req.username.trim().is_empty() {
        return Err("SSH 用户名不能为空".to_string());
    }
    if req.listen_port == 0 {
        return Err("监听端口无效".to_string());
    }
    if req.listen_port < 1024 {
        return Err("为降低影响目标服务器风险，默认禁止使用 1024 以下监听端口".to_string());
    }

    let listen_host = normalized_listen_host(req);
    if !is_safe_loopback_host(&listen_host) && !allow_public_bind(req) {
        return Err(
            "安全策略已阻止公网监听。若确需公网绑定，请显式启用“允许公网监听”并确认风险"
                .to_string(),
        );
    }

    match req.mode.trim().to_ascii_lowercase().as_str() {
        "local" | "remote" => {
            if req.target_host.as_deref().unwrap_or("").trim().is_empty() {
                return Err("目标主机不能为空".to_string());
            }
            if req.target_port.unwrap_or(0) == 0 {
                return Err("目标端口无效".to_string());
            }
        }
        "dynamic" => {}
        _ => return Err("不支持的隧道模式，请使用 local/remote/dynamic".to_string()),
    }

    Ok(())
}

fn preview_for_request(req: &TunnelRequest) -> String {
    let listen_host = normalized_listen_host(req);
    match req.mode.trim().to_ascii_lowercase().as_str() {
        "local" => format!(
            "session-runtime local {}:{} -> {}:{} via {}@{}:{}",
            listen_host,
            req.listen_port,
            req.target_host.clone().unwrap_or_default(),
            req.target_port.unwrap_or_default(),
            req.username.trim(),
            req.host.trim(),
            req.port
        ),
        "remote" => format!(
            "session-runtime remote {}:{} -> {}:{} via {}@{}:{}",
            listen_host,
            req.listen_port,
            req.target_host.clone().unwrap_or_default(),
            req.target_port.unwrap_or_default(),
            req.username.trim(),
            req.host.trim(),
            req.port
        ),
        _ => format!(
            "session-runtime dynamic {}:{} via {}@{}:{}",
            listen_host,
            req.listen_port,
            req.username.trim(),
            req.host.trim(),
            req.port
        ),
    }
}

fn build_tunnel_info(req: &TunnelRequest) -> TunnelInfo {
    let id = normalize_non_empty(req.id.clone()).unwrap_or_else(|| format!("tunnel-{}", now_millis()));
    let name = normalize_non_empty(req.name.clone()).unwrap_or_else(|| {
        let mode_tag = match req.mode.to_ascii_lowercase().as_str() {
            "local" => "L",
            "remote" => "R",
            "dynamic" => "D",
            _ => "T",
        };
        format!("{}:{} [{}]", req.host, req.listen_port, mode_tag)
    });

    TunnelInfo {
        id,
        name,
        config_id: req.config_id.clone(),
        session_id: req.session_id.clone(),
        session_name: req.session_name.clone(),
        host: req.host.trim().to_string(),
        port: req.port,
        username: req.username.trim().to_string(),
        mode: req.mode.trim().to_ascii_lowercase(),
        listen_host: normalized_listen_host(req),
        listen_port: req.listen_port,
        target_host: normalize_non_empty(req.target_host.clone()),
        target_port: req.target_port,
        status: "running".to_string(),
        pid: None,
        started_at: now_millis(),
        command_preview: preview_for_request(req),
    }
}

fn load_saved_tunnel_launch(
    storage_state: &SharedStorageState,
    config_id: &str,
) -> Result<SavedTunnelLaunch, String> {
    let state = storage_state
        .lock()
        .map_err(|_| "Failed to access storage state".to_string())?;
    let config = storage::load_tunnel_config_record(&state, config_id)?;
    let session = storage::load_session_record(&state, &config.session_id)?;
    let session = storage::decrypt_session_config(&state, session)?;

    let protocol = session
        .protocol
        .clone()
        .unwrap_or_else(|| "ssh".to_string())
        .trim()
        .to_ascii_lowercase();
    if protocol != "ssh" {
        return Err("Independent tunnel startup only supports SSH sessions".to_string());
    }

    let request = TunnelRequest {
        id: None,
        name: Some(config.name.clone()),
        config_id: Some(config.id.clone()),
        session_id: Some(session.id.clone()),
        session_name: Some(session.name.clone()),
        host: session.host.clone(),
        port: session.port,
        username: session.username.clone(),
        private_key_path: session.private_key_path.clone(),
        mode: config.mode.clone(),
        listen_host: Some(config.listen_host.clone()),
        listen_port: config.listen_port,
        target_host: config.target_host.clone(),
        target_port: config.target_port,
        server_alive_interval: Some(config.server_alive_interval),
        allow_public_bind: Some(config.allow_public_bind),
    };

    let ssh_config = ssh::SshConfig {
        protocol: Some("ssh".to_string()),
        host: session.host,
        port: session.port,
        username: session.username,
        password: session.password,
        private_key_path: session.private_key_path,
        passphrase: session.passphrase,
        connect_timeout: session.connect_timeout,
        keep_alive_interval: Some(config.server_alive_interval),
        term_type: None,
        login_script: session.login_script,
        jump_host: session.jump_host,
        jump_port: session.jump_port,
        jump_username: session.jump_username,
        jump_auth_type: session.jump_auth_type,
        jump_password: session.jump_password,
        jump_private_key_path: session.jump_private_key_path,
        jump_passphrase: session.jump_passphrase,
        serial_path: None,
        baud_rate: None,
        data_bits: None,
        stop_bits: None,
        parity: None,
        flow_control: None,
    };

    Ok(SavedTunnelLaunch { request, ssh_config })
}

fn build_direct_ssh_config(request: &TunnelRequest) -> ssh::SshConfig {
    ssh::SshConfig {
        protocol: Some("ssh".to_string()),
        host: request.host.clone(),
        port: request.port,
        username: request.username.clone(),
        password: None,
        private_key_path: request.private_key_path.clone(),
        passphrase: None,
        connect_timeout: Some(10),
        keep_alive_interval: request.server_alive_interval,
        term_type: None,
        login_script: None,
        jump_host: None,
        jump_port: None,
        jump_username: None,
        jump_auth_type: None,
        jump_password: None,
        jump_private_key_path: None,
        jump_passphrase: None,
        serial_path: None,
        baud_rate: None,
        data_bits: None,
        stop_bits: None,
        parity: None,
        flow_control: None,
    }
}

pub fn load_session_id_for_tunnel_config(
    storage_state: &SharedStorageState,
    config_id: &str,
) -> Result<String, String> {
    let state = storage_state
        .lock()
        .map_err(|_| "Failed to access storage state".to_string())?;
    let config = storage::load_tunnel_config_record(&state, config_id)?;
    Ok(config.session_id)
}

fn live_shared_session_slot(
    slot: Option<SharedSshSessionSlot>,
) -> Option<SharedSshSessionSlot> {
    let candidate = slot?;
    if candidate
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
        .is_some()
    {
        Some(candidate)
    } else {
        None
    }
}

async fn resolve_local_tunnel_resources(
    app_handle: AppHandle,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    live_slot: Option<SharedSshSessionSlot>,
    ssh_config: Option<ssh::SshConfig>,
) -> Result<LocalTunnelResources, String> {
    if let Some(shared_session_slot) = live_shared_session_slot(live_slot) {
        return Ok(LocalTunnelResources {
            shared_session_slot,
            owned_connection: None,
        });
    }

    let owned_connection = ssh::connect_shared_ssh_runtime(
        app_handle,
        pending_hostkey,
        session_id,
        ssh_config.ok_or_else(|| "Tunnel startup requires a saved SSH session or direct key-auth data".to_string())?,
        None,
    )
    .await?;

    let shared_session_slot = owned_connection.shared_session_slot();
    Ok(LocalTunnelResources {
        shared_session_slot,
        owned_connection: Some(owned_connection),
    })
}

async fn open_direct_tcpip_channel(
    shared_session_slot: SharedSshSessionSlot,
    target_host: String,
    target_port: u16,
    origin_host: String,
    origin_port: u32,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    let shared_session = shared_session_slot
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "SSH session not ready for tunnel forwarding".to_string())?;

    let session = shared_session.lock().await;
    session
        .channel_open_direct_tcpip(target_host, target_port as u32, origin_host, origin_port)
        .await
        .map_err(|error| format!("Failed to open direct-tcpip channel: {}", error))
}

async fn is_shared_session_closed(shared_session_slot: &SharedSshSessionSlot) -> bool {
    let shared_session = {
        let guard = match shared_session_slot.lock() {
            Ok(guard) => guard,
            Err(_) => return true,
        };
        guard.clone()
    };

    let Some(shared_session) = shared_session else {
        return true;
    };

    let session = shared_session.lock().await;
    session.is_closed()
}

async fn proxy_stream_to_ssh(
    inbound: TcpStream,
    shared_session_slot: SharedSshSessionSlot,
    target_host: String,
    target_port: u16,
) -> Result<(), String> {
    let peer = inbound.peer_addr().ok();
    let origin_host = peer
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let origin_port = peer.map(|addr| addr.port() as u32).unwrap_or(0);

    let channel = open_direct_tcpip_channel(
        shared_session_slot,
        target_host,
        target_port,
        origin_host,
        origin_port,
    )
    .await?;

    let mut remote_stream = channel.into_stream();
    let mut local_stream = inbound;
    let _ = tokio::io::copy_bidirectional(&mut remote_stream, &mut local_stream).await;
    let _ = remote_stream.shutdown().await;
    let _ = local_stream.shutdown().await;
    Ok(())
}

async fn write_socks_reply(stream: &mut TcpStream, status: u8) -> Result<(), String> {
    stream
        .write_all(&[0x05, status, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(|error| format!("Failed to write SOCKS reply: {}", error))
}

async fn handle_socks5_client(
    mut inbound: TcpStream,
    shared_session_slot: SharedSshSessionSlot,
) -> Result<(), String> {
    let mut greeting = [0u8; 2];
    inbound
        .read_exact(&mut greeting)
        .await
        .map_err(|error| format!("Failed to read SOCKS greeting: {}", error))?;
    if greeting[0] != 0x05 {
        return Err("Unsupported SOCKS version".to_string());
    }

    let mut methods = vec![0u8; greeting[1] as usize];
    inbound
        .read_exact(&mut methods)
        .await
        .map_err(|error| format!("Failed to read SOCKS methods: {}", error))?;
    if !methods.contains(&0x00) {
        inbound
            .write_all(&[0x05, 0xff])
            .await
            .map_err(|error| format!("Failed to reject SOCKS auth methods: {}", error))?;
        return Err("SOCKS client does not support no-auth mode".to_string());
    }

    inbound
        .write_all(&[0x05, 0x00])
        .await
        .map_err(|error| format!("Failed to accept SOCKS auth method: {}", error))?;

    let mut request_head = [0u8; 4];
    inbound
        .read_exact(&mut request_head)
        .await
        .map_err(|error| format!("Failed to read SOCKS request head: {}", error))?;
    if request_head[0] != 0x05 {
        return Err("Invalid SOCKS request version".to_string());
    }
    if request_head[1] != 0x01 {
        let _ = write_socks_reply(&mut inbound, 0x07).await;
        return Err("Only SOCKS CONNECT is supported".to_string());
    }

    let target_host = match request_head[3] {
        0x01 => {
            let mut addr = [0u8; 4];
            inbound
                .read_exact(&mut addr)
                .await
                .map_err(|error| format!("Failed to read IPv4 target: {}", error))?;
            std::net::Ipv4Addr::from(addr).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            inbound
                .read_exact(&mut len)
                .await
                .map_err(|error| format!("Failed to read domain length: {}", error))?;
            let mut domain = vec![0u8; len[0] as usize];
            inbound
                .read_exact(&mut domain)
                .await
                .map_err(|error| format!("Failed to read domain target: {}", error))?;
            String::from_utf8(domain).map_err(|_| "Invalid domain target".to_string())?
        }
        0x04 => {
            let mut addr = [0u8; 16];
            inbound
                .read_exact(&mut addr)
                .await
                .map_err(|error| format!("Failed to read IPv6 target: {}", error))?;
            std::net::Ipv6Addr::from(addr).to_string()
        }
        _ => {
            let _ = write_socks_reply(&mut inbound, 0x08).await;
            return Err("Unsupported SOCKS address type".to_string());
        }
    };

    let mut port_bytes = [0u8; 2];
    inbound
        .read_exact(&mut port_bytes)
        .await
        .map_err(|error| format!("Failed to read SOCKS target port: {}", error))?;
    let target_port = u16::from_be_bytes(port_bytes);

    let peer = inbound.peer_addr().ok();
    let origin_host = peer
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let origin_port = peer.map(|addr| addr.port() as u32).unwrap_or(0);

    let channel = match open_direct_tcpip_channel(
        shared_session_slot,
        target_host,
        target_port,
        origin_host,
        origin_port,
    )
    .await
    {
        Ok(channel) => channel,
        Err(error) => {
            let _ = write_socks_reply(&mut inbound, 0x05).await;
            return Err(error);
        }
    };

    write_socks_reply(&mut inbound, 0x00).await?;

    let mut remote_stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut remote_stream, &mut inbound).await;
    let _ = remote_stream.shutdown().await;
    let _ = inbound.shutdown().await;
    Ok(())
}

async fn spawn_local_listener_task(
    info: TunnelInfo,
    listener: TcpListener,
    tunnel_state: TunnelState,
    shared_session_slot: SharedSshSessionSlot,
    target_host: String,
    target_port: u16,
    owned_connection: Option<ssh::SharedTunnelSshConnection>,
    connection_limiter: Arc<Semaphore>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                break;
            }
            _ = heartbeat.tick() => {
                if is_shared_session_closed(&shared_session_slot).await {
                    break;
                }
            }
            accept_result = listener.accept() => {
                let Ok((mut inbound, _)) = accept_result else {
                    break;
                };
                let permit = match connection_limiter.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        let _ = inbound.shutdown().await;
                        continue;
                    }
                };
                let target_host = target_host.clone();
                let shared_session_slot = shared_session_slot.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = proxy_stream_to_ssh(inbound, shared_session_slot, target_host, target_port).await;
                });
            }
        }
    }

    let _ = tunnel_state.remove(&info.id);
    if let Some(connection) = owned_connection {
        connection.disconnect().await;
    }
}

async fn spawn_dynamic_listener_task(
    info: TunnelInfo,
    listener: TcpListener,
    tunnel_state: TunnelState,
    shared_session_slot: SharedSshSessionSlot,
    owned_connection: Option<ssh::SharedTunnelSshConnection>,
    connection_limiter: Arc<Semaphore>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                break;
            }
            _ = heartbeat.tick() => {
                if is_shared_session_closed(&shared_session_slot).await {
                    break;
                }
            }
            accept_result = listener.accept() => {
                let Ok((mut inbound, _)) = accept_result else {
                    break;
                };
                let permit = match connection_limiter.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        let _ = inbound.shutdown().await;
                        continue;
                    }
                };
                let shared_session_slot = shared_session_slot.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = handle_socks5_client(inbound, shared_session_slot).await;
                });
            }
        }
    }

    let _ = tunnel_state.remove(&info.id);
    if let Some(connection) = owned_connection {
        connection.disconnect().await;
    }
}

pub async fn start_tunnel_runtime(
    app_handle: AppHandle,
    tunnel_state: &TunnelState,
    runtime_state: &mut TunnelRuntimeState,
    live_shared_session: Option<SharedSshSessionSlot>,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    request: TunnelRequest,
    ssh_config: Option<ssh::SshConfig>,
) -> Result<TunnelInfo, String> {
    validate_request(&request)?;

    let mut info = build_tunnel_info(&request);
    tunnel_state.ensure_can_register(&info)?;

    match info.mode.as_str() {
        "local" => {
            let listener = TcpListener::bind((info.listen_host.as_str(), info.listen_port))
                .await
                .map_err(|error| format!("Failed to bind local tunnel listener: {}", error))?;

            let session_id = request
                .session_id
                .clone()
                .unwrap_or_else(|| info.id.clone());
            let resources = resolve_local_tunnel_resources(
                app_handle,
                pending_hostkey,
                session_id,
                live_shared_session,
                ssh_config.or_else(|| Some(build_direct_ssh_config(&request))),
            )
            .await?;

            tunnel_state.register(info.clone())?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let task = tokio::spawn(spawn_local_listener_task(
                info.clone(),
                listener,
                tunnel_state.clone(),
                resources.shared_session_slot,
                info.target_host.clone().unwrap_or_default(),
                info.target_port.unwrap_or_default(),
                resources.owned_connection,
                Arc::new(Semaphore::new(MAX_TUNNEL_CONNECTIONS_PER_TUNNEL)),
                shutdown_rx,
            ));
            runtime_state.handles.insert(
                info.id.clone(),
                crate::session::state::ManagedTunnelRuntime {
                    shutdown: Some(shutdown_tx),
                    task: Some(task),
                },
            );
            Ok(info)
        }
        "dynamic" => {
            let listener = TcpListener::bind((info.listen_host.as_str(), info.listen_port))
                .await
                .map_err(|error| format!("Failed to bind dynamic tunnel listener: {}", error))?;

            let session_id = request
                .session_id
                .clone()
                .unwrap_or_else(|| info.id.clone());
            let resources = resolve_local_tunnel_resources(
                app_handle,
                pending_hostkey,
                session_id,
                live_shared_session,
                ssh_config.or_else(|| Some(build_direct_ssh_config(&request))),
            )
            .await?;

            tunnel_state.register(info.clone())?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let task = tokio::spawn(spawn_dynamic_listener_task(
                info.clone(),
                listener,
                tunnel_state.clone(),
                resources.shared_session_slot,
                resources.owned_connection,
                Arc::new(Semaphore::new(MAX_TUNNEL_CONNECTIONS_PER_TUNNEL)),
                shutdown_rx,
            ));
            runtime_state.handles.insert(
                info.id.clone(),
                crate::session::state::ManagedTunnelRuntime {
                    shutdown: Some(shutdown_tx),
                    task: Some(task),
                },
            );
            Ok(info)
        }
        "remote" => {
            let session_id = request
                .session_id
                .clone()
                .unwrap_or_else(|| info.id.clone());
            let connection = ssh::connect_shared_ssh_runtime(
                app_handle,
                pending_hostkey,
                session_id,
                ssh_config.unwrap_or_else(|| build_direct_ssh_config(&request)),
                Some(RemoteForwardTarget {
                    target_host: info.target_host.clone().unwrap_or_default(),
                    target_port: info.target_port.unwrap_or_default(),
                }),
            )
            .await?;

            let session = connection.shared_session.lock().await;
            let allocated_port = session
                .tcpip_forward(info.listen_host.clone(), info.listen_port as u32)
                .await
                .map_err(|error| format!("Failed to request remote forward: {}", error))?;
            drop(session);

            if allocated_port > 0 {
                info.listen_port = allocated_port as u16;
            }

            tunnel_state.register(info.clone())?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let tunnel_state_clone = tunnel_state.clone();
            let info_clone = info.clone();
            let task = tokio::spawn(async move {
                let mut shutdown_rx = shutdown_rx;
                let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => {
                            break;
                        }
                        _ = heartbeat.tick() => {
                            if is_shared_session_closed(&connection.shared_session_slot).await {
                                break;
                            }
                        }
                    }
                }
                {
                    let session = connection.shared_session.lock().await;
                    let _ = session
                        .cancel_tcpip_forward(info_clone.listen_host.clone(), info_clone.listen_port as u32)
                        .await;
                }
                connection.disconnect().await;
                let _ = tunnel_state_clone.remove(&info_clone.id);
            });
            runtime_state.handles.insert(
                info.id.clone(),
                crate::session::state::ManagedTunnelRuntime {
                    shutdown: Some(shutdown_tx),
                    task: Some(task),
                },
            );
            Ok(info)
        }
        _ => Err("不支持的隧道模式".to_string()),
    }
}

pub async fn start_tunnel_from_config_runtime(
    app_handle: AppHandle,
    tunnel_state: &TunnelState,
    runtime_state: &mut TunnelRuntimeState,
    live_shared_session: Option<SharedSshSessionSlot>,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    storage_state: &SharedStorageState,
    config_id: String,
) -> Result<TunnelInfo, String> {
    let launch = load_saved_tunnel_launch(storage_state, &config_id)?;
    start_tunnel_runtime(
        app_handle,
        tunnel_state,
        runtime_state,
        live_shared_session,
        pending_hostkey,
        launch.request,
        Some(launch.ssh_config),
    )
    .await
}

pub async fn stop_tunnel_runtime(
    tunnel_state: &TunnelState,
    runtime_state: &mut TunnelRuntimeState,
    id: &str,
) -> Result<(), String> {
    if let Some(mut handle) = runtime_state.handles.remove(id) {
        if let Some(shutdown) = handle.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = handle.task.take() {
            let _ = task.await;
        }
        let _ = tunnel_state.remove(id);
        return Ok(());
    }

    if tunnel_state.remove(id)?.is_some() {
        return Ok(());
    }

    Err("隧道不存在或已停止".to_string())
}

pub async fn stop_all_runtime_tunnels(
    tunnel_state: &TunnelState,
    runtime_state: &mut TunnelRuntimeState,
) -> Result<(), String> {
    let ids = runtime_state.handles.keys().cloned().collect::<Vec<_>>();
    for id in ids {
        stop_tunnel_runtime(tunnel_state, runtime_state, &id).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_tunnels(state: tauri::State<'_, TunnelState>) -> Result<Vec<TunnelInfo>, String> {
    state.list()
}

#[tauri::command]
pub async fn start_tunnel(
    app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    state: tauri::State<'_, TunnelState>,
    request: TunnelRequest,
) -> Result<TunnelInfo, String> {
    supervisor
        .start_tunnel(app_handle, state.inner().clone(), request)
        .await
}

#[tauri::command]
pub async fn start_tunnel_from_config(
    app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    state: tauri::State<'_, TunnelState>,
    storage_state: tauri::State<'_, SharedStorageState>,
    config_id: String,
) -> Result<TunnelInfo, String> {
    supervisor
        .start_tunnel_from_config(
            app_handle,
            state.inner().clone(),
            storage_state.inner().clone(),
            config_id,
        )
        .await
}

#[tauri::command]
pub async fn stop_tunnel(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    state: tauri::State<'_, TunnelState>,
    id: String,
) -> Result<(), String> {
    supervisor.stop_tunnel(state.inner().clone(), id).await
}

#[tauri::command]
pub async fn stop_all_tunnels(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    state: tauri::State<'_, TunnelState>,
) -> Result<(), String> {
    for id in state.tunnel_ids()? {
        supervisor.stop_tunnel(state.inner().clone(), id).await?;
    }
    Ok(())
}
