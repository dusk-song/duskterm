use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::{remote_monitor, sftp, ssh, tunnel};

use super::{
    message::SessionMessage,
    state::{LifecycleState, SessionRuntimeState, SharedHostkeyDecision},
};

#[derive(Clone)]
pub struct SessionActorHandle {
    pub sender: UnboundedSender<SessionMessage>,
}

pub fn spawn_session_actor(
    session_id: String,
    pending_ssh_hostkey: SharedHostkeyDecision,
    pending_sftp_hostkey: SharedHostkeyDecision,
) -> SessionActorHandle {
    let (sender, mut receiver) = unbounded_channel::<SessionMessage>();
    let actor_sender = sender.clone();

    tokio::spawn(async move {
        let mut runtime_state =
            SessionRuntimeState::with_hostkey_decisions(pending_ssh_hostkey, pending_sftp_hostkey);

        while let Some(message) = receiver.recv().await {
            match message {
                SessionMessage::Connect {
                    app_handle,
                    sftp_state,
                    config,
                    respond_to,
                } => {
                    runtime_state.lifecycle = LifecycleState::Starting;
                    runtime_state.terminal.attached = true;

                    let result = ssh::connect_ssh_runtime(
                        app_handle,
                        sftp_state,
                        runtime_state.security.pending_ssh_hostkey.clone(),
                        session_id.clone(),
                        config,
                    )
                    .await;

                    let response = match result {
                        Ok(runtime) => {
                            runtime_state.ssh = Some(runtime);
                            runtime_state.lifecycle = LifecycleState::Active;
                            Ok(session_id.clone())
                        }
                        Err(error) => {
                            runtime_state.lifecycle = LifecycleState::Error;
                            Err(error)
                        }
                    };
                    let _ = respond_to.send(response);
                }
                SessionMessage::WriteTerminal { data, respond_to } => {
                    let result = runtime_state
                        .ssh
                        .as_ref()
                        .ok_or_else(|| "Session not connected".to_string())
                        .and_then(|runtime| ssh::write_ssh_runtime(&runtime.handle, data));
                    let _ = respond_to.send(result);
                }
                SessionMessage::ResizeTerminal {
                    cols,
                    rows,
                    respond_to,
                } => {
                    let result = runtime_state
                        .ssh
                        .as_ref()
                        .ok_or_else(|| "Session not connected".to_string())
                        .and_then(|runtime| ssh::resize_ssh_runtime(&runtime.handle, cols, rows));
                    let _ = respond_to.send(result);
                }
                SessionMessage::OpenShellChannel {
                    app_handle,
                    channel_id,
                    term_type,
                    login_script,
                    respond_to,
                } => {
                    let result = if runtime_state.shell_channels.contains_key(&channel_id) {
                        Err("Shell channel already exists".to_string())
                    } else if let Some(root_runtime) = runtime_state.ssh.as_ref() {
                        ssh::open_shared_shell_channel_runtime(
                            app_handle,
                            &root_runtime.handle,
                            channel_id.clone(),
                            term_type,
                            login_script,
                        )
                        .await
                        .map(|runtime| {
                            runtime_state.shell_channels.insert(channel_id, runtime);
                        })
                    } else {
                        Err("Root SSH session is not connected".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::WriteShellChannel { channel_id, data, respond_to } => {
                    let result = runtime_state.shell_channels.get(&channel_id)
                        .ok_or_else(|| "Shell channel not found".to_string())
                        .and_then(|runtime| ssh::write_ssh_runtime(&runtime.handle, data));
                    let _ = respond_to.send(result);
                }
                SessionMessage::ResizeShellChannel { channel_id, cols, rows, respond_to } => {
                    let result = runtime_state.shell_channels.get(&channel_id)
                        .ok_or_else(|| "Shell channel not found".to_string())
                        .and_then(|runtime| ssh::resize_ssh_runtime(&runtime.handle, cols, rows));
                    let _ = respond_to.send(result);
                }
                SessionMessage::CloseShellChannel { channel_id, respond_to } => {
                    let result = if let Some(mut runtime) = runtime_state.shell_channels.remove(&channel_id) {
                        let _ = runtime.handle.close_tx.send(());
                        if let Some(task) = runtime.task.take() { let _ = task.await; }
                        Ok(())
                    } else {
                        Ok(())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::Disconnect {
                    sftp_state,
                    tunnel_state,
                    respond_to,
                } => {
                    runtime_state.lifecycle = LifecycleState::Closed;
                    runtime_state.terminal.attached = false;
                    runtime_state.transfers.cancel_all();
                    let sftp_handle = runtime_state.sftp.take();
                    runtime_state.sftp = None;
                    let ssh_runtime = runtime_state.ssh.take();
                    let child_runtimes = std::mem::take(&mut runtime_state.shell_channels);
                    for (_, mut runtime) in child_runtimes {
                        let _ = runtime.handle.close_tx.send(());
                        if let Some(task) = runtime.task.take() { let _ = task.await; }
                    }
                    let tunnel_result =
                        tunnel::stop_all_runtime_tunnels(&tunnel_state, &mut runtime_state.tunnels)
                            .await;
                    let sftp_result =
                        sftp::disconnect_sftp_runtime(sftp_handle, session_id.clone()).await;
                    let ssh_result =
                        ssh::disconnect_ssh_runtime(ssh_runtime, &sftp_state, session_id.clone())
                            .await;
                    let result = match (sftp_result, ssh_result, tunnel_result) {
                        (Err(error), _, _) => Err(error),
                        (Ok(_), Err(error), _) => Err(error),
                        (Ok(_), Ok(_), Err(error)) => Err(error),
                        (Ok(_), Ok(_), Ok(_)) => Ok(()),
                    };
                    let _ = respond_to.send(result);
                    break;
                }
                SessionMessage::ConnectSftp {
                    app_handle,
                    sftp_state,
                    config,
                    respond_to,
                } => {
                    let result = sftp::connect_sftp_runtime(
                        app_handle,
                        sftp_state,
                        runtime_state
                            .ssh
                            .as_ref()
                            .map(|runtime| runtime.handle.shared_session.clone()),
                        runtime_state.security.pending_sftp_hostkey.clone(),
                        session_id.clone(),
                        config,
                    )
                    .await;
                    let response = match result {
                        Ok(runtime) => {
                            runtime_state.sftp = Some(runtime);
                            Ok(())
                        }
                        Err(error) => Err(error),
                    };
                    let _ = respond_to.send(response);
                }
                SessionMessage::DisconnectSftp { respond_to } => {
                    runtime_state.transfers.cancel_all();
                    let sftp_handle = runtime_state.sftp.take();
                    runtime_state.sftp = None;
                    let result =
                        sftp::disconnect_sftp_runtime(sftp_handle, session_id.clone()).await;
                    let _ = respond_to.send(result);
                }
                SessionMessage::GetRemoteStats { respond_to } => {
                    let result = if let Some(handle) = runtime_state.ssh.as_ref() {
                        remote_monitor::get_remote_stats_runtime(&handle.handle.shared_session)
                            .await
                    } else {
                        Err("SSH session not available for monitoring".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::IsSftpConnected { respond_to } => {
                    let _ = respond_to.send(Ok(runtime_state.sftp.is_some()));
                }
                SessionMessage::ListSftpDir { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_ls_runtime(&runtime.state, runtime, session_id.clone(), path)
                            .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::ListSftpDirPaged {
                    path,
                    offset,
                    limit,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_ls_paged_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                            offset,
                            limit,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::ReadSftpFile { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_read_file_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::OpenSftpTextFile { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_open_text_file_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::WriteSftpFile {
                    path,
                    content,
                    expected_modified,
                    expected_size,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_write_file_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                            content,
                            expected_modified,
                            expected_size,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::SaveSftpTextFile {
                    path,
                    content,
                    expected_cas_token,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_save_text_file_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                            content,
                            expected_cas_token,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::SftpExists { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_exists_runtime(&runtime.state, runtime, session_id.clone(), path)
                            .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::MkdirSftp { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_mkdir_runtime(&runtime.state, runtime, session_id.clone(), path)
                            .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::RenameSftp {
                    from_path,
                    to_path,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_rename_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            from_path,
                            to_path,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::RemoveSftp {
                    path,
                    is_dir,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_remove_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                            is_dir,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::ChmodSftp {
                    path,
                    permissions,
                    respond_to,
                } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_chmod_runtime(
                            &runtime.state,
                            runtime,
                            session_id.clone(),
                            path,
                            permissions,
                        )
                        .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::StatSftp { path, respond_to } => {
                    let result = if let Some(runtime) = runtime_state.sftp.as_ref() {
                        sftp::sftp_stat_runtime(&runtime.state, runtime, session_id.clone(), path)
                            .await
                    } else {
                        Err("SFTP Session not found".to_string())
                    };
                    let _ = respond_to.send(result);
                }
                SessionMessage::StartSftpDownload {
                    window,
                    remote_path,
                    local_path,
                    req_id,
                    respond_to,
                } => {
                    let transfer_session_id = session_id.clone();
                    let transfer_runtime = runtime_state.sftp.as_ref().map(|runtime| {
                        (
                            runtime.state.clone(),
                            runtime.handle.sftp.clone(),
                            runtime.handle.reused_from_ssh,
                            runtime.handle.connection_config.clone(),
                        )
                    });
                    let pending_sftp_hostkey = runtime_state.security.pending_sftp_hostkey.clone();
                    let transfers = runtime_state.transfers.clone();
                    tokio::spawn(async move {
                        let result = if let Some((
                            runtime_state,
                            sftp_handle,
                            reused_from_ssh,
                            connection_config,
                        )) = transfer_runtime
                        {
                            sftp::sftp_download_file_runtime(
                                window,
                                &runtime_state,
                                sftp_handle,
                                reused_from_ssh,
                                connection_config,
                                pending_sftp_hostkey,
                                transfers,
                                transfer_session_id,
                                remote_path,
                                local_path,
                                req_id,
                            )
                            .await
                        } else {
                            Err("SFTP Session not found".to_string())
                        };
                        let _ = respond_to.send(result);
                    });
                }
                SessionMessage::StartSftpUpload {
                    window,
                    local_path,
                    remote_path,
                    req_id,
                    respond_to,
                } => {
                    let transfer_session_id = session_id.clone();
                    let transfer_runtime = runtime_state.sftp.as_ref().map(|runtime| {
                        (
                            runtime.state.clone(),
                            runtime.handle.sftp.clone(),
                            runtime.handle.reused_from_ssh,
                            runtime.handle.connection_config.clone(),
                        )
                    });
                    let pending_sftp_hostkey = runtime_state.security.pending_sftp_hostkey.clone();
                    let transfers = runtime_state.transfers.clone();
                    tokio::spawn(async move {
                        let result = if let Some((
                            runtime_state,
                            sftp_handle,
                            reused_from_ssh,
                            connection_config,
                        )) = transfer_runtime
                        {
                            sftp::sftp_upload_file_runtime(
                                window,
                                &runtime_state,
                                sftp_handle,
                                reused_from_ssh,
                                connection_config,
                                pending_sftp_hostkey,
                                transfers,
                                transfer_session_id,
                                local_path,
                                remote_path,
                                req_id,
                            )
                            .await
                        } else {
                            Err("SFTP Session not found".to_string())
                        };
                        let _ = respond_to.send(result);
                    });
                }
                SessionMessage::CancelSftpTransfer { req_id, respond_to } => {
                    let result = runtime_state.transfers.cancel(&req_id);
                    let _ = respond_to.send(result);
                }
                SessionMessage::StartTunnel {
                    app_handle,
                    tunnel_state,
                    request,
                    respond_to,
                } => {
                    let result = tunnel::start_tunnel_runtime(
                        app_handle,
                        &tunnel_state,
                        &mut runtime_state.tunnels,
                        runtime_state
                            .ssh
                            .as_ref()
                            .map(|runtime| runtime.handle.shared_session.clone()),
                        runtime_state.security.pending_ssh_hostkey.clone(),
                        request,
                        None,
                    )
                    .await;
                    let _ = respond_to.send(result);
                }
                SessionMessage::StartTunnelFromConfig {
                    app_handle,
                    tunnel_state,
                    storage_state,
                    config_id,
                    respond_to,
                } => {
                    let result = tunnel::start_tunnel_from_config_runtime(
                        app_handle,
                        &tunnel_state,
                        &mut runtime_state.tunnels,
                        runtime_state
                            .ssh
                            .as_ref()
                            .map(|runtime| runtime.handle.shared_session.clone()),
                        runtime_state.security.pending_ssh_hostkey.clone(),
                        &storage_state,
                        config_id,
                    )
                    .await;
                    let _ = respond_to.send(result);
                }
                SessionMessage::StopTunnel {
                    tunnel_state,
                    tunnel_id,
                    respond_to,
                } => {
                    let result = tunnel::stop_tunnel_runtime(
                        &tunnel_state,
                        &mut runtime_state.tunnels,
                        &tunnel_id,
                    )
                    .await;
                    let _ = respond_to.send(result);
                }
            }
        }
    });

    SessionActorHandle {
        sender: actor_sender,
    }
}
