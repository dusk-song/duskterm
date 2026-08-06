mod files;
mod runtime;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

pub use runtime::TerminalTransferRuntime;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalTransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadCollisionPolicy {
    #[default]
    AutoRename,
    Overwrite,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalTransferSelection {
    Upload {
        paths: Vec<String>,
    },
    Download {
        directory: String,
        #[serde(default, rename = "collisionPolicy")]
        collision_policy: DownloadCollisionPolicy,
    },
}

impl TerminalTransferSelection {
    pub const fn direction(&self) -> TerminalTransferDirection {
        match self {
            Self::Upload { .. } => TerminalTransferDirection::Upload,
            Self::Download { .. } => TerminalTransferDirection::Download,
        }
    }
}

pub enum TerminalTransferControl {
    Accept {
        request_id: String,
        selection: TerminalTransferSelection,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    Reject {
        request_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    Cancel {
        operation_id: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
}

impl TerminalTransferControl {
    pub(crate) fn respond(self, result: Result<(), String>) {
        let respond_to = match self {
            Self::Accept { respond_to, .. }
            | Self::Reject { respond_to, .. }
            | Self::Cancel { respond_to, .. } => respond_to,
        };
        let _ = respond_to.send(result);
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTransferRequest {
    pub request_id: String,
    pub session_id: String,
    pub workspace_session_id: String,
    pub channel_id: Option<String>,
    pub protocol: &'static str,
    pub direction: TerminalTransferDirection,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressPayload {
    pub operation_id: String,
    pub task_id: String,
    pub session_id: String,
    pub workspace_session_id: String,
    pub channel_id: Option<String>,
    pub protocol: &'static str,
    pub direction: TerminalTransferDirection,
    pub file_name: String,
    pub local_path: Option<String>,
    pub remote_path: Option<String>,
    pub current: u64,
    pub total: u64,
    pub percent: u8,
    pub status: &'static str,
    pub phase: &'static str,
    pub terminal_restored: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTransferEnded {
    pub operation_id: Option<String>,
    pub request_id: Option<String>,
    pub session_id: String,
    pub workspace_session_id: String,
    pub channel_id: Option<String>,
    pub direction: TerminalTransferDirection,
    pub terminal_restored: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn accept_terminal_transfer(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    workspace_session_id: String,
    channel_id: Option<String>,
    request_id: String,
    selection: TerminalTransferSelection,
) -> Result<(), String> {
    supervisor
        .accept_terminal_transfer(workspace_session_id, channel_id, request_id, selection)
        .await
}

#[tauri::command]
pub async fn reject_terminal_transfer(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    workspace_session_id: String,
    channel_id: Option<String>,
    request_id: String,
) -> Result<(), String> {
    supervisor
        .reject_terminal_transfer(workspace_session_id, channel_id, request_id)
        .await
}

#[tauri::command]
pub async fn cancel_terminal_transfer(
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    workspace_session_id: String,
    channel_id: Option<String>,
    operation_id: String,
) -> Result<(), String> {
    supervisor
        .cancel_terminal_transfer(workspace_session_id, channel_id, operation_id)
        .await
}
