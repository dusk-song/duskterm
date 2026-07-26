<script setup>
import { ChevronDown, ChevronUp, CircleCheck, ListChecks, X } from '@lucide/vue';
import { storeToRefs } from 'pinia';
import { computed } from 'vue';
import { useSftpTransfersStore } from '@/stores/sftpTransfers';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand } from '@/utils/ipc';

defineProps({ expanded: Boolean });
const emit = defineEmits(['toggle', 'close']);

const transferStore = useSftpTransfersStore();
const sshStore = useSshStore();
const { dockStatus: status } = storeToRefs(transferStore);

const statusOrder = {
  uploading: 0,
  waiting: 1,
  cancelling: 2,
  failed: 3,
  cancelled: 4,
  success: 5,
};

const orderedItems = computed(() => [...status.value.items].sort((a, b) => (
  (statusOrder[a.status] ?? 9) - (statusOrder[b.status] ?? 9)
)));
const activeItems = computed(() => orderedItems.value.filter((item) => (
  item.status === 'uploading' || item.status === 'waiting' || item.status === 'cancelling'
)));
const failedItems = computed(() => orderedItems.value.filter((item) => item.status === 'failed'));
const summaryItem = computed(() => failedItems.value[0] || activeItems.value[0] || orderedItems.value[0] || null);
const totalRate = computed(() => activeItems.value.reduce((sum, item) => sum + Number(item.rate || 0), 0));

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
  const minutes = Math.floor(total / 60);
  const rest = total % 60;
  return minutes ? `${minutes}:${String(rest).padStart(2, '0')}` : `${rest}s`;
};
const sessionName = (item) => {
  const session = sshStore.getSession(item.sessionId);
  return session?.name || session?.config?.name || session?.config?.host || '当前会话';
};
const transferRoute = (item) => {
  const source = item.direction === 'download' ? item.remotePath : item.localPath;
  const target = item.direction === 'download' ? item.localPath : item.remotePath;
  if (!source && !target) return '';
  return `${source || '未知路径'} → ${target || '未知路径'}`;
};
const statusLabel = (item) => ({
  waiting: '等待中',
  uploading: item.direction === 'download' ? '下载中' : '上传中',
  cancelling: '正在取消',
  success: '已完成',
  failed: '失败',
  cancelled: '已取消',
}[item.status] || item.status);

const summaryText = computed(() => {
  if (!summaryItem.value) return '暂无传输任务';
  if (failedItems.value.length) return `${failedItems.value.length} 个传输失败 · ${summaryItem.value.name}`;
  if (activeItems.value.length) {
    return `${activeItems.value.length} 个进行中 · ${summaryItem.value.name} · ${Math.round(summaryItem.value.progress || 0)}%`;
  }
  return `${status.value.total} 个任务已完成`;
});

const cancel = async (item) => {
  if (!item.sessionId) return;
  const mode = transferStore.requestCancel(item.sessionId, item.id);
  if (mode !== 'remote') return;
  try {
    await invokeCommand('sftp_cancel_transfer', { sessionId: item.sessionId, reqId: item.id });
  } catch (error) {
    const task = transferStore.findTask(item.sessionId, item.id);
    if (task?.status === 'cancelling') {
      task.status = 'uploading';
      task.error = String(error || '取消传输失败');
    }
  }
};

const clear = (item) => transferStore.removeTask(item.sessionId, item.id);
const clearFinished = () => {
  orderedItems.value
    .filter((item) => !['uploading', 'waiting', 'cancelling'].includes(item.status))
    .forEach(clear);
};
</script>

<template>
  <section class="transfer-panel" :class="{ expanded }" aria-label="传输列表">
    <div class="transfer-header">
      <button type="button" class="transfer-summary" @click="emit('toggle')">
        <ListChecks :size="14" />
        <span class="transfer-summary-title">传输</span>
        <span v-if="status.active" class="transfer-count">{{ status.active }}</span>
        <span class="transfer-summary-text" :class="{ error: failedItems.length }">{{ summaryText }}</span>
        <span v-if="totalRate" class="transfer-summary-rate">{{ formatRate(totalRate) }}</span>
        <ChevronDown v-if="expanded" :size="14" />
        <ChevronUp v-else :size="14" />
      </button>
      <button type="button" class="transfer-close" title="关闭传输列表" aria-label="关闭传输列表"
        @click="emit('close')">
        <X :size="14" />
      </button>
    </div>

    <div v-if="expanded" class="transfer-body">
      <div class="transfer-body-toolbar">
        <span>全部 {{ status.total }} · 进行中 {{ status.active }} · 失败 {{ failedItems.length }}</span>
        <button type="button" :disabled="status.total === status.active" @click="clearFinished">清除已结束</button>
      </div>

      <div v-if="orderedItems.length" class="transfer-list">
        <article v-for="item in orderedItems" :key="`${item.sessionId}:${item.id}`" class="transfer-task"
          :class="`status-${item.status}`">
          <div class="transfer-direction">{{ item.direction === 'download' ? '↓' : '↑' }}</div>
          <div class="transfer-task-main">
            <div class="transfer-task-heading">
              <strong :title="item.name">{{ item.name }}</strong>
              <span>{{ statusLabel(item) }}</span>
            </div>
            <div class="transfer-task-meta">
              <span>{{ sessionName(item) }}</span>
              <span v-if="transferRoute(item)" class="transfer-route" :title="transferRoute(item)">{{ transferRoute(item) }}</span>
              <span>{{ formatSize(item.loaded) }} / {{ formatSize(item.total) }}</span>
              <span v-if="item.status === 'uploading'">{{ formatRate(item.rate) }} · 剩余 {{ formatEta(item.etaSeconds) }}</span>
              <span v-else-if="item.error" class="task-error" :title="item.error">{{ item.error }}</span>
            </div>
            <div class="transfer-progress">
              <i :style="{ width: `${Math.max(0, Math.min(100, item.progress || 0))}%` }" />
            </div>
          </div>
          <button v-if="item.status === 'uploading' || item.status === 'waiting'" type="button"
            class="transfer-action" title="取消" @click="cancel(item)"><X :size="14" /></button>
          <span v-else-if="item.status === 'cancelling'" class="transfer-action pending">…</span>
          <button v-else type="button" class="transfer-action" title="清除" @click="clear(item)">
            <CircleCheck v-if="item.status === 'success'" :size="14" />
            <X v-else :size="14" />
          </button>
        </article>
      </div>
      <div v-else class="transfer-empty">暂无传输任务</div>
    </div>
  </section>
