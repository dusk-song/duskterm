use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_HISTORY_LIMIT: usize = 1000;
const MAX_HISTORY_LIMIT: usize = 5000;
const MAX_COMMAND_CHARS: usize = 4096;
const MAX_SOURCE_CHARS: usize = 64;
const MAX_PROTOCOL_CHARS: usize = 32;
const MAX_HOST_CHARS: usize = 255;
const MAX_USERNAME_CHARS: usize = 255;

pub struct CommandHistoryState {
    connection: Mutex<Connection>,
}

pub type SharedCommandHistoryState = Arc<CommandHistoryState>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandHistoryEntry {
    id: i64,
    cmd: String,
    count: u64,
    last_used_at: u64,
}

impl CommandHistoryState {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut connection = Connection::open(path).map_err(|error| error.to_string())?;
        configure_connection(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn in_memory() -> Result<Self, String> {
        let mut connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
        configure_connection(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "cache_size", -2048_i64)
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "wal_autocheckpoint", 1000_i64)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "Command history database schema {} is newer than supported schema {}",
            version, SCHEMA_VERSION
        ));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    if version == 0 {
        transaction
            .execute_batch(
                "
                CREATE TABLE command_history (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    command      TEXT NOT NULL,
                    normalized   TEXT NOT NULL,
                    use_count    INTEGER NOT NULL DEFAULT 1 CHECK(use_count >= 1),
                    last_used_at INTEGER NOT NULL,
                    created_at   INTEGER NOT NULL,
                    source       TEXT NOT NULL DEFAULT 'terminal',
                    protocol     TEXT,
                    host         TEXT,
                    username     TEXT,
                    scope_key    TEXT NOT NULL DEFAULT 'global',
                    UNIQUE(scope_key, command)
                );
                CREATE INDEX idx_command_history_recent
                    ON command_history(scope_key, last_used_at DESC);
                CREATE INDEX idx_command_history_normalized
                    ON command_history(scope_key, normalized);
                ",
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn normalize_scope_key(scope_key: Option<String>) -> String {
    let value = scope_key.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        "global".to_string()
    } else {
        value.chars().take(256).collect()
    }
}

fn normalize_metadata(value: Option<String>, max_chars: usize) -> Option<String> {
    let value: String = value?.trim().chars().take(max_chars).collect();
    (!value.is_empty()).then_some(value)
}

fn validate_command(command: String) -> Result<String, String> {
    let command = command.trim().to_string();
    if command.is_empty() {
        return Err("Command history entry cannot be empty".to_string());
    }
    if command.chars().count() > MAX_COMMAND_CHARS {
        return Err(format!(
            "Command history entry exceeds {} characters",
            MAX_COMMAND_CHARS
        ));
    }
    if command.contains(['\r', '\n']) {
        return Err("Multiline commands are not stored in command history".to_string());
    }
    if command.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("Command history entry contains control characters".to_string());
    }
    Ok(command)
}

fn current_timestamp_ms() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .map_err(|error| error.to_string())
}

fn clamp_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .clamp(1, MAX_HISTORY_LIMIT)
}

