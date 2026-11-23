mod connection;
pub(crate) mod console;
pub(crate) mod data;
mod write;

use crate::storage::{load_session_record, save_session_record, SessionConfig, SharedStorageState};
use connection::Runtime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::State;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Standalone,
    Sentinel,
    Cluster,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    host: String,
    port: u16,
}

impl Endpoint {
    fn valid(&self) -> bool {
        self.port != 0
            && !self.host.is_empty()
            && self.host.len() <= 255
            && !self
                .host
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control() || "@/\\?#".contains(ch))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileOptions {
    pub mode: Mode,
    pub db: u32,
    pub tls: bool,
    pub password_saved: bool,
    pub seeds: Vec<Endpoint>,
    pub sentinel_service: String,
    pub sentinel_username: String,
    pub sentinel_tls: bool,
    pub sentinel_password_saved: bool,
    #[serde(default = "default_key_separator")]
    pub key_separator: String,
}

fn default_key_separator() -> String {
    ":".into()
}

pub fn is_redis_profile(config: &SessionConfig) -> bool {
    config
        .protocol
        .as_deref()
        .is_some_and(|protocol| protocol.trim().eq_ignore_ascii_case("redis"))
}

fn validate_profile(config: &SessionConfig) -> Result<(), String> {
    let endpoint = Endpoint {
        host: config.host.trim().to_owned(),
        port: config.port,
    };
    if !is_redis_profile(config) || config.name.trim().is_empty() || !endpoint.valid() {
        return Err("请填写有效的 Redis 会话名称、主机和端口".into());
    }
    if let Some(options) = &config.redis {
        if options.key_separator.len() > 32 || options.key_separator.chars().any(char::is_control) {
            return Err("键分隔符不能包含控制字符或超过 32 字节".into());
        }
        if options.seeds.len() > 15 || options.seeds.iter().any(|seed| !seed.valid()) {
            return Err("Redis 备用节点地址无效或超过 15 个".into());
        }
        if options.mode == Mode::Cluster && options.db != 0 {
            return Err("Redis 集群仅支持 DB 0".into());
        }
        if options.mode == Mode::Sentinel
            && (options.sentinel_service.is_empty()
                || options.sentinel_service.len() > 128
                || options
                    .sentinel_service
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control()))
        {
            return Err("请填写有效的哨兵主节点服务名".into());
        }
    }
    Ok(())
}

fn credential(config: &SessionConfig) -> Result<keyring::Entry, String> {
    let mut key = Sha256::new();
    for part in [&config.id, &config.host, &config.username] {
        key.update((part.len() as u64).to_be_bytes());
        key.update(part);
    }
    key.update(config.port.to_be_bytes());
    key.update([u8::from(
        config.redis.as_ref().is_some_and(|options| options.tls),
    )]);
    if let Some(options) = config
        .redis
        .as_ref()
        .filter(|options| options.mode != Mode::Standalone)
    {
        let scope = serde_json::to_vec(&(
            options.mode,
            &options.seeds,
            &options.sentinel_service,
            &options.sentinel_username,
            options.sentinel_tls,
        ))
        .map_err(|_| "Redis 凭据配置无效")?;
        key.update(scope);
    }
    keyring::Entry::new("com.duskterm.redis", &format!("{:x}", key.finalize()))
        .map_err(|_| "无法访问 Redis 系统凭据".into())
}

#[derive(Default)]
struct Passwords {
    data: Option<Zeroizing<String>>,
    sentinel: Option<Zeroizing<String>>,
}

fn read_passwords(config: &SessionConfig) -> Result<Passwords, String> {
    let Some(options) = config
        .redis
        .as_ref()
        .filter(|options| options.password_saved || options.sentinel_password_saved)
    else {
        return Ok(Passwords::default());
    };
    let secret = credential(config)?
        .get_password()
        .map(Zeroizing::new)
        .map_err(|_| "未找到可用的 Redis 密码，请重新编辑该会话")?;
    if options.mode != Mode::Sentinel {
        return Ok(Passwords {
            data: Some(secret),
            sentinel: None,
        });
    }
    let (data, sentinel): (Option<String>, Option<String>) =
        serde_json::from_str(&secret).map_err(|_| "Redis 系统凭据无效，请重新编辑该会话")?;
    if data.is_some() != options.password_saved
        || sentinel.is_some() != options.sentinel_password_saved
    {
        return Err("Redis 系统凭据无效，请重新编辑该会话".into());
    }
    Ok(Passwords {
        data: data.map(Zeroizing::new),
        sentinel: sentinel.map(Zeroizing::new),
    })
}

