<script setup>
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { BookOpen, FolderOpen, ListChecks } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand, isTauriRuntime } from '@/utils/ipc';
import { loadLightbarSettings } from '@/utils/lightbar';
import { loadMonitorSettings } from '@/utils/monitor';
import { isOnBattery } from '@/utils/performance';

const props = defineProps({
  showSessionPanel: Boolean,
  showSftpPanel: Boolean,
  showCommandKnowledgePanel: Boolean
});

const emit = defineEmits(['toggle-session-panel', 'toggle-sftp-panel', 'toggle-command-knowledge-panel']);

const sshStore = useSshStore();
const showSessionPanel = computed(() => props.showSessionPanel);
const showSftpPanel = computed(() => props.showSftpPanel);
const showCommandKnowledgePanel = computed(() => props.showCommandKnowledgePanel);

// ── Session dots ──
const sessionDots = computed(() => {
  return (sshStore.sessions || []).map((s, i) => ({
    id: s.id,
    active: s.id === sshStore.activeSessionId,
    connected: s.status === 'connected',
    failed: s.status === 'disconnected' || s.status === 'error',
    name: s.name || `Session ${i + 1}`,
    index: i
  }));
});

const scrollToSession = (index) => {
  window.dispatchEvent(new CustomEvent('terminal-scroll-to', { detail: { index } }));
};

// ── Monitor ──
const activeSession = computed(() => sshStore.sessions.find(s => s.id === sshStore.activeSessionId));

const monitorSettings = ref(loadMonitorSettings());
const stats = ref({ cpu: 0, memory: 0, disk: 0, net_rx: 0, net_tx: 0 });
const netRate = ref({ rx: 0, tx: 0 });
let lastNetSample = null;
let lastCpuSample = null;
let isPolling = false;
let pollTimer = null;
let diskPollTimer = null;
const useRemoteStats = ref(false);
const remoteError = ref(false);
const isMonitorExpanded = ref(false);
let monitorHideTimer = null;

