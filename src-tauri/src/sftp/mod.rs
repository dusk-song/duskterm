use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{connection_log, ssh, ssh_algorithms};
use russh::{
    client,
    keys::{check_known_hosts_path, HashAlg, PublicKey},
};
use russh_sftp::{
    client::{error::Error as SftpClientError, RawSftpSession, SftpSession},
    protocol::{FileAttributes, StatusCode},
};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, Window};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use zeroize::Zeroize;
const SFTP_TRANSFER_BUFFER_SIZE: usize = 256 * 1024;
const SFTP_TRANSFER_CHANNEL_SIZE: usize = 8;
const SFTP_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(80);
const SFTP_PROGRESS_EMIT_STEP_BYTES: u64 = 256 * 1024;

pub(crate) fn sftp_client_config() -> russh_sftp::client::Config {
    let mut config = russh_sftp::client::Config::default();
    config.request_timeout_secs = 30;
    config.max_concurrent_writes = 16;
    config
}
const MAX_INLINE_EDITOR_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// 限制 SFTP list_cache 每个 session 最多缓存 40 个目录列表，防止内存泄漏
const MAX_LIST_CACHE_PER_SESSION: usize = 40;

fn should_emit_transfer_progress(last_emit: &std::time::Instant, bytes_since_emit: u64) -> bool {
    bytes_since_emit >= SFTP_PROGRESS_EMIT_STEP_BYTES
        || last_emit.elapsed() >= SFTP_PROGRESS_EMIT_INTERVAL
}

const EDITABLE_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "log", "conf", "cfg", "ini", "env", "json", "jsonc", "yaml", "yml", "toml", "xml",
    "csv", "tsv", "md", "markdown", "sql", "gql", "graphql", "js", "jsx", "mjs", "cjs", "ts",
    "tsx", "vue", "html", "htm", "css", "scss", "less", "rs", "py", "sh", "bash", "zsh", "ps1",
    "bat", "c", "h", "cpp", "cc", "cxx", "hpp", "hxx", "java", "go", "php", "rb", "swift", "kt",
];

const EDITABLE_TEXT_FILE_NAMES: &[&str] = &[
    "dockerfile",
    "makefile",
    "readme",
    "license",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".npmrc",
    ".prettierrc",
    ".eslintrc",
    ".bashrc",
    ".zshrc",
    ".profile",
];

fn is_supported_text_path(path: &str) -> bool {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .trim()
        .to_ascii_lowercase();

    if file_name.is_empty() {
        return false;
    }

    if file_name.starts_with(".env") || EDITABLE_TEXT_FILE_NAMES.contains(&file_name.as_str()) {
        return true;
    }

    Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .map(|ext| EDITABLE_TEXT_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false)
}

fn ensure_editable_text_path(path: &str) -> Result<(), String> {
    if is_supported_text_path(path) {
        return Ok(());
    }

    Err(
        "Unsupported file type: current editor only supports text, code, and config files"
            .to_string(),
    )
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return true;
    }

    let sample_len = bytes.len().min(4096);
    if sample_len == 0 {
        return false;
    }

    let suspicious = bytes[..sample_len]
        .iter()
        .filter(|byte| matches!(**byte, 0..=8 | 11 | 12 | 14..=31))
        .count();

    suspicious * 8 > sample_len
}

#[derive(Clone)]
pub struct SftpAppState {
    pub list_cache: Arc<Mutex<HashMap<String, Vec<FileEntry>>>>,
    dir_pagers: Arc<Mutex<HashMap<String, DirectoryPagerState>>>,
    pub ug_cache: Arc<Mutex<HashMap<String, (HashMap<u32, String>, HashMap<u32, String>)>>>,
    pub path_locks: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

pub struct SftpConnectionHandle {
    pub sftp: Arc<SftpSession>,
    pub session: Option<Arc<client::Handle<DummyHandler>>>,
    pub jump_session: Option<Arc<client::Handle<DummyHandler>>>,
    pub keepalive: Option<ssh::supervisor::KeepaliveTask>,
    pub reused_from_ssh: bool,
    pub connection_config: SshConfig,
    pub shared_ssh_session: Option<crate::ssh::SharedSshSessionSlot>,
}

#[allow(dead_code)]
struct TransferSftpSession {
    sftp: Arc<SftpSession>,
    owned: Option<Arc<client::Handle<DummyHandler>>>,
    jump_owned: Option<Arc<client::Handle<DummyHandler>>>,
    keepalive: Option<ssh::supervisor::KeepaliveTask>,
}

struct DirectoryPagerState {
    raw_sftp: Arc<RawSftpSession>,
    directory_handle: String,
    directories: Vec<FileEntry>,
    files: Vec<FileEntry>,
    exhausted: bool,
}

impl SftpAppState {
    pub fn new() -> Self {
        Self {
            list_cache: Arc::new(Mutex::new(HashMap::new())),
            dir_pagers: Arc::new(Mutex::new(HashMap::new())),
            ug_cache: Arc::new(Mutex::new(HashMap::new())),
            path_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub(crate) fn cleanup_session_state(state: &SftpAppState, session_id: &str) {
    let mut ug_cache = state.ug_cache.lock().unwrap();
    ug_cache.remove(session_id);
    drop(ug_cache);

    let mut path_locks = state.path_locks.lock().unwrap();
    path_locks.retain(|key, _| !key.starts_with(&format!("{}::", session_id)));
    drop(path_locks);

    let mut cache = state.list_cache.lock().unwrap();
    cache.retain(|key, _| !key.starts_with(&format!("{}::", session_id)));
    drop(cache);

    for pager in take_session_dir_pagers(state, session_id) {
        close_directory_pager(pager);
    }
}

#[allow(dead_code)]
#[derive(Clone, serde::Deserialize, Debug)]
pub struct SshConfig {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    private_key_path: Option<String>,
    passphrase: Option<String>,
    connect_timeout: Option<u64>,
    keep_alive_interval: Option<u64>,
    jump_host: Option<String>,
    jump_port: Option<u16>,
    jump_username: Option<String>,
    jump_auth_type: Option<String>,
    jump_password: Option<String>,
    jump_private_key_path: Option<String>,
    jump_passphrase: Option<String>,
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

pub struct DummyHandler {
    app_handle: AppHandle,
    session_id: String,
    host: String,
    port: u16,
    known_hosts_path: PathBuf,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
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

impl client::Handler for DummyHandler {
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
                "SFTP SSH algorithms negotiated kex={} host_key={} cipher={} client_mac={} server_mac={} client_compression={:?} server_compression={:?} strict_kex={}",
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
            Ok(true) => Ok(true),
            Ok(false) => {
                let (tx, rx) = oneshot::channel::<bool>();
                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    *pending = Some(tx);
                }

                let fingerprint = server_public_key
                    .fingerprint(HashAlg::Sha256)
                    .to_string();
                let algorithm = server_public_key.algorithm().to_string();

                let _ = self.app_handle.emit(
                    "sftp-hostkey-request",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "fingerprint": fingerprint,
                        "algorithm": algorithm,
                        "confirmCommand": "confirm_sftp_hostkey",
                    }),
                );

                let decision = tokio::time::timeout(Duration::from_secs(30), rx).await;
                let accepted = matches!(decision, Ok(Ok(true)));

                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    pending.take();
                }

                if accepted {
                    if let Err(error) = append_known_host(
                        &self.host,
                        self.port,
                        server_public_key,
                        &self.known_hosts_path,
                    ) {
                        let _ = self.app_handle.emit(
                            &format!("sftp-error-{}", self.session_id),
                            format!("Failed to save host key: {}", error),
                        );
                    }
                    Ok(true)
                } else {
                    let _ = self.app_handle.emit(
                        &format!("sftp-error-{}", self.session_id),
                        "Host key not trusted. Connection cancelled.".to_string(),
                    );
                    Ok(false)
                }
            }
            Err(error) => {
                // Key mismatch — prompt user with warning instead of rejecting
                let (tx, rx) = oneshot::channel::<bool>();
                {
                    let mut pending = self.pending_hostkey.lock().unwrap();
                    *pending = Some(tx);
                }

                let fingerprint = server_public_key
                    .fingerprint(HashAlg::Sha256)
                    .to_string();
                let algorithm = server_public_key.algorithm().to_string();

                let _ = self.app_handle.emit(
                    "sftp-hostkey-request",
                    serde_json::json!({
                        "sessionId": self.session_id,
                        "host": self.host,
                        "port": self.port,
                        "fingerprint": fingerprint,
                        "algorithm": algorithm,
                        "confirmCommand": "confirm_sftp_hostkey",
                        "warning": format!("主机密钥与已保存的不匹配！可能是代理/堡垒机或中间人攻击。\n原始错误: {}", error),
                    }),
                );

                let decision = tokio::time::timeout(Duration::from_secs(30), rx).await;
                let accepted = matches!(decision, Ok(Ok(true)));

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
                        &format!("sftp-error-{}", self.session_id),
                        "Host key not trusted. Connection cancelled.".to_string(),
                    );
                    Ok(false)
                }
            }
        }
    }
}

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

fn build_client_config(
    keep_alive_interval: Option<u64>,
    profile: ssh_algorithms::NegotiationProfile,
) -> Arc<client::Config> {
    let mut client_config = ssh_algorithms::build_client_config(keep_alive_interval, profile);
    client_config.window_size = 64 * 1024 * 1024;
    client_config.maximum_packet_size = 1024 * 1024;
    Arc::new(client_config)
}