fn same_endpoint(left: &SessionConfig, right: &SessionConfig) -> bool {
    let default = ProfileOptions::default();
    let left_options = left.redis.as_ref().unwrap_or(&default);
    let right_options = right.redis.as_ref().unwrap_or(&default);
    left.host == right.host
        && left.port == right.port
        && left.username == right.username
        && left_options.mode == right_options.mode
        && left_options.tls == right_options.tls
        && left_options.seeds == right_options.seeds
        && left_options.sentinel_service == right_options.sentinel_service
        && left_options.sentinel_username == right_options.sentinel_username
        && left_options.sentinel_tls == right_options.sentinel_tls
}

fn change_password(entry: &keyring::Entry, password: Option<&str>) -> Result<(), String> {
    match password {
        Some(password) => entry.set_password(password),
        None => match entry.delete_credential() {
            Err(keyring::Error::NoEntry) => Ok(()),
            result => result,
        },
    }
    .map_err(|_| "无法更新 Redis 系统凭据".into())
}

fn with_password_change(
    entry: &keyring::Entry,
    password: Option<&str>,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let old = match entry.get_password() {
        Ok(password) => Some(Zeroizing::new(password)),
        Err(keyring::Error::NoEntry) => None,
        Err(_) => return Err("无法读取 Redis 系统凭据".into()),
    };
    change_password(entry, password)?;
    if let Err(error) = save() {
        change_password(entry, old.as_ref().map(|value| value.as_str()))?;
        return Err(error);
    }
    Ok(())
}

struct PasswordUpdate {
    incoming: Passwords,
    forget_data: bool,
    forget_sentinel: bool,
}

impl PasswordUpdate {
    fn new(
        password: Option<String>,
        forget_data: bool,
        sentinel_password: Option<String>,
        forget_sentinel: bool,
    ) -> Self {
        let nonempty =
            |value: Option<String>| value.filter(|text| !text.is_empty()).map(Zeroizing::new);
        Self {
            incoming: Passwords {
                data: nonempty(password),
                sentinel: nonempty(sentinel_password),
            },
            forget_data,
            forget_sentinel,
        }
    }

    fn validate(&self, config: &SessionConfig) -> Result<(), String> {
        if (self.incoming.sentinel.is_some() || self.forget_sentinel)
            && !config
                .redis
                .as_ref()
                .is_some_and(|options| options.mode == Mode::Sentinel)
        {
            return Err("当前模式无需哨兵凭据".into());
        }
        Ok(())
    }

    fn resolve(self, saved: Option<&SessionConfig>) -> Result<Passwords, String> {
        let options = saved.and_then(|profile| profile.redis.as_ref());
        let keep_data = self.incoming.data.is_none()
            && !self.forget_data
            && options.is_some_and(|options| options.password_saved);
        let keep_sentinel = self.incoming.sentinel.is_none()
            && !self.forget_sentinel
            && options.is_some_and(|options| options.sentinel_password_saved);
        let saved_passwords = match saved {
            Some(saved) if keep_data || keep_sentinel => read_passwords(saved)?,
            _ => Passwords::default(),
        };
        Ok(Passwords {
            data: if keep_data {
                saved_passwords.data
            } else {
                self.incoming.data
            },
            sentinel: if keep_sentinel {
                saved_passwords.sentinel
            } else {
                self.incoming.sentinel
            },
        })
    }
}

pub(crate) fn remove_profile_password(
    profile: &SessionConfig,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if is_redis_profile(profile)
        && profile
            .redis
            .as_ref()
            .is_some_and(|options| options.password_saved || options.sentinel_password_saved)
    {
        with_password_change(&credential(profile)?, None, save)
    } else {
        save()
    }
}

