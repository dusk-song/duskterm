use super::{connection::Connection, redis_error, write, Mode, RedisState};
use ::redis::{
    cluster_async::ClusterConnection,
    cluster_routing::{Route, RoutingInfo, SingleNodeRoutingInfo, Slot, SlotAddr},
    FromRedisValue, Script, Value,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::State;

const MAX_KEY: usize = 8_192;
const MAX_VALUE: usize = 65_536;
const MAX_INFO_PREVIEW: usize = 128 * 1024;
const SCAN: &str = include_str!("scan.lua");
const DETAIL: &str = include_str!("detail.lua");
const MUTATE: &str = include_str!("mutate.lua");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bytes {
    base64: String,
    length: usize,
}

impl Bytes {
    fn new(value: Vec<u8>, length: usize) -> Self {
        Self {
            base64: STANDARD.encode(value),
            length,
        }
    }
}

fn bytes(encoded: &str, limit: usize) -> Result<Vec<u8>, String> {
    if encoded.len() > limit.div_ceil(3) * 4 {
        return Err("字节数据超出操作上限".into());
    }
    let decoded = STANDARD.decode(encoded).map_err(|_| "无效的字节数据")?;
    if decoded.len() > limit {
        return Err("字节数据超出操作上限".into());
    }
    Ok(decoded)
}

fn cursor(value: &str) -> Result<&str, String> {
    value.parse::<u64>().map_err(|_| "无效的扫描游标")?;
    Ok(value)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPage {
    keys: Vec<Key>,
    next_cursor: String,
}

type ScanReply = (String, Vec<(Vec<u8>, usize, String)>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterNode {
    id: String,
    address: String,
    replica_of: String,
    role: String,
    status: String,
    slots: String,
    observer: bool,
    #[serde(skip)]
    first_slot: Option<Slot>,
}

async fn cluster_nodes(
    connection: &mut ClusterConnection,
    include_unresolved: bool,
) -> Result<Vec<ClusterNode>, String> {
    let mut command = ::redis::cmd("CLUSTER");
    command.arg("NODES");
    let reply = connection
        .route_command(
            command,
            RoutingInfo::SingleNode(SingleNodeRoutingInfo::Random),
        )
        .await
        .map_err(redis_error)?;
    let raw = String::from_redis_value(reply).map_err(|_| "Redis 集群节点响应无效")?;
    parse_cluster_nodes(&raw, include_unresolved)
}

fn parse_cluster_nodes(raw: &str, include_unresolved: bool) -> Result<Vec<ClusterNode>, String> {
    let mut nodes = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() < 8 {
            return Err("Redis 集群节点响应无效".into());
        }
        let flags: Vec<_> = fields[2].split(',').collect();
        let handshake = flags.contains(&"handshake");
        let no_address = flags.contains(&"noaddr");
        if !include_unresolved && (handshake || no_address) {
            continue;
        }
        let primary = flags.contains(&"master");
        let first_slot = if primary {
            fields[8..]
                .iter()
                .find_map(|range| range.split('-').next()?.parse().ok().and_then(Slot::new))
        } else {
            None
        };
        nodes.push(ClusterNode {
            id: fields[0].to_owned(),
            address: if no_address {
                String::new()
            } else {
                fields[1].split('@').next().unwrap_or_default().to_owned()
            },
            replica_of: fields[3].to_owned(),
            role: if primary {
                "primary".into()
            } else if flags.contains(&"slave") || flags.contains(&"replica") {
                "replica".into()
            } else {
                "unknown".into()
            },
            status: if flags.contains(&"fail") {
                "fail".into()
            } else if flags.contains(&"fail?") {
                "suspected".into()
            } else if handshake {
                "handshake".into()
            } else if no_address {
                "no-address".into()
            } else {
                fields[7].to_owned()
            },
            slots: fields[8..].join(" "),
            observer: flags.contains(&"myself"),
            first_slot,
        });
    }
    nodes.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    if nodes.is_empty() {
        return Err("Redis 集群未返回可用节点".into());
    }
    Ok(nodes)
}

#[tauri::command]
pub async fn redis_cluster_nodes(
    state: State<'_, RedisState>,
    id: String,
) -> Result<Vec<ClusterNode>, String> {
    let runtime = state.session(&id).await?;
    let mut connection = runtime.connection(0, false).await?;
    match &mut connection {
        Connection::Cluster(cluster, _) => cluster_nodes(cluster, true).await,
        _ => Err("当前会话未使用集群模式".into()),
    }
}

#[tauri::command]
pub async fn redis_sentinel_nodes(
    state: State<'_, RedisState>,
    id: String,
) -> Result<Vec<super::connection::SentinelNode>, String> {
    state.session(&id).await?.sentinel_nodes().await
}

fn scan_stamp(target: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    target.hash(&mut hasher);
    hasher.finish()
}

fn distributed_cursor(start: &str, stamp: u64) -> Result<(usize, &str), String> {
    if start == "0" {
        return Ok((0, "0"));
    }
    if let Some((token, rest)) = start.split_once(':') {
        if token.parse() == Ok(stamp) {
            if let Some((index, start)) = rest.split_once(':') {
                if let Ok(index) = index.parse() {
                    return Ok((index, cursor(start)?));
                }
            }
        }
    }
    Err("Redis 拓扑已变化，请刷新键列表后重新扫描".into())
}

async fn scan_cluster(
    connection: &mut ClusterConnection,
    start: &str,
    pattern: &str,
    kind: &str,
    count: usize,
) -> Result<ScanReply, String> {
    let nodes = cluster_nodes(connection, false).await?;
    let primaries: Vec<_> = nodes
        .iter()
        .filter_map(|node| Some((node, node.first_slot?)))
        .collect();
    let stamp = scan_stamp(
        &primaries
            .iter()
            .map(|(node, _)| (&node.id, &node.slots))
            .collect::<Vec<_>>(),
    );
    let (index, start) = distributed_cursor(start, stamp)?;
    let (node, slot) = primaries
        .get(index)
        .ok_or("Redis 无可用主节点，请刷新键列表")?;
    if node.status != "connected" {
        return Err("Redis 集群有主节点暂不可用，请稍后刷新键列表".into());
    }
    let mut commands = ::redis::pipe();
    commands
        .cmd("CLUSTER")
        .arg("MYID")
        .cmd("EVAL")
        .arg(SCAN)
        .arg(0)
        .arg(start)
        .arg(pattern)
        .arg(kind)
        .arg(count);
    let route = SingleNodeRoutingInfo::SpecificNode(Route::with_slot(*slot, SlotAddr::Master));
    let results = connection
        .route_pipeline(commands, 0, 2, route)
        .await
        .map_err(redis_error)?;
    let (actual, (next, keys)): (String, ScanReply) =
        FromRedisValue::from_redis_value(Value::Array(results))
            .map_err(|_| "Redis 集群扫描响应无效")?;
    if actual != node.id {
        return Err("Redis 集群扫描节点已变化，请刷新键列表".into());
    }
    let next = if next != "0" {
        format!("{stamp}:{index}:{next}")
    } else if index + 1 < primaries.len() {
        format!("{stamp}:{}:0", index + 1)
    } else {
        "0".into()
    };
    Ok((next, keys))
}

#[derive(Serialize)]
struct Key {
    key: Bytes,
    kind: String,
}

#[tauri::command]
pub async fn redis_scan(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    start: String,
    pattern: String,
    kind: String,
    bulk: Option<bool>,
) -> Result<KeyPage, String> {
    if pattern.len() > 1024
        || !matches!(
            kind.as_str(),
            "" | "string" | "hash" | "list" | "set" | "zset"
        )
    {
        return Err("键名筛选条件无效".into());
    }
    let runtime = state.session(&id).await?;
    let (mut connection, primary) = runtime.connection_target(db, false).await?;
    let pattern = if pattern.is_empty() { "*" } else { &pattern };
    let count = if bulk.unwrap_or(false) { 512 } else { 256 };
    let (next_cursor, keys) = match &mut connection {
        Connection::Cluster(cluster, _) => {
            scan_cluster(cluster, &start, pattern, &kind, count).await?
        }
        Connection::Direct(direct) => {
            let stamp = primary.as_ref().map(scan_stamp);
            let start = match stamp {
                Some(stamp) => distributed_cursor(&start, stamp)?.1,
                None => cursor(&start)?,
            };
            let (mut next, keys): ScanReply = Script::new(SCAN)
                .arg(start)
                .arg(pattern)
                .arg(kind)
                .arg(count)
                .invoke_async(direct)
                .await
                .map_err(redis_error)?;
            if let Some(stamp) = stamp.filter(|_| next != "0") {
                next = format!("{stamp}:0:{next}");
            }
            (next, keys)
        }
    };
    Ok(KeyPage {
        next_cursor,
        keys: keys
            .into_iter()
            .map(|(value, len, kind)| Key {
                key: Bytes::new(value, len),
                kind,
            })
            .collect(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    kind: String,
    ttl_ms: i64,
    size: usize,
    value: Option<Bytes>,
    entries: Vec<Entry>,
    next_cursor: String,
    primary: Option<super::Endpoint>,
}

#[derive(Serialize)]
struct Entry {
    field: Bytes,
    value: Bytes,
    score: String,
    index: Option<u64>,
}

#[tauri::command]
pub async fn redis_detail(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    key: String,
    start: String,
) -> Result<Detail, String> {
    let key = bytes(&key, MAX_KEY)?;
    let start = cursor(&start)?;
    let runtime = state.session(&id).await?;
    let (mut connection, primary) = runtime.connection_target(db, false).await?;
    type Row = (Vec<u8>, usize, Vec<u8>, usize, String);
    type DetailReply = (String, i64, usize, String, Vec<u8>, Vec<Row>);
    let (kind, ttl_ms, size, next_cursor, value, entries): DetailReply = Script::new(DETAIL)
        .key(key)
        .arg(start)
        .invoke_async(&mut connection)
        .await
        .map_err(redis_error)?;
    let indexed = kind == "list";
    let offset = start.parse::<u64>().unwrap_or_default();
    Ok(Detail {
        value: (kind == "string").then(|| Bytes::new(value, size)),
        kind,
        ttl_ms,
        size,
        next_cursor,
        primary,
        entries: entries
            .into_iter()
            .enumerate()
            .map(|(position, (field, field_len, value, len, score))| Entry {
                field: Bytes::new(field, field_len),
                value: Bytes::new(value, len),
                score,
                index: indexed.then_some(offset.saturating_add(position as u64)),
            })
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutation {
    action: String,
    kind: String,
    key: String,
    field: Option<String>,
    expected: Option<String>,
    expected_size: Option<u64>,
    value: Option<String>,
    value_base64: Option<String>,
    score: Option<f64>,
    expected_score: Option<f64>,
    index: Option<u64>,
    rename_to: Option<String>,
    ttl_ms: Option<u64>,
    operation_id: Option<String>,
}

#[tauri::command]
pub async fn redis_ttl(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    key: String,
) -> Result<i64, String> {
    let key = bytes(&key, MAX_KEY)?;
    let runtime = state.session(&id).await?;
    let mut connection = runtime.connection(db, false).await?;
    ::redis::cmd("PTTL")
        .arg(key)
        .query_async(&mut connection)
        .await
        .map_err(redis_error)
}

#[tauri::command]
pub async fn redis_mutate(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    change: Mutation,
) -> Result<(), String> {
    if change.operation_id.is_none() {
        return Err("Redis 写入缺少操作标识".into());
    }
    apply_mutation(&state, &id, db, change).await
}

async fn apply_mutation(
    state: &RedisState,
    id: &str,
    db: u32,
    change: Mutation,
) -> Result<(), String> {
    if !matches!(
        change.action.as_str(),
        "create" | "metadata" | "string" | "add" | "update" | "remove" | "delete"
    ) || !matches!(
        change.kind.as_str(),
        "string" | "hash" | "list" | "set" | "zset"
    ) || change.score.is_some_and(|score| !score.is_finite())
        || change
            .expected_score
            .is_some_and(|score| !score.is_finite())
        || change
            .value
            .as_ref()
            .is_some_and(|value| value.len() > MAX_VALUE)
        || change.value_base64.is_some()
            && (change.value.is_some()
                || change.kind != "string"
                || !matches!(change.action.as_str(), "create" | "string"))
        || change.ttl_ms.is_some_and(|ttl| ttl > 31_536_000_000_000)
        || matches!(change.action.as_str(), "string" | "update" | "remove")
            && change.expected.is_none()
        || change.kind == "list"
            && matches!(change.action.as_str(), "update" | "remove")
            && change.index.is_none_or(|index| index > u32::MAX.into())
        || change.kind == "zset"
            && matches!(change.action.as_str(), "create" | "add" | "update")
            && change.score.is_none()
        || change.kind == "zset"
            && matches!(change.action.as_str(), "update" | "remove")
            && change.expected_score.is_none()
    {
        return Err("Redis 修改参数无效或超出上限".into());
    }
    let key = bytes(&change.key, MAX_KEY)?;
    let renamed = change
        .rename_to
        .as_deref()
        .map(|value| bytes(value, MAX_KEY))
        .transpose()?;
    let runtime = state.session(id).await?;
    if runtime.mode == Mode::Cluster
        && renamed
            .as_ref()
            .is_some_and(|target| Slot::for_key(&key) != Slot::for_key(target))
    {
        return Err("Redis 集群仅支持同槽重命名，可给两个键使用相同的 {标签}".into());
    }
    let field = change
        .field
        .as_deref()
        .map(|value| bytes(value, MAX_KEY))
        .transpose()?
        .unwrap_or_default();
    let expected = change
        .expected
        .as_deref()
        .map(|value| bytes(value, MAX_VALUE))
        .transpose()?
        .unwrap_or_default();
    let value = match change.value_base64 {
        Some(encoded) => bytes(&encoded, MAX_VALUE)?,
        None => change.value.unwrap_or_default().into_bytes(),
    };
    let operation = change
        .operation_id
        .as_deref()
        .map(write::operation_id)
        .transpose()?
        .unwrap_or_else(uuid::Uuid::new_v4);
    runtime
        .writes
        .execute(operation, async {
            let mut connection = runtime.connection(db, false).await?;
            let response = Script::new(MUTATE)
                .key(&key)
                .key(renamed.as_ref().unwrap_or(&key))
                .arg(change.action)
                .arg(change.kind)
                .arg(field)
                .arg(expected)
                .arg(value)
                .arg(change.score.unwrap_or_default())
                .arg(change.expected_score.unwrap_or_default())
                .arg(change.index.unwrap_or_default())
                .arg(change.ttl_ms.map_or(-1, |ttl| ttl as i64))
                .arg(operation.as_bytes().as_slice())
                .arg(
                    change
                        .expected_size
                        .map_or(-1, |size| i64::try_from(size).unwrap_or(i64::MAX)),
                )
                .arg(u8::from(change.expected.is_some()))
                .invoke_async::<String>(&mut connection)
                .await;
            match response {
                Ok(status) if status == "ok" => Ok(status),
                Ok(status) if status == "uncertain" => Err(write::UNKNOWN.into()),
                Ok(_) => Err("键或成员已变化，或目标已经存在，请刷新确认后再操作".into()),
                Err(error) => Err(write::error(error)),
            }
        })
        .await
        .map(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseReport {
    total: Option<u32>,
    keys: std::collections::BTreeMap<u32, String>,
    keyspace_available: bool,
    warnings: Vec<String>,
}

fn parse_keyspace(raw: &str) -> Result<std::collections::BTreeMap<u32, String>, String> {
    let mut keys = std::collections::BTreeMap::new();
    for line in raw.lines().filter(|line| line.starts_with("db")) {
        let (name, fields) = line.split_once(':').ok_or("Redis 库统计响应无效")?;
        let id = name[2..].parse::<u32>().map_err(|_| "Redis 库编号无效")?;
        let count = fields
            .split(',')
            .find_map(|field| field.strip_prefix("keys="))
            .ok_or("Redis 库键数缺失")?;
        count.parse::<u64>().map_err(|_| "Redis 库键数无效")?;
        keys.insert(id, count.to_owned());
    }
    Ok(keys)
}

#[tauri::command]
pub async fn redis_databases(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
) -> Result<DatabaseReport, String> {
    let runtime = state.session(&id).await?;
    let mut connection = runtime.connection(db, false).await?;
    let mut report = DatabaseReport {
        total: None,
        keys: Default::default(),
        keyspace_available: false,
        warnings: Vec::new(),
    };
    if matches!(&connection, Connection::Cluster(..)) {
        report.total = Some(1);
        // redis-rs routes DBSIZE to all primaries and sums the replies.
        match ::redis::cmd("DBSIZE")
            .query_async::<u64>(&mut connection)
            .await
        {
            Ok(count) => {
                report.keys.insert(0, count.to_string());
                report.keyspace_available = true;
            }
            Err(error) => report
                .warnings
                .push(format!("无法读取集群键数：{}", redis_error(error))),
        }
        return Ok(report);
    }
    match ::redis::cmd("CONFIG")
        .arg("GET")
        .arg("databases")
        .query_async::<std::collections::HashMap<String, String>>(&mut connection)
        .await
    {
        Ok(config) => {
            report.total = config
                .get("databases")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|count| *count > 0);
            if report.total.is_none() {
                report
                    .warnings
                    .push("服务端未返回数据库总数，仅列出已发现的库".into());
            }
        }
        Err(error) => report.warnings.push(format!(
            "无法读取库总数，仅列出已发现的库：{}",
            redis_error(error)
        )),
    }
    match read_instance_info(&mut connection, "keyspace")
        .await
        .and_then(|raw| parse_keyspace(&raw))
    {
        Ok(keys) => {
            report.keys = keys;
            report.keyspace_available = true;
        }
        Err(error) => report.warnings.push(format!("无法读取各库键数：{error}")),
    }
    Ok(report)
}

#[tauri::command]
pub async fn redis_info(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    section: String,
) -> Result<String, String> {
    if !matches!(
        section.as_str(),
        "" | "server" | "memory" | "stats" | "keyspace" | "replication" | "cluster"
    ) {
        return Err("Redis 状态分区无效".into());
    }
    let runtime = state.session(&id).await?;
    let (mut connection, primary) = runtime.connection_target(db, false).await?;
    if section == "cluster" {
        let Connection::Cluster(cluster, _) = &mut connection else {
            return Err("当前会话未使用集群模式".into());
        };
        let mut command = ::redis::cmd("CLUSTER");
        command.arg("INFO");
        let output = cluster
            .route_command(
                command,
                RoutingInfo::SingleNode(SingleNodeRoutingInfo::Random),
            )
            .await
            .map_err(redis_error)?;
        return Ok(String::from_redis_value(output)
            .map_err(|_| "Redis 集群状态响应无效")?
            .chars()
            .take(32_768)
            .collect());
    }
    let mut output = read_instance_info(&mut connection, &section).await?;
    if section == "replication" {
        if let Some(primary) = primary {
            let host = primary.host.trim_matches(['[', ']']);
            let address = if host.contains(':') {
                format!("[{host}]:{}", primary.port)
            } else {
                format!("{host}:{}", primary.port)
            };
            output.insert_str(0, &format!("duskterm_primary:{address}\n"));
        }
    }
    if matches!(section.as_str(), "" | "replication") {
        if output.len() > MAX_INFO_PREVIEW {
            let preview = &output[..output.floor_char_boundary(MAX_INFO_PREVIEW)];
            output.truncate(preview.rfind('\n').unwrap_or(preview.len()));
            output.push_str("\r\n\r\n# DuskTerm: INFO 输出已截断\r\n");
        }
        return Ok(output);
    }
    Ok(output.chars().take(32_768).collect())
}

async fn read_instance_info(connection: &mut Connection, section: &str) -> Result<String, String> {
    let mut command = ::redis::cmd("INFO");
    if !section.is_empty() {
        command.arg(section);
    }
    match connection {
        Connection::Cluster(cluster, _) if section.is_empty() => {
            // A stable shard avoids mixing primary/replica INFO snapshots between refreshes.
            let route = SingleNodeRoutingInfo::SpecificNode(Route::with_slot(
                Slot::new(0).expect("slot zero is valid"),
                SlotAddr::Master,
            ));
            let output = cluster
                .route_command(command, RoutingInfo::SingleNode(route))
                .await
                .map_err(redis_error)?;
            String::from_redis_value(output).map_err(|_| "Redis INFO 响应无效".into())
        }
        _ => command.query_async(connection).await.map_err(redis_error),
    }
}

pub(super) fn printable(value: Value) -> String {
    match value {
        Value::Nil => "(nil)".into(),
        Value::BulkString(bytes) => String::from_utf8(bytes).unwrap_or_else(|error| {
            let bytes = error.into_bytes();
            format!(
                "[{} 字节] Base64 {}",
                bytes.len(),
                STANDARD.encode(&bytes[..bytes.len().min(MAX_VALUE)])
            )
        }),
        Value::SimpleString(text) => text,
        Value::Int(number) => number.to_string(),
        Value::Okay => "OK".into(),
        _ => "此响应类型暂不可预览".into(),
    }
}

#[tauri::command]
pub async fn redis_preview_msgpack(data: String) -> Result<String, String> {
    let data = bytes(&data, MAX_VALUE)?;
    tokio::task::spawn_blocking(move || preview_msgpack(data))
        .await
        .map_err(|_| "MessagePack 预览任务已中断")?
}

fn preview_msgpack(data: Vec<u8>) -> Result<String, String> {
    use serde::Deserialize;
    use std::io::{self, Write};

    struct Preview(Vec<u8>);
    impl Write for Preview {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0.len() + bytes.len() > 512 * 1024 {
                return Err(io::Error::other("MessagePack preview exceeds 512 KiB"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut cursor = std::io::Cursor::new(data.as_slice());
    let mut decoder = rmp_serde::Deserializer::new(&mut cursor);
    decoder.set_max_depth(32);
    let value = serde_json::Value::deserialize(&mut decoder)
        .map_err(|_| "MessagePack 格式不匹配或包含暂不支持的类型")?;
    if cursor.position() != data.len() as u64 {
        return Err("MessagePack 数据包含多余字节".into());
    }
    let mut preview = Preview(Vec::new());
    serde_json::to_writer_pretty(&mut preview, &value)
        .map_err(|_| "MessagePack 格式化超出预览上限")?;
    String::from_utf8(preview.0).map_err(|_| "无法生成 MessagePack 预览".into())
}
