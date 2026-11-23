<script setup>
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { HelpCircle } from '@lucide/vue';
import { useRedisStore, redisModeLabels } from '@/stores/redis';
import { invokeCommand } from '@/utils/ipc';
import { parseRedisInfo } from './redisInfo';
import { formatRedisEndpoint } from './redisTopology';
import { redisErrorMessage } from './redisRecovery';

const props = defineProps({
  session: { type: Object, required: true },
  database: { type: Number, required: true },
  databases: { type: Object, default: null },
  active: Boolean,
});
const redis = useRedisStore();
const report = shallowRef(null);
const pending = ref(false);
const error = ref('');
const updatedAt = ref(null);
let revision = 0;
let loadedGeneration = -1;
const mode = computed(() => props.session.config.redis?.mode ?? 'standalone');
const fields = computed(() => new Map(parseRedisInfo(report.value ?? '').flatMap(section => section.rows.map(row => [row.name, row.value]))));
const unavailable = computed(() => pending.value && report.value === null ? '读取中…' : '—');

function field(name) {
  return fields.value.get(name) || unavailable.value;
}

function bytes(name) {
  const value = fields.value.get(name);
  if (value == null || !/^\d+$/.test(value)) return unavailable.value;
  const size = Number(value);
  if (!Number.isFinite(size)) return unavailable.value;
  const index = Math.min(size > 0 ? Math.floor(Math.log(size) / Math.log(1024)) : 0, 5);
  return new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 1 }).format(size / 1024 ** index) + ' ' + ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'][index];
}

const metrics = computed(() => [
  { label: 'Redis 版本', value: field('redis_version') },
  { label: '已用内存', value: bytes('used_memory') },
  { label: '客户端连接', value: field('connected_clients') },
  { label: '每秒操作数', value: field('instantaneous_ops_per_sec') },
]);
const connection = computed(() => {
  const config = props.session.config;
  const options = config.redis ?? {};
  const count = props.databases?.keys?.[props.database] ?? (props.databases?.keyspaceAvailable ? '0' : null);
  const rows = [
    { label: mode.value === 'sentinel' ? '哨兵入口' : mode.value === 'cluster' ? '集群入口' : '连接地址', value: formatRedisEndpoint(config) || '—' },
    { label: '连接模式', value: redisModeLabels[mode.value] ?? mode.value },
    ...(mode.value === 'sentinel' && props.session.primary ? [{ label: '当前主节点', value: formatRedisEndpoint(props.session.primary) || '—' }] : []),
    { label: '当前数据库', value: 'DB ' + props.database },
    { label: mode.value === 'cluster' ? '集群键数' : '当前库键数', value: count ?? '—' },
    { label: 'ACL 用户', value: config.username || 'default' },
    { label: '传输方式', value: options.tls ? 'TLS 加密' : 'TCP' },
    { label: '键分隔符', value: options.keySeparator === '' ? '不分组' : options.keySeparator ?? ':' },
  ];
  if (mode.value === 'sentinel') rows.push({ label: '监控服务', value: options.sentinelService || '—' });
  return rows;
});
const runtime = computed(() => {
  const uptime = fields.value.get('uptime_in_seconds');
  const seconds = /^\d+$/.test(uptime ?? '') && Number.isSafeInteger(Number(uptime)) ? Number(uptime) : null;
  const duration = seconds === null ? unavailable.value : seconds < 60 ? seconds + ' 秒'
    : [seconds >= 86400 ? Math.floor(seconds / 86400) + ' 天' : '', seconds >= 3600 ? Math.floor(seconds % 86400 / 3600) + ' 小时' : '', Math.floor(seconds % 3600 / 60) + ' 分钟'].filter(Boolean).join(' ');
  const role = fields.value.get('role');
  return [
    { label: '运行时长', value: duration },
    { label: mode.value === 'cluster' ? '采样节点角色' : '节点角色', value: ({ master: '主节点', slave: '副本', replica: '副本', sentinel: '哨兵' })[role] ?? role ?? unavailable.value },
    { label: '内存峰值', value: bytes('used_memory_peak') },
    { label: '内存上限', value: fields.value.get('maxmemory') === '0' ? '未设置' : bytes('maxmemory') },
    { label: '阻塞客户端', value: field('blocked_clients') },
    { label: '累计执行命令', value: field('total_commands_processed') },
  ];
});

