<script setup>
import { computed } from 'vue';
import { useSshStore } from '@/stores/ssh';
import DuskDock from './DuskDock.vue';

const sshStore = useSshStore();
const sessions = computed(() => (sshStore.sessions || []).filter((session) => !session.isSplitChild));
const active = computed(() => sessions.value.find((session) => session.id === sshStore.activeSessionId) || null);
const sessionName = computed(() => active.value?.name || active.value?.config?.name || '暂无活动会话');
const endpoint = computed(() => {
  if (!active.value) return '';
  const config = active.value.config || active.value;
  const username = config.username || '';
  const host = config.host || config.hostname || '';
  return username && host ? `${username}@${host}` : (host || username);
});
const stateClass = (session) => ({
  active: session.id === sshStore.activeSessionId,
  connected: session.status === 'connected',
  failed: session.status === 'disconnected' || session.status === 'error',
});
const activate = (session) => {
  if (!session?.id || session.id === sshStore.activeSessionId) return;
  sshStore.activeSessionId = session.id;
  window.dispatchEvent(new CustomEvent('terminal:focus', { detail: { sessionId: session.id } }));
};
</script>

<template>
  <div class="session-dock-wrap">
    <DuskDock compact class="session-current">
      <span class="session-state" :class="active ? stateClass(active) : null" />
      <span class="session-name">{{ sessionName }}</span>
      <span v-if="endpoint" class="session-endpoint">· {{ endpoint }}</span>
    </DuskDock>
    <div v-if="sessions.length" class="session-dots" aria-label="活动会话">
      <button v-for="(session, index) in sessions" :key="session.id" class="session-dot"
        :class="stateClass(session)" :title="session.name || `Session ${index + 1}`" @click="activate(session)" />
    </div>
  </div>
</template>

<style scoped>
.session-dock-wrap { display: flex; flex-direction: column; align-items: center; min-width: 0; pointer-events: none; }
.session-current { max-width: min(360px, 34vw); font-size: 11px; white-space: nowrap; }
.session-state, .session-dot { width: 6px; height: 6px; border-radius: 50%; background: color-mix(in srgb, var(--app-text-muted) 55%, transparent); flex: 0 0 auto; }
.session-state { margin-right: 6px; }
.session-state.connected, .session-dot.connected { background: var(--color-success); }
.session-state.failed, .session-dot.failed { background: var(--color-danger); }
.session-name, .session-endpoint { overflow: hidden; text-overflow: ellipsis; }
.session-name { color: var(--app-text); font-weight: 600; }
.session-endpoint { color: var(--app-text-muted); margin-left: 4px; }
.session-dots { display: flex; gap: 6px; max-width: min(320px, 32vw); overflow-x: auto; padding: 3px 4px 0; scrollbar-width: none; pointer-events: auto; }
.session-dots::-webkit-scrollbar { display: none; }
.session-dot { border: 0; padding: 0; cursor: pointer; transition: transform 120ms ease, box-shadow 120ms ease; }
.session-dot.active { background: var(--color-primary); transform: scale(1.45); box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 24%, transparent); }
</style>
