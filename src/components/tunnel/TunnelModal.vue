<script setup>
import '@/components/ui/dialog/management-dialog.css';
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import { TooltipHint } from '@/components/ui/tooltip';
import TunnelSessionTreeSelect from '@/components/tunnel/TunnelSessionTreeSelect.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { Copy, MoreHorizontal, Plus, RefreshCw, Save as SaveIcon, Trash2 } from '@lucide/vue';
import { computed, onUnmounted, reactive, ref, watch } from 'vue';
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuPortal,
  DropdownMenuRoot,
  DropdownMenuTrigger,
} from 'reka-ui';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand } from '@/utils/ipc';
import { notifyTunnelsChanged } from '@/utils/tunnelEvents';

const LOOPBACK_HOSTS = ['127.0.0.1', 'localhost', '::1'];
const props = defineProps({
  visible: Boolean,
  preferredSessionId: String,
});

const emit = defineEmits(['update:visible']);

const dialogOpen = computed({
  get: () => props.visible,
  set: (open) => {
    if (!open) closeModal();
  },
});

const sshStore = useSshStore();

const loadingConfigs = ref(false);
const loadingTunnels = ref(false);
const saving = ref(false);
const starting = ref(false);
const stopping = ref(false);
const tunnelConfigs = ref([]);
const tunnels = ref([]);
const selectedSessionId = ref('');
const selectedConfigId = ref('');

const draft = reactive(createEmptyDraft());

let modalLoadId = 0;
let configsRequestId = 0;
let tunnelsRequestId = 0;

function createEmptyDraft(sessionId = '') {
  return {
    id: '',
    sessionId,
    name: '',
    mode: 'local',
    listenHost: '127.0.0.1',
    listenPort: 15432,
    targetHost: '127.0.0.1',
    targetPort: 5432,
    serverAliveInterval: 0,
    allowPublicBind: false,
    createdAt: 0,
    updatedAt: 0,
  };
}

function applyDraft(nextDraft = createEmptyDraft(selectedSessionId.value)) {
  Object.assign(draft, createEmptyDraft(selectedSessionId.value), {
    ...nextDraft,
    sessionId: nextDraft.sessionId || selectedSessionId.value || '',
    listenHost: nextDraft.listenHost || '127.0.0.1',
    listenPort: Number(nextDraft.listenPort || 15432),
    targetHost: nextDraft.targetHost || '127.0.0.1',
    targetPort: Number(nextDraft.targetPort || 5432),
    serverAliveInterval: Number(nextDraft.serverAliveInterval ?? 0),
    allowPublicBind: !!nextDraft.allowPublicBind,
    createdAt: Number(nextDraft.createdAt || 0),
    updatedAt: Number(nextDraft.updatedAt || 0),
  });
}

const savedSessions = computed(() =>
  (sshStore.savedSessions || []).filter((session) => {
    const protocol = String(session.protocol || 'ssh').toLowerCase();
    return protocol === 'ssh';
  }),
);

const currentConfigTunnels = computed(() =>
  tunnels.value.filter((item) => item.configId === selectedConfigId.value),
);

const currentTunnel = computed(() => currentConfigTunnels.value[0] || null);
const operationPending = computed(() => saving.value || starting.value || stopping.value);

const runningConfigIds = computed(() => {
  const ids = new Set();
  for (const item of tunnels.value) {
    if (item?.configId) ids.add(item.configId);
  }
  return ids;
});

const requiresTarget = computed(() => draft.mode === 'local' || draft.mode === 'remote');

const listenHostLabel = computed(() => {
  if (draft.mode === 'remote') return '远程监听地址';
  if (draft.mode === 'dynamic') return '本地 SOCKS5 地址';
  return '本地监听地址';
});

const listenPortLabel = computed(() => {
  if (draft.mode === 'remote') return '远程端口';
  if (draft.mode === 'dynamic') return 'SOCKS5 端口';
  return '本地端口';
});

const targetHostLabel = computed(() => draft.mode === 'remote' ? '本地目标主机' : '远程目标主机');

const isPublicBindHost = computed(() => {
  const host = String(draft.listenHost || '').trim().toLowerCase();
  return host && !LOOPBACK_HOSTS.includes(host);
});

