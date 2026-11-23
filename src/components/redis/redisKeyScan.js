// SCAN can return an empty page before its cursor reaches zero, and may repeat keys.
export async function scanRedisKeyPages({
  start = '0', initial = [], limit = 500, readPage, formatRow = row => row,
  isCurrent = () => true, shouldPause = () => false, onUpdate = () => {},
}) {
  const signature = row => row.key.length + ':' + row.key.base64;
  const unique = new Map(initial.map(row => [signature(row), row]));
  let cursor = start;
  let publishedAt = performance.now();
  let publishedSize = initial.length;

  function publish() {
    onUpdate({ keys: [...unique.values()], cursor, complete: cursor === '0' });
    publishedAt = performance.now();
    publishedSize = unique.size;
  }

  do {
    const page = await readPage(cursor);
    if (!isCurrent()) return;
    for (const row of page.keys) {
      const id = signature(row);
      if (unique.get(id)?.kind !== row.kind) unique.set(id, formatRow(row));
    }
    cursor = page.nextCursor;
    const finished = cursor === '0' || unique.size >= limit || shouldPause();
    const elapsed = performance.now() - publishedAt;
    const added = unique.size - publishedSize;
    const publishProgress = added > 0 && (publishedSize === 0
      || (elapsed >= 150 && added >= Math.max(256, publishedSize / 4)));
    if (finished || publishProgress) publish();
    if (finished) return;
    // Give input/scroll/stop events time to run between IPC batches.
    await new Promise(resolve => setTimeout(resolve, 0));
    if (!isCurrent()) return;
    if (shouldPause()) {
      if (!publishProgress) publish();
      return;
    }
  } while (true);
}
