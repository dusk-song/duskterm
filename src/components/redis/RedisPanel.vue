<script setup>
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue';
import { ArrowLeft, Check, ChevronDown, ChevronRight, Copy, Download, File, Folder, HelpCircle, Pencil, Play, Plus, RefreshCw, Search, Square, TextCursorInput, Trash2, X } from '@lucide/vue';
import { AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import Badge from '@/components/ui/badge/Badge.vue';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { TooltipHint } from '@/components/ui/tooltip';
import SettingsSelect from '@/components/settings/SettingsSelect.vue';
import { useVirtualList } from '@/composables/useVirtualList';
import { toast } from '@/composables/useToast';
import { redisModeLabels, useRedisStore } from '@/stores/redis';
import { invokeCommand } from '@/utils/ipc';
import RedisStringEditor from './RedisStringEditor.vue';
import RedisValueView from './RedisValueView.vue';
import RedisConnectionOverview from './RedisConnectionOverview.vue';
import { byteLabel, commandArgs, complete, decodeBytes, encodeText, formatStringInput, readText, validateStringInput } from './redisBytes';
import { isRedisConnectionError, redisAffectedKeys, redisErrorMessage, redisReconnectDelay, sameRedisPrimary } from './redisRecovery';
import { clusterTopology, formatRedisEndpoint, replicationTopology, sentinelTopology } from './redisTopology';
import { parseRedisInfo, redisDatabaseOptions } from './redisInfo';
import { buildRedisKeyTree, flattenRedisKeyTree } from './redisKeyTree';
import { scanRedisKeyPages } from './redisKeyScan';

const RedisTopologyGraph = defineAsyncComponent(() => import('./RedisTopologyGraph.vue'));
const props = defineProps({ session: { type: Object, required: true }, active: Boolean });
const redis = useRedisStore();
const mode = computed(() => props.session.config.redis?.mode ?? 'standalone');
const cluster = computed(() => mode.value === 'cluster');
const sentinel = computed(() => mode.value === 'sentinel');
const kinds = ['String', 'Hash', 'List', 'Set', 'ZSet'].map(label => ({ value: label.toLowerCase(), label }));
const kindNames = new Map(kinds.map(({ value, label }) => [value, label]));
const filters = [{ value: '', label: '全部类型' }, ...kinds];
const pages = [
  { id: 'data', label: '数据' }, { id: 'console', label: '控制台' },
  { id: 'status', label: '状态' }, { id: 'topology', label: '拓扑' },
];
const topologyStatuses = {
  connected: ['已连接', 'ok'],
  online: ['在线', 'ok'],
  replicating: ['复制链路正常', 'ok'],
  responding: ['查询成功', 'ok'],
  known: ['已发现', 'neutral'],
  disconnected: ['已断开', 'error'],
  'replication-down': ['复制链路断开', 'error'],
  fail: ['故障', 'error'],
  syncing: ['同步中', 'warning'],
  suspected: ['疑似故障', 'warning'],
  handshake: ['握手中', 'warning'],
  'no-address': ['地址未知', 'warning'],
  configured: ['仅配置', 'neutral'],
  'not-probed': ['未直接探测', 'neutral'],
  unknown: ['未知', 'neutral'],
};
const topologyStatusClasses = {
  ok: 'text-[var(--app-toast-success)]',
  error: 'text-destructive',
  warning: 'text-[var(--app-risk-warning)]',
  neutral: 'text-muted-foreground',
};
const displayLimit = 2000;
const db = ref(cluster.value ? 0 : props.session.config.redis?.db ?? 0);
const dbInput = ref(String(db.value));
const page = ref('data');
const narrowView = ref('keys');
const patternInput = ref('');
const pattern = ref('');
const kind = ref('');
const keys = shallowRef([]);
const expandedGroups = ref(new Set());
const keyTree = computed(() => buildRedisKeyTree(keys.value, props.session.config.redis?.keySeparator ?? ':'));
const keyRows = computed(() => flattenRedisKeyTree(keyTree.value, expandedGroups.value));
const keySearch = ref(null);
const keyViewport = ref(null);
const consoleInput = ref(null);
let keyScrollFrame = 0;
const rowHeight = 32;
const { visibleItems, totalHeight, translateY, onScroll, setViewportHeight, resetScroll } =
  useVirtualList({ items: keyRows, rowHeight, overscan: 4 });
const selected = ref(null);
const detail = shallowRef(null);
const nextKeyCursor = ref('0');
const info = shallowRef({});
const infoCategory = ref('');
const infoSections = computed(() => parseRedisInfo(info.value.status));
const infoCategories = computed(() => [{ value: '', label: '全部类别' }, ...infoSections.value.map(section => ({ value: section.name, label: section.name }))]);
const visibleInfoSections = computed(() => infoSections.value.filter(section => !infoCategory.value || section.name === infoCategory.value));
const databases = shallowRef(null);
const databasesPending = ref(false);
const databasesError = ref('');
const databaseOptions = computed(() => redisDatabaseOptions(databases.value, db.value));
const scanPending = ref(false);
const scanStopRequested = ref(false);
const scanComplete = ref(false);
const detailPending = ref(false);
const infoPending = ref(false);
const topologyError = ref('');
const dbPending = ref(false);
const saving = ref(false);
const uncertainWrites = reactive(new Map());
const retryDelay = ref(null);
const draft = ref(null);
const task = ref(null);
const taskRef = ref(null);
const detailHeader = ref(null);
const connectionOverview = ref(null);
const stringEditor = ref(null);
const renameInput = ref(null);
const formats = ref({});
const consoles = ref({});
const consoleConfirmation = shallowRef(null);
const consoleWrites = new Set(['SET', 'SETNX', 'INCR', 'DECR', 'HSET', 'HDEL', 'SADD', 'SREM', 'ZADD', 'ZREM', 'EXPIRE', 'PERSIST', 'UNLINK']);
let guardResolve;
let scanRevision = 0;
let detailRevision = 0;
let infoRevision = 0;
let databasesRevision = 0;
let initializedGeneration = 0;
let needsScan = true;
let retryAttempts = 0;

const connected = computed(() => props.session.status === 'connected' && !props.session.closing);
const writeUncertain = computed(() => uncertainWrites.has(db.value));
const requestedDb = computed(() => readUnsigned(dbInput.value, 4294967295));
const primaryLabel = computed(() => formatRedisEndpoint(props.session.primary));
const verificationReady = computed(() => task.value?.type === 'verify'
  && task.value.rows !== null && !task.value.pending && connected.value
  && task.value.db === db.value && task.value.generation === props.session.generation);
const editable = computed(() => connected.value && kinds.some(option => option.value === detail.value?.kind)
  && detail.value?.ttlMs !== -2 && !saving.value && !dbPending.value && !writeUncertain.value);
const canEditString = computed(() => complete(detail.value?.value));
const editedString = computed(() => draft.value?.stringInput && validateStringInput(draft.value.stringInput));
const editedName = computed(() => {
  if (draft.value?.renameInput == null) return null;
  const name = draft.value.renameInput;
  if (!name.length) return { error: '请输入新的键名' };
  if (new TextEncoder().encode(name).length > 8192) return { error: '键名不能超过 8 KiB' };
  return { base64: encodeText(name) };
});
const editedTtl = computed(() => draft.value?.ttlInput != null ? parseTtlInput(draft.value.ttlInput) : null);
const draftError = computed(() => editedString.value?.error || editedName.value?.error || editedTtl.value?.error);
const newString = computed(() => task.value?.type === 'create' && task.value.kind === 'string'
  ? validateStringInput(task.value.stringInput) : null);
const newTtl = computed(() => task.value?.type === 'create' ? parseTtlInput(task.value.ttlInput) : null);
const taskSubmitDisabled = computed(() => saving.value || !connected.value || writeUncertain.value || !!newString.value?.error || !!newTtl.value?.error);
const dirty = computed(() => {
  const change = draft.value;
  if (!change) return false;
  if (change.ttlInput !== change.initialTtl) return true;
  if (change.renameInput != null && (editedName.value?.error || editedName.value?.base64 !== selected.value?.base64)) return true;
  if (change.renameTo || change.ttlMs != null) return true;
  if (change.action === 'string') return editedString.value.base64 !== change.expected;
  return !!change.action;
});
const selectionOutsideFilter = computed(() => selected.value && !keys.value.some(row => row.key.base64 === selected.value.base64) && (pattern.value || kind.value));
const ttlLabel = computed(() => {
  const ttl = detail.value?.ttlMs;
  if (ttl === -1) return '永久';
  return ttl >= 0 ? Math.ceil(ttl / 1000) + ' 秒' : '已过期';
});
const valueMode = computed({
  get: () => formats.value[db.value + ':' + selected.value?.base64] || 'auto',
  set: mode => { formats.value[db.value + ':' + selected.value?.base64] = mode; }
});
const currentConsole = computed(() => {
  const database = db.value;
  if (!consoles.value[database]) consoles.value[database] = { command: '', history: [], log: [], pending: false };
  return consoles.value[database];
});
const topologyRows = computed(() => {
  const { config } = props.session;
  const rows = cluster.value ? clusterTopology(info.value.topology ?? []) : replicationTopology(info.value.topology, config);
  if (sentinel.value) {
    rows.push(...sentinelTopology(info.value.sentinels?.nodes, [config, ...(config.redis.seeds ?? [])], config.redis.sentinelService));
  }
  return rows.map(row => {
    const [statusLabel, tone] = Object.hasOwn(topologyStatuses, row.status) ? topologyStatuses[row.status] : topologyStatuses.unknown;
    const statusTitle = cluster.value ? '集群链路' : row.role === 'sentinel' ? '哨兵状态'
      : row.role === 'replica' ? '复制状态' : row.status === 'not-probed' ? '探测状态' : '连接状态';
    return { ...row, statusLabel, statusTitle, statusTone: tone, statusClass: topologyStatusClasses[tone] };
  });
});
const keyLabel = computed(() => selected.value ? byteLabel(selected.value) : '');
const targetLabel = computed(() => props.session.name + ' · DB ' + db.value);

function readUnsigned(value, max) {
  const text = value.trim();
  const number = Number(text);
  return /^\d+$/.test(text) && Number.isSafeInteger(number) && number <= max ? number : null;
}

function parseTtlInput(value) {
  const text = String(value ?? '').trim();
  if (!text || text === '-1') return { ttlMs: 0 };
  const seconds = readUnsigned(text, 31536000000);
  return seconds === null || seconds === 0 ? { error: 'TTL 须为正整数，单位秒；-1 或留空为永久' } : { ttlMs: seconds * 1000 };
}

function readScore(value) {
  const text = value.trim();
  const number = Number(text);
  return /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(text) && Number.isFinite(number) ? number : null;
}

async function request(command, data = {}, targetDb = db.value) {
  const generation = props.session.generation;
  try {
    return await invokeCommand(command, { id: props.session.id, db: targetDb, ...data });
  } catch (cause) {
    if (targetDb === db.value) redis.reportFailure(props.session.id, generation, cause);
    throw cause;
  }
}

function reportRequestError(cause) {
  if (!connected.value && isRedisConnectionError(cause)) return;
  toast.error(redisErrorMessage(cause), 4000);
}

function entrySignature(row) {
  return row.index ?? (detail.value?.kind === 'hash' ? row.field.base64 : row.value.base64);
}

async function loadKeys(reset = true, all = false) {
  if (!connected.value || !props.active || page.value !== 'data' || (!reset && scanPending.value)) return;
  const revision = ++scanRevision;
  const database = db.value;
  const generation = props.session.generation;
  const filterPattern = pattern.value;
  const filterKind = kind.value;
  scanStopRequested.value = false;
  scanPending.value = true;
  if (reset) {
    needsScan = false;
    keys.value = [];
    scanComplete.value = false;
    nextKeyCursor.value = '0';
    resetScroll();
    if (keyViewport.value) keyViewport.value.scrollTop = 0;
  }
  try {
    await scanRedisKeyPages({
      start: nextKeyCursor.value, initial: keys.value, limit: all ? Infinity : 500,
      readPage: start => request('redis_scan', { start, pattern: filterPattern, kind: filterKind, bulk: all }, database),
      formatRow: row => {
        const text = readText(row.key);
        return { ...row, text, label: byteLabel(row.key, text) };
      },
      isCurrent: () => revision === scanRevision && generation === props.session.generation && database === db.value,
      shouldPause: () => scanStopRequested.value || !props.active || !connected.value || page.value !== 'data',
      onUpdate: result => {
        keys.value = result.keys;
        nextKeyCursor.value = result.cursor;
        scanComplete.value = result.complete;
      },
    });
  } catch (cause) {
    if (props.active && revision === scanRevision) reportRequestError(cause);
  } finally {
    if (revision === scanRevision) scanPending.value = false;
  }
}

function queueKeyScroll(event) {
  if (keyScrollFrame) return;
  const target = event.currentTarget;
  keyScrollFrame = requestAnimationFrame(() => {
    keyScrollFrame = 0;
    onScroll({ target });
  });
}

function searchKeys() {
  if (scanPending.value && pattern.value === patternInput.value) return;
  pattern.value = patternInput.value;
  return loadKeys();
}

async function loadDetail(reset = true) {
  if (!connected.value || !selected.value || (!reset && detailPending.value)) return;
  const revision = ++detailRevision;
  detailPending.value = true;
  if (reset) detail.value = null;
  try {
    const result = await request('redis_detail', { key: selected.value.base64, start: reset ? '0' : detail.value.nextCursor });
    if (revision !== detailRevision) return;
    if (result.kind === 'none') {
      detail.value = null;
      if (props.active) toast.info('键已过期或被删除，请刷新键列表');
      return;
    }
    if (!reset && detail.value?.kind === result.kind) {
      const unique = new Map(detail.value.entries.map(row => [entrySignature(row), row]));
      for (const row of result.entries) unique.set(entrySignature(row), row);
      result.entries = [...unique.values()];
    }
    detail.value = result;
  } catch (cause) {
    if (props.active && revision === detailRevision) reportRequestError(cause);
  } finally {
    if (revision === detailRevision) detailPending.value = false;
  }
}

async function loadInfo() {
  if (!connected.value || !props.active || !['status', 'topology'].includes(page.value)) return;
  const revision = ++infoRevision;
  const target = page.value;
  // Sentinel discovery uses separate credentials and cannot mark the data connection as failed.
  const sentinels = target === 'topology' && sentinel.value
    ? invokeCommand('redis_sentinel_nodes', { id: props.session.id }).then(nodes => ({ nodes }), cause => ({ error: redisErrorMessage(cause) }))
    : null;
  infoPending.value = true;
  try {
    const raw = cluster.value && target === 'topology'
      ? await request('redis_cluster_nodes')
      : await request('redis_info', { section: target === 'topology' ? 'replication' : '' });
    if (revision === infoRevision) {
      info.value = { ...info.value, [target]: raw };
      if (target === 'topology') {
        topologyError.value = '';
        info.value = { ...info.value, topologyUpdatedAt: Date.now() };
      }
    }
  } catch (cause) {
    if (revision === infoRevision) {
      if (target === 'topology') topologyError.value = redisErrorMessage(cause);
      if (props.active) reportRequestError(cause);
    }
  } finally {
    if (sentinels && revision === infoRevision) {
      const report = await sentinels;
      if (revision === infoRevision) {
        info.value = { ...info.value, sentinels: report.error ? { ...info.value.sentinels, error: report.error } : report };
      }
    }
    if (revision === infoRevision) infoPending.value = false;
  }
}

async function loadDatabases() {
  if (!connected.value || !props.active) return;
  const revision = ++databasesRevision;
  const generation = props.session.generation;
  databasesPending.value = true;
  try {
    const result = await request('redis_databases');
    if (revision !== databasesRevision || generation !== props.session.generation) return;
    databases.value = result;
    databasesError.value = '';
  } catch (cause) {
    if (revision === databasesRevision) databasesError.value = redisErrorMessage(cause);
  } finally {
    if (revision === databasesRevision) databasesPending.value = false;
  }
}

watch(infoCategories, options => {
  if (!options.some(option => option.value === infoCategory.value)) infoCategory.value = '';
});

async function copyInfo() {
  if (!info.value.status) return;
  try {
    await navigator.clipboard.writeText(info.value.status);
    toast.success('已复制 Redis INFO 输出');
  } catch {
    toast.error('无法访问剪贴板');
  }
}

watch([() => props.active, () => props.session.generation, page], ([active, generation, currentPage]) => {
  if (!active || !generation || !connected.value) return;
  if (initializedGeneration !== generation) {
    initializedGeneration = generation;
    detailRevision += 1;
    detailPending.value = false;
    infoRevision += 1;
    infoPending.value = false;
    info.value = {};
    databases.value = null;
    databasesError.value = '';
    void loadDatabases();
    topologyError.value = '';
    needsScan = true;
    void loadKeys();
    if (selected.value && !draft.value) void loadDetail();
  } else if (needsScan) {
    void loadKeys();
  }
  if (currentPage !== 'data') void loadInfo();
}, { immediate: true });
watch(kind, () => { needsScan = true; void loadKeys(); });
watch([() => props.active, page, db], async ([active, currentPage]) => {
  if (!active || currentPage !== 'console') return;
  await nextTick();
  if (props.active && page.value === 'console') consoleInput.value?.$el?.focus();
}, { flush: 'post' });
watch([() => props.active, connected, page, () => selected.value?.base64, db, () => props.session.generation],
  ([active, online, currentPage, key, database, generation], _, onCleanup) => {
    if (!active || !online || currentPage !== 'data' || key == null) return;
    let cancelled = false;
    let timer;
    onCleanup(() => { cancelled = true; clearTimeout(timer); });
    const poll = async () => {
      if (!saving.value && !dbPending.value && !detailPending.value && detail.value && !task.value) {
        const revision = detailRevision;
        try {
          const ttlMs = await request('redis_ttl', { key }, database);
          if (!cancelled && revision === detailRevision && generation === props.session.generation && detail.value
            && detail.value.ttlMs !== ttlMs) {
            detail.value = { ...detail.value, ttlMs };
          }
        } catch (cause) {
          if (!cancelled) redis.reportFailure(props.session.id, generation, cause);
        }
      }
      if (!cancelled) timer = setTimeout(poll, 5000);
    };
    timer = setTimeout(poll, 5000);
  }, { immediate: true });
watch([() => props.active, () => props.session.status, () => props.session.closing], ([active, status, closing], _, onCleanup) => {
  retryDelay.value = null;
  if (!active || closing) return;
  let timer;
  let cancelled = false;
  onCleanup(() => {
    cancelled = true;
    clearTimeout(timer);
  });

  if (status === 'disconnected') {
    retryDelay.value = redisReconnectDelay(props.session, retryAttempts);
    if (retryDelay.value === null) return;
    const retry = () => {
      if (saving.value || dbPending.value) {
        timer = setTimeout(retry, 1000);
        return;
      }
      retryAttempts += 1;
      void redis.connect(props.session.id, { automatic: true });
    };
    timer = setTimeout(retry, retryDelay.value);
    return;
  }

  if (status !== 'connected') return;
  retryAttempts = 0;
  const probe = async () => {
    if (!saving.value && !dbPending.value) {
      const id = props.session.id;
      const generation = props.session.generation;
      const targetDb = db.value;
      try {
        const health = await invokeCommand('redis_ping', { id, db: targetDb });
        if (!cancelled && targetDb === db.value) redis.reportHealth(id, generation, health);
      } catch (cause) {
        if (!cancelled && targetDb === db.value) redis.reportFailure(id, generation, cause, true);
      }
    }
    if (!cancelled && connected.value) timer = setTimeout(probe, 15000);
  };
  timer = setTimeout(probe, 15000);
}, { immediate: true });
watch(keyViewport, (element, _, onCleanup) => {
  if (!element) return;
  const observer = new ResizeObserver(() => setViewportHeight(element.clientHeight));
  observer.observe(element);
  onCleanup(() => observer.disconnect());
});

function showTask(state) {
  task.value = { db: db.value, generation: props.session.generation, ...state, returnPage: page.value, trigger: document.activeElement, keyLabel: keyLabel.value };
  page.value = 'data';
  focusTask();
}

function focusTask() {
  nextTick(() => (taskRef.value?.querySelector('form [data-initial-focus]')
    || taskRef.value?.querySelector('[data-initial-focus]'))?.focus());
}

function closeTask(restoreFocus = true) {
  const previous = task.value;
  task.value = previous.previous ?? null;
  page.value = previous.returnPage;
  if (!restoreFocus) return;
  if (task.value) focusTask();
  else nextTick(() => {
    if (!props.active) return;
    const preferred = previous.trigger?.disabled ? previous.trigger.closest('form')?.querySelector('input') : previous.trigger;
    if (preferred?.getClientRects().length && !preferred.disabled) preferred.focus();
    else focusBrowser();
  });
}

function focusBrowser() {
  if (!props.active || page.value !== 'data') return;
  const back = detailHeader.value?.querySelector('.redis-back');
  if (back?.getClientRects().length) back.focus();
  else (keyViewport.value?.querySelector('[aria-pressed="true"]') || keySearch.value?.querySelector('input'))?.focus();
}

function resolveGuard(value) {
  const resolve = guardResolve;
  guardResolve = undefined;
  closeTask(!value);
  resolve?.(value);
}

function canLeave() {
  if (saving.value || dbPending.value || guardResolve) return false;
  if (!dirty.value) {
    draft.value = null;
    task.value = null;
    return true;
  }
  redis.activate(props.session.id);
  showTask({ type: 'unsaved', title: '有未保存的修改', previous: task.value });
  return new Promise(resolve => { guardResolve = resolve; });
}

onMounted(() => redis.guardClose(props.session.id, canLeave));
onBeforeUnmount(() => {
  cancelAnimationFrame(keyScrollFrame);
  redis.guardClose(props.session.id, null);
  scanRevision += 1;
  detailRevision += 1;
  infoRevision += 1;
  databasesRevision += 1;
  guardResolve?.(false);
});

async function openKey(row) {
  if (task.value || !complete(row.key)) return;
  const changed = selected.value?.base64 !== row.key.base64;
  if (changed && !await canLeave()) return;
  narrowView.value = 'detail';
  if (changed) {
    selected.value = row.key;
    void loadDetail();
  }
  nextTick(focusBrowser);
}

function returnToKeys() {
  narrowView.value = 'keys';
  nextTick(focusBrowser);
}

async function closeKey() {
  if (task.value || !await canLeave()) return;
  detailRevision += 1;
  detailPending.value = false;
  selected.value = null;
  detail.value = null;
  narrowView.value = 'keys';
  nextTick(focusBrowser);
}

function toggleGroup(row) {
  const expanded = new Set(expandedGroups.value);
  if (expanded.has(row.id)) expanded.delete(row.id);
  else expanded.add(row.id);
  expandedGroups.value = expanded;
}

async function switchDb(value = dbInput.value) {
  if (cluster.value || !connected.value || saving.value || dbPending.value || task.value || consoleConfirmation.value) return;
  const target = readUnsigned(String(value), 4294967295);
  if (target === null) {
    toast.warning('请输入有效的 DB 编号');
    return;
  }
  dbInput.value = String(db.value);
  if (target === db.value || !await canLeave()) return;
  if (!connected.value || saving.value || dbPending.value) return;
  dbPending.value = true;
  const generation = props.session.generation;
  let health;
  try {
    health = await request('redis_ping', {}, target);
  } catch (cause) {
    reportRequestError(cause);
    return;
  } finally {
    dbPending.value = false;
  }
  if (!connected.value || generation !== props.session.generation) return;
  db.value = target;
  expandedGroups.value = new Set();
  dbInput.value = String(target);
  scanRevision += 1;
  scanPending.value = false;
  keys.value = [];
  nextKeyCursor.value = '0';
  needsScan = true;
  detailRevision += 1;
  infoRevision += 1;
  detailPending.value = false;
  selected.value = null;
  detail.value = null;
  narrowView.value = 'keys';
  info.value = {};
  topologyError.value = '';
  redis.reportHealth(props.session.id, generation, health);
  if (generation === props.session.generation) await Promise.all([loadKeys(), loadInfo(), loadDatabases()]);
}

async function refresh() {
  if (task.value || !await canLeave()) return;
  await loadKeys();
  if (selected.value) await loadDetail();
  await loadDatabases();
  if (!selected.value) await connectionOverview.value?.refresh();
}

function openVerification() {
  if (!connected.value || !writeUncertain.value || saving.value || dbPending.value ||
    guardResolve || task.value?.type === 'verify') return;
  showTask({
    type: 'verify', title: '核实写入结果', db: db.value, generation: props.session.generation,
    rows: null, pending: false, previous: task.value,
  });
  void readVerification();
}

async function readVerification() {
  const verification = task.value;
  const targetDb = verification.db;
  if (verification.pending || !connected.value || targetDb !== db.value) return;
  const generation = props.session.generation;
  verification.rows = null;
  verification.pending = true;
  try {
    const keys = uncertainWrites.get(targetDb);
    const details = await Promise.all(keys.map(key => request('redis_detail', { key, start: '0' }, targetDb)));
    if (task.value !== verification) return;
    const primary = details[0].primary;
    if (!connected.value || props.session.generation !== generation ||
      details.some(result => !sameRedisPrimary(primary, result.primary))) {
      toast.warning('连接或主节点已变化，请重新读取目标键');
      return;
    }
    redis.reportHealth(props.session.id, generation, { primary });
    verification.generation = props.session.generation;
    verification.rows = details.map((result, index) => ({
      key: keys[index],
      label: byteLabel({ base64: keys[index], length: decodeBytes(keys[index]).length }),
      detail: result,
    }));
  } catch (cause) {
    if (task.value === verification) reportRequestError(cause);
  } finally {
    verification.pending = false;
  }
}

function finishVerification(confirmed = false) {
  if (confirmed && !verificationReady.value) return;
  const verification = task.value;
  if (confirmed) {
    uncertainWrites.delete(verification.db);
    toast.success('已核实目标键，可继续操作');
  }
  closeTask();
}

function beginDraft(change) {
  const ttl = detail.value?.ttlMs;
  const initialTtl = ttl > 0 ? String(Math.ceil(ttl / 1000)) : '-1';
  draft.value = { generation: props.session.generation, ttlInput: initialTtl, initialTtl, ...draft.value, ...change };
}

function openStringEditor() {
  const value = detail.value?.value;
  if (!complete(value)) return;
  const text = formatStringInput(value, 'utf8');
  const mode = valueMode.value;
  const format = mode === 'hex' || mode === 'base64' ? mode : text === null ? 'hex' : 'utf8';
  beginDraft({
    action: 'string',
    expected: value.base64,
    stringInput: { format, text: format === 'utf8' ? text : formatStringInput(value, format) },
  });
  nextTick(() => stringEditor.value?.querySelector('textarea')?.focus());
}

function showEntry(action, row) {
  const current = detail.value.kind;
  if (row && (!complete(row.value) || current === 'hash' && !complete(row.field))) return;
  showTask({
    type: 'entry', action, row, kind: current, title: action === 'remove' ? '删除成员' : action === 'add' ? '新增成员' : '编辑成员',
    expectedSize: current === 'list' && row ? detail.value.size : undefined,
    field: '',
    value: row ? readText(row.value) ?? '' : '',
    score: row?.score || '0',
  });
}

function openRename() {
  const name = readText(selected.value) ?? '';
  beginDraft({ renameInput: name });
  nextTick(() => { renameInput.value?.$el?.focus(); renameInput.value?.$el?.select(); });
}

function stageTask() {
  const state = task.value;
  if (state.type === 'entry') {
    const score = state.kind === 'zset' ? readScore(state.score) : undefined;
    if (score === null) {
      toast.warning('请输入有效的分数');
      return;
    }
    beginDraft({
      action: state.action,
      field: state.row?.field.base64 ?? encodeText(state.field),
      expected: state.row?.value.base64,
      expectedSize: state.expectedSize,
      value: state.value,
      index: state.row?.index,
      score,
      expectedScore: state.kind === 'zset' && state.row ? Number(state.row.score) : undefined,
    });
  }
  closeTask();
}

async function apply(change) {
  if (saving.value || !connected.value || writeUncertain.value) return false;
  const targetDb = db.value;
  saving.value = true;
  try {
    await request('redis_mutate', { change: { ...change, operationId: crypto.randomUUID() } }, targetDb);
    toast.success(change.action === 'delete' ? 'Redis 键已删除' : change.action === 'create' ? 'Redis 键已创建' : 'Redis 修改已保存');
    return true;
  } catch (cause) {
    const message = redisErrorMessage(cause);
    if (message.includes('写入结果未知')) {
      uncertainWrites.set(targetDb, redisAffectedKeys(change));
      toast.warning(message, 4000);
    } else {
      reportRequestError(cause);
    }
    return false;
  } finally {
    saving.value = false;
  }
}

async function saveDraft(leaving = false) {
  if (!dirty.value) {
    discardDraft();
    return true;
  }
  if (writeUncertain.value || draftError.value) return false;
  const { generation, stringInput, renameInput: name, ttlInput, initialTtl, ...pending } = draft.value;
  if (generation !== props.session.generation || !selected.value || !detail.value) {
    toast.warning('连接已变化，请放弃当前编辑后重新读取键详情');
    return false;
  }
  const change = { action: 'metadata', ...pending, key: selected.value.base64, kind: detail.value.kind };
  if (ttlInput !== initialTtl) change.ttlMs = editedTtl.value.ttlMs;
  if (name != null && editedName.value.base64 !== selected.value.base64) change.renameTo = editedName.value.base64;
  if (stringInput) change.valueBase64 = editedString.value.base64;
  if (!await apply(change)) return false;
  if (change.renameTo) selected.value = { base64: change.renameTo, length: atob(change.renameTo).length };
  draft.value = null;
  void loadDatabases();
  if (!leaving) {
    await loadKeys();
    await loadDetail();
  }
  return true;
}

function discardDraft() {
  draft.value = null;
}

async function submitTask() {
  const current = task.value;
  if (!current || taskSubmitDisabled.value) return;
  if (current.db !== db.value || current.generation !== props.session.generation) {
    toast.warning('连接或数据库已变化，请关闭此操作并重新打开');
    return;
  }
  if (current.type === 'create') {
    const score = current.kind === 'zset' ? readScore(current.score) : undefined;
    if (!current.key || score === null) {
      toast.warning('请填写键名和有效的初始值');
      return;
    }
    const key = encodeText(current.key);
    const change = newString.value
      ? { action: 'create', kind: 'string', key, valueBase64: newString.value.base64 }
      : { action: 'create', kind: current.kind, key, field: encodeText(current.field), value: current.value, score };
    if (newTtl.value.ttlMs > 0) change.ttlMs = newTtl.value.ttlMs;
    if (!await apply(change)) return;
    closeTask(false);
    selected.value = { base64: key, length: atob(key).length };
    narrowView.value = 'detail';
  } else if (current.type === 'delete') {
    const snapshot = current.snapshot;
    const change = { action: 'delete', kind: snapshot.kind, key: selected.value.base64, expectedSize: snapshot.size };
    if (snapshot.kind === 'string' && complete(snapshot.value)) change.expected = snapshot.value.base64;
    if (!await apply(change)) return;
    closeTask(false);
    selected.value = null;
    detail.value = null;
    narrowView.value = 'keys';
  } else { stageTask(); return; }
  await loadKeys();
  if (selected.value) await loadDetail();
  void loadDatabases();
  if (props.active) focusBrowser();
}

async function createKey() {
  if (task.value || !await canLeave()) return;
  showTask({
    type: 'create', title: '新建键', kind: 'string', key: '', field: '', value: '', score: '0',
    stringInput: { format: 'utf8', text: '' },
    ttlInput: '',
  });
}

async function copyKey() {
  const text = readText(selected.value);
  try {
    await navigator.clipboard.writeText(text ?? selected.value.base64);
    toast.success(text === null ? '已复制键名的 Base64 字节表示' : '已复制键名');
  } catch {
    toast.error('无法访问剪贴板');
  }
}

function recallCommand(command) {
  currentConsole.value.command = command;
  nextTick(() => consoleInput.value?.$el?.focus());
}

async function runCommand(fromHistory = null) {
  const state = currentConsole.value;
  const command = (fromHistory ?? state.command).trim();
  if (!connected.value || state.pending || consoleConfirmation.value || !command) return;
  let args;
  try {
    args = commandArgs(command);
  } catch (cause) {
    reportRequestError(cause);
    return;
  }
  const execution = { state, database: db.value, generation: props.session.generation, command, args, fromHistory };
  if (consoleWrites.has(args[0]?.toUpperCase())) {
    if (writeUncertain.value || saving.value || dbPending.value || dirty.value) {
      toast.warning('请先处理当前数据库的待核实写入或键详情中的修改');
      return;
    }
    consoleConfirmation.value = { ...execution, operationId: crypto.randomUUID() };
    return;
  }
  await executeConsole(execution);
}

function confirmConsoleWrite() {
  const execution = consoleConfirmation.value;
  if (!execution) return;
  consoleConfirmation.value = null;
  void executeConsole(execution);
}

async function executeConsole({ state, database, generation, command, args, fromHistory, operationId }) {
  if (!connected.value || state.pending || database !== db.value || generation !== props.session.generation) return;
  if (operationId && (saving.value || dbPending.value || writeUncertain.value || dirty.value)) return;
  state.pending = true;
  if (operationId) saving.value = true;
  if (fromHistory === null) state.command = '';
  state.history = [command, ...state.history.filter(item => item !== command)].slice(0, 50);
  try {
    const result = await request('redis_console', { args, operationId }, database);
    state.log = [...state.log.slice(-19), { command, result }];
    if (operationId) {
      needsScan = true;
      if (selected.value) {
        detailRevision += 1;
        detailPending.value = false;
        selected.value = null;
        detail.value = null;
        narrowView.value = 'keys';
      }
      void loadDatabases();
    }
  } catch (cause) {
    const message = redisErrorMessage(cause);
    state.log = [...state.log.slice(-19), { command, result: message, failed: true }];
    if (operationId && message.includes('写入结果未知')) {
      uncertainWrites.set(database, [encodeText(args[1])]);
    }
    reportRequestError(cause);
  } finally {
    state.pending = false;
    if (operationId) saving.value = false;
  }
}

function focusKeys(event) {
  const buttons = [...event.currentTarget.querySelectorAll('[data-redis-row]:not([disabled])')];
  const offset = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
  if (!offset) return;
  const index = buttons.indexOf(document.activeElement);
  if (buttons[index + offset]) {
    event.preventDefault();
    buttons[index + offset].focus();
  }
}
</script>

<template>
  <section class="redis-panel flex h-full min-h-0 flex-col text-foreground" aria-label="Redis 会话">
    <div v-if="databasesError || databases?.warnings?.length" role="status" class="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2 text-xs text-muted-foreground">
      <span>{{ databasesError || databases.warnings.join('；') }}</span>
      <form v-if="!cluster && databases?.total == null" class="flex items-center gap-2" @submit.prevent="switchDb()">
        <Input v-model="dbInput" class="w-24" inputmode="numeric" aria-label="手动指定数据库编号" :disabled="!connected || dbPending || !!task" />
        <Button type="submit" size="sm" variant="outline" :disabled="!connected || dbPending || !!task || requestedDb === null || requestedDb === db">切换</Button>
      </form>
    </div>
    <div v-if="writeUncertain" class="flex flex-wrap items-center gap-2 border-b border-border px-3 py-1.5 text-xs">
      <span class="text-destructive" role="status">DB {{ db }} 有写入结果待核实</span>
      <Button v-if="task?.type !== 'verify'" size="xs" variant="outline" :disabled="!connected || saving || dbPending || task?.type === 'unsaved'" @click="openVerification">核实写入</Button>
    </div>
    <Tabs v-model="page" :unmount-on-hide="false" class="min-h-0 flex-1 !gap-0">
      <div class="redis-toolbar flex shrink-0 items-center gap-2 border-b border-border pb-2 pt-1 pl-3 pr-20">
        <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
          <TabsList class="shrink-0" variant="line" aria-label="Redis 视图">
            <TabsTrigger v-for="item in pages" :key="item.id" :value="item.id" :disabled="task?.type === 'unsaved'">{{ item.label }}</TabsTrigger>
          </TabsList>
        </div>
        <div class="ml-auto flex shrink-0 items-center gap-1">
          <Button v-if="session.status === 'disconnected'" variant="outline" size="sm" @click="redis.connect(session.id)">重连</Button>
          <SettingsSelect item-class="text-xs" :model-value="db" :options="databaseOptions" :trigger-label="'DB ' + db"
            aria-label="选择 Redis 数据库" class="redis-db-select !w-24"
            :disabled="!connected || cluster || saving || dbPending || !!task || !!consoleConfirmation" @update:model-value="switchDb" />
        </div>
      </div>
      <TabsContent value="data" class="min-h-0 flex-1">
        <div class="redis-browser h-full" :data-view="task ? 'detail' : narrowView">
          <section class="redis-keys flex min-h-0 min-w-0 flex-col border-r border-border" aria-label="键列表">
            <header class="grid shrink-0 gap-2 px-2.5 pb-2 pt-2.5">
              <form ref="keySearch" class="flex min-w-0 items-center gap-1" autocomplete="off" @submit.prevent="searchKeys">
                <div class="relative min-w-0 flex-1">
                  <Search class="pointer-events-none absolute left-2 top-1/2 size-3 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
                  <Input v-model="patternInput" :maxlength="1024" type="text" size="sm" class="pl-6"
                    placeholder="键名模式，例如 user:*" aria-label="搜索 Redis 键" />
                </div>
                <Button type="submit" size="sm" variant="outline" :disabled="!connected">搜索</Button>
              </form>
              <div class="flex min-w-0 items-center justify-between gap-2">
                <SettingsSelect item-class="text-xs" v-model="kind" class="max-w-28" aria-label="键类型筛选" :options="filters" :disabled="!connected" />
                <div class="flex shrink-0 items-center gap-1">
                  <TooltipHint text="刷新">
                    <Button size="icon-sm" variant="ghost" aria-label="刷新" :disabled="!connected || scanPending || detailPending || databasesPending || connectionOverview?.pending || !!task" @click="refresh"><RefreshCw /></Button>
                  </TooltipHint>
                  <TooltipHint text="新建键">
                    <Button size="icon-sm" variant="ghost" aria-label="新建键" :disabled="!connected || saving || dbPending || writeUncertain || !!task" @click="createKey"><Plus /></Button>
                  </TooltipHint>
                </div>
              </div>
            </header>
            <div ref="keyViewport" class="min-h-0 flex-1 overflow-auto p-1 [overflow-anchor:none]" @keydown="focusKeys" @scroll.passive="queueKeyScroll">
              <div v-if="keys.length" class="relative" :style="{ height: totalHeight + 'px' }">
                <div class="absolute inset-x-0 top-0" :style="{ transform: 'translateY(' + translateY + 'px)' }">
                  <TooltipHint v-for="row in visibleItems" :key="row.id" v-memo="[row, selected?.base64 === row.key?.base64, !!task]" :text="row.fullLabel || row.label" side="right">
                    <Button data-redis-row :data-redis-group="row.group ? '' : undefined" :data-redis-key="row.group ? undefined : ''"
                      :variant="!row.group && selected?.base64 === row.key.base64 ? 'secondary' : 'ghost'"
                      :aria-pressed="row.group ? undefined : selected?.base64 === row.key.base64"
                      :aria-expanded="row.group ? row.expanded : undefined" size="sm" class="w-full !justify-start gap-1.5 !transition-none"
                      :class="{ 'disabled:opacity-75': !row.group && task && complete(row.key) }"
                      :style="{ height: rowHeight + 'px', paddingLeft: (8 + Math.min(row.depth, 10) * 16) + 'px' }"
                      :disabled="!!task || !row.group && !complete(row.key)" @click="row.group ? toggleGroup(row) : openKey(row)">
                      <span class="flex size-3.5 shrink-0 items-center justify-center" aria-hidden="true">
                        <ChevronDown v-if="row.group && row.expanded" />
                        <ChevronRight v-else-if="row.group" />
                      </span>
                      <Folder v-if="row.group" class="text-muted-foreground" aria-hidden="true" />
                      <File v-else class="text-muted-foreground" aria-hidden="true" />
                      <code class="min-w-0 flex-1 truncate text-left text-[13px] font-normal text-foreground">{{ row.label }}</code>
                      <span v-if="!row.group" class="shrink-0 font-normal text-muted-foreground">{{ kindNames.get(row.kind) ?? row.kind }}</span>
                    </Button>
                  </TooltipHint>
                </div>
              </div>
              <p v-if="!keys.length" class="p-2 text-xs text-muted-foreground">{{ scanPending ? '正在扫描匹配的键…' : !scanComplete ? '尚未扫描完，可继续加载全部' : '没有匹配的键' }}</p>
            </div>
            <footer class="flex min-h-8 shrink-0 items-center justify-between gap-1 border-t border-border px-2.5 py-1 text-xs text-muted-foreground">
              <span aria-live="polite">{{ scanPending ? '扫描中 · ' : '' }}已加载 {{ keys.length }} 个键</span>
              <TooltipHint v-if="scanPending" text="停止加载">
                <Button variant="ghost" size="icon-xs" aria-label="停止加载" :disabled="scanStopRequested" @click="scanStopRequested = true"><Square /></Button>
              </TooltipHint>
              <TooltipHint v-else-if="!scanComplete" text="加载全部">
                <Button variant="ghost" size="icon-xs" aria-label="加载全部" :disabled="!connected || dbPending" @click="loadKeys(false, true)"><Download /></Button>
              </TooltipHint>
            </footer>
          </section>
          <section class="redis-detail flex min-h-0 min-w-0 flex-col" aria-label="键详情">
            <header v-show="!task && !!selected" ref="detailHeader" class="shrink-0 border-b border-border px-3 py-2.5">
              <Button class="redis-back mb-1 !pl-0" size="sm" variant="ghost" @click="returnToKeys"><ArrowLeft />返回键列表</Button>
              <div v-if="selected" class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
                <div class="flex min-w-0 max-w-full items-center gap-1">
                  <Input v-if="draft?.renameInput != null" ref="renameInput" v-model="draft.renameInput" aria-label="重命名键"
                    class="!h-7 min-w-0 w-48 max-w-full font-mono" autocomplete="off" :disabled="saving" :aria-invalid="!!editedName?.error"
                    @keydown.enter.prevent="saveDraft()" @keydown.esc.prevent="discardDraft" />
                  <h3 v-else class="redis-key-title min-w-0 break-all font-mono font-semibold leading-6">{{ keyLabel }}</h3>
                  <TooltipHint text="复制键名">
                    <Button variant="ghost" size="icon-xs" aria-label="复制键名" @click="copyKey"><Copy /></Button>
                  </TooltipHint>
                </div>
                <div v-if="detail" class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                  <Badge variant="secondary" class="rounded-md font-mono font-normal">{{ kindNames.get(detail.kind) ?? detail.kind }}</Badge>
                  <div v-if="draft" class="flex items-center gap-1.5">
                    <Label :for="'redis-edit-ttl-' + session.id">TTL</Label>
                    <TooltipHint text="单位秒，-1 或留空设为永久">
                      <Input :id="'redis-edit-ttl-' + session.id" v-model="draft.ttlInput" class="!h-7 !w-24 font-mono" type="text"
                        inputmode="text" placeholder="-1" autocomplete="off" :disabled="saving" :aria-invalid="!!editedTtl?.error"
                        @keydown.enter.prevent="saveDraft()" />
                    </TooltipHint>
                    <span>秒</span>
                  </div>
                  <TooltipHint v-else text="每 5 秒从 Redis 获取剩余存活时间">
                    <Badge variant="outline" class="rounded-md font-mono font-normal">TTL {{ ttlLabel }}</Badge>
                  </TooltipHint>
                  <Badge v-if="detail.value || kindNames.has(detail.kind)" variant="outline" class="rounded-md font-mono font-normal">
                    {{ detail.value ? detail.value.length + ' B' : detail.size + ' 项' }}
                  </Badge>
                </div>
                <div class="ml-auto flex flex-wrap items-center justify-end gap-1" role="group" aria-label="键操作">
                  <template v-if="detail">
                  <template v-if="draft">
                    <TooltipHint :text="draft.renameInput != null ? '取消重命名' : '取消编辑'">
                      <Button variant="outline" size="icon-sm" :aria-label="draft.renameInput != null ? '取消重命名' : '取消编辑'" :disabled="saving" @click="discardDraft"><X /></Button>
                    </TooltipHint>
                    <TooltipHint :text="saving ? '保存中…' : '保存修改'">
                      <Button size="icon-sm" aria-label="保存修改" :disabled="!dirty || saving || !connected || writeUncertain || !!draftError" @click="saveDraft()"><Check /></Button>
                    </TooltipHint>
                  </template>
                  <template v-else>
                    <TooltipHint v-if="detail.kind === 'string' && canEditString" text="编辑值">
                      <Button size="icon-sm" aria-label="编辑值" :disabled="!editable" @click="openStringEditor"><Pencil /></Button>
                    </TooltipHint>
                    <TooltipHint v-else-if="kindNames.has(detail.kind) && detail.kind !== 'string'" :text="detail.kind === 'hash' ? '新增字段' : '新增成员'">
                      <Button size="icon-sm" :aria-label="detail.kind === 'hash' ? '新增字段' : '新增成员'" :disabled="!editable" @click="showEntry('add')"><Plus /></Button>
                    </TooltipHint>
                    <TooltipHint text="重命名">
                      <Button variant="outline" size="icon-sm" aria-label="重命名" :disabled="!editable" @click="openRename"><TextCursorInput /></Button>
                    </TooltipHint>
                  </template>
                  <TooltipHint text="删除键">
                    <Button variant="destructive" size="icon-sm" aria-label="删除键" :disabled="!editable || !!draft" @click="showTask({ type: 'delete', title: '删除键', snapshot: detail })"><Trash2 /></Button>
                  </TooltipHint>
                  </template>
                  <span class="mx-1 h-4 border-l border-border" aria-hidden="true" />
                  <TooltipHint text="关闭键详情">
                    <Button variant="ghost" size="icon-sm" aria-label="关闭键详情" :disabled="saving || dbPending" @click="closeKey"><X /></Button>
                  </TooltipHint>
                </div>
              </div>
              <p v-if="draftError" class="mt-1 text-xs text-destructive" role="alert">{{ draftError }}</p>
            </header>
            <div v-show="!task" class="min-h-0 min-w-0 flex-1 overflow-auto p-3"
              :class="detail?.value && draft?.action !== 'string' ? 'flex flex-col' : ''">
              <p v-if="selectionOutsideFilter" class="mb-2 text-xs text-muted-foreground">当前键不在已加载的筛选结果中。</p>
              <RedisConnectionOverview v-if="!selected" ref="connectionOverview" :session="session" :database="db" :databases="databases" :active="active && page === 'data' && !task && !session.closing" />
              <p v-else-if="!detail" class="text-xs text-muted-foreground">{{ detailPending ? '正在读取键…' : '选择一个键查看详情。' }}</p>
              <template v-else-if="detail.value">
                <div v-if="draft?.action === 'string'" ref="stringEditor">
                  <RedisStringEditor v-model="draft.stringInput" :result="editedString" :disabled="saving" />
                </div>
                <RedisValueView v-else v-model="valueMode" :value="detail.value" hide-size fill />
              </template>
              <template v-else-if="kinds.some(option => option.value === detail.kind)">
                <Table>
                  <TableHeader><TableRow>
                    <TableHead v-if="detail.kind === 'hash' || detail.kind === 'list'">{{ detail.kind === 'hash' ? '字段' : '索引' }}</TableHead>
                    <TableHead>{{ detail.kind === 'hash' ? '值' : detail.kind === 'list' ? '元素' : '成员' }}</TableHead>
                    <TableHead v-if="detail.kind === 'zset'">分数</TableHead><TableHead>操作</TableHead>
                  </TableRow></TableHeader>
                  <TableBody><TableRow v-for="(row, index) in detail.entries" :key="String(entrySignature(row)) + ':' + index">
                    <TableCell v-if="detail.kind === 'hash'" class="max-w-40"><Button variant="ghost" size="xs" class="max-w-full" @click="showTask({ type: 'preview', title: '查看字段', value: row.field })"><code class="truncate">{{ byteLabel(row.field) }}</code></Button></TableCell>
                    <TableCell v-else-if="detail.kind === 'list'">{{ row.index }}</TableCell>
                    <TableCell class="max-w-48"><Button variant="ghost" size="xs" class="max-w-full" @click="showTask({ type: 'preview', title: '查看成员值', value: row.value })"><code class="truncate">{{ byteLabel(row.value) }}</code></Button></TableCell>
                    <TableCell v-if="detail.kind === 'zset'">{{ row.score }}</TableCell>
                    <TableCell class="whitespace-nowrap">
                      <Button variant="ghost" size="xs" :disabled="!editable || !!draft?.action || readText(row.value) === null || row.score !== '' && !Number.isFinite(Number(row.score))" @click="showEntry('update', row)">编辑</Button>
                      <Button variant="destructive" size="xs" :disabled="!editable || !!draft?.action || !complete(row.value) || row.score !== '' && !Number.isFinite(Number(row.score))" @click="showEntry('remove', row)">删除</Button>
                    </TableCell>
                  </TableRow></TableBody>
                </Table>
                <Button v-if="detail.nextCursor !== '0' && detail.entries.length < displayLimit" variant="ghost" size="xs" :disabled="detailPending" @click="loadDetail(false)">加载更多</Button>
              </template>
              <p v-else class="text-xs text-muted-foreground">当前键类型为 {{ detail.kind }}，暂无编辑器。</p>
            </div>
            <section v-if="task" ref="taskRef" class="flex min-h-0 flex-1 flex-col" :aria-label="task.title">
              <header class="shrink-0 border-b border-border px-3 py-2.5">
                <h3 class="text-xs font-semibold">{{ task.title }}</h3>
                <p class="mt-1 break-all text-xs text-muted-foreground">
                  {{ targetLabel }}<template v-if="task.keyLabel && !['create', 'verify'].includes(task.type)"> · <code>{{ task.keyLabel }}</code></template>
                </p>
                <div class="flex shrink-0 flex-wrap items-center gap-2 mt-3">
                  <template v-if="task.type === 'verify'">
                    <Button type="button" size="sm" variant="outline" data-initial-focus @click="finishVerification()">返回</Button>
                    <Button type="button" size="sm" variant="outline" :disabled="task.pending || !connected" @click="readVerification">重新读取</Button>
                    <Button type="button" size="sm" :disabled="!verificationReady" @click="finishVerification(true)">已核实，恢复写入</Button>
                  </template>
                  <template v-else>
                    <Button type="button" size="sm" variant="outline" data-initial-focus :disabled="saving" @click="resolveGuard(false)">{{ task.type === 'preview' ? '返回键详情' : '取消' }}</Button>
                    <template v-if="task.type === 'unsaved'">
                      <Button type="button" size="sm" variant="outline" :disabled="saving" @click="discardDraft(); resolveGuard(true)">放弃修改</Button>
                      <Button type="button" size="sm" :disabled="saving || !connected || writeUncertain || !!draftError"
                        @click="async () => { if (await saveDraft(true)) resolveGuard(true); }">保存并继续</Button>
                    </template>
                    <Button v-else-if="task.type !== 'preview'" type="submit" :form="'redis-task-form-' + session.id" size="sm" :variant="task.type === 'delete' || task.action === 'remove' ? 'destructive' : 'default'"
                      :disabled="taskSubmitDisabled">{{ task.type === 'delete' ? '确认删除' : task.type === 'create' ? '创建' : '暂存修改' }}</Button>
                  </template>
                </div>
              </header>
              <form :id="'redis-task-form-' + session.id" class="min-h-0 flex-1 overflow-auto" autocomplete="off" @submit.prevent="submitTask">
                <div class="p-3">
                  <div class="grid max-w-xl gap-3">
                    <RedisValueView v-if="task.type === 'preview'" :value="task.value" />
                    <template v-else-if="task.type === 'verify'">
                      <p v-if="task.pending" class="text-xs text-muted-foreground">正在重新读取目标键…</p>
                      <p v-else-if="task.rows && !verificationReady" class="text-xs text-destructive">连接已变化，请重新读取目标键。</p>
                      <div v-for="row in verificationReady ? task.rows : []" :key="row.key" class="grid gap-2 rounded-md border border-border p-3">
                        <p class="break-all font-mono text-xs">{{ row.label }}</p>
                        <p v-if="row.detail.kind === 'none'" class="text-xs text-muted-foreground">键不存在</p>
                        <template v-else>
                          <p class="text-xs text-muted-foreground">{{ row.detail.kind }} · TTL {{ row.detail.ttlMs === -1 ? '永久' : Math.ceil(row.detail.ttlMs / 1000) + ' 秒' }}</p>
                          <RedisValueView v-if="row.detail.value" :value="row.detail.value" />
                          <p v-else class="text-xs text-muted-foreground">成员数量 {{ row.detail.size }}</p>
                        </template>
                      </div>
                    </template>
                    <template v-else-if="task.type === 'unsaved'">
                      <p class="text-xs">继续前，可以保存或放弃当前键的修改。</p>
                      <p v-if="draftError" class="text-xs text-destructive">{{ draftError }}</p>
                    </template>
                    <p v-else-if="task.type === 'delete'" class="text-xs">确认删除这个键及其中的所有数据？</p>
                    <template v-else-if="task.type === 'create' || task.type === 'entry'">
                      <template v-if="task.type === 'create'">
                        <div class="redis-task-field">
                          <Label :for="'redis-task-key-' + session.id">键名</Label>
                          <Input :id="'redis-task-key-' + session.id" v-model="task.key" type="text" autocomplete="off" data-initial-focus :disabled="saving" />
                        </div>
                        <div class="redis-task-field">
                          <Label :for="'redis-task-kind-' + session.id">类型</Label>
                          <SettingsSelect item-class="text-xs" :id="'redis-task-kind-' + session.id" v-model="task.kind" :options="kinds" :disabled="saving" />
                        </div>
                        <div class="redis-task-field">
                          <Label :for="'redis-create-ttl-' + session.id">TTL（秒）</Label>
                          <div class="grid gap-1">
                            <Input :id="'redis-create-ttl-' + session.id" v-model="task.ttlInput" type="text" inputmode="text" autocomplete="off"
                              placeholder="-1" :disabled="saving" :aria-invalid="!!newTtl?.error" :aria-describedby="'redis-create-ttl-hint-' + session.id" />
                            <p :id="'redis-create-ttl-hint-' + session.id" class="text-xs text-muted-foreground">-1 或留空设为永久</p>
                            <p v-if="newTtl?.error" class="text-xs text-destructive" role="alert">{{ newTtl.error }}</p>
                          </div>
                        </div>
                      </template>
                      <p v-if="task.action === 'remove'" class="break-all text-xs">确认删除「{{ byteLabel(task.row.value) }}」？</p>
                      <RedisStringEditor v-else-if="newString" v-model="task.stringInput" :result="newString" :disabled="saving" />
                      <template v-else>
                        <div v-if="task.kind === 'hash' && !task.row" class="redis-task-field">
                          <Label :for="'redis-task-field-' + session.id">字段名</Label>
                          <Input :id="'redis-task-field-' + session.id" v-model="task.field" type="text" autocomplete="off"
                            :data-initial-focus="task.type === 'entry' ? '' : undefined" :disabled="saving" />
                        </div>
                        <div class="redis-task-field">
                          <Label :for="'redis-task-value-' + session.id">{{ task.kind === 'hash' ? '值' : task.kind === 'list' ? '元素' : '成员' }}</Label>
                          <Textarea :id="'redis-task-value-' + session.id" v-model="task.value" :rows="3" :disabled="saving"
                            :data-initial-focus="task.type === 'entry' && (task.kind !== 'hash' || !!task.row) ? '' : undefined" />
                        </div>
                        <div v-if="task.kind === 'zset'" class="redis-task-field">
                          <Label :for="'redis-task-score-' + session.id">分数</Label>
                          <Input :id="'redis-task-score-' + session.id" v-model="task.score" type="text" inputmode="decimal" autocomplete="off" :disabled="saving" />
                        </div>
                      </template>
                    </template>
                  </div>
                </div>
              </form>
            </section>
          </section>
        </div>
      </TabsContent>
      <TabsContent value="console" class="min-h-0 flex-1">
        <div class="redis-console grid h-full min-h-0 gap-2 p-3">
          <section class="flex min-h-0 min-w-0 flex-col gap-2">
            <p class="text-xs text-muted-foreground">{{ targetLabel }}</p>
            <div role="log" class="min-h-0 flex-1 overflow-auto rounded-md border border-border bg-muted/20 p-2">
              <div v-for="(entry, index) in currentConsole.log" :key="index" class="mb-2 text-xs">
                <p class="break-all font-mono text-muted-foreground">&gt; {{ entry.command }}</p>
                <pre class="whitespace-pre-wrap break-all" :class="entry.failed ? 'text-destructive' : 'text-foreground'">{{ entry.result }}</pre>
              </div>
            </div>
            <form class="flex gap-2" autocomplete="off" @submit.prevent="runCommand()">
              <Input ref="consoleInput" v-model="currentConsole.command" size="sm" placeholder="例如 GET key 或 SET key value" aria-label="Redis 命令" />
              <TooltipHint :text="'写命令：' + [...consoleWrites].join('、') + '\n查询：PING、DBSIZE、GET、HGET、TYPE、TTL、PTTL、EXISTS、HLEN、LLEN、SCARD、ZCARD、ZSCORE'">
                <Button type="button" variant="ghost" size="icon-sm" aria-label="支持的 Redis 命令"><HelpCircle /></Button>
              </TooltipHint>
              <Button type="submit" size="sm" :disabled="!connected || currentConsole.pending">执行</Button>
            </form>
          </section>
          <aside class="redis-console-history flex min-h-0 min-w-0 flex-col border-l border-border pl-3 text-xs">
            <div class="mb-3 flex shrink-0 items-center justify-between gap-2"><h3 class="font-semibold">命令历史</h3><span class="text-muted-foreground">DB {{ db }}</span></div>
            <div class="min-h-0 flex-1 overflow-auto">
              <p v-if="!currentConsole.history.length" class="py-2 leading-relaxed text-muted-foreground">执行过的命令会显示在这里。</p>
              <div v-for="command in currentConsole.history" :key="command" class="group mb-1 flex min-w-0 items-start gap-1 rounded-md hover:bg-muted/40">
                <TooltipHint :text="command">
                  <Button variant="ghost" size="sm" class="!h-auto min-h-7 min-w-0 flex-1 !justify-start !whitespace-normal px-2 py-1.5" @click="recallCommand(command)"><code class="line-clamp-2 min-w-0 break-all text-left font-normal leading-5">{{ command }}</code></Button>
                </TooltipHint>
                <TooltipHint text="立即执行">
                  <Button variant="ghost" size="icon-sm" class="mt-1 shrink-0" aria-label="立即执行历史命令" :disabled="!connected || currentConsole.pending" @click="runCommand(command)"><Play /></Button>
                </TooltipHint>
              </div>
            </div>
          </aside>
        </div>
      </TabsContent>
      <TabsContent value="status" class="min-h-0 flex-1 overflow-hidden p-3">
        <div class="flex h-full min-h-0 flex-col gap-2">
          <div class="flex shrink-0 flex-wrap items-center gap-2">
            <code class="font-semibold">INFO</code>
            <SettingsSelect item-class="text-xs" v-model="infoCategory" :options="infoCategories" aria-label="INFO 类别筛选" class="!w-44" :disabled="!infoSections.length" />
            <span v-if="cluster" class="text-xs text-muted-foreground">单个集群节点</span>
            <span v-else-if="primaryLabel" class="text-xs text-muted-foreground">主节点 {{ primaryLabel }}</span>
            <Button class="ml-auto" size="sm" variant="outline" :disabled="!connected || infoPending" @click="loadInfo">
              <RefreshCw />{{ infoPending ? '刷新中…' : '刷新' }}
            </Button>
            <Button size="sm" variant="ghost" :disabled="!info.status" @click="copyInfo"><Copy />复制输出</Button>
          </div>
          <p v-if="info.status?.includes('# DuskTerm: INFO 输出已截断')" role="status" class="text-xs text-muted-foreground">INFO 输出超出预览上限，当前仅显示部分信息。</p>
          <div v-if="infoSections.length" class="min-h-0 min-w-0 flex-1 overflow-auto rounded-lg border border-border">
            <Table class="redis-info-table table-fixed">
              <TableCaption class="sr-only">Redis INFO 状态信息</TableCaption>
              <TableHeader class="sticky top-0 z-10 bg-background"><TableRow>
                <TableHead class="w-[45%]">名称</TableHead><TableHead>值</TableHead>
              </TableRow></TableHeader>
              <TableBody v-for="section in visibleInfoSections" :key="section.name">
                <TableRow class="redis-info-category bg-muted/40"><TableCell :colspan="2" class="font-semibold">{{ section.name }}</TableCell></TableRow>
                <TableRow v-for="(row, index) in section.rows" :key="row.name + ':' + index">
                  <TableCell class="!whitespace-normal break-all font-mono">{{ row.name }}</TableCell>
                  <TableCell class="!whitespace-normal break-all font-mono">{{ row.value || '（空值）' }}</TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
          <p v-else role="status" class="text-xs text-muted-foreground">{{ infoPending ? '正在读取 INFO…' : '暂无 INFO 输出' }}</p>
        </div>
      </TabsContent>
      <TabsContent value="topology" class="min-h-0 flex-1 overflow-hidden p-3">
        <div class="flex h-full min-h-0 flex-col gap-2">
        <div class="mb-2 flex flex-wrap items-center gap-2">
          <div class="mr-auto flex flex-wrap items-center gap-2">
            <span class="text-xs font-semibold">{{ redisModeLabels[mode] }}</span>
            <span v-if="topologyRows.length" class="text-xs text-muted-foreground">{{ topologyRows.length }} 个节点</span>
            <span v-if="info.topologyUpdatedAt" class="text-xs text-muted-foreground">查询于 {{ new Date(info.topologyUpdatedAt).toLocaleTimeString('zh-CN', { hour12: false }) }}</span>
          </div>
          <Button size="sm" variant="outline" :disabled="!connected || infoPending" @click="loadInfo">
            <RefreshCw />{{ infoPending ? '刷新中…' : '刷新' }}
          </Button>
        </div>
        <p v-if="topologyError" role="status" class="mb-2 break-words text-xs text-destructive">
          拓扑读取失败 · {{ topologyError }}<template v-if="info.topology"> · 显示上次结果</template>
        </p>
        <p v-else-if="!connected && info.topology" class="mb-2 text-xs text-muted-foreground">连接已断开，显示上次查询结果。</p>
        <p v-if="info.sentinels?.error" role="status" class="mb-2 break-words text-xs text-[var(--app-risk-warning)]">
          哨兵节点读取失败 · {{ info.sentinels.error }}<template v-if="info.sentinels.nodes"> · 显示上次发现结果</template>
        </p>
        <p v-if="!cluster && info.topology?.includes('# DuskTerm: INFO 输出已截断')" class="mb-2 text-xs text-muted-foreground">
          复制信息超出预览上限，部分副本可能未列出。
        </p>
        <RedisTopologyGraph v-if="topologyRows.length" :rows="topologyRows" :active="active && page === 'topology'" />
        <p v-else-if="!topologyError" role="status" class="text-xs text-muted-foreground">
          {{ infoPending ? '正在读取拓扑…' : connected ? '暂无拓扑节点' : '连接后可查看拓扑' }}
        </p>
        </div>
      </TabsContent>
    </Tabs>
    <AlertDialog :open="!!consoleConfirmation" @update:open="open => { if (!open) consoleConfirmation = null; }">
      <AlertDialogContent v-if="consoleConfirmation">
        <AlertDialogHeader>
          <AlertDialogTitle>执行 Redis 写命令</AlertDialogTitle>
          <AlertDialogDescription>将在 {{ session.name }}（{{ primaryLabel || formatRedisEndpoint(session.config) }}）的 DB {{ consoleConfirmation.database }} 执行以下命令。</AlertDialogDescription>
        </AlertDialogHeader>
        <code class="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted p-3 text-xs">{{ consoleConfirmation.command }}</code>
        <p v-if="consoleConfirmation.args[0]?.toUpperCase() === 'SET'" class="text-xs text-muted-foreground">SET 会覆盖同名字符串及原有的过期时间。</p>
        <AlertDialogFooter>
          <AlertDialogCancel>取消</AlertDialogCancel>
          <Button variant="destructive" :disabled="!connected || saving || dbPending || writeUncertain" @click="confirmConsoleWrite">确认执行</Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </section>
</template>

<style scoped>
.redis-panel { container-type: inline-size; font-size: 12px; }
.redis-panel :deep([data-slot="button"]),
.redis-panel :deep([data-slot="input"]),
.redis-panel :deep([data-slot="textarea"]),
.redis-panel :deep([data-slot="label"]),
.redis-panel :deep([data-slot="tabs-trigger"]),
.redis-panel :deep([data-slot="select-trigger"]),
.redis-panel :deep([data-slot="table"]) { font-size: 12px; }
.redis-key-title { font-size: 12px; }
.redis-panel :deep([data-redis-group][aria-expanded="true"]:not(:hover)) { background: transparent; }
.redis-panel :deep(.redis-info-table thead tr),
.redis-panel :deep(.redis-info-table tbody tr) { border-bottom: 0; }
.redis-panel :deep(.redis-info-table tbody tr.redis-info-category) { border-bottom: 1px solid hsl(var(--border)); }
.redis-browser { display: grid; grid-template-columns: clamp(190px, 31%, 290px) minmax(0, 1fr); }
.redis-back { display: none; }
.redis-task-field {
  display: grid;
  grid-template-columns: minmax(5.5rem, max-content) minmax(0, 1fr);
  align-items: start;
  column-gap: 0.75rem;
}
.redis-task-field > :first-child { padding-top: 0.5rem; }
.redis-console { grid-template-columns: minmax(0, 1fr) minmax(160px, 28%); }
@container (max-width: 540px) {
  .redis-browser { grid-template-columns: minmax(0, 1fr); }
  .redis-keys { border-right: 0; }
  .redis-browser[data-view="keys"] .redis-detail,
  .redis-browser[data-view="detail"] .redis-keys { display: none; }
  .redis-back { display: inline-flex; }
  .redis-console { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) minmax(100px, 30%); }
  .redis-console-history { border-left: 0; border-top: 1px solid hsl(var(--border)); padding-left: 0; padding-top: 0.75rem; }
}
@container (max-width: 420px) {
  .redis-toolbar { gap: 0.25rem; padding-left: 0.25rem; }
  .redis-db-select { width: 5rem !important; }
}
</style>