</template>

<style scoped>
.transfer-panel {
  flex: 0 0 34px;
  min-height: 34px;
  margin: 0 4px 4px;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--app-border-shadow) 72%, transparent);
  border-radius: 9px;
  background: color-mix(in srgb, var(--app-bg-dialog) 88%, transparent);
  box-shadow: var(--niri-shadow-panel);
  backdrop-filter: blur(12px);
  transition: flex-basis var(--app-motion-panel, 160ms ease);
}

.transfer-panel.expanded {
  flex-basis: min(240px, 34vh);
}

.transfer-header {
  display: flex;
  height: 32px;
  align-items: stretch;
}

.transfer-summary {
  display: flex;
  min-width: 0;
  flex: 1;
  height: 32px;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border: 0;
  color: var(--app-text);
  background: transparent;
  font: inherit;
  text-align: left;
}

.transfer-summary:hover {
  background: color-mix(in srgb, var(--app-text) 6%, transparent);
}

.transfer-close {
  display: inline-flex;
  width: 32px;
  flex: 0 0 32px;
  align-items: center;
  justify-content: center;
  border: 0;
  color: var(--app-text-secondary);
  background: transparent;
}

.transfer-close:hover {
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
}

.transfer-summary-title {
  font-weight: 700;
}

.transfer-count {
  min-width: 17px;
  padding: 1px 5px;
  border-radius: 999px;
  color: #fff;
  background: var(--color-primary);
  font-size: 10px;
  text-align: center;
}

.transfer-summary-text {
  min-width: 0;
  overflow: hidden;
  color: var(--app-text-secondary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transfer-summary-text.error,
.task-error {
  color: var(--color-danger);
}

.transfer-summary-rate {
  margin-left: auto;
  color: var(--app-text-secondary);
  white-space: nowrap;
}

.transfer-body {
  display: flex;
  height: calc(100% - 32px);
  min-height: 0;
  flex-direction: column;
  border-top: 1px solid color-mix(in srgb, var(--app-border-shadow) 62%, transparent);
}

.transfer-body-toolbar {
  display: flex;
  flex: 0 0 30px;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
  color: var(--app-text-secondary);
  font-size: 11px;
}

.transfer-body-toolbar button,
.transfer-action {
  border: 0;
  color: var(--color-primary);
  background: transparent;
}

.transfer-body-toolbar button:disabled {
  opacity: .35;
}

.transfer-list {
  min-height: 0;
  overflow: auto;
  padding: 0 7px 7px;
}

.transfer-task {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) 28px;
  align-items: center;
  gap: 6px;
  min-height: 54px;
  padding: 5px 6px;
  border-radius: 7px;
}

.transfer-task:hover {
  background: color-mix(in srgb, var(--app-text) 5%, transparent);
}

.transfer-direction {
  color: var(--color-primary);
  font-size: 16px;
  text-align: center;
}

.transfer-task-main {
  min-width: 0;
}

.transfer-task-heading,
.transfer-task-meta {
  display: flex;
  align-items: center;
  gap: 10px;
}

.transfer-task-heading strong {
  min-width: 0;
  overflow: hidden;
  color: var(--app-text);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transfer-task-heading span,
.transfer-task-meta {
  color: var(--app-text-secondary);
  font-size: 10px;
}

.transfer-task-meta span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transfer-route {
  max-width: min(42vw, 520px);
}

.transfer-progress {
  height: 3px;
  margin-top: 5px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--app-text) 9%, transparent);
}

.transfer-progress i {
  display: block;
  height: 100%;
  background: var(--color-primary);
}

.status-failed .transfer-progress i {
  background: var(--color-danger);
}

.transfer-action {
  display: inline-flex;
  width: 26px;
  height: 26px;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
}

.transfer-action:hover {
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
}

.transfer-action.pending {
  color: var(--app-text-secondary);
}

.transfer-empty {
  display: grid;
  flex: 1;
  place-items: center;
  color: var(--app-text-secondary);
}
</style>
