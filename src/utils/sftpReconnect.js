import { watch } from 'vue';

// All panels for a session await the same recovery attempt.
const pending = new Map();
export function pendingSftpReconnect(sessionId) { return pending.get(sessionId); }
export function reconnectSftpOnce(sessionId, reconnect) {
  const existing = pending.get(sessionId);
  if (existing) return existing;
  const result = Promise.resolve().then(reconnect).finally(() => {
    if (pending.get(sessionId) === result) pending.delete(sessionId);
  });
  pending.set(sessionId, result);
  return result;
}

// Cancelling one waiter releases its transfer slot, without aborting shared recovery.
export async function waitForSftpReady(task, operation) {
  let stop;
  const cancelled = new Promise(resolve => {
    stop = watch(() => task.status, status => {
      if (status === 'cancelled' || status === 'cancelling') resolve(false);
    }, { immediate: true, flush: 'sync' });
  });
  try { return await Promise.race([operation, cancelled]); }
  finally { stop?.(); }
}
