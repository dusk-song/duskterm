export const ACTIVE_TRANSFER_STATUSES = new Set([
  'waiting',
  'negotiating',
  'uploading',
  'transferring',
  'finalizing',
  'cancelling',
  'paused',
]);

export const COMPACT_TRANSFER_STATUSES = new Set(['success', 'cancelled', 'skipped']);
const CLEARABLE_TRANSFER_STATUSES = new Set(['success', 'cancelled', 'skipped']);

export function isActiveTransfer(item) {
  return ACTIVE_TRANSFER_STATUSES.has(item?.status)
    || (item?.protocol === 'zmodem' && !item?.terminalRestored);
}

export function isClearableTransfer(item) {
  return CLEARABLE_TRANSFER_STATUSES.has(item?.status)
    && (item?.protocol !== 'zmodem' || item?.terminalRestored);
}

export function filterTransferItems(items, filter) {
  const list = Array.isArray(items) ? items : [];
  if (filter === 'active') return list.filter(isActiveTransfer);
  if (filter === 'failed') return list.filter((item) => item?.status === 'failed');
  return list;
}

export function remoteParentPath(path) {
  const normalized = String(path || '').replace(/\\/g, '/').replace(/\/{2,}/g, '/');
  if (!normalized) return '';
  const end = normalized.endsWith('/') && normalized.length > 1
    ? normalized.slice(0, -1)
    : normalized;
  const separator = end.lastIndexOf('/');
  if (separator < 0) return '.';
  if (separator === 0) return '/';
  return end.slice(0, separator);
}

export function quotePosixShell(value) {
  return `'${String(value ?? '').replace(/'/g, `'"'"'`)}'`;
}

export function resolveTransferLocateTarget(item) {
  if (item?.status !== 'success') return null;
  if (item.protocol === 'zmodem' && !item.terminalRestored) return null;

  if (item.direction === 'download') {
    const path = String(item.localPath || '').trim();
    return path ? { kind: 'local', path } : null;
  }

  if (item.protocol !== 'sftp') return null;
  const directory = remoteParentPath(item.remotePath);
  return directory ? { kind: 'remote', directory } : null;
}

export function buildRemoteDirectoryCommand(directory) {
  return `cd ${quotePosixShell(directory)} && pwd\r`;
}
