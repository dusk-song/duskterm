use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use duskterm_terminal_transfer::{
    StreamEvent, TerminalStreamMux, TransferDirection as CoreDirection, ZmodemProbe,
    ZMODEM_CANCEL_SEQUENCE,
};
use russh::{client, Channel};
use tauri::{AppHandle, Emitter};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;
use zmodem2::{Action, Event, FileInfo, Position, Receiver, Sender};

use super::files::{cleanup_temp_file, commit_download, prepare_download_target, DownloadTarget};
use super::{
    DownloadCollisionPolicy, TerminalTransferControl, TerminalTransferDirection,
    TerminalTransferEnded, TerminalTransferRequest, TerminalTransferSelection,
    TransferProgressPayload,
};

const DECISION_TIMEOUT: Duration = Duration::from_secs(30);
const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(90);
const PROTOCOL_RETRY_INTERVAL: Duration = Duration::from_secs(10);
const RECOVERY_DELAY: Duration = Duration::from_millis(800);
const POST_UPLOAD_ENTER_DELAY: Duration = Duration::from_millis(250);
const MAX_RECOVERY_BUFFER: usize = 64 * 1024;
const MAX_UPLOAD_DRIVER_STEPS: usize = 32 * 1024;
const MAX_DOWNLOAD_DRIVER_STEPS: usize = 4096;
const UPLOAD_CHANNEL_BATCH_SIZE: usize = 128 * 1024;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone)]
struct RuntimeContext {
    session_id: String,
    workspace_session_id: String,
    channel_id: Option<String>,
}

struct PendingRequest {
    id: String,
    direction: TerminalTransferDirection,
    expires: Instant,
}

struct RecoveryState {
    operation_id: Option<String>,
    request_id: Option<String>,
    direction: TerminalTransferDirection,
    error: Option<String>,
    deadline: Instant,
    buffered: Vec<u8>,
}

pub struct TerminalTransferRuntime {
    context: RuntimeContext,
    mux: TerminalStreamMux,
    pending: Option<PendingRequest>,
    active: Option<ActiveZmodemRuntime>,
    recovery: Option<RecoveryState>,
    owned: Arc<AtomicBool>,
}

impl TerminalTransferRuntime {
    pub fn new(
        workspace_session_id: String,
        channel_id: Option<String>,
        owned: Arc<AtomicBool>,
    ) -> Self {
        let session_id = channel_id
            .clone()
            .unwrap_or_else(|| workspace_session_id.clone());
        Self {
            context: RuntimeContext {
                session_id,
                workspace_session_id,
                channel_id,
            },
            mux: TerminalStreamMux::default(),
            pending: None,
            active: None,
            recovery: None,
            owned,
        }
    }

    pub async fn handle_remote(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
        data: &[u8],
    ) -> Result<Vec<Vec<u8>>, String> {
        let events = match self.mux.push_remote(data) {
            Ok(events) => events,
            Err(error) => {
                let message = format!("ZMODEM 流缓冲失败：{error:?}");
                let request_id = self.pending.as_ref().map(|pending| pending.id.clone());
                self.begin_recovery(channel, request_id, Some(message.clone()))
                    .await?;
                return Err(message);
            }
        };
        self.process_stream_events(app_handle, channel, events)
            .await
    }

    pub async fn handle_control(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
        control: TerminalTransferControl,
    ) {
        match control {
            TerminalTransferControl::Accept {
                request_id,
                selection,
                respond_to,
            } => {
                let result = self
                    .accept(app_handle, channel, &request_id, selection)
                    .await;
                let _ = respond_to.send(result);
            }
            TerminalTransferControl::Reject {
                request_id,
                respond_to,
            } => {
                let result = self.reject(channel, &request_id, None).await;
                let _ = respond_to.send(result);
            }
            TerminalTransferControl::Cancel {
                operation_id,
                respond_to,
            } => {
                let result = self.cancel(app_handle, channel, &operation_id).await;
                let _ = respond_to.send(result);
            }
        }
    }