#[tauri::command]
pub async fn redis_save_profile(
    storage: State<'_, SharedStorageState>,
    mut config: SessionConfig,
    password: Option<String>,
    forget_password: bool,
    sentinel_password: Option<String>,
    forget_sentinel_password: Option<bool>,
) -> Result<(), String> {
    validate_profile(&config)?;
    uuid::Uuid::parse_str(&config.id).map_err(|_| "Redis 会话 ID 无效")?;
    config.host = config.host.trim().to_owned();
    config.name = config.name.trim().to_owned();
    config.username = config.username.trim().to_owned();
    let update = PasswordUpdate::new(
        password,
        forget_password,
        sentinel_password,
        forget_sentinel_password.unwrap_or(false),
    );
    update.validate(&config)?;
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let storage = storage.lock().map_err(|_| "无法访问会话存储")?;
        let previous = load_session_record(&storage, &config.id).ok();
        if previous
            .as_ref()
            .is_some_and(|profile| !is_redis_profile(profile))
        {
            return Err("该会话 ID 已由终端会话使用".into());
        }
        let same = previous
            .as_ref()
            .filter(|profile| same_endpoint(profile, &config));
        let replaced = previous
            .as_ref()
            .filter(|profile| !same_endpoint(profile, &config));
        let saved = same.and_then(|profile| profile.redis.as_ref());
        let saved_data = saved.is_some_and(|options| options.password_saved);
        let saved_sentinel = saved.is_some_and(|options| options.sentinel_password_saved);
        let options = config.redis.get_or_insert_with(ProfileOptions::default);
        options.password_saved =
            update.incoming.data.is_some() || (saved_data && !update.forget_data);
        options.sentinel_password_saved =
            update.incoming.sentinel.is_some() || (saved_sentinel && !update.forget_sentinel);
        let sentinel_mode = options.mode == Mode::Sentinel;
        let save = || {
            if update.incoming.data.is_none()
                && update.incoming.sentinel.is_none()
                && (!saved_data || !update.forget_data)
                && (!saved_sentinel || !update.forget_sentinel)
            {
                return save_session_record(&storage, config);
            }
            let entry = credential(&config)?;
            let passwords = update.resolve(same)?;
            let secret =
                if sentinel_mode && (passwords.data.is_some() || passwords.sentinel.is_some()) {
                    Some(Zeroizing::new(
                        serde_json::to_string(&(
                            passwords.data.as_ref().map(|value| value.as_str()),
                            passwords.sentinel.as_ref().map(|value| value.as_str()),
                        ))
                        .map_err(|_| "无法编码 Redis 系统凭据")?,
                    ))
                } else {
                    passwords.data
                };
            with_password_change(&entry, secret.as_ref().map(|value| value.as_str()), || {
                save_session_record(&storage, config)
            })
        };
        match replaced {
            Some(profile) => remove_profile_password(profile, save),
            None => save(),
        }
    })
    .await
    .map_err(|_| "保存 Redis 会话失败")?
}

#[tauri::command]
pub async fn redis_duplicate_profile(
    storage: State<'_, SharedStorageState>,
    source_id: String,
    name: String,
) -> Result<(), String> {
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let storage = storage.lock().map_err(|_| "无法访问会话存储")?;
        let mut config = load_session_record(&storage, &source_id)?;
        validate_profile(&config)?;
        let passwords = read_passwords(&config)?;
        config.id = uuid::Uuid::new_v4().to_string();
        config.name = name.trim().to_owned();
        config.last_connected = None;
        validate_profile(&config)?;
        let options = config.redis.get_or_insert_with(ProfileOptions::default);
        options.password_saved = passwords.data.is_some();
        options.sentinel_password_saved = passwords.sentinel.is_some();
        let secret = if options.mode == Mode::Sentinel
            && (options.password_saved || options.sentinel_password_saved)
        {
            Some(Zeroizing::new(
                serde_json::to_string(&(
                    passwords.data.as_ref().map(|value| value.as_str()),
                    passwords.sentinel.as_ref().map(|value| value.as_str()),
                ))
                .map_err(|_| "无法编码 Redis 系统凭据")?,
            ))
        } else {
            passwords.data
        };
        match secret {
            Some(secret) => {
                with_password_change(&credential(&config)?, Some(secret.as_str()), || {
                    save_session_record(&storage, config)
                })
            }
            None => save_session_record(&storage, config),
        }
    })
    .await
    .map_err(|_| "复制 Redis 会话失败")?
}

