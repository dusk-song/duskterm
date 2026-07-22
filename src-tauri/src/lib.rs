use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri::PhysicalSize;

mod fileio;
mod background;
mod connection_log;
mod remote_monitor;
mod session;
mod sftp;
mod ssh;
mod ssh_algorithms;
mod storage;
mod system;
mod terminal_transfer;
mod tunnel;

#[tauri::command]
fn exit_app() {
    std::process::exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(session::supervisor::SessionSupervisor::new())
        .manage(ssh::SshAppState::new())
        .manage(sftp::SftpAppState::new())
        .manage(system::SystemState::new())
        .manage(tunnel::TunnelState::new())
        .manage(Arc::new(Mutex::new(storage::StorageState::new())))
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_min_size(Some(PhysicalSize::new(460, 250)));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            exit_app,
            background::import_background_image,
            background::ensure_background_image,
            background::delete_background_image,
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
            system::get_system_stats,
            fileio::save_text_file,
            fileio::save_binary_file,
            fileio::append_binary_file,
            fileio::import_desktop_pet_asset,
            remote_monitor::get_remote_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
