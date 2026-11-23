use super::{data::printable, redis_error, write, RedisState};
use ::redis::{Cmd, Script, Value};
use tauri::State;

const MAX_ARG: usize = 8192;

fn validate(args: &[String]) -> Result<(String, bool), String> {
    if args.len() > 4 || args.iter().any(|arg| arg.len() > MAX_ARG) {
        return Err("控制台命令超出大小上限".into());
    }
    let verb = args
        .first()
        .map(|arg| arg.to_ascii_uppercase())
        .unwrap_or_default();
    let (arity, writes) = match verb.as_str() {
        "PING" | "DBSIZE" => (1, false),
        "GET" | "TYPE" | "TTL" | "PTTL" | "EXISTS" | "HLEN" | "LLEN" | "SCARD" | "ZCARD" => {
            (2, false)
        }
        "HGET" | "ZSCORE" => (3, false),
        "INCR" | "DECR" | "PERSIST" | "UNLINK" => (2, true),
        "SET" | "SETNX" | "HDEL" | "SADD" | "SREM" | "ZREM" | "EXPIRE" => (3, true),
        "HSET" | "ZADD" => (4, true),
        _ => return Err("此命令未开放，可在命令输入框旁查看支持列表".into()),
    };
    if args.len() != arity {
        return Err("Redis 命令参数数量不正确，每次只接受单键操作".into());
    }
    if verb == "ZADD"
        && args[2]
            .parse::<f64>()
            .map_or(true, |score| !score.is_finite())
    {
        return Err("ZADD 需要有限数值作为分数".into());
    }
    if verb == "EXPIRE" && !matches!(args[2].parse::<u64>(), Ok(1..=31_536_000_000)) {
        return Err("EXPIRE 需要正整数秒数，删除键请使用 UNLINK".into());
    }
    Ok((verb, writes))
}

#[tauri::command]
pub async fn redis_console(
    state: State<'_, RedisState>,
    id: String,
    db: u32,
    args: Vec<String>,
    operation_id: Option<String>,
) -> Result<String, String> {
    let (verb, writes) = validate(&args)?;
    let runtime = state.session(&id).await?;
    if writes {
        let operation_id =
            write::operation_id(operation_id.as_deref().ok_or("请先确认执行 Redis 写命令")?)?;
        return runtime.writes.execute(operation_id, async {
            let mut connection = runtime.connection(db, true).await?;
            let result = if verb == "SET" {
                // SET must not turn a potentially large collection into a string.
                Script::new("local kind = redis.call('TYPE', KEYS[1]).ok; if kind ~= 'none' and kind ~= 'string' then return redis.error_reply('WRONGTYPE SET expects a string') end; return redis.call('SET', KEYS[1], ARGV[1])")
                    .key(&args[1]).arg(&args[2]).invoke_async(&mut connection).await
            } else {
                command(&verb, &args).query_async::<Value>(&mut connection).await
            };
            result.map(printable).map_err(write::error)
        }).await;
    }
    let mut connection = runtime.connection(db, true).await?;
    let result = match verb.as_str() {
        "GET" | "HGET" => {
            let script = if verb == "GET" {
                "local n = redis.call('STRLEN', KEYS[1]); if n > 65536 then return '[值超过控制台预览上限]' end; return redis.call('GET', KEYS[1])"
            } else {
                "local n = redis.call('HSTRLEN', KEYS[1], ARGV[1]); if n > 65536 then return '[值超过控制台预览上限]' end; return redis.call('HGET', KEYS[1], ARGV[1])"
            };
            Script::new(script).key(&args[1]).arg(args.get(2).map(String::as_str).unwrap_or_default())
                .invoke_async(&mut connection).await
        }
        _ => command(&verb, &args).query_async(&mut connection).await,
    }.map_err(redis_error)?;
    Ok(printable(result))
}

fn command(verb: &str, args: &[String]) -> Cmd {
    let mut command = ::redis::cmd(verb);
    command.arg(&args[1..]);
    command
}