async fn connect_handle<H, A>(
    config: Arc<client::Config>,
    addrs: A,
    handler: H,
    timeout_secs: Option<u64>,
) -> Result<client::Handle<H>, ssh_algorithms::ConnectAttemptError>
where
    H: client::Handler<Error = russh::Error> + Send + 'static,
    A: tokio::net::ToSocketAddrs,
{
    let connect_fut = client::connect(config, addrs, handler);
    if let Some(timeout) = timeout_secs {
        match tokio::time::timeout(Duration::from_secs(timeout), connect_fut).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(ssh_algorithms::ConnectAttemptError::Timeout),
        }
    } else {
        connect_fut.await.map_err(ssh_algorithms::ConnectAttemptError::from)
    }
}

async fn connect_stream_handle<H, R>(
    config: Arc<client::Config>,
    stream: R,
    handler: H,
    timeout_secs: Option<u64>,
) -> Result<client::Handle<H>, ssh_algorithms::ConnectAttemptError>
where
    H: client::Handler<Error = russh::Error> + Send + 'static,
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let connect_fut = client::connect_stream(config, stream, handler);
    if let Some(timeout) = timeout_secs {
        match tokio::time::timeout(Duration::from_secs(timeout), connect_fut).await {
            Ok(Ok(session)) => Ok(session),
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(ssh_algorithms::ConnectAttemptError::Timeout),
        }
    } else {
        connect_fut.await.map_err(ssh_algorithms::ConnectAttemptError::from)
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
    F: FnMut(Arc<client::Config>, ssh_algorithms::NegotiationProfile) -> Fut,
    Fut: Future<Output = Result<client::Handle<H>, ssh_algorithms::ConnectAttemptError>>,
{
    let mut profile = app_handle
        .state::<ssh::SshAppState>()
        .preferred_profile_for_endpoint(host, port);

    loop {
        let client_config = build_client_config(keep_alive_interval, profile);
        match attempt(client_config, profile).await {
            Ok(handle) => {
                app_handle
                    .state::<ssh::SshAppState>()
                    .remember_successful_profile(host, port, profile);
                return Ok(handle);
            }
            Err(error) if ssh_algorithms::should_retry_with_legacy(profile, &error) => {
                profile = ssh_algorithms::NegotiationProfile::LegacyRsaSha1;
            }
            Err(error) => return Err(format!("SFTP Connect Error: {}", error)),
        }
    }
}

pub async fn connect_sftp_legacy(
    app_handle: AppHandle,
    state: SftpAppState,
    shared_session_slot: Option<crate::ssh::SharedSshSessionSlot>,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
) -> Result<SftpConnectionHandle, String> {
    cleanup_session_state(&state, &session_id);

    if let Some(shared_session_slot) = shared_session_slot.as_ref() {
        if let Ok(sftp) = ssh::open_sftp_subsystem_for_session(shared_session_slot).await {
            return Ok(SftpConnectionHandle {
                sftp: Arc::new(sftp),
                session: None,
                jump_session: None,
                keepalive: None,
                reused_from_ssh: true,
                connection_config: config,
                shared_ssh_session: Some(shared_session_slot.clone()),
            });
        }
    }

    let known_hosts_path = app_known_hosts_path()?;

    let mut jump_session = if let Some(jump) = extract_jump_host(&config) {
        let jump_known_hosts_path = app_known_hosts_path()?;
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
                let jump_handler = DummyHandler {
                    app_handle: app_handle.clone(),
                    session_id: jump_session_id.clone(),
                    host: jump_host.clone(),
                    port: jump_port,
                    known_hosts_path: jump_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
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
        .await?;

        ssh::auth::authenticate_session(
            &session_id,
            &mut jump_handle,
            jump.username,
            jump.private_key_path,
            jump.password,
            jump.passphrase,
        )
        .await?;

        Some(jump_handle)
    } else {
        None
    };

    let mut session = if let Some(jump_handle) = jump_session.as_mut() {
        let target_host = config.host.clone();
        let target_port = config.port;
        let target_known_hosts_path = known_hosts_path.clone();
        let mut profile = app_handle
            .state::<ssh::SshAppState>()
            .preferred_profile_for_endpoint(&target_host, target_port);

        loop {
            let handler = DummyHandler {
                app_handle: app_handle.clone(),
                session_id: session_id.clone(),
                host: target_host.clone(),
                port: target_port,
                known_hosts_path: target_known_hosts_path.clone(),
                pending_hostkey: pending_hostkey.clone(),
            };
            let stream = jump_handle
                .channel_open_direct_tcpip(target_host.clone(), target_port as u32, "127.0.0.1", 0)
                .await
                .map_err(|error| format!("SFTP jump tunnel open failed: {}", error))?
                .into_stream();

            let client_config = build_client_config(config.keep_alive_interval, profile);
            match connect_stream_handle(client_config, stream, handler, config.connect_timeout).await
            {
                Ok(session) => {
                    app_handle
                        .state::<ssh::SshAppState>()
                        .remember_successful_profile(&target_host, target_port, profile);
                    break session;
                }
                Err(error) if ssh_algorithms::should_retry_with_legacy(profile, &error) => {
                    profile = ssh_algorithms::NegotiationProfile::LegacyRsaSha1;
                }
                Err(error) => return Err(format!("SFTP Connect Error: {}", error)),
            }
        }
    } else {
        let target_host = config.host.clone();
        let target_port = config.port;
        let target_known_hosts_path = known_hosts_path.clone();

        connect_with_profile_retry(
            &app_handle,
            &target_host,
            target_port,
            config.keep_alive_interval,
            |client_config, _profile| {
                let connect_host = target_host.clone();
                let handler = DummyHandler {
                    app_handle: app_handle.clone(),
                    session_id: session_id.clone(),
                    host: target_host.clone(),
                    port: target_port,
                    known_hosts_path: target_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
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

    ssh::auth::authenticate_session(
        &session_id,
        &mut session,
        config.username.clone(),
        config.private_key_path.clone(),
        config.password.clone(),
        config.passphrase.clone(),
    )
    .await?;

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open SSH channel: {}", e))?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to request sftp subsystem: {}", e))?;

    let sftp = SftpSession::new_with_config(channel.into_stream(), sftp_client_config())
        .await
        .map_err(|e| format!("Failed to init SFTP session: {}", e))?;
    let session = Arc::new(session);
    let jump_session = jump_session.map(Arc::new);
    let keepalive = ssh::supervisor::spawn_keepalive_task(
        session_id,
        ssh_algorithms::effective_keepalive_interval(config.keep_alive_interval),
        session.clone(),
        jump_session.clone(),
    );

    Ok(SftpConnectionHandle {
        sftp: Arc::new(sftp),
        session: Some(session),
        jump_session,
        keepalive,
        reused_from_ssh: false,
        connection_config: config,
        shared_ssh_session: None,
    })
}

pub async fn connect_sftp_runtime(
    app_handle: AppHandle,
    _state: SftpAppState,
    shared_session_slot: Option<crate::ssh::SharedSshSessionSlot>,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: String,
    config: SshConfig,
) -> Result<crate::session::state::ManagedSftpRuntime, String> {
    let runtime_state = SftpAppState::new();
    let handle = connect_sftp_legacy(
        app_handle,
        runtime_state.clone(),
        shared_session_slot,
        pending_hostkey,
        session_id,
        config,
    )
    .await?;

    Ok(crate::session::state::ManagedSftpRuntime {
        handle,
        state: runtime_state,
    })
}

pub async fn sftp_ls_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    sftp_ls_legacy(state, &runtime.handle, session_id, path).await
}

pub async fn sftp_ls_paged_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<SftpLsPagedResult, String> {
    sftp_ls_paged_legacy(state, &runtime.handle, session_id, path, offset, limit).await
}

pub async fn sftp_read_file_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<String, String> {
    sftp_read_file_legacy(state, &runtime.handle, session_id, path).await
}

pub async fn sftp_open_text_file_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<SftpOpenTextFileResult, String> {
    sftp_open_text_file_legacy(state, &runtime.handle, session_id, path).await
}

pub async fn sftp_write_file_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
    content: String,
    expected_modified: Option<u64>,
    expected_size: Option<u64>,
) -> Result<(), String> {
    sftp_write_file_legacy(
        state,
        &runtime.handle,
        session_id,
        path,
        content,
        expected_modified,
        expected_size,
    )
    .await
}

pub async fn sftp_save_text_file_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
    content: String,
    expected_cas_token: String,
) -> Result<SftpSaveTextFileResult, String> {
    sftp_save_text_file_legacy(
        state,
        &runtime.handle,
        session_id,
        path,
        content,
        expected_cas_token,
    )
    .await
}

pub async fn sftp_download_file_runtime(
    window: Window,
    state: &SftpAppState,
    sftp: Arc<SftpSession>,
    reused_from_ssh: bool,
    connection_config: SshConfig,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    cancel: Arc<AtomicBool>,
    session_id: String,
    remote_path: String,
    local_path: String,
    req_id: String,
) -> Result<(), String> {
    sftp_download_file_legacy(
        window,
        state,
        sftp,
        reused_from_ssh,
        connection_config,
        pending_hostkey,
        cancel,
        session_id,
        remote_path,
        local_path,
        req_id,
    )
    .await
}

pub async fn sftp_upload_file_runtime(
    window: Window,
    state: &SftpAppState,
    sftp: Arc<SftpSession>,
    reused_from_ssh: bool,
    connection_config: SshConfig,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    cancel: Arc<AtomicBool>,
    session_id: String,
    local_path: String,
    remote_path: String,
    req_id: String,
) -> Result<(), String> {
    sftp_upload_file_legacy(
        window,
        state,
        sftp,
        reused_from_ssh,
        connection_config,
        pending_hostkey,
        cancel,
        session_id,
        local_path,
        remote_path,
        req_id,
    )
    .await
}

pub async fn disconnect_sftp_runtime(
    runtime: Option<crate::session::state::ManagedSftpRuntime>,
    session_id: String,
) -> Result<(), String> {
    if let Some(runtime) = runtime {
        sftp_disconnect_legacy(&runtime.state, Some(runtime.handle), session_id).await
    } else {
        Ok(())
    }
}

pub async fn sftp_exists_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<bool, String> {
    sftp_exists_legacy(state, &runtime.handle, session_id, path).await
}

pub async fn sftp_mkdir_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<(), String> {
    sftp_mkdir_legacy(state, &runtime.handle, session_id, path).await
}

pub async fn sftp_rename_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    sftp_rename_legacy(state, &runtime.handle, session_id, from_path, to_path).await
}

