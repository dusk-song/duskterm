<script setup>
import { ListChecks } from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { invokeCommand } from '@/utils/ipc';
import DuskDock from './DuskDock.vue';

const status = ref({ active: 0, total: 0, lastName: '', items: [] });
const open = ref(false);
const rootRef = ref(null);
const popupStyle = ref({});
let popupPositionFrame = null;
const transferCount = computed(() => status.value.active || status.value.total || 0);

const formatSize = (bytes) => {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / (1024 ** index)).toFixed(1)} ${units[index]}`;
};
const formatRate = (bytes) => `${formatSize(bytes)}/s`;
const formatEta = (seconds) => Number.isFinite(seconds) ? `${Math.max(0, Math.round(seconds))}s` : '--';
const cancel = async (item) => {
  if (item.sessionId) await invokeCommand('sftp_cancel_transfer', { sessionId: item.sessionId, reqId: item.id });
};
const clear = (id) => window.dispatchEvent(new CustomEvent('sftp-clear-transfer', { detail: { id } }));
const updatePopupPosition = () => {
  const rect = rootRef.value?.getBoundingClientRect();
  if (!rect) return;
  const width = 320;
  const margin = 8;
  const left = Math.max(margin, Math.min(window.innerWidth - width - margin, rect.right - width));
  popupStyle.value = {
    top: `${Math.round(rect.bottom + 7)}px`,
    left: `${Math.round(left)}px`,
    width: `${width}px`,
  };
};
const schedulePopupPosition = () => {
  if (!open.value || popupPositionFrame) return;
  popupPositionFrame = requestAnimationFrame(() => {
    popupPositionFrame = null;
    updatePopupPosition();
  });
};
const toggleOpen = async () => {
  open.value = !open.value;
  if (open.value) {
    await nextTick();
    updatePopupPosition();
  }
};
const onStatus = (event) => {
  const detail = event.detail || {};
  if (Number(detail.active || 0) > 0 && status.value.active === 0) open.value = true;
  status.value = {
    active: Number(detail.active || 0),
    total: Number(detail.total || 0),
    lastName: detail.lastName || '',
    items: Array.isArray(detail.items) ? detail.items : [],
  };
};
const onOutside = (event) => {
  if (open.value && !event.composedPath().some((element) => element?.classList?.contains('transfer-dock-root') || element?.classList?.contains('transfer-popup'))) open.value = false;
};
watch(open, async (value) => {
  if (value) {
    await nextTick();
    updatePopupPosition();
  } else if (popupPositionFrame) {
    cancelAnimationFrame(popupPositionFrame);
    popupPositionFrame = null;
  }
});
onMounted(() => {
  window.addEventListener('sftp-transfer-status', onStatus);
  window.addEventListener('click', onOutside);
  window.addEventListener('resize', schedulePopupPosition);
  window.addEventListener('scroll', schedulePopupPosition, true);
});
onUnmounted(() => {
  window.removeEventListener('sftp-transfer-status', onStatus);
  window.removeEventListener('click', onOutside);
  window.removeEventListener('resize', schedulePopupPosition);
  window.removeEventListener('scroll', schedulePopupPosition, true);
  if (popupPositionFrame) cancelAnimationFrame(popupPositionFrame);
});
</script>

<template>
  <div ref="rootRef" class="transfer-dock-root" @dblclick.stop>
    <DuskDock class="transfer-dock" :class="{ active: open }" interactive @click="toggleOpen">
      <ListChecks :size="14" />
      <span v-if="transferCount" class="transfer-badge" :class="{ busy: status.active }">{{ transferCount }}</span>
    </DuskDock>
    <Teleport to="body">
      <div v-if="open" class="transfer-popup" :style="popupStyle">
        <div class="transfer-title">传输队列</div>
        <div v-if="status.items.length" class="transfer-items">
          <div v-for="item in status.items" :key="item.id" class="transfer-item">
            <div class="transfer-row">
              <span class="transfer-name">{{ item.direction === 'download' ? '↓' : '↑' }} {{ item.name }}</span>
              <span>{{ formatSize(item.loaded) }} / {{ formatSize(item.total) }}</span>
            </div>
            <div class="transfer-progress"><i :style="{ width: `${item.progress || 0}%` }" /></div>
            <div class="transfer-row transfer-meta">
              <span>{{ formatRate(item.rate || 0) }} · 剩余 {{ formatEta(item.etaSeconds) }}</span>
              <button v-if="item.status === 'uploading' || item.status === 'waiting'" @click.stop="cancel(item)">取消</button>
              <button v-else @click.stop="clear(item.id)">清除</button>
            </div>
          </div>
        </div>
        <div v-else class="transfer-empty">暂无传输任务</div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.transfer-dock-root { position: relative; pointer-events: auto; }
.transfer-dock { gap: 6px; padding: 0 9px; font-size: 11px; cursor: pointer; white-space: nowrap; }
.transfer-dock.active { border-color: color-mix(in srgb, var(--color-primary) 65%, transparent); }
.transfer-badge {
  display: inline-flex;
  min-width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  padding: 0 5px;
  border-radius: 999px;
  color: var(--tb-text, var(--app-text));
  background: color-mix(in srgb, var(--app-text) 12%, transparent);
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
}
.transfer-badge.busy {
  color: #fff;
  background: var(--color-primary);
}
.transfer-popup { position: fixed; max-height: 300px; overflow: auto; padding: 10px; border: 1px solid var(--app-border-shadow); border-radius: 9px; background: color-mix(in srgb, var(--app-bg-dialog) 94%, transparent); box-shadow: var(--niri-shadow-dialog); backdrop-filter: blur(12px); z-index: 99999; }
.transfer-title { padding: 0 2px 7px; font-size: 12px; font-weight: 700; color: var(--app-text); }
.transfer-items { display: flex; flex-direction: column; gap: 7px; }
.transfer-item { padding: 7px; border-radius: 6px; background: var(--app-input-bg); font-size: 10px; color: var(--app-text-muted); }
.transfer-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.transfer-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--app-text); }
.transfer-progress { height: 4px; margin: 6px 0; overflow: hidden; border-radius: 2px; background: color-mix(in srgb, var(--app-text) 9%, transparent); }
.transfer-progress i { display: block; height: 100%; background: var(--color-primary); }
.transfer-meta button { border: 0; color: var(--color-primary); background: transparent; cursor: pointer; }
.transfer-empty { padding: 15px; text-align: center; color: var(--app-text-muted); font-size: 11px; }
</style>
