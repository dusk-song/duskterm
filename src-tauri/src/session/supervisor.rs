use std::collections::HashMap;
use std::sync::Mutex;

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
    tunnel::{self, TunnelInfo, TunnelRequest, TunnelState},
};

use super::{
    actor::{spawn_session_actor, SessionActorHandle},
    message::SessionMessage,
    state::{resolve_hostkey_decision, SharedHostkeyDecision},
};

pub struct SessionSupervisor {
    actors: Mutex<HashMap<String, SessionActorHandle>>,
    pending_ssh_hostkeys: Mutex<HashMap<String, SharedHostkeyDecision>>,
    pending_sftp_hostkeys: Mutex<HashMap<String, SharedHostkeyDecision>>,
}

#[cfg(test)]
mod tests {
    use super::SessionSupervisor;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn confirm_hostkey_resolves_pending_decision_without_actor_roundtrip() {
        let supervisor = SessionSupervisor::new();
        let (tx, rx) = oneshot::channel();

        {
            let pending = supervisor.ssh_hostkey_decision("session-1");
            *pending.lock().unwrap() = Some(tx);
        }

        let result = supervisor
            .confirm_hostkey("session-1".to_string(), true)
            .await;

        assert!(result.is_ok());
        assert_eq!(rx.await.unwrap(), true);
    }

    #[tokio::test]
    async fn confirm_sftp_hostkey_resolves_pending_decision_without_actor_roundtrip() {
        let supervisor = SessionSupervisor::new();
        let (tx, rx) = oneshot::channel();

        {
            let pending = supervisor.sftp_hostkey_decision("session-1");
            *pending.lock().unwrap() = Some(tx);
        }

        let result = supervisor
            .confirm_sftp_hostkey("session-1".to_string(), false)
            .await;

        assert!(result.is_ok());
        assert_eq!(rx.await.unwrap(), false);
    }

    #[tokio::test]
    async fn confirm_jump_hostkey_uses_parent_session_pending_decision() {
        let supervisor = SessionSupervisor::new();
        let (tx, rx) = oneshot::channel();

        {
            let pending = supervisor.ssh_hostkey_decision("session-1");
            *pending.lock().unwrap() = Some(tx);
        }

        let result = supervisor
            .confirm_hostkey("session-1::jump".to_string(), true)
            .await;

        assert!(result.is_ok());
        assert_eq!(rx.await.unwrap(), true);
    }
}

impl SessionSupervisor {
    const GLOBAL_TUNNEL_RUNTIME_ID: &'static str = "__global_tunnel_runtime__";

    pub fn new() -> Self {
        Self {
            actors: Mutex::new(HashMap::new()),
            pending_ssh_hostkeys: Mutex::new(HashMap::new()),
            pending_sftp_hostkeys: Mutex::new(HashMap::new()),
        }
    }