    pub async fn on_tick(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
    ) -> Result<Vec<Vec<u8>>, String> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.expires)
        {
            if let Some(request_id) = self.pending.as_ref().map(|pending| pending.id.clone()) {
                self.reject(channel, &request_id, Some("文件选择已超时".to_string()))
                    .await?;
            }
        }

        let protocol_timed_out = self
            .active
            .as_ref()
            .is_some_and(|active| active.last_remote_activity().elapsed() >= PROTOCOL_TIMEOUT);
        if protocol_timed_out {
            let error = "ZMODEM 协议等待远端响应超时".to_string();
            if let Some(active) = self.active.as_mut() {
                active.fail(app_handle, &self.context, &error).await;
            }
            self.begin_recovery(channel, None, Some(error)).await?;
        } else if self
            .active
            .as_ref()
            .is_some_and(|active| active.last_retry().elapsed() >= PROTOCOL_RETRY_INTERVAL)
        {
            let timeout_result = self
                .active
                .as_mut()
                .expect("active checked above")
                .timeout();
            if let Err(error) = timeout_result {
                if let Some(active) = self.active.as_mut() {
                    active.fail(app_handle, &self.context, &error).await;
                }
                self.begin_recovery(channel, None, Some(error.clone()))
                    .await?;
                return Err(error);
            }
            self.active
                .as_mut()
                .expect("active checked above")
                .mark_retry();
            return self.drive_active(app_handle, channel).await;
        }

        if self
            .recovery
            .as_ref()
            .is_some_and(|recovery| Instant::now() >= recovery.deadline)
        {
            return Ok(self.finish_recovery(app_handle));
        }

        // A fully streaming sender can have more immediately available work
        // after the bounded driver loop yields. Continue it on the lightweight
        // transfer tick even when no reverse-channel packet has arrived yet.
        if self.active.is_some() {
            return self.drive_active(app_handle, channel).await;
        }

        Ok(Vec::new())
    }

    pub async fn shutdown(&mut self, app_handle: &AppHandle) {
        let ended = if let Some(mut active) = self.active.take() {
            active.cancel(app_handle, &self.context, "会话已关闭").await;
            Some((
                Some(active.operation_id().to_string()),
                None,
                active.direction(),
            ))
        } else if let Some(pending) = self.pending.take() {
            Some((None, Some(pending.id), pending.direction))
        } else if let Some(recovery) = self.recovery.take() {
            Some((
                recovery.operation_id,
                recovery.request_id,
                recovery.direction,
            ))
        } else {
            None
        };
        self.mux.restore_terminal();
        self.owned.store(false, Ordering::Release);
        if let Some((operation_id, request_id, direction)) = ended {
            let _ = app_handle.emit(
                "terminal-transfer-ended",
                TerminalTransferEnded {
                    operation_id,
                    request_id,
                    session_id: self.context.session_id.clone(),
                    workspace_session_id: self.context.workspace_session_id.clone(),
                    channel_id: self.context.channel_id.clone(),
                    direction,
                    terminal_restored: true,
                    error: Some("会话已关闭".to_string()),
                },
            );
        }
    }

    pub fn flush_terminal_data(&mut self) -> Vec<u8> {
        self.mux.flush_terminal_data()
    }

    async fn process_stream_events(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
        events: Vec<StreamEvent>,
    ) -> Result<Vec<Vec<u8>>, String> {
        let mut terminal_chunks = self.process_passive_events(app_handle, events);

        if self.active.is_some() {
            terminal_chunks.extend(self.drive_active(app_handle, channel).await?);
        }
        Ok(terminal_chunks)
    }

    fn process_passive_events(
        &mut self,
        app_handle: &AppHandle,
        events: Vec<StreamEvent>,
    ) -> Vec<Vec<u8>> {
        let mut terminal_chunks = Vec::new();
        for event in events {
            match event {
                StreamEvent::TerminalData(data) => terminal_chunks.push(data),
                StreamEvent::Detected(detection) => {
                    let direction = map_direction(detection.direction);
                    let request_id = Uuid::new_v4().to_string();
                    let expires = Instant::now() + DECISION_TIMEOUT;
                    self.pending = Some(PendingRequest {
                        id: request_id.clone(),
                        direction,
                        expires,
                    });
                    self.owned.store(true, Ordering::Release);
                    let _ = app_handle.emit(
                        "terminal-transfer-request",
                        TerminalTransferRequest {
                            request_id,
                            session_id: self.context.session_id.clone(),
                            workspace_session_id: self.context.workspace_session_id.clone(),
                            channel_id: self.context.channel_id.clone(),
                            protocol: "zmodem",
                            direction,
                            expires_at: epoch_millis_after(DECISION_TIMEOUT),
                        },
                    );
                }
                StreamEvent::ProtocolData(data) => {
                    if let Some(active) = self.active.as_mut() {
                        active.push_wire(&data);
                    } else if let Some(recovery) = self.recovery.as_mut() {
                        let remaining = MAX_RECOVERY_BUFFER.saturating_sub(recovery.buffered.len());
                        recovery
                            .buffered
                            .extend_from_slice(&data[..data.len().min(remaining)]);
                    }
                }
            }
        }

        terminal_chunks
    }

    async fn accept(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
        request_id: &str,
        selection: TerminalTransferSelection,
    ) -> Result<(), String> {
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| "当前没有等待处理的 ZMODEM 请求".to_string())?;
        if pending.id != request_id {
            return Err("ZMODEM 请求已失效".to_string());
        }
        if pending.direction != selection.direction() {
            return Err("文件选择方向与 ZMODEM 请求不匹配".to_string());
        }

        let initial_wire = self
            .mux
            .accept()
            .map_err(|_| "ZMODEM 流状态已变化".to_string())?;
        let operation_id = Uuid::new_v4().to_string();
        let active_result = match selection {
            TerminalTransferSelection::Upload { paths } => {
                UploadRuntime::new(app_handle, &self.context, operation_id, paths)
                    .await
                    .map(ActiveZmodemRuntime::Upload)
            }
            TerminalTransferSelection::Download {
                directory,
                collision_policy,
            } => DownloadRuntime::new(operation_id, PathBuf::from(directory), collision_policy)
                .map(ActiveZmodemRuntime::Download),
        };
        let active = match active_result {
            Ok(active) => active,
            Err(error) => {
                self.begin_recovery(channel, Some(request_id.to_string()), Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };

        self.pending = None;
        self.active = Some(active);
        if let Some(active) = self.active.as_mut() {
            active.push_wire(&initial_wire);
        }

        for data in self.drive_active(app_handle, channel).await? {
            let _ = app_handle.emit(&format!("ssh-data-{}", self.context.session_id), data);
        }
        Ok(())
    }

    async fn reject(
        &mut self,
        channel: &Channel<client::Msg>,
        request_id: &str,
        error: Option<String>,
    ) -> Result<(), String> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| "当前没有等待处理的 ZMODEM 请求".to_string())?;
        if pending.id != request_id {
            self.pending = Some(pending);
            return Err("ZMODEM 请求已失效".to_string());
        }
        self.mux
            .reject()
            .map_err(|_| "ZMODEM 流状态已变化".to_string())?;
        let cancel_result = channel
            .data(ZMODEM_CANCEL_SEQUENCE)
            .await
            .map_err(|value| format!("发送 ZMODEM 取消序列失败：{value}"));
        self.recovery = Some(RecoveryState {
            operation_id: None,
            request_id: Some(pending.id),
            direction: pending.direction,
            error,
            deadline: Instant::now() + RECOVERY_DELAY,
            buffered: Vec::new(),
        });
        cancel_result
    }

    async fn cancel(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
        operation_id: &str,
    ) -> Result<(), String> {
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| "当前没有进行中的 ZMODEM 传输".to_string())?;
        if active.operation_id() != operation_id {
            return Err("ZMODEM 传输任务已失效".to_string());
        }
        active
            .cancel(app_handle, &self.context, "用户取消了传输")
            .await;
        self.begin_recovery(channel, None, None).await
    }

    async fn drive_active(
        &mut self,
        app_handle: &AppHandle,
        channel: &Channel<client::Msg>,
    ) -> Result<Vec<Vec<u8>>, String> {
        let outcome_result = match self.active.as_mut() {
            Some(active) => active.drive(app_handle, &self.context, channel).await,
            None => return Ok(Vec::new()),
        };
        let outcome = match outcome_result {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Some(active) = self.active.as_mut() {
                    active.fail(app_handle, &self.context, &error).await;
                }
                self.begin_recovery(channel, None, Some(error.clone()))
                    .await?;
                return Err(error);
            }
        };

        match outcome {
            DriveOutcome::Running => Ok(Vec::new()),
            DriveOutcome::Completed { remaining_wire } => {
                let active = self.active.take().expect("active runtime disappeared");
                let operation_id = active.operation_id().to_string();
                let direction = active.direction();
                if direction == TerminalTransferDirection::Upload {
                    // A PTY may deliver the final `OO` and an immediately following CR
                    // in the same read. In that case rz consumes the CR before exiting
                    // and the shell never sees it. Keep input ownership while allowing
                    // rz a short exit window, then send one best-effort Enter.
                    tokio::time::sleep(POST_UPLOAD_ENTER_DELAY).await;
                    let _ = channel.data(&b"\r"[..]).await;
                }
                self.mux.restore_terminal();
                self.owned.store(false, Ordering::Release);
                let _ = app_handle.emit(
                    "terminal-transfer-ended",
                    TerminalTransferEnded {
                        operation_id: Some(operation_id),
                        request_id: None,
                        session_id: self.context.session_id.clone(),
                        workspace_session_id: self.context.workspace_session_id.clone(),
                        channel_id: self.context.channel_id.clone(),
                        direction,
                        terminal_restored: true,
                        error: None,
                    },
                );
                if remaining_wire.is_empty() {
                    Ok(Vec::new())
                } else {
                    let events = match self.mux.push_remote(&remaining_wire) {
                        Ok(events) => events,
                        Err(error) => {
                            // The completed operation has already released input
                            // ownership. Never leave the mux in Recovering without
                            // a matching recovery state to drive it back to Normal.
                            self.mux.restore_terminal();
                            return Err(format!("恢复终端流失败：{error:?}"));
                        }
                    };
                    Ok(self.process_passive_events(app_handle, events))
                }
            }
        }
    }

    async fn begin_recovery(
        &mut self,
        channel: &Channel<client::Msg>,
        request_id: Option<String>,
        error: Option<String>,
    ) -> Result<(), String> {
        let (operation_id, direction) = if let Some(active) = self.active.take() {
            (Some(active.operation_id().to_string()), active.direction())
        } else if let Some(pending) = self.pending.take() {
            (None, pending.direction)
        } else if let Some(recovery) = self.recovery.as_ref() {
            (recovery.operation_id.clone(), recovery.direction)
        } else {
            self.mux.restore_terminal();
            self.owned.store(false, Ordering::Release);
            return Ok(());
        };

        self.mux.begin_recovery();
        self.recovery = Some(RecoveryState {
            operation_id,
            request_id,
            direction,
            error,
            deadline: Instant::now() + RECOVERY_DELAY,
            buffered: Vec::new(),
        });
        channel
            .data(ZMODEM_CANCEL_SEQUENCE)
            .await
            .map_err(|value| format!("发送 ZMODEM 取消序列失败：{value}"))
    }

    fn finish_recovery(&mut self, app_handle: &AppHandle) -> Vec<Vec<u8>> {
        let Some(recovery) = self.recovery.take() else {
            return Vec::new();
        };
        self.mux.restore_terminal();
        self.owned.store(false, Ordering::Release);
        let _ = app_handle.emit(
            "terminal-transfer-ended",
            TerminalTransferEnded {
                operation_id: recovery.operation_id,
                request_id: recovery.request_id,
                session_id: self.context.session_id.clone(),
                workspace_session_id: self.context.workspace_session_id.clone(),
                channel_id: self.context.channel_id.clone(),
                direction: recovery.direction,
                terminal_restored: true,
                error: recovery.error,
            },
        );
        let terminal = recovery_terminal_text(&recovery.buffered);
        if terminal.is_empty() {
            Vec::new()
        } else {
            vec![terminal]
        }
    }
}