const formatPercent = (value) => `${Math.round(value)}%`;
const formatRate = (bytesPerSec) => {
  if (!Number.isFinite(bytesPerSec)) return '0 KB/s';
  const kb = bytesPerSec / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB/s`;
  return `${(kb / 1024).toFixed(1)} MB/s`;
};

const updateStats = async () => {
  if (isPolling) return;
  isPolling = true;
  try {
    let data = null;
    const active = activeSession.value;
    if (active && active.status === 'connected' && active.config) {
      data = await invokeCommand('get_remote_stats', { sessionId: active.id });
      useRemoteStats.value = true;
      remoteError.value = false;
      let cpuPct = 0;
      if (data.cpu_total && lastCpuSample) {
        const totalDiff = data.cpu_total - lastCpuSample.total;
        const idleDiff = data.cpu_idle - lastCpuSample.idle;
        if (totalDiff > 0) cpuPct = ((totalDiff - idleDiff) / totalDiff) * 100;
      }
      lastCpuSample = { total: data.cpu_total, idle: data.cpu_idle };
      data.cpu = cpuPct;
      stats.value = data;
    } else {
      data = await invokeCommand('get_system_stats');
      useRemoteStats.value = false;
      remoteError.value = false;
      lastCpuSample = null;
      stats.value = data;
    }
    const now = Date.now();
    if (lastNetSample) {
      const dt = Math.max(0.1, (now - lastNetSample.time) / 1000);
      let rx = (data.net_rx - lastNetSample.rx) / dt;
      let tx = (data.net_tx - lastNetSample.tx) / dt;
      if (rx < 0) rx = 0;
      if (tx < 0) tx = 0;
      netRate.value = { rx, tx };
    }
    lastNetSample = { rx: data.net_rx, tx: data.net_tx, time: now };
  } catch (e) {
    if (activeSession.value && activeSession.value.status === 'connected') remoteError.value = true;
  } finally { isPolling = false; }
};

const updateDiskOnly = async () => {
  try {
    let data;
    const active = activeSession.value;
    if (active && active.status === 'connected' && active.config) {
      data = await invokeCommand('get_remote_stats', { sessionId: active.id });
      stats.value = { ...stats.value, disk: data.disk };
    } else {
      data = await invokeCommand('get_system_stats');
      stats.value = { ...stats.value, disk: data.disk };
    }
  } catch (e) { /* ignore */ }
};

const startPolling = () => {
  if (pollTimer) clearInterval(pollTimer);
  if (diskPollTimer) clearInterval(diskPollTimer);
  pollTimer = null;
  diskPollTimer = null;
  if (!isTauriRuntime()) return;
  if (!monitorSettings.value.showMonitor) return;
  // Double the polling interval when on battery to reduce CPU wake-ups
  const batteryMultiplier = isOnBattery() ? 2 : 1;
  const refreshInterval = Math.max(2000, Number(monitorSettings.value.refreshIntervalMs || 2000)) * batteryMultiplier;
  const diskInterval = Math.max(10000, Number(monitorSettings.value.diskIntervalMs || 10000)) * batteryMultiplier;
  updateStats();
  pollTimer = setInterval(updateStats, refreshInterval);
  if (monitorSettings.value.showDisk) {
    diskPollTimer = setInterval(updateDiskOnly, diskInterval);
  }
};

const monitorItems = computed(() => {
  if (!monitorSettings.value.showMonitor) return [];
  const items = [];
  if (monitorSettings.value.showCpu) items.push({ label: 'CPU', value: formatPercent(stats.value.cpu) });
  if (monitorSettings.value.showMemory) items.push({ label: 'MEM', value: formatPercent(stats.value.memory) });
  if (monitorSettings.value.showDisk) items.push({ label: 'DISK', value: formatPercent(stats.value.disk) });
  if (monitorSettings.value.showNet) {
    items.push({ label: 'NET', value: `↑${formatRate(netRate.value.tx)} ↓${formatRate(netRate.value.rx)}` });
  }
  return items;
});

const monitorColor = computed(() => {
  return useRemoteStats.value
    ? (monitorSettings.value.remoteColor || 'var(--color-primary)')
    : (monitorSettings.value.localColor || 'var(--app-text-muted)');
});

const monitorCapsuleText = computed(() => {
  if (monitorItems.value.length > 0) return monitorItems.value[0].value;
  return '';
});

const handleMonitorEnter = () => {
  if (monitorHideTimer) clearTimeout(monitorHideTimer);
  isMonitorExpanded.value = true;
};
const handleMonitorLeave = () => {
  monitorHideTimer = setTimeout(() => { isMonitorExpanded.value = false; }, 400);
};

const refreshMonitorSettings = () => {
  monitorSettings.value = loadMonitorSettings();
  startPolling();
};

// ── Transfer queue ──
const transferStatus = ref({ active: 0, total: 0, lastName: '', items: [] });
const isTransferPanelOpen = ref(false);
const completedTransferCount = computed(() =>
  (transferStatus.value.items || []).filter(i => i.status === 'done').length
);
const transferTotalCount = computed(() => Number(transferStatus.value.total || 0));

const formatEta = (seconds) => {
  if (!Number.isFinite(seconds) || seconds == null || seconds < 0) return '--';
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60), s = seconds % 60;
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60), rm = m % 60;
  return `${h}h ${rm}m`;
};
const formatSize = (bytes) => {
  if (!bytes) return '0 B';
  const k = 1024, sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
};

const handleTransferCancel = async (taskId) => {
  const task = (transferStatus.value.items || []).find((item) => item.id === taskId);
  if (!task?.sessionId) return;
  try { await invokeCommand('sftp_cancel_transfer', { sessionId: task.sessionId, reqId: taskId }); } catch (err) { console.error(err); }
};
const handleTransferClear = (taskId) => {
  window.dispatchEvent(new CustomEvent('sftp-clear-transfer', { detail: { id: taskId } }));
};
const onTransferStatus = (e) => {
  const detail = e?.detail || {};
  if (Number(detail.active || 0) > 0 && transferStatus.value.active === 0) {
    isTransferPanelOpen.value = true;
  }
  transferStatus.value = {
    active: Number(detail.active || 0),
    total: Number(detail.total || 0),
    lastName: detail.lastName || '',
    items: Array.isArray(detail.items) ? detail.items : []
  };
};
const onDocClick = (e) => {
  if (!isTransferPanelOpen.value) return;
  if (!e.composedPath().some(el =>
    el.classList && (el.classList.contains('transfer-popup') || el.classList.contains('indicator-btn'))
  )) {
    isTransferPanelOpen.value = false;
  }
};

// ── Lightbar (26 bars = A–Z, key-driven per-column spikes) ──
const lightbarSettings = ref(loadLightbarSettings());
const LIGHTBAR_COUNT = 26;
const lightbarPeakHeights = ref(new Array(LIGHTBAR_COUNT).fill(0));
const lightbarPeakHoldUntil = ref(new Array(LIGHTBAR_COUNT).fill(0));
const bottomLightbarHeights = ref(new Array(LIGHTBAR_COUNT).fill(0.08));
let lightbarAnimTimer = null;

/** Map a key to a bar index (0–25 for a–z), -1 for others. */
function lightbarIndexForKey(key) {
  if (!key || key.length !== 1) return -1;
  const code = key.charCodeAt(0);
  if (code >= 97 && code <= 122) return code - 97;       // a–z
  if (code >= 65 && code <= 90) return code - 65;        // A–Z
  return -1;
}

function tickLightbarDecay() {
  const now = Date.now();
  const enablePeakHold = !!lightbarSettings.value.enablePeakHold;
  const peakHoldMs = Number(lightbarSettings.value.peakHoldMs || 600);
  const trailDecay = Number(lightbarSettings.value.trailDecay || 0.88);
  const nextPeak = lightbarPeakHeights.value.slice();
  const nextHold = lightbarPeakHoldUntil.value.slice();
  const raw = new Array(LIGHTBAR_COUNT).fill(0.08);

  for (let i = 0; i < LIGHTBAR_COUNT; i++) {
    let h = nextPeak[i];
    if (enablePeakHold) {
      if (now > nextHold[i]) h = Math.max(0.08, h * trailDecay);
    } else {
      h = Math.max(0.08, h * trailDecay);
    }
    if (h <= 0.085) h = 0.08; // snap to idle
    raw[i] = h;
    nextPeak[i] = h;
    if (h <= 0.085) nextHold[i] = 0;
  }

  const allIdle = raw.every(v => v <= 0.085);
  lightbarPeakHeights.value = nextPeak;
  lightbarPeakHoldUntil.value = nextHold;
  bottomLightbarHeights.value = raw;

  if (allIdle) {
    clearInterval(lightbarAnimTimer);
    lightbarAnimTimer = null;
  }
}

const startLightbarAnimator = () => {
  if (lightbarAnimTimer) return; // already running
  if (!lightbarSettings.value.enabled && lightbarSettings.value.enabled !== undefined) return;
  lightbarAnimTimer = setInterval(tickLightbarDecay, 50);
};

const handleGlobalTyping = (e) => {
  if (e.isComposing) return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const idx = lightbarIndexForKey(e.key);
  if (idx < 0) return;

  const peakHoldMs = Number(lightbarSettings.value.peakHoldMs || 600);
  const now = Date.now();
  const peaks = lightbarPeakHeights.value.slice();
  const holds = lightbarPeakHoldUntil.value.slice();
  peaks[idx] = 0.4 + Math.random() * 0.6;
  holds[idx] = now + peakHoldMs;
  lightbarPeakHeights.value = peaks;
  lightbarPeakHoldUntil.value = holds;

  if (!lightbarAnimTimer && lightbarSettings.value.enabled !== false) {
    startLightbarAnimator();
  }
};

const refreshLightbarSettings = () => {
  lightbarSettings.value = loadLightbarSettings();
  startLightbarAnimator();
};

// ── Lifecycle ──
onMounted(() => {
  window.addEventListener('sftp-transfer-status', onTransferStatus);
  window.addEventListener('monitor-settings-changed', refreshMonitorSettings);
  window.addEventListener('lightbar-settings-changed', refreshLightbarSettings);
  window.addEventListener('click', onDocClick);
  window.addEventListener('keydown', handleGlobalTyping, true);
  startPolling();
  startLightbarAnimator();
});

onUnmounted(() => {
  window.removeEventListener('sftp-transfer-status', onTransferStatus);
  window.removeEventListener('monitor-settings-changed', refreshMonitorSettings);
  window.removeEventListener('lightbar-settings-changed', refreshLightbarSettings);
  window.removeEventListener('click', onDocClick);
  window.removeEventListener('keydown', handleGlobalTyping, true);
  if (pollTimer) clearInterval(pollTimer);
  if (diskPollTimer) clearInterval(diskPollTimer);
  if (lightbarAnimTimer) clearInterval(lightbarAnimTimer);
  if (monitorHideTimer) clearTimeout(monitorHideTimer);
});

watch(() => monitorSettings.value.showMonitor, startPolling);
watch(() => activeSession.value?.id, startPolling);
watch(() => monitorSettings.value.refreshIntervalMs, startPolling);
watch(() => monitorSettings.value.diskIntervalMs, startPolling);
watch(() => monitorSettings.value.showDisk, startPolling);
watch(() => lightbarSettings.value.speed, startLightbarAnimator);

const toggleSessionPanel = () => emit('toggle-session-panel');
const toggleSftpPanel = () => emit('toggle-sftp-panel');
const toggleCommandKnowledgePanel = () => emit('toggle-command-knowledge-panel');
</script>

<template>
  <!-- Bottom indicator strip -->
  <div class="bottom-indicator" role="status" aria-label="状态栏">
    <!-- Session list toggle: far left -->
    <button class="indicator-btn" :class="{ active: showSessionPanel }" aria-label="会话列表"
      @click="toggleSessionPanel">
      <span class="indicator-icon">☰</span>
    </button>

    <!-- Left section: session dots, centered -->
    <div class="indicator-left">
      <div class="indicator-dots" v-if="sessionDots.length > 0">
        <Tooltip v-for="(dot, i) in sessionDots" :key="dot.id" :delay-duration="150">
          <TooltipTrigger as-child>
            <button class="indicator-dot"
              :class="{ active: dot.active, connected: dot.connected, failed: dot.failed }"
              @click="scrollToSession(i)" />
          </TooltipTrigger>
          <TooltipContent side="top" :side-offset="6">
            {{ dot.name }}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>

    <!-- Center: lightbar wave -->
    <div class="indicator-center">
      <div class="indicator-lightbar"
        :style="{ '--lb-start': lightbarSettings.colorStart || 'var(--app-text-muted)', '--lb-end': lightbarSettings.colorEnd || 'var(--color-primary)' }">
        <span v-for="(h, idx) in bottomLightbarHeights" :key="idx" class="lightbar-pip"
          :style="{ transform: `scaleY(${Math.max(0.04, h)})`, opacity: 0.2 + h * 0.8 }" />
      </div>
    </div>

    <!-- Right section: Transfer + SFTP toggle -->
    <div class="indicator-right">
      <button class="indicator-btn" :class="{ active: showCommandKnowledgePanel }" aria-label="命令知识库"
        @click="toggleCommandKnowledgePanel">
        <BookOpen :size="14" stroke-width="1.8" />
      </button>
      <button class="indicator-btn" :class="{ active: isTransferPanelOpen, 'has-badge': transferStatus.active > 0 }"
        aria-label="传输列表" @click="isTransferPanelOpen = !isTransferPanelOpen">
        <ListChecks :size="14" stroke-width="1.8" />
        <span v-if="transferStatus.active > 0" class="transfer-badge">{{ transferStatus.active }}</span>
      </button>
      <button class="indicator-btn indicator-sftp-btn" :class="{ active: showSftpPanel }" aria-label="SFTP 文件面板"
        @click="toggleSftpPanel">
        <FolderOpen :size="14" stroke-width="1.8" />
      </button>
    </div>

    <!-- Monitor capsule (floating bottom-right) -->
    <div v-if="monitorSettings.showMonitor && monitorCapsuleText" class="monitor-capsule"
      :class="{ expanded: isMonitorExpanded }" :style="{ '--mc-color': monitorColor }" @mouseenter="handleMonitorEnter"
      @mouseleave="handleMonitorLeave">
      <span class="capsule-text">{{ monitorCapsuleText }}</span>
      <div class="capsule-expand" v-show="isMonitorExpanded">
        <div class="capsule-row" v-for="(item, idx) in monitorItems" :key="idx">
          <span class="capsule-label">{{ item.label }}</span>
          <span class="capsule-value">{{ item.value }}</span>
        </div>
      </div>
    </div>
  </div>

  <!-- Transfer queue popup -->
  <Teleport to="body">
    <div class="transfer-popup" v-if="isTransferPanelOpen">
      <div class="transfer-popup-title">传输队列</div>
      <div class="transfer-popup-items" v-if="transferStatus.items.length">
        <div class="transfer-popup-item" v-for="item in transferStatus.items" :key="item.id"
          :class="`is-${item.status}`">
          <div class="transfer-item-row">
            <span class="transfer-item-name">
              <span class="transfer-arrow">{{ item.direction === 'download' ? '↓' : '↑' }}</span>{{ item.name }}
            </span>
            <span class="transfer-item-size" v-if="item.status === 'uploading' && item.total">
              {{ formatSize(item.loaded) }} / {{ formatSize(item.total) }}
            </span>
            <span class="transfer-item-size" v-else-if="item.status === 'waiting'"
              style="color: var(--app-text-muted)">等待中</span>
            <span class="transfer-item-size" v-else-if="item.status === 'done'"
              style="color: var(--color-success)">完成</span>
            <span class="transfer-item-size" v-else-if="item.status === 'error'"
              style="color: var(--color-danger)">失败</span>
            <span class="transfer-item-size" v-else-if="item.status === 'cancelled'"
              style="color: var(--color-warning)">已取消</span>
          </div>
          <div class="transfer-item-row2">
            <div class="transfer-progress-bg">
              <div class="transfer-progress-bar" :style="{ width: (item.progress || 0) + '%' }"></div>
            </div>
            <span class="transfer-item-pct">{{ (item.progress || 0).toFixed(1) }}%</span>
          </div>
          <div v-if="item.status === 'uploading'" class="transfer-item-meta">
            <span>{{ formatRate(item.rate || 0) }}</span>
            <span>剩余 {{ formatEta(item.etaSeconds) }}</span>
          </div>
          <div class="transfer-item-actions">
            <button v-if="item.status === 'uploading' || item.status === 'waiting'" class="transfer-action-btn cancel"
              @click="handleTransferCancel(item.id)">取消</button>
            <button v-if="item.status === 'done' || item.status === 'error' || item.status === 'cancelled'"
              class="transfer-action-btn clear" @click="handleTransferClear(item.id)">清除</button>
          </div>
        </div>
      </div>
      <div v-else class="transfer-popup-empty">暂无传输任务</div>
    </div>
  </Teleport>
</template>

<style scoped>
/* ── Bottom indicator strip ── */
.bottom-indicator {
  --status-btn-color: color-mix(in srgb, var(--app-text-muted) 78%, transparent);
  --status-btn-hover-color: var(--app-text);
  --status-btn-hover-bg: color-mix(in srgb, var(--app-text) 8%, transparent);
  --status-btn-active-color: var(--color-primary);
  --status-btn-active-bg: color-mix(in srgb, var(--color-primary) 16%, transparent);
  --status-btn-disabled-color: color-mix(in srgb, var(--app-text-muted) 34%, transparent);
  --status-btn-opacity: 0.68;
  height: 24px;
  display: flex;
  align-items: center;
  padding: 0 10px;
  user-select: none;
  z-index: var(--z-chrome);
  background: var(--tb-bg);
  border-top: 1px solid var(--tb-divider);
  /* border-top removed — visual unity with workspace */
  flex-shrink: 0;
}

.indicator-left {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.indicator-center {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
}

.indicator-right {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: flex-end;
}

/* ── Side buttons ── */
.indicator-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 22px;
  width: 22px;
  padding: 0;
  background: transparent;
  border: none;
  cursor: pointer;
  opacity: var(--status-btn-opacity);
  transition: background var(--app-motion-control, 120ms ease), color var(--app-motion-control, 120ms ease), opacity var(--app-motion-control, 120ms ease);
  flex-shrink: 0;
  color: var(--status-btn-color);
  font-size: 12px;
  border-radius: var(--niri-radius-sm, 5px);
}

.indicator-btn:hover {
  opacity: 1;
  color: var(--status-btn-hover-color);
  background: var(--status-btn-hover-bg);
}

.indicator-btn.active {
  opacity: 1;
  color: var(--status-btn-active-color);
  background: var(--status-btn-active-bg);
}

.indicator-btn:disabled,
.indicator-btn.disabled {
  opacity: 1;
  color: var(--status-btn-disabled-color);
  background: transparent;
  cursor: default;
  pointer-events: none;
}

.indicator-icon {
  font-size: 12px;
  line-height: 1;
  color: var(--mac-text-primary, var(--app-text-muted));
  font-weight: 600;
}

.indicator-btn svg {
  width: 14px;
  height: 14px;
  fill: none;
  stroke: currentColor;
}

.indicator-sftp-btn {
  margin-left: 2px;
}

.indicator-btn.has-badge {
  position: relative;
  opacity: 1;
}

.transfer-badge {
  position: absolute;
  top: -2px;
  right: -4px;
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  font-size: 9px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
  color: var(--color-primary-foreground);
  background: var(--color-primary);
  border-radius: 7px;
  pointer-events: none;
}

/* ── Session dots ── */
.indicator-dots {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 0 0 auto;
}

.indicator-dot {
  width: 6px;
  height: 6px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(128, 128, 128, 0.35);
  cursor: pointer;
  transition: background 180ms ease, transform 180ms ease;
}

.indicator-dot.connected {
  background: rgba(128, 128, 128, 0.55);
}

.indicator-dot.active {
  background: var(--color-primary);
  transform: scale(1.5);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 24%, transparent);
}

.indicator-dot:hover {
  transform: scale(1.8);
  background: var(--color-primary);
}

/* ── Failure indicator ── */
.indicator-dot.failed {
  background: var(--color-danger) !important;
  box-shadow: 0 0 4px rgba(255, 77, 79, 0.5);
}

/* ── Lightbar wave ── */
.indicator-lightbar {
  display: flex;
  align-items: flex-end;
  gap: 1px;
  height: 10px;
  flex: 0 0 auto;
  width: 400px;
  max-width: 50vw;
  overflow: hidden;
}

.lightbar-pip {
  flex: 1;
  min-width: 1px;
  height: 100%;
  border-radius: 1px;
  background: linear-gradient(to top, var(--lb-start, var(--app-text-muted)), var(--lb-end, var(--color-primary)));
  transform-origin: bottom;
  transition: transform 80ms linear;
}

/* ── Monitor capsule ── */
.monitor-capsule {
  position: fixed;
  right: 12px;
  bottom: 34px;
  z-index: var(--z-floating);
  min-width: 36px;
  height: 20px;
  padding: 0 8px;
  border-radius: 10px;
  background: var(--app-bg-dialog);
  border: 1px solid var(--app-border-shadow);
  color: var(--mc-color, var(--color-success));
  font-family: 'Consolas', 'Menlo', monospace;
  font-size: 11px;
  font-weight: 600;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: default;
  transition: background 180ms ease, color 180ms ease;
}

.monitor-capsule.expanded {
  height: auto;
  min-width: 140px;
  padding: 6px 10px;
  border-radius: 8px;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.capsule-text {
  white-space: nowrap;
}

.capsule-expand {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 100%;
}

.capsule-row {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  font-size: 11px;
}

.capsule-label {
  color: var(--app-text-muted);
  font-weight: 600;
}

.capsule-value {
  color: inherit;
  font-weight: 700;
}

/* ── Transfer popup ── */
.transfer-popup {
  position: fixed;
  right: 16px;
  bottom: 40px;
  width: 300px;
  max-height: 240px;
  overflow-y: auto;
  background: var(--app-bg-dialog);
  border: 1px solid var(--app-border-shadow, rgba(255,255,255,0.08));
  box-shadow: var(--niri-shadow-dialog);
  padding: 12px;
  z-index: var(--z-popover);
  border-radius: 12px;
}

.transfer-popup-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
  color: var(--app-text);
  border-bottom: 1px solid var(--app-border-shadow, rgba(255,255,255,0.08));
  padding-bottom: 6px;
}

.transfer-popup-items {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.transfer-popup-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  background: var(--app-input-bg);
  padding: 8px;
  border-radius: 6px;
  font-size: 12px;
}

.transfer-item-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.transfer-item-name {
  flex: 1;
  margin-right: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
  color: var(--app-text);
}

.transfer-arrow {
  display: inline-block;
  width: 14px;
  margin-right: 4px;
  text-align: center;
  color: var(--app-text-muted);
}

.transfer-item-size {
  color: var(--app-text-muted);
  white-space: nowrap;
}

.transfer-item-row2 {
  display: flex;
  align-items: center;
  gap: 6px;
}

.transfer-progress-bg {
  flex: 1;
  height: 4px;
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
  border-radius: 2px;
  overflow: hidden;
}

.transfer-progress-bar {
  height: 100%;
  background: var(--color-primary);
  border-radius: 2px;
  transition: width 200ms ease;
}

.transfer-popup-item.is-uploading .transfer-progress-bar {
  background: linear-gradient(90deg, var(--color-primary), var(--app-selection-bg), var(--color-primary));
  background-size: 200% 100%;
  animation: transfer-pulse 1.2s linear infinite;
}

@keyframes transfer-pulse {
  from {
    background-position: 200% 0;
  }

  to {
    background-position: 0 0;
  }
}

.transfer-popup-item.is-done .transfer-progress-bar {
  background: var(--color-success);
}

.transfer-item-pct {
  font-size: 10px;
  color: var(--app-text-muted);
  min-width: 35px;
  text-align: right;
}

.transfer-item-meta {
  display: flex;
  justify-content: space-between;
  font-size: 10px;
  color: var(--app-text-muted);
}

.transfer-item-actions {
  display: flex;
  gap: 4px;
  justify-content: flex-end;
}

.transfer-action-btn {
  border: none;
  background: transparent;
  font-size: 11px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}

.transfer-action-btn.cancel {
  color: var(--color-warning);
}

.transfer-action-btn.cancel:hover {
  background: rgba(245, 158, 11, 0.15);
}

.transfer-action-btn.clear {
  color: var(--app-text-muted);
}

.transfer-action-btn.clear:hover {
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
}

.transfer-popup-empty {
  text-align: center;
  color: var(--app-text-muted);
  font-size: 12px;
  padding: 16px 0;
}
</style>
