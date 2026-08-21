<script setup>
import IconButton from '@/components/common/IconButton.vue';
import { TooltipHint } from '@/components/ui/tooltip';
import { toast } from '@/composables/useToast';
import { useSshStore } from '@/stores/ssh';
import { useTransfersStore } from '@/stores/transfers';
import { invokeCommand } from '@/utils/ipc';
import {
  ACTIVE_TRANSFER_STATUSES,
  buildRemoteDirectoryCommand,
  COMPACT_TRANSFER_STATUSES,
  filterTransferItems,
  isActiveTransfer,
  isClearableTransfer,
  resolveTransferLocateTarget,
} from '@/utils/transferPanel';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { ChevronDown, ChevronUp, FolderOpen, LoaderCircle, Pause, Trash2, X } from '@lucide/vue';
import { storeToRefs } from 'pinia';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

defineProps({ expanded: Boolean });
const emit = defineEmits(['toggle', 'close']);

const transferStore = useTransfersStore();
const sshStore = useSshStore();
const { dockStatus: status } = storeToRefs(transferStore);
const activeFilter = ref('all');
const locatingTaskKey = ref('');
const clock = ref(typeof performance !== 'undefined' ? performance.now() : Date.now());
let clockTimer = null;
let stopClockWatch = null;

const needsLiveClock = () => status.value.items.some((item) => (
  (Number.isFinite(item.transferStartedAt) && !Number.isFinite(item.transferElapsedMs))
  || (Number.isFinite(item.finalizingStartedAt) && !Number.isFinite(item.finalizingElapsedMs))
));

const stopClock = () => {
  if (clockTimer === null) return;
  window.clearInterval(clockTimer);
  clockTimer = null;
};

onMounted(() => {
  stopClockWatch = watch(needsLiveClock, (active) => {
    stopClock();
    if (!active) return;
    clock.value = typeof performance !== 'undefined' ? performance.now() : Date.now();
    clockTimer = window.setInterval(() => {
      clock.value = typeof performance !== 'undefined' ? performance.now() : Date.now();
    }, 250);
  }, { immediate: true });
});

onUnmounted(() => {
  stopClockWatch?.();
  stopClock();
});

const statusOrder = {
  uploading: 0,
  transferring: 0,
  finalizing: 1,
  negotiating: 2,
  waiting: 3,
  cancelling: 4,
  failed: 5,
  success: 6,
  cancelled: 7,
  skipped: 8,
};

const orderedItems = computed(() => [...status.value.items].sort((a, b) => (
  (statusOrder[a.status] ?? 9) - (statusOrder[b.status] ?? 9)
)));
const activeItems = computed(() => orderedItems.value.filter(isActiveTransfer));
const failedItems = computed(() => orderedItems.value.filter((item) => item.status === 'failed'));
const clearableItems = computed(() => orderedItems.value.filter(isClearableTransfer));
const visibleItems = computed(() => filterTransferItems(orderedItems.value, activeFilter.value));

const formatSize = (bytes) => {
  const value = Number(bytes || 0);
  if (value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(units.length - 1, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / (1024 ** index)).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};
const formatRate = (bytes) => `${formatSize(bytes)}/s`;
const formatEta = (seconds) => {
  if (!Number.isFinite(seconds)) return '--';
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours) return `${hours}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`;
  return minutes ? `${minutes}:${String(rest).padStart(2, '0')}` : `${rest}s`;
};
const formatDuration = (milliseconds) => {
  if (!Number.isFinite(milliseconds)) return '--';
  const seconds = Math.max(0, milliseconds) / 1000;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  return formatEta(seconds);
};
const transferElapsed = (item) => {
  if (Number.isFinite(item.transferElapsedMs)) return item.transferElapsedMs;
  if (Number.isFinite(item.transferStartedAt)) return Math.max(0, clock.value - item.transferStartedAt);
  return null;
};
const finalizingElapsed = (item) => {
  if (Number.isFinite(item.finalizingElapsedMs)) return item.finalizingElapsedMs;
  if (Number.isFinite(item.finalizingStartedAt)) return Math.max(0, clock.value - item.finalizingStartedAt);
  return null;
};
const formatProgressSize = (item) => (
  `${formatSize(item.loaded)} / ${item.total > 0 ? formatSize(item.total) : '--'}`
);