enum ActiveZmodemRuntime {
    Upload(UploadRuntime),
    Download(DownloadRuntime),
}

impl ActiveZmodemRuntime {
    fn operation_id(&self) -> &str {
        match self {
            Self::Upload(runtime) => &runtime.operation_id,
            Self::Download(runtime) => &runtime.operation_id,
        }
    }

    const fn direction(&self) -> TerminalTransferDirection {
        match self {
            Self::Upload(_) => TerminalTransferDirection::Upload,
            Self::Download(_) => TerminalTransferDirection::Download,
        }
    }

    fn push_wire(&mut self, data: &[u8]) {
        match self {
            Self::Upload(runtime) => runtime.push_wire(data),
            Self::Download(runtime) => runtime.push_wire(data),
        }
    }

    async fn drive(
        &mut self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
        channel: &Channel<client::Msg>,
    ) -> Result<DriveOutcome, String> {
        match self {
            Self::Upload(runtime) => runtime.drive(app_handle, context, channel).await,
            Self::Download(runtime) => runtime.drive(app_handle, context, channel).await,
        }
    }

    async fn cancel(&mut self, app_handle: &AppHandle, context: &RuntimeContext, reason: &str) {
        match self {
            Self::Upload(runtime) => runtime.cancel(app_handle, context, reason).await,
            Self::Download(runtime) => runtime.cancel(app_handle, context, reason).await,
        }
    }

