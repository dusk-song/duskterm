use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

// Logging is best effort; a slow disk must never stall the window or SSH runtime.
const LOG_QUEUE_CAPACITY: usize = 1024;
static LOG_SENDER: OnceLock<Option<SyncSender<String>>> = OnceLock::new();
static DROPPED_LOGS: AtomicUsize = AtomicUsize::new(0);

fn enqueue(sender: &SyncSender<String>, line: String) -> bool {
    sender.try_send(line).is_ok()
}

fn start_writer() -> Option<SyncSender<String>> {
    let path = log_path()?;
    let (sender, receiver) = sync_channel::<String>(LOG_QUEUE_CAPACITY);
    std::thread::Builder::new()
        .name("connection-log".into())
        .spawn(move || {
            for line in receiver {
                let Some(parent) = path.parent() else {
                    continue;
                };
                if create_dir_all(parent).is_err() {
                    continue;
                }
                let mut options = OpenOptions::new();
                options.create(true).append(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                if let Ok(mut file) = options.open(&path) {
                    if enforce_private_permissions(&path).is_err() {
                        continue;
                    }
                    let dropped = DROPPED_LOGS.swap(0, Ordering::Relaxed);
                    if dropped > 0 {
                        let _ = writeln!(file, "connection-log dropped_entries={dropped}");
                    }
                    let _ = writeln!(file, "{line}");
                }
            }
        })
        .ok()?;
    Some(sender)
}

fn log_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".duskterm")
            .join("sessions")
            .join("ssh-connections.log"),
    )
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(unix)]
fn enforce_private_permissions(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    if metadata.permissions().mode() & 0o777 != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_private_permissions(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

pub fn append(session_id: &str, event: impl AsRef<str>) {
    let Some(sender) = LOG_SENDER.get_or_init(start_writer) else {
        return;
    };
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let line = format!(
        "{} [{}] {}",
        timestamp_ms,
        sanitize_field(session_id),
        sanitize_field(event.as_ref())
    );
    if !enqueue(sender, line) {
        DROPPED_LOGS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn describe_payload(data: &[u8]) -> &'static str {
    if data.is_empty() {
        "empty"
    } else if data.iter().all(|byte| byte.is_ascii_control()) {
        "control"
    } else if data.iter().all(|byte| !byte.is_ascii_control()) {
        "text"
    } else {
        "mixed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_writer_cannot_block_callers_or_grow_the_queue() {
        let (sender, receiver) = sync_channel(1);
        let (finished, completion) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            assert!(enqueue(&sender, "first".into()));
            for _ in 0..10_000 {
                assert!(!enqueue(&sender, "overflow".into()));
            }
            finished.send(()).unwrap();
        });
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("a stalled consumer must not block logging");
        assert_eq!(receiver.recv().unwrap(), "first");
        worker.join().unwrap();
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn disconnected_writer_and_control_characters_are_handled() {
        let (sender, receiver) = sync_channel(1);
        drop(receiver);
        assert!(!enqueue(&sender, "closed".into()));
        assert_eq!(sanitize_field("one\r\ntwo\t"), "one  two ");
    }
}