const sessionFor = (item) => (
  sshStore.getSession(item.sessionId) || sshStore.getSession(item.workspaceSessionId)
);
const sessionName = (item) => {
  const session = sessionFor(item);
  return session?.name || session?.config?.name || session?.config?.host || '当前会话';
};
const transferRoute = (item) => {
  const source = item.direction === 'download' ? item.remotePath : item.localPath;
  const target = item.direction === 'download' ? item.localPath : item.remotePath;
  if (!source && !target) return '';
  return `${source || '未知路径'} → ${target || '未知路径'}`;
};
const taskDetail = (item) => [
  item.name,
  `${sessionName(item)} · ${item.protocol === 'zmodem' ? 'ZMODEM' : 'SFTP'}`,
  transferRoute(item),
].filter(Boolean).join('\n');

const statusLabel = (item) => ({
  waiting: '等待中',
  negotiating: '协商中',
  uploading: item.direction === 'download' ? '下载中' : '上传中',
  transferring: item.direction === 'download' ? '下载中' : '上传中',
  finalizing: '正在完成',
  cancelling: '正在取消',
  paused: '已暂停',
  success: '已完成',
  failed: '失败',
  cancelled: '已取消',
  skipped: '已跳过',
}[item.status] || item.status);
const displayStatusLabel = (item) => (
  item.protocol === 'zmodem'
    && !item.terminalRestored
    && !ACTIVE_TRANSFER_STATUSES.has(item.status)
    ? '正在恢复终端'
    : statusLabel(item)
);

const secondaryText = (item) => {
  if (item.status === 'failed') return item.error || '传输失败';
  if (item.status === 'success') {
    const parts = [formatSize(item.total || item.loaded)];
    const transferMs = transferElapsed(item);
    const finalizingMs = finalizingElapsed(item);
    if (Number.isFinite(transferMs)) parts.push(`传输 ${formatDuration(transferMs)}`);
    if (Number.isFinite(finalizingMs)) parts.push(`完成 ${formatDuration(finalizingMs)}`);
    return parts.join(' · ');
  }
  if (item.status === 'cancelled' || item.status === 'skipped') {
    return item.error || statusLabel(item);
  }
  if (item.status === 'waiting') return '等待传输';
  if (item.status === 'negotiating') return '正在与远端协商';

  const parts = [formatProgressSize(item)];
  const transferMs = transferElapsed(item);
  const finalizingMs = finalizingElapsed(item);
  if (item.status === 'finalizing') {
    if (Number.isFinite(transferMs)) parts.push(`传输 ${formatDuration(transferMs)}`);
    if (Number.isFinite(finalizingMs)) parts.push(`完成中 ${formatDuration(finalizingMs)}`);
    return parts.join(' · ');
  }
  if (item.status !== 'paused' && Number(item.rate || 0) > 0) {
    parts.push(formatRate(item.rate));
  }
  if (Number.isFinite(transferMs)) parts.push(`已传输 ${formatDuration(transferMs)}`);
  if (item.status !== 'paused' && Number.isFinite(item.etaSeconds)) {
    parts.push(`剩余 ${formatEta(item.etaSeconds)}`);
  }
  return parts.join(' · ');
};

const progressPercent = (item) => Math.max(0, Math.min(100, Number(item.progress || 0)));
const showsProgress = (item) => (
  ['uploading', 'transferring', 'finalizing', 'cancelling', 'paused'].includes(item.status)
);
const isCompact = (item) => COMPACT_TRANSFER_STATUSES.has(item.status);
const canCancel = (item) => (
  ACTIVE_TRANSFER_STATUSES.has(item.status) && item.status !== 'cancelling' && item.status !== 'paused'
);
const showsPausePlaceholder = (item) => ['uploading', 'transferring'].includes(item.status);
const canRemove = (item) => !isActiveTransfer(item)
  && (item.protocol !== 'zmodem' || item.terminalRestored);

const taskKey = (item) => `${item.sessionId}:${item.id}`;
const locateTarget = (item) => resolveTransferLocateTarget(item);
const hasLocateAction = (item) => !!locateTarget(item);
const isLocating = (item) => locatingTaskKey.value === taskKey(item);
const canLocate = (item) => {
  const target = locateTarget(item);
  if (!target || isLocating(item)) return false;
  if (target.kind === 'local') return true;
  return sessionFor(item)?.status === 'connected';
};
const locateLabel = (item) => (
  item.direction === 'download' ? '在资源管理器中显示' : '进入远端目录'
);

