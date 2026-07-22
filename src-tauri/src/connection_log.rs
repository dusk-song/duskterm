use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
    let Some(path) = log_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    let Ok(_guard) = LOG_WRITE_LOCK.get_or_init(|| Mutex::new(())).lock() else {
        return;
    };
    if create_dir_all(parent).is_err() {
        return;
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
            return;
        }
        let session_id = sanitize_field(session_id);
        let event = sanitize_field(event.as_ref());
        let _ = writeln!(file, "{} [{}] {}", timestamp_ms, session_id, event);
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