pub async fn sftp_remove_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    sftp_remove_legacy(state, &runtime.handle, session_id, path, is_dir).await
}

pub async fn sftp_chmod_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
    permissions: u32,
) -> Result<(), String> {
    sftp_chmod_legacy(state, &runtime.handle, session_id, path, permissions).await
}

pub async fn sftp_stat_runtime(
    state: &SftpAppState,
    runtime: &crate::session::state::ManagedSftpRuntime,
    session_id: String,
    path: String,
) -> Result<FileEntry, String> {
    sftp_stat_legacy(state, &runtime.handle, session_id, path).await
}

#[tauri::command]
pub async fn connect_sftp(
    app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    state: tauri::State<'_, SftpAppState>,
    _ssh_state: tauri::State<'_, crate::ssh::SshAppState>,
    session_id: String,
    config: SshConfig,
) -> Result<(), String> {
    supervisor
        .connect_sftp(app_handle, state.inner().clone(), session_id, config)
        .await
}

#[derive(Clone, serde::Serialize)]
pub struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: u64,
    permissions: u32,
    owner: Option<String>,
    group: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SftpLsPagedResult {
    items: Vec<FileEntry>,
    offset: usize,
    limit: usize,
    next_offset: usize,
    has_more: bool,
    total: usize,
    total_known: bool,
}

#[derive(serde::Serialize)]
pub struct SftpOpenTextFileResult {
    content: String,
    file: FileEntry,
    cas_token: String,
}

#[derive(serde::Serialize)]
pub struct SftpSaveTextFileResult {
    file: FileEntry,
    cas_token: String,
}

fn get_sftp_session(handle: &SftpConnectionHandle) -> Arc<SftpSession> {
    handle.sftp.clone()
}