    async fn fail(&mut self, app_handle: &AppHandle, context: &RuntimeContext, error: &str) {
        match self {
            Self::Upload(runtime) => runtime.fail(app_handle, context, error),
            Self::Download(runtime) => runtime.fail(app_handle, context, error).await,
        }
    }

    fn timeout(&mut self) -> Result<(), String> {
        match self {
            Self::Upload(runtime) => runtime.engine.timeout(),
            Self::Download(runtime) => runtime.engine.timeout(),
        }
        .map_err(|error| format!("ZMODEM 超时恢复失败：{error}"))
    }

    fn last_remote_activity(&self) -> Instant {
        match self {
            Self::Upload(runtime) => runtime.last_remote_activity,
            Self::Download(runtime) => runtime.last_remote_activity,
        }
    }

    fn last_retry(&self) -> Instant {
        match self {
            Self::Upload(runtime) => runtime.last_retry,
            Self::Download(runtime) => runtime.last_retry,
        }
    }

    fn mark_retry(&mut self) {
        match self {
            Self::Upload(runtime) => runtime.last_retry = Instant::now(),
            Self::Download(runtime) => runtime.last_retry = Instant::now(),
        }
    }
}

enum DriveOutcome {
    Running,
    Completed { remaining_wire: Vec<u8> },
}

struct UploadSpec {
    task_id: String,
    path: PathBuf,
    file_name: String,
    wire_name: Vec<u8>,
    size: u64,
    modified_unix_seconds: Option<u64>,
}

struct ActiveUploadFile {
    spec: UploadSpec,
    file: File,
    current: u64,
    read_buffer: Vec<u8>,
    last_progress_emit: Instant,
}

struct UploadRuntime {
    operation_id: String,
    engine: Sender,
    pending_files: VecDeque<UploadSpec>,
    current_file: Option<ActiveUploadFile>,
    wire_buffer: Vec<u8>,
    wire_offset: usize,
    pending_channel_write: Vec<u8>,
    session_completed: bool,
    last_remote_activity: Instant,
    last_retry: Instant,
}