const cancel = async (item) => {
  if (!item.sessionId) return;
  const previousStatus = item.status;
  const mode = transferStore.requestCancel(item.sessionId, item.id);
  if (mode !== 'remote') return;
  try {
    if (item.protocol === 'zmodem') {
      await invokeCommand('cancel_terminal_transfer', {
        workspaceSessionId: item.workspaceSessionId,
        channelId: item.channelId ?? null,
        operationId: item.operationId,
      });
    } else {
      await invokeCommand('sftp_cancel_transfer', { sessionId: item.sessionId, reqId: item.id });
    }
  } catch (error) {
    const task = transferStore.findTask(item.sessionId, item.id);
    if (task?.status === 'cancelling') {
      task.status = previousStatus;
      task.error = String(error || '取消传输失败');
    }
  }
};

const locate = async (item) => {
  const target = locateTarget(item);
  if (!target || !canLocate(item)) return;
  locatingTaskKey.value = taskKey(item);
  try {
    if (target.kind === 'local') {
      await revealItemInDir(target.path);
      return;
    }

    const session = sessionFor(item);
    const terminalSessionId = session?.id || item.sessionId;
    const workspaceSessionId = session?.isSplitChild
      ? (session.workspaceSessionId || session.parentId)
      : (item.workspaceSessionId || terminalSessionId);
    const data = buildRemoteDirectoryCommand(target.directory);

    sshStore.activeSessionId = workspaceSessionId;
    if (session?.isSplitChild) {
      await invokeCommand('write_ssh_shell_channel', {
        rootSessionId: workspaceSessionId,
        channelId: terminalSessionId,
        data,
      });
    } else {
      await invokeCommand('write_ssh', { sessionId: terminalSessionId, data });
    }

    await nextTick();
    window.dispatchEvent(new CustomEvent('terminal:focus', {
      detail: { sessionId: terminalSessionId },
    }));
  } catch (error) {
    toast.error(`定位传输文件失败：${String(error)}`);
  } finally {
    if (locatingTaskKey.value === taskKey(item)) locatingTaskKey.value = '';
  }
};

const clear = (item) => transferStore.removeTask(item.sessionId, item.id);
const clearFinished = () => transferStore.clearFinishedTasks();
</script>

