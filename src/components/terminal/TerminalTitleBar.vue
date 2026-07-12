<script setup>
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { getSessionSyncBadgeState, SYNC_INPUT_CHANNELS_STORAGE_KEY } from '@/utils/syncInputChannels';

const props = defineProps({
  sessionId: {
    type: String,
    required: true,
  },
  sessionName: {
    type: String,
    default: '',
  },
  showActionDivider: {
    type: Boolean,
    default: true,
  },
});

const sshStore = useSshStore();

const createEmptySyncState = () => ({
  visible: false,
  channelId: '',
  channelName: '',
  connectedCount: 0,
  isPrimary: false,
  sourceMode: 'all',
  sendMode: 'realtime',
  broadcastEnabled: false,
});

const syncBadgeState = ref(createEmptySyncState());

const session = computed(() => sshStore.sessions.find((item) => item.id === props.sessionId) || null);

const displayName = computed(() => {
  const explicitName = String(props.sessionName || '').trim();
  if (explicitName) return explicitName;

  const activeSession = session.value;
  const savedName = String(activeSession?.name || '').trim();
  if (savedName) return savedName;

  const username = String(activeSession?.config?.username || '').trim();
  const host = String(activeSession?.config?.host || activeSession?.host || '').trim();
  if (username && host) return `${username}@${host}`;
  return host || '未命名会话';
});

const isConnected = computed(() => session.value?.status === 'connected');
const connectionTitle = computed(() => {
  if (session.value?.status === 'connected') return '连接中';
  if (session.value?.status === 'connecting') return '连接中';
  return '已断开';
});

const syncSummary = computed(() => {
  const state = syncBadgeState.value;
  if (!state.visible) return '';

  const roleLabel = state.sourceMode === 'primary'
    ? (state.isPrimary ? '主控' : '跟随')
    : '任意输入';
  const sendLabel = state.sendMode === 'line' ? '回车发送' : '实时发送';
  const parts = [
    state.channelName,
    `${state.connectedCount} 台`,
    roleLabel,
    sendLabel,
  ];

  if (!state.broadcastEnabled) {
    parts.push('已暂停');
  }

  return parts.join(' | ');
});

const loadSyncInputState = () => {
  try {
    const raw = localStorage.getItem(SYNC_INPUT_CHANNELS_STORAGE_KEY);
    if (!raw) {
      syncBadgeState.value = getSessionSyncBadgeState([], props.sessionId);
      return;
    }

    const parsed = JSON.parse(raw) || {};
    syncBadgeState.value = getSessionSyncBadgeState(parsed.channels || [], props.sessionId);
  } catch {
    syncBadgeState.value = getSessionSyncBadgeState([], props.sessionId);
  }
};

const onSyncInputChanged = (event) => {
  const detail = event?.detail || {};
  syncBadgeState.value = getSessionSyncBadgeState(detail.syncChannels || [], props.sessionId);
};

onMounted(() => {
  loadSyncInputState();
  window.addEventListener('sync-input-changed', onSyncInputChanged);
  window.addEventListener('storage', loadSyncInputState);
});

onUnmounted(() => {
  window.removeEventListener('sync-input-changed', onSyncInputChanged);
  window.removeEventListener('storage', loadSyncInputState);
});
</script>

<template>
  <div class="terminal-titlebar">
    <div class="terminal-titlebar-main" :title="displayName">
      <span class="session-status-dot" :class="{ live: isConnected, dead: !isConnected }" :title="connectionTitle" />
      <span class="terminal-titlebar-name">{{ displayName }}</span>
    </div>

    <div class="terminal-titlebar-side">
      <span v-if="syncBadgeState.visible" class="terminal-titlebar-sync" :title="syncSummary">{{ syncSummary }}</span>
      <span v-if="syncBadgeState.visible && showActionDivider" class="terminal-titlebar-divider" aria-hidden="true" />
      <div class="terminal-titlebar-actions">
        <slot name="actions" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-titlebar {
  width: 100%;
  min-height: 34px;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 0 12px;
  color: var(--app-text-muted);
}

.terminal-titlebar-main {
  min-width: 0;
  flex: 1 1 auto;
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.session-status-dot {
  width: 8px;
  height: 8px;
  flex: 0 0 auto;
  border-radius: 999px;
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--app-border-shadow) 86%, transparent);
}

.session-status-dot.live {
  background: var(--color-success);
}

.session-status-dot.dead {
  background: var(--color-danger);
}

.terminal-titlebar-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 600;
  color: var(--app-text);
}

.terminal-titlebar-side {
  min-width: 0;
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.terminal-titlebar-sync {
  min-width: 0;
  flex: 0 1 auto;
  max-width: min(36vw, 320px);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--app-text-muted);
  font-size: 11px;
}

.terminal-titlebar-divider {
  width: 1px;
  height: 14px;
  flex: 0 0 auto;
  background: color-mix(in srgb, var(--app-border-shadow) 92%, transparent);
}

.terminal-titlebar-actions {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
}
</style>