impl UploadRuntime {
    async fn new(
        app_handle: &AppHandle,
        context: &RuntimeContext,
        operation_id: String,
        paths: Vec<String>,
    ) -> Result<Self, String> {
        if paths.is_empty() {
            return Err("至少选择一个上传文件".to_string());
        }

        let mut pending_files = VecDeque::new();
        for path in paths {
            let path = PathBuf::from(path);
            let metadata = tokio::fs::metadata(&path)
                .await
                .map_err(|error| format!("无法读取上传文件：{error}"))?;
            if !metadata.is_file() {
                return Err(format!("上传目标不是普通文件：{}", path.display()));
            }
            if metadata.len() > u64::from(u32::MAX) {
                return Err(format!("文件超过 ZMODEM 4 GiB 限制：{}", path.display()));
            }
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("文件名无法用于 ZMODEM：{}", path.display()))?
                .to_string();
            pending_files.push_back(UploadSpec {
                task_id: Uuid::new_v4().to_string(),
                path,
                wire_name: file_name.as_bytes().to_vec(),
                file_name,
                size: metadata.len(),
                modified_unix_seconds: metadata
                    .modified()
                    .ok()
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_secs()),
            });
        }

        let mut engine =
            Sender::new().map_err(|error| format!("初始化 ZMODEM Sender 失败：{error}"))?;
        // SSH already provides an ordered, reliable byte stream. Waiting for a
        // ZMODEM acknowledgement every small window adds one network round trip
        // without improving integrity here.
        engine.set_streaming_window(usize::MAX);

        for spec in &pending_files {
            emit_progress(
                app_handle,
                context,
                &operation_id,
                &spec.task_id,
                TerminalTransferDirection::Upload,
                &spec.file_name,
                Some(spec.path.to_string_lossy().into_owned()),
                Some(spec.file_name.clone()),
                0,
                spec.size,
                "waiting",
                "waiting",
                false,
                None,
            );
        }

        let mut runtime = Self {
            operation_id,
            engine,
            pending_files,
            current_file: None,
            wire_buffer: Vec::new(),
            wire_offset: 0,
            pending_channel_write: Vec::with_capacity(UPLOAD_CHANNEL_BATCH_SIZE),
            session_completed: false,
            last_remote_activity: Instant::now(),
            last_retry: Instant::now(),
        };
        if let Err(error) = runtime.activate_next(app_handle, context).await {
            runtime.emit_incomplete(app_handle, context, "failed", "failed", &error);
            return Err(error);
        }
        Ok(runtime)
    }

    fn push_wire(&mut self, data: &[u8]) {
        self.wire_buffer.extend_from_slice(data);
        self.last_remote_activity = Instant::now();
    }

    async fn activate_next(
        &mut self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
    ) -> Result<(), String> {
        let Some(spec) = self.pending_files.front() else {
            self.engine
                .finish()
                .map_err(|error| format!("结束 ZMODEM 上传失败：{error}"))?;
            return Ok(());
        };
        let file = File::open(&spec.path)
            .await
            .map_err(|error| format!("无法打开上传文件：{error}"))?;
        let mut info = FileInfo::new(&spec.wire_name, Some(Position::new(spec.size as u32)));
        if let Some(modified) = spec.modified_unix_seconds {
            info = info.with_modified_unix_seconds(modified);
        }
        self.engine
            .start_file(info)
            .map_err(|error| format!("启动 ZMODEM 文件发送失败：{error}"))?;
        let spec = self
            .pending_files
            .pop_front()
            .ok_or_else(|| "ZMODEM 上传队列状态异常".to_string())?;
        emit_progress(
            app_handle,
            context,
            &self.operation_id,
            &spec.task_id,
            TerminalTransferDirection::Upload,
            &spec.file_name,
            Some(spec.path.to_string_lossy().into_owned()),
            Some(spec.file_name.clone()),
            0,
            spec.size,
            "negotiating",
            "negotiating",
            false,
            None,
        );
        self.current_file = Some(ActiveUploadFile {
            spec,
            file,
            current: 0,
            read_buffer: Vec::with_capacity(1024),
            last_progress_emit: Instant::now() - PROGRESS_EMIT_INTERVAL,
        });
        Ok(())
    }

    async fn drive(
        &mut self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
        channel: &Channel<client::Msg>,
    ) -> Result<DriveOutcome, String> {
        for _ in 0..MAX_UPLOAD_DRIVER_STEPS {
            match self.engine.poll() {
                Action::WriteWire(bytes) => {
                    let output_len = bytes.len();
                    self.pending_channel_write.extend_from_slice(bytes);
                    self.engine.wire_written(output_len);
                    if self.pending_channel_write.len() >= UPLOAD_CHANNEL_BATCH_SIZE {
                        self.flush_pending_channel_write(channel).await?;
                    }
                }
                Action::ReadFile { offset, max_len } => {
                    let current = self
                        .current_file
                        .as_mut()
                        .ok_or_else(|| "ZMODEM 请求文件数据时没有活动文件".to_string())?;
                    let requested_offset = u64::from(offset.get());
                    if current.current != requested_offset {
                        current
                            .file
                            .seek(std::io::SeekFrom::Start(requested_offset))
                            .await
                            .map_err(|error| format!("定位上传文件失败：{error}"))?;
                    }
                    current.read_buffer.resize(max_len, 0);
                    current
                        .file
                        .read_exact(&mut current.read_buffer)
                        .await
                        .map_err(|error| format!("读取上传文件失败：{error}"))?;
                    self.engine
                        .submit_file(&current.read_buffer)
                        .map_err(|error| format!("提交 ZMODEM 文件数据失败：{error}"))?;
                    current.current = requested_offset.saturating_add(max_len as u64);
                    if current.last_progress_emit.elapsed() >= PROGRESS_EMIT_INTERVAL
                        || current.current >= current.spec.size
                    {
                        emit_progress(
                            app_handle,
                            context,
                            &self.operation_id,
                            &current.spec.task_id,
                            TerminalTransferDirection::Upload,
                            &current.spec.file_name,
                            Some(current.spec.path.to_string_lossy().into_owned()),
                            Some(current.spec.file_name.clone()),
                            current.current,
                            current.spec.size,
                            "transferring",
                            "sending",
                            false,
                            None,
                        );
                        current.last_progress_emit = Instant::now();
                    }
                }
                Action::Event(event) => match event {
                    Event::FileCompleted => {
                        if let Some(current) = self.current_file.take() {
                            let remote_already_complete =
                                current.spec.size > 0 && current.current < current.spec.size;
                            emit_progress(
                                app_handle,
                                context,
                                &self.operation_id,
                                &current.spec.task_id,
                                TerminalTransferDirection::Upload,
                                &current.spec.file_name,
                                Some(current.spec.path.to_string_lossy().into_owned()),
                                Some(current.spec.file_name.clone()),
                                if remote_already_complete {
                                    current.current
                                } else {
                                    current.spec.size
                                },
                                current.spec.size,
                                if remote_already_complete {
                                    "skipped"
                                } else {
                                    "success"
                                },
                                if remote_already_complete {
                                    "skipped"
                                } else {
                                    "completed"
                                },
                                false,
                                remote_already_complete.then(|| {
                                    "远端未请求文件内容，通常是同名目标已经存在".to_string()
                                }),
                            );
                        }
                        self.activate_next(app_handle, context).await?;
                    }
                    Event::FileSkipped => {
                        if let Some(current) = self.current_file.take() {
                            emit_progress(
                                app_handle,
                                context,
                                &self.operation_id,
                                &current.spec.task_id,
                                TerminalTransferDirection::Upload,
                                &current.spec.file_name,
                                Some(current.spec.path.to_string_lossy().into_owned()),
                                Some(current.spec.file_name.clone()),
                                current.current,
                                current.spec.size,
                                "skipped",
                                "skipped",
                                false,
                                Some(
                                    "远端已跳过该文件，通常是目标位置存在同名文件或目录"
                                        .to_string(),
                                ),
                            );
                        }
                        self.activate_next(app_handle, context).await?;
                    }
                    Event::SessionCompleted => self.session_completed = true,
                    Event::Aborted => return Err("远端取消了 ZMODEM 上传".to_string()),
                    Event::FileStarted(_) => {}
                    _ => {}
                },
                Action::Idle => {
                    self.flush_pending_channel_write(channel).await?;
                    if self.session_completed {
                        return Ok(DriveOutcome::Completed {
                            remaining_wire: self.take_remaining_wire(),
                        });
                    }
                    if self.wire_offset < self.wire_buffer.len() {
                        let consumed = self
                            .engine
                            .submit_wire(&self.wire_buffer[self.wire_offset..])
                            .map_err(|error| format!("解析 ZMODEM 响应失败：{error}"))?;
                        if consumed == 0 {
                            return Ok(DriveOutcome::Running);
                        }
                        self.wire_offset += consumed;
                        self.compact_wire_buffer();
                        continue;
                    }
                    return Ok(DriveOutcome::Running);
                }
                Action::WriteFile(_) => {
                    return Err("ZMODEM Sender 返回了无效的文件写入请求".to_string())
                }
                _ => {}
            }
        }
        self.flush_pending_channel_write(channel).await?;
        Ok(DriveOutcome::Running)
    }

    async fn flush_pending_channel_write(
        &mut self,
        channel: &Channel<client::Msg>,
    ) -> Result<(), String> {
        if self.pending_channel_write.is_empty() {
            return Ok(());
        }
        let output = std::mem::take(&mut self.pending_channel_write);
        channel
            .data_bytes(output)
            .await
            .map_err(|error| format!("写入 ZMODEM 协议数据失败：{error}"))?;
        self.pending_channel_write = Vec::with_capacity(UPLOAD_CHANNEL_BATCH_SIZE);
        Ok(())
    }

    async fn cancel(&mut self, app_handle: &AppHandle, context: &RuntimeContext, reason: &str) {
        self.engine.abort();
        self.emit_incomplete(app_handle, context, "cancelled", "cancelled", reason);
    }

    fn fail(&mut self, app_handle: &AppHandle, context: &RuntimeContext, error: &str) {
        self.engine.abort();
        self.emit_incomplete(app_handle, context, "failed", "failed", error);
    }

    fn emit_incomplete(
        &self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
        status: &'static str,
        phase: &'static str,
        error: &str,
    ) {
        if let Some(current) = self.current_file.as_ref() {
            emit_progress(
                app_handle,
                context,
                &self.operation_id,
                &current.spec.task_id,
                TerminalTransferDirection::Upload,
                &current.spec.file_name,
                Some(current.spec.path.to_string_lossy().into_owned()),
                Some(current.spec.file_name.clone()),
                current.current,
                current.spec.size,
                status,
                phase,
                false,
                Some(error.to_string()),
            );
        }
        for pending in &self.pending_files {
            emit_progress(
                app_handle,
                context,
                &self.operation_id,
                &pending.task_id,
                TerminalTransferDirection::Upload,
                &pending.file_name,
                Some(pending.path.to_string_lossy().into_owned()),
                Some(pending.file_name.clone()),
                0,
                pending.size,
                status,
                phase,
                false,
                Some(error.to_string()),
            );
        }
    }

    fn compact_wire_buffer(&mut self) {
        if self.wire_offset == self.wire_buffer.len() {
            self.wire_buffer.clear();
            self.wire_offset = 0;
        } else if self.wire_offset > 4096 {
            self.wire_buffer.drain(..self.wire_offset);
            self.wire_offset = 0;
        }
    }

    fn take_remaining_wire(&mut self) -> Vec<u8> {
        let remaining = self.wire_buffer[self.wire_offset..].to_vec();
        self.wire_buffer.clear();
        self.wire_offset = 0;
        remaining
    }
}

