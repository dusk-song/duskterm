/**
 * @typedef {Object} PermissionInfo
 * @property {number} mode
 * @property {string} display
 */

/**
 * @typedef {Object} FileItem
 * @property {string} name
 * @property {boolean} is_dir
 * @property {number} size
 * @property {number} modified
 * @property {number} permissions
 * @property {string=} owner
 * @property {string=} group
 */

/**
 * @typedef {Object} BatchActionResult
 * @property {'download'|'upload'|'delete'|'rename'|'chmod'} action
 * @property {number} total
 * @property {number} success
 * @property {number} failed
 * @property {Array<{name: string, error: string}>} errors
 */

export function formatSize(bytes) {
  if (!bytes || bytes <= 0) return '-';
  const units = ['B', 'KB', 'MB', 'GB'];
  let index = 0;
  let value = bytes;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  const precision = index === 0 ? 0 : value >= 100 ? 1 : 2;
  return `${value.toFixed(precision)} ${units[index]}`;
}

export function formatLocalTime(unixSeconds) {
  if (!unixSeconds) return '-';
  return new Date(unixSeconds * 1000).toLocaleString();
}

export function formatPermissions(mode) {
  if (!mode) return '----------';
  let type = '-';
  if ((mode & 0o040000) === 0o040000) type = 'd';
  if ((mode & 0o120000) === 0o120000) type = 'l';
  const p = (mask, char) => (mode & mask ? char : '-');
  return `${type}${p(0o400, 'r')}${p(0o200, 'w')}${p(0o100, 'x')}${p(0o040, 'r')}${p(0o020, 'w')}${p(0o010, 'x')}${p(0o004, 'r')}${p(0o002, 'w')}${p(0o001, 'x')}`;
}

export function joinRemotePath(basePath, name) {
  if (!basePath || basePath === '/') return `/${name}`;
  return basePath.endsWith('/') ? `${basePath}${name}` : `${basePath}/${name}`;
}