<template>
  <section class="transfer-panel" :class="{ expanded }" aria-label="传输列表">
    <header class="transfer-header">
      <button type="button" class="transfer-title" @click="emit('toggle')">
        <span>传输列表</span>
        <ChevronDown v-if="expanded" :size="16" />
        <ChevronUp v-else :size="16" />
      </button>
      <IconButton class="header-action" :icon="X" size="28px" aria-label="关闭传输列表"
        tooltip-side="top" :action="() => emit('close')" />
    </header>

    <div v-if="expanded" class="transfer-body">
      <div class="transfer-toolbar">
        <div class="transfer-filters" aria-label="筛选传输任务">
          <button type="button" :class="{ active: activeFilter === 'all' }" @click="activeFilter = 'all'">
            全部 {{ status.total }}
          </button>
          <span>·</span>
          <button type="button" :class="{ active: activeFilter === 'active' }" @click="activeFilter = 'active'">
            进行中 {{ activeItems.length }}
          </button>
          <span>·</span>
          <button type="button" :class="{ active: activeFilter === 'failed' }" @click="activeFilter = 'failed'">
            失败 {{ failedItems.length }}
          </button>
        </div>
        <IconButton class="clear-finished" :icon="Trash2" size="28px" aria-label="清除已完成"
          tooltip-side="top" :disabled="clearableItems.length === 0" :action="clearFinished" />
      </div>

      <div v-if="visibleItems.length" class="transfer-list">
        <article v-for="item in visibleItems" :key="`${item.sessionId}:${item.id}`" class="transfer-task"
          :class="[
            `status-${item.status}`,
            { 'is-active': isActiveTransfer(item), 'is-compact': isCompact(item), 'is-failed': item.status === 'failed' }
          ]">
          <TooltipHint :text="item.direction === 'download' ? '下载' : '上传'">
            <div class="transfer-direction">
              {{ item.direction === 'download' ? '↓' : '↑' }}
            </div>
          </TooltipHint>

          <div class="transfer-task-main">
            <div class="transfer-task-heading">
              <TooltipHint :text="taskDetail(item)">
                <strong>{{ item.name }}</strong>
              </TooltipHint>
            </div>
            <TooltipHint :text="item.status === 'failed' ? secondaryText(item) : taskDetail(item)">
              <div class="transfer-task-meta" :class="{ error: item.status === 'failed' }">
                {{ secondaryText(item) }}
              </div>
            </TooltipHint>
            <div v-if="showsProgress(item)" class="transfer-progress-row">
              <div class="transfer-progress">
                <i :style="{ width: `${progressPercent(item)}%` }" />
              </div>
              <span>{{ Math.round(progressPercent(item)) }}%</span>
            </div>
          </div>

          <div class="transfer-task-side">
            <span class="transfer-state">{{ displayStatusLabel(item) }}</span>
            <div class="transfer-actions">
              <TooltipHint v-if="showsPausePlaceholder(item)" text="暂不支持暂停">
                <span class="disabled-action">
                  <IconButton :icon="Pause" size="28px" aria-label="暂不支持暂停" :tooltip="false" disabled />
                </span>
              </TooltipHint>
              <IconButton v-if="canCancel(item)" :icon="X" size="28px" aria-label="取消传输"
                tooltip-side="top" :action="() => cancel(item)" />
              <TooltipHint v-else-if="item.status === 'cancelling'" text="正在取消">
                <span class="pending-action">
                  <LoaderCircle :size="16" />
                </span>
              </TooltipHint>
              <TooltipHint v-if="hasLocateAction(item) && !canLocate(item) && !isLocating(item)"
                text="对应会话未连接">
                <span class="locate-action" :class="{ locating: isLocating(item) }">
                  <IconButton :icon="isLocating(item) ? LoaderCircle : FolderOpen" size="28px"
                    :aria-label="isLocating(item) ? '正在定位' : locateLabel(item)" tooltip-side="top"
                    :tooltip="canLocate(item)" :disabled="!canLocate(item)" :action="() => locate(item)" />
                </span>
              </TooltipHint>
              <span v-else-if="hasLocateAction(item)" class="locate-action"
                :class="{ locating: isLocating(item) }">
                <IconButton :icon="isLocating(item) ? LoaderCircle : FolderOpen" size="28px"
                  :aria-label="isLocating(item) ? '正在定位' : locateLabel(item)" tooltip-side="top"
                  :tooltip="canLocate(item)" :disabled="!canLocate(item)" :action="() => locate(item)" />
              </span>
              <IconButton v-if="canRemove(item)" :icon="Trash2" size="28px" aria-label="删除记录"
                tooltip-side="top" :action="() => clear(item)" />
            </div>
          </div>
        </article>
      </div>
      <div v-else class="transfer-empty">
        {{ status.total ? '当前筛选下没有任务' : '暂无传输任务' }}
      </div>
    </div>
  </section>
</template>

<style scoped>
.transfer-panel {
  flex: 0 0 40px;
  min-height: 40px;
  margin: 0 4px 4px;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--app-border-shadow) 72%, transparent);
  border-radius: 9px;
  background: color-mix(in srgb, var(--app-bg-dialog) 92%, transparent);
  box-shadow: var(--niri-shadow-panel);
  backdrop-filter: blur(12px);
  transition: flex-basis var(--app-motion-panel, 160ms ease);
}

.transfer-panel.expanded {
  flex-basis: min(360px, 42vh);
}

.transfer-header {
  display: flex;
  height: 38px;
  align-items: center;
  border-bottom: 1px solid transparent;
}

.expanded .transfer-header {
  border-bottom-color: color-mix(in srgb, var(--app-border-shadow) 62%, transparent);
}

.transfer-title {
  display: flex;
  min-width: 0;
  flex: 1;
  height: 100%;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  border: 0;
  color: var(--app-text);
  background: transparent;
  font: inherit;
  font-size: 13px;
  font-weight: 700;
  text-align: left;
}

.transfer-title:hover {
  background: color-mix(in srgb, var(--app-text) 5%, transparent);
}

.header-action {
  margin-right: 5px;
}

.transfer-body {
  display: flex;
  height: calc(100% - 38px);
  min-height: 0;
  flex-direction: column;
}

.transfer-toolbar {
  display: flex;
  flex: 0 0 36px;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-shadow) 52%, transparent);
}

.transfer-filters {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
  color: var(--app-text-muted);
  font-size: 11px;
}

.transfer-filters button {
  padding: 0;
  border: 0;
  color: var(--app-text-muted);
  background: transparent;
  font: inherit;
  white-space: nowrap;
}

.transfer-filters button:hover {
  color: var(--app-text-secondary);
}

.transfer-filters button.active {
  color: var(--app-text);
  font-weight: 700;
}

.transfer-filters > span {
  opacity: .45;
}

.clear-finished.disabled {
  opacity: .28;
}

