use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::session::state::ManagedSshRuntime;
use crate::ssh::{
    create_terminal_runtime_channels, SessionCloseReceiver, SessionIoReceiver,
    SessionResizeReceiver, SshConfig,
};

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(8);
const MAX_OUTPUT_BATCH_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalShellProfile {
    pub id: String,
    pub name: String,
    pub executable: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalShellProfileResult {
    pub platform: String,
    pub default_profile_id: String,
    pub profiles: Vec<LocalShellProfile>,
}

#[derive(Clone, Debug)]
pub struct LocalTerminalConfig {
    profile_id: String,
    executable: PathBuf,
    args: Vec<String>,
    working_directory: PathBuf,
    initial_cols: u16,
    initial_rows: u16,
}

enum ReaderMessage {
    Data(Vec<u8>),
    Eof,
    Error(String),
}

fn append_output_chunk(output_batch: &mut Vec<u8>, data: Vec<u8>) -> Option<Vec<u8>> {
    let ready =
        if !output_batch.is_empty() && output_batch.len() + data.len() > MAX_OUTPUT_BATCH_BYTES {
            Some(std::mem::take(output_batch))
        } else {
            None
        };
    output_batch.extend_from_slice(&data);
    ready
}

#[cfg(windows)]
struct ProcessTreeGuard {
    job: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl ProcessTreeGuard {
    fn attach(process_id: u32) -> Result<Self, String> {
        use std::ptr::null;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() || job == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "无法创建本地终端 Job Object：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("无法配置本地终端 Job Object：{error}"));
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id);
            if process.is_null() || process == INVALID_HANDLE_VALUE {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("无法打开本地 Shell 进程：{error}"));
            }
            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            if assigned == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("无法管理本地 Shell 进程树：{error}"));
            }
            Ok(Self { job })
        }
    }

    fn terminate(&self, _force: bool) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

#[cfg(unix)]
struct ProcessTreeGuard {
    process_group: libc::pid_t,
}

#[cfg(unix)]
impl ProcessTreeGuard {
    fn attach(process_id: u32) -> Result<Self, String> {
        let process_group = libc::pid_t::try_from(process_id)
            .map_err(|_| format!("无效的本地 Shell 进程 ID：{process_id}"))?;
        Ok(Self { process_group })
    }

    fn terminate(&self, force: bool) {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        unsafe {
            libc::kill(-self.process_group, signal);
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.terminate(true);
    }
}

fn stop_process_tree(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    process_tree: &ProcessTreeGuard,
) -> Result<Option<u32>, String> {
    process_tree.terminate(false);
    let graceful_deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < graceful_deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status.exit_code())),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("无法读取本地 Shell 状态：{error}")),
        }
    }

    process_tree.terminate(true);
    let _ = child.kill();
    let force_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < force_deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status.exit_code())),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("无法读取本地 Shell 状态：{error}")),
        }
    }
    Err("本地 Shell 进程树未能在超时前退出".to_string())
}

#[cfg(windows)]
fn windows_system_root() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

#[cfg(windows)]
fn windows_powershell_path() -> PathBuf {
    windows_system_root()
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[cfg(windows)]
fn windows_cmd_path() -> PathBuf {
    std::env::var_os("COMSPEC")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| windows_system_root().join("System32").join("cmd.exe"))
}

#[cfg(windows)]
fn shell_profiles() -> LocalShellProfileResult {
    let powershell = windows_powershell_path();
    let cmd = windows_cmd_path();
    LocalShellProfileResult {
        platform: "windows".to_string(),
        default_profile_id: "powershell".to_string(),
        profiles: vec![
            LocalShellProfile {
                id: "powershell".to_string(),
                name: "PowerShell".to_string(),
                executable: powershell.to_string_lossy().into_owned(),
            },
            LocalShellProfile {
                id: "cmd".to_string(),
                name: "CMD".to_string(),
                executable: cmd.to_string_lossy().into_owned(),
            },
        ],
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_absolute()
        && path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(unix)]
fn account_login_shell() -> Option<PathBuf> {
    use std::ffi::CStr;

    unsafe {
        let mut passwd = std::mem::zeroed::<libc::passwd>();
        let mut result = std::ptr::null_mut();
        let suggested = libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX);
        let mut buffer = vec![
            0u8;
            if suggested > 0 {
                suggested as usize
            } else {
                16 * 1024
            }
        ];
        if libc::getpwuid_r(
            libc::geteuid(),
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        ) != 0
            || result.is_null()
            || passwd.pw_shell.is_null()
        {
            return None;
        }
        let path = PathBuf::from(
            CStr::from_ptr(passwd.pw_shell)
                .to_string_lossy()
                .into_owned(),
        );
        is_executable_file(&path).then_some(path)
    }
}

