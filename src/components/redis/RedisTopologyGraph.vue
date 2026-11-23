<script setup>
import { computed, nextTick, shallowRef, watch } from 'vue';
import { Background, Handle, MarkerType, Position, VueFlow } from '@xyflow/vue';
import { Info, Maximize, Minus, Plus } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import Badge from '@/components/ui/badge/Badge.vue';
import { TooltipHint } from '@/components/ui/tooltip';
import { redisTopologyGraph, topologyRoles } from './redisTopologyGraph';
import '@xyflow/vue/style.css';

const props = defineProps({ rows: { type: Array, required: true }, active: Boolean });
const nodes = shallowRef([]);
const edges = shallowRef([]);
const viewer = shallowRef(null);
const roleCounts = computed(() => Object.entries(topologyRoles).map(([key, value]) => ({
  ...value, count: props.rows.filter(row => (topologyRoles[row.role] ? row.role : 'unknown') === key).length,
})).filter(role => role.count));
const fitOptions = { padding: 0.18, minZoom: 0.25, maxZoom: 1, duration: 180 };
let structure = '';
let needsFit = true;

function role(row) {
  return topologyRoles[row.role] ?? topologyRoles.unknown;
}

function fitView() {
  if (props.active && viewer.value) {
    needsFit = false;
    void viewer.value.fitView(fitOptions);
  }
}

function fitIfNeeded() {
  if (needsFit) fitView();
}

function updateGraph(keepLayout) {
  const positions = keepLayout ? new Map(nodes.value.map(node => [node.id, node.position])) : null;
  const graph = redisTopologyGraph(props.rows, positions);
  nodes.value = graph.nodes;
  edges.value = graph.edges.map(edge => {
    const color = edge.data.kind === 'monitor' ? topologyRoles.sentinel.color
      : edge.data.tone === 'error' ? 'hsl(var(--destructive))'
      : edge.data.tone === 'warning' ? 'var(--app-risk-warning)'
      : edge.data.tone === 'ok' ? 'var(--app-toast-success)' : 'hsl(var(--muted-foreground))';
    return { ...edge, markerEnd: { type: MarkerType.ArrowClosed, color },
      style: { stroke: color, strokeWidth: 1.5, ...(edge.data.kind === 'monitor' ? { strokeDasharray: '5 5' } : {}) } };
  });
}

watch(() => props.rows, rows => {
  const nextStructure = JSON.stringify(rows.map(row => [row.key, row.role, row.upstreamKey, row.monitorService, row.status === 'configured', row.attributes?.length, row.details?.length, !!row.relation]));
  const changed = nextStructure !== structure;
  structure = nextStructure;
  updateGraph(!changed);
  // Refit only when the topology structure changes; status refreshes preserve the viewport.
  if (changed) {
    needsFit = true;
    void nextTick(fitIfNeeded);
  }
}, { immediate: true });

watch(() => props.active, active => { if (active) void nextTick(fitView); });
</script>

