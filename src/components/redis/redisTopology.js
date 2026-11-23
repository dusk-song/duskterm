export function formatRedisEndpoint(endpoint) {
  if (!endpoint?.host || !/^[1-9]\d{0,4}$/.test(endpoint.port) || Number(endpoint.port) > 65535) return '';
  const { host, port } = endpoint;
  return (host.includes(':') && !host.startsWith('[') ? '[' + host + ']' : host) + ':' + port;
}

function parseFields(lines, delimiter) {
  const fields = new Map();
  for (const line of lines) {
    const index = line.indexOf(delimiter);
    if (index > 0) fields.set(line.slice(0, index), line.slice(index + 1));
  }
  return fields;
}

const replicationStatuses = {
  online: 'online',
  wait_bgsave: 'syncing',
  send_bulk: 'syncing',
  offline: 'disconnected',
};
const replicationRoles = new Map([['master', 'primary'], ['slave', 'replica'], ['replica', 'replica']]);
const addressOrder = new Intl.Collator(undefined, { numeric: true });

function unsignedField(fields, name) {
  const value = fields.get(name);
  return /^\d+$/.test(value ?? '') ? value : undefined;
}

function slotCount(ranges) {
  const slots = new Set();
  for (const range of ranges) {
    const [start, end = start] = range.split('-').map(Number);
    if (start > end || end > 16383) continue;
    for (let slot = start; slot <= end; slot += 1) slots.add(slot);
  }
  return slots.size;
}

export function replicationTopology(raw, configuredEndpoint) {
  if (!raw) return [];
  const fields = parseFields(raw.split(/\r?\n/), ':');
  const address = fields.get('duskterm_primary') || formatRedisEndpoint(configuredEndpoint);
  const role = replicationRoles.get(fields.get('role')) ?? 'unknown';
  const upstream = role === 'replica'
    ? formatRedisEndpoint({ host: fields.get('master_host'), port: fields.get('master_port') }) : '';
  const link = fields.get('master_link_status');
  let status = 'connected';
  if (role === 'replica') {
    if (fields.get('master_sync_in_progress') === '1') status = 'syncing';
    else if (link === 'up') status = 'replicating';
    else if (link === 'down') status = 'replication-down';
    else status = 'unknown';
  }
  const current = {
    key: 'data:' + address,
    address,
    role,
    source: '当前连接',
    status,
    upstreamKey: upstream && upstream !== address ? 'upstream:' + upstream : undefined,
    relation: upstream ? '跟随 ' + upstream : '',
    details: fields.has('connected_slaves') ? ['直连副本 ' + fields.get('connected_slaves')] : [],
    attributes: [
      ...(upstream ? [{ label: '上游地址', value: upstream }] : []),
      ...(unsignedField(fields, 'connected_slaves') !== undefined ? [{ label: '直连副本', value: fields.get('connected_slaves') }] : []),
      ...(unsignedField(fields, role === 'replica' ? 'slave_repl_offset' : 'master_repl_offset') !== undefined
        ? [{ label: '复制偏移', value: fields.get(role === 'replica' ? 'slave_repl_offset' : 'master_repl_offset') }] : []),
    ],
  };
  const nodes = new Map();
  if (upstream && upstream !== address) {
    nodes.set(upstream, {
      key: 'upstream:' + upstream, address: upstream, role: 'primary',
      source: '由当前副本报告', status: 'not-probed', relation: '', details: [],
    });
  }
  nodes.set(address, current);
  for (const [name, value] of fields) {
    if (!/^slave\d+$/.test(name)) continue;
    const replica = parseFields(value.split(','), '=');
    const endpoint = formatRedisEndpoint({ host: replica.get('ip'), port: replica.get('port') });
    if (!endpoint || nodes.has(endpoint)) continue;
    const details = [];
    if (/^\d+$/.test(replica.get('lag'))) details.push('延迟 ' + replica.get('lag') + ' 秒');
    if (/^\d+$/.test(replica.get('offset'))) details.push('复制偏移 ' + replica.get('offset'));
    nodes.set(endpoint, {
      key: 'replica:' + endpoint, address: endpoint, role: 'replica',
      status: Object.hasOwn(replicationStatuses, replica.get('state')) ? replicationStatuses[replica.get('state')] : 'unknown',
      upstreamKey: current.key,
      relation: address ? '跟随 ' + address : '', details,
      attributes: [
        ...(address ? [{ label: '上游地址', value: address }] : []),
        ...(unsignedField(replica, 'lag') !== undefined ? [{ label: '报告延迟', value: replica.get('lag') + ' 秒' }] : []),
        ...(unsignedField(replica, 'offset') !== undefined ? [{ label: '复制偏移', value: replica.get('offset') }] : []),
      ],
    });
  }
  return [...nodes.values()];
}

