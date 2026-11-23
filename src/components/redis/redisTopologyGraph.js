import { graphlib, layout } from '@dagrejs/dagre';

export const topologyRoles = {
  primary: { label: '主', name: '主节点', color: 'var(--app-toast-info)' },
  replica: { label: '副', name: '副本', color: 'var(--app-toast-success)' },
  sentinel: { label: '哨', name: '哨兵', color: '#a78bda' },
  unknown: { label: '?', name: '角色未知', color: 'hsl(var(--muted-foreground))' },
};

export function redisTopologyGraph(rows, positions = null) {
  const byKey = new Map(rows.map(row => [row.key, row]));
  const primaries = rows.filter(row => row.role === 'primary');
  const edges = [];
  for (const row of rows) {
    if (row.upstreamKey && row.upstreamKey !== row.key && byKey.has(row.upstreamKey)) {
      edges.push({ id: 'replication:' + row.key, source: row.upstreamKey, target: row.key, type: 'smoothstep',
        label: '复制', class: 'redis-replication-edge nopan', reconnectable: false, selectable: false, deletable: false,
        data: { kind: 'replication', tone: row.statusTone } });
    }
    // Only discovered sentinels have an observed monitoring relationship. Configured seeds stay separate.
    if (row.role === 'sentinel' && row.monitorService && row.status !== 'configured' && primaries.length === 1) {
      edges.push({ id: 'monitor:' + row.key, source: row.key, target: primaries[0].key, type: 'smoothstep',
        label: '监控', class: 'redis-monitor-edge nopan', reconnectable: false, selectable: false, deletable: false,
        data: { kind: 'monitor', tone: 'neutral' } });
    }
  }
  const nodes = rows.map(row => {
    const width = 264;
    const attributes = row.attributes ?? [
      ...(row.relation ? [{ label: '关系', value: row.relation }] : []),
      ...(row.details ?? []).map(value => ({ label: '信息', value })),
    ];
    const height = 90 + 20 * attributes.length;
    return { id: row.key, type: 'redis', data: { ...row, attributes }, position: positions?.get(row.key), width, height,
      style: { width: width + 'px', height: height + 'px' },
      ariaLabel: (topologyRoles[row.role] ?? topologyRoles.unknown).name + ' ' + (row.address || row.id || '地址未知'),
      deletable: false, connectable: false, draggable: false, selectable: false };
  });
  if (nodes.every(node => node.position)) return { nodes, edges };
  const graph = new graphlib.Graph();
  graph.setGraph({ rankdir: 'TB', nodesep: 44, ranksep: 84, marginx: 24, marginy: 24 });
  graph.setDefaultEdgeLabel(() => ({}));
  for (const node of nodes) graph.setNode(node.id, { width: node.width, height: node.height });
  for (const edge of edges) graph.setEdge(edge.source, edge.target);
  if (nodes.length) layout(graph);
  for (const node of nodes) {
    const point = graph.node(node.id);
    node.position = { x: point.x - node.width / 2, y: point.y - node.height / 2 };
  }
  return { nodes, edges };
}
