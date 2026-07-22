export const SYNC_INPUT_CHANNELS_STORAGE_KEY = 'sync-input-channels-v1';
export const LEGACY_SYNC_INPUT_GROUP_STORAGE_KEY = 'sync-input-group-v1';

const DEFAULT_CHANNEL_NAME_PREFIX = '频道';

const fallbackId = () => `sync-channel-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

const createId = () => {
  try {
    if (globalThis?.crypto?.randomUUID) {
      return globalThis.crypto.randomUUID();
    }
  } catch {
  }
  return fallbackId();
};

const uniqueStrings = (values = []) => {
  const normalized = [];
  const seen = new Set();
  for (const value of values) {
    const next = String(value || '').trim();
    if (!next || seen.has(next)) continue;
    seen.add(next);
    normalized.push(next);
  }
  return normalized;
};

export const normalizeSourceMode = (value) => (value === 'primary' ? 'primary' : 'all');
export const normalizeSendMode = (value) => (value === 'line' ? 'line' : 'realtime');

export const buildSyncInputWriteRequest = (session, data) => {
  const sessionId = String(session?.id || '').trim();
  const rootSessionId = String(session?.workspaceSessionId || session?.parentId || '').trim();
  if (session?.isSplitChild && rootSessionId) {
    return {
      command: 'write_ssh_shell_channel',
      args: { rootSessionId, channelId: sessionId, data },
    };
  }
  return {
    command: 'write_ssh',
    args: { sessionId, data },
  };
};

export const createSyncChannelName = (index = 1) => `${DEFAULT_CHANNEL_NAME_PREFIX} ${Math.max(1, Number(index) || 1)}`;

export const createSyncChannel = (overrides = {}) => ({
  id: String(overrides.id || createId()),
  name: String(overrides.name || '').trim() || createSyncChannelName(1),
  enabled: overrides.enabled !== false,
  sourceMode: normalizeSourceMode(overrides.sourceMode),
  primarySessionId: String(overrides.primarySessionId || '').trim(),
  sendMode: normalizeSendMode(overrides.sendMode),
  sessionIds: uniqueStrings(overrides.sessionIds),
  connectedIds: uniqueStrings(overrides.connectedIds),
  connectedCount: Math.max(0, Number(overrides.connectedCount) || 0),
  broadcastEnabled: !!overrides.broadcastEnabled,
});

export const cloneSyncChannels = (channels = []) =>
  (Array.isArray(channels) ? channels : []).map((channel) => createSyncChannel(channel));

export const reassignSessionsToChannel = (channels = [], targetChannelId, sessionIds = []) => {
  const targetId = String(targetChannelId || '').trim();
  const reassignedIds = uniqueStrings(sessionIds);
  const movedSet = new Set(reassignedIds);

  return cloneSyncChannels(channels).map((channel) => {
    if (channel.id === targetId) {
      return createSyncChannel({
        ...channel,
        sessionIds: reassignedIds,
      });
    }

    return createSyncChannel({
      ...channel,
      sessionIds: channel.sessionIds.filter((sessionId) => !movedSet.has(sessionId)),
    });
  });
};

export const reconcileSyncChannels = (channels = [], { sessionIds = [], connectedSessionIds = [] } = {}) => {
  const knownSessionIds = new Set(uniqueStrings(sessionIds));
  const connectedSet = new Set(uniqueStrings(connectedSessionIds));
  const seenSessionIds = new Set();
  const sessionChannelMap = {};

  const normalizedChannels = cloneSyncChannels(channels).map((channel) => {
    const nextSessionIds = [];

    for (const sessionId of uniqueStrings(channel.sessionIds)) {
      if (!knownSessionIds.has(sessionId) || seenSessionIds.has(sessionId)) continue;
      seenSessionIds.add(sessionId);
      sessionChannelMap[sessionId] = channel.id;
      nextSessionIds.push(sessionId);
    }

    const connectedIds = nextSessionIds.filter((sessionId) => connectedSet.has(sessionId));
    let primarySessionId = '';

    if (channel.sourceMode === 'primary') {
      const candidate = String(channel.primarySessionId || '').trim();
      if (candidate && connectedIds.includes(candidate)) {
        primarySessionId = candidate;
      } else if (connectedIds.length > 0) {
        primarySessionId = connectedIds[0];
      } else if (candidate && nextSessionIds.includes(candidate)) {
        primarySessionId = candidate;
      } else {
        primarySessionId = nextSessionIds[0] || '';
      }
    }

    const connectedCount = connectedIds.length;
    return createSyncChannel({
      ...channel,
      primarySessionId,
      sessionIds: nextSessionIds,
      connectedIds,
      connectedCount,
      broadcastEnabled: channel.enabled !== false && connectedCount >= 2,
    });
  }).filter((channel) => channel.sessionIds.length > 0);

  return {
    channels: normalizedChannels,
    sessionChannelMap,
  };
};

export const findChannelBySessionId = (channels = [], sessionId) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return null;
  return cloneSyncChannels(channels).find((channel) => channel.sessionIds.includes(targetId)) || null;
};

export const getSessionSyncBadgeState = (channels = [], sessionId) => {
  const channel = findChannelBySessionId(channels, sessionId);
  if (!channel) {
    return {
      visible: false,
      channelId: '',
      channelName: '',
      connectedCount: 0,
      isPrimary: false,
      sourceMode: 'all',
      sendMode: 'realtime',
      broadcastEnabled: false,
    };
  }

  return {
    visible: true,
    channelId: channel.id,
    channelName: channel.name,
    connectedCount: Number(channel.connectedCount) || 0,
    isPrimary: channel.sourceMode === 'primary' && String(channel.primarySessionId || '').trim() === String(sessionId || '').trim(),
    sourceMode: channel.sourceMode,
    sendMode: channel.sendMode,
    broadcastEnabled: !!channel.broadcastEnabled,
  };
};
