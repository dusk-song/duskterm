use tauri::{AppHandle, Window};
use tokio::sync::oneshot;

use crate::{
    remote_monitor::RemoteStats,
    sftp::{
        FileEntry, SftpAppState, SftpLsPagedResult, SftpOpenTextFileResult, SftpSaveTextFileResult,
        SshConfig as SftpConfig,
    },
    ssh::SshConfig,
    storage::SharedStorageState,
    tunnel::{TunnelInfo, TunnelRequest, TunnelState},
};

pub enum SessionMessage {
    Connect {
        app_handle: AppHandle,
        sftp_state: SftpAppState,
        config: SshConfig,
        respond_to: oneshot::Sender<Result<String, String>>,
    },
    WriteTerminal {
        data: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    ResizeTerminal {
        cols: u32,
        rows: u32,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    OpenShellChannel {
        app_handle: AppHandle,
        channel_id: String,
        term_type: Option<String>,
        login_script: Option<String>,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    WriteShellChannel {
        channel_id: String,
        data: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    ResizeShellChannel {
        channel_id: String,
        cols: u32,
        rows: u32,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    CloseShellChannel {
        channel_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    Disconnect {
        sftp_state: SftpAppState,
        tunnel_state: TunnelState,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    ConnectSftp {
        app_handle: AppHandle,
        sftp_state: SftpAppState,
        config: SftpConfig,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    DisconnectSftp {
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    GetRemoteStats {
        respond_to: oneshot::Sender<Result<RemoteStats, String>>,
    },
    IsSftpConnected {
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    ListSftpDir {
        path: String,
        respond_to: oneshot::Sender<Result<Vec<FileEntry>, String>>,
    },
    ListSftpDirPaged {
        path: String,
        offset: Option<usize>,
        limit: Option<usize>,
        respond_to: oneshot::Sender<Result<SftpLsPagedResult, String>>,
    },
    ReadSftpFile {
        path: String,
        respond_to: oneshot::Sender<Result<String, String>>,
    },
    OpenSftpTextFile {
        path: String,
        respond_to: oneshot::Sender<Result<SftpOpenTextFileResult, String>>,
    },
    WriteSftpFile {
        path: String,
        content: String,
        expected_modified: Option<u64>,
        expected_size: Option<u64>,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    SaveSftpTextFile {
        path: String,
        content: String,
        expected_cas_token: String,
        respond_to: oneshot::Sender<Result<SftpSaveTextFileResult, String>>,
    },
    SftpExists {
        path: String,
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    MkdirSftp {
        path: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    RenameSftp {
        from_path: String,
        to_path: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    RemoveSftp {
        path: String,
        is_dir: bool,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    ChmodSftp {
        path: String,
        permissions: u32,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    StatSftp {
        path: String,
        respond_to: oneshot::Sender<Result<FileEntry, String>>,
    },
    StartSftpDownload {
        window: Window,
        remote_path: String,
        local_path: String,
        req_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    StartSftpUpload {
        window: Window,
        local_path: String,
        remote_path: String,
        req_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    CancelSftpTransfer {
        req_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    StartTunnel {
        app_handle: AppHandle,
        tunnel_state: TunnelState,
        request: TunnelRequest,
        respond_to: oneshot::Sender<Result<TunnelInfo, String>>,
    },
    StartTunnelFromConfig {
        app_handle: AppHandle,
        tunnel_state: TunnelState,
        storage_state: SharedStorageState,
        config_id: String,
        respond_to: oneshot::Sender<Result<TunnelInfo, String>>,
    },
    StopTunnel {
        tunnel_state: TunnelState,
        tunnel_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
}