struct ActiveDownloadFile {
    task_id: String,
    file_name: String,
    target_path: PathBuf,
    temp_path: PathBuf,
    file: File,
    current: u64,
    total: u64,
    last_progress_emit: Instant,
}

struct DownloadRuntime {
    operation_id: String,
    directory: PathBuf,
    collision_policy: DownloadCollisionPolicy,
    engine: Receiver,
    current_file: Option<ActiveDownloadFile>,
    wire_buffer: Vec<u8>,
    wire_offset: usize,
    session_completed: bool,
    last_remote_activity: Instant,
    last_retry: Instant,
}

impl DownloadRuntime {
    fn new(
        operation_id: String,
        directory: PathBuf,
        collision_policy: DownloadCollisionPolicy,
    ) -> Result<Self, String> {
        let mut engine =
            Receiver::new().map_err(|error| format!("初始化 ZMODEM Receiver 失败：{error}"))?;
        engine.set_manual_file_accept(true);
        Ok(Self {
            operation_id,
            directory,
            collision_policy,
            engine,
            current_file: None,
            wire_buffer: Vec::new(),
            wire_offset: 0,
            session_completed: false,
            last_remote_activity: Instant::now(),
            last_retry: Instant::now(),
        })
    }

    fn push_wire(&mut self, data: &[u8]) {
        self.wire_buffer.extend_from_slice(data);
        self.last_remote_activity = Instant::now();
    }

