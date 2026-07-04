use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::tunnel::TunnelState;

#[cfg(unix)]
fn ensure_private_key_permissions(path: &PathBuf) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_key_permissions(_path: &PathBuf) -> Result<(), String> {
    Ok(())
}
// Removed unused imports: std::collections::HashMap, Path, AppHandle, Manager

// --- Data Structures ---

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionConfig {
    pub id: String,
    pub name: String,
    pub protocol: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String, // "password" | "key"

    // Encrypted fields (stored as base64 strings)
    pub password: Option<String>,
    pub private_key_path: Option<String>,
    pub passphrase: Option<String>, // Key passphrase

    // Basic Config
    pub remarks: Option<String>,
    #[serde(alias = "group_id")]
    pub group: Option<String>,

    // Terminal Config
    pub term_type: Option<String>,
    pub encoding: Option<String>,
    pub font_size: Option<u16>,
    pub font_family: Option<String>,

    // Connection Config
    pub connect_timeout: Option<u64>,
    pub keep_alive_interval: Option<u64>,
    pub last_connected: Option<u64>,

    // Advanced Config
    pub local_forward: Option<String>,
    pub remote_forward: Option<String>,
    pub proxy_type: Option<String>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_auth: Option<bool>,
    pub proxy_user: Option<String>,
    pub proxy_pass: Option<String>, // Encrypted
    pub jump_host: Option<String>,
    pub jump_port: Option<u16>,
    pub jump_username: Option<String>,
    pub jump_auth_type: Option<String>,
    pub jump_password: Option<String>,
    pub jump_private_key_path: Option<String>,
    pub jump_passphrase: Option<String>,
    pub login_script: Option<String>,
    pub serial_path: Option<String>,
    pub baud_rate: Option<u32>,
    pub data_bits: Option<u8>,
    pub stop_bits: Option<String>,
    pub parity: Option<String>,
    pub flow_control: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TunnelConfig {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub mode: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub server_alive_interval: u64,
    pub allow_public_bind: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommandKnowledgeEntry {
    pub id: String,
    pub title: String,
    pub command: String,
    pub trigger: Option<String>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub favorite: bool,
    pub safety_level: String,
    pub execution_policy: String,
    pub usage_count: u64,
    pub last_used_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub legacy_source: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct StorageData {
    sessions: Vec<SessionConfig>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CommandKnowledgeStorageData {
    schema_version: u16,
    entries: Vec<CommandKnowledgeEntry>,
}

impl Default for CommandKnowledgeStorageData {
    fn default() -> Self {
        Self {
            schema_version: 1,
            entries: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct TunnelStorageData {
    tunnels: Vec<TunnelConfig>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct PreferencesData {
    toolbar_order: Option<Vec<String>>,
}

pub struct StorageState {
    pub key: Key<Aes256Gcm>,
    pub app_dir: PathBuf,
}

pub type SharedStorageState = Arc<Mutex<StorageState>>;

impl StorageState {
    pub fn new() -> Self {
        let home = dirs::home_dir().expect("Could not find home directory");
        let app_dir = home.join(".duskterm").join("sessions");

        // Ensure directory exists
        if !app_dir.exists() {
            fs::create_dir_all(&app_dir).expect("Failed to create app local directory");
        }

        // Generate or load a consistent key for the user.
        // In a real app, we might use system keychain or a user-provided master password.
        // For this demo, we'll use a fixed salt + machine specific logic if possible,
        // OR just a stored key file for now (less secure but functional for demo).
        // Let's generate a key file if it doesn't exist.
        let key_path = app_dir.join(".key");
        let key = if key_path.exists() {
            let key_bytes = fs::read(&key_path).expect("Failed to read key file");
            *Key::<Aes256Gcm>::from_slice(&key_bytes)
        } else {
            let key = Aes256Gcm::generate_key(OsRng);
            fs::write(&key_path, key).expect("Failed to write key file");
            key
        };
        let _ = ensure_private_key_permissions(&key_path);

        Self { key, app_dir }
    }

    fn get_sessions_path(&self) -> PathBuf {
        self.app_dir.join("sessions.json")
    }

    fn get_preferences_path(&self) -> PathBuf {
        self.app_dir.join("preferences.json")
    }

    fn get_tunnels_path(&self) -> PathBuf {
        self.app_dir.join("tunnels.json")
    }

    fn get_command_knowledge_path(&self) -> PathBuf {
        self.app_dir.join("command_knowledge.json")
    }
}

// --- Helper Functions ---

fn encrypt(data: &str, key: &Key<Aes256Gcm>) -> String {
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(OsRng); // 96-bits; unique per message
    let ciphertext = cipher
        .encrypt(&nonce, data.as_bytes())
        .expect("Encryption failed");

    // Prepend nonce to ciphertext for storage
    let mut combined = nonce.to_vec();
    combined.extend(ciphertext);

    general_purpose::STANDARD.encode(combined)
}

fn decrypt(data: &str, key: &Key<Aes256Gcm>) -> Result<String, String> {
    let encrypted_bytes = general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())?;

    if encrypted_bytes.len() < 12 {
        return Err("Invalid encrypted data".to_string());
    }

    let (nonce_bytes, ciphertext) = encrypted_bytes.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| e.to_string())?;
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

fn decrypt_optional_field(value: &mut Option<String>, key: &Key<Aes256Gcm>) -> Result<(), String> {
    if let Some(encrypted) = value {
        if !encrypted.is_empty() {
            *encrypted = decrypt(encrypted, key)?;
        }
    }
    Ok(())
}

pub fn decrypt_session_config(
    state: &StorageState,
    mut session: SessionConfig,
) -> Result<SessionConfig, String> {
    decrypt_optional_field(&mut session.password, &state.key)?;
    decrypt_optional_field(&mut session.passphrase, &state.key)?;
    decrypt_optional_field(&mut session.proxy_pass, &state.key)?;
    decrypt_optional_field(&mut session.jump_password, &state.key)?;
    decrypt_optional_field(&mut session.jump_passphrase, &state.key)?;
    Ok(session)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_mode(mode: &str) -> Result<String, String> {
    let normalized = mode.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "local" | "remote" | "dynamic" => Ok(normalized),
        _ => Err("Unsupported tunnel mode; use local, remote, or dynamic".to_string()),
    }
}

fn load_storage_data(state: &StorageState) -> Result<StorageData, String> {
    let path = state.get_sessions_path();
    if !path.exists() {
        return Ok(StorageData::default());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

fn save_storage_data(state: &StorageState, data: &StorageData) -> Result<(), String> {
    let path = state.get_sessions_path();
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn load_tunnel_storage_data(state: &StorageState) -> Result<TunnelStorageData, String> {
    let path = state.get_tunnels_path();
    if !path.exists() {
        return Ok(TunnelStorageData::default());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

fn save_tunnel_storage_data(state: &StorageState, data: &TunnelStorageData) -> Result<(), String> {
    let path = state.get_tunnels_path();
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn load_command_knowledge_storage_data(
    state: &StorageState,
) -> Result<CommandKnowledgeStorageData, String> {
    let path = state.get_command_knowledge_path();
    if !path.exists() {
        return Ok(CommandKnowledgeStorageData::default());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

fn save_command_knowledge_storage_data(
    state: &StorageState,
    data: &CommandKnowledgeStorageData,
) -> Result<(), String> {
    let path = state.get_command_knowledge_path();
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn normalize_command_knowledge_entry(mut entry: CommandKnowledgeEntry) -> Result<CommandKnowledgeEntry, String> {
    entry.id = entry.id.trim().to_string();
    if entry.id.is_empty() {
        entry.id = Uuid::new_v4().to_string();
    }

    entry.title = entry.title.trim().to_string();
    entry.command = entry.command.trim().to_string();
    if entry.title.is_empty() {
        return Err("Command knowledge title is required".to_string());
    }
    if entry.command.is_empty() {
        return Err("Command knowledge command is required".to_string());
    }

    entry.trigger = normalize_optional_text(entry.trigger);
    entry.description = normalize_optional_text(entry.description);
    entry.legacy_source = normalize_optional_text(entry.legacy_source);
    entry.tags = entry
        .tags
        .into_iter()
        .filter_map(|tag| {
            let trimmed = tag.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect();

    entry.safety_level = match entry.safety_level.trim() {
        "sensitive" => "sensitive".to_string(),
        "dangerous" => "dangerous".to_string(),
        _ => "normal".to_string(),
    };

    entry.execution_policy = match entry.execution_policy.trim() {
        "confirmBeforeExecute" => "confirmBeforeExecute".to_string(),
        "blockDirectExecute" => "blockDirectExecute".to_string(),
        _ => {
            if entry.safety_level == "dangerous" {
                "blockDirectExecute".to_string()
            } else if entry.safety_level == "sensitive" {
                "confirmBeforeExecute".to_string()
            } else {
                "insertOnly".to_string()
            }
        }
    };

    let timestamp = now_millis();
    if entry.created_at == 0 {
        entry.created_at = timestamp;
    }
    entry.updated_at = timestamp;

    Ok(entry)
}

fn normalize_tunnel_config(
    mut config: TunnelConfig,
    existing_created_at: Option<u64>,
) -> Result<TunnelConfig, String> {
    if config.id.trim().is_empty() {
        config.id = Uuid::new_v4().to_string();
    } else {
        config.id = config.id.trim().to_string();
    }

    config.session_id = config.session_id.trim().to_string();
    if config.session_id.is_empty() {
        return Err("Tunnel config must belong to a saved session".to_string());
    }

    config.name = config.name.trim().to_string();
    if config.name.is_empty() {
        config.name = format!(
            "{}:{} [{}]",
            config.listen_host, config.listen_port, config.mode
        );
    }

    config.mode = normalize_mode(&config.mode)?;
    config.listen_host = {
        let host = config.listen_host.trim();
        if host.is_empty() {
            "127.0.0.1".to_string()
        } else {
            host.to_string()
        }
    };
    if config.listen_port == 0 {
        return Err("Tunnel listen port is required".to_string());
    }

    config.server_alive_interval = if config.server_alive_interval == 0 {
        30
    } else {
        config.server_alive_interval.clamp(10, 120)
    };

    match config.mode.as_str() {
        "local" | "remote" => {
            config.target_host = normalize_optional_text(config.target_host);
            if config.target_host.is_none() {
                return Err("Target host is required for local and remote tunnels".to_string());
            }
            if config.target_port.unwrap_or(0) == 0 {
                return Err("Target port is required for local and remote tunnels".to_string());
            }
        }
        "dynamic" => {
            config.target_host = None;
            config.target_port = None;
        }
        _ => {}
    }

    let timestamp = now_millis();
    config.created_at = existing_created_at.unwrap_or_else(|| {
        if config.created_at == 0 {
            timestamp
        } else {
            config.created_at
        }
    });
    config.updated_at = timestamp;

    Ok(config)
}

pub fn load_session_record(state: &StorageState, id: &str) -> Result<SessionConfig, String> {
    let data = load_storage_data(state)?;
    data.sessions
        .into_iter()
        .find(|session| session.id == id)
        .ok_or_else(|| "Session not found".to_string())
}

pub fn load_tunnel_config_record(state: &StorageState, id: &str) -> Result<TunnelConfig, String> {
    let data = load_tunnel_storage_data(state)?;
    data.tunnels
        .into_iter()
        .find(|config| config.id == id)
        .ok_or_else(|| "Tunnel config not found".to_string())
}

fn delete_tunnel_configs_by_session_locked(
    state: &StorageState,
    session_id: &str,
) -> Result<(), String> {
    let mut data = load_tunnel_storage_data(state)?;
    let before = data.tunnels.len();
    data.tunnels
        .retain(|config| config.session_id != session_id);
    if data.tunnels.len() != before {
        save_tunnel_storage_data(state, &data)?;
    }
    Ok(())
}

// --- Tauri Commands ---

#[tauri::command]
pub fn load_sessions(
    state: tauri::State<'_, SharedStorageState>,
) -> Result<Vec<SessionConfig>, String> {
    let state = state.lock().unwrap();
    Ok(load_storage_data(&state)?.sessions)
}

#[tauri::command]
pub fn save_session(
    state: tauri::State<'_, SharedStorageState>,
    mut session: SessionConfig,
) -> Result<(), String> {
    // Logic:
    // 1. Load existing
    // 2. Encrypt sensitive fields of the incoming session
    // 3. Update or Add to list
    // 4. Save to disk

    // NOTE: In a real app, we should probably receive plain text from frontend (over https/ipc is safeish locally)
    // and encrypt here. Ideally frontend sends plain text fields separate from the struct if they are transient.
    // However, assuming the frontend sends the SessionConfig struct with plain text in the fields for 'save':

    let state = state.lock().unwrap();

    // Encrypt fields
    if let Some(pwd) = &session.password {
        if !pwd.is_empty() {
            session.password = Some(encrypt(pwd, &state.key));
        }
    }
    if let Some(pp) = &session.passphrase {
        if !pp.is_empty() {
            session.passphrase = Some(encrypt(pp, &state.key));
        }
    }
    if let Some(pp) = &session.proxy_pass {
        if !pp.is_empty() {
            session.proxy_pass = Some(encrypt(pp, &state.key));
        }
    }
    if let Some(pwd) = &session.jump_password {
        if !pwd.is_empty() {
            session.jump_password = Some(encrypt(pwd, &state.key));
        }
    }
    if let Some(pp) = &session.jump_passphrase {
        if !pp.is_empty() {
            session.jump_passphrase = Some(encrypt(pp, &state.key));
        }
    }
    // (Repeat for passphrase if needed)

    let mut data = load_storage_data(&state)?;

    if let Some(idx) = data.sessions.iter().position(|s| s.id == session.id) {
        data.sessions[idx] = session;
    } else {
        data.sessions.push(session);
    }

    save_storage_data(&state, &data)
}

#[tauri::command]
pub async fn delete_session(
    state: tauri::State<'_, SharedStorageState>,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    tunnel_state: tauri::State<'_, TunnelState>,
    id: String,
) -> Result<(), String> {
    for tunnel_id in tunnel_state.tunnel_ids_for_session(&id)? {
        supervisor
            .stop_tunnel(tunnel_state.inner().clone(), tunnel_id)
            .await?;
    }

    let state = state.lock().unwrap();
    delete_tunnel_configs_by_session_locked(&state, &id)?;

    let mut data = load_storage_data(&state)?;

    data.sessions.retain(|s| s.id != id);

    save_storage_data(&state, &data)
}

#[tauri::command]
pub fn get_decrypted_session(
    state: tauri::State<'_, SharedStorageState>,
    id: String,
) -> Result<SessionConfig, String> {
    let state = state.lock().unwrap();
    if let Ok(session) = load_session_record(&state, &id) {
        decrypt_session_config(&state, session)
    } else {
        Err("Session not found".to_string())
    }
}

#[tauri::command]
pub fn load_command_knowledge(
    state: tauri::State<'_, SharedStorageState>,
) -> Result<Vec<CommandKnowledgeEntry>, String> {
    let state = state.lock().unwrap();
    Ok(load_command_knowledge_storage_data(&state)?.entries)
}

#[tauri::command]
pub fn save_command_knowledge_entry(
    state: tauri::State<'_, SharedStorageState>,
    entry: CommandKnowledgeEntry,
) -> Result<CommandKnowledgeEntry, String> {
    let state = state.lock().unwrap();
    let normalized = normalize_command_knowledge_entry(entry)?;
    let mut data = load_command_knowledge_storage_data(&state)?;

    if let Some(index) = data
        .entries
        .iter()
        .position(|item| item.id == normalized.id)
    {
        data.entries[index] = normalized.clone();
    } else {
        data.entries.push(normalized.clone());
    }

    save_command_knowledge_storage_data(&state, &data)?;
    Ok(normalized)
}

#[tauri::command]
pub fn delete_command_knowledge_entry(
    state: tauri::State<'_, SharedStorageState>,
    id: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    let mut data = load_command_knowledge_storage_data(&state)?;
    data.entries.retain(|entry| entry.id != id);
    save_command_knowledge_storage_data(&state, &data)
}

#[tauri::command]
pub fn replace_command_knowledge_entries(
    state: tauri::State<'_, SharedStorageState>,
    entries: Vec<CommandKnowledgeEntry>,
) -> Result<Vec<CommandKnowledgeEntry>, String> {
    let state = state.lock().unwrap();
    let normalized: Vec<CommandKnowledgeEntry> = entries
        .into_iter()
        .map(normalize_command_knowledge_entry)
        .collect::<Result<Vec<_>, _>>()?;
    let data = CommandKnowledgeStorageData {
        schema_version: 1,
        entries: normalized.clone(),
    };
    save_command_knowledge_storage_data(&state, &data)?;
    Ok(normalized)
}

#[tauri::command]
pub fn list_tunnel_configs(
    state: tauri::State<'_, SharedStorageState>,
    session_id: Option<String>,
) -> Result<Vec<TunnelConfig>, String> {
    let state = state.lock().unwrap();
    let mut configs = load_tunnel_storage_data(&state)?.tunnels;
    if let Some(session_id) = normalize_optional_text(session_id) {
        configs.retain(|config| config.session_id == session_id);
    }
    configs.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(configs)
}

#[tauri::command]
pub fn save_tunnel_config(
    state: tauri::State<'_, SharedStorageState>,
    config: TunnelConfig,
) -> Result<TunnelConfig, String> {
    let state = state.lock().unwrap();
    let sessions = load_storage_data(&state)?.sessions;
    if !sessions
        .iter()
        .any(|session| session.id == config.session_id)
    {
        return Err("Saved session for tunnel config no longer exists".to_string());
    }

    let mut data = load_tunnel_storage_data(&state)?;
    let existing_created_at = data
        .tunnels
        .iter()
        .find(|item| item.id == config.id)
        .map(|item| item.created_at);
    let normalized = normalize_tunnel_config(config, existing_created_at)?;

    if let Some(index) = data
        .tunnels
        .iter()
        .position(|item| item.id == normalized.id)
    {
        data.tunnels[index] = normalized.clone();
    } else {
        data.tunnels.push(normalized.clone());
    }

    save_tunnel_storage_data(&state, &data)?;
    Ok(normalized)
}

#[tauri::command]
pub fn delete_tunnel_config(
    state: tauri::State<'_, SharedStorageState>,
    id: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    let mut data = load_tunnel_storage_data(&state)?;
    let before = data.tunnels.len();
    data.tunnels.retain(|config| config.id != id);
    if before == data.tunnels.len() {
        return Ok(());
    }
    save_tunnel_storage_data(&state, &data)
}

#[tauri::command]
pub fn duplicate_tunnel_config(
    state: tauri::State<'_, SharedStorageState>,
    id: String,
) -> Result<TunnelConfig, String> {
    let state = state.lock().unwrap();
    let mut data = load_tunnel_storage_data(&state)?;
    let source = data
        .tunnels
        .iter()
        .find(|config| config.id == id)
        .cloned()
        .ok_or_else(|| "Tunnel config not found".to_string())?;

    let timestamp = now_millis();
    let duplicate = TunnelConfig {
        id: Uuid::new_v4().to_string(),
        session_id: source.session_id.clone(),
        name: format!("{} Copy", source.name),
        mode: source.mode.clone(),
        listen_host: source.listen_host.clone(),
        listen_port: source.listen_port,
        target_host: source.target_host.clone(),
        target_port: source.target_port,
        server_alive_interval: source.server_alive_interval,
        allow_public_bind: source.allow_public_bind,
        created_at: timestamp,
        updated_at: timestamp,
    };
    let duplicate = normalize_tunnel_config(duplicate, None)?;

    data.tunnels.push(duplicate.clone());
    save_tunnel_storage_data(&state, &data)?;
    Ok(duplicate)
}

#[tauri::command]
pub fn delete_tunnel_configs_by_session(
    state: tauri::State<'_, SharedStorageState>,
    session_id: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    delete_tunnel_configs_by_session_locked(&state, &session_id)
}

fn load_preferences(state: &StorageState) -> Result<PreferencesData, String> {
    let path = state.get_preferences_path();
    if !path.exists() {
        return Ok(PreferencesData::default());
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let data: PreferencesData = serde_json::from_str(&content).unwrap_or_default();
    Ok(data)
}

fn save_preferences(state: &StorageState, prefs: &PreferencesData) -> Result<(), String> {
    let path = state.get_preferences_path();
    let json = serde_json::to_string_pretty(&prefs).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_toolbar_layout(
    state: tauri::State<'_, SharedStorageState>,
) -> Result<Vec<String>, String> {
    let state = state.lock().unwrap();
    let prefs = load_preferences(&state)?;
    Ok(prefs.toolbar_order.unwrap_or_default())
}

#[tauri::command]
pub fn save_toolbar_layout(
    state: tauri::State<'_, SharedStorageState>,
    toolbar_order: Vec<String>,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    let mut prefs = load_preferences(&state)?;
    prefs.toolbar_order = Some(toolbar_order);
    save_preferences(&state, &prefs)
}

/// 验证导出/导入路径是否安全，返回可用于读写的规范化路径
fn validate_io_path(raw_path: &str, must_exist: bool) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw_path);
    // Reject paths containing ".." to prevent directory traversal
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("路径包含非法字符".to_string());
    }
    // Canonicalize based on whether file must exist
    if must_exist || path.exists() {
        path.canonicalize()
            .map_err(|_| format!("无法解析路径: {}", raw_path))
    } else {
        // Export: file doesn't exist yet, canonicalize parent then join filename
        let parent = path.parent().ok_or_else(|| "无效的文件路径".to_string())?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|_| "目标目录不存在".to_string())?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("export.json");
        Ok(canonical_parent.join(name))
    }
}

#[tauri::command]
pub fn export_sessions_to(
    state: tauri::State<'_, SharedStorageState>,
    target_path: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    let source = state.get_sessions_path();
    if !source.exists() {
        return Err("No sessions file found".to_string());
    }
    // Validate target path is safe (export creates new file, so must_exist=false)
    let _validated = validate_io_path(&target_path, false)?;
    let content = fs::read_to_string(&source).map_err(|e| e.to_string())?;
    let mut data: StorageData = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // Decrypt sensitive fields so the export file is self-contained and transportable.
    // Fail closed: exporting blank credentials after a decrypt error is worse than a clear error.
    for session in &mut data.sessions {
        *session = decrypt_session_config(&state, session.clone()).map_err(|error| {
            format!(
                "Export failed: could not decrypt credentials for session '{}': {}",
                session.name, error
            )
        })?;
    }

    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(&_validated, json).map_err(|e| format!("导出失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn export_command_knowledge_to(
    state: tauri::State<'_, SharedStorageState>,
    target_path: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    let data = load_command_knowledge_storage_data(&state)?;
    let validated = validate_io_path(&target_path, false)?;
    let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(&validated, json).map_err(|e| format!("导出命令知识库失败: {}", e))
}

#[tauri::command]
pub fn import_command_knowledge_from(
    state: tauri::State<'_, SharedStorageState>,
    source_path: String,
) -> Result<Vec<CommandKnowledgeEntry>, String> {
    let state = state.lock().unwrap();
    let validated = validate_io_path(&source_path, true)?;
    let content = fs::read_to_string(&validated).map_err(|e| format!("读取命令知识库失败: {}", e))?;
    let imported: CommandKnowledgeStorageData = serde_json::from_str(&content)
        .map_err(|_| "文件格式不正确，请选择有效的命令知识库导出文件".to_string())?;

    let mut existing = load_command_knowledge_storage_data(&state)?;
    let mut existing_ids: std::collections::HashSet<String> =
        existing.entries.iter().map(|entry| entry.id.clone()).collect();
    let mut added = Vec::new();

    let entries = &mut existing.entries;
    for entry in imported.entries {
        if entry.id.trim().is_empty() || existing_ids.contains(&entry.id) {
            continue;
        }
        let _safety_level = entry.safety_level.clone();
        let _execution_policy = entry.execution_policy.clone();
        let entry = normalize_command_knowledge_entry(entry)?;
        existing_ids.insert(entry.id.clone());
        added.push(entry.clone());
        entries.push(entry);
    }

    save_command_knowledge_storage_data(&state, &existing)?;
    Ok(added)
}

#[tauri::command]
pub fn import_sessions_from(
    state: tauri::State<'_, SharedStorageState>,
    source_path: String,
) -> Result<(), String> {
    let state = state.lock().unwrap();
    // Validate source path exists and is readable (import requires existing file)
    let validated = validate_io_path(&source_path, true)?;
    let content = fs::read_to_string(&validated).map_err(|e| format!("读取导入文件失败: {}", e))?;
    let imported: StorageData = serde_json::from_str(&content)
        .map_err(|_| "文件格式不正确，请选择有效的会话导出文件".to_string())?;

    let dest_path = state.get_sessions_path();
    let mut existing: StorageData = if dest_path.exists() {
        let existing_content =
            fs::read_to_string(&dest_path).map_err(|e| format!("读取现有会话失败: {}", e))?;
        serde_json::from_str(&existing_content).unwrap_or_default()
    } else {
        StorageData::default()
    };

    let existing_ids: std::collections::HashSet<String> =
        existing.sessions.iter().map(|s| s.id.clone()).collect();

    let mut added = 0u32;
    let mut skipped = 0u32;
    for mut session in imported.sessions {
        let sid = session.id.clone();
        if sid.is_empty() {
            skipped += 1;
            continue;
        }
        if existing_ids.contains(&sid) {
            skipped += 1;
            continue;
        }
        // Re-encrypt sensitive fields with the local machine's key
        if let Some(ref pwd) = session.password {
            if !pwd.is_empty() {
                session.password = Some(encrypt(pwd, &state.key));
            }
        }
        if let Some(ref pp) = session.passphrase {
            if !pp.is_empty() {
                session.passphrase = Some(encrypt(pp, &state.key));
            }
        }
        if let Some(ref pp) = session.proxy_pass {
            if !pp.is_empty() {
                session.proxy_pass = Some(encrypt(pp, &state.key));
            }
        }
        if let Some(ref pwd) = session.jump_password {
            if !pwd.is_empty() {
                session.jump_password = Some(encrypt(pwd, &state.key));
            }
        }
        if let Some(ref pp) = session.jump_passphrase {
            if !pp.is_empty() {
                session.jump_passphrase = Some(encrypt(pp, &state.key));
            }
        }
        existing.sessions.push(session);
        added += 1;
    }

    if added == 0 {
        return Err(format!("未添加新会话（{} 个重复或无效）", skipped));
    }

    let json = serde_json::to_string_pretty(&existing).map_err(|e| e.to_string())?;
    fs::write(&dest_path, json).map_err(|e| format!("写入会话文件失败: {}", e))?;

    Ok(())
}
