<script setup>
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { computed, onUnmounted, reactive, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand } from '@/utils/ipc';

const LOOPBACK_HOSTS = ['127.0.0.1', 'localhost', '::1'];
const tunnelModeLabelMap = {
  local: '本地转发',
  remote: '远程转发',
  dynamic: '动态代理',
};

const formatTunnelMode = (mode) =>
  tunnelModeLabelMap[String(mode || '').trim().toLowerCase()] || String(mode || '').trim() || '未知';

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

let refreshTimer = null;
let suppressSessionRefresh = false;

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

const selectedSession = computed(() =>
  savedSessions.value.find((session) => session.id === selectedSessionId.value) || null,
);

const sessionConfigs = computed(() =>
  (tunnelConfigs.value || []).filter((config) => config.sessionId === selectedSessionId.value),
);

const visibleTunnels = computed(() =>
  (tunnels.value || []).filter((item) => !selectedSessionId.value || item.sessionId === selectedSessionId.value),
);

const currentConfigTunnels = computed(() =>
  (visibleTunnels.value || []).filter((item) => item.configId === selectedConfigId.value),
);

const runningConfigIds = computed(() => {
  const ids = new Set();
  for (const item of visibleTunnels.value || []) {
    if (item?.configId) ids.add(item.configId);
  }
  return ids;
});

const requiresTarget = computed(() => draft.mode === 'local' || draft.mode === 'remote');

const isPublicBindHost = computed(() => {
  const host = String(draft.listenHost || '').trim().toLowerCase();
  return host && !LOOPBACK_HOSTS.includes(host);
});

const usesPrivilegedListenPort = computed(() => Number(draft.listenPort) > 0 && Number(draft.listenPort) < 1024);
const usesPrivilegedTargetPort = computed(() =>
  requiresTarget.value && Number(draft.targetPort) > 0 && Number(draft.targetPort) < 1024,
);

function getConfigTunnelCount(configId) {
  if (!configId) return 0;
  return (visibleTunnels.value || []).filter((item) => item.configId === configId).length;
}

function isConfigRunning(configId) {
  return runningConfigIds.value.has(configId);
}

