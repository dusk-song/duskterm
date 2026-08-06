use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri::PhysicalSize;

static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

mod background;
mod connection_log;
mod fileio;
mod local_terminal;
mod native_drag;
mod session;
mod sftp;
mod ssh;
mod ssh_algorithms;
mod storage;
mod terminal_transfer;
mod tunnel;

#[tauri::command]
async fn shutdown_app(app_handle: tauri::AppHandle) -> Result<(), String> {
    if SHUTDOWN_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let supervisor = app_handle.state::<session::supervisor::SessionSupervisor>();
    let sftp_state = app_handle.state::<sftp::SftpAppState>();
    let tunnel_state = app_handle.state::<tunnel::TunnelState>();
    let result = supervisor
        .disconnect_all(sftp_state.inner().clone(), tunnel_state.inner().clone())
        .await;
    app_handle.exit(0);
    result
}

#[tauri::command]
async fn exit_app(app_handle: tauri::AppHandle) -> Result<(), String> {
    shutdown_app(app_handle).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(session::supervisor::SessionSupervisor::new())
        .manage(ssh::SshAppState::new())
        .manage(sftp::SftpAppState::new())
        .manage(tunnel::TunnelState::new())
        .manage(Arc::new(Mutex::new(storage::StorageState::new())))
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_min_size(Some(PhysicalSize::new(460, 250)));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if !SHUTDOWN_STARTED.load(Ordering::SeqCst) {
                    let app_handle = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = shutdown_app(app_handle).await;
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            exit_app,
            background::import_background_image,
            background::ensure_background_image,
            background::delete_background_image,
            local_terminal::list_local_shell_profiles,
            native_drag::native_drag_capabilities,
            native_drag::start_native_local_file_drag,
            native_drag::start_native_sftp_file_drag,
            ssh::connect_ssh,
            ssh::test_ssh_connection,
            ssh::list_serial_ports,
            ssh::write_ssh,
            ssh::resize_ssh,
            ssh::open_ssh_shell_channel,
            ssh::write_ssh_shell_channel,
            ssh::resize_ssh_shell_channel,
            ssh::close_ssh_shell_channel,
            storage::load_sessions,
            storage::clear_recent_sessions,
            storage::trim_recent_sessions,
            storage::save_session,
            storage::delete_session,
            storage::get_decrypted_session,
            storage::load_command_knowledge,
            storage::save_command_knowledge_entry,
            storage::delete_command_knowledge_entry,
            storage::replace_command_knowledge_entries,
            storage::export_command_knowledge_to,
            storage::import_command_knowledge_from,
            storage::list_tunnel_configs,
            storage::save_tunnel_config,
            storage::delete_tunnel_config,
            storage::duplicate_tunnel_config,
            storage::delete_tunnel_configs_by_session,
            storage::load_toolbar_layout,
            storage::save_toolbar_layout,
            storage::export_sessions_to,
            storage::import_sessions_from,
            ssh::disconnect_ssh,
            tunnel::start_tunnel,
            tunnel::start_tunnel_from_config,
            tunnel::stop_tunnel,
            tunnel::list_tunnels,
            tunnel::stop_all_tunnels,
            ssh::confirm_hostkey,
            sftp::connect_sftp,
            sftp::confirm_sftp_hostkey,
            sftp::sftp_ls,
            sftp::sftp_ls_paged,
            sftp::sftp_read_file,
            sftp::sftp_open_text_file,
            sftp::sftp_write_file,
            sftp::sftp_save_text_file,
            sftp::sftp_download_file,
            sftp::sftp_upload_file,
            sftp::sftp_cancel_transfer,
            sftp::sftp_disconnect,
            sftp::sftp_is_connected,
            sftp::sftp_exists,
            sftp::sftp_mkdir,
            sftp::sftp_rename,
            sftp::sftp_remove,
            sftp::sftp_chmod,
            sftp::sftp_stat,
            fileio::save_text_file,
            fileio::save_binary_file,
            fileio::append_binary_file,
            fileio::inspect_local_drop_paths,
            fileio::import_desktop_pet_asset
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