    fn get_actor(&self, session_id: &str) -> Result<SessionActorHandle, String> {
        self.actors
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| "Session runtime not found".to_string())
    }

    fn get_or_spawn_actor(&self, session_id: &str) -> SessionActorHandle {
        let pending_ssh_hostkey = self.ssh_hostkey_decision(session_id);
        let pending_sftp_hostkey = self.sftp_hostkey_decision(session_id);
        let mut actors = self.actors.lock().unwrap();
        actors
            .entry(session_id.to_string())
            .or_insert_with(|| {
                spawn_session_actor(
                    session_id.to_string(),
                    pending_ssh_hostkey,
                    pending_sftp_hostkey,
                )
            })
            .clone()
    }

    fn remove_actor(&self, session_id: &str) {
        self.actors.lock().unwrap().remove(session_id);
        self.pending_ssh_hostkeys.lock().unwrap().remove(session_id);
        self.pending_sftp_hostkeys
            .lock()
            .unwrap()
            .remove(session_id);
    }

    fn hostkey_decision(
        map: &Mutex<HashMap<String, SharedHostkeyDecision>>,
        session_id: &str,
    ) -> SharedHostkeyDecision {
        map.lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    fn ssh_hostkey_decision(&self, session_id: &str) -> SharedHostkeyDecision {
        Self::hostkey_decision(&self.pending_ssh_hostkeys, session_id)
    }

    fn sftp_hostkey_decision(&self, session_id: &str) -> SharedHostkeyDecision {
        Self::hostkey_decision(&self.pending_sftp_hostkeys, session_id)
    }

    fn jump_parent_session_id(session_id: &str) -> Option<&str> {
        const JUMP_MARKER: &str = "::jump";
        session_id
            .find(JUMP_MARKER)
            .filter(|index| *index > 0)
            .map(|index| &session_id[..index])
    }

    fn resolve_hostkey_for_session(
        map: &Mutex<HashMap<String, SharedHostkeyDecision>>,
        session_id: &str,
        accept: bool,
    ) -> Result<(), String> {
        let exact = map.lock().unwrap().get(session_id).cloned();
        if let Some(pending) = exact {
            if resolve_hostkey_decision(&pending, accept).is_ok() {
                return Ok(());
            }
        }

        if let Some(parent_session_id) = Self::jump_parent_session_id(session_id) {
            let parent = map.lock().unwrap().get(parent_session_id).cloned();
            if let Some(pending) = parent {
                return resolve_hostkey_decision(&pending, accept);
            }
        }

        Err("No pending host key confirmation".to_string())
    }

    fn normalized_session_id(session_id: Option<String>) -> Option<String> {
        session_id.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
    }

    pub async fn connect(
        &self,
        app_handle: AppHandle,
        sftp_state: SftpAppState,
        session_id: String,
        config: SshConfig,
    ) -> Result<String, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::Connect {
                app_handle,
                sftp_state,
                config,
                respond_to,
            })
            .map_err(|_| "Failed to send connect message to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped connect response".to_string())?
    }

    pub async fn confirm_hostkey(&self, session_id: String, accept: bool) -> Result<(), String> {
        Self::resolve_hostkey_for_session(&self.pending_ssh_hostkeys, &session_id, accept)
    }

    pub async fn write_terminal(&self, session_id: String, data: String) -> Result<(), String> {
        let actor = self.get_actor(&session_id)?;
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::WriteTerminal { data, respond_to })
            .map_err(|_| "Failed to send terminal write to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped terminal write response".to_string())?
    }

    pub async fn resize_terminal(
        &self,
        session_id: String,
        cols: u32,
        rows: u32,
    ) -> Result<(), String> {
        let actor = self.get_actor(&session_id)?;
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ResizeTerminal {
                cols,
                rows,
                respond_to,
            })
            .map_err(|_| "Failed to send terminal resize to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped resize response".to_string())?
    }

    pub async fn disconnect(
        &self,
        sftp_state: SftpAppState,
        tunnel_state: TunnelState,
        session_id: String,
    ) -> Result<(), String> {
        let actor = self.get_actor(&session_id)?;
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::Disconnect {
                sftp_state,
                tunnel_state,
                respond_to,
            })
            .map_err(|_| "Failed to send disconnect to session actor".to_string())?;

        let result = rx
            .await
            .map_err(|_| "Session actor dropped disconnect response".to_string())?;
        self.remove_actor(&session_id);
        result
    }

    pub async fn start_tunnel(
        &self,
        app_handle: AppHandle,
        tunnel_state: TunnelState,
        request: TunnelRequest,
    ) -> Result<TunnelInfo, String> {
        let session_id = Self::normalized_session_id(request.session_id.clone())
            .unwrap_or_else(|| Self::GLOBAL_TUNNEL_RUNTIME_ID.to_string());

        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StartTunnel {
                app_handle,
                tunnel_state,
                request,
                respond_to,
            })
            .map_err(|_| "Failed to send tunnel start message to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped tunnel start response".to_string())?
    }

    pub async fn start_tunnel_from_config(
        &self,
        app_handle: AppHandle,
        tunnel_state: TunnelState,
        storage_state: SharedStorageState,
        config_id: String,
    ) -> Result<TunnelInfo, String> {
        let session_id = tunnel::load_session_id_for_tunnel_config(&storage_state, &config_id)?;
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StartTunnelFromConfig {
                app_handle,
                tunnel_state,
                storage_state,
                config_id,
                respond_to,
            })
            .map_err(|_| {
                "Failed to send saved tunnel start message to session actor".to_string()
            })?;

        rx.await
            .map_err(|_| "Session actor dropped saved tunnel start response".to_string())?
    }

    pub async fn stop_tunnel(
        &self,
        tunnel_state: TunnelState,
        tunnel_id: String,
    ) -> Result<(), String> {
        let session_id = tunnel_state
            .session_id_for_tunnel(&tunnel_id)?
            .unwrap_or_else(|| Self::GLOBAL_TUNNEL_RUNTIME_ID.to_string());
        let actor = self.get_or_spawn_actor(&session_id);

        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StopTunnel {
                tunnel_state,
                tunnel_id,
                respond_to,
            })
            .map_err(|_| "Failed to send tunnel stop message to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped tunnel stop response".to_string())?
    }

    pub async fn connect_sftp(
        &self,
        app_handle: AppHandle,
        sftp_state: SftpAppState,
        session_id: String,
        config: SftpConfig,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ConnectSftp {
                app_handle,
                sftp_state,
                config,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP connect message to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP connect response".to_string())?
    }

    pub async fn confirm_sftp_hostkey(
        &self,
        session_id: String,
        accept: bool,
    ) -> Result<(), String> {
        Self::resolve_hostkey_for_session(&self.pending_sftp_hostkeys, &session_id, accept)
    }

    pub async fn disconnect_sftp(&self, session_id: String) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);

        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::DisconnectSftp { respond_to })
            .map_err(|_| "Failed to send SFTP disconnect to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP disconnect response".to_string())?
    }

    pub async fn get_remote_stats(&self, session_id: String) -> Result<RemoteStats, String> {
        let actor = self.get_or_spawn_actor(&session_id);

        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::GetRemoteStats { respond_to })
            .map_err(|_| "Failed to send remote monitor request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped remote monitor response".to_string())?
    }

    pub async fn is_sftp_connected(&self, session_id: String) -> Result<bool, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::IsSftpConnected { respond_to })
            .map_err(|_| "Failed to send SFTP status request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP status response".to_string())?
    }

    pub async fn list_sftp_dir(
        &self,
        session_id: String,
        path: String,
    ) -> Result<Vec<FileEntry>, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ListSftpDir { path, respond_to })
            .map_err(|_| "Failed to send SFTP list request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP list response".to_string())?
    }

    pub async fn list_sftp_dir_paged(
        &self,
        session_id: String,
        path: String,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<SftpLsPagedResult, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ListSftpDirPaged {
                path,
                offset,
                limit,
                respond_to,
            })
            .map_err(|_| "Failed to send paged SFTP list request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped paged SFTP list response".to_string())?
    }

    pub async fn read_sftp_file(&self, session_id: String, path: String) -> Result<String, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ReadSftpFile { path, respond_to })
            .map_err(|_| "Failed to send SFTP read request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP read response".to_string())?
    }

    pub async fn open_sftp_text_file(
        &self,
        session_id: String,
        path: String,
    ) -> Result<SftpOpenTextFileResult, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::OpenSftpTextFile { path, respond_to })
            .map_err(|_| "Failed to send SFTP text open request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP text open response".to_string())?
    }

    pub async fn write_sftp_file(
        &self,
        session_id: String,
        path: String,
        content: String,
        expected_modified: Option<u64>,
        expected_size: Option<u64>,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::WriteSftpFile {
                path,
                content,
                expected_modified,
                expected_size,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP write request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP write response".to_string())?
    }

    pub async fn save_sftp_text_file(
        &self,
        session_id: String,
        path: String,
        content: String,
        expected_cas_token: String,
    ) -> Result<SftpSaveTextFileResult, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::SaveSftpTextFile {
                path,
                content,
                expected_cas_token,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP text save request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP text save response".to_string())?
    }

    pub async fn sftp_exists(&self, session_id: String, path: String) -> Result<bool, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::SftpExists { path, respond_to })
            .map_err(|_| "Failed to send SFTP exists request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP exists response".to_string())?
    }

    pub async fn mkdir_sftp(&self, session_id: String, path: String) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::MkdirSftp { path, respond_to })
            .map_err(|_| "Failed to send SFTP mkdir request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP mkdir response".to_string())?
    }

    pub async fn rename_sftp(
        &self,
        session_id: String,
        from_path: String,
        to_path: String,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::RenameSftp {
                from_path,
                to_path,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP rename request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP rename response".to_string())?
    }

    pub async fn remove_sftp(
        &self,
        session_id: String,
        path: String,
        is_dir: bool,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::RemoveSftp {
                path,
                is_dir,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP remove request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP remove response".to_string())?
    }

    pub async fn chmod_sftp(
        &self,
        session_id: String,
        path: String,
        permissions: u32,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::ChmodSftp {
                path,
                permissions,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP chmod request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP chmod response".to_string())?
    }

    pub async fn stat_sftp(&self, session_id: String, path: String) -> Result<FileEntry, String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StatSftp { path, respond_to })
            .map_err(|_| "Failed to send SFTP stat request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP stat response".to_string())?
    }

    pub async fn start_sftp_download(
        &self,
        window: Window,
        session_id: String,
        remote_path: String,
        local_path: String,
        req_id: String,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StartSftpDownload {
                window,
                remote_path,
                local_path,
                req_id,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP download request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP download response".to_string())?
    }

    pub async fn start_sftp_upload(
        &self,
        window: Window,
        session_id: String,
        local_path: String,
        remote_path: String,
        req_id: String,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);
        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::StartSftpUpload {
                window,
                local_path,
                remote_path,
                req_id,
                respond_to,
            })
            .map_err(|_| "Failed to send SFTP upload request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP upload response".to_string())?
    }

    pub async fn cancel_sftp_transfer(
        &self,
        session_id: String,
        req_id: String,
    ) -> Result<(), String> {
        let actor = self.get_or_spawn_actor(&session_id);

        let (respond_to, rx) = oneshot::channel();
        actor
            .sender
            .send(SessionMessage::CancelSftpTransfer { req_id, respond_to })
            .map_err(|_| "Failed to send SFTP cancel request to session actor".to_string())?;

        rx.await
            .map_err(|_| "Session actor dropped SFTP cancel response".to_string())?
    }
}