function stopRefreshTimer() {
  if (refreshTimer) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

function closeModal() {
  stopRefreshTimer();
  emit('update:visible', false);
}

function hydrateFromConfig(config) {
  selectedConfigId.value = config?.id || '';
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

async function fetchTunnels() {
  loadingTunnels.value = true;
  try {
    tunnels.value = await invokeCommand('list_tunnels');
  } catch (error) {
    toast.error(`读取隧道列表失败: ${error}`);
  } finally {
    loadingTunnels.value = false;
  }
}

async function loadConfigs(preferredConfigId = '') {
  if (!selectedSessionId.value) {
    tunnelConfigs.value = [];
    hydrateFromConfig(null);
    return;
  }

  loadingConfigs.value = true;
  try {
    tunnelConfigs.value = await invokeCommand('list_tunnel_configs', {
      sessionId: selectedSessionId.value,
    });
  } catch (error) {
    toast.error(`读取隧道配置失败: ${error}`);
    tunnelConfigs.value = [];
  } finally {
    loadingConfigs.value = false;
  }

  const nextConfig =
    sessionConfigs.value.find((item) => item.id === preferredConfigId)
    || sessionConfigs.value.find((item) => item.id === selectedConfigId.value)
    || sessionConfigs.value[0]
    || null;

  if (nextConfig) {
    hydrateFromConfig(nextConfig);
  } else {
    selectedConfigId.value = '';
    applyDraft(createEmptyDraft(selectedSessionId.value));
  }
}

async function ensureSelectedSession() {
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

  suppressSessionRefresh = true;
  selectedSessionId.value = nextSessionId;
  applyDraft({ ...draft, sessionId: nextSessionId });
  suppressSessionRefresh = false;
}

function createNewConfig() {
  selectedConfigId.value = '';
  applyDraft(createEmptyDraft(selectedSessionId.value));
}

async function saveCurrentConfig({ silent = false } = {}) {
  if (!validateDraft()) return null;

  saving.value = true;
  try {
    const saved = await invokeCommand('save_tunnel_config', {
      config: normalizePayload(),
    });
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

async function duplicateCurrentConfig() {
  if (!selectedConfigId.value) {
    toast.info('请先选择一个已保存的隧道配置。');
    return;
  }

  try {
    const duplicated = await invokeCommand('duplicate_tunnel_config', { id: selectedConfigId.value });
    await loadConfigs(duplicated.id);
    toast.success('隧道配置已复制');
  } catch (error) {
    toast.error(`复制隧道配置失败: ${error}`);
  }
}

async function deleteCurrentConfig() {
  if (!selectedConfigId.value) {
    toast.info('当前没有可删除的已保存配置。');
    return;
  }

  try {
    await confirm({
      title: '删除隧道配置',
      content: '删除后不会再保留这条持久化配置，已运行的隧道不会自动停止。',
      okText: '删除',
      cancelText: '取消',
      danger: true,
    });
  } catch {
    return;
  }

  try {
    await invokeCommand('delete_tunnel_config', { id: selectedConfigId.value });
    toast.success('隧道配置已删除');
    await loadConfigs();
  } catch (error) {
    toast.error(`删除隧道配置失败: ${error}`);
  }
}

async function startCurrentTunnel() {
  if (!validateDraft()) return;

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

  starting.value = true;
  try {
    await invokeCommand('start_tunnel_from_config', { configId: saved.id });
    toast.success('隧道已启动');
    await fetchTunnels();
    await loadConfigs(saved.id);
  } catch (error) {
    toast.error(`启动隧道失败: ${error}`);
  } finally {
    starting.value = false;
  }
}

async function stopTunnel(id) {
  if (!id) return;
  stopping.value = true;
  try {
    await invokeCommand('stop_tunnel', { id });
    toast.success('隧道已停止');
    await fetchTunnels();
  } catch (error) {
    toast.error(`停止隧道失败: ${error}`);
  } finally {
    stopping.value = false;
  }
}

async function stopCurrentConfigTunnels() {
  if (!currentConfigTunnels.value.length) {
    toast.info('当前配置没有正在运行的隧道。');
    return;
  }

  stopping.value = true;
  try {
    await Promise.all(currentConfigTunnels.value.map((item) => invokeCommand('stop_tunnel', { id: item.id })));
    toast.success('当前配置关联的隧道已停止');
    await fetchTunnels();
  } catch (error) {
    toast.error(`停止当前配置隧道失败: ${error}`);
  } finally {
    stopping.value = false;
  }
}

async function stopAllTunnels() {
  stopping.value = true;
  try {
    await invokeCommand('stop_all_tunnels');
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
  await sshStore.loadSavedSessions();
  await ensureSelectedSession();
  await loadConfigs();
  await fetchTunnels();

  stopRefreshTimer();
  refreshTimer = setInterval(() => {
    fetchTunnels();
  }, 3000);
}

watch(
  () => props.visible,
  async (visible) => {
    if (visible) {
      await openModal();
    } else {
      stopRefreshTimer();
    }
  },
);

watch(selectedSessionId, async (nextSessionId, previousSessionId) => {
  if (!props.visible || suppressSessionRefresh || nextSessionId === previousSessionId) return;
  selectedConfigId.value = '';
  applyDraft(createEmptyDraft(nextSessionId));
  await loadConfigs();
});

watch(
  () => props.preferredSessionId,
  (nextSessionId) => {
    if (!props.visible || !nextSessionId || !savedSessions.value.some((item) => item.id === nextSessionId)) return;
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
  stopRefreshTimer();
});
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent :show-close-button="false"
      class="flex h-[min(640px,calc(100vh-4rem))] max-h-[calc(100vh-4rem)] w-[860px] max-w-[92vw] flex-col overflow-hidden sm:max-w-[92vw]">
      <DialogHeader>
        <DialogTitle>隧道管理</DialogTitle>
        <!-- <p class="text-xs text-muted-foreground">
          隧道配置按会话持久化保存，可在未连接会话时维护；启动时的连接校验以后端结果为准。
        </p> -->
      </DialogHeader>

      <div class="flex min-h-0 flex-1 overflow-hidden">
        <div class="flex w-[280px] shrink-0 flex-col border-r border-border">
          <div class="px-4 pb-3 pt-1">
            <div class="mb-2 text-xs font-medium text-muted-foreground">会话</div>
            <Select v-model="selectedSessionId">
              <SelectTrigger size="sm" class="w-full">
                <SelectValue placeholder="选择已保存的 SSH 会话" />
              </SelectTrigger>
              <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                <SelectItem v-for="session in savedSessions" :key="session.id" :value="session.id">
                  {{ session.name || `${session.username || 'user'}@${session.host || 'host'}` }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="flex items-center gap-2 px-4 pb-3">
            <Button size="sm" variant="outline" @click="createNewConfig">新建</Button>
            <Button size="sm" variant="outline" :disabled="!selectedConfigId" @click="duplicateCurrentConfig">复制</Button>
            <Button size="sm" variant="ghost" :disabled="!selectedConfigId" @click="deleteCurrentConfig">删除</Button>
          </div>

          <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
            <div v-if="!savedSessions.length" class="px-3 py-4 text-xs text-muted-foreground">
              还没有已保存的 SSH 会话。
            </div>
            <div v-else-if="loadingConfigs" class="px-3 py-4 text-xs text-muted-foreground">
              正在读取隧道配置...
            </div>
            <div v-else-if="sessionConfigs.length === 0" class="px-3 py-4 text-xs text-muted-foreground">
              当前会话还没有持久化隧道配置。
            </div>
            <div v-else class="space-y-2">
              <button
                v-for="config in sessionConfigs"
                :key="config.id"
                type="button"
                :class="[
                  'w-full rounded-md border px-3 py-2 text-left transition-colors',
                  selectedConfigId === config.id
                    ? 'border-primary bg-primary/10'
                    : 'border-border bg-background hover:bg-muted/50',
                ]"
                @click="hydrateFromConfig(config)"
              >
                <div class="flex items-center justify-between gap-2">
                  <span class="truncate text-sm font-medium">{{ buildConfigLabel(config) }}</span>
                  <div class="flex shrink-0 items-center gap-1.5">
                    <span
                      :class="[
                        'rounded-sm px-1.5 py-0.5 text-[10px] font-medium',
                        isConfigRunning(config.id)
                          ? 'bg-emerald-500/15 text-emerald-300'
                          : 'bg-muted text-muted-foreground',
                      ]"
                    >
                      {{ isConfigRunning(config.id) ? `运行中 ${getConfigTunnelCount(config.id)}` : '未运行' }}
                    </span>
                    <span class="rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                      {{ formatTunnelMode(config.mode) }}
                    </span>
                  </div>
                </div>
                <div class="mt-1 truncate text-[11px] text-muted-foreground">
                  {{ config.listenHost }}:{{ config.listenPort }}
                  <template v-if="config.mode !== 'dynamic'">
                    -> {{ config.targetHost }}:{{ config.targetPort }}
                  </template>
                </div>
              </button>
            </div>
          </div>
        </div>

        <div class="min-w-0 flex-1 overflow-y-auto px-5 py-3">
          <div class="grid grid-cols-[120px_minmax(0,1fr)] items-center gap-x-3 gap-y-3">
            <div class="text-right text-sm text-muted-foreground">会话</div>
            <div class="text-sm text-foreground">
              {{ selectedSession?.name || (selectedSession ? `${selectedSession.username}@${selectedSession.host}` : '未选择') }}
            </div>

            <div class="text-right text-sm text-muted-foreground">名称</div>
            <Input v-model="draft.name" size="sm" placeholder="如：postgres-dev" />

            <div class="text-right text-sm text-muted-foreground">类型</div>
            <Select v-model="draft.mode">
              <SelectTrigger size="sm" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                <SelectItem value="local">本地转发</SelectItem>
                <SelectItem value="remote">远程转发</SelectItem>
                <SelectItem value="dynamic">动态代理</SelectItem>
              </SelectContent>
            </Select>

            <div class="text-right text-sm text-muted-foreground">监听地址</div>
            <Input v-model="draft.listenHost" size="sm" placeholder="127.0.0.1" />

            <div class="text-right text-sm text-muted-foreground">监听端口</div>
            <Input v-model.number="draft.listenPort" type="text" inputmode="numeric" autocomplete="off" size="sm" />

            <template v-if="requiresTarget">
              <div class="text-right text-sm text-muted-foreground">目标主机</div>
              <Input v-model="draft.targetHost" size="sm" placeholder="127.0.0.1" />

              <div class="text-right text-sm text-muted-foreground">目标端口</div>
              <Input v-model.number="draft.targetPort" type="text" inputmode="numeric" autocomplete="off" size="sm" />
            </template>

            <div class="text-right text-sm text-muted-foreground">保活间隔</div>
            <Input v-model.number="draft.serverAliveInterval" type="text" inputmode="numeric" autocomplete="off" size="sm" />

            <div class="text-right text-sm text-muted-foreground">风险策略</div>
            <label class="flex items-center gap-2 text-sm">
              <Checkbox
                :model-value="draft.allowPublicBind"
                @update:model-value="(value) => { draft.allowPublicBind = !!value; }"
              />
              <span>允许公网监听（高风险）</span>
            </label>
          </div>

          <div class="mt-5 border-t border-border pt-4">
            <div class="mb-2 flex items-center justify-between gap-3">
              <div class="text-xs font-semibold">运行中的隧道</div>
              <div class="text-[11px] text-muted-foreground">
                {{ loadingTunnels ? '刷新中...' : `${visibleTunnels.length} 个` }}
              </div>
            </div>

            <div v-if="visibleTunnels.length === 0" class="rounded-md border border-dashed border-border px-3 py-5 text-center text-xs text-muted-foreground">
              当前会话暂无运行中的隧道。
            </div>

            <div v-else class="space-y-2">
              <div
                v-for="item in visibleTunnels"
                :key="item.id"
                class="flex items-start justify-between gap-3 rounded-md border border-border bg-muted/20 px-3 py-3"
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="truncate text-sm font-medium">{{ item.name || '未命名隧道' }}</span>
                    <span class="rounded-sm bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                      {{ formatTunnelMode(item.mode) }}
                    </span>
                  </div>
                  <div class="mt-1 truncate text-xs text-muted-foreground">
                    {{ item.listenHost }}:{{ item.listenPort }}
                    <template v-if="item.mode !== 'dynamic'">
                      -> {{ item.targetHost }}:{{ item.targetPort }}
                    </template>
                  </div>
                  <div class="mt-1 truncate text-[11px] text-muted-foreground">
                    {{ item.username }}@{{ item.host }}:{{ item.port }}
                  </div>
                </div>
                <div class="flex shrink-0 items-center gap-1">
                  <Button size="sm" variant="outline" @click="copyProxyAddress(item)">复制地址</Button>
                  <Button size="sm" variant="outline" @click="copyCommandPreview(item)">复制命令</Button>
                  <Button size="sm" variant="destructive" @click="stopTunnel(item.id)">停止</Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button size="sm" variant="ghost" @click="closeModal">关闭</Button>
        <Button size="sm" variant="outline" :disabled="!selectedSessionId || saving" @click="saveCurrentConfig()">保存配置</Button>
        <Button size="sm" variant="outline" :disabled="!currentConfigTunnels.length || stopping" @click="stopCurrentConfigTunnels">
          停止当前配置
        </Button>
        <Button size="sm" variant="outline" :disabled="loadingTunnels" @click="fetchTunnels">刷新状态</Button>
        <Button size="sm" variant="destructive" :disabled="!tunnels.length || stopping" @click="stopAllTunnels">停止全部</Button>
        <Button size="sm" :disabled="starting || !selectedSessionId" @click="startCurrentTunnel">启动隧道</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