async function refresh() {
  if (!props.active || props.session.status !== 'connected') return;
  const current = ++revision;
  const generation = props.session.generation;
  pending.value = true;
  error.value = '';
  try {
    const raw = await invokeCommand('redis_info', { id: props.session.id, db: props.database, section: '' });
    if (current !== revision || generation !== props.session.generation) return;
    report.value = raw;
    loadedGeneration = generation;
    updatedAt.value = new Date().toLocaleTimeString('zh-CN', { hour12: false });
  } catch (cause) {
    if (current !== revision || generation !== props.session.generation) return;
    error.value = redisErrorMessage(cause);
    redis.reportFailure(props.session.id, generation, cause);
  } finally {
    if (current === revision) pending.value = false;
  }
}

watch([() => props.active, () => props.session.status, () => props.session.generation], ([active, status, generation]) => {
  revision += 1;
  pending.value = false;
  if (generation !== loadedGeneration) {
    report.value = null;
    updatedAt.value = null;
    error.value = '';
  }
  if (active && status === 'connected' && (loadedGeneration !== generation || report.value === null)) void refresh();
}, { immediate: true });
onBeforeUnmount(() => { revision += 1; });
defineExpose({ refresh, pending });
</script>

<template>
  <section class="redis-overview mx-auto w-full max-w-6xl px-4 py-5 text-xs" aria-label="Redis 连接概览">
    <header class="mb-8 flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-base font-semibold tracking-wide">连接概览</h2>
        <p class="mt-2 flex items-start gap-2 leading-relaxed text-muted-foreground">
          <HelpCircle class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>从左侧选择一个键，查看或编辑其中的数据。</span>
        </p>
      </div>
      <p v-if="updatedAt" class="pt-1 text-muted-foreground">{{ session.status !== 'connected' ? '上次查询' : pending ? '更新中…' : '查询于' }} {{ updatedAt }}</p>
    </header>

    <div class="redis-overview-metrics mb-9 grid gap-x-8 gap-y-6 border-b border-border pb-7" aria-label="运行指标">
      <div v-for="item in metrics" :key="item.label" class="min-w-0">
        <p class="mb-3 text-muted-foreground">{{ item.label }}</p>
        <p class="break-words font-mono text-sm font-medium leading-6">{{ item.value }}</p>
      </div>
    </div>

    <div class="redis-overview-groups grid gap-x-12 gap-y-9">
      <section v-for="group in [{ title: '连接信息', rows: connection }, { title: '运行情况', rows: runtime }]" :key="group.title" class="min-w-0">
        <h3 class="mb-5 font-semibold">{{ group.title }}</h3>
        <dl class="grid gap-4">
          <div v-for="item in group.rows" :key="item.label" class="grid grid-cols-[6rem_minmax(0,1fr)] items-baseline gap-4 leading-6">
            <dt class="text-muted-foreground">{{ item.label }}</dt>
            <dd class="min-w-0 break-all font-mono">{{ item.value }}</dd>
          </div>
        </dl>
      </section>
    </div>

    <p v-if="mode === 'cluster'" class="mt-8 flex items-start gap-2 leading-relaxed text-muted-foreground">
      <HelpCircle class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      <span>版本、内存及运行指标来自槽位 0 对应的主节点，键数为集群统计。</span>
    </p>
    <p v-if="error" role="status" class="mt-5 flex items-start gap-2 break-words leading-relaxed text-muted-foreground">
      <HelpCircle class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      <span class="min-w-0">运行信息暂时无法读取：{{ error }}<template v-if="report !== null">；当前保留上次查询结果。</template></span>
    </p>
  </section>
</template>

<style scoped>
.redis-overview { container-type: inline-size; }
.redis-overview-metrics { grid-template-columns: repeat(2, minmax(0, 1fr)); }
.redis-overview-groups { grid-template-columns: minmax(0, 1fr); }
@container (min-width: 620px) {
  .redis-overview-metrics { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .redis-overview-groups { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
