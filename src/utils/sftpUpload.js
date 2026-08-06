export function normalizeLocalUploadPaths(sources) {
  const values = Array.isArray(sources) ? sources : [sources];
  const seen = new Set();
  const paths = [];

  for (const value of values) {
    const path = typeof value === 'string' ? value : value?.path;
    if (typeof path !== 'string' || !path.trim() || seen.has(path)) continue;
    seen.add(path);
    paths.push(path);
  }

  return paths;
}

export function classifyLocalUploadEntries(entries) {
  const files = [];
  let directoryCount = 0;
  let unsupportedCount = 0;

  for (const entry of Array.isArray(entries) ? entries : []) {
    if (entry?.isFile && entry.path) {
      files.push(entry);
    } else if (entry?.isDir) {
      directoryCount += 1;
    } else {
      unsupportedCount += 1;
    }
  }

  return { files, directoryCount, unsupportedCount };
}