const usesPrivilegedListenPort = computed(() => Number(draft.listenPort) > 0 && Number(draft.listenPort) < 1024);
const usesPrivilegedTargetPort = computed(() =>
  requiresTarget.value && Number(draft.targetPort) > 0 && Number(draft.targetPort) < 1024,
);

function isConfigRunning(configId) {
  return runningConfigIds.value.has(configId);
}

function cancelModalWork() {
  modalLoadId += 1;
  configsRequestId += 1;
  tunnelsRequestId += 1;
  loadingConfigs.value = false;
  loadingTunnels.value = false;
}

function closeModal() {
  cancelModalWork();
  emit('update:visible', false);
}

function hydrateFromConfig(config) {
  selectedConfigId.value = config?.id || '';
  if (config?.sessionId) {
    selectedSessionId.value = config.sessionId;
  }
  applyDraft(config || createEmptyDraft(selectedSessionId.value));
}

function buildConfigLabel(config) {
  if (config?.name) return config.name;
  const mode = String(config?.mode || 'local').toUpperCase();
  const listen = `${config?.listenHost || '127.0.0.1'}:${config?.listenPort || ''}`;
  return `${listen} [${mode}]`;
}

function normalizePayload() {
  return {
    id: draft.id || selectedConfigId.value || '',
    sessionId: selectedSessionId.value,
    name: String(draft.name || '').trim(),
    mode: String(draft.mode || 'local'),
    listenHost: String(draft.listenHost || '').trim() || '127.0.0.1',
    listenPort: Number(draft.listenPort),
    targetHost: requiresTarget.value ? String(draft.targetHost || '').trim() : null,
    targetPort: requiresTarget.value ? Number(draft.targetPort) : null,
    serverAliveInterval: Number(draft.serverAliveInterval ?? 0),
    allowPublicBind: !!draft.allowPublicBind,
    createdAt: Number(draft.createdAt || 0),
    updatedAt: Number(draft.updatedAt || 0),
  };
}

function validateDraft() {
  if (!selectedSessionId.value) {
    toast.warning('请先选择要维护隧道配置的会话。');
    return false;
  }

  const payload = normalizePayload();
  if (!payload.listenPort || payload.listenPort < 1 || payload.listenPort > 65535) {
    toast.warning('监听端口必须在 1-65535 之间。');
    return false;
  }

  if (payload.serverAliveInterval !== 0 && (payload.serverAliveInterval < 10 || payload.serverAliveInterval > 120)) {
    toast.warning('保活间隔必须为 0（禁用）或 10-120 秒。');
    return false;
  }

  if (requiresTarget.value) {
    if (!payload.targetHost) {
      toast.warning('本地转发和远程转发需要填写目标主机。');
      return false;
    }
    if (!payload.targetPort || payload.targetPort < 1 || payload.targetPort > 65535) {
      toast.warning('目标端口必须在 1-65535 之间。');
      return false;
    }
  }

  return true;
}

async function fetchTunnels({ silent = false } = {}) {
  const requestId = ++tunnelsRequestId;
  loadingTunnels.value = true;
  try {
    const nextTunnels = await invokeCommand('list_tunnels');
    if (requestId === tunnelsRequestId) {
      tunnels.value = nextTunnels;
    }
  } catch (error) {
    if (requestId === tunnelsRequestId && !silent) {
      toast.error(`读取隧道列表失败: ${error}`);
    }
  } finally {
    if (requestId === tunnelsRequestId) {
      loadingTunnels.value = false;
    }
  }
}

async function loadConfigs(preferredConfigId = '') {
  const requestId = ++configsRequestId;
  loadingConfigs.value = true;
  let nextConfigs = [];
  try {
    nextConfigs = await invokeCommand('list_tunnel_configs');
  } catch (error) {
    if (requestId === configsRequestId) {
      toast.error(`读取隧道配置失败: ${error}`);
    }
  } finally {
    if (requestId === configsRequestId) {
      loadingConfigs.value = false;
    }
  }

  if (requestId !== configsRequestId) return;
  tunnelConfigs.value = nextConfigs;

  const nextConfig =
    tunnelConfigs.value.find((item) => item.id === preferredConfigId)
    || tunnelConfigs.value.find((item) => item.id === selectedConfigId.value)
    || tunnelConfigs.value.find((item) => item.sessionId === selectedSessionId.value)
    || tunnelConfigs.value[0]
    || null;

  if (nextConfig) {
    hydrateFromConfig(nextConfig);
  } else {
    selectedConfigId.value = '';
    applyDraft(createEmptyDraft(selectedSessionId.value));
  }
}