    async fn drive(
        &mut self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
        channel: &Channel<client::Msg>,
    ) -> Result<DriveOutcome, String> {
        for _ in 0..MAX_DOWNLOAD_DRIVER_STEPS {
            match self.engine.poll() {
                Action::WriteWire(bytes) => {
                    let output = bytes.to_vec();
                    channel
                        .data_bytes(output.clone())
                        .await
                        .map_err(|error| format!("写入 ZMODEM 协议数据失败：{error}"))?;
                    self.engine.wire_written(output.len());
                }
                Action::WriteFile(bytes) => {
                    let output = bytes.to_vec();
                    let current = self
                        .current_file
                        .as_mut()
                        .ok_or_else(|| "收到 ZMODEM 文件数据时没有活动文件".to_string())?;
                    current
                        .file
                        .write_all(&output)
                        .await
                        .map_err(|error| format!("写入下载临时文件失败：{error}"))?;
                    self.engine
                        .file_written(output.len())
                        .map_err(|error| format!("确认 ZMODEM 文件写入失败：{error}"))?;
                    current.current = current.current.saturating_add(output.len() as u64);
                    if current.last_progress_emit.elapsed() >= PROGRESS_EMIT_INTERVAL
                        || (current.total > 0 && current.current >= current.total)
                    {
                        emit_progress(
                            app_handle,
                            context,
                            &self.operation_id,
                            &current.task_id,
                            TerminalTransferDirection::Download,
                            &current.file_name,
                            Some(current.target_path.to_string_lossy().into_owned()),
                            Some(current.file_name.clone()),
                            current.current,
                            current.total,
                            "transferring",
                            "receiving",
                            false,
                            None,
                        );
                        current.last_progress_emit = Instant::now();
                    }
                }
                Action::Event(event) => match event {
                    Event::FileStarted(info) => {
                        if self.current_file.is_some() {
                            return Err("远端在上一文件完成前启动了新文件".to_string());
                        }
                        let remote_name = info.name.to_vec();
                        let total = info.size.map(Position::get).unwrap_or(0) as u64;
                        let DownloadTarget {
                            display_name,
                            target_path,
                            temp_path,
                            file,
                        } = prepare_download_target(
                            &self.directory,
                            &remote_name,
                            self.collision_policy,
                            &self.operation_id,
                        )
                        .await?;
                        let task_id = Uuid::new_v4().to_string();
                        emit_progress(
                            app_handle,
                            context,
                            &self.operation_id,
                            &task_id,
                            TerminalTransferDirection::Download,
                            &display_name,
                            Some(target_path.to_string_lossy().into_owned()),
                            Some(display_name.clone()),
                            0,
                            total,
                            "negotiating",
                            "preparing",
                            false,
                            None,
                        );
                        self.current_file = Some(ActiveDownloadFile {
                            task_id,
                            file_name: display_name,
                            target_path,
                            temp_path,
                            file,
                            current: 0,
                            total,
                            last_progress_emit: Instant::now() - PROGRESS_EMIT_INTERVAL,
                        });
                        self.engine
                            .accept_file_at(0)
                            .map_err(|error| format!("接受 ZMODEM 下载文件失败：{error}"))?;
                    }
                    Event::FileCompleted => self.complete_current(app_handle, context).await?,
                    Event::SessionCompleted => self.session_completed = true,
                    Event::Aborted => return Err("远端取消了 ZMODEM 下载".to_string()),
                    _ => {}
                },
                Action::Idle => {
                    if self.session_completed {
                        return Ok(DriveOutcome::Completed {
                            remaining_wire: self.take_remaining_wire(),
                        });
                    }
                    if self.wire_offset < self.wire_buffer.len() {
                        let consumed = self
                            .engine
                            .submit_wire(&self.wire_buffer[self.wire_offset..])
                            .map_err(|error| format!("解析 ZMODEM 数据失败：{error}"))?;
                        if consumed == 0 {
                            return Ok(DriveOutcome::Running);
                        }
                        self.wire_offset += consumed;
                        self.compact_wire_buffer();
                        continue;
                    }
                    return Ok(DriveOutcome::Running);
                }
                Action::ReadFile { .. } => {
                    return Err("ZMODEM Receiver 返回了无效的文件读取请求".to_string())
                }
                _ => {}
            }
        }
        Ok(DriveOutcome::Running)
    }

