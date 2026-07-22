use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

pub type SharedHostkeyDecision = Arc<Mutex<Option<oneshot::Sender<bool>>>>;

pub struct SessionRuntimeState {
    pub lifecycle: LifecycleState,
    pub terminal: TerminalState,
    pub ssh: Option<ManagedSshRuntime>,
    pub shell_channels: HashMap<String, ManagedSshRuntime>,
    pub sftp: Option<ManagedSftpRuntime>,
    pub tunnels: TunnelRuntimeState,
    pub security: SecurityState,
    pub transfers: TransferState,
    pub transfer_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

impl SessionRuntimeState {
    pub fn with_hostkey_decisions(
        pending_ssh_hostkey: SharedHostkeyDecision,
        pending_sftp_hostkey: SharedHostkeyDecision,
    ) -> Self {
        Self {
            security: SecurityState::with_hostkey_decisions(
                pending_ssh_hostkey,
                pending_sftp_hostkey,
            ),
            ..Self::default()
        }
    }
}

impl Default for SessionRuntimeState {
    fn default() -> Self {
        Self {
            lifecycle: LifecycleState::default(),
            terminal: TerminalState::default(),
            ssh: None,
            shell_channels: HashMap::new(),
            sftp: None,
            tunnels: TunnelRuntimeState::default(),
            security: SecurityState::default(),
            transfers: TransferState::default(),
            transfer_tasks: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub enum LifecycleState {
    #[default]
    Idle,
    Starting,
    Active,
    Error,
    Closed,
}

#[derive(Clone, Debug, Default)]
pub struct TerminalState {
    pub attached: bool,
}

#[derive(Clone, Debug)]
pub struct SecurityState {
    pub pending_ssh_hostkey: SharedHostkeyDecision,
    pub pending_sftp_hostkey: SharedHostkeyDecision,
}

impl Default for SecurityState {
    fn default() -> Self {
        Self {
            pending_ssh_hostkey: Arc::new(Mutex::new(None)),
            pending_sftp_hostkey: Arc::new(Mutex::new(None)),
        }
    }
}

impl SecurityState {
    pub fn with_hostkey_decisions(
        pending_ssh_hostkey: SharedHostkeyDecision,
        pending_sftp_hostkey: SharedHostkeyDecision,
    ) -> Self {
        Self {
            pending_ssh_hostkey,
            pending_sftp_hostkey,
        }
    }
}

pub fn resolve_hostkey_decision(
    pending: &SharedHostkeyDecision,
    accept: bool,
) -> Result<(), String> {
    let sender = pending.lock().unwrap().take();
    if let Some(tx) = sender {
        let _ = tx.send(accept);
        Ok(())
    } else {
        Err("No pending host key confirmation".to_string())
    }
}

#[derive(Clone, Debug, Default)]
pub struct TransferState {
    pub cancel_tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl TransferState {
    pub fn register_cancel_token(&self, req_id: &str) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        self.cancel_tokens
            .lock()
            .unwrap()
            .insert(req_id.to_string(), token.clone());
        token
    }

    pub fn cleanup(&self, req_id: &str) {
        self.cancel_tokens.lock().unwrap().remove(req_id);
    }

    pub fn cancel(&self, req_id: &str) -> Result<(), String> {
        let map = self.cancel_tokens.lock().unwrap();
        if let Some(token) = map.get(req_id) {
            token.store(true, Ordering::Relaxed);
            Ok(())
        } else {
            Err("No active transfer found for this ID".to_string())
        }
    }

    pub fn cancel_all(&self) {
        let map = self.cancel_tokens.lock().unwrap();
        for token in map.values() {
            token.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
pub struct TunnelRuntimeState {
    pub handles: HashMap<String, ManagedTunnelRuntime>,
}

pub struct ManagedTunnelRuntime {
    pub shutdown: Option<oneshot::Sender<()>>,
    pub task: Option<tokio::task::JoinHandle<()>>,
}

pub struct ManagedSshRuntime {
    pub handle: crate::ssh::TerminalRuntimeHandle,
    pub task: Option<tokio::task::JoinHandle<()>>,
}

pub struct ManagedSftpRuntime {
    pub handle: crate::sftp::SftpConnectionHandle,
    pub state: crate::sftp::SftpAppState,
}