function ensureSelectedSession() {
  if (!savedSessions.value.length) {
    selectedSessionId.value = '';
    selectedConfigId.value = '';
    applyDraft(createEmptyDraft());
    return;
  }

  const preferred = props.preferredSessionId && savedSessions.value.some((item) => item.id === props.preferredSessionId)
    ? props.preferredSessionId
    : '';
  const current = selectedSessionId.value && savedSessions.value.some((item) => item.id === selectedSessionId.value)
    ? selectedSessionId.value
    : '';
  const nextSessionId = preferred || current || savedSessions.value[0].id;

  selectedSessionId.value = nextSessionId;
  applyDraft({ ...draft, sessionId: nextSessionId });
}

function createNewConfig() {
  selectedConfigId.value = '';
  const nextSessionId = selectedSessionId.value || savedSessions.value[0]?.id || '';
  selectedSessionId.value = nextSessionId;
  applyDraft(createEmptyDraft(nextSessionId));
}

async function saveCurrentConfig({ silent = false } = {}) {
  if (saving.value) return null;
  if (!validateDraft()) return null;

  saving.value = true;
  try {
    const saved = await invokeCommand('save_tunnel_config', {
      config: normalizePayload(),
    });
    notifyTunnelsChanged();
    await loadConfigs(saved.id);
    if (!silent) {
      toast.success('隧道配置已保存');
    }
    return saved;
  } catch (error) {
    toast.error(`保存隧道配置失败: ${error}`);
    return null;
  } finally {
    saving.value = false;
  }
}

async function duplicateConfig(config = null) {
  const configId = config?.id || selectedConfigId.value;
  if (!configId) {
    toast.info('请先选择一个已保存的隧道配置。');
    return;
  }

  try {
    const duplicated = await invokeCommand('duplicate_tunnel_config', { id: configId });
    notifyTunnelsChanged();
    await loadConfigs(duplicated.id);
    toast.success('隧道配置已复制');
  } catch (error) {
    toast.error(`复制隧道配置失败: ${error}`);
  }
}

async function deleteConfig(config = null) {
  if (stopping.value) return;
  const configId = config?.id || selectedConfigId.value;
  if (!configId) {
    toast.info('当前没有可删除的已保存配置。');
    return;
  }

  const targetConfig = config || tunnelConfigs.value.find((item) => item.id === configId) || null;
  const runningTunnels = tunnels.value.filter((item) => item.configId === configId);
  const configName = buildConfigLabel(targetConfig);

  try {
    await confirm({
      title: '删除隧道配置',
      content: runningTunnels.length
        ? `“${configName}”正在运行。删除前将先停止关联隧道，此操作无法撤销。`
        : `确定删除“${configName}”吗？此操作无法撤销。`,
      okText: runningTunnels.length ? '停止并删除' : '删除',
      cancelText: '取消',
      danger: true,
    });
  } catch {
    return;
  }

  const stopsRunningTunnels = runningTunnels.length > 0;
  if (stopsRunningTunnels) stopping.value = true;
  try {
    if (runningTunnels.length) {
      await Promise.all(runningTunnels.map((item) => invokeCommand('stop_tunnel', { id: item.id })));
    }
    await invokeCommand('delete_tunnel_config', { id: configId });
    notifyTunnelsChanged();
    if (selectedConfigId.value === configId) selectedConfigId.value = '';
    toast.success('隧道配置已删除');
    await Promise.all([fetchTunnels(), loadConfigs()]);
  } catch (error) {
    toast.error(`删除隧道配置失败: ${error}`);
  } finally {
    if (stopsRunningTunnels) stopping.value = false;
  }
}

