<script setup>
import { ArrowDown, ArrowUp, Cpu, HardDrive, MemoryStick } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand, isTauriRuntime } from '@/utils/ipc';
import { loadMonitorSettings } from '@/utils/monitor';
import { isOnBattery } from '@/utils/performance';
import DuskDock from './DuskDock.vue';

const sshStore = useSshStore();
const settings = ref(loadMonitorSettings());
const stats = ref({ cpu: 0, memory: 0, disk: 0, net_rx: 0, net_tx: 0 });
const netRate = ref({ rx: 0, tx: 0 });
const activeSession = computed(() => sshStore.sessions.find((session) => session.id === sshStore.activeSessionId));
const remoteSource = ref(false);
let lastNetSample = null;
let lastCpuSample = null;
let polling = false;
let pollTimer = null;
let diskTimer = null;

const percent = (value) => `${Math.round(Number(value) || 0)}%`;
const rate = (value) => {
  const kb = Math.max(0, Number(value) || 0) / 1024;
  return kb < 1024 ? `${kb.toFixed(1)}K/s` : `${(kb / 1024).toFixed(1)}M/s`;
};

async function fetchStats(diskOnly = false) {
  if (document.hidden) return;
  if (polling && !diskOnly) return;
  if (!diskOnly) polling = true;
  try {
    const active = activeSession.value;
    const remote = active?.status === 'connected' && active.config;
    remoteSource.value = Boolean(remote);
    const data = remote
      ? await invokeCommand('get_remote_stats', { sessionId: active.id })
      : await invokeCommand('get_system_stats');
    if (remote && !diskOnly) {
      let cpu = 0;
      if (lastCpuSample && data.cpu_total) {
        const total = data.cpu_total - lastCpuSample.total;
        const idle = data.cpu_idle - lastCpuSample.idle;
        if (total > 0) cpu = ((total - idle) / total) * 100;
      }
      lastCpuSample = { total: data.cpu_total, idle: data.cpu_idle };
      data.cpu = cpu;
    } else if (!remote) lastCpuSample = null;
    if (diskOnly) {
      stats.value = { ...stats.value, disk: data.disk };
      return;
    }
    const now = Date.now();
    if (lastNetSample) {
      const seconds = Math.max(0.1, (now - lastNetSample.time) / 1000);
      netRate.value = {
        rx: Math.max(0, (data.net_rx - lastNetSample.rx) / seconds),
        tx: Math.max(0, (data.net_tx - lastNetSample.tx) / seconds),
      };
    }
    lastNetSample = { rx: data.net_rx, tx: data.net_tx, time: now };
    stats.value = data;
  } catch { /* keep the last successful sample */ }
  finally { if (!diskOnly) polling = false; }
}

function stopPolling() {
  clearInterval(pollTimer);
  clearInterval(diskTimer);
  pollTimer = null;
  diskTimer = null;
  polling = false;
}

function startPolling() {
  stopPolling();
  if (!isTauriRuntime() || !settings.value.showMonitor || document.hidden) return;
  const battery = isOnBattery() ? 2 : 1;
  pollTimer = setInterval(() => fetchStats(), Math.max(2000, Number(settings.value.refreshIntervalMs) || 2000) * battery);
  if (settings.value.showDisk) diskTimer = setInterval(() => fetchStats(true), Math.max(10000, Number(settings.value.diskIntervalMs) || 10000) * battery);
  fetchStats();
}

function refreshSettings() { settings.value = loadMonitorSettings(); startPolling(); }
function handleVisibilityChange() {
  if (document.hidden) {
    stopPolling();
    return;
  }
  lastCpuSample = null;
  lastNetSample = null;
  startPolling();
}
onMounted(() => {
  window.addEventListener('monitor-settings-changed', refreshSettings);
  document.addEventListener('visibilitychange', handleVisibilityChange);
  startPolling();
});
onUnmounted(() => {
  window.removeEventListener('monitor-settings-changed', refreshSettings);
  document.removeEventListener('visibilitychange', handleVisibilityChange);
  stopPolling();
});
watch(() => activeSession.value?.id, () => { lastCpuSample = null; lastNetSample = null; startPolling(); });
</script>

<template>
  <DuskDock v-if="settings.showMonitor" class="monitor-dock" @dblclick.stop>
    <span v-if="settings.showCpu" title="CPU"><Cpu :size="12" />{{ percent(stats.cpu) }}</span>
    <span v-if="settings.showMemory" title="Memory"><MemoryStick :size="12" />{{ percent(stats.memory) }}</span>
    <span v-if="settings.showDisk" title="Disk"><HardDrive :size="12" />{{ percent(stats.disk) }}</span>
    <span v-if="settings.showNet" title="Network upload"><ArrowUp :size="12" />{{ rate(netRate.tx) }}</span>
    <span v-if="settings.showNet" title="Network download"><ArrowDown :size="12" />{{ rate(netRate.rx) }}</span>
  </DuskDock>
</template>

<style scoped>
.monitor-dock {
  gap: 10px;
  padding: 0 10px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.monitor-dock span { display: inline-flex; align-items: center; gap: 3px; }
.monitor-dock svg { color: var(--tb-text-muted, var(--app-text-muted)); flex: 0 0 auto; stroke-width: 2.2; }
</style>