async fn open_dedicated_sftp_session(
    app_handle: &AppHandle,
    _state: &SftpAppState,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: &str,
    req_id: &str,
    config: SshConfig,
) -> Result<TransferSftpSession, String> {
    let known_hosts_path = app_known_hosts_path()?;
    let transfer_prompt_id = format!("{}::transfer::{}", session_id, req_id);

    let mut jump_session = if let Some(jump) = extract_jump_host(&config) {
        let jump_known_hosts_path = app_known_hosts_path()?;
        let jump_session_id = format!("{}::jump::{}", session_id, req_id);
        let jump_host = jump.host.clone();
        let jump_port = jump.port;

        let mut jump_handle = connect_with_profile_retry(
            app_handle,
            &jump_host,
            jump_port,
            config.keep_alive_interval,
            |client_config, _profile| {
                let connect_host = jump_host.clone();
                let jump_handler = DummyHandler {
                    app_handle: app_handle.clone(),
                    session_id: jump_session_id.clone(),
                    host: jump_host.clone(),
                    port: jump_port,
                    known_hosts_path: jump_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
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
        .await?;

        ssh::auth::authenticate_session(
            &transfer_prompt_id,
            &mut jump_handle,
            jump.username,
            jump.private_key_path,
            jump.password,
            jump.passphrase,
        )
        .await?;

        Some(jump_handle)
    } else {
        None
    };

    let mut session = if let Some(jump_handle) = jump_session.as_mut() {
        let target_host = config.host.clone();
        let target_port = config.port;
        let target_known_hosts_path = known_hosts_path.clone();
        let mut profile = app_handle
            .state::<ssh::SshAppState>()
            .preferred_profile_for_endpoint(&target_host, target_port);

        loop {
            let handler = DummyHandler {
                app_handle: app_handle.clone(),
                session_id: transfer_prompt_id.clone(),
                host: target_host.clone(),
                port: target_port,
                known_hosts_path: target_known_hosts_path.clone(),
                pending_hostkey: pending_hostkey.clone(),
            };
            let stream = jump_handle
                .channel_open_direct_tcpip(target_host.clone(), target_port as u32, "127.0.0.1", 0)
                .await
                .map_err(|error| format!("SFTP jump tunnel open failed: {}", error))?
                .into_stream();

            let client_config = build_client_config(config.keep_alive_interval, profile);
            match connect_stream_handle(client_config, stream, handler, config.connect_timeout).await
            {
                Ok(session) => {
                    app_handle
                        .state::<ssh::SshAppState>()
                        .remember_successful_profile(&target_host, target_port, profile);
                    break session;
                }
                Err(error) if ssh_algorithms::should_retry_with_legacy(profile, &error) => {
                    profile = ssh_algorithms::NegotiationProfile::LegacyRsaSha1;
                }
                Err(error) => return Err(format!("SFTP Connect Error: {}", error)),
            }
        }
    } else {
        let target_host = config.host.clone();
        let target_port = config.port;
        let target_known_hosts_path = known_hosts_path.clone();

        connect_with_profile_retry(
            app_handle,
            &target_host,
            target_port,
            config.keep_alive_interval,
            |client_config, _profile| {
                let connect_host = target_host.clone();
                let handler = DummyHandler {
                    app_handle: app_handle.clone(),
                    session_id: transfer_prompt_id.clone(),
                    host: target_host.clone(),
                    port: target_port,
                    known_hosts_path: target_known_hosts_path.clone(),
                    pending_hostkey: pending_hostkey.clone(),
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

    ssh::auth::authenticate_session(
        &transfer_prompt_id,
        &mut session,
        config.username.clone(),
        config.private_key_path.clone(),
        config.password.clone(),
        config.passphrase.clone(),
    )
    .await?;

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open SSH channel: {}", e))?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to request sftp subsystem: {}", e))?;

    let sftp = SftpSession::new_with_config(channel.into_stream(), sftp_client_config())
        .await
        .map_err(|e| format!("Failed to init SFTP session: {}", e))?;
    let session = Arc::new(session);
    let jump_session = jump_session.map(Arc::new);
    let keepalive = ssh::supervisor::spawn_keepalive_task(
        transfer_prompt_id,
        ssh_algorithms::effective_keepalive_interval(config.keep_alive_interval),
        session.clone(),
        jump_session.clone(),
    );

    Ok(TransferSftpSession {
        sftp: Arc::new(sftp),
        owned: Some(session),
        jump_owned: jump_session,
        keepalive,
    })
}

async fn open_transfer_sftp_session(
    state: &SftpAppState,
    app_handle: &AppHandle,
    sftp: Arc<SftpSession>,
    reused_from_ssh: bool,
    connection_config: SshConfig,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    session_id: &str,
    req_id: &str,
) -> Result<TransferSftpSession, String> {
    if reused_from_ssh {
        return open_dedicated_sftp_session(
            app_handle,
            state,
            pending_hostkey,
            session_id,
            req_id,
            connection_config,
        )
        .await;
    }

    Ok(TransferSftpSession {
        sftp,
        owned: None,
        jump_owned: None,
        keepalive: None,
    })
}

async fn open_raw_sftp_subsystem_for_client_session(
    session: &client::Handle<DummyHandler>,
) -> Result<RawSftpSession, String> {
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open shared SSH channel: {}", e))?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to request shared sftp subsystem: {}", e))?;

    let raw = RawSftpSession::new_with_config(channel.into_stream(), sftp_client_config());
    raw.init()
        .await
        .map_err(|e| format!("Failed to init shared raw SFTP session: {}", e))?;
    Ok(raw)
}

async fn open_raw_sftp_subsystem_for_shared_session(
    shared_session_slot: &crate::ssh::SharedSshSessionSlot,
) -> Result<RawSftpSession, String> {
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

    let raw = RawSftpSession::new_with_config(channel.into_stream(), sftp_client_config());
    raw.init()
        .await
        .map_err(|e| format!("Failed to init shared raw SFTP session: {}", e))?;
    Ok(raw)
}

async fn open_paged_directory_session(
    handle: &SftpConnectionHandle,
) -> Result<Arc<RawSftpSession>, String> {
    if let Some(shared_session_slot) = handle.shared_ssh_session.as_ref() {
        return open_raw_sftp_subsystem_for_shared_session(shared_session_slot)
            .await
            .map(Arc::new);
    }

    if let Some(session) = handle.session.as_ref() {
        return open_raw_sftp_subsystem_for_client_session(session)
            .await
            .map(Arc::new);
    }

    Err("SFTP connection cannot open a paged directory channel".to_string())
}

fn cache_key(session_id: &str, path: &str) -> String {
    format!("{}::{}", session_id, path)
}

fn close_directory_pager(pager: DirectoryPagerState) {
    let _ = pager.raw_sftp.close_session();
}

fn take_session_dir_pagers(state: &SftpAppState, session_id: &str) -> Vec<DirectoryPagerState> {
    let prefix = format!("{}::", session_id);
    let mut pagers = state.dir_pagers.lock().unwrap();
    let keys = pagers
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();

    keys.into_iter()
        .filter_map(|key| pagers.remove(&key))
        .collect::<Vec<_>>()
}

fn take_dir_pager(state: &SftpAppState, pager_key: &str) -> Option<DirectoryPagerState> {
    state.dir_pagers.lock().unwrap().remove(pager_key)
}

fn invalidate_session_cache(state: &SftpAppState, session_id: &str) {
    let mut cache = state.list_cache.lock().unwrap();
    cache.retain(|key, _| !key.starts_with(&format!("{}::", session_id)));
    drop(cache);

    for pager in take_session_dir_pagers(state, session_id) {
        close_directory_pager(pager);
    }
}

fn append_known_host(
    host: &str,
    port: u16,
    key: &PublicKey,
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

fn normalize_lock_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path.to_string()
    } else {
        trimmed.to_string()
    }
}

fn mutation_lock_key(session_id: &str, path: &str) -> String {
    format!("{}::{}", session_id, normalize_lock_path(path))
}

async fn acquire_path_lock(
    state: &SftpAppState,
    session_id: &str,
    path: &str,
) -> OwnedMutexGuard<()> {
    let key = mutation_lock_key(session_id, path);
    let lock = {
        let mut locks = state.path_locks.lock().unwrap();
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    lock.lock_owned().await
}

async fn acquire_path_locks(
    state: &SftpAppState,
    session_id: &str,
    paths: &[&str],
) -> Vec<OwnedMutexGuard<()>> {
    let mut keys = paths
        .iter()
        .map(|path| mutation_lock_key(session_id, path))
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();

    let locks = {
        let mut map = state.path_locks.lock().unwrap();
        keys.into_iter()
            .map(|key| {
                map.entry(key)
                    .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                    .clone()
            })
            .collect::<Vec<_>>()
    };

    let mut guards = Vec::with_capacity(locks.len());
    for lock in locks {
        guards.push(lock.lock_owned().await);
    }
    guards
}

async fn read_remote_size_with_retry(
    sftp: &SftpSession,
    path: &str,
    retries: usize,
    delay_ms: u64,
) -> Result<u64, String> {
    for _ in 0..retries {
        if let Ok(stat) = sftp.metadata(path).await {
            return Ok(stat.size.unwrap_or(0));
        }
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    Err(format!("Stat Error: {}", path))
}

#[cfg(unix)]
fn local_permissions_from_metadata(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn local_permissions_from_metadata(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

async fn read_remote_text(sftp: &SftpSession, path: &str) -> Option<String> {
    let mut file = sftp.open(path).await.ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.ok()?;
    Some(contents)
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn ensure_inline_editor_file_size(path: &str, size: u64) -> Result<(), String> {
    if size <= MAX_INLINE_EDITOR_FILE_BYTES {
        return Ok(());
    }

    Err(format!(
        "Inline editor limit exceeded: {} is too large ({} bytes). Please download it or use an external editor.",
        path, size
    ))
}

fn build_text_cas_token(text: &str) -> String {
    let normalized = normalize_newlines(text);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    format!("sha256:{}", hex)
}

async fn read_remote_editable_text(sftp: &SftpSession, path: &str) -> Result<String, String> {
    let mut file = sftp
        .open(path)
        .await
        .map_err(|e| format!("Open Error: {}", e))?;

    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .await
        .map_err(|e| format!("Read Error: {}", e))?;

    if is_probably_binary(&contents) {
        return Err(
            "Read Error: file contains binary content and cannot be edited here".to_string(),
        );
    }

    String::from_utf8(contents).map_err(|_| {
        "Read Error: file is not valid UTF-8 text and cannot be edited here".to_string()
    })
}

fn file_entry_from_attrs(
    file_name: String,
    attrs: &FileAttributes,
    passwd_map: &HashMap<u32, String>,
    group_map: &HashMap<u32, String>,
) -> FileEntry {
    FileEntry {
        name: file_name,
        is_dir: attrs.is_dir(),
        size: attrs.size.unwrap_or(0),
        modified: attrs.mtime.unwrap_or(0) as u64,
        permissions: attrs.permissions.unwrap_or(0),
        owner: attrs.uid.and_then(|uid| passwd_map.get(&uid).cloned()),
        group: attrs.gid.and_then(|gid| group_map.get(&gid).cloned()),
    }
}

fn sort_file_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
}

fn combine_paged_entries(pager: &DirectoryPagerState) -> Vec<FileEntry> {
    let mut combined = pager.directories.clone();
    combined.extend(pager.files.iter().cloned());
    combined
}

async fn read_next_directory_batch(
    state: &SftpAppState,
    session_id: &str,
    sftp: &SftpSession,
    raw_sftp: &RawSftpSession,
    directory_handle: &str,
) -> Result<(Vec<FileEntry>, bool), String> {
    let (passwd_map, group_map) = get_ug_maps(state, session_id, sftp).await;

    match raw_sftp.readdir(directory_handle).await {
        Ok(name) => {
            let items = name
                .files
                .into_iter()
                .filter(|entry| entry.filename != "." && entry.filename != "..")
                .map(|entry| {
                    file_entry_from_attrs(entry.filename, &entry.attrs, &passwd_map, &group_map)
                })
                .collect::<Vec<_>>();
            Ok((items, false))
        }
        Err(SftpClientError::Status(status)) if status.status_code == StatusCode::Eof => {
            Ok((Vec::new(), true))
        }
        Err(error) => Err(format!("LS Error: {}", error)),
    }
}

async fn verify_remote_text_with_retry(
    sftp: &SftpSession,
    path: &str,
    expected: String,
    retries: usize,
    delay_ms: u64,
) -> Result<(), String> {
    let expected_normalized = normalize_newlines(&expected);
    let mut last_observed = String::new();

    for _ in 0..retries {
        match sftp.open(path).await {
            Ok(mut file) => {
                let mut observed = String::new();
                match file.read_to_string(&mut observed).await {
                    Ok(_) => {
                        last_observed = observed.clone();
                        if observed == expected
                            || normalize_newlines(&observed) == expected_normalized
                        {
                            return Ok(());
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    Err(format!(
        "Write verification failed after retry: observed length {}, expected length {}",
        last_observed.len(),
        expected.len()
    ))
}

async fn load_passwd_map(sftp: &SftpSession) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let contents = if let Some(text) = read_remote_text(sftp, "/etc/passwd").await {
        text
    } else {
        return map;
    };
    for line in contents.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        if let Ok(uid) = parts[2].parse::<u32>() {
            map.insert(uid, parts[0].to_string());
        }
    }
    map
}

async fn load_group_map(sftp: &SftpSession) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let contents = if let Some(text) = read_remote_text(sftp, "/etc/group").await {
        text
    } else {
        return map;
    };
    for line in contents.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        if let Ok(gid) = parts[2].parse::<u32>() {
            map.insert(gid, parts[0].to_string());
        }
    }
    map
}

async fn get_ug_maps(
    state: &SftpAppState,
    session_id: &str,
    sftp: &SftpSession,
) -> (HashMap<u32, String>, HashMap<u32, String>) {
    let cached = {
        let cache = state.ug_cache.lock().unwrap();
        cache.get(session_id).cloned()
    };
    if let Some(c) = cached {
        return c;
    }
    let p_map = load_passwd_map(sftp).await;
    let g_map = load_group_map(sftp).await;
    let mut cache = state.ug_cache.lock().unwrap();
    cache.insert(session_id.to_string(), (p_map.clone(), g_map.clone()));
    (p_map, g_map)
}

pub async fn sftp_ls_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let sftp = get_sftp_session(handle);

    let entries = sftp
        .read_dir(&path)
        .await
        .map_err(|e| format!("LS Error: {}", e))?;

    let (passwd_map, group_map) = get_ug_maps(&state, &session_id, &sftp).await;

    let mut result = Vec::new();
    for entry in entries {
        let file_name = entry.file_name().to_string();
        if file_name == "." || file_name == ".." {
            continue;
        }

        result.push(file_entry_from_attrs(
            file_name,
            &entry.metadata(),
            &passwd_map,
            &group_map,
        ));
    }

    result.sort_by(|a, b| {
        if a.is_dir == b.is_dir {
            a.name.cmp(&b.name)
        } else {
            b.is_dir.cmp(&a.is_dir)
        }
    });

    // 写入缓存并限制每个 session 的目录缓存条目数，防止内存泄漏
    {
        let mut cache = state.list_cache.lock().unwrap();
        cache.insert(cache_key(&session_id, &path), result.clone());

        let prefix = format!("{}::", &session_id);
        let session_keys: Vec<String> = cache
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        if session_keys.len() > MAX_LIST_CACHE_PER_SESSION {
            let remove_count = session_keys.len() - MAX_LIST_CACHE_PER_SESSION;
            for key in session_keys.iter().take(remove_count) {
                cache.remove(key);
            }
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn sftp_ls(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    supervisor
        .list_sftp_dir(session_id, path)
        .await
}

pub async fn sftp_ls_paged_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<SftpLsPagedResult, String> {
    let safe_offset = offset.unwrap_or(0);
    let safe_limit = limit.unwrap_or(200).max(1).min(1000);
    let pager_key = cache_key(&session_id, &path);
    let target_end = safe_offset.saturating_add(safe_limit);

    if safe_offset == 0 {
        for pager in take_session_dir_pagers(state, &session_id) {
            close_directory_pager(pager);
        }
    }

    loop {
        let current = {
            let pagers = state.dir_pagers.lock().unwrap();
            pagers.get(&pager_key).map(|pager| {
                (
                    combine_paged_entries(pager),
                    pager.exhausted,
                    pager.raw_sftp.clone(),
                    pager.directory_handle.clone(),
                )
            })
        };

        if let Some((buffered, exhausted, raw_sftp, directory_handle)) = current {
            if exhausted || buffered.len() >= target_end {
                let total = buffered.len();
                let start = safe_offset.min(total);
                let end = (start + safe_limit).min(total);
                if exhausted {
                    if let Some(pager) = take_dir_pager(state, &pager_key) {
                        close_directory_pager(pager);
                    }
                }
                return Ok(SftpLsPagedResult {
                    items: buffered[start..end].to_vec(),
                    offset: start,
                    limit: safe_limit,
                    next_offset: end,
                    has_more: !exhausted || end < total,
                    total,
                    total_known: exhausted,
                });
            }

            let sftp = get_sftp_session(handle);
            let (next_items, exhausted) = read_next_directory_batch(
                state,
                &session_id,
                &sftp,
                raw_sftp.as_ref(),
                &directory_handle,
            )
            .await?;

            let mut pagers = state.dir_pagers.lock().unwrap();
            let pager = pagers
                .get_mut(&pager_key)
                .ok_or_else(|| "Directory pager lost during pagination".to_string())?;
            let (mut directories, mut files): (Vec<_>, Vec<_>) =
                next_items.into_iter().partition(|entry| entry.is_dir);
            pager.directories.extend(directories.drain(..));
            pager.files.extend(files.drain(..));
            sort_file_entries(&mut pager.directories);
            sort_file_entries(&mut pager.files);
            pager.exhausted = exhausted;
            continue;
        }

        let raw_sftp = open_paged_directory_session(handle).await?;
        let directory_handle = raw_sftp
            .opendir(&path)
            .await
            .map_err(|e| format!("LS Error: {}", e))?
            .handle;

        state.dir_pagers.lock().unwrap().insert(
            pager_key.clone(),
            DirectoryPagerState {
                raw_sftp,
                directory_handle,
                directories: Vec::new(),
                files: Vec::new(),
                exhausted: false,
            },
        );
    }
}

#[tauri::command]
pub async fn sftp_ls_paged(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<SftpLsPagedResult, String> {
    supervisor
        .list_sftp_dir_paged(session_id, path, offset, limit)
        .await
}

pub async fn sftp_read_file_legacy(
    _state: &SftpAppState,
    handle: &SftpConnectionHandle,
    _session_id: String,
    path: String,
) -> Result<String, String> {
    ensure_editable_text_path(&path)?;
    let sftp = get_sftp_session(handle);
    read_remote_editable_text(&sftp, &path).await
}

#[tauri::command]
pub async fn sftp_read_file(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<String, String> {
    supervisor
        .read_sftp_file(session_id, path)
        .await
}

pub async fn sftp_open_text_file_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
) -> Result<SftpOpenTextFileResult, String> {
    ensure_editable_text_path(&path)?;
    let sftp = get_sftp_session(handle);
    let attrs = sftp
        .metadata(&path)
        .await
        .map_err(|e| format!("Stat Error: {}", e))?;
    ensure_inline_editor_file_size(&path, attrs.size.unwrap_or(0))?;
    let content = read_remote_editable_text(&sftp, &path).await?;
    let (passwd_map, group_map) = get_ug_maps(state, &session_id, &sftp).await;
    let file_name = Path::new(&path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&path)
        .to_string();

    Ok(SftpOpenTextFileResult {
        cas_token: build_text_cas_token(&content),
        content,
        file: file_entry_from_attrs(file_name, &attrs, &passwd_map, &group_map),
    })
}

#[tauri::command]
pub async fn sftp_open_text_file(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<SftpOpenTextFileResult, String> {
    supervisor
        .open_sftp_text_file(session_id, path)
        .await
}

async fn write_text_file_internal(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    content: String,
    expected_modified: Option<u64>,
    expected_size: Option<u64>,
    expected_cas_token: Option<&str>,
) -> Result<(), String> {
    ensure_editable_text_path(&path)?;
    let sftp = get_sftp_session(handle);

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Invalid filename or path".into());
    }

    let _path_guard = acquire_path_lock(state, &session_id, trimmed).await;

    let (parent_dir, base_name) = if let Some((dir, name)) = trimmed.rsplit_once('/') {
        if !dir.is_empty() {
            (dir.to_string(), name.to_string())
        } else {
            ("/".to_string(), name.to_string())
        }
    } else {
        ("".to_string(), trimmed.to_string())
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_name = format!(".{}.tmp.{}.{}", base_name, std::process::id(), ts);
    let temp_path = if parent_dir == "/" {
        format!("/{}", temp_name)
    } else if parent_dir.is_empty() {
        temp_name
    } else {
        format!("{}/{}", parent_dir, temp_name)
    };

    let original_meta = sftp.metadata(&path).await.ok();
    let original_exists = original_meta.is_some();
    let original_permissions = original_meta.as_ref().and_then(|m| m.permissions);

    if let Some(expected_token) = expected_cas_token {
        if !original_exists {
            return Err("CAS mismatch: remote file no longer exists".to_string());
        }

        let current_text = read_remote_editable_text(&sftp, &path).await?;
        let current_token = build_text_cas_token(&current_text);
        if current_token != expected_token {
            return Err(
                "CAS mismatch: remote file changed; please reopen before saving".to_string(),
            );
        }
    }

    if let (Some(meta), Some(expected)) = (original_meta.as_ref(), expected_modified) {
        let actual_modified = u64::from(meta.mtime.unwrap_or(0));
        if actual_modified != expected {
            return Err(format!(
                "CAS mismatch: remote file changed (mtime {} != expected {})",
                actual_modified, expected
            ));
        }
    }

    if let (Some(meta), Some(expected)) = (original_meta.as_ref(), expected_size) {
        let actual_size = meta.size.unwrap_or(0);
        if actual_size != expected {
            return Err(format!(
                "CAS mismatch: remote file changed (size {} != expected {})",
                actual_size, expected
            ));
        }
    }

    if !original_exists && (expected_modified.is_some() || expected_size.is_some()) {
        return Err("CAS mismatch: remote file no longer exists".to_string());
    }

    let backup_name = format!(".{}.bak.{}.{}", base_name, std::process::id(), ts);
    let backup_path = if parent_dir == "/" {
        format!("/{}", backup_name)
    } else if parent_dir.is_empty() {
        backup_name
    } else {
        format!("{}/{}", parent_dir, backup_name)
    };

    {
        let mut file = sftp
            .create(&temp_path)
            .await
            .map_err(|e| format!("Create/Open Error: {}", e))?;

        file.write_all(content.as_bytes())
            .await
            .map_err(|e| format!("Write Error: {}", e))?;

        file.flush()
            .await
            .map_err(|e| format!("Flush Error: {}", e))?;

        file.shutdown()
            .await
            .map_err(|e| format!("Shutdown Error: {}", e))?;
    }

    let expected_content = content.clone();
    verify_remote_text_with_retry(&sftp, temp_path.as_str(), expected_content.clone(), 5, 80)
        .await
        .map_err(|e| format!("Temp verify Error: {}", e))?;

    if original_exists {
        if let Err(e) = sftp.rename(&path, &backup_path).await {
            let _ = sftp.remove_file(&temp_path).await;
            return Err(format!("Backup Error: {}", e));
        }
    }

    if let Err(e) = sftp.rename(&temp_path, &path).await {
        if original_exists {
            let _ = sftp.rename(&backup_path, &path).await;
        }
        let _ = sftp.remove_file(&temp_path).await;
        return Err(format!("Promote Error: {}", e));
    }

    if let Err(e) = verify_remote_text_with_retry(&sftp, &path, expected_content, 6, 100).await {
        if original_exists {
            let _ = sftp.remove_file(&path).await;
            let _ = sftp.rename(&backup_path, &path).await;
        }
        return Err(format!("Final verify Error: {}", e));
    }

    if let Some(perms) = original_permissions {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(perms);
        let _ = sftp.set_metadata(&path, attrs).await;
    }

    if original_exists {
        let _ = sftp.remove_file(&backup_path).await;
    }

    invalidate_session_cache(state, &session_id);
    Ok(())
}

pub async fn sftp_write_file_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    content: String,
    expected_modified: Option<u64>,
    expected_size: Option<u64>,
) -> Result<(), String> {
    write_text_file_internal(
        state,
        handle,
        session_id,
        path,
        content,
        expected_modified,
        expected_size,
        None,
    )
    .await
}

#[tauri::command]
pub async fn sftp_write_file(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
    content: String,
    expected_modified: Option<u64>,
    expected_size: Option<u64>,
) -> Result<(), String> {
    supervisor
        .write_sftp_file(
            session_id,
            path,
            content,
            expected_modified,
            expected_size,
        )
        .await
}

pub async fn sftp_save_text_file_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    content: String,
    expected_cas_token: String,
) -> Result<SftpSaveTextFileResult, String> {
    write_text_file_internal(
        state,
        handle,
        session_id.clone(),
        path.clone(),
        content.clone(),
        None,
        None,
        Some(expected_cas_token.as_str()),
    )
    .await?;

    let sftp = get_sftp_session(handle);
    let attrs = sftp
        .metadata(&path)
        .await
        .map_err(|e| format!("Stat Error: {}", e))?;
    let (passwd_map, group_map) = get_ug_maps(state, &session_id, &sftp).await;
    let file_name = Path::new(&path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&path)
        .to_string();

    Ok(SftpSaveTextFileResult {
        cas_token: build_text_cas_token(&content),
        file: file_entry_from_attrs(file_name, &attrs, &passwd_map, &group_map),
    })
}

#[tauri::command]
pub async fn sftp_save_text_file(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
    content: String,
    expected_cas_token: String,
) -> Result<SftpSaveTextFileResult, String> {
    supervisor
        .save_sftp_text_file(
            session_id,
            path,
            content,
            expected_cas_token,
        )
        .await
}

fn emit_transfer_cancelled(
    window: &Window,
    session_id: &str,
    req_id: &str,
    direction: &str,
    current: u64,
    total: u64,
) {
    let _ = window.emit(
        "sftp-progress",
        ProgressPayload {
            session_id: session_id.to_string(),
            id: req_id.to_string(),
            direction: direction.to_string(),
            current,
            total,
            percent: if total > 0 {
                ((current as f64 / total as f64) * 100.0) as u8
            } else {
                0
            },
            status: "cancelled".to_string(),
            error: Some("Transfer cancelled".to_string()),
        },
    );
}

fn ensure_transfer_active(
    window: &Window,
    cancel: &Arc<AtomicBool>,
    session_id: &str,
    req_id: &str,
    direction: &str,
    current: u64,
    total: u64,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        emit_transfer_cancelled(
            window,
            session_id,
            req_id,
            direction,
            current,
            total,
        );
        Err("Transfer cancelled".to_string())
    } else {
        Ok(())
    }
}

pub async fn sftp_download_file_legacy(
    window: Window,
    state: &SftpAppState,
    sftp: Arc<SftpSession>,
    reused_from_ssh: bool,
    connection_config: SshConfig,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    cancel: Arc<AtomicBool>,
    session_id: String,
    remote_path: String,
    local_path: String,
    req_id: String,
) -> Result<(), String> {
    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "download",
        0,
        0,
    )?;
    let transfer_session = open_transfer_sftp_session(
        state,
        &window.app_handle(),
        sftp,
        reused_from_ssh,
        connection_config,
        pending_hostkey,
        &session_id,
        &req_id,
    )
    .await?;
    let sftp = transfer_session.sftp.clone();

    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "download",
        0,
        0,
    )?;

    let total_size = sftp
        .metadata(&remote_path)
        .await
        .ok()
        .and_then(|meta| meta.size)
        .unwrap_or(0);

    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "download",
        0,
        total_size,
    )?;

    let emit_failed = |current: u64, total: u64, err: String| {
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.clone(),
                id: req_id.clone(),
                direction: "download".to_string(),
                current,
                total,
                percent: if total > 0 {
                    ((current as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err),
            },
        );
    };

    let remote = sftp.open(&remote_path).await.map_err(|e| {
        let err = format!("Open Error: {}", e);
        emit_failed(0, total_size, err.clone());
        err
    })?;
    let mut local = tokio::fs::File::create(&local_path).await.map_err(|e| {
        let err = format!("Create Error: {}", e);
        emit_failed(0, total_size, err.clone());
        err
    })?;

    let _ = window.emit(
        "sftp-progress",
        ProgressPayload {
            session_id: session_id.clone(),
            id: req_id.clone(),
            direction: "download".to_string(),
            current: 0,
            total: total_size,
            percent: 0,
            status: "uploading".to_string(),
            error: None,
        },
    );

    let result =
        pipelined_sftp_download(
            &window,
            &session_id,
            &req_id,
            total_size,
            remote,
            &mut local,
            &cancel,
        )
        .await;

    match result {
        Ok(downloaded) => {
            ensure_transfer_active(
                &window,
                &cancel,
                &session_id,
                &req_id,
                "download",
                downloaded,
                total_size,
            )?;
            use tokio::io::AsyncWriteExt;
            local.flush().await.map_err(|e| {
                let err = format!("Flush Error: {}", e);
                emit_failed(downloaded, total_size, err.clone());
                err
            })?;

            let _ = window.emit(
                "sftp-progress",
                ProgressPayload {
                    session_id: session_id.clone(),
                    id: req_id,
                    direction: "download".to_string(),
                    current: downloaded,
                    total: total_size,
                    percent: if total_size > 0 { 100 } else { 0 },
                    status: "success".to_string(),
                    error: None,
                },
            );
            Ok(())
        }
        Err(e) => {
            if cancel.load(Ordering::Relaxed) {
                let _ = window.emit(
                    "sftp-progress",
                    ProgressPayload {
                        session_id: session_id.clone(),
                        id: req_id,
                        direction: "download".to_string(),
                        current: 0,
                        total: total_size,
                        percent: 0,
                        status: "cancelled".to_string(),
                        error: Some("用户取消了下载".to_string()),
                    },
                );
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn sftp_download_file(
    window: Window,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    remote_path: String,
    local_path: String,
    req_id: String,
) -> Result<(), String> {
    supervisor
        .start_sftp_download(
            window,
            session_id,
            remote_path,
            local_path,
            req_id,
        )
        .await
}

async fn pipelined_sftp_download<RemoteReader: tokio::io::AsyncRead + Unpin + Send + 'static>(
    window: &Window,
    session_id: &str,
    req_id: &str,
    total_size: u64,
    mut remote: RemoteReader,
    local: &mut tokio::fs::File,
    cancel: &Arc<AtomicBool>,
) -> Result<u64, String> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    let (tx, mut rx) =
        tokio::sync::mpsc::channel::<Result<Vec<u8>, String>>(SFTP_TRANSFER_CHANNEL_SIZE);

    let read_cancel = cancel.clone();
    let read_session_id = session_id.to_string();
    let read_req_id = req_id.to_string();
    let read_window = window.clone();

    let reader = tokio::spawn(async move {
        let mut buffer = vec![0u8; SFTP_TRANSFER_BUFFER_SIZE];
        loop {
            if read_cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Err("cancelled".into())).await;
                return;
            }
            match remote.read(&mut buffer).await {
                Ok(0) => return,
                Ok(n) => {
                    let chunk = buffer[..n].to_vec();
                    if tx.send(Ok(chunk)).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let err = format!("Read Error: {}", e);
                    let _ = read_window.emit(
                        "sftp-progress",
                        ProgressPayload {
                            session_id: read_session_id.clone(),
                            id: read_req_id,
                            direction: "download".to_string(),
                            current: 0,
                            total: total_size,
                            percent: 0,
                            status: "failed".to_string(),
                            error: Some(err.clone()),
                        },
                    );
                    let _ = tx.send(Err(err)).await;
                    return;
                }
            }
        }
    });

    let emit_failed = |current: u64, total: u64, err: String| {
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.to_string(),
                id: req_id.to_string(),
                direction: "download".to_string(),
                current,
                total,
                percent: if total > 0 {
                    ((current as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err),
            },
        );
    };

    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut last_emit_bytes: u64 = 0;

    while let Some(chunk_result) = rx.recv().await {
        let chunk = chunk_result.map_err(|e| {
            if e == "cancelled" {
                "传输已取消".to_string()
            } else {
                e
            }
        })?;
        let n = chunk.len();
        local.write_all(&chunk).await.map_err(|e| {
            let err = format!("Write Error: {}", e);
            emit_failed(downloaded, total_size, err.clone());
            err
        })?;
        downloaded += u64::try_from(n).map_err(|_| "Read size conversion error".to_string())?;

        if should_emit_transfer_progress(&last_emit, downloaded.saturating_sub(last_emit_bytes)) {
            let _ = window.emit(
                "sftp-progress",
                ProgressPayload {
                    session_id: session_id.to_string(),
                    id: req_id.to_string(),
                    direction: "download".to_string(),
                    current: downloaded,
                    total: total_size,
                    percent: if total_size > 0 {
                        ((downloaded as f64 / total_size as f64) * 100.0) as u8
                    } else {
                        0
                    },
                    status: "uploading".to_string(),
                    error: None,
                },
            );
            last_emit = std::time::Instant::now();
            last_emit_bytes = downloaded;
        }
    }

    let _ = reader.await;
    Ok(downloaded)
}

pub async fn sftp_upload_file_legacy(
    window: Window,
    state: &SftpAppState,
    sftp: Arc<SftpSession>,
    reused_from_ssh: bool,
    connection_config: SshConfig,
    pending_hostkey: crate::session::state::SharedHostkeyDecision,
    cancel: Arc<AtomicBool>,
    session_id: String,
    local_path: String,
    remote_path: String,
    req_id: String,
) -> Result<(), String> {
    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "upload",
        0,
        0,
    )?;
    let transfer_session = open_transfer_sftp_session(
        state,
        &window.app_handle(),
        sftp,
        reused_from_ssh,
        connection_config,
        pending_hostkey,
        &session_id,
        &req_id,
    )
    .await?;
    let sftp = transfer_session.sftp.clone();

    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "upload",
        0,
        0,
    )?;

    let emit_failed = |current: u64, total: u64, err: String| {
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.clone(),
                id: req_id.clone(),
                direction: "upload".to_string(),
                current,
                total,
                percent: if total > 0 {
                    ((current as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err),
            },
        );
    };

    let local = match tokio::fs::File::open(&local_path).await {
        Ok(file) => file,
        Err(e) => {
            let err = format!("Open Error: {}", e);
            emit_failed(0, 0, err.clone());
            return Err(err);
        }
    };

    let local_meta = match local.metadata().await {
        Ok(m) => m,
        Err(e) => {
            let err = format!("Metadata Error: {}", e);
            emit_failed(0, 0, err.clone());
            return Err(err);
        }
    };
    let total_size = local_meta.len();
    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "upload",
        0,
        total_size,
    )?;
    let source_origin_time = local_meta
        .created()
        .ok()
        .or_else(|| local_meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as u32);
    let local_atime = local_meta
        .accessed()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as u32)
        .or(source_origin_time);
    let local_mtime = source_origin_time;

    let trimmed = remote_path.trim_end_matches('/');
    if trimmed.is_empty() {
        let err = "Invalid filename or path".to_string();
        emit_failed(0, total_size, err.clone());
        return Err(err);
    }

    let _path_guard = acquire_path_lock(state, &session_id, trimmed).await;

    let (parent_dir, base_name) = if let Some((dir, name)) = trimmed.rsplit_once('/') {
        if !dir.is_empty() {
            (dir.to_string(), name.to_string())
        } else {
            ("/".to_string(), name.to_string())
        }
    } else {
        ("".to_string(), trimmed.to_string())
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_name = format!(".{}.upload.tmp.{}.{}", base_name, std::process::id(), ts);
    let temp_path = if parent_dir == "/" {
        format!("/{}", temp_name)
    } else if parent_dir.is_empty() {
        temp_name
    } else {
        format!("{}/{}", parent_dir, temp_name)
    };

    let original_meta = sftp.metadata(&remote_path).await.ok();
    let had_original = original_meta.is_some();
    let original_permissions = original_meta.as_ref().and_then(|meta| meta.permissions);
    let local_permissions = local_permissions_from_metadata(&local_meta);
    let target_permissions = original_permissions.or(local_permissions);
    let backup_name = format!(".{}.upload.bak.{}.{}", base_name, std::process::id(), ts);
    let backup_path = if parent_dir == "/" {
        format!("/{}", backup_name)
    } else if parent_dir.is_empty() {
        backup_name
    } else {
        format!("{}/{}", parent_dir, backup_name)
    };

    ensure_transfer_active(
        &window,
        &cancel,
        &session_id,
        &req_id,
        "upload",
        0,
        total_size,
    )?;

    let mut remote = match sftp.create(&temp_path).await {
        Ok(file) => file,
        Err(e) => {
            let err = format!("Create Error: {}", e);
            emit_failed(0, total_size, err.clone());
            return Err(err);
        }
    };

    let _ = window.emit(
        "sftp-progress",
        ProgressPayload {
            session_id: session_id.clone(),
            id: req_id.clone(),
            direction: "upload".to_string(),
            current: 0,
            total: total_size,
            percent: 0,
            status: "uploading".to_string(),
            error: None,
        },
    );

    let result =
        pipelined_sftp_upload(
            &window,
            &session_id,
            &req_id,
            total_size,
            local,
            &mut remote,
            &cancel,
        )
        .await;
    let uploaded = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = sftp.remove_file(&temp_path).await;
            if cancel.load(Ordering::Relaxed) {
                let _ = window.emit(
                    "sftp-progress",
                    ProgressPayload {
                        session_id: session_id.clone(),
                        id: req_id,
                        direction: "upload".to_string(),
                        current: 0,
                        total: total_size,
                        percent: 0,
                        status: "cancelled".to_string(),
                        error: Some("用户取消了上传".to_string()),
                    },
                );
            }
            return Err(e);
        }
    };
    if cancel.load(Ordering::Relaxed) {
        let _ = sftp.remove_file(&temp_path).await;
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }
    // 强一致性防御：如果你本地真的扫出了总大小，而传传传...传到最后发现实际上累计传出去的字节(uploaded)
    // 根本就没有达到本地计算的总文件大小(total_size)，说明读取流在本地提前断了（比如被占用或报了0）。
    // 这种情况下直接算上传失败，绝不允许继续下一步将残次品替换上去！
    if uploaded < total_size && total_size > 0 {
        let _ = sftp.remove_file(&temp_path).await;
        let err = format!(
            "Premature EOF or local stream truncated: expected {}, sent {}",
            total_size, uploaded
        );
        emit_failed(uploaded, total_size, err.clone());
        return Err(err);
    }

    if let Err(e) = remote.flush().await {
        let err = format!("Flush Error: {}", e);
        let _ = sftp.remove_file(&temp_path).await;
        emit_failed(uploaded, total_size, err.clone());
        return Err(err);
    }
    if let Err(e) = remote.shutdown().await {
        let err = format!("Shutdown Error: {}", e);
        let _ = sftp.remove_file(&temp_path).await;
        emit_failed(uploaded, total_size, err.clone());
        return Err(err);
    }
    drop(remote);

    if cancel.load(Ordering::Relaxed) {
        let _ = sftp.remove_file(&temp_path).await;
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    let temp_size = match read_remote_size_with_retry(&sftp, temp_path.as_str(), 8, 120).await {
        Ok(size) => size,
        Err(e) => {
            let _ = sftp.remove_file(&temp_path).await;
            let err = format!("Temp verify Error: {}", e);
            emit_failed(uploaded, total_size, err.clone());
            return Err(err);
        }
    };

    if temp_size != total_size {
        let _ = sftp.remove_file(&temp_path).await;
        let err = format!(
            "Temp size mismatch: expected {}, observed {}",
            total_size, temp_size
        );
        emit_failed(temp_size, total_size, err.clone());
        return Err(err);
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = sftp.remove_file(&temp_path).await;
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    if had_original {
        if let Err(e) = sftp.rename(&remote_path, &backup_path).await {
            let _ = sftp.remove_file(&temp_path).await;
            let err = format!("Backup Error: {}", e);
            let _ = window.emit(
                "sftp-progress",
                ProgressPayload {
                    session_id: session_id.clone(),
                    id: req_id.clone(),
                    direction: "upload".to_string(),
                    current: uploaded,
                    total: total_size,
                    percent: if total_size > 0 {
                        ((uploaded as f64 / total_size as f64) * 100.0) as u8
                    } else {
                        0
                    },
                    status: "failed".to_string(),
                    error: Some(err.clone()),
                },
            );
            return Err(err);
        }
    }

    if cancel.load(Ordering::Relaxed) {
        if had_original {
            let _ = sftp.rename(&backup_path, &remote_path).await;
        }
        let _ = sftp.remove_file(&temp_path).await;
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    if let Err(e) = sftp.rename(&temp_path, &remote_path).await {
        if had_original {
            let _ = sftp.rename(&backup_path, &remote_path).await;
        }
        let _ = sftp.remove_file(&temp_path).await;
        let err = format!("Promote Error: {}", e);
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.clone(),
                id: req_id.clone(),
                direction: "upload".to_string(),
                current: uploaded,
                total: total_size,
                percent: if total_size > 0 {
                    ((uploaded as f64 / total_size as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err.clone()),
            },
        );
        return Err(err);
    }

    if cancel.load(Ordering::Relaxed) {
        if had_original {
            let _ = sftp.remove_file(&remote_path).await;
            let _ = sftp.rename(&backup_path, &remote_path).await;
        } else {
            let _ = sftp.remove_file(&remote_path).await;
        }
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    // We rely heavily on exact copy streams.
    // Allow up to 2 seconds for remote metadata updates to sync across clustered file systems
    let final_size = match read_remote_size_with_retry(&sftp, &remote_path, 10, 200).await {
        Ok(size) => size,
        Err(_e) => {
            // In case of un-stattable targets, we don't rollback immediately if streams finished completely
            // Just report success locally as best effort.
            total_size
        }
    };
    // Only strictly rollback if the server definitely reports an unexpected non-zero size mismatch
    if final_size != total_size && total_size > 0 && final_size > 0 {
        if had_original {
            let _ = sftp.remove_file(&remote_path).await;
            let _ = sftp.rename(&backup_path, &remote_path).await;
        } else {
            let _ = sftp.remove_file(&remote_path).await;
        }
        let err = format!(
            "Final verify Error: expected {}, observed {}",
            total_size, final_size
        );
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.clone(),
                id: req_id.clone(),
                direction: "upload".to_string(),
                current: final_size,
                total: total_size,
                percent: if total_size > 0 {
                    ((final_size as f64 / total_size as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err.clone()),
            },
        );
        return Err(err);
    }

    if cancel.load(Ordering::Relaxed) {
        if had_original {
            let _ = sftp.remove_file(&remote_path).await;
            let _ = sftp.rename(&backup_path, &remote_path).await;
        } else {
            let _ = sftp.remove_file(&remote_path).await;
        }
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    if local_atime.is_some() || local_mtime.is_some() || target_permissions.is_some() {
        let mut attrs = FileAttributes::empty();
        attrs.atime = local_atime;
        attrs.mtime = local_mtime;
        attrs.permissions = target_permissions;
        if let Err(e) = sftp.set_metadata(&remote_path, attrs).await {
            if had_original {
                let _ = sftp.remove_file(&remote_path).await;
                let _ = sftp.rename(&backup_path, &remote_path).await;
            } else {
                let _ = sftp.remove_file(&remote_path).await;
            }
            let err = format!("Timestamp Sync Error: {}", e);
            emit_failed(uploaded, total_size, err.clone());
            return Err(err);
        }
    }

    if cancel.load(Ordering::Relaxed) {
        if had_original {
            let _ = sftp.remove_file(&remote_path).await;
            let _ = sftp.rename(&backup_path, &remote_path).await;
        } else {
            let _ = sftp.remove_file(&remote_path).await;
        }
        emit_transfer_cancelled(
            &window,
            &session_id,
            &req_id,
            "upload",
            uploaded,
            total_size,
        );
        return Err("Transfer cancelled".to_string());
    }

    if had_original {
        let _ = sftp.remove_file(&backup_path).await;
    }

    // Final success event
    let final_percent = if total_size > 0 { 100 } else { 0 };
    let _ = window.emit(
        "sftp-progress",
        ProgressPayload {
            session_id: session_id.clone(),
            id: req_id,
            direction: "upload".to_string(),
            current: total_size,
            total: total_size,
            percent: final_percent,
            status: "success".to_string(),
            error: None,
        },
    );

    invalidate_session_cache(state, &session_id);

    Ok(())
}

#[tauri::command]
pub async fn sftp_upload_file(
    window: Window,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    local_path: String,
    remote_path: String,
    req_id: String,
) -> Result<(), String> {
    supervisor
        .start_sftp_upload(
            window,
            session_id,
            local_path,
            remote_path,
            req_id,
        )
        .await
}

async fn pipelined_sftp_upload<
    LocalReader: tokio::io::AsyncRead + Unpin + Send + 'static,
    RemoteWriter: tokio::io::AsyncWrite + Unpin,
>(
    window: &Window,
    session_id: &str,
    req_id: &str,
    total_size: u64,
    mut local: LocalReader,
    remote: &mut RemoteWriter,
    cancel: &Arc<AtomicBool>,
) -> Result<u64, String> {
    let (tx, mut rx) =
        tokio::sync::mpsc::channel::<Result<Vec<u8>, String>>(SFTP_TRANSFER_CHANNEL_SIZE);
    let read_cancel = cancel.clone();

    let reader = tokio::spawn(async move {
        let mut buffer = vec![0u8; SFTP_TRANSFER_BUFFER_SIZE];
        loop {
            if read_cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Err("cancelled".into())).await;
                return;
            }
            match local.read(&mut buffer).await {
                Ok(0) => return,
                Ok(n) => {
                    if tx.send(Ok(buffer[..n].to_vec())).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("Local Read Error: {}", e))).await;
                    return;
                }
            }
        }
    });

    let mut uploaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut last_emit_bytes: u64 = 0;
    let emit_failed = |current: u64, total: u64, err: String| {
        let _ = window.emit(
            "sftp-progress",
            ProgressPayload {
                session_id: session_id.to_string(),
                id: req_id.to_string(),
                direction: "upload".to_string(),
                current,
                total,
                percent: if total > 0 {
                    ((current as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                },
                status: "failed".to_string(),
                error: Some(err),
            },
        );
    };

    while let Some(chunk_result) = rx.recv().await {
        let chunk = chunk_result.map_err(|e| {
            emit_failed(uploaded, total_size, e.clone());
            e
        })?;
        let n = chunk.len();
        remote.write_all(&chunk).await.map_err(|e| {
            let err = format!("Remote Write Error: {}", e);
            emit_failed(uploaded, total_size, err.clone());
            err
        })?;
        uploaded += u64::try_from(n).map_err(|_| "Read size conversion error".to_string())?;

        if should_emit_transfer_progress(&last_emit, uploaded.saturating_sub(last_emit_bytes)) {
            let _ = window.emit(
                "sftp-progress",
                ProgressPayload {
                    session_id: session_id.to_string(),
                    id: req_id.to_string(),
                    direction: "upload".to_string(),
                    current: uploaded,
                    total: total_size,
                    percent: if total_size > 0 {
                        ((uploaded as f64 / total_size as f64) * 100.0) as u8
                    } else {
                        0
                    },
                    status: "uploading".to_string(),
                    error: None,
                },
            );
            last_emit = std::time::Instant::now();
            last_emit_bytes = uploaded;
        }
    }

    let _ = reader.await;
    Ok(uploaded)
}

#[tauri::command]
pub async fn sftp_cancel_transfer(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    req_id: String,
) -> Result<(), String> {
    supervisor.cancel_sftp_transfer(session_id, req_id).await
}

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    #[serde(rename = "sessionId")]
    session_id: String,
    id: String,
    direction: String,
    current: u64,
    total: u64,
    percent: u8,
    status: String,
    error: Option<String>,
}

pub async fn sftp_disconnect_legacy(
    state: &SftpAppState,
    handle: Option<SftpConnectionHandle>,
    session_id: String,
) -> Result<(), String> {
    if let Some(mut handle) = handle {
        if let Some(keepalive) = handle.keepalive.take() {
            keepalive.stop("SFTP connection disconnecting").await;
        }
        let _ = handle.sftp.close().await;
        if let Some(session) = handle.session {
            let _ = session
                .disconnect(russh::Disconnect::ByApplication, "", "English")
                .await;
        }
        if let Some(jump_session) = handle.jump_session {
            let _ = jump_session
                .disconnect(russh::Disconnect::ByApplication, "", "English")
                .await;
        }
    }
    cleanup_session_state(state, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_disconnect(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
) -> Result<(), String> {
    supervisor
        .disconnect_sftp(session_id)
        .await
}

#[tauri::command]
pub async fn sftp_is_connected(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
) -> Result<bool, String> {
    supervisor.is_sftp_connected(session_id).await
}

#[tauri::command]
pub async fn confirm_sftp_hostkey(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    accept: bool,
) -> Result<(), String> {
    supervisor.confirm_sftp_hostkey(session_id, accept).await
}

pub async fn sftp_exists_legacy(
    _state: &SftpAppState,
    handle: &SftpConnectionHandle,
    _session_id: String,
    path: String,
) -> Result<bool, String> {
    let sftp = get_sftp_session(handle);

    match sftp.metadata(&path).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[tauri::command]
pub async fn sftp_exists(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<bool, String> {
    supervisor
        .sftp_exists(session_id, path)
        .await
}

pub async fn sftp_mkdir_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let sftp = get_sftp_session(handle);
    sftp.create_dir(&path)
        .await
        .map_err(|e| format!("MKDIR Error: {}", e))?;
    invalidate_session_cache(&state, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_mkdir(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    supervisor
        .mkdir_sftp(session_id, path)
        .await
}

pub async fn sftp_rename_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    let sftp = get_sftp_session(handle);
    let lock_paths = vec![from_path.as_str(), to_path.as_str()];
    let _path_guards = acquire_path_locks(&state, &session_id, &lock_paths).await;
    sftp.rename(&from_path, &to_path)
        .await
        .map_err(|e| format!("Rename Error: {}", e))?;
    invalidate_session_cache(&state, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_rename(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    supervisor
        .rename_sftp(session_id, from_path, to_path)
        .await
}

pub async fn sftp_remove_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let sftp = get_sftp_session(handle);
    let _path_guard = acquire_path_lock(&state, &session_id, &path).await;
    if is_dir {
        sftp.remove_dir(&path)
            .await
            .map_err(|e| format!("Remove dir Error: {}", e))?;
    } else {
        sftp.remove_file(&path)
            .await
            .map_err(|e| format!("Remove file Error: {}", e))?;
    }
    invalidate_session_cache(&state, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_remove(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    supervisor
        .remove_sftp(session_id, path, is_dir)
        .await
}

pub async fn sftp_chmod_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
    permissions: u32,
) -> Result<(), String> {
    let sftp = get_sftp_session(handle);
    let _path_guard = acquire_path_lock(&state, &session_id, &path).await;
    let mut attrs = FileAttributes::empty();
    attrs.permissions = Some(permissions);
    sftp.set_metadata(&path, attrs)
        .await
        .map_err(|e| format!("Chmod Error: {}", e))?;
    invalidate_session_cache(&state, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_chmod(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
    permissions: u32,
) -> Result<(), String> {
    supervisor
        .chmod_sftp(session_id, path, permissions)
        .await
}

pub async fn sftp_stat_legacy(
    state: &SftpAppState,
    handle: &SftpConnectionHandle,
    session_id: String,
    path: String,
) -> Result<FileEntry, String> {
    let sftp = get_sftp_session(handle);
    let attrs = sftp
        .metadata(&path)
        .await
        .map_err(|e| format!("Stat Error: {}", e))?;
    let (passwd_map, group_map) = get_ug_maps(&state, &session_id, &sftp).await;
    let file_name = std::path::Path::new(&path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(&path)
        .to_string();

    Ok(file_entry_from_attrs(
        file_name,
        &attrs,
        &passwd_map,
        &group_map,
    ))
}

#[tauri::command]
pub async fn sftp_stat(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _state: tauri::State<'_, SftpAppState>,
    session_id: String,
    path: String,
) -> Result<FileEntry, String> {
    supervisor
        .stat_sftp(session_id, path)
        .await
}
