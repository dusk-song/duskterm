use super::{redis_error, write::WriteHistory, Endpoint, Mode, Passwords};
use crate::storage::SessionConfig;
use ::redis::{
    aio::{ConnectionLike, MultiplexedConnection},
    cluster::ClusterClient,
    cluster_async::ClusterConnection,
    AsyncConnectionConfig, Client, Cmd, ConnectionAddr, ConnectionInfo, FromRedisValue,
    IntoConnectionInfo, Pipeline, ProtocolVersion, RedisConnectionInfo, RedisFuture, RedisResult,
    Value,
};
use futures::{stream::FuturesUnordered, StreamExt};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(super) enum Connection {
    Direct(MultiplexedConnection),
    Cluster(ClusterConnection, Arc<Access>),
}

impl ConnectionLike for Connection {
    fn req_packed_command<'a>(&'a mut self, cmd: &'a Cmd) -> RedisFuture<'a, Value> {
        match self {
            Self::Direct(connection) => connection.req_packed_command(cmd),
            Self::Cluster(connection, access) => Box::pin(async move {
                let error = match connection.req_packed_command(cmd).await {
                    Ok(value) => return Ok(value),
                    Err(error) => error,
                };
                // Only a Redis redirect proves that this command was not executed.
                // Transport failures can hide a completed write.
                let (Some("MOVED" | "ASK"), Some((address, _))) =
                    (error.code(), error.redirect_node())
                else {
                    return Err(error);
                };
                let Some(endpoint) = parse_endpoint(address) else {
                    return Err(error);
                };
                let mut target = access.connect(&endpoint, 0).await?;
                if error.code() == Some("ASK") {
                    let replies = ::redis::pipe()
                        .cmd("ASKING")
                        .add_command(cmd.clone())
                        .query_async::<(Value, Value)>(&mut target)
                        .await?;
                    return Ok(replies.1);
                }
                target.req_packed_command(cmd).await
            }),
        }
    }

    fn req_packed_commands<'a>(
        &'a mut self,
        pipeline: &'a Pipeline,
        offset: usize,
        count: usize,
    ) -> RedisFuture<'a, Vec<Value>> {
        match self {
            Self::Direct(connection) => connection.req_packed_commands(pipeline, offset, count),
            Self::Cluster(connection, _) => connection.req_packed_commands(pipeline, offset, count),
        }
    }

    fn get_db(&self) -> i64 {
        match self {
            Self::Direct(connection) => connection.get_db(),
            Self::Cluster(_, _) => 0,
        }
    }
}

fn parse_endpoint(address: &str) -> Option<Endpoint> {
    let (host, port) = address.rsplit_once(':')?;
    let endpoint = Endpoint {
        host: host.trim_matches(['[', ']']).to_owned(),
        port: port.parse().ok()?,
    };
    endpoint.valid().then_some(endpoint)
}

pub(super) struct Access {
    username: String,
    password: Option<Zeroizing<String>>,
    tls: bool,
    timeout: Duration,
}

#[derive(serde::Serialize)]
pub struct SentinelNode {
    endpoint: Endpoint,
    status: &'static str,
}

impl Access {
    fn endpoint(&self, endpoint: &Endpoint, db: u32) -> RedisResult<ConnectionInfo> {
        let host = endpoint.host.trim_matches(['[', ']']).to_owned();
        let addr = if self.tls {
            ConnectionAddr::TcpTls {
                host,
                port: endpoint.port,
                insecure: false,
                tls_params: None,
            }
        } else {
            ConnectionAddr::Tcp(host, endpoint.port)
        };
        let mut settings = RedisConnectionInfo::default()
            .set_db(db.into())
            .set_protocol(ProtocolVersion::RESP2);
        if !self.username.is_empty() {
            settings = settings.set_username(&self.username);
        }
        if let Some(password) = &self.password {
            settings = settings.set_password(password.as_str());
        }
        Ok(addr.into_connection_info()?.set_redis_settings(settings))
    }

    async fn connect(&self, endpoint: &Endpoint, db: u32) -> RedisResult<MultiplexedConnection> {
        let client = Client::open(self.endpoint(endpoint, db)?)?;
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(self.timeout))
            .set_response_timeout(Some(Duration::from_secs(6)));
        client
            .get_multiplexed_async_connection_with_config(&config)
            .await
    }
}

struct CachedConnection {
    db: u32,
    primary: Option<Endpoint>,
    connection: Connection,
}

const SESSION_REQUESTS: u32 = 8;

pub(super) struct SessionLease {
    runtime: Arc<Runtime>,
    _permit: OwnedSemaphorePermit,
}