#[cfg(unix)]
fn fallback_unix_shell() -> PathBuf {
    #[cfg(target_os = "macos")]
    let candidates = ["/bin/zsh", "/bin/bash", "/bin/sh"];
    #[cfg(not(target_os = "macos"))]
    let candidates = ["/bin/bash", "/bin/sh", "/usr/bin/sh"];

    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| is_executable_file(path))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

#[cfg(unix)]
fn default_unix_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| is_executable_file(path))
        .or_else(account_login_shell)
        .unwrap_or_else(fallback_unix_shell)
}

#[cfg(unix)]
fn shell_display_name(path: &Path) -> String {
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Shell");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Shell".to_string(),
    }
}

#[cfg(unix)]
fn shell_profiles() -> LocalShellProfileResult {
    let shell = default_unix_shell();
    LocalShellProfileResult {
        platform: if cfg!(target_os = "macos") {
            "macos".to_string()
        } else {
            "linux".to_string()
        },
        default_profile_id: "default".to_string(),
        profiles: vec![LocalShellProfile {
            id: "default".to_string(),
            name: shell_display_name(&shell),
            executable: shell.to_string_lossy().into_owned(),
        }],
    }
}

#[tauri::command]
pub fn list_local_shell_profiles() -> Result<LocalShellProfileResult, String> {
    let profiles = shell_profiles();
    if profiles.profiles.iter().any(|profile| {
        let path = Path::new(&profile.executable);
        #[cfg(unix)]
        {
            !is_executable_file(path)
        }
        #[cfg(windows)]
        {
            !path.is_absolute() || !path.is_file()
        }
    }) {
        return Err("未找到可用的系统 Shell".to_string());
    }
    Ok(profiles)
}

pub fn resolve_local_config(config: &SshConfig) -> Result<LocalTerminalConfig, String> {
    let profiles = list_local_shell_profiles()?;
    let requested = config
        .local_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&profiles.default_profile_id);
    let profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.id == requested)
        .ok_or_else(|| format!("不支持的本地 Shell：{requested}"))?;
    let executable = PathBuf::from(&profile.executable);
    if !executable.is_file() {
        return Err(format!("未找到本地 Shell：{}", executable.display()));
    }

    let working_directory = config
        .local_working_directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .ok_or_else(|| "无法确定当前用户主目录".to_string())?;
    if !working_directory.is_dir() {
        return Err(format!(
            "本地终端工作目录不存在：{}",
            working_directory.display()
        ));
    }

    let mut args = Vec::new();
    #[cfg(windows)]
    if profile.id == "powershell" {
        args.push("-NoLogo".to_string());
    }
    #[cfg(unix)]
    {
        let file_name = executable
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match file_name.as_str() {
            "bash" | "fish" => args.push("--login".to_string()),
            "zsh" => args.push("-l".to_string()),
            _ => {}
        }
    }

    Ok(LocalTerminalConfig {
        profile_id: profile.id.clone(),
        executable,
        args,
        working_directory,
        initial_cols: config.initial_cols.unwrap_or(DEFAULT_COLS).clamp(2, 1000),
        initial_rows: config.initial_rows.unwrap_or(DEFAULT_ROWS).clamp(2, 1000),
    })
}

pub async fn connect_local_terminal_runtime(
    app_handle: AppHandle,
    session_id: String,
    config: LocalTerminalConfig,
) -> Result<ManagedSshRuntime, String> {
    let (handle, input_rx, resize_rx, close_rx) = create_terminal_runtime_channels();
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::task::spawn_blocking(move || {
        run_local_terminal(
            app_handle, session_id, config, input_rx, resize_rx, close_rx, ready_tx,
        );
    });

    match ready_rx.await {
        Ok(Ok(())) => Ok(ManagedSshRuntime {
            handle,
            task: Some(task),
        }),
        Ok(Err(error)) => {
            let _ = task.await;
            Err(error)
        }
        Err(_) => {
            let _ = task.await;
            Err("本地终端在启动完成前意外退出".to_string())
        }
    }
}

