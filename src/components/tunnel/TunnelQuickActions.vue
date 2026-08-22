<script setup>
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { invokeCommand } from '@/utils/ipc';
import { notifyTunnelsChanged, TUNNELS_CHANGED_EVENT } from '@/utils/tunnelEvents';
import { ArrowRight, LoaderCircle, Network, Play, Square } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger } from 'reka-ui';

const LOOPBACK_HOSTS = ['127.0.0.1', 'localhost', '::1'];

const props = defineProps({
  sessionId: {
    type: String,
    default: '',
  },
});

const open = ref(false);
const loading = ref(false);
const configs = ref([]);
const tunnels = ref([]);
const pendingConfigIds = ref(new Set());
const eventSource = `tunnel-quick-${crypto.randomUUID()}`;
let loadRequestId = 0;

const runningTunnelsByConfig = computed(() => {
  const grouped = new Map();
  for (const tunnel of tunnels.value) {
    if (!tunnel?.configId) continue;
    const items = grouped.get(tunnel.configId) || [];
    items.push(tunnel);
    grouped.set(tunnel.configId, items);
  }
  return grouped;
});

const runningCount = computed(() => configs.value.reduce(
  (count, config) => count + (isRunning(config.id) ? 1 : 0),
  0,
));

function isRunning(configId) {
  return (runningTunnelsByConfig.value.get(configId) || []).length > 0;
}

function isPending(configId) {
  return pendingConfigIds.value.has(configId);
}

function setPending(configId, pending) {
  const next = new Set(pendingConfigIds.value);
  if (pending) next.add(configId);
  else next.delete(configId);
  pendingConfigIds.value = next;
}

function modeTag(mode) {
  if (mode === 'remote') return 'R';
  if (mode === 'dynamic') return 'D';
  return 'L';
}

function endpoint(config) {
  const source = `${config.listenHost}:${config.listenPort}`;
  if (config.mode === 'dynamic') return `${source} SOCKS5`;
  return `${source} → ${config.targetHost}:${config.targetPort}`;
}

function isPublicBindHost(host) {
  return !LOOPBACK_HOSTS.includes(String(host || '').trim().toLowerCase());
}

function highRiskReasons(config) {
  const reasons = [];
  if (isPublicBindHost(config.listenHost)) {
    reasons.push(`监听地址 ${config.listenHost} 会暴露到非本机网络`);
  }
  if (Number(config.listenPort) < 1024) {
    reasons.push(`监听端口 ${config.listenPort} 属于系统保留端口`);
  }
  if (config.mode !== 'dynamic' && Number(config.targetPort) < 1024) {
    reasons.push(`目标端口 ${config.targetPort} 属于系统保留端口`);
  }
  if (config.mode === 'remote') {
    reasons.push('远程转发会直接影响目标服务器的暴露面');
  }
  return reasons;
}

async function loadData({ silent = true } = {}) {
  const sessionId = String(props.sessionId || '').trim();
  const requestId = ++loadRequestId;
  if (!sessionId) {
    configs.value = [];
    tunnels.value = [];
    loading.value = false;
    return;
  }

  loading.value = true;
  const [configsResult, tunnelsResult] = await Promise.allSettled([
    invokeCommand('list_tunnel_configs'),
    invokeCommand('list_tunnels'),
  ]);

  if (requestId !== loadRequestId || sessionId !== props.sessionId) return;

  if (configsResult.status === 'fulfilled') {
    configs.value = configsResult.value.filter((config) => config.sessionId === sessionId);
  } else if (!silent) {
    toast.error(`读取隧道配置失败: ${configsResult.reason}`);
  }

  if (tunnelsResult.status === 'fulfilled') {
    tunnels.value = tunnelsResult.value;
  } else if (!silent) {
    toast.error(`读取隧道状态失败: ${tunnelsResult.reason}`);
  }

  loading.value = false;
}

async function startConfig(config) {
  if (isPending(config.id) || isRunning(config.id)) return;

  if (isPublicBindHost(config.listenHost) && !config.allowPublicBind) {
    toast.warning('公网监听需要先在隧道管理中启用“允许公网监听”。');
    return;
  }

  const reasons = highRiskReasons(config);
  if (reasons.length) {
    try {
      await confirm({
        title: '确认高风险隧道配置',
        content: `检测到以下风险：${reasons.join('；')}。确认后继续启动。`,
        okText: '继续启动',
        cancelText: '取消',
        danger: true,
      });
    } catch {
      return;
    }
  }

  setPending(config.id, true);
  try {
    await invokeCommand('start_tunnel_from_config', { configId: config.id });
    await loadData({ silent: false });
    notifyTunnelsChanged({ source: eventSource });
    toast.success(`隧道“${config.name}”已启动`);
  } catch (error) {
    await loadData();
    toast.error(`启动隧道失败: ${error}`);
  } finally {
    setPending(config.id, false);
  }
}