<template>
  <section class="redis-topology-graph flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-border" aria-label="Redis 拓扑图">
    <div class="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-3 py-2 text-xs">
      <div class="mr-auto flex flex-wrap items-center gap-2" aria-label="节点角色统计">
        <Badge v-for="item in roleCounts" :key="item.name" variant="outline" class="redis-role-badge rounded-md font-normal" :style="{ '--redis-role-color': item.color }">{{ item.name }} {{ item.count }}</Badge>
      </div>
      <span class="flex items-center gap-1.5 text-muted-foreground"><span class="w-5 border-t border-[var(--app-toast-success)]" aria-hidden="true" />复制</span>
      <span v-if="roleCounts.some(item => item.name === '哨兵')" class="flex items-center gap-1.5 text-muted-foreground"><span class="w-5 border-t border-dashed border-[#a78bda]" aria-hidden="true" />监控</span>
    </div>
    <div class="relative min-h-0 flex-1">
    <VueFlow v-model:nodes="nodes" v-model:edges="edges" :min-zoom="0.25" :max-zoom="1.5"
      :pro-options="{ hideAttribution: true }" :nodes-draggable="false" :nodes-connectable="false"
      :edges-reconnectable="false" :elements-selectable="false" :delete-key-code="null"
      :nodes-focusable="false" :edges-focusable="false" :zoom-on-double-click="false" :fit-view-options="fitOptions" fit-view
      @init="viewer = $event; fitIfNeeded()" @nodes-initialized="fitIfNeeded">
      <Background :gap="24" :size="1" />
      <template #node-redis="{ data }">
        <article class="redis-topology-node nopan h-full w-full rounded-lg border border-border bg-card p-3 text-foreground shadow-sm" :style="{ '--redis-role-color': role(data).color }">
          <Handle type="target" :position="Position.Top" :is-connectable="false" :is-connectable-start="false" :is-connectable-end="false" />
          <div class="mb-2 flex h-6 min-w-0 items-center gap-2">
            <TooltipHint :text="role(data).name">
              <Badge variant="outline" class="redis-role-badge shrink-0 rounded-md px-1.5 font-normal">{{ role(data).label }}</Badge>
            </TooltipHint>
            <TooltipHint :text="[data.address || '地址未知', data.id].filter(Boolean).join(' · ')">
              <code class="min-w-0 flex-1 truncate text-xs font-semibold">{{ data.address || data.id || '地址未知' }}</code>
            </TooltipHint>
            <TooltipHint v-if="data.source" :text="data.source">
              <Info class="size-3.5 shrink-0 text-muted-foreground" :aria-label="data.source" />
            </TooltipHint>
          </div>
          <dl class="text-xs leading-5">
            <div class="flex h-5 min-w-0 items-center gap-3">
              <dt class="shrink-0 text-muted-foreground">{{ data.statusTitle || '状态' }}</dt>
              <dd class="ml-auto flex min-w-0 items-center gap-1.5" :class="data.statusClass"><span class="size-1.5 shrink-0 rounded-full bg-current" aria-hidden="true" />{{ data.statusLabel }}</dd>
            </div>
            <div v-for="(item, index) in data.attributes" :key="index" class="flex h-5 min-w-0 items-center gap-3">
              <dt class="shrink-0 text-muted-foreground">{{ item.label }}</dt>
              <TooltipHint :text="item.value"><dd class="ml-auto min-w-0 truncate font-mono text-right">{{ item.value }}</dd></TooltipHint>
            </div>
          </dl>
          <Handle type="source" :position="Position.Bottom" :is-connectable="false" :is-connectable-start="false" :is-connectable-end="false" />
        </article>
      </template>
    </VueFlow>
    <div class="absolute bottom-3 right-3 z-10 flex items-center gap-1 rounded-lg border border-border bg-card p-1 shadow-sm" role="group" aria-label="拓扑视图操作">
      <TooltipHint text="放大"><Button variant="ghost" size="icon-sm" aria-label="放大" :disabled="!viewer" @click="viewer?.zoomIn({ duration: 150 })"><Plus /></Button></TooltipHint>
      <TooltipHint text="缩小"><Button variant="ghost" size="icon-sm" aria-label="缩小" :disabled="!viewer" @click="viewer?.zoomOut({ duration: 150 })"><Minus /></Button></TooltipHint>
      <TooltipHint text="适应画布"><Button variant="ghost" size="icon-sm" aria-label="适应画布" :disabled="!viewer" @click="fitView"><Maximize /></Button></TooltipHint>
    </div>
    </div>
  </section>
</template>

<style scoped>
.redis-topology-graph {
  --xy-background-color: hsl(var(--background));
  --xy-background-pattern-dots-color: hsl(var(--border));
  --xy-edge-label-color: hsl(var(--muted-foreground));
  --xy-edge-label-background-color: hsl(var(--background));
}
.redis-topology-node { border-top: 2px solid var(--redis-role-color); }
.redis-role-badge { color: var(--redis-role-color); border-color: color-mix(in srgb, var(--redis-role-color) 35%, transparent); background: color-mix(in srgb, var(--redis-role-color) 12%, transparent); }
.redis-topology-graph :deep(.vue-flow__handle) { width: 6px; height: 6px; background: var(--redis-role-color); border: 1px solid hsl(var(--card)); pointer-events: none; }
.redis-topology-graph :deep(.vue-flow__edge) { cursor: default; }
.redis-topology-graph :deep(.vue-flow__edge-text) { font-size: 12px; }
</style>
