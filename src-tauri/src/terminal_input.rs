use tokio::sync::mpsc::{error::TrySendError, Sender};

pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const INPUT_CHUNK_BYTES: usize = 32 * 1024;
pub const INPUT_TOO_LARGE: &str = "输入超过 4 MiB，请分批粘贴或使用文件传输；本次输入未发送";

// The caller serializes producers for the entire batch. Reserve every slot
// before sending anything, so a full queue never accepts half a paste.
pub fn enqueue(tx: &Sender<Vec<u8>>, bytes: Vec<u8>) -> Result<(), String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(INPUT_TOO_LARGE.into());
    }
    if bytes.is_empty() {
        return Ok(());
    }
    let permits = tx
        .try_reserve_many(bytes.len().div_ceil(INPUT_CHUNK_BYTES))
        .map_err(|error| match error {
            TrySendError::Full(_) => "输入队列已满，本次输入未发送，请稍后重试".to_string(),
            TrySendError::Closed(_) => "Terminal session input channel is closed".to_string(),
        })?;
    for (chunk, permit) in bytes.chunks(INPUT_CHUNK_BYTES).zip(permits) {
        permit.send(chunk.to_vec());
    }
    Ok(())
}
