import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

const TELEMETRY_WINDOW_MS = 1500;
const TELEMETRY_MIN_SAMPLE_MS = 150;

const nowMs = () => (typeof performance !== 'undefined' && typeof performance.now === 'function'
  ? performance.now()
  : Date.now());

const taskKey = (sessionId, requestId) => `${sessionId || ''}\u0000${requestId || ''}`;

function updateTelemetry(task, payload) {
  const current = Number(payload.current || 0);
  const total = Number(payload.total || 0);
  const stamp = nowMs();

  if (payload.status === 'failed' || payload.status === 'waiting') {
    task.rate = 0;
    task.etaSeconds = null;
    task.telemetrySamples = [{ at: stamp, bytes: current }];
    task.lastSampleAt = stamp;
    task.lastSampleBytes = current;
    return;
  }

  let samples = Array.isArray(task.telemetrySamples) ? [...task.telemetrySamples] : [];
  const lastSample = samples[samples.length - 1];
  if (!lastSample || current < Number(lastSample.bytes || 0)) {
    samples = [{ at: stamp, bytes: current }];
  } else if (
    current !== Number(lastSample.bytes || 0)
    || stamp - Number(lastSample.at || 0) >= TELEMETRY_MIN_SAMPLE_MS
  ) {
    samples.push({ at: stamp, bytes: current });
  }

  const cutoff = stamp - TELEMETRY_WINDOW_MS;
  const firstValidIndex = samples.findIndex((sample) => Number(sample.at || 0) >= cutoff);
  task.telemetrySamples = firstValidIndex > 0 ? samples.slice(firstValidIndex - 1) : samples;
  task.lastSampleAt = stamp;
  task.lastSampleBytes = current;

  const first = task.telemetrySamples[0];
  const last = task.telemetrySamples[task.telemetrySamples.length - 1];
  const deltaBytes = Number(last?.bytes || 0) - Number(first?.bytes || 0);
  const deltaMs = Number(last?.at || 0) - Number(first?.at || 0);
  const measuredRate = deltaBytes > 0 && deltaMs > 0 ? deltaBytes / (deltaMs / 1000) : 0;
  task.rate = measuredRate > 0 ? measuredRate : (payload.status === 'success' ? Number(task.rate || 0) : 0);

  if (task.rate > 0 && total > current) {
    task.etaSeconds = Math.ceil((total - current) / task.rate);
  } else if (total > 0 && total <= current) {
    task.etaSeconds = 0;
  } else {
    task.etaSeconds = null;
  }
}

export const useSftpTransfersStore = defineStore('sftpTransfers', () => {
  const tasks = ref([]);

  const activeCount = computed(() => tasks.value.filter(
    (task) => task.status === 'uploading' || task.status === 'waiting' || task.status === 'cancelling',
  ).length);

  const dockStatus = computed(() => ({
    active: activeCount.value,
    total: tasks.value.length,
    lastName: tasks.value[0]?.fileName || '',
    items: tasks.value.map((task) => ({
      id: task.id,
      sessionId: task.sessionId,
      name: task.fileName,
      direction: task.direction,
      loaded: Number(task.current || 0),
      total: Number(task.total || 0),
      progress: Number(task.percent || 0),
      rate: Number(task.rate || 0),
      etaSeconds: Number.isFinite(task.etaSeconds) ? Number(task.etaSeconds) : null,
      status: task.status,
      error: task.error || '',
    })),
  }));

  function findTask(sessionId, requestId) {
    const key = taskKey(sessionId, requestId);
    return tasks.value.find((task) => taskKey(task.sessionId, task.id) === key);
  }

  function createTask({
    id,
    sessionId,
    direction,
    fileName,
    localPath = '',
    remotePath = '',
  }) {
    if (!sessionId) throw new Error('SFTP transfer sessionId is required');
    const prefix = direction === 'download' ? 'down' : 'up';
    const requestId = id || `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    const existing = findTask(sessionId, requestId);
    if (existing) return existing;

    tasks.value.unshift({
      id: requestId,
      sessionId,
      fileName: fileName || 'unknown',
      direction,
      localPath,
      remotePath,
      current: 0,
      total: 0,
      percent: 0,
      rate: 0,
      etaSeconds: null,
      lastSampleAt: 0,
      lastSampleBytes: 0,
      telemetrySamples: [],
      status: 'waiting',
      error: '',
    });
    return tasks.value[0];
  }

  function applyProgress(payload = {}) {
    const sessionId = payload.sessionId || payload.session_id || '';
    let task = sessionId ? findTask(sessionId, payload.id) : undefined;
    if (!task && !sessionId) {
      const matches = tasks.value.filter((item) => item.id === payload.id);
      if (matches.length === 1) task = matches[0];
    }
    if (!task) return false;

    const cancellationPending = task.status === 'cancelling';
    task.current = Number(payload.current || 0);
    task.total = Number(payload.total || 0);
    task.percent = Number(payload.percent || 0);
    task.direction = payload.direction || task.direction;
    updateTelemetry(task, payload);

    if (payload.status === 'failed') {
      task.status = 'failed';
      task.error = String(payload.error || `${task.direction === 'download' ? '下载' : '上传'}失败`);
    } else if (payload.status === 'cancelled') {
      task.status = 'cancelled';
      task.error = String(payload.error || '已取消');
      task.rate = 0;
      task.etaSeconds = null;
    } else if (payload.status === 'success') {
      task.status = 'success';
    } else if (payload.status && !(cancellationPending && payload.status === 'uploading')) {
      task.status = payload.status;
    }
    return true;
  }

  function removeTask(sessionId, requestId) {
    const key = taskKey(sessionId, requestId);
    const index = tasks.value.findIndex((task) => taskKey(task.sessionId, task.id) === key);
    if (index >= 0) tasks.value.splice(index, 1);
  }

  function requestCancel(sessionId, requestId) {
    const task = findTask(sessionId, requestId);
    if (!task) return 'missing';
    if (task.status === 'waiting') {
      task.status = 'cancelled';
      task.error = '已取消';
      return 'local';
    }
    if (task.status === 'uploading') {
      task.status = 'cancelling';
      task.error = '';
      return 'remote';
    }
    return 'ignored';
  }

  return {
    tasks,
    activeCount,
    dockStatus,
    createTask,
    findTask,
    applyProgress,
    removeTask,
    requestCancel,
  };
});
