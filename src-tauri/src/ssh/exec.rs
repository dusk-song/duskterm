//! Non-interactive POSIX execution on a dedicated channel. Never writes to a PTY.
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Execution {
    pub outcome: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
    pub bytes_seen: u64,
    pub truncated: bool,
    pub encoding_replaced: bool,
    pub output_complete: bool,
    pub end_reason: String,
    pub elapsed_ms: u64,
}

use futures::future::BoxFuture;
use russh::{client, Channel, ChannelMsg, Sig};
use std::time::{Duration, Instant};

pub(crate) const OUTPUT_LIMIT: usize = 262_144;
pub(crate) const EXEC_TIMEOUT: u64 = 30;

#[derive(Clone)]
pub(crate) struct ExecOptions {
    pub timeout: Duration,
    pub output_limit: usize,
}
impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(EXEC_TIMEOUT),
            output_limit: OUTPUT_LIMIT,
        }
    }
}

pub(crate) fn shell_command(command: &str, cwd: Option<&str>) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\"'\"'"));
    let script = cwd.map_or_else(
        || command.to_string(),
        |cwd| format!("cd {} || exit $?\n{command}", quote(cwd)),
    );
    format!("exec /bin/sh -c {}", quote(&script))
}

struct ChannelGuard {
    channel: Option<Channel<client::Msg>>,
    attempted: bool,
}
impl Drop for ChannelGuard {
    fn drop(&mut self) {
        let Some(channel) = self.channel.take() else {
            return;
        };
        let attempted = self.attempted;
        tokio::spawn(async move {
            if attempted {
                let _ =
                    tokio::time::timeout(Duration::from_secs(1), channel.signal(Sig::TERM)).await;
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), channel.close()).await;
        });
    }
}

struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    state: Execution,
    limit: usize,
}
impl Output {
    fn append(&mut self, stderr: bool, data: &[u8]) {
        self.state.bytes_seen = self.state.bytes_seen.saturating_add(data.len() as u64);
        let remaining = self
            .limit
            .saturating_sub(self.stdout.len() + self.stderr.len());
        let take = data.len().min(remaining);
        if stderr {
            self.stderr.extend_from_slice(&data[..take]);
        } else {
            self.stdout.extend_from_slice(&data[..take]);
        }
        self.state.truncated |= take < data.len();
    }
    fn snapshot(&self) -> Execution {
        let stdout = String::from_utf8_lossy(&self.stdout);
        let stderr = String::from_utf8_lossy(&self.stderr);
        let mut state = self.state.clone();
        state.encoding_replaced = matches!(stdout, std::borrow::Cow::Owned(_))
            || matches!(stderr, std::borrow::Cow::Owned(_));
        state.stdout = stdout.into_owned();
        state.stderr = stderr.into_owned();
        state
    }
}

pub(crate) async fn execute<F>(
    channel: Channel<client::Msg>,
    command: &str,
    cwd: Option<&str>,
    options: ExecOptions,
    mut report: F,
) -> Result<Execution, String>
where
    F: FnMut(Execution, bool) -> BoxFuture<'static, Result<(), String>>,
{
    let mut guard = ChannelGuard {
        channel: Some(channel),
        attempted: false,
    };
    let mut output = Output {
        stdout: Vec::new(),
        stderr: Vec::new(),
        state: Execution {
            outcome: "dispatching".into(),
            ..Default::default()
        },
        limit: options.output_limit,
    };
    // A durable 'dispatching' record precedes the only place issuing SSH exec.
    report(output.snapshot(), true).await?;
    guard.attempted = true;
    let execution_started = Instant::now();
    let outcome = tokio::time::timeout(options.timeout, async {
        guard
            .channel
            .as_ref()
            .unwrap()
            .exec(true, shell_command(command, cwd))
            .await
            .map_err(|_| "执行请求发送失败")?;
        guard
            .channel
            .as_ref()
            .unwrap()
            .eof()
            .await
            .map_err(|_| "执行输入通道关闭失败")?;
        let mut last = Instant::now();
        while let Some(message) = guard.channel.as_mut().unwrap().wait().await {
            match message {
                ChannelMsg::Success => output.state.outcome = "running".into(),
                ChannelMsg::Failure => {
                    if output.state.bytes_seen == 0 && output.state.outcome == "dispatching" {
                        output.state.outcome = "rejected".into();
                        guard.attempted = false;
                    }
                    output.state.end_reason = "rejected".into();
                    return Err("服务器拒绝执行请求");
                }
                ChannelMsg::Data { data } => output.append(false, &data),
                ChannelMsg::ExtendedData { ext: 1, data } => output.append(true, &data),
                ChannelMsg::ExitStatus { exit_status } => {
                    output.state.exit_code = Some(exit_status);
                    guard.attempted = false;
                }
                ChannelMsg::ExitSignal { signal_name, .. } => {
                    output.state.signal = Some(format!("{signal_name:?}"));
                    guard.attempted = false;
                }
                ChannelMsg::Close => {
                    output.state.output_complete = true;
                    output.state.end_reason = "channel_closed".into();
                    break;
                }
                _ => {}
            }
            if last.elapsed() >= Duration::from_millis(100) {
                output.state.elapsed_ms = execution_started.elapsed().as_millis() as u64;
                report(output.snapshot(), false)
                    .await
                    .map_err(|_| "输出接收端已关闭")?;
                last = Instant::now();
            }
        }
        Ok::<(), &str>(())
    })
    .await;
    output.state.elapsed_ms = execution_started.elapsed().as_millis() as u64;
    if output.state.end_reason.is_empty() {
        output.state.end_reason = match &outcome {
            Err(_) => "timeout",
            Ok(Err(_)) => "transport_error",
            _ => "stream_closed",
        }
        .into();
    }
    if output.state.exit_code.is_some() || output.state.signal.is_some() {
        output.state.outcome = "exited".into();
    } else if output.state.outcome != "rejected" {
        output.state.outcome = "unknown".into();
    }
    if !matches!(outcome, Ok(Ok(()))) {
        output.state.output_complete = false;
    }
    Ok(output.snapshot())
}