async function startCurrentTunnel() {
  if (starting.value || saving.value) return;
  if (!validateDraft()) return;

  starting.value = true;
  try {
    const payload = normalizePayload();
    const highRiskReasons = [];
    if (isPublicBindHost.value && !payload.allowPublicBind) {
      toast.warning('公网监听需要先显式启用“允许公网监听”。');
      return;
    }
    if (isPublicBindHost.value) highRiskReasons.push(`监听地址 ${payload.listenHost} 会暴露到非本机网络`);
    if (usesPrivilegedListenPort.value) highRiskReasons.push(`监听端口 ${payload.listenPort} 属于系统保留端口`);
    if (usesPrivilegedTargetPort.value) highRiskReasons.push(`目标端口 ${payload.targetPort} 属于系统保留端口`);
    if (payload.mode === 'remote') highRiskReasons.push('远程转发会直接影响目标服务器的暴露面');

    if (highRiskReasons.length > 0) {
      try {
        await confirm({
          title: '确认高风险隧道配置',
          content: `检测到以下风险：${highRiskReasons.join('；')}。确认后继续启动。`,
          okText: '继续启动',
          cancelText: '取消',
          danger: true,
        });
      } catch {
        return;
      }
    }

    const saved = await saveCurrentConfig({ silent: true });
    if (!saved) return;

    await invokeCommand('start_tunnel_from_config', { configId: saved.id });
    notifyTunnelsChanged();
    toast.success('隧道已启动');
    await Promise.all([fetchTunnels(), loadConfigs(saved.id)]);
  } catch (error) {
    toast.error(`启动隧道失败: ${error}`);
  } finally {
    starting.value = false;
  }
}

async function stopConfigTunnels(configId = selectedConfigId.value) {
  if (stopping.value) return;
  const configTunnels = tunnels.value.filter((item) => item.configId === configId);
  if (!configTunnels.length) {
    toast.info('当前配置没有正在运行的隧道。');
    return;
  }

  stopping.value = true;
  try {
    await Promise.all(configTunnels.map((item) => invokeCommand('stop_tunnel', { id: item.id })));
    notifyTunnelsChanged();
    toast.success('当前配置关联的隧道已停止');
    await fetchTunnels();
  } catch (error) {
    toast.error(`停止当前配置隧道失败: ${error}`);
  } finally {
    stopping.value = false;
  }
}

async function stopAllTunnels() {
  if (stopping.value || !tunnels.value.length) return;
  stopping.value = true;
  try {
    await invokeCommand('stop_all_tunnels');
    notifyTunnelsChanged();
    toast.success('全部隧道已停止');
    await fetchTunnels();
  } catch (error) {
    toast.error(`停止全部隧道失败: ${error}`);
  } finally {
    stopping.value = false;
  }
}

async function copyToClipboard(text, label = '内容') {
  try {
    await navigator.clipboard.writeText(String(text || ''));
    toast.success(`${label}已复制`);
  } catch {
    toast.error(`复制${label}失败`);
  }
}

function copyProxyAddress(record) {
  if (!record) return;
  copyToClipboard(`${record.listenHost}:${record.listenPort}`, '代理地址');
}

function copyCommandPreview(record) {
  if (!record?.commandPreview) return;
  copyToClipboard(record.commandPreview, '隧道命令');
}

async function openModal() {
  const loadId = ++modalLoadId;

  await sshStore.loadSavedSessions();
  if (!props.visible || loadId !== modalLoadId) return;

  if (
    props.preferredSessionId
    && props.preferredSessionId !== selectedSessionId.value
    && savedSessions.value.some((item) => item.id === props.preferredSessionId)
  ) {
    selectedConfigId.value = '';
  }
  ensureSelectedSession();
  await Promise.all([loadConfigs(), fetchTunnels({ silent: true })]);
}

watch(
  () => props.visible,
  async (visible) => {
    if (visible) {
      await openModal();
    } else {
      cancelModalWork();
    }
  },
);

watch(selectedSessionId, (nextSessionId) => {
  draft.sessionId = nextSessionId || '';
});

watch(
  () => props.preferredSessionId,
  (nextSessionId) => {
    if (
      !props.visible
      || selectedConfigId.value
      || !nextSessionId
      || !savedSessions.value.some((item) => item.id === nextSessionId)
    ) return;
    selectedSessionId.value = nextSessionId;
  },
);

