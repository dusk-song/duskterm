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
      name: String(channel.name || '').trim() || '同步频道',
      sessions: members,
      connectedCount,
      description: describeChannel(channel, connectedCount, members.length),
      selectSessionId: selectChannelSession(members, channel, activeSessionId),
    });
  }

  return items;
};
