import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { useCommandKnowledgeStore } from '@/stores/commandKnowledge';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { invokeCommand } from '../utils/ipc';
import { matchSensitiveCommand, sanitizeCommandText } from '../utils/sensitiveCommand';
import {
  LEGACY_SYNC_INPUT_GROUP_STORAGE_KEY,
  SYNC_INPUT_CHANNELS_STORAGE_KEY,
  buildSyncInputWriteRequest,
  cloneSyncChannels,
  createSyncChannel,
  findChannelBySessionId,
  normalizeSendMode,
  normalizeSourceMode,
  reassignSessionsToChannel,
  reconcileSyncChannels,
} from '../utils/syncInputChannels';

const BROADCAST_MIN_INTERVAL_MS = 35;
const BROADCAST_MAX_PAYLOAD_BYTES = 4096;

export function useInputRouter({ sshStore }) {
  const commandKnowledgeStore = useCommandKnowledgeStore();
  const syncChannels = ref([]);
  const selectedSyncChannelId = ref('');
  const sessionChannelMap = ref({});

  const availableSessionIds = computed(() =>
    (sshStore.sessions || [])
      .map((session) => session?.id)
      .filter(Boolean)
  );

  const connectedSessionIds = computed(() =>
    (sshStore.sessions || [])
      .filter((session) => session.status === 'connected')
      .map((session) => session.id)
  );
  const knowledgeSensitiveRules = computed(() => commandKnowledgeStore.sensitiveRules || []);

  const syncStats = ref({
    attempts: 0,
    failures: 0,
    lastError: '',
    lastFailedTarget: '',
  });

  const pendingPayloadMap = new Map();
  const linePayloadMap = new Map();
  const inputBufferMap = new Map();
  const sourceRouteQueueMap = new Map();
  let flushTimer = null;
  let lastFlushAt = 0;

  const emitSyncInputStats = () => {
    window.dispatchEvent(
      new CustomEvent('sync-input-stats', {
        detail: {
          ...syncStats.value,
        },
      }),
    );
  };

  const resetSyncStats = () => {
    syncStats.value = {
      attempts: 0,
      failures: 0,
      lastError: '',
      lastFailedTarget: '',
    };
    emitSyncInputStats();
  };

  const clearSourceState = (sessionId) => {
    pendingPayloadMap.delete(sessionId);
    linePayloadMap.delete(sessionId);
    inputBufferMap.delete(sessionId);
    sourceRouteQueueMap.delete(sessionId);
  };

  const clearAllSourceState = () => {
    pendingPayloadMap.clear();
    linePayloadMap.clear();
    inputBufferMap.clear();
    sourceRouteQueueMap.clear();
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
  };

  const pruneSourceState = () => {
    const validIds = new Set(availableSessionIds.value);
    for (const map of [pendingPayloadMap, linePayloadMap, inputBufferMap, sourceRouteQueueMap]) {
      for (const sessionId of map.keys()) {
        if (!validIds.has(sessionId)) {
          map.delete(sessionId);
        }
      }
    }
  };

  const emitSyncInputState = () => {
    window.dispatchEvent(
      new CustomEvent('sync-input-changed', {
        detail: {
          syncChannels: syncChannels.value.map((channel) => ({
            ...channel,
            sessionIds: [...(channel.sessionIds || [])],
            connectedIds: [...(channel.connectedIds || [])],
          })),
          selectedChannelId: selectedSyncChannelId.value,
          sessionChannelMap: { ...sessionChannelMap.value },
        },
      }),
    );
  };

  const persistSyncChannels = () => {
    try {
      localStorage.setItem(
        SYNC_INPUT_CHANNELS_STORAGE_KEY,
        JSON.stringify({
          channels: syncChannels.value,
          selectedChannelId: selectedSyncChannelId.value,
        }),
      );
      localStorage.removeItem(LEGACY_SYNC_INPUT_GROUP_STORAGE_KEY);
    } catch {
    }
  };

  const buildMigratedLegacyChannel = () => {
    try {
      const raw = localStorage.getItem(LEGACY_SYNC_INPUT_GROUP_STORAGE_KEY);
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object') return [];
      const sessionIds = Array.isArray(parsed.sessionIds) ? parsed.sessionIds.filter(Boolean) : [];
      if (sessionIds.length === 0) return [];

      return [
        createSyncChannel({
          name: '频道 1',
          enabled: !!parsed.enabled,
          sourceMode: parsed.sourceMode,
          primarySessionId: parsed.primarySessionId,
          sendMode: parsed.sendMode,
          sessionIds,
        }),
      ];
    } catch {
      return [];
    }
  };

  const reconcileAndPersist = ({ preferredSelectedId, resetStats: shouldResetStats = false } = {}) => {
    const { channels, sessionChannelMap: nextChannelMap } = reconcileSyncChannels(syncChannels.value, {
      sessionIds: availableSessionIds.value,
      connectedSessionIds: connectedSessionIds.value,
    });

    syncChannels.value = channels;
    sessionChannelMap.value = nextChannelMap;

    const preferredId = String(preferredSelectedId || selectedSyncChannelId.value || '').trim();
    selectedSyncChannelId.value = channels.some((channel) => channel.id === preferredId)
      ? preferredId
      : (channels[0]?.id || '');

    pruneSourceState();
    if (channels.length === 0) {
      clearAllSourceState();
    }
    if (shouldResetStats) {
      resetSyncStats();
    }

    persistSyncChannels();
    emitSyncInputState();
  };

  const loadSyncChannels = () => {
    try {
      const raw = localStorage.getItem(SYNC_INPUT_CHANNELS_STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed === 'object') {
          syncChannels.value = cloneSyncChannels(parsed.channels);
          selectedSyncChannelId.value = String(parsed.selectedChannelId || '').trim();
          return;
        }
      }
    } catch {
    }

    syncChannels.value = buildMigratedLegacyChannel();
    selectedSyncChannelId.value = syncChannels.value[0]?.id || '';
  };

  const replaceSyncChannels = (nextChannels = [], preferredSelectedId = '') => {
    syncChannels.value = cloneSyncChannels(nextChannels);
    reconcileAndPersist({
      preferredSelectedId,
      resetStats: true,
    });
  };

  const clearSyncChannels = () => {
    syncChannels.value = [];
    selectedSyncChannelId.value = '';
    sessionChannelMap.value = {};
    clearAllSourceState();
    resetSyncStats();
    persistSyncChannels();
    emitSyncInputState();
  };

  const setSelectedSyncChannelId = (channelId) => {
    const nextId = String(channelId || '').trim();
    selectedSyncChannelId.value = syncChannels.value.some((channel) => channel.id === nextId)
      ? nextId
      : (syncChannels.value[0]?.id || '');
    persistSyncChannels();
    emitSyncInputState();
  };

  const shouldBroadcastPayload = (payload, sendMode) => {
    if (sendMode !== 'line') return true;
    return payload.includes('\r') || payload.includes('\n') || payload.includes('\x03');
  };

  const parseExecutedCommands = (sessionId, payload) => {
    const commands = [];
    const text = String(payload || '');
    let buffer = inputBufferMap.get(sessionId) || '';

    for (let index = 0; index < text.length; index += 1) {
      const ch = text[index];

      if (ch === '\u0003') {
        buffer = '';
        continue;
      }

      if (ch === '\u0015') {
        buffer = '';
        continue;
      }

      if (ch === '\u007F' || ch === '\b') {
        buffer = buffer.slice(0, -1);
        continue;
      }

      if (ch === '\r' || ch === '\n') {
        const normalized = sanitizeCommandText(buffer);
        if (normalized) {
          commands.push(normalized);
        }
        buffer = '';
        continue;
      }

      if (ch === '\u001B') {
        continue;
      }

      buffer += ch;
    }

    const capped = buffer.length > BROADCAST_MAX_PAYLOAD_BYTES
      ? buffer.slice(buffer.length - BROADCAST_MAX_PAYLOAD_BYTES)
      : buffer;
    inputBufferMap.set(sessionId, capped);
    return commands;
  };

  const confirmSensitiveBroadcast = (matched, targetCount) =>
    new Promise((resolve) => {
      const isCritical = matched?.severity === 'critical';
      const title = isCritical ? '高危命令同步确认' : '敏感命令同步确认';
      const content = isCritical
        ? `检测到高危命令“${matched.content}”。该命令将同步到 ${targetCount} 个目标会话，是否继续？`
        : `检测到敏感命令“${matched.content}”。该命令将同步到 ${targetCount} 个目标会话，是否继续？`;

      confirm({
        title,
        content,
        okText: '确认同步',
        cancelText: '取消',
        okButtonProps: isCritical ? { danger: true } : {},
        onOk: () => resolve(true),
        onCancel: () => resolve(false),
      });
    });

  const shouldBlockSensitiveBroadcast = async (sourceSessionId, targets, payload) => {
    if (!targets?.length) return false;
    if (!String(payload || '').includes('\r') && !String(payload || '').includes('\n')) {
      return false;
    }

    const executed = parseExecutedCommands(sourceSessionId, payload);
    if (!executed.length) return false;

    for (const command of executed) {
      const matched = matchSensitiveCommand(command, knowledgeSensitiveRules.value);
      if (!matched) continue;

      const confirmed = await confirmSensitiveBroadcast(matched, targets.length);
      if (!confirmed) {
        syncStats.value = {
          ...syncStats.value,
          failures: syncStats.value.failures + targets.length,
          lastError: '用户取消了敏感命令同步',
          lastFailedTarget: targets[0] || '',
        };
        emitSyncInputStats();
        toast.warning('已阻止敏感命令同步执行');
        window.dispatchEvent(
          new CustomEvent('sync-input-guard-blocked', {
            detail: {
              sourceSessionId,
              command: matched.content,
              severity: matched.severity,
              targetCount: targets.length,
            },
          }),
        );
        return true;
      }
    }

    return false;
  };

  const resolveRoutingChannel = (sessionId) => {
    const channelId = sessionChannelMap.value[String(sessionId || '').trim()];
    if (!channelId) return null;
    return syncChannels.value.find((channel) => channel.id === channelId) || null;
  };

  const writeSessionInput = (sessionId, data) => {
    const session = (sshStore.sessions || []).find((candidate) => candidate.id === sessionId) || { id: sessionId };
    const request = buildSyncInputWriteRequest(session, data);
    return invokeCommand(request.command, request.args);
  };

  const broadcastOnce = async (sourceSessionId, payload, explicitTargets = null) => {
    const channel = resolveRoutingChannel(sourceSessionId);
    if (!channel?.broadcastEnabled) return;

    const targets = Array.isArray(explicitTargets)
      ? explicitTargets.filter((id) => id && id !== sourceSessionId)
      : (channel.connectedIds || []).filter((id) => id !== sourceSessionId);
    if (!targets.length) return;

    syncStats.value = {
      ...syncStats.value,
      attempts: syncStats.value.attempts + targets.length,
    };

    await Promise.all(
      targets.map(async (sessionId) => {
        try {
          await writeSessionInput(sessionId, payload);
        } catch (error) {
          const msg = String(error || 'broadcast failed');
          syncStats.value = {
            ...syncStats.value,
            failures: syncStats.value.failures + 1,
            lastError: msg,
            lastFailedTarget: sessionId,
          };
          window.dispatchEvent(
            new CustomEvent('sync-input-error', {
              detail: {
                sourceSessionId,
                targetSessionId: sessionId,
                error: msg,
              },
            }),
          );
        }
      }),
    );

    emitSyncInputStats();
  };

  const flushBroadcastQueue = async () => {
    flushTimer = null;
    if (!pendingPayloadMap.size) return;

    const now = Date.now();
    const elapsed = now - lastFlushAt;
    if (elapsed < BROADCAST_MIN_INTERVAL_MS) {
      flushTimer = setTimeout(flushBroadcastQueue, BROADCAST_MIN_INTERVAL_MS - elapsed);
      return;
    }

    const entries = Array.from(pendingPayloadMap.entries());
    pendingPayloadMap.clear();
    lastFlushAt = Date.now();

    for (const [sourceSessionId, payload] of entries) {
      await broadcastOnce(sourceSessionId, payload);
    }

    if (pendingPayloadMap.size && !flushTimer) {
      flushTimer = setTimeout(flushBroadcastQueue, BROADCAST_MIN_INTERVAL_MS);
    }
  };

  const scheduleFlush = () => {
    if (flushTimer) return;
    flushTimer = setTimeout(flushBroadcastQueue, BROADCAST_MIN_INTERVAL_MS);
  };

  const enqueuePayload = (sourceSessionId, payload) => {
    if (!sourceSessionId || typeof payload !== 'string' || !payload.length) return;
    const existing = pendingPayloadMap.get(sourceSessionId) || '';
    const merged = `${existing}${payload}`;
    const limited = merged.length > BROADCAST_MAX_PAYLOAD_BYTES
      ? merged.slice(merged.length - BROADCAST_MAX_PAYLOAD_BYTES)
      : merged;
    pendingPayloadMap.set(sourceSessionId, limited);
  };

  const resolveBroadcastPayload = (sourceSessionId, payload, sendMode) => {
    if (sendMode !== 'line') return payload;

    const existing = linePayloadMap.get(sourceSessionId) || '';
    const merged = `${existing}${payload}`;
    if (!shouldBroadcastPayload(payload, sendMode)) {
      const capped = merged.length > BROADCAST_MAX_PAYLOAD_BYTES
        ? merged.slice(merged.length - BROADCAST_MAX_PAYLOAD_BYTES)
        : merged;
      linePayloadMap.set(sourceSessionId, capped);
      return '';
    }

    linePayloadMap.delete(sourceSessionId);
    return merged;
  };

  const enqueueSourceRouteTask = (sourceSessionId, task) => {
    const previous = sourceRouteQueueMap.get(sourceSessionId) || Promise.resolve();
    const current = previous
      .catch(() => {
      })
      .then(task);

    sourceRouteQueueMap.set(sourceSessionId, current);

    current.finally(() => {
      if (sourceRouteQueueMap.get(sourceSessionId) === current) {
        sourceRouteQueueMap.delete(sourceSessionId);
      }
    });

    return current;
  };

  const canRouteSourceWithSync = (sourceSessionId) => {
    const channel = resolveRoutingChannel(sourceSessionId);
    if (!channel?.broadcastEnabled) return false;

    if (channel.sourceMode !== 'primary') {
      return true;
    }

    return String(channel.primarySessionId || '').trim() === String(sourceSessionId || '').trim();
  };

  const writeSourceInput = async (sourceSessionId, payload) => {
    if (!sourceSessionId || typeof payload !== 'string' || !payload.length) {
      return;
    }

    try {
      await writeSessionInput(sourceSessionId, payload);
    } catch (error) {
      const msg = String(error || 'write source failed');
      syncStats.value = {
        ...syncStats.value,
        failures: syncStats.value.failures + 1,
        lastError: msg,
        lastFailedTarget: sourceSessionId,
      };
      emitSyncInputStats();
    }
  };

  const routeBroadcastInput = async (sourceSessionId, payload) => {
    if (!sourceSessionId || !payload || typeof payload !== 'string') return false;

    const channel = resolveRoutingChannel(sourceSessionId);
    if (!channel?.broadcastEnabled) return false;
    if (!canRouteSourceWithSync(sourceSessionId)) return false;

    const targets = (channel.connectedIds || []).filter((id) => id !== sourceSessionId);
    const resolvedPayload = resolveBroadcastPayload(sourceSessionId, payload, normalizeSendMode(channel.sendMode));

    if (!resolvedPayload) {
      await writeSourceInput(sourceSessionId, payload);
      return true;
    }

    const guardTargets = [sourceSessionId, ...targets];
    const blocked = await shouldBlockSensitiveBroadcast(sourceSessionId, guardTargets, resolvedPayload);
    if (blocked) {
      return true;
    }

    await writeSourceInput(sourceSessionId, payload);

    if (!targets.length) {
      return true;
    }

    enqueuePayload(sourceSessionId, resolvedPayload);
    scheduleFlush();
    return true;
  };

  const onInputRoute = (event) => {
    const panelId = event?.detail?.panelId;
    const payload = event?.detail?.payload;
    const respond = event?.detail?.respond;

    if (!canRouteSourceWithSync(panelId)) {
      if (typeof respond === 'function') respond({ handled: false });
      return;
    }

    if (event?.detail && typeof event.detail === 'object') {
      event.detail.handledByRouter = true;
    }

    enqueueSourceRouteTask(panelId, () => routeBroadcastInput(panelId, payload))
      .then((handled) => {
        if (typeof respond === 'function') respond({ handled: !!handled });
      })
      .catch(() => {
        if (typeof respond === 'function') respond({ handled: false });
      });
  };

  const onEmergencyStop = () => {
    clearSyncChannels();
  };

  const onBeforeUnload = () => {
    clearSyncChannels();
  };

  onMounted(() => {
    loadSyncChannels();
    reconcileAndPersist();
    emitSyncInputStats();

    window.addEventListener('terminal-input-route', onInputRoute);
    window.addEventListener('app:sync-input-stop', onEmergencyStop);
    window.addEventListener('beforeunload', onBeforeUnload);
  });

  onUnmounted(() => {
    window.removeEventListener('terminal-input-route', onInputRoute);
    window.removeEventListener('app:sync-input-stop', onEmergencyStop);
    window.removeEventListener('beforeunload', onBeforeUnload);
  });

  watch(
    () => `${availableSessionIds.value.join('|')}::${connectedSessionIds.value.join('|')}`,
    () => {
      reconcileAndPersist();
    },
  );

  return {
    syncChannels,
    selectedSyncChannelId,
    connectedSessionIds,
    replaceSyncChannels,
    clearSyncChannels,
    setSelectedSyncChannelId,
    findChannelBySessionId: (sessionId) => findChannelBySessionId(syncChannels.value, sessionId),
  };
}