impl std::ops::Deref for SessionLease {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

pub(super) struct Runtime {
    pub(super) profile: SessionConfig,
    pub(super) mode: Mode,
    data: Arc<Access>,
    sentinel: Access,
    seeds: Vec<Endpoint>,
    sentinel_service: String,
    primary: Mutex<Option<(Endpoint, Instant)>>,
    browse: Mutex<Option<CachedConnection>>,
    console: Mutex<Option<CachedConnection>>,
    pub(super) closed: AtomicBool,
    requests: Arc<Semaphore>,
    pub(super) writes: WriteHistory,
}

impl Runtime {
    pub(super) fn new(profile: SessionConfig, passwords: Passwords) -> Self {
        let options = profile.redis.clone().unwrap_or_default();
        let timeout = Duration::from_secs(profile.connect_timeout.unwrap_or(5).clamp(1, 30));
        let data = Arc::new(Access {
            username: profile.username.clone(),
            password: passwords.data,
            tls: options.tls,
            timeout,
        });
        let sentinel = Access {
            username: options.sentinel_username,
            password: passwords.sentinel,
            tls: options.sentinel_tls,
            timeout,
        };
        let mut seeds = vec![Endpoint {
            host: profile.host.clone(),
            port: profile.port,
        }];
        seeds.extend(options.seeds);
        Self {
            profile,
            mode: options.mode,
            data,
            sentinel,
            seeds,
            sentinel_service: options.sentinel_service,
            primary: Mutex::default(),
            browse: Mutex::default(),
            console: Mutex::default(),
            closed: AtomicBool::new(false),
            requests: Arc::new(Semaphore::new(SESSION_REQUESTS as usize)),
            writes: WriteHistory::default(),
        }
    }

    pub(super) fn acquire(self: &Arc<Self>) -> Result<SessionLease, String> {
        let permit = self
            .requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Redis 会话请求较多，请稍后重试")?;
        if self.closed.load(Ordering::Acquire) {
            return Err("Redis 会话已断开，请重连".into());
        }
        Ok(SessionLease {
            runtime: self.clone(),
            _permit: permit,
        })
    }

    pub(super) async fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let _drained = self.requests.acquire_many(SESSION_REQUESTS).await;
        self.browse.lock().await.take();
        self.console.lock().await.take();
    }

    async fn discover_primary(&self) -> Result<Endpoint, String> {
        let pending = FuturesUnordered::new();
        for seed in &self.seeds {
            pending.push(async move {
                let mut connection = self.sentinel.connect(seed, 0).await?;
                ::redis::cmd("SENTINEL")
                    .arg("GET-MASTER-ADDR-BY-NAME")
                    .arg(&self.sentinel_service)
                    .query_async::<Option<(String, u16)>>(&mut connection)
                    .await
            });
        }
        let mut pending = pending;
        let mut auth_error = None;
        let mut checked = HashSet::new();
        while let Some(answer) = pending.next().await {
            match answer {
                Ok(Some((host, port))) => {
                    let endpoint = Endpoint { host, port };
                    if endpoint.valid() && checked.insert(endpoint.clone()) {
                        let role = async {
                            let mut connection = self.data.connect(&endpoint, 0).await?;
                            is_primary(&mut connection).await
                        }
                        .await;
                        match role {
                            Ok(true) => return Ok(endpoint),
                            Err(error)
                                if matches!(
                                    error.code(),
                                    Some("NOAUTH" | "WRONGPASS" | "NOPERM")
                                ) =>
                            {
                                auth_error = Some(redis_error(error));
                            }
                            _ => {}
                        }
                    }
                }
                Err(error) if matches!(error.code(), Some("NOAUTH" | "WRONGPASS" | "NOPERM")) => {
                    auth_error = Some(redis_error(error));
                }
                _ => {}
            }
        }
        Err(auth_error.unwrap_or_else(|| "Redis 网络连接失败，哨兵无法发现指定服务的主节点".into()))
    }

    pub(super) async fn sentinel_nodes(&self) -> Result<Vec<SentinelNode>, String> {
        if self.mode != Mode::Sentinel {
            return Err("当前会话未使用哨兵模式".into());
        }
        let read = async {
            let mut pending = FuturesUnordered::new();
            for seed in &self.seeds {
                pending.push(async move {
                    let result = async {
                        let mut connection = self.sentinel.connect(seed, 0).await?;
                        ::redis::cmd("SENTINEL")
                            .arg("SENTINELS")
                            .arg(&self.sentinel_service)
                            .query_async::<Vec<HashMap<String, String>>>(&mut connection)
                            .await
                    }
                    .await;
                    (seed, result)
                });
            }
            let mut auth_error = None;
            while let Some((seed, result)) = pending.next().await {
                match result {
                    Ok(peers) => {
                        let mut seen = HashSet::from([seed.clone()]);
                        let mut nodes = vec![SentinelNode {
                            endpoint: seed.clone(),
                            status: "responding",
                        }];
                        for peer in peers {
                            if let Some(node) = sentinel_peer(&peer) {
                                if seen.insert(node.endpoint.clone()) {
                                    nodes.push(node);
                                }
                            }
                        }
                        return Ok(nodes);
                    }
                    Err(error)
                        if matches!(error.code(), Some("NOAUTH" | "WRONGPASS" | "NOPERM")) =>
                    {
                        auth_error = Some(redis_error(error));
                    }
                    Err(_) => {}
                }
            }
            Err(auth_error.unwrap_or_else(|| "无法读取哨兵节点列表".into()))
        };
        tokio::time::timeout(self.sentinel.timeout, read)
            .await
            .map_err(|_| "读取哨兵节点列表超时")?
    }