async function stopConfig(config) {
  if (isPending(config.id)) return;
  const runningTunnels = runningTunnelsByConfig.value.get(config.id) || [];
  if (!runningTunnels.length) return;

  setPending(config.id, true);
  try {
    await Promise.all(runningTunnels.map((tunnel) => invokeCommand('stop_tunnel', { id: tunnel.id })));
    await loadData({ silent: false });
    notifyTunnelsChanged({ source: eventSource });
    toast.success(`隧道“${config.name}”已停止`);
  } catch (error) {
    await loadData();
    toast.error(`停止隧道失败: ${error}`);
  } finally {
    setPending(config.id, false);
  }
}

async function toggleConfig(config) {
  if (isRunning(config.id)) await stopConfig(config);
  else await startConfig(config);
}

function handleTunnelsChanged(event) {
  if (event?.detail?.source === eventSource) return;
  void loadData();
}

watch(
  () => props.sessionId,
  () => {
    open.value = false;
    configs.value = [];
    tunnels.value = [];
    pendingConfigIds.value = new Set();
    void loadData();
  },
  { immediate: true },
);

watch(open, (nextOpen) => {
  if (nextOpen) void loadData({ silent: false });
});

onMounted(() => {
  window.addEventListener(TUNNELS_CHANGED_EVENT, handleTunnelsChanged);
});

onUnmounted(() => {
  loadRequestId += 1;
  window.removeEventListener(TUNNELS_CHANGED_EVENT, handleTunnelsChanged);
});
</script>

<template>
  <div v-if="configs.length" class="tunnel-quick-actions">
    <PopoverRoot v-model:open="open">
      <PopoverTrigger as-child>
        <button
          type="button"
          class="tunnel-quick-trigger"
          :class="{ 'is-open': open, 'has-running': runningCount > 0 }"
          :aria-label="`当前会话隧道（${runningCount}/${configs.length} 运行中）`"
          title="当前会话隧道"
          @mousedown.stop
          @click.stop
        >
          <Network :size="15" stroke-width="1.9" />
          <span v-if="runningCount" class="tunnel-running-dot" aria-hidden="true" />
        </button>
      </PopoverTrigger>

      <PopoverPortal>
        <PopoverContent
          side="bottom"
          align="end"
          :side-offset="5"
          :collision-padding="12"
          class="tunnel-quick-popover"
          @mousedown.stop
        >
          <div class="tunnel-quick-header">
            <strong>当前会话隧道</strong>
            <span>{{ runningCount }}/{{ configs.length }} 运行中</span>
          </div>

          <div class="tunnel-quick-list">
            <div
              v-for="config in configs"
              :key="config.id"
              class="tunnel-quick-row"
              :class="{ 'is-running': isRunning(config.id) }"
            >
              <div class="tunnel-quick-main">
                <div class="tunnel-quick-title">
                  <span class="tunnel-mode-tag">{{ modeTag(config.mode) }}</span>
                  <strong>{{ config.name }}</strong>
                  <span class="tunnel-status">
                    <span class="tunnel-state-dot" aria-hidden="true" />
                    {{ isRunning(config.id) ? '运行中' : '已停止' }}
                  </span>
                </div>
                <div class="tunnel-endpoint" :title="endpoint(config)">
                  <span>{{ config.listenHost }}:{{ config.listenPort }}</span>
                  <template v-if="config.mode !== 'dynamic'">
                    <ArrowRight :size="12" />
                    <span>{{ config.targetHost }}:{{ config.targetPort }}</span>
                  </template>
                  <span v-else>SOCKS5</span>
                </div>
              </div>

              <button
                type="button"
                class="tunnel-row-action"
                :class="{ 'is-stop': isRunning(config.id) }"
                :disabled="isPending(config.id)"
                :aria-label="`${isRunning(config.id) ? '停止' : '启动'} ${config.name}`"
                :title="isRunning(config.id) ? '停止隧道' : '启动隧道'"
                @click.stop="toggleConfig(config)"
              >
                <LoaderCircle v-if="isPending(config.id)" :size="14" class="tunnel-spinner" />
                <Square v-else-if="isRunning(config.id)" :size="13" fill="currentColor" />
                <Play v-else :size="14" fill="currentColor" />
              </button>
            </div>
          </div>

          <div v-if="loading" class="tunnel-loading">正在更新状态…</div>
        </PopoverContent>
      </PopoverPortal>
    </PopoverRoot>
  </div>
