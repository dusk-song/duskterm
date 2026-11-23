export function parseRedisInfo(raw = '') {
  const sections = [];
  let current;
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith('# ')) {
      const name = line.slice(2).trim();
      if (name.startsWith('DuskTerm:')) continue;
      current = { name, rows: [] };
      sections.push(current);
      continue;
    }
    const separator = line.indexOf(':');
    if (separator <= 0) continue;
    if (!current) {
      current = { name: '其他', rows: [] };
      sections.push(current);
    }
    current.rows.push({ name: line.slice(0, separator), value: line.slice(separator + 1) });
  }
  return sections.filter(section => section.rows.length);
}

export function redisDatabaseOptions(report, currentDb) {
  const ids = new Set([currentDb, ...Object.keys(report?.keys ?? {}).map(Number)]);
  if (report?.total != null) {
    for (let id = 0; id < report.total; id++) ids.add(id);
  }
  return [...ids].sort((a, b) => a - b).map(id => {
    const count = report?.keys?.[id] ?? (report?.keyspaceAvailable ? '0' : null);
    return { value: id, label: `DB ${id} (${count ?? '—'})` };
  });
}
