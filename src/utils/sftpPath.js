export function normalizeAbsoluteSftpPath(path, fallback = '/') {
  const normalized = String(path || '').trim();
  return normalized.startsWith('/') ? normalized : fallback;
}

export function resolveSftpPath(path, defaultDirectory = '') {
  const normalized = String(path || '').trim();
  if (normalized.startsWith('/')) return normalized;

  const base = normalizeAbsoluteSftpPath(defaultDirectory, '');
  if (!base) return '';
  if (normalized === '~') return base;
  if (!normalized.startsWith('~/')) return '';

  const suffix = normalized.slice(2);
  if (!suffix) return base;
  return base === '/' ? `/${suffix}` : `${base.replace(/\/+$/, '')}/${suffix}`;
}