    async fn sentinel_primary(&self) -> Result<Endpoint, String> {
        let mut cached = self.primary.lock().await;
        if let Some((endpoint, checked)) = cached.as_ref() {
            if checked.elapsed() < Duration::from_secs(3) {
                return Ok(endpoint.clone());
            }
        }
        let endpoint = tokio::time::timeout(self.sentinel.timeout, self.discover_primary())
            .await
            .map_err(|_| "Redis 网络连接失败，哨兵发现主节点超时")??;
        *cached = Some((endpoint.clone(), Instant::now()));
        Ok(endpoint)
    }

    async fn open_cluster(&self) -> Result<Connection, String> {
        let seeds = self
            .seeds
            .iter()
            .map(|seed| self.data.endpoint(seed, 0))
            .collect::<RedisResult<Vec<_>>>()
            .map_err(redis_error)?;
        let client = ClusterClient::builder(seeds)
            .retries(0)
            .connection_timeout(self.data.timeout)
            .response_timeout(Duration::from_secs(6))
            .overall_response_timeout(Some(Duration::from_secs(8)))
            .build()
            .map_err(redis_error)?;
        let connection = tokio::time::timeout(self.data.timeout, client.get_async_connection())
            .await
            .map_err(|_| "Redis 网络连接失败，集群拓扑发现超时")?
            .map_err(redis_error)?;
        Ok(Connection::Cluster(connection, self.data.clone()))
    }

    async fn open_at(&self, db: u32, primary: Option<&Endpoint>) -> Result<Connection, String> {
        if self.mode == Mode::Cluster {
            return self.open_cluster().await;
        }
        let mut connection = self
            .data
            .connect(primary.unwrap_or(&self.seeds[0]), db)
            .await
            .map_err(redis_error)?;
        if primary.is_some() {
            if !is_primary(&mut connection).await.map_err(redis_error)? {
                *self.primary.lock().await = None;
                return Err("Redis 网络连接失败，哨兵返回的主节点校验失败，请重连".into());
            }
        }
        Ok(Connection::Direct(connection))
    }

    pub(super) async fn connection(&self, db: u32, console: bool) -> Result<Connection, String> {
        self.connection_target(db, console)
            .await
            .map(|(connection, _)| connection)
    }

    pub(super) async fn connection_target(
        &self,
        db: u32,
        console: bool,
    ) -> Result<(Connection, Option<Endpoint>), String> {
        if self.mode == Mode::Cluster && db != 0 {
            return Err("Redis 集群仅支持 DB 0".into());
        }
        let source = if console { &self.console } else { &self.browse };
        let mut slot = source.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err("Redis 会话已关闭".into());
        }
        let primary = if self.mode == Mode::Sentinel {
            Some(self.sentinel_primary().await?)
        } else {
            None
        };
        if let Some(cached) = slot.as_ref() {
            if cached.db == db && cached.primary == primary {
                return Ok((cached.connection.clone(), primary));
            }
        }
        let connection = self.open_at(db, primary.as_ref()).await?;
        if self.closed.load(Ordering::Acquire) {
            return Err("Redis 会话已关闭".into());
        }
        *slot = Some(CachedConnection {
            db,
            primary: primary.clone(),
            connection: connection.clone(),
        });
        Ok((connection, primary))
    }
}

fn sentinel_peer(peer: &HashMap<String, String>) -> Option<SentinelNode> {
    let endpoint = Endpoint {
        host: peer.get("ip")?.trim_matches(['[', ']']).to_owned(),
        port: peer.get("port")?.parse().ok()?,
    };
    if !endpoint.valid() {
        return None;
    }
    let flags = peer.get("flags")?.split(',').collect::<Vec<_>>();
    let status = if flags.contains(&"o_down") {
        "fail"
    } else if flags.contains(&"s_down") {
        "suspected"
    } else if flags.contains(&"disconnected") {
        "disconnected"
    } else {
        "known"
    };
    Some(SentinelNode { endpoint, status })
}

async fn is_primary(connection: &mut MultiplexedConnection) -> RedisResult<bool> {
    let response: Vec<Value> = ::redis::cmd("ROLE").query_async(connection).await?;
    Ok(response
        .into_iter()
        .next()
        .and_then(|role| String::from_redis_value(role).ok())
        .as_deref()
        == Some("master"))
}
