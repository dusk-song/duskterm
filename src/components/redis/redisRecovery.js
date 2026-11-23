export function redisErrorMessage(cause) {
  return cause instanceof Error ? cause.message : String(cause ?? 'Redis 请求失败');
}

export function isRedisConnectionError(cause, includeTimeout = false) {
  const message = redisErrorMessage(cause);
  return /^Redis (网络连接失败|会话已断开)/.test(message)
    || (includeTimeout && message === 'Redis 请求超时');
}

export function redisReconnectDelay(session, attempts) {
  if (!session.generation || !isRedisConnectionError(session.error, true)) return null;
  return Math.min(30000, 2000 * 2 ** attempts);
}

export function sameRedisPrimary(left, right) {
  return left?.host === right?.host && left?.port === right?.port;
}

export function redisAffectedKeys(change) {
  return change.renameTo != null && change.renameTo !== change.key
    ? [change.key, change.renameTo]
    : [change.key];
}
