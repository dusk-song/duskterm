<script setup>
import { computed } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { buildSessionDisplayNameMap } from '@/utils/sessionOverview';
import DuskDock from './DuskDock.vue';

const sshStore = useSshStore();
const sessions = computed(() => (sshStore.sessions || []).filter((session) => !session.isSplitChild));
const active = computed(() => sessions.value.find((session) => session.id === sshStore.activeSessionId) || null);
const sessionDisplayNames = computed(() => buildSessionDisplayNameMap(sessions.value));
const sessionDisplayName = computed(() => {
  if (!active.value) return '暂无活动会话';
  return sessionDisplayNames.value.get(active.value.id) || active.value.name || '暂无活动会话';
});
const stateClass = (session) => ({
  connected: session.status === 'connected',
});
</script>

<template>
  <div class="session-dock-wrap">
    <DuskDock interactive class="session-current">
      <span class="session-center session-drag-region">
        <span class="session-state" :class="active ? stateClass(active) : null" />
        <span class="session-identity">{{ sessionDisplayName }}</span>
      </span>
    </DuskDock>
  </div>
</template>

<style scoped>
.session-dock-wrap { display: flex; width: 100%; min-width: 0; align-items: center; justify-content: center; pointer-events: none; }
.session-current {
  width: max-content;
  max-width: min(260px, 100%);
  padding-right: 10px;
  padding-left: 10px;
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
  pointer-events: auto;
}
.session-drag-region { cursor: default; }
.session-center {
  min-width: 0;
  flex: 0 1 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.session-state {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--app-connection-offline);
  flex: 0 0 auto;
}
.session-state.connected { background: var(--app-connection-online); }
.session-identity {
  min-width: 0;
  max-width: 220px;
  overflow: hidden;
  color: var(--app-text);
  text-align: center;
  text-overflow: ellipsis;
}
</style>