fn run_local_terminal(
    app_handle: AppHandle,
    session_id: String,
    config: LocalTerminalConfig,
    mut input_rx: SessionIoReceiver,
    mut resize_rx: SessionResizeReceiver,
    mut close_rx: SessionCloseReceiver,
    ready_tx: oneshot::Sender<Result<(), String>>,
) {
    let startup = (|| -> Result<_, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: config.initial_rows,
                cols: config.initial_cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("无法创建本地 PTY：{error}"))?;

        let mut command = CommandBuilder::new(&config.executable);
        command.args(&config.args);
        command.cwd(&config.working_directory);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "DuskTerm");

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("无法启动 {}：{error}", config.profile_id))?;
        let process_id = child
            .process_id()
            .ok_or_else(|| "无法获取本地 Shell 进程 ID".to_string())?;
        let process_tree = match ProcessTreeGuard::attach(process_id) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("无法读取本地 PTY：{error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("无法写入本地 PTY：{error}"))?;

        Ok((pair.master, child, reader, writer, process_tree))
    })();

    let (master, mut child, mut reader, mut writer, process_tree) = match startup {
        Ok(runtime) => {
            let _ = ready_tx.send(Ok(()));
            runtime
        }
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };

    let _ = app_handle.emit(&format!("ssh-connected-{session_id}"), ());

    let (reader_message_tx, reader_message_rx) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    let _ = reader_message_tx.send(ReaderMessage::Eof);
                    break;
                }
                Ok(read) => {
                    if reader_message_tx
                        .send(ReaderMessage::Data(buffer[..read].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = reader_message_tx.send(ReaderMessage::Error(error.to_string()));
                    break;
                }
            }
        }
    });

    let mut terminal_error = None;
    let mut reader_outcome = None;
    let exit_code = loop {
        if close_rx.try_recv().is_ok() {
            break match stop_process_tree(&mut child, &process_tree) {
                Ok(exit_code) => exit_code,
                Err(error) => {
                    terminal_error = Some(error);
                    None
                }
            };
        }

        while let Ok(data) = input_rx.try_recv() {
            if data.is_empty() {
                continue;
            }
            if let Err(error) = writer.write_all(&data).and_then(|_| writer.flush()) {
                terminal_error = Some(format!("本地终端写入失败：{error}"));
                break;
            }
        }
        if terminal_error.is_some() {
            break stop_process_tree(&mut child, &process_tree).ok().flatten();
        }

        let mut latest_size = None;
        while let Ok((cols, rows)) = resize_rx.try_recv() {
            latest_size = Some((cols, rows));
        }
        if let Some((cols, rows)) = latest_size {
            let size = PtySize {
                rows: rows.clamp(1, 1000) as u16,
                cols: cols.clamp(1, 1000) as u16,
                pixel_width: 0,
                pixel_height: 0,
            };
            if let Err(error) = master.resize(size) {
                terminal_error = Some(format!("本地终端调整尺寸失败：{error}"));
                break stop_process_tree(&mut child, &process_tree).ok().flatten();
            }
        }

        let mut output_batch = Vec::new();
        loop {
            match reader_message_rx.try_recv() {
                Ok(ReaderMessage::Data(data)) => {
                    if let Some(ready) = append_output_chunk(&mut output_batch, data) {
                        let _ = app_handle.emit(&format!("ssh-data-{session_id}"), ready);
                    }
                }
                Ok(ReaderMessage::Eof) => {
                    reader_outcome = Some(ReaderMessage::Eof);
                    break;
                }
                Ok(ReaderMessage::Error(error)) => {
                    reader_outcome = Some(ReaderMessage::Error(error));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if !output_batch.is_empty() {
            let _ = app_handle.emit(&format!("ssh-data-{session_id}"), output_batch);
        }

        match child.try_wait() {
            Ok(Some(status)) => break Some(status.exit_code()),
            Ok(None) => {}
            Err(error) => {
                terminal_error = Some(format!("无法读取本地 Shell 状态：{error}"));
                break stop_process_tree(&mut child, &process_tree).ok().flatten();
            }
        }

        match reader_outcome.take() {
            Some(ReaderMessage::Eof) => match child.try_wait() {
                Ok(Some(status)) => break Some(status.exit_code()),
                Ok(None) => {
                    reader_outcome = Some(ReaderMessage::Eof);
                }
                Err(error) => {
                    terminal_error = Some(format!("无法读取本地 Shell 状态：{error}"));
                    break stop_process_tree(&mut child, &process_tree).ok().flatten();
                }
            },
            Some(ReaderMessage::Error(error)) => {
                terminal_error = Some(format!("本地终端读取失败：{error}"));
                break stop_process_tree(&mut child, &process_tree).ok().flatten();
            }
            Some(ReaderMessage::Data(_)) | None => {}
        }

        thread::sleep(IO_POLL_INTERVAL);
    };

    drop(child);
    drop(writer);
    drop(master);
    drop(process_tree);
    let _ = reader_thread.join();

    if let Some(error) = terminal_error {
        let _ = app_handle.emit(&format!("ssh-error-{session_id}"), error);
    }
    let reason = exit_code
        .map(|code| format!("本地 Shell 已退出，退出码 {code}"))
        .unwrap_or_else(|| "本地 Shell 已关闭".to_string());
    let _ = app_handle.emit(&format!("ssh-closed-{session_id}"), reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_only_expose_supported_shells() {
        let result = list_local_shell_profiles().expect("profiles should resolve");
        assert!(!result.profiles.is_empty());
        #[cfg(windows)]
        assert_eq!(
            result
                .profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["powershell", "cmd"]
        );
        #[cfg(unix)]
        assert_eq!(result.profiles.len(), 1);
    }

    #[test]
    fn resolved_working_directory_defaults_to_home() {
        let config = SshConfig {
            protocol: Some("local".to_string()),
            host: String::new(),
            port: 0,
            username: String::new(),
            password: None,
            private_key_path: None,
            passphrase: None,
            connect_timeout: None,
            keep_alive_interval: None,
            term_type: None,
            login_script: None,
            jump_host: None,
            jump_port: None,
            jump_username: None,
            jump_auth_type: None,
            jump_password: None,
            jump_private_key_path: None,
            jump_passphrase: None,
            serial_path: None,
            baud_rate: None,
            data_bits: None,
            stop_bits: None,
            parity: None,
            flow_control: None,
            local_profile: None,
            local_working_directory: None,
            initial_cols: None,
            initial_rows: None,
        };

        let resolved = resolve_local_config(&config).expect("config should resolve");
        assert!(resolved.working_directory.is_dir());
    }

    #[test]
    fn output_batch_keeps_chunks_within_the_target_size() {
        let mut batch = vec![1; 10 * 1024];
        let ready = append_output_chunk(&mut batch, vec![2; 8 * 1024])
            .expect("the existing batch should be ready");

        assert_eq!(ready.len(), 10 * 1024);
        assert_eq!(batch.len(), 8 * 1024);
        assert!(ready.iter().all(|byte| *byte == 1));
        assert!(batch.iter().all(|byte| *byte == 2));
    }

    #[test]
    fn output_batch_combines_small_chunks_without_reordering() {
        let mut batch = vec![1, 2];
        let ready = append_output_chunk(&mut batch, vec![3, 4]);

        assert!(ready.is_none());
        assert_eq!(batch, vec![1, 2, 3, 4]);
    }

    #[test]
    fn native_pty_runs_supported_shell() {
        let profiles = list_local_shell_profiles().expect("profiles should resolve");
        let profile = profiles
            .profiles
            .first()
            .expect("at least one shell profile");
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .expect("native pty should open");
        let mut command = CommandBuilder::new(&profile.executable);
        #[cfg(windows)]
        command.args(["-NoLogo", "-NoProfile"]);

        let mut child = pair
            .slave
            .spawn_command(command)
            .expect("shell should start in pty");
        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("pty reader should clone");
        let mut writer = pair
            .master
            .take_writer()
            .expect("pty writer should be acquired");
        let (output_tx, output_rx) = mpsc::channel();
        let output_thread = thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if output_tx.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        drop(pair.slave);
        let mut output = output_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("pty should produce initial output");
        if output.windows(4).any(|window| window == b"\x1b[6n") {
            writer
                .write_all(b"\x1b[1;1R")
                .expect("terminal status response should be writable");
            writer
                .flush()
                .expect("terminal status response should flush");
        }
        #[cfg(windows)]
        writer
            .write_all(b"Write-Output ('DUSKTERM_' + 'PTY_OK')\rexit\r")
            .expect("commands should be writable");
        #[cfg(unix)]
        writer
            .write_all(b"printf DUSKTERM_PTY_OK\nexit\n")
            .expect("commands should be writable");
        writer.flush().expect("commands should flush");
        while !String::from_utf8_lossy(&output).contains("DUSKTERM_PTY_OK") {
            output.extend(
                output_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("pty should produce output"),
            );
        }
        child.kill().expect("shell should be killable");
        let status = child.wait().expect("shell should exit");
        drop(child);
        drop(writer);
        drop(pair.master);
        let _ = output_thread.join();
        let output = String::from_utf8_lossy(&output);

        assert!(
            !status.success(),
            "smoke test intentionally terminates the shell"
        );
        assert!(
            output.contains("DUSKTERM_PTY_OK"),
            "unexpected pty output: {output}"
        );
    }
}