export function sentinelTopology(observed, seeds, service) {
  const rows = new Map();
  for (const { endpoint, status } of observed ?? []) {
    const address = formatRedisEndpoint(endpoint);
    if (!address) continue;
    rows.set(address, { address, status, source: status === 'responding' ? '查询来源' : '' });
  }
  for (const seed of seeds) {
    const address = formatRedisEndpoint(seed);
    if (address && !rows.has(address)) rows.set(address, { address, status: 'configured', source: '' });
  }
  return [...rows.values()].map(row => ({
    ...row, key: 'sentinel:' + row.address, role: 'sentinel', monitorService: service || '', relation: service ? '监控 ' + service : '', details: [],
    attributes: service ? [{ label: '监控服务', value: service }] : [],
  }));
}

export function clusterTopology(nodes) {
  const byId = new Map(nodes.map(node => [node.id, node]));
  const replicaCounts = new Map();
  for (const node of nodes) {
    if (node.role === 'replica' && byId.has(node.replicaOf)) replicaCounts.set(node.replicaOf, (replicaCounts.get(node.replicaOf) ?? 0) + 1);
  }
  const reference = id => byId.get(id)?.address || id;
  const group = node => node.role === 'primary' ? reference(node.id) : reference(node.replicaOf || node.id);
  const ordered = [...nodes].sort((left, right) => addressOrder.compare(group(left), group(right))
    || Number(right.role === 'primary') - Number(left.role === 'primary')
    || addressOrder.compare(left.address || left.id, right.address || right.id));
  return ordered.map(node => {
    const parentId = node.role === 'replica' && node.replicaOf !== '-' ? node.replicaOf : '';
    const slots = [];
    const moves = [];
    for (const range of (node.slots || '').split(/\s+/)) {
      if (/^\d+(?:-\d+)?$/.test(range)) slots.push(range);
      const move = /^\[(\d+)-(>|<)-([^\]]+)\]$/.exec(range);
      if (move) {
        const [, slot, direction, other] = move;
        moves.push(direction === '>' ? '迁出槽 ' + slot + ' 至 ' + reference(other)
          : '导入槽 ' + slot + '，来源 ' + reference(other));
      }
    }
    return {
      key: node.id, id: node.id, address: node.address, role: node.role, status: node.status,
      upstreamKey: parentId && byId.has(parentId) ? parentId : undefined,
      source: node.observer ? '查询来源' : '',
      relation: parentId ? '跟随 ' + reference(parentId) : node.role === 'replica' ? '上游未知' : '',
      details: [...(slots.length ? ['槽位 ' + slots.join('、')] : []), ...moves],
      attributes: [
        { label: '节点 ID', value: node.id },
        ...(node.role === 'primary' ? [
          { label: '槽位范围', value: slots.join('、') || '未分配' },
          { label: '槽位数量', value: String(slotCount(slots)) },
          { label: '副本数量', value: String(replicaCounts.get(node.id) ?? 0) },
        ] : []),
        ...(node.role === 'replica' ? [{ label: '上游地址', value: parentId ? reference(parentId) : '未知' }] : []),
        ...moves.map(value => ({ label: '槽位迁移', value })),
      ],
    };
  });
}