watch(
  () => draft.mode,
  (mode) => {
    if (mode === 'dynamic') {
      draft.targetHost = '';
      draft.targetPort = 0;
      return;
    }

    if (!draft.targetHost) draft.targetHost = '127.0.0.1';
    if (!draft.targetPort) draft.targetPort = 5432;
  },
);

onUnmounted(() => {
  cancelModalWork();
});
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent showCloseButton draggable class="management-dialog tunnel-dialog">
      <DialogHeader>
        <DialogTitle>隧道管理</DialogTitle>
        <DialogDescription class="sr-only">管理 SSH 隧道配置、转发方式与运行状态</DialogDescription>
      </DialogHeader>

      <div class="tunnel-toolbar">
        <div class="tunnel-status" :class="{ 'is-running': tunnels.length > 0 }">
          <span class="tunnel-status-dot" />
          <span>{{ loadingTunnels ? '正在刷新...' : (tunnels.length + ' 个隧道运行中') }}</span>
        </div>
        <div class="tunnel-toolbar-actions">
          <Button v-if="tunnels.length" class="tunnel-stop-all" variant="destructive" :disabled="operationPending" @click="stopAllTunnels">停止全部</Button>
          <TooltipHint text="刷新状态">
            <Button size="icon" variant="ghost" aria-label="刷新状态" :disabled="loadingTunnels" @click="fetchTunnels()"><RefreshCw /></Button>
          </TooltipHint>
          <Button variant="secondary" :disabled="operationPending" @click="createNewConfig"><Plus />新建配置</Button>
          <Button variant="secondary" :disabled="!selectedSessionId || operationPending" @click="saveCurrentConfig()"><SaveIcon />保存配置</Button>
        </div>
      </div>

      <div class="tunnel-layout">
        <aside class="tunnel-sidebar" aria-label="已保存配置">
          <div class="management-caption tunnel-sidebar-label">已保存配置</div>
          <div class="tunnel-config-list">
            <div v-if="loadingConfigs" class="management-empty">正在读取隧道配置...</div>
            <div v-else-if="tunnelConfigs.length === 0" class="management-empty">还没有已保存的隧道配置</div>
            <div v-else class="tunnel-config-rows">
              <div v-for="config in tunnelConfigs" :key="config.id" class="tunnel-config-item"
                :class="{ 'is-selected': selectedConfigId === config.id }">
                <button type="button" class="tunnel-config-select" :disabled="operationPending"
                  :aria-pressed="selectedConfigId === config.id" @click="hydrateFromConfig(config)">
                  <span class="tunnel-status-dot" :class="{ 'is-running': isConfigRunning(config.id) }" />
                  <span class="tunnel-config-name" :title="buildConfigLabel(config)">{{ buildConfigLabel(config) }}</span>
                </button>
                <DropdownMenuRoot>
                  <DropdownMenuTrigger as-child>
                    <Button size="icon" variant="ghost" :disabled="operationPending"
                      :aria-label="buildConfigLabel(config) + '更多操作'"><MoreHorizontal /></Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuPortal>
                    <DropdownMenuContent side="bottom" align="end" :side-offset="4" :collision-padding="16"
                      class="management-dialog-options z-[var(--z-select)] min-w-[108px] rounded-[10px] border border-[var(--app-border-dark)] bg-popover p-1 text-popover-foreground shadow-[var(--niri-shadow-dialog)] outline-none">
                      <DropdownMenuItem class="flex cursor-default select-none items-center gap-2 rounded-md px-3 outline-none focus:bg-accent focus:text-accent-foreground" @select="duplicateConfig(config)">
                        <Copy class="size-4" />复制
                      </DropdownMenuItem>
                      <DropdownMenuItem class="flex cursor-default select-none items-center gap-2 rounded-md px-3 text-destructive outline-none focus:bg-destructive/10" @select="deleteConfig(config)">
                        <Trash2 class="size-4" />删除
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenuPortal>
                </DropdownMenuRoot>
              </div>
            </div>
          </div>
        </aside>

        <div class="tunnel-form">
          <section class="tunnel-form-section">
            <div class="management-section-head">
              <h3 class="management-section-title">基本配置</h3>
              <span class="tunnel-status" :class="{ 'is-running': currentConfigTunnels.length > 0 }">
                <span class="tunnel-status-dot" />
                {{ currentConfigTunnels.length ? ('运行中' + (currentConfigTunnels.length > 1 ? ' ' + currentConfigTunnels.length : '')) : (selectedConfigId ? '已停止' : '新配置') }}
              </span>
            </div>
            <div class="tunnel-basic-fields">
              <div class="management-field">
                <label for="tunnel-session" class="management-label">所属会话</label>
                <TunnelSessionTreeSelect id="tunnel-session" v-model="selectedSessionId"
                  :sessions="savedSessions" :group-order="sshStore.groupOrder" :disabled="!!selectedConfigId" />
              </div>
              <label class="management-field">
                <span class="management-label">名称</span>
                <Input v-model="draft.name" placeholder="如：postgres-dev" />
              </label>
            </div>
          </section>

          <section class="tunnel-form-section">
            <h3 id="tunnel-mode-label" class="management-section-title">转发类型</h3>
            <div class="management-segmented" role="group" aria-labelledby="tunnel-mode-label">
              <Button v-for="mode in [
                { value: 'local', label: '本地转发' },
                { value: 'remote', label: '远程转发' },
                { value: 'dynamic', label: '动态代理' },
              ]" :key="mode.value" type="button" variant="ghost" :aria-pressed="draft.mode === mode.value" @click="draft.mode = mode.value">
                {{ mode.label }}
              </Button>
            </div>
          </section>

          <section class="tunnel-form-section">
            <h3 class="management-section-title">连接配置</h3>
            <div class="tunnel-connection-fields">
              <label class="management-field">
                <span class="management-label">{{ listenHostLabel }}</span>
                <Input v-model="draft.listenHost" placeholder="127.0.0.1" class="font-mono" />
              </label>
              <label class="management-field">
                <span class="management-label">{{ listenPortLabel }}</span>
                <Input v-model.number="draft.listenPort" type="text" inputmode="numeric" autocomplete="off" class="font-mono" />
              </label>
              <template v-if="requiresTarget">
                <label class="management-field">
                  <span class="management-label">{{ targetHostLabel }}</span>
                  <Input v-model="draft.targetHost" placeholder="127.0.0.1" class="font-mono" />
                </label>
                <label class="management-field">
                  <span class="management-label">目标端口</span>
                  <Input v-model.number="draft.targetPort" type="text" inputmode="numeric" autocomplete="off" class="font-mono" />
                </label>
              </template>
            </div>
          </section>

          <details class="tunnel-advanced">
            <summary>高级选项</summary>
            <div class="tunnel-advanced-fields">
              <label class="management-field">
                <span class="management-label">保活间隔（秒）</span>
                <Input v-model.number="draft.serverAliveInterval" type="text" inputmode="numeric" autocomplete="off" class="font-mono" />
              </label>
              <label class="tunnel-public-bind">
                <Checkbox :model-value="draft.allowPublicBind" @update:model-value="(value) => { draft.allowPublicBind = !!value; }" />
                <span>允许公网监听（高风险）</span>
              </label>
            </div>
          </details>
        </div>
      </div>

      <DialogFooter class="management-footer">
        <Button variant="ghost" @click="closeModal">关闭</Button>
        <Button v-if="currentTunnel" variant="outline" @click="copyProxyAddress(currentTunnel)">复制地址</Button>
        <Button v-if="currentTunnel?.commandPreview" variant="outline" @click="copyCommandPreview(currentTunnel)">复制命令</Button>
        <Button v-if="currentConfigTunnels.length" variant="destructive" :disabled="operationPending" @click="stopConfigTunnels()">停止隧道</Button>
        <Button v-else :disabled="operationPending || !selectedSessionId" @click="startCurrentTunnel">启动隧道</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.tunnel-toolbar { display: flex; flex-shrink: 0; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; padding-bottom: 12px; border-bottom: 1px solid var(--app-border-shadow); }
