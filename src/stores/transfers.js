import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

const TELEMETRY_WINDOW_MS = 1500;
const TELEMETRY_MIN_SAMPLE_MS = 150;
const TELEMETRY_WARMUP_MS = 750;
const MAX_FINISHED_TASKS = 200;
const ACTIVE_STATUSES = new Set([
  'waiting',
  'negotiating',
  'uploading',
  'transferring',
  'finalizing',
  'cancelling',
]);
const CLEARABLE_STATUSES = new Set(['success', 'cancelled', 'skipped']);
const DATA_TRANSFER_STATUSES = new Set(['uploading', 'transferring']);
const TERMINAL_STATUSES = new Set(['success', 'failed', 'cancelled', 'skipped']);

const nowMs = () => (typeof performance !== 'undefined' && typeof performance.now === 'function'
  ? performance.now()
  : Date.now());
const taskKey = (sessionId, requestId) => `${sessionId || ''}\u0000${requestId || ''}`;

function updateTelemetry(task, payload) {
  const current = Number(payload.current || 0);
  const total = Number(payload.total || 0);
  const stamp = nowMs();

  if (['failed', 'skipped', 'waiting', 'negotiating', 'finalizing'].includes(payload.status)) {
    task.rate = 0;
    task.etaSeconds = null;
    task.telemetrySamples = [{ at: stamp, bytes: current }];
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

  const first = task.telemetrySamples[0];
  const last = task.telemetrySamples[task.telemetrySamples.length - 1];
  const deltaBytes = Number(last?.bytes || 0) - Number(first?.bytes || 0);
  const deltaMs = Number(last?.at || 0) - Number(first?.at || 0);
  const measuredRate = deltaBytes > 0 && deltaMs >= TELEMETRY_WARMUP_MS
    ? deltaBytes / (deltaMs / 1000)
    : 0;
  task.rate = measuredRate > 0
    ? (Number(task.rate || 0) > 0 ? (Number(task.rate) * 0.65) + (measuredRate * 0.35) : measuredRate)
    : (payload.status === 'success' ? Number(task.rate || 0) : 0);
  task.etaSeconds = task.rate > 0 && total > current
    ? Math.ceil((total - current) / task.rate)
    : (total > 0 && total <= current ? 0 : null);
}

function updatePhaseTimings(task, status) {
  if (!status) return;
  const stamp = nowMs();

  if (DATA_TRANSFER_STATUSES.has(status) && !Number.isFinite(task.transferStartedAt)) {
    task.transferStartedAt = stamp;
  }

  if (status === 'finalizing') {
    if (!Number.isFinite(task.transferStartedAt)) task.transferStartedAt = stamp;
    if (!Number.isFinite(task.transferElapsedMs)) {
      task.transferElapsedMs = Math.max(0, stamp - task.transferStartedAt);
    }
    if (!Number.isFinite(task.finalizingStartedAt)) task.finalizingStartedAt = stamp;
  }

  if (TERMINAL_STATUSES.has(status)) {
    if (!Number.isFinite(task.transferElapsedMs) && Number.isFinite(task.transferStartedAt)) {
      const transferEndedAt = Number.isFinite(task.finalizingStartedAt)
        ? task.finalizingStartedAt
        : stamp;
      task.transferElapsedMs = Math.max(0, transferEndedAt - task.transferStartedAt);
    }
    if (Number.isFinite(task.finalizingStartedAt) && !Number.isFinite(task.finalizingElapsedMs)) {
      task.finalizingElapsedMs = Math.max(0, stamp - task.finalizingStartedAt);
    }
  }
}

export const useTransfersStore = defineStore('transfers', () => {
  const tasks = ref([]);
  const terminalRequests = ref([]);
  const panelOpenRequestVersion = ref(0);

  const isTaskActive = (task) => ACTIVE_STATUSES.has(task.status)
    || (task.protocol === 'zmodem' && !task.terminalRestored);
  const activeCount = computed(() => tasks.value.filter(isTaskActive).length);
  const dockStatus = computed(() => ({
    active: activeCount.value,
    total: tasks.value.length,
    lastName: tasks.value[0]?.fileName || '',
    items: tasks.value.map((task) => ({
      id: task.id,
      operationId: task.operationId,
      sessionId: task.sessionId,
      workspaceSessionId: task.workspaceSessionId,
      channelId: task.channelId,
      protocol: task.protocol,
      name: task.fileName,
      direction: task.direction,
      localPath: task.localPath || '',
      remotePath: task.remotePath || '',
      loaded: Number(task.current || 0),
      total: Number(task.total || 0),
      progress: Number(task.percent || 0),
      rate: Number(task.rate || 0),
      etaSeconds: Number.isFinite(task.etaSeconds) ? Number(task.etaSeconds) : null,
      transferStartedAt: Number.isFinite(task.transferStartedAt) ? Number(task.transferStartedAt) : null,
      finalizingStartedAt: Number.isFinite(task.finalizingStartedAt) ? Number(task.finalizingStartedAt) : null,
      transferElapsedMs: Number.isFinite(task.transferElapsedMs) ? Number(task.transferElapsedMs) : null,
      finalizingElapsedMs: Number.isFinite(task.finalizingElapsedMs) ? Number(task.finalizingElapsedMs) : null,
      status: task.status,
      phase: task.phase || '',
      terminalRestored: task.terminalRestored,
      error: task.error || '',
    })),
  }));

  function findTask(sessionId, requestId) {
    const key = taskKey(sessionId, requestId);
    return tasks.value.find((task) => taskKey(task.sessionId, task.id) === key);
  }

  function requestPanelOpen() {
    panelOpenRequestVersion.value += 1;
  }

  function createTask({
    id,
    operationId,
    sessionId,
    workspaceSessionId = sessionId,
    channelId = null,
    protocol = 'sftp',
    direction,
    fileName,
    localPath = '',
    remotePath = '',
  }) {
    if (!sessionId) throw new Error('Transfer sessionId is required');
    const prefix = direction === 'download' ? 'down' : 'up';
    const requestId = id || `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
    const existing = findTask(sessionId, requestId);
    if (existing) return existing;

    tasks.value.unshift({
      id: requestId,
      operationId: operationId || requestId,
      sessionId,
      workspaceSessionId,
      channelId,
      protocol,
      fileName: fileName || 'unknown',
      direction,
      localPath,
      remotePath,
      current: 0,
      total: 0,
      percent: 0,
      rate: 0,
      etaSeconds: null,
      telemetrySamples: [],
      transferStartedAt: null,
      finalizingStartedAt: null,
      transferElapsedMs: null,
      finalizingElapsedMs: null,
      status: 'waiting',
      phase: 'waiting',
      terminalRestored: protocol !== 'zmodem',
      error: '',
    });
    requestPanelOpen();
    return tasks.value[0];
  }

  function applyProgress(payload = {}) {
    const sessionId = payload.sessionId || payload.session_id || '';
    const requestId = payload.taskId || payload.task_id || payload.id || '';
    let task = sessionId && requestId ? findTask(sessionId, requestId) : undefined;
    if (!task && !sessionId && requestId) {
      const matches = tasks.value.filter((item) => item.id === requestId);
      if (matches.length === 1) task = matches[0];
    }
    if (!task && sessionId && requestId && payload.protocol === 'zmodem') {
      task = createTask({
        id: requestId,
        operationId: payload.operationId || payload.operation_id,
        sessionId,
        workspaceSessionId: payload.workspaceSessionId || payload.workspace_session_id || sessionId,
        channelId: payload.channelId ?? payload.channel_id ?? null,
        protocol: 'zmodem',
        direction: payload.direction,
        fileName: payload.fileName || payload.file_name,
        localPath: payload.localPath || payload.local_path || '',
        remotePath: payload.remotePath || payload.remote_path || '',
      });
    }
    if (!task) return false;

    const cancellationPending = task.status === 'cancelling';
    task.current = Number(payload.current || 0);
    task.total = Number(payload.total || 0);
    task.percent = Number(payload.percent || 0);
    task.direction = payload.direction || task.direction;
    task.phase = payload.phase || task.phase;
    task.localPath = payload.localPath || payload.local_path || task.localPath;
    task.remotePath = payload.remotePath || payload.remote_path || task.remotePath;
    if (typeof payload.terminalRestored === 'boolean') {
      task.terminalRestored = payload.terminalRestored;
    }
    updatePhaseTimings(task, payload.status);
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
    } else if (payload.status === 'skipped') {
      task.status = 'skipped';
      task.error = String(payload.error || '远端已跳过');
    } else if (payload.status && !(cancellationPending && ACTIVE_STATUSES.has(payload.status))) {
      task.status = payload.status;
    }
    if (['success', 'failed', 'cancelled', 'skipped'].includes(task.status)) {
      pruneFinishedTasks();
    }
    return true;
  }

  function registerTerminalRequest(request = {}) {
    if (!request.requestId || !request.sessionId) return;
    terminalRequests.value = [
      ...terminalRequests.value.filter((item) => item.requestId !== request.requestId),
      request,
    ];
  }

  function finishTerminalTransfer(payload = {}) {
    terminalRequests.value = terminalRequests.value.filter((request) => {
      if (payload.requestId) return request.requestId !== payload.requestId;
      if (payload.sessionId) return request.sessionId !== payload.sessionId;
      return true;
    });
    tasks.value.forEach((task) => {
      if (
        task.protocol === 'zmodem'
        && ((payload.operationId && task.operationId === payload.operationId)
          || (!payload.operationId && payload.sessionId && task.sessionId === payload.sessionId))
      ) {
        task.terminalRestored = payload.terminalRestored !== false;
      }
    });
    pruneFinishedTasks();
  }

  function isTerminalOwned(sessionId) {
    if (!sessionId) return false;
    return terminalRequests.value.some((request) => request.sessionId === sessionId)
      || tasks.value.some((task) => (
        task.protocol === 'zmodem'
        && task.sessionId === sessionId
        && isTaskActive(task)
      ));
  }

  function removeTask(sessionId, requestId) {
    const key = taskKey(sessionId, requestId);
    const index = tasks.value.findIndex((task) => taskKey(task.sessionId, task.id) === key);
    if (index >= 0) tasks.value.splice(index, 1);
  }

  function clearFinishedTasks() {
    tasks.value = tasks.value.filter((task) => (
      !CLEARABLE_STATUSES.has(task.status)
      || (task.protocol === 'zmodem' && !task.terminalRestored)
    ));
  }

  function pruneFinishedTasks() {
    let finished = 0;
    tasks.value = tasks.value.filter((task) => {
      if (isTaskActive(task)) return true;
      finished += 1;
      return finished <= MAX_FINISHED_TASKS;
    });
  }

  function requestCancel(sessionId, requestId) {
    const task = findTask(sessionId, requestId);
    if (!task) return 'missing';
    if (task.status === 'waiting' && task.protocol === 'sftp') {
      task.status = 'cancelled';
      task.error = '已取消';
      return 'local';
    }
    if (ACTIVE_STATUSES.has(task.status)) {
      task.status = 'cancelling';
      task.error = '';
      return 'remote';
    }
    return 'ignored';
  }

  return {
    tasks,
    terminalRequests,
    panelOpenRequestVersion,
    activeCount,
    dockStatus,
    createTask,
    findTask,
    applyProgress,
    registerTerminalRequest,
    finishTerminalTransfer,
    isTerminalOwned,
    removeTask,
    clearFinishedTasks,
    requestCancel,
    requestPanelOpen,
  };
});