    async fn complete_current(
        &mut self,
        app_handle: &AppHandle,
        context: &RuntimeContext,
    ) -> Result<(), String> {
        let Some(current) = self.current_file.take() else {
            return Err("ZMODEM 文件完成事件缺少活动文件".to_string());
        };
        let ActiveDownloadFile {
            task_id,
            file_name,
            target_path,
            temp_path,
            mut file,
            current,
            total,
            ..
        } = current;
        let target_path_text = target_path.to_string_lossy().into_owned();
        let emit_state = |status, phase, error| {
            emit_progress(
                app_handle,
                context,
                &self.operation_id,
                &task_id,
                TerminalTransferDirection::Download,
                &file_name,
                Some(target_path_text.clone()),
                Some(file_name.clone()),
                current,
                total,
                status,
                phase,
                false,
                error,
            );
        };

        emit_state("finalizing", "committing", None);
        if total > 0 && current != total {
            let error = format!("ZMODEM 文件长度不一致：预期 {total}，实际 {current}");
            drop(file);
            cleanup_temp_file(&temp_path).await;
            emit_state("failed", "failed", Some(error.clone()));
            return Err(error);
        }
        let finalize_result = async {
            file.flush()
                .await
                .map_err(|error| format!("刷新下载临时文件失败：{error}"))?;
            file.sync_all()
                .await
                .map_err(|error| format!("同步下载临时文件失败：{error}"))
        }
        .await;
        drop(file);
        if let Err(error) = finalize_result {
            cleanup_temp_file(&temp_path).await;
            emit_state("failed", "failed", Some(error.clone()));
            return Err(error);
        }
        let committed_path = match commit_download(
            temp_path.clone(),
            target_path,
            self.collision_policy,
            &file_name,
        )
        .await
        {
            Ok(path) => path,
            Err(error) => {
                cleanup_temp_file(&temp_path).await;
                emit_state("failed", "failed", Some(error.clone()));
                return Err(error);
            }
        };
        emit_progress(
            app_handle,
            context,
            &self.operation_id,
            &task_id,
            TerminalTransferDirection::Download,
            &file_name,
            Some(committed_path.to_string_lossy().into_owned()),
            Some(file_name.clone()),
            current,
            total,
            "success",
            "completed",
            false,
            None,
        );
        Ok(())
    }

    async fn cancel(&mut self, app_handle: &AppHandle, context: &RuntimeContext, reason: &str) {
        let _ = self.engine.abort();
        if let Some(current) = self.current_file.take() {
            drop(current.file);
            cleanup_temp_file(&current.temp_path).await;
            emit_progress(
                app_handle,
                context,
                &self.operation_id,
                &current.task_id,
                TerminalTransferDirection::Download,
                &current.file_name,
                Some(current.target_path.to_string_lossy().into_owned()),
                Some(current.file_name.clone()),
                current.current,
                current.total,
                "cancelled",
                "cancelled",
                false,
                Some(reason.to_string()),
            );
        }
    }

    async fn fail(&mut self, app_handle: &AppHandle, context: &RuntimeContext, error: &str) {
        let _ = self.engine.abort();
        if let Some(current) = self.current_file.take() {
            drop(current.file);
            cleanup_temp_file(&current.temp_path).await;
            emit_progress(
                app_handle,
                context,
                &self.operation_id,
                &current.task_id,
                TerminalTransferDirection::Download,
                &current.file_name,
                Some(current.target_path.to_string_lossy().into_owned()),
                Some(current.file_name.clone()),
                current.current,
                current.total,
                "failed",
                "failed",
                false,
                Some(error.to_string()),
            );
        }
    }

    fn compact_wire_buffer(&mut self) {
        if self.wire_offset == self.wire_buffer.len() {
            self.wire_buffer.clear();
            self.wire_offset = 0;
        } else if self.wire_offset > 4096 {
            self.wire_buffer.drain(..self.wire_offset);
            self.wire_offset = 0;
        }
    }

    fn take_remaining_wire(&mut self) -> Vec<u8> {
        let remaining = self.wire_buffer[self.wire_offset..].to_vec();
        self.wire_buffer.clear();
        self.wire_offset = 0;
        remaining
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    app_handle: &AppHandle,
    context: &RuntimeContext,
    operation_id: &str,
    task_id: &str,
    direction: TerminalTransferDirection,
    file_name: &str,
    local_path: Option<String>,
    remote_path: Option<String>,
    current: u64,
    total: u64,
    status: &'static str,
    phase: &'static str,
    terminal_restored: bool,
    error: Option<String>,
) {
    let percent = if total > 0 {
        ((current.min(total) as f64 / total as f64) * 100.0) as u8
    } else if status == "success" {
        100
    } else {
        0
    };
    let _ = app_handle.emit(
        "transfer-progress",
        TransferProgressPayload {
            operation_id: operation_id.to_string(),
            task_id: task_id.to_string(),
            session_id: context.session_id.clone(),
            workspace_session_id: context.workspace_session_id.clone(),
            channel_id: context.channel_id.clone(),
            protocol: "zmodem",
            direction,
            file_name: file_name.to_string(),
            local_path,
            remote_path,
            current,
            total,
            percent,
            status,
            phase,
            terminal_restored,
            error,
        },
    );
}

fn map_direction(direction: CoreDirection) -> TerminalTransferDirection {
    match direction {
        CoreDirection::Upload => TerminalTransferDirection::Upload,
        CoreDirection::Download => TerminalTransferDirection::Download,
    }
}

fn epoch_millis_after(duration: Duration) -> u64 {
    SystemTime::now()
        .checked_add(duration)
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn recovery_terminal_text(data: &[u8]) -> Vec<u8> {
    let mut probe = ZmodemProbe::new();
    let output = probe.inspect(data);
    let mut terminal = output.terminal_data;
    if output.detection.is_none() {
        terminal.extend(probe.flush());
    }
    terminal
        .into_iter()
        .filter(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0x1b | 0x20..=0x7e) || *byte >= 0x80)
        .collect()
}