.tunnel-status { display: inline-flex; align-items: center; gap: 8px; color: var(--app-text-secondary); font-size: var(--app-font-caption-size); }
.tunnel-status.is-running, .tunnel-status-dot.is-running { color: var(--app-toast-success); }
.tunnel-status-dot { width: 8px; height: 8px; flex-shrink: 0; border-radius: 50%; background: currentColor; color: inherit; }
.tunnel-toolbar-actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 8px; margin-left: auto; }
.tunnel-layout { display: grid; min-height: 0; flex: 1; grid-template-columns: 180px minmax(0, 1fr); gap: 16px; overflow: hidden; }
.tunnel-sidebar { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 8px; padding-right: 16px; border-right: 1px solid var(--app-border-shadow); }
.tunnel-sidebar-label { min-height: var(--app-control-height); display: flex; align-items: center; }
.tunnel-config-list { min-height: 0; overflow-y: auto; }
.tunnel-config-rows { display: flex; flex-direction: column; gap: 4px; }
.tunnel-config-item { display: flex; align-items: center; gap: 4px; padding-right: 4px; border-radius: 8px; transition: background var(--app-motion-control); }
.tunnel-config-item:hover { background: hsl(var(--foreground) / 0.05); }
.tunnel-config-item.is-selected { background: hsl(var(--primary) / 0.12); box-shadow: inset 2px 0 hsl(var(--primary)); }
.tunnel-config-select { display: flex; min-width: 0; min-height: 40px; flex: 1; align-items: center; gap: 8px; padding: 8px 10px; border: 0; border-radius: 8px; color: var(--app-text); background: transparent; text-align: left; }
.tunnel-config-select:disabled { cursor: not-allowed; opacity: .5; }
.tunnel-config-select:focus-visible { outline: none; box-shadow: var(--app-focus-shadow); }
.tunnel-config-select:active { background: hsl(var(--primary) / 0.18); }
.tunnel-config-select .tunnel-status-dot { color: var(--app-text-muted); }
.tunnel-config-select .tunnel-status-dot.is-running { color: var(--app-toast-success); }
.tunnel-config-name { overflow: hidden; font-size: var(--app-font-body-size); font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
.tunnel-form { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 16px; overflow-y: auto; padding-right: 4px; scrollbar-gutter: stable; }
.tunnel-form-section { display: flex; flex-shrink: 0; flex-direction: column; gap: 10px; }
.tunnel-basic-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.tunnel-connection-fields { display: grid; grid-template-columns: minmax(0, 1fr) 120px; gap: 12px; }
.tunnel-advanced { flex-shrink: 0; padding-top: 12px; border-top: 1px solid var(--app-border-shadow); }
.tunnel-advanced summary { color: var(--app-text-secondary); font-size: var(--app-font-body-size); cursor: pointer; }
.tunnel-advanced summary:focus-visible { outline: 1px solid var(--app-focus-border); outline-offset: 3px; }
.tunnel-advanced-fields { display: flex; flex-direction: column; gap: 12px; margin-top: 12px; }
.tunnel-advanced-fields .management-field { max-width: 240px; }
.tunnel-public-bind { display: flex; align-items: center; gap: 10px; font-size: var(--app-font-body-size); }
@media (max-width: 680px) {
  .tunnel-toolbar-actions { display: grid; width: 100%; grid-template-columns: var(--app-control-height) repeat(2, minmax(0, 1fr)); gap: 6px; }
  .tunnel-toolbar-actions :deep([data-slot="button"]) { min-width: 0; padding-inline: 8px; }
  .tunnel-stop-all { grid-column: 1 / -1; }
  .tunnel-basic-fields { grid-template-columns: minmax(0, 1fr); }
  .tunnel-layout { display: flex; flex-direction: column; overflow-y: auto; }
  .tunnel-sidebar { max-height: 190px; flex-shrink: 0; gap: 8px; padding: 0 0 16px; border-right: 0; border-bottom: 1px solid var(--app-border-shadow); }
  .tunnel-form { flex-shrink: 0; overflow: visible; padding-right: 0; }
  .tunnel-connection-fields { grid-template-columns: minmax(0, 1fr) 100px; gap: 16px 12px; }
}
</style>
