use serde::Serialize;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod windows_virtual;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDragCapabilities {
    pub local_files: bool,
    pub local_directories: bool,
    pub remote_virtual_files: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDragOutcome {
    pub dropped: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
}

#[tauri::command]
pub fn native_drag_capabilities() -> NativeDragCapabilities {
    NativeDragCapabilities {
        local_files: cfg!(target_os = "windows"),
        local_directories: cfg!(target_os = "windows"),
        remote_virtual_files: cfg!(target_os = "windows"),
    }
}

#[tauri::command]
pub async fn start_native_local_file_drag(
    app: tauri::AppHandle,
    window: tauri::Window,
    paths: Vec<String>,
) -> Result<NativeDragOutcome, String> {
    let paths = validate_local_paths(paths)?;
    start_local_file_drag(app, window, paths).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_native_sftp_file_drag(
    app: tauri::AppHandle,
    window: tauri::Window,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    session_id: String,
    remote_path: String,
    file_name: String,
    size: u64,
    req_id: String,
) -> Result<NativeDragOutcome, String> {
    if session_id.is_empty() || remote_path.is_empty() || file_name.is_empty() || req_id.is_empty()
    {
        return Err("远程文件拖动参数不完整".to_string());
    }

    let bridge = supervisor
        .start_sftp_drag_download(
            window,
            session_id.clone(),
            remote_path,
            req_id.clone(),
            size,
        )
        .await?;

    let cancel = bridge.cancel.clone();
    let result = start_sftp_file_drag(app, session_id.clone(), file_name, bridge).await;
    if result
        .as_ref()
        .map(|outcome| !outcome.dropped)
        .unwrap_or(true)
    {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = supervisor.cancel_sftp_transfer(session_id, req_id).await;
    }
    result
}

fn validate_local_paths(paths: Vec<String>) -> Result<Vec<PathBuf>, String> {
    if paths.is_empty() {
        return Err("没有可拖动的本地文件".to_string());
    }

    paths
        .into_iter()
        .map(|raw_path| {
            let path = PathBuf::from(raw_path);
            if !path.is_absolute() {
                return Err(format!("拖动路径必须是绝对路径: {}", path.display()));
            }
            if !path.exists() {
                return Err(format!("拖动路径不存在: {}", path.display()));
            }
            Ok(path)
        })
        .collect()
}

#[cfg(target_os = "windows")]
async fn start_local_file_drag(
    app: tauri::AppHandle,
    window: tauri::Window,
    paths: Vec<PathBuf>,
) -> Result<NativeDragOutcome, String> {
    use std::sync::mpsc;

    let (start_tx, start_rx) = mpsc::channel();
    let (outcome_tx, outcome_rx) = mpsc::channel();

    app.run_on_main_thread(move || {
        let result = drag::start_drag(
            &window,
            drag::DragItem::Files(paths),
            drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec()),
            move |result, cursor_position| {
                let _ = outcome_tx.send(NativeDragOutcome {
                    dropped: matches!(result, drag::DragResult::Dropped),
                    cursor_x: cursor_position.x,
                    cursor_y: cursor_position.y,
                });
            },
            drag::Options::default(),
        );
        let _ = start_tx.send(result.map_err(|error| error.to_string()));
    })
    .map_err(|error| format!("无法在窗口线程启动拖动: {error}"))?;

    start_rx
        .recv()
        .map_err(|_| "本地文件拖动线程意外结束".to_string())??;

    outcome_rx
        .recv()
        .map_err(|_| "未收到本地文件拖动结果".to_string())
}

#[cfg(not(target_os = "windows"))]
async fn start_local_file_drag(
    _app: tauri::AppHandle,
    _window: tauri::Window,
    _paths: Vec<PathBuf>,
) -> Result<NativeDragOutcome, String> {
    Err("当前平台尚未启用原生文件拖出".to_string())
}

#[cfg(target_os = "windows")]
async fn start_sftp_file_drag(
    app: tauri::AppHandle,
    session_id: String,
    file_name: String,
    bridge: crate::sftp::SftpStreamBridge,
) -> Result<NativeDragOutcome, String> {
    use std::sync::mpsc;

    let (result_tx, result_rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let result = windows_virtual::start_virtual_file_drag(session_id, file_name, bridge).map(
            |outcome| NativeDragOutcome {
                dropped: outcome.dropped,
                cursor_x: outcome.cursor_x,
                cursor_y: outcome.cursor_y,
            },
        );
        let _ = result_tx.send(result);
    })
    .map_err(|error| format!("无法在窗口线程启动远程文件拖动: {error}"))?;

    result_rx
        .recv()
        .map_err(|_| "远程文件拖动线程意外结束".to_string())?
}

#[cfg(not(target_os = "windows"))]
async fn start_sftp_file_drag(
    _app: tauri::AppHandle,
    _session_id: String,
    _file_name: String,
    _bridge: crate::sftp::SftpStreamBridge,
) -> Result<NativeDragOutcome, String> {
    Err("当前平台尚未启用远程文件拖出".to_string())
}
