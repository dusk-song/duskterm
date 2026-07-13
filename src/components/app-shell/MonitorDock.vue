<script setup>
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
const sourceColor = computed(() => remoteSource.value
  ? (settings.value.remoteColor || 'var(--color-primary)')
  : (settings.value.localColor || 'var(--app-text)'));
const labelColor = computed(() => settings.value.labelColor || 'var(--app-text-muted)');
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

function startPolling() {
  clearInterval(pollTimer);
  clearInterval(diskTimer);
  pollTimer = null;
  diskTimer = null;
  if (!isTauriRuntime() || !settings.value.showMonitor) return;
  const battery = isOnBattery() ? 2 : 1;
  pollTimer = setInterval(() => fetchStats(), Math.max(2000, Number(settings.value.refreshIntervalMs) || 2000) * battery);
  if (settings.value.showDisk) diskTimer = setInterval(() => fetchStats(true), Math.max(10000, Number(settings.value.diskIntervalMs) || 10000) * battery);
  fetchStats();
}

function refreshSettings() { settings.value = loadMonitorSettings(); startPolling(); }
onMounted(() => { window.addEventListener('monitor-settings-changed', refreshSettings); startPolling(); });
onUnmounted(() => { window.removeEventListener('monitor-settings-changed', refreshSettings); clearInterval(pollTimer); clearInterval(diskTimer); });
watch(() => activeSession.value?.id, () => { lastCpuSample = null; lastNetSample = null; startPolling(); });
</script>

<template>
  <DuskDock v-if="settings.showMonitor" class="monitor-dock"
    :style="{ '--monitor-value': sourceColor, '--monitor-label': labelColor }">
    <span v-if="settings.showCpu"><b>CPU</b>{{ percent(stats.cpu) }}</span>
    <span v-if="settings.showMemory"><b>内存</b>{{ percent(stats.memory) }}</span>
    <span v-if="settings.showDisk"><b>磁盘</b>{{ percent(stats.disk) }}</span>
    <span v-if="settings.showNet"><b>↑</b>{{ rate(netRate.tx) }} <b>↓</b>{{ rate(netRate.rx) }}</span>
  </DuskDock>
</template>

<style scoped>
.monitor-dock {
  gap: 10px;
  padding: 0 10px;
  border-color: color-mix(in srgb, var(--app-text) 18%, var(--app-border-light)) !important;
  background: color-mix(in srgb, var(--app-bg-dialog) 92%, transparent) !important;
  color: color-mix(in srgb, var(--monitor-value) 92%, var(--app-text)) !important;
  font: 700 11px/1 Consolas, monospace;
  text-shadow: 0 1px 2px color-mix(in srgb, var(--app-workspace-gap, #000) 40%, transparent);
  white-space: nowrap;
}
.monitor-dock span { display: inline-flex; align-items: center; gap: 3px; }
.monitor-dock b { color: color-mix(in srgb, var(--monitor-label) 72%, var(--app-text)); font-weight: 800; }
</style>
