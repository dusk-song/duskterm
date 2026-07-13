<script setup>
import { ChevronLeft, ChevronRight } from '@lucide/vue';
import { computed } from 'vue';
import { useSshStore } from '@/stores/ssh';
import DuskDock from './DuskDock.vue';

const sshStore = useSshStore();
const sessions = computed(() => (sshStore.sessions || []).filter((session) => !session.isSplitChild));
const active = computed(() => sessions.value.find((session) => session.id === sshStore.activeSessionId) || null);
const activeIndex = computed(() => sessions.value.findIndex((session) => session.id === sshStore.activeSessionId));
const canSwitchSession = computed(() => sessions.value.length > 1);
const sessionIdentity = computed(() => {
  if (!active.value) return '暂无活动会话';
  const config = active.value.config || active.value;
  const username = config.username || '';
  const host = config.host || config.hostname || '';
  return username && host ? `${username}@${host}` : (host || username || '暂无活动会话');
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
const switchSession = (direction) => {
  const list = sessions.value;
  if (!list.length) return;
  if (activeIndex.value < 0) return activate(list[0]);
  const nextIndex = (activeIndex.value + direction + list.length) % list.length;
  activate(list[nextIndex]);
};
</script>

<template>
  <div class="session-dock-wrap">
    <DuskDock interactive class="session-current">
      <button class="session-nav session-nav--prev" :disabled="!canSwitchSession" title="上一个会话"
        @click.stop="switchSession(-1)">
        <ChevronLeft :size="13" />
      </button>
      <span class="session-state session-drag-region" :class="active ? stateClass(active) : null" />
      <span class="session-identity session-drag-region">{{ sessionIdentity }}</span>
      <button class="session-nav session-nav--next" :disabled="!canSwitchSession" title="下一个会话"
        @click.stop="switchSession(1)">
        <ChevronRight :size="13" />
      </button>
    </DuskDock>
  </div>
</template>

<style scoped>
.session-dock-wrap { display: flex; align-items: center; min-width: 0; pointer-events: none; }
.session-current { max-width: min(360px, 34vw); gap: 6px; font-size: 12px; font-weight: 600; white-space: nowrap; }
.session-nav {
  display: inline-flex;
  width: 20px;
  height: 20px;
  align-items: center;
  justify-content: center;
  flex: 0 0 auto;
  padding: 0;
  border: 0;
  border-radius: 999px;
  color: var(--tb-text-muted, var(--app-text-muted));
  background: transparent;
  cursor: default;
}
.session-nav:not(:disabled):hover {
  color: var(--tb-text, var(--app-text));
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
}
.session-nav:disabled { opacity: 0.32; }
.session-drag-region { pointer-events: none !important; }
.session-state {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--app-text-muted) 55%, transparent);
  flex: 0 0 auto;
}
.session-state.connected { background: var(--color-success); }
.session-state.failed { background: var(--color-danger); }
.session-identity {
  min-width: 0;
  overflow: hidden;
  color: var(--app-text);
  text-overflow: ellipsis;
}
</style>
