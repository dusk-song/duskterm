//! Runs an approved CMD line in a separate Windows process tree.
use crate::{
    local_terminal::{windows_system_root, ProcessTreeGuard},
    ssh::exec::{ExecOptions, Execution, Output},
};
use futures::future::BoxFuture;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Component, Path, Prefix},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Globalization::{MultiByteToWideChar, CP_OEMCP, MB_ERR_INVALID_CHARS},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        Threading::{
            GetProcessIdOfThread, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
            THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

pub(super) fn validate_directory(directory: &Path) -> Result<(), String> {
    let displayable = directory.to_str().is_some_and(|path| {
        path.len() <= 4096
            && !path.chars().any(|c| {
                c.is_control()
                    || ('\u{202a}'..='\u{202e}').contains(&c)
                    || ('\u{2066}'..='\u{2069}').contains(&c)
            })
    });
    if !displayable
        || !directory.is_absolute()
        || !matches!(directory.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || !directory.is_dir()
    {
        return Err("CMD 诊断需要可用的本地盘符启动目录".into());
    }
    Ok(())
}

fn resume_initial_thread(pid: u32) -> io::Result<()> {
    // The primary thread is the only thread in our suspended CMD process.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw_handle(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                );
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = OwnedHandle::from_raw_handle(thread);
                if GetProcessIdOfThread(thread.as_raw_handle()) == pid
                    && ResumeThread(thread.as_raw_handle()) == 1
                {
                    return Ok(());
                }
                break;
            }
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
        }
    }
    Err(io::Error::other("无法恢复受管理的 CMD 进程"))
}

fn start(directory: &Path, command: &str) -> Result<(Child, ProcessTreeGuard), String> {
    let executable = windows_system_root().join("System32").join("cmd.exe");
    let child = Command::new(executable)
        .args(["/d", "/s", "/c"])
        .raw_arg(format!("\"{command}\""))
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("无法启动 CMD：{error}"))?;
    let pid = child.id().ok_or("无法获取 CMD 进程 ID")?;
    // Attach while suspended so no command or child process can escape enrollment.
    let tree = ProcessTreeGuard::attach(pid)?;
    resume_initial_thread(pid).map_err(|error| error.to_string())?;
    Ok((child, tree))
}

fn decode(bytes: &[u8]) -> (String, bool) {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.to_owned(), false);
    }
    // Some CMD built-ins and Windows utilities use the system OEM code page.
    let mut wide = vec![0; bytes.len()];
    let length = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            wide.len() as i32,
        )
    };
    if length > 0 {
        if let Ok(text) = String::from_utf16(&wide[..length as usize]) {
            return (text, false);
        }
    }
    (String::from_utf8_lossy(bytes).into_owned(), true)
}

pub(super) async fn execute<F>(
    directory: &Path,
    command: &str,
    options: ExecOptions,
    mut report: F,
) -> Result<Execution, String>
where
    F: FnMut(Execution, bool) -> BoxFuture<'static, Result<(), String>>,
{
    validate_directory(directory)?;
    let mut output = Output::new(options.output_limit);
    report(output.state.clone(), true).await?;
    let started = Instant::now();
    let (mut child, tree) = match start(directory, command) {
        Ok(process) => process,
        Err(error) => {
            return Ok(Execution {
                outcome: "rejected".into(),
                stderr: error,
                end_reason: "spawn_failed".into(),
                ..Default::default()
            })
        }
    };
    output.state.outcome = "running".into();
    let mut stdout = child.stdout.take().expect("CMD stdout is piped");
    let mut stderr = child.stderr.take().expect("CMD stderr is piped");
    let mut stdout_buf = [0; 8192];
    let mut stderr_buf = [0; 8192];
    let (mut stdout_done, mut stderr_done, mut exited) = (false, false, false);
    let mut progress = tokio::time::interval(Duration::from_millis(100));
    progress.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let outcome = tokio::time::timeout(options.timeout, async {
        while !(stdout_done && stderr_done && exited) {
            tokio::select! {
                result = stdout.read(&mut stdout_buf), if !stdout_done => {
                    let count = result.map_err(|_| "无法读取 CMD 标准输出")?;
                    stdout_done = count == 0;
                    output.append(false, &stdout_buf[..count]);
                }
                result = stderr.read(&mut stderr_buf), if !stderr_done => {
                    let count = result.map_err(|_| "无法读取 CMD 错误输出")?;
                    stderr_done = count == 0;
                    output.append(true, &stderr_buf[..count]);
                }
                status = child.wait(), if !exited => {
                    output.state.exit_code = status.map_err(|_| "无法取得 CMD 退出状态")?.code().map(|code| code as u32);
                    exited = true;
                    output.state.outcome = "exited".into();
                    output.state.output_complete = stdout_done && stderr_done;
                    output.state.elapsed_ms = started.elapsed().as_millis() as u64;
                    report(output.snapshot_with(decode), true).await.map_err(|_| "CMD 输出接收端已关闭")?;
                }
                _ = progress.tick() => {
                    output.state.elapsed_ms = started.elapsed().as_millis() as u64;
                    report(output.snapshot_with(decode), false).await.map_err(|_| "CMD 输出接收端已关闭")?;
                }
            }
        }
        Ok::<(), &str>(())
    }).await;
    // Closing the job also cleans up managed descendants on timeout or future drop.
    drop(tree);
    output.state.elapsed_ms = started.elapsed().as_millis() as u64;
    output.state.output_complete = stdout_done && stderr_done;
    output.state.end_reason = match outcome {
        Err(_) => "timeout",
        Ok(Err(_)) => "process_io_error",
        Ok(Ok(())) => "process_exited",
    }
    .into();
    output.state.outcome = if exited { "exited" } else { "unknown" }.into();
    Ok(output.snapshot_with(decode))
}
