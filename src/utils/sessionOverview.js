const normalizeComparable = (value) => String(value || '').trim().toLowerCase();
const getSessionConfig = (session) => session?.config || session || {};

export const getSessionSourceKey = (session) => {
  const config = getSessionConfig(session);
  const configId = String(config.id || session?.configId || session?.profileId || '').trim();
  if (configId) return `config:${configId}`;

  const protocol = String(config.protocol || session?.protocol || 'ssh').toLowerCase();
  return [
    protocol,
    normalizeComparable(config.username || session?.username),
    normalizeComparable(config.host || session?.host),
    Number(config.port || session?.port || (protocol === 'telnet' ? 23 : protocol === 'ssh' ? 22 : 0)),
    normalizeComparable(config.serial_path || session?.serial_path),
    Number(config.baud_rate || session?.baud_rate || 0),
    normalizeComparable(config.local_profile || session?.local_profile),
  ].join('\u0000');
};

export const getSessionBaseDisplayName = (session) => {
  const config = getSessionConfig(session);
  const configuredName = String(session?.name || config.name || '').trim();
  if (configuredName) return configuredName;

  const protocol = String(config.protocol || session?.protocol || 'ssh').toLowerCase();
  if (protocol === 'serial') {
    return String(config.serial_path || session?.serial_path || '串口会话').trim();
  }
  if (protocol === 'local') {
    return String(config.local_shell_name || session?.local_shell_name || '本地终端').trim();
  }

  const host = String(config.host || session?.host || '').trim();
  const username = String(config.username || session?.username || '').trim();
  return host ? `${username ? `${username}@` : ''}${host}` : '未命名会话';
};

export const ensureSessionDisplayMetadata = (sessions = []) => {
  const sessionsBySource = new Map();

  for (const session of Array.isArray(sessions) ? sessions : []) {
    if (!session?.id || session.isSplitChild) continue;
    const sourceKey = getSessionSourceKey(session);
    if (!sessionsBySource.has(sourceKey)) sessionsBySource.set(sourceKey, []);
    sessionsBySource.get(sourceKey).push(session);
  }

  for (const [sourceKey, sourceSessions] of sessionsBySource) {
    let nextIndex = 0;
    for (const session of sourceSessions) {
      const hasStableIndex = session.runtimeDisplaySourceKey === sourceKey
        && Number.isInteger(session.runtimeDisplayIndex)
        && session.runtimeDisplayIndex >= 0;
      if (!hasStableIndex) continue;
      nextIndex = Math.max(
        nextIndex,
        session.runtimeDisplayIndex + 1,
        Number(session.runtimeDisplayNextIndex) || 0,
      );
    }

    for (const session of sourceSessions) {
      const hasStableIndex = session.runtimeDisplaySourceKey === sourceKey
        && Number.isInteger(session.runtimeDisplayIndex)
        && session.runtimeDisplayIndex >= 0;
      if (!hasStableIndex) {
        session.runtimeDisplaySourceKey = sourceKey;
        session.runtimeDisplayIndex = nextIndex;
        session.runtimeDisplayBaseName = getSessionBaseDisplayName(session);
        nextIndex += 1;
      } else if (!session.runtimeDisplayBaseName) {
        session.runtimeDisplayBaseName = getSessionBaseDisplayName(session);
      }
    }

    for (const session of sourceSessions) {
      session.runtimeDisplayNextIndex = nextIndex;
    }
  }
};

export const buildSessionDisplayNameMap = (sessions = []) => {
  const sessionList = Array.isArray(sessions) ? sessions : [];
  const names = new Map();
  const fallbackNextIndexBySource = new Map();

  // Rendering must stay read-only. Runtime metadata is assigned when a session is added;
  // this fallback only covers older/in-memory sessions that do not have that metadata yet.
  for (const session of sessionList) {
    if (!session?.id || session.isSplitChild) continue;
    const sourceKey = getSessionSourceKey(session);
    const hasStableIndex = session.runtimeDisplaySourceKey === sourceKey
      && Number.isInteger(session.runtimeDisplayIndex)
      && session.runtimeDisplayIndex >= 0;
    if (!hasStableIndex) continue;
    fallbackNextIndexBySource.set(
      sourceKey,
      Math.max(
        fallbackNextIndexBySource.get(sourceKey) || 0,
        session.runtimeDisplayIndex + 1,
      ),
    );
  }

  for (const session of sessionList) {
    if (!session?.id || session.isSplitChild) continue;
    const sourceKey = getSessionSourceKey(session);
    const hasStableIndex = session.runtimeDisplaySourceKey === sourceKey
      && Number.isInteger(session.runtimeDisplayIndex)
      && session.runtimeDisplayIndex >= 0;
    const duplicateIndex = hasStableIndex
      ? session.runtimeDisplayIndex
      : (fallbackNextIndexBySource.get(sourceKey) || 0);
    if (!hasStableIndex) fallbackNextIndexBySource.set(sourceKey, duplicateIndex + 1);
    const baseName = session.runtimeDisplayBaseName || getSessionBaseDisplayName(session);
    names.set(session.id, duplicateIndex === 0 ? baseName : `${baseName} (${duplicateIndex})`);
  }
  return names;
};

const selectChannelSession = (members, channel, activeSessionId) => {
  const memberIds = new Set(members.map((session) => session.id));
  if (memberIds.has(activeSessionId)) return activeSessionId;
  if (memberIds.has(channel.primarySessionId)) return channel.primarySessionId;
  return members.find((session) => session.status === 'connected')?.id || members[0]?.id || '';
};

const describeChannel = (channel, connectedCount, memberCount) => {
  const source = channel.sourceMode === 'primary' ? '主控会话输入' : '任意成员输入';
  const send = channel.sendMode === 'line' ? '回车后同步' : '实时同步';
  return `${connectedCount}/${memberCount} 在线 · ${source} · ${send}`;
};

export const buildSessionOverviewItems = (sessions = [], channels = [], activeSessionId = '') => {
  const sessionList = Array.isArray(sessions) ? sessions.filter((session) => session?.id) : [];
  const sessionById = new Map(sessionList.map((session) => [session.id, session]));
  const channelBySessionId = new Map();

  for (const channel of Array.isArray(channels) ? channels : []) {
    const memberIds = Array.from(new Set(channel?.sessionIds || [])).filter((id) => sessionById.has(id));
    if (!channel?.id || memberIds.length === 0) continue;
    for (const sessionId of memberIds) {
      if (!channelBySessionId.has(sessionId)) channelBySessionId.set(sessionId, channel);
    }
  }

  const emittedChannels = new Set();
  const items = [];
  for (const session of sessionList) {
    const channel = channelBySessionId.get(session.id);
    if (!channel) {
      items.push({
        type: 'session',
        id: `session:${session.id}`,
        session,
        sessions: [session],
        selectSessionId: session.id,
      });
      continue;
    }
    if (emittedChannels.has(channel.id)) continue;
    emittedChannels.add(channel.id);

    const memberSet = new Set(channel.sessionIds || []);
    const members = sessionList.filter((candidate) => memberSet.has(candidate.id));
    const connectedCount = members.filter((member) => member.status === 'connected').length;
    items.push({
      type: 'channel',
      id: `channel:${channel.id}`,
      channelId: channel.id,
      channel,
      name: String(channel.name || '').trim() || '同步频道',
      sessions: members,
      connectedCount,
      description: describeChannel(channel, connectedCount, members.length),
      selectSessionId: selectChannelSession(members, channel, activeSessionId),
    });
  }

  return items;
};
