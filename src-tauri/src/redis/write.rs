use std::{collections::VecDeque, future::Future};
use tokio::sync::Mutex;
use uuid::Uuid;

pub(super) const UNKNOWN: &str = "Redis 写入结果未知，请刷新核实后再重试";
const HISTORY_LIMIT: usize = 128;

type Outcome = Result<String, String>;

#[derive(Default)]
pub(super) struct WriteHistory(Mutex<VecDeque<(Uuid, Outcome)>>);

impl WriteHistory {
    pub(super) async fn execute(
        &self,
        operation_id: Uuid,
        write: impl Future<Output = Outcome>,
    ) -> Outcome {
        let mut history = self.0.try_lock().map_err(|_| "已有 Redis 写入正在执行")?;
        if let Some((_, outcome)) = history.iter().find(|(id, _)| *id == operation_id) {
            return outcome.clone();
        }
        if history.len() == HISTORY_LIMIT {
            history.pop_front();
        }
        // Cancellation may happen after Redis received the command.
        history.push_back((operation_id, Err(UNKNOWN.into())));
        let outcome = write.await;
        history
            .back_mut()
            .expect("current write is in the history")
            .1 = outcome.clone();
        outcome
    }
}

pub(super) fn operation_id(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| "Redis 写入操作标识无效".into())
}

pub(super) fn error(error: ::redis::RedisError) -> String {
    if error.is_timeout()
        || error.is_io_error()
        || error.kind() == ::redis::ErrorKind::ClusterConnectionNotFound
    {
        UNKNOWN.into()
    } else {
        super::redis_error(error)
    }
}