</template>

<style scoped>
.tunnel-quick-actions {
  position: absolute;
  z-index: 21;
  top: 8px;
  right: 52px;
}

.tunnel-quick-trigger {
  position: relative;
  display: inline-flex;
  width: 26px;
  height: 26px;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid var(--app-border-shadow);
  border-radius: 7px;
  color: var(--app-terminal-close-color);
  background: var(--app-terminal-close-bg);
  box-shadow: var(--app-control-shadow);
  opacity: .82;
  transition: color var(--app-motion-control), background var(--app-motion-control), opacity var(--app-motion-control);
}

.tunnel-quick-trigger:hover,
.tunnel-quick-trigger:focus-visible,
.tunnel-quick-trigger.is-open {
  color: var(--app-terminal-close-hover-color);
  background: var(--app-terminal-close-hover-bg);
  opacity: 1;
}

.tunnel-quick-trigger.is-open,
.tunnel-quick-trigger.has-running {
  color: hsl(var(--primary));
}

.tunnel-quick-trigger:focus-visible {
  outline: none;
  box-shadow: var(--app-focus-shadow);
}

.tunnel-running-dot {
  position: absolute;
  top: 4px;
  right: 4px;
  width: 5px;
  height: 5px;
  border-radius: 999px;
  background: #22c55e;
  box-shadow: 0 0 0 2px color-mix(in srgb, #22c55e 18%, transparent);
}

:global(.tunnel-quick-popover) {
  z-index: var(--z-select);
  width: min(370px, calc(100vw - 28px));
  overflow: hidden;
  border: 1px solid var(--app-border-dark);
  border-radius: 11px;
  color: hsl(var(--popover-foreground));
  background: hsl(var(--popover));
  box-shadow: var(--niri-shadow-dialog);
  outline: none;
}

:global(.tunnel-quick-header) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border-bottom: 1px solid hsl(var(--border));
}

:global(.tunnel-quick-header strong) {
  font-size: 12px;
  font-weight: 600;
}

:global(.tunnel-quick-header span),
:global(.tunnel-loading) {
  color: hsl(var(--muted-foreground));
  font-size: 11px;
}

:global(.tunnel-quick-list) {
  max-height: min(360px, calc(100vh - 90px));
  overflow-y: auto;
}

:global(.tunnel-quick-row) {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 12px;
  padding: 10px 12px;
  border-bottom: 1px solid hsl(var(--border));
}

:global(.tunnel-quick-row:last-child) {
  border-bottom: 0;
}

:global(.tunnel-quick-main) {
  min-width: 0;
}

:global(.tunnel-quick-title) {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

:global(.tunnel-quick-title strong) {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 500;
}

:global(.tunnel-mode-tag) {
  display: inline-flex;
  width: 17px;
  height: 17px;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  color: hsl(var(--muted-foreground));
  background: hsl(var(--muted));
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 10px;
}

:global(.tunnel-status) {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 4px;
  color: hsl(var(--muted-foreground));
  font-size: 11px;
}

:global(.tunnel-state-dot) {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: hsl(var(--muted-foreground));
  opacity: .55;
}

:global(.tunnel-quick-row.is-running .tunnel-status) {
  color: #22c55e;
}

:global(.tunnel-quick-row.is-running .tunnel-state-dot) {
  background: #22c55e;
  opacity: 1;
}

:global(.tunnel-endpoint) {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 5px;
  margin-top: 5px;
  overflow: hidden;
  color: hsl(var(--muted-foreground));
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
  white-space: nowrap;
}

:global(.tunnel-endpoint span) {
  overflow: hidden;
  text-overflow: ellipsis;
}

:global(.tunnel-row-action) {
  display: inline-flex;
  width: 30px;
  height: 30px;
  align-self: center;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid var(--app-border-dark);
  border-radius: 8px;
  color: hsl(var(--primary));
  background: hsl(var(--background));
  transition: color var(--app-motion-control), background var(--app-motion-control), opacity var(--app-motion-control);
}

:global(.tunnel-row-action:hover:not(:disabled)) {
  background: hsl(var(--accent));
}

:global(.tunnel-row-action.is-stop) {
  color: hsl(var(--destructive));
}

:global(.tunnel-row-action:disabled) {
  cursor: wait;
  opacity: .55;
}

:global(.tunnel-spinner) {
  animation: tunnel-spin .8s linear infinite;
}

:global(.tunnel-loading) {
  padding: 7px 12px;
  border-top: 1px solid hsl(var(--border));
  text-align: center;
}

@keyframes tunnel-spin {
  to { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  :global(.tunnel-spinner) {
    animation-duration: 1.6s;
  }
}
</style>
