import { readText } from './redisBytes.js';
const labels = new Intl.Collator();

// Keep full keys and byte identities on leaves; only the presentation is grouped.
export function buildRedisKeyTree(keys, separator = ':') {
  const root = { children: new Map(), leaves: [] };
  for (const row of keys) {
    const text = row.text === undefined ? readText(row.key) : row.text;
    const parts = separator && text !== null ? text.split(separator) : [row.label];
    // Bound grouping for pathological keys while preserving the complete suffix.
    if (parts.length > 32) parts.splice(31, parts.length - 31, parts.slice(31).join(separator));
    let node = root;
    const path = [];
    for (const part of parts.slice(0, -1)) {
      path.push(part);
      if (!node.children.has(part)) node.children.set(part, {
        id: 'group:' + JSON.stringify(path), label: part || '（空）', children: new Map(), leaves: [],
      });
      node = node.children.get(part);
    }
    node.leaves.push({ ...row, id: 'key:' + row.key.base64, label: parts.at(-1) || '（空）', fullLabel: row.label });
  }
  function sort(node) {
    node.groups = [...node.children.values()].sort((a, b) => labels.compare(a.label, b.label));
    node.children.clear();
    node.leaves.sort((a, b) => labels.compare(a.label, b.label));
    for (const group of node.groups) sort(group);
  }
  sort(root);
  return root;
}

export function flattenRedisKeyTree(root, expandedGroups = new Set()) {
  const rows = [];
  function visit(node, depth) {
    for (const group of node.groups) {
      const expanded = expandedGroups.has(group.id);
      rows.push({ id: group.id, label: group.label, group: true, expanded, depth });
      if (expanded) visit(group, depth + 1);
    }
    for (const row of node.leaves) rows.push({ ...row, depth });
  }
  visit(root, 0);
  return rows;
}