fn query_entry(
    connection: &Connection,
    scope_key: &str,
    command: &str,
) -> Result<CommandHistoryEntry, String> {
    connection
        .query_row(
            "SELECT id, command, use_count, last_used_at
             FROM command_history
             WHERE scope_key = ?1 AND command = ?2",
            params![scope_key, command],
            |row| {
                Ok(CommandHistoryEntry {
                    id: row.get(0)?,
                    cmd: row.get(1)?,
                    count: row.get::<_, i64>(2)?.max(0) as u64,
                    last_used_at: row.get::<_, i64>(3)?.max(0) as u64,
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn trim_history_tx(
    transaction: &Transaction<'_>,
    scope_key: &str,
    max: usize,
) -> Result<usize, String> {
    transaction
        .execute(
            "DELETE FROM command_history
             WHERE scope_key = ?1
               AND id NOT IN (
                   SELECT id FROM command_history
                   WHERE scope_key = ?1
                   ORDER BY last_used_at DESC, id DESC
                   LIMIT ?2
               )",
            params![scope_key, max as i64],
        )
        .map_err(|error| error.to_string())
}

fn load_entries(
    connection: &Connection,
    scope_key: &str,
    limit: usize,
) -> Result<Vec<CommandHistoryEntry>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, command, use_count, last_used_at
             FROM (
                 SELECT id, command, use_count, last_used_at
                 FROM command_history
                 WHERE scope_key = ?1
                 ORDER BY last_used_at DESC, id DESC
                 LIMIT ?2
             )
             ORDER BY last_used_at ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![scope_key, limit as i64], |row| {
            Ok(CommandHistoryEntry {
                id: row.get(0)?,
                cmd: row.get(1)?,
                count: row.get::<_, i64>(2)?.max(0) as u64,
                last_used_at: row.get::<_, i64>(3)?.max(0) as u64,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn load_command_history(
    state: tauri::State<'_, SharedCommandHistoryState>,
    scope_key: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<CommandHistoryEntry>, String> {
    let state = state.inner().clone();
    let scope_key = normalize_scope_key(scope_key);
    let limit = clamp_limit(limit);
    tokio::task::spawn_blocking(move || {
        let connection = state
            .connection
            .lock()
            .map_err(|_| "Command history database lock is poisoned".to_string())?;
        load_entries(&connection, &scope_key, limit)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn record_command_history(
    state: tauri::State<'_, SharedCommandHistoryState>,
    command: String,
    source: Option<String>,
    protocol: Option<String>,
    host: Option<String>,
    username: Option<String>,
    scope_key: Option<String>,
    max: Option<usize>,
) -> Result<CommandHistoryEntry, String> {
    let state = state.inner().clone();
    let command = validate_command(command)?;
    let normalized = command.to_lowercase();
    let scope_key = normalize_scope_key(scope_key);
    let source =
        normalize_metadata(source, MAX_SOURCE_CHARS).unwrap_or_else(|| "terminal".to_string());
    let protocol = normalize_metadata(protocol, MAX_PROTOCOL_CHARS);
    let host = normalize_metadata(host, MAX_HOST_CHARS);
    let username = normalize_metadata(username, MAX_USERNAME_CHARS);
    let max = clamp_limit(max);
    tokio::task::spawn_blocking(move || {
        let mut connection = state
            .connection
            .lock()
            .map_err(|_| "Command history database lock is poisoned".to_string())?;
        let timestamp = current_timestamp_ms()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO command_history (
                    command, normalized, use_count, last_used_at, created_at,
                    source, protocol, host, username, scope_key
                 ) VALUES (?1, ?2, 1, ?3, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(scope_key, command) DO UPDATE SET
                    normalized = excluded.normalized,
                    use_count = command_history.use_count + 1,
                    last_used_at = excluded.last_used_at,
                    source = excluded.source,
                    protocol = excluded.protocol,
                    host = excluded.host,
                    username = excluded.username",
                params![
                    command, normalized, timestamp, source, protocol, host, username, scope_key
                ],
            )
            .map_err(|error| error.to_string())?;
        trim_history_tx(&transaction, &scope_key, max)?;
        transaction.commit().map_err(|error| error.to_string())?;
        query_entry(&connection, &scope_key, &command)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn clear_command_history(
    state: tauri::State<'_, SharedCommandHistoryState>,
    scope_key: Option<String>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let scope_key = normalize_scope_key(scope_key);
    tokio::task::spawn_blocking(move || {
        let connection = state
            .connection
            .lock()
            .map_err(|_| "Command history database lock is poisoned".to_string())?;
        connection
            .execute(
                "DELETE FROM command_history WHERE scope_key = ?1",
                params![scope_key],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(
        state: &CommandHistoryState,
        command: &str,
        timestamp: i64,
        scope_key: &str,
    ) -> CommandHistoryEntry {
        let connection = state.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO command_history (
                    command, normalized, use_count, last_used_at, created_at, source, scope_key
                 ) VALUES (?1, ?2, 1, ?3, ?3, 'test', ?4)
                 ON CONFLICT(scope_key, command) DO UPDATE SET
                    use_count = command_history.use_count + 1,
                    last_used_at = excluded.last_used_at",
                params![command, command.to_lowercase(), timestamp, scope_key],
            )
            .unwrap();
        query_entry(&connection, scope_key, command).unwrap()
    }

    #[test]
    fn creates_schema_and_records_exact_commands() {
        let state = CommandHistoryState::in_memory().unwrap();
        let first = insert(&state, "./report.sh start test.jar", 1, "global");
        let second = insert(&state, "./report.sh start test.jar", 2, "global");

        assert_eq!(first.count, 1);
        assert_eq!(second.count, 2);
        assert_eq!(second.cmd, "./report.sh start test.jar");
        assert_eq!(second.last_used_at, 2);
    }

    #[test]
    fn isolates_scopes_and_trims_old_entries() {
        let state = CommandHistoryState::in_memory().unwrap();
        insert(&state, "one command", 1, "host-a");
        insert(&state, "two command", 2, "host-a");
        insert(&state, "other host", 3, "host-b");

        let mut connection = state.connection.lock().unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(trim_history_tx(&transaction, "host-a", 1).unwrap(), 1);
        transaction.commit().unwrap();

        let host_a = load_entries(&connection, "host-a", 10).unwrap();
        let host_b = load_entries(&connection, "host-b", 10).unwrap();
        assert_eq!(host_a.len(), 1);
        assert_eq!(host_a[0].cmd, "two command");
        assert_eq!(host_b.len(), 1);
    }

    #[test]
    fn rejects_multiline_control_and_oversized_commands() {
        assert!(validate_command("echo one\necho two".to_string()).is_err());
        assert!(validate_command("echo\u{0000}secret".to_string()).is_err());
        assert!(validate_command("x".repeat(MAX_COMMAND_CHARS + 1)).is_err());
    }

    #[test]
    fn normalizes_bounded_metadata() {
        assert_eq!(normalize_metadata(None, 10), None);
        assert_eq!(normalize_metadata(Some("   ".to_string()), 10), None);
        assert_eq!(
            normalize_metadata(Some(" terminal ".to_string()), 4),
            Some("term".to_string())
        );
    }
}
