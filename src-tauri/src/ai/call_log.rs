use super::types::CallRecord;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

// An allowlisted DTO is serialized, never an HTTP request/response or SDK error.
pub struct CallLog {
    path: Option<PathBuf>,
    lock: Mutex<()>,
}
impl CallLog {
    pub fn new(database: &Path) -> Self {
        let path = if database == Path::new(":memory:") {
            None
        } else {
            database
                .parent()
                .map(|p| p.join("logs").join("ai-calls.jsonl"))
        };
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
    pub fn write(&self, record: &CallRecord) {
        if self.append(record).is_err() {
            eprintln!(
                "AI diagnostic log write failed; check local directory permissions and disk space"
            );
        }
    }
    fn append(&self, record: &CallRecord) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _guard = self
            .lock
            .lock()
            .map_err(|_| std::io::Error::other("AI log lock unavailable"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::metadata(path).is_ok_and(|m| m.len() >= 4 * 1024 * 1024) {
            let previous = path.with_file_name("ai-calls.previous.jsonl");
            match fs::remove_file(&previous) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            fs::rename(path, previous)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")
    }
}

pub(super) struct PendingCall {
    pub record: CallRecord,
    log: Arc<CallLog>,
}
impl Drop for PendingCall {
    fn drop(&mut self) {
        if self.record.status == "running" {
            self.record.status = "interrupted".into();
            self.record.elapsed_ms = super::types::now().saturating_sub(self.record.started_at);
            self.log.write(&self.record);
        }
    }
}
impl CallLog {
    pub fn guard(self: &Arc<Self>, record: CallRecord) -> PendingCall {
        PendingCall {
            record,
            log: self.clone(),
        }
    }
}