.transfer-list {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding: 0 10px 6px;
}

.transfer-task {
  display: grid;
  min-height: 80px;
  grid-template-columns: 28px minmax(0, 7fr) minmax(150px, 3fr);
  align-items: center;
  column-gap: 10px;
  padding: 8px 8px 7px;
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-shadow) 50%, transparent);
  transition: background-color 120ms ease;
}

.transfer-task:hover {
  background: color-mix(in srgb, var(--app-text) 4%, transparent);
}

.transfer-task.is-compact {
  min-height: 56px;
  padding-top: 5px;
  padding-bottom: 5px;
  color: var(--app-text-secondary);
}

.transfer-task.is-failed {
  min-height: 62px;
}

.transfer-direction {
  align-self: start;
  padding-top: 5px;
  color: var(--color-primary);
  font-size: 21px;
  line-height: 1;
  text-align: center;
}

.is-compact .transfer-direction,
.status-cancelled .transfer-direction,
.status-skipped .transfer-direction {
  color: var(--app-text-muted);
}

.transfer-task-main {
  min-width: 0;
}

.transfer-task-heading {
  display: flex;
  min-width: 0;
  align-items: center;
}

.transfer-task-heading strong {
  min-width: 0;
  overflow: hidden;
  color: var(--app-text);
  font-size: 12px;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.is-compact .transfer-task-heading strong {
  color: var(--app-text-secondary);
}

.transfer-task-meta {
  min-width: 0;
  margin-top: 4px;
  overflow: hidden;
  color: var(--app-text-muted);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transfer-task-meta.error {
  color: color-mix(in srgb, var(--color-danger) 78%, var(--app-text-muted));
}

.transfer-progress-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 34px;
  align-items: center;
  gap: 8px;
  margin-top: 7px;
}

.transfer-progress-row > span {
  color: var(--app-text-muted);
  font-size: 10px;
  text-align: right;
}

.transfer-progress {
  height: 3px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--app-text) 9%, transparent);
}

.transfer-progress i {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-primary);
}

.transfer-task-side {
  display: grid;
  min-width: 0;
  grid-template-columns: minmax(62px, 1fr) auto;
  align-items: center;
  gap: 8px;
}

.transfer-state {
  color: var(--app-text-secondary);
  font-size: 11px;
  text-align: right;
  white-space: nowrap;
}

.status-uploading .transfer-state,
.status-transferring .transfer-state,
.status-finalizing .transfer-state,
.status-negotiating .transfer-state {
  color: var(--color-primary);
}

.status-success .transfer-state {
  color: var(--app-status-success, var(--color-primary));
}

.status-failed .transfer-state {
  color: color-mix(in srgb, var(--color-danger) 78%, var(--app-text-secondary));
}

.transfer-actions {
  display: flex;
  min-width: 28px;
  align-items: center;
  justify-content: flex-end;
  gap: 3px;
  transition: opacity 120ms ease;
}

.is-compact .transfer-actions {
  opacity: .48;
}

.is-compact:hover .transfer-actions,
.is-compact:focus-within .transfer-actions {
  opacity: 1;
}

.disabled-action {
  display: inline-flex;
  opacity: .3;
}

.pending-action {
  display: inline-grid;
  width: 28px;
  height: 28px;
  place-items: center;
  color: var(--app-text-muted);
}

.pending-action svg {
  animation: transfer-spin 900ms linear infinite;
}

.locate-action {
  display: inline-flex;
}

.locate-action.locating :deep(svg) {
  animation: transfer-spin 900ms linear infinite;
}

.transfer-actions :deep(.icon-button),
:deep(.icon-button.header-action),
:deep(.icon-button.clear-finished) {
  color: var(--app-text-secondary);
  background: transparent;
}

.transfer-actions :deep(.icon-button:hover),
:deep(.icon-button.header-action:hover),
:deep(.icon-button.clear-finished:hover) {
  color: var(--app-text);
}

.transfer-empty {
  display: grid;
  flex: 1;
  place-items: center;
  color: var(--app-text-muted);
  font-size: 11px;
}

@keyframes transfer-spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 700px) {
  .transfer-task {
    grid-template-columns: 24px minmax(0, 1fr) minmax(124px, 34%);
    column-gap: 6px;
    padding-right: 4px;
    padding-left: 4px;
  }

  .transfer-task-side {
    grid-template-columns: minmax(54px, 1fr) auto;
    gap: 4px;
  }

  .transfer-actions {
    gap: 0;
  }
}
</style>