#[derive(Default)]
pub struct RedisState {
    sessions: Mutex<HashMap<String, Arc<Runtime>>>,
    connecting: std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>,
    shutting_down: AtomicBool,
}

struct PendingConnect<'a> {
    state: &'a RedisState,
    id: &'a str,
    cancelled: Arc<AtomicBool>,
}

impl Drop for PendingConnect<'_> {
    fn drop(&mut self) {
        if let Ok(mut connecting) = self.state.connecting.lock() {
            connecting.remove(self.id);
        }
    }
}

impl RedisState {
    fn begin_connect<'a>(&'a self, id: &'a str) -> Result<PendingConnect<'a>, String> {
        let mut connecting = self.connecting.lock().map_err(|_| "无法登记 Redis 连接")?;
        if self.shutting_down.load(Ordering::Acquire) || connecting.contains_key(id) {
            return Err("Redis 会话无法重复连接".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        connecting.insert(id.to_owned(), cancelled.clone());
        Ok(PendingConnect {
            state: self,
            id,
            cancelled,
        })
    }

    async fn session(&self, id: &str) -> Result<connection::SessionLease, String> {
        let sessions = self.sessions.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("Redis 会话已断开，请重连".into());
        }
        sessions
            .get(id)
            .ok_or("Redis 会话已断开，请重连")?
            .acquire()
    }

    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        let runtimes: Vec<_> = self
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, runtime)| runtime)
            .collect();
        futures::future::join_all(runtimes.iter().map(|runtime| runtime.close())).await;
    }
}

fn redis_error(error: ::redis::RedisError) -> String {
    if error.is_timeout() {
        return "Redis 请求超时".into();
    }
    match error.code() {
        Some("NOAUTH" | "WRONGPASS") => "Redis 认证失败，请检查用户和密码".into(),
        Some("NOPERM") => "当前 Redis 用户没有执行此操作的权限".into(),
        Some("WRONGTYPE") => "Redis 键的类型已变化，请刷新".into(),
        Some("VIEWLIMIT") => "Redis 扫描结果超出预览上限，请缩小范围或升级服务器版本".into(),
        Some("WRITELIMIT") => "列表超过 100000 项，不能在面板中按索引删除成员".into(),
        Some("MUTATEUNSUPPORTED") => "组合修改需要 Redis 7 或更新版本".into(),
        Some("CROSSSLOT") => "Redis 集群的多个目标键必须位于同一槽位".into(),
        Some("MOVED" | "ASK" | "CLUSTERDOWN" | "TRYAGAIN") => {
            "Redis 集群槽位正在迁移或暂不可用，请刷新或重连".into()
        }
        Some("READONLY") => "Redis 网络连接失败，当前数据节点已成为副本，请重连".into(),
        _ if error.is_io_error()
            || error.kind() == ::redis::ErrorKind::ClusterConnectionNotFound =>
        {
            "Redis 网络连接失败，请检查地址、TLS 和连接状态".into()
        }
        Some("ERR")
            if error
                .detail()
                .is_some_and(|detail| detail.contains("DB index")) =>
        {
            "该 Redis DB 不可用".into()
        }
        _ => "Redis 拒绝了该请求或响应不符合预期".into(),
    }
}

#[derive(Serialize)]
pub struct ConnectionHealth {
    primary: Option<Endpoint>,
}

async fn probe(runtime: &Runtime, db: u32) -> Result<ConnectionHealth, String> {
    let (mut connection, primary) = runtime.connection_target(db, false).await?;
    ::redis::cmd("PING")
        .query_async::<String>(&mut connection)
        .await
        .map_err(redis_error)?;
    if runtime.closed.load(Ordering::Acquire) {
        return Err("Redis 会话已关闭".into());
    }
    Ok(ConnectionHealth { primary })
}

#[tauri::command]
pub async fn redis_test_profile(
    storage: State<'_, SharedStorageState>,
    config: SessionConfig,
    password: Option<String>,
    forget_password: bool,
    sentinel_password: Option<String>,
    forget_sentinel_password: Option<bool>,
) -> Result<(), String> {
    validate_profile(&config)?;
    let update = PasswordUpdate::new(
        password,
        forget_password,
        sentinel_password,
        forget_sentinel_password.unwrap_or(false),
    );
    update.validate(&config)?;
    let storage = storage.inner().clone();
    let runtime = tauri::async_runtime::spawn_blocking(move || {
        let saved = {
            let storage = storage.lock().map_err(|_| "无法访问会话存储")?;
            load_session_record(&storage, &config.id).ok()
        };
        let saved = saved
            .as_ref()
            .filter(|saved| is_redis_profile(saved) && same_endpoint(saved, &config));
        let passwords = update.resolve(saved)?;
        Ok::<_, String>(Runtime::new(config, passwords))
    })
    .await
    .map_err(|_| "读取 Redis 会话失败")??;
    let db = runtime
        .profile
        .redis
        .as_ref()
        .map_or(0, |options| options.db);
    probe(&runtime, db).await.map(|_| ())
}

#[tauri::command]
pub async fn redis_connect(
    state: State<'_, RedisState>,
    storage: State<'_, SharedStorageState>,
    id: String,
    profile_id: String,
    endpoint: SessionConfig,
) -> Result<ConnectionHealth, String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Redis 面板 ID 无效")?;
    let pending = state.begin_connect(&id)?;
    let storage = storage.inner().clone();
    let runtime = Arc::new(
        tauri::async_runtime::spawn_blocking(move || {
            let config = {
                let storage = storage.lock().map_err(|_| "无法访问会话存储")?;
                load_session_record(&storage, &profile_id)?
            };
            validate_profile(&config)?;
            if !is_redis_profile(&endpoint) || !same_endpoint(&config, &endpoint) {
                return Err("Redis 连接模式、节点地址或认证配置已修改，请从会话树重新打开".into());
            }
            let passwords = read_passwords(&config)?;
            Ok::<_, String>(Runtime::new(config, passwords))
        })
        .await
        .map_err(|_| "读取 Redis 会话失败")??,
    );
    let lease = runtime.acquire()?;
    {
        let mut sessions = state.sessions.lock().await;
        if pending.cancelled.load(Ordering::Acquire) || state.shutting_down.load(Ordering::Acquire)
        {
            return Err("Redis 会话已关闭".into());
        }
        if sessions.contains_key(&id) {
            return Err("Redis 会话无法重复连接".into());
        }
        sessions.insert(id.clone(), runtime.clone());
    }
    let db = runtime
        .profile
        .redis
        .as_ref()
        .map_or(0, |options| options.db);
    let result = probe(&lease, db).await;
    drop(lease);
    if result.is_err() {
        runtime.close().await;
        let mut sessions = state.sessions.lock().await;
        if sessions
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, &runtime))
        {
            sessions.remove(&id);
        }
    }
    result
}

#[tauri::command]
pub async fn redis_disconnect(state: State<'_, RedisState>, id: String) -> Result<(), String> {
    if let Ok(connecting) = state.connecting.lock() {
        if let Some(cancelled) = connecting.get(&id) {
            cancelled.store(true, Ordering::Release);
        }
    }
    let runtime = state.sessions.lock().await.get(&id).cloned();
    if let Some(runtime) = runtime {
        runtime.close().await;
        let mut sessions = state.sessions.lock().await;
        if sessions
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, &runtime))
        {
            sessions.remove(&id);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn redis_ping(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
) -> Result<ConnectionHealth, String> {
    let runtime = state.session(&id).await?;
    probe(&runtime, db).await
}
