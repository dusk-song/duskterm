<script setup>
import IconButton from '@/components/common/IconButton.vue';
import { CircleX, Code2, Search, Server, Usb, Waypoints } from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { buildSessionDisplayNameMap, buildSessionOverviewItems } from '@/utils/sessionOverview';

const props = defineProps({
  visible: Boolean,
  sessions: { type: Array, default: () => [] },
  syncChannels: { type: Array, default: () => [] },
  activeSessionId: { type: String, default: '' }
});

const emit = defineEmits(['close', 'select', 'close-all']);

const listRef = ref(null);
const searchRef = ref(null);
const searchText = ref('');
const selectedSessionId = ref('');

const getSessionConfig = (session) => session?.config || session || {};
const getProtocol = (session) => String(getSessionConfig(session).protocol || session?.protocol || 'ssh').toLowerCase();
const getProtocolIcon = (protocol) => {
  if (protocol === 'telnet') return Server;
  if (protocol === 'serial') return Usb;
  return Code2;
};
const getProtocolLabel = (protocol) => {
  if (protocol === 'telnet') return 'Telnet';
  if (protocol === 'serial') return 'Serial';
  if (protocol === 'local') return 'Local';
  return 'SSH';
};
const sessionDisplayNames = computed(() => buildSessionDisplayNameMap(props.sessions));

const createSessionView = (session) => {
  const config = getSessionConfig(session);
  const protocol = getProtocol(session);
  const protocolLabel = getProtocolLabel(protocol);
  const host = String(config.host || session?.host || '').trim();
  const username = String(config.username || session?.username || '').trim();
  const group = String(config.group || session?.group || '').trim();
  const serialPath = String(config.serial_path || session?.serial_path || '').trim();
  const baudRate = Number(config.baud_rate || session?.baud_rate || 0);
  const port = Number(config.port || session?.port || (protocol === 'telnet' ? 23 : protocol === 'ssh' ? 22 : 0));
  const configuredName = String(config.name || session?.name || '').trim();
  const displayName = sessionDisplayNames.value.get(session.id)
    || configuredName
    || host
    || serialPath
    || (protocol === 'local' ? '本地终端' : '未命名会话');

  let identity = '';
  if (protocol === 'serial') {
    identity = [serialPath || '串口设备', baudRate || ''].filter(Boolean).join(' · ');
  } else if (protocol === 'local') {
    identity = String(config.local_shell_name || session?.local_shell_name || configuredName || '本地终端');
  } else {
    const defaultPort = protocol === 'telnet' ? 23 : protocol === 'ssh' ? 22 : 0;
    const hostDisplay = host && port && port !== defaultPort ? `${host}:${port}` : host;
    identity = [username, hostDisplay].filter(Boolean).join(' · ');
  }

  const searchIndex = [
    configuredName,
    displayName,
    identity,
    username,
    host,
    port,
    group,
    protocol,
    protocolLabel,
    serialPath,
    baudRate,
  ].map((value) => String(value || '').toLowerCase()).join('\u0000');

  return {
    id: session.id,
    session,
    name: displayName,
    identity: identity || '—',
    group,
    protocol,
    protocolLabel,
    icon: getProtocolIcon(protocol),
    connected: session.status === 'connected',
    searchIndex,
  };
};

const baseItems = computed(() => buildSessionOverviewItems(
  props.sessions,
  props.syncChannels,
  props.activeSessionId,
));
const normalizedSearch = computed(() => searchText.value.trim().toLowerCase());

const overviewItems = computed(() => {
  const keyword = normalizedSearch.value;
  return baseItems.value.flatMap((item) => {
    const sessionViews = item.sessions.map(createSessionView);
    if (!keyword) return [{ ...item, sessionViews }];

    if (item.type === 'session') {
      return sessionViews[0]?.searchIndex.includes(keyword)
        ? [{ ...item, sessionViews }]
        : [];
    }

    const channelSearchIndex = [item.name, item.description]
      .map((value) => String(value || '').toLowerCase())
      .join('\u0000');
    const matchingSessions = channelSearchIndex.includes(keyword)
      ? sessionViews
      : sessionViews.filter((session) => session.searchIndex.includes(keyword));
    return matchingSessions.length
      ? [{ ...item, sessionViews: matchingSessions }]
      : [];
  });
});

const selectableSessions = computed(() => overviewItems.value.flatMap((item) => item.sessionViews));
const selectableSessionIds = computed(() => selectableSessions.value.map((session) => session.id));
const channelCount = computed(() => baseItems.value.filter((item) => item.type === 'channel').length);

const getChannelMode = (item) => {
  const sendMode = item.channel?.sendMode === 'line' ? '回车后同步' : '实时同步';
  return `${item.connectedCount}/${item.sessions.length} 在线 · ${sendMode}`;
};

const getChannelSource = (item) => {
  if (item.channel?.sourceMode !== 'primary') return '组内任意成员输入';
  const primarySession = item.sessions.find((session) => session.id === item.channel?.primarySessionId);
  const primary = primarySession ? createSessionView(primarySession) : null;
  return `主控：${primary?.identity || primary?.name || '未设置'}`;
};

const getChannelRole = (item, session) => {
  if (item.channel?.sourceMode !== 'primary') return '组内任意';
  return session.id === item.channel?.primarySessionId ? '主控' : '跟随';
};

const ensureSelectedSession = (preferredId = '', forcePreferred = false) => {
  const ids = selectableSessionIds.value;
  if (!ids.length) {
    selectedSessionId.value = '';
    return;
  }
  if (forcePreferred && preferredId && ids.includes(preferredId)) {
    selectedSessionId.value = preferredId;
    return;
  }
  if (ids.includes(selectedSessionId.value)) return;
  selectedSessionId.value = preferredId && ids.includes(preferredId) ? preferredId : ids[0];
};

const scrollToSelected = async () => {
  await nextTick();
  const selected = Array.from(listRef.value?.querySelectorAll?.('[data-session-id]') || [])
    .find((element) => element.dataset.sessionId === selectedSessionId.value);
  selected?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
};

const moveSelection = (offset) => {
  const ids = selectableSessionIds.value;
  if (!ids.length) return;
  const currentIndex = Math.max(0, ids.indexOf(selectedSessionId.value));
  selectedSessionId.value = ids[Math.max(0, Math.min(ids.length - 1, currentIndex + offset))];
  scrollToSelected();
};

const confirmSelection = () => {
  if (!selectedSessionId.value) return;
  emit('select', selectedSessionId.value);
  emit('close');
};

const selectSession = (sessionId) => {
  if (!sessionId) return;
  selectedSessionId.value = sessionId;
  confirmSelection();
};

const requestCloseAll = () => {
  if (props.sessions.length === 0) return;
  emit('close-all');
};

const focusSearch = () => {
  searchRef.value?.focus({ preventScroll: true });
  searchRef.value?.select?.();
};
const focusList = () => listRef.value?.focus({ preventScroll: true });
const toggleSearchFocus = () => {
  if (document.activeElement === searchRef.value) focusList();
  else focusSearch();
};

const handleKeyDown = (event) => {
  if (!props.visible || event.isComposing) return;
  switch (event.key) {
    case 'Escape':
      event.preventDefault();
      event.stopPropagation();
      emit('close');
      break;
    case 'Tab':
      event.preventDefault();
      event.stopPropagation();
      toggleSearchFocus();
      break;
    case 'ArrowUp':
      event.preventDefault();
      event.stopPropagation();
      moveSelection(-1);
      break;
    case 'ArrowDown':
      event.preventDefault();
      event.stopPropagation();
      moveSelection(1);
      break;
    case 'Enter':
      event.preventDefault();
      event.stopPropagation();
      confirmSelection();
      break;
  }
};

onMounted(() => window.addEventListener('keydown', handleKeyDown, true));
onUnmounted(() => window.removeEventListener('keydown', handleKeyDown, true));

watch(() => props.visible, async (visible) => {
  if (!visible) return;
  searchText.value = '';
  await nextTick();
  ensureSelectedSession(props.activeSessionId, true);
  focusList();
  scrollToSelected();
});

watch(selectableSessionIds, () => {
  ensureSelectedSession(props.activeSessionId);
  scrollToSelected();
});
</script>

<template>
  <Teleport to="body">
    <Transition name="overview">
      <div v-if="visible" class="overview-backdrop" @click.self="emit('close')">
        <div class="overview-container">
          <div class="overview-search">
            <Search :size="15" aria-hidden="true" />
            <input ref="searchRef" v-model="searchText" type="search"
              placeholder="搜索会话、主机、用户、分组..." autocomplete="off" spellcheck="false"
              aria-label="搜索会话" />
          </div>

          <div ref="listRef" class="overview-list" tabindex="0" role="listbox"
            aria-label="当前打开的会话">
            <template v-for="item in overviewItems" :key="item.id">
              <div v-if="item.type === 'session' && item.sessionViews[0]"
                class="overview-session-row"
                :class="{
                  selected: item.sessionViews[0].id === selectedSessionId,
                  current: item.sessionViews[0].id === props.activeSessionId,
                }"
                role="option"
                tabindex="-1"
                :data-session-id="item.sessionViews[0].id"
                :aria-selected="item.sessionViews[0].id === selectedSessionId"
                @mouseenter="selectedSessionId = item.sessionViews[0].id"
                @click="selectSession(item.sessionViews[0].id)">
                <span class="session-protocol-icon" :title="item.sessionViews[0].protocolLabel">
                  <component :is="item.sessionViews[0].icon" :size="15" />
                </span>
                <span class="session-status" :class="{ online: item.sessionViews[0].connected }"
                  :title="item.sessionViews[0].connected ? '在线' : '离线'" />
                <span class="session-name" :title="item.sessionViews[0].name">{{ item.sessionViews[0].name }}</span>
                <span class="session-identity" :title="item.sessionViews[0].identity">{{ item.sessionViews[0].identity }}</span>
                <span v-if="item.sessionViews[0].group" class="session-meta">
                  {{ item.sessionViews[0].group }}
                </span>
              </div>

              <section v-else-if="item.type === 'channel'" class="overview-channel">
                <header class="channel-header">
                  <span class="channel-icon"><Waypoints :size="16" /></span>
                  <span class="channel-copy">
                    <strong>{{ item.name }}</strong>
                    <small>{{ getChannelSource(item) }}</small>
                  </span>
                  <span class="channel-mode">{{ getChannelMode(item) }}</span>
                </header>

                <div class="channel-members">
                  <div v-for="session in item.sessionViews" :key="session.id"
                    class="overview-session-row channel-member"
                    :class="{
                      selected: session.id === selectedSessionId,
                      current: session.id === props.activeSessionId,
                    }"
                    role="option"
                    tabindex="-1"
                    :data-session-id="session.id"
                    :aria-selected="session.id === selectedSessionId"
                    @mouseenter="selectedSessionId = session.id"
                    @click="selectSession(session.id)">
                    <span class="session-protocol-icon" :title="session.protocolLabel">
                      <component :is="session.icon" :size="15" />
                    </span>
                    <span class="session-status" :class="{ online: session.connected }"
                      :title="session.connected ? '在线' : '离线'" />
                    <span class="session-name" :title="session.name">{{ session.name }}</span>
                    <span class="session-identity" :title="session.identity">{{ session.identity }}</span>
                    <span class="channel-role" :class="{ primary: getChannelRole(item, session) === '主控' }">
                      {{ getChannelRole(item, session) }}
                    </span>
                  </div>
                </div>
              </section>
            </template>

            <div v-if="overviewItems.length === 0" class="overview-empty">
              {{ props.sessions.length ? '未找到匹配的会话' : '暂无活跃会话' }}
            </div>
          </div>

          <div class="overview-footer">
            <div class="overview-footer-side overview-footer-start">
              <span class="overview-count">
                {{ props.sessions.length }} 个会话
                <span v-if="channelCount"> · {{ channelCount }} 个同步频道</span>
              </span>
            </div>
            <div class="overview-hint">
              <kbd>↑↓</kbd> 导航
              <kbd>Tab</kbd> 搜索
              <kbd>Enter</kbd> 选择
              <kbd>Esc</kbd> 关闭
            </div>
            <div class="overview-footer-side overview-footer-actions">
              <IconButton class="overview-close-all" :icon="CircleX" size="sm"
                aria-label="关闭所有会话" tooltip-side="top" :tooltip-z-index="3100"
                :disabled="props.sessions.length === 0" :action="requestCloseAll" />
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.overview-backdrop {
  position: fixed;
  inset: 0;
  z-index: var(--z-critical-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 40px;
  background: rgba(0, 0, 0, 0.72);
}

.overview-container {
  display: flex;
  box-sizing: border-box;
  width: 68vw;
  max-width: 920px;
  min-width: 0;
  max-height: 82vh;
  flex-direction: column;
  gap: 12px;
  padding: 12px;
  border: 1px solid color-mix(in srgb, var(--app-text) 14%, var(--app-border-shadow));
  border-radius: 12px;
  background: var(--app-bg-dialog);
  box-shadow: 0 18px 52px rgba(0, 0, 0, 0.28);
}

.overview-count {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 2px 9px;
  border: 1px solid color-mix(in srgb, var(--app-text) 12%, var(--app-border-shadow));
  border-radius: 999px;
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-text) 12%, var(--app-bg-dialog));
  font-size: 11px;
}

.overview-search {
  display: flex;
  width: 100%;
  min-width: 0;
  box-sizing: border-box;
  height: 36px;
  flex: 0 0 36px;
  align-items: center;
  gap: 9px;
  padding: 0 12px;
  border: 1px solid var(--app-border-shadow);
  border-radius: var(--niri-radius-md, 8px);
  color: var(--app-text-muted);
  background: color-mix(in srgb, var(--app-input-bg) 82%, var(--app-bg-dialog));
  transition: border-color 120ms ease, background-color 120ms ease;
}

.overview-search:focus-within {
  border-color: color-mix(in srgb, var(--color-primary) 72%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--app-input-bg) 92%, var(--app-bg-dialog));
}

.overview-search input {
  appearance: none;
  display: block;
  width: 100%;
  height: 100%;
  min-width: 0;
  flex: 1;
  margin: 0;
  padding: 0;
  border: 0;
  border-radius: 0;
  outline: 0;
  color: var(--app-text);
  background: transparent;
  box-shadow: none;
  font: inherit;
  font-size: 12px;
}

.overview-search input::placeholder { color: var(--app-text-muted); }
.overview-search input::-webkit-search-cancel-button { display: none; }

.overview-list {
  display: flex;
  min-height: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 5px;
  overflow-y: auto;
  padding: 4px;
  outline: none;
}

.overview-session-row {
  position: relative;
  display: flex;
  box-sizing: border-box;
  width: 100%;
  min-height: 42px;
  flex: 0 0 auto;
  align-items: center;
  gap: 12px;
  margin: 0;
  padding: 0 11px;
  overflow: hidden;
  border: 1px solid var(--app-border-shadow);
  border-radius: var(--niri-radius-md, 8px);
  color: var(--app-text);
  text-align: left;
  background: color-mix(in srgb, var(--app-input-bg) 62%, var(--app-bg-dialog));
  cursor: pointer;
  transition: border-color 120ms ease, background-color 120ms ease;
}

.overview-session-row:hover {
  border-color: color-mix(in srgb, var(--app-text) 18%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--app-input-bg) 78%, var(--app-bg-dialog));
}

.overview-session-row.current { box-shadow: inset 3px 0 0 var(--color-primary); }
.overview-session-row.selected {
  border-color: color-mix(in srgb, var(--color-primary) 78%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--color-primary) 11%, var(--app-bg-dialog));
}

.session-protocol-icon {
  display: flex;
  width: 26px;
  height: 26px;
  flex: 0 0 26px;
  align-items: center;
  justify-content: center;
  border: 1px solid color-mix(in srgb, var(--app-text) 9%, transparent);
  border-radius: 6px;
  color: var(--app-text-muted);
  background: color-mix(in srgb, var(--app-input-bg) 82%, transparent);
}

.session-status {
  display: block;
  width: 7px;
  height: 7px;
  flex: 0 0 7px;
  border-radius: 50%;
  background: var(--app-connection-offline);
}
.session-status.online { background: var(--app-connection-online); }

.session-name,
.session-identity,
.session-meta {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-name {
  width: 180px;
  flex: 0 1 180px;
  color: var(--app-text);
  font-size: 13px;
  font-weight: 600;
}
.session-identity {
  flex: 1 1 240px;
  color: var(--app-text-muted);
  font-size: 12px;
}
.session-meta {
  width: 150px;
  flex: 0 1 150px;
  color: var(--app-text-muted);
  font-size: 11px;
  text-align: right;
}

.overview-channel {
  flex: 0 0 auto;
  overflow: hidden;
  border: 1px solid var(--app-border-shadow);
  border-radius: var(--niri-radius-md, 8px);
  background: color-mix(in srgb, var(--app-bg-dialog) 91%, var(--app-input-bg));
}

.channel-header {
  display: flex;
  min-height: 54px;
  align-items: center;
  gap: 10px;
  padding: 7px 13px;
}

.channel-icon {
  display: inline-flex;
  width: 30px;
  height: 30px;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  color: var(--app-text-muted);
  background: color-mix(in srgb, var(--app-text) 7%, transparent);
}

.channel-copy {
  display: flex;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 2px;
}

.channel-copy strong,
.channel-copy small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-copy strong { color: var(--app-text); font-size: 13px; }
.channel-copy small { color: var(--app-text-muted); font-size: 11px; }
.channel-mode {
  flex: 0 0 auto;
  margin-left: auto;
  color: var(--app-text-muted);
  font-size: 11px;
  white-space: nowrap;
}

.channel-members {
  display: flex;
  flex-direction: column;
  margin: 0 10px 10px;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--app-text) 8%, transparent);
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-input-bg) 54%, transparent);
}

.overview-session-row.channel-member {
  min-height: 38px;
  border: 0;
  border-top: 1px solid color-mix(in srgb, var(--app-text) 7%, transparent);
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}
.overview-session-row.channel-member:first-child { border-top: 0; }
.overview-session-row.channel-member:hover { background: color-mix(in srgb, var(--app-text) 5%, transparent); }
.overview-session-row.channel-member.current { box-shadow: inset 3px 0 0 var(--color-primary); }
.overview-session-row.channel-member.selected { background: color-mix(in srgb, var(--color-primary) 10%, transparent); }

.channel-role {
  flex: 0 0 auto;
  margin-left: auto;
  padding: 2px 7px;
  border-radius: 5px;
  color: var(--app-text-muted);
  background: color-mix(in srgb, var(--app-text) 7%, transparent);
  font-size: 10px;
  white-space: nowrap;
}
.channel-role.primary {
  color: var(--color-primary);
  background: color-mix(in srgb, var(--color-primary) 11%, transparent);
}

.overview-empty {
  display: flex;
  min-height: 150px;
  align-items: center;
  justify-content: center;
  color: var(--app-text-muted);
  font-size: 13px;
}

.overview-footer {
  display: flex;
  align-items: center;
  min-height: 28px;
  padding: 0 4px;
}

.overview-footer-side {
  display: flex;
  min-width: 0;
  flex: 1 1 0;
  align-items: center;
}

.overview-footer-actions { justify-content: flex-end; }

.overview-hint {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 8px;
  color: var(--app-text-muted);
  font-size: 11px;
}
.overview-hint kbd {
  display: inline-block;
  padding: 1px 6px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 3px;
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
  font-family: inherit;
  font-size: 10px;
}

.overview-close-all {
  --icon-btn-color: color-mix(in srgb, var(--color-danger) 76%, var(--app-text-muted));
  --icon-btn-hover-color: var(--color-danger);
  --icon-btn-hover-bg: color-mix(in srgb, var(--color-danger) 12%, transparent);
  --icon-btn-active-bg: color-mix(in srgb, var(--color-danger) 18%, transparent);
}

:global(html:not(.dark)) .overview-backdrop {
  background: rgba(48, 45, 41, 0.32);
}

:global(html:not(.dark)) .overview-container {
  border-color: color-mix(in srgb, var(--app-text) 16%, var(--app-border-shadow));
  box-shadow: 0 16px 44px rgba(49, 43, 36, 0.24);
}

:global(html:not(.dark)) .overview-search {
  border-color: color-mix(in srgb, var(--app-text) 13%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--app-input-bg) 76%, var(--app-bg-dialog));
}

:global(html:not(.dark)) .overview-session-row {
  border-color: color-mix(in srgb, var(--app-text) 12%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--app-input-bg) 68%, var(--app-bg-dialog));
}

:global(html:not(.dark)) .overview-session-row:hover {
  background: color-mix(in srgb, var(--app-input-bg) 88%, var(--app-bg-dialog));
}

:global(html:not(.dark)) .overview-session-row.selected {
  border-color: color-mix(in srgb, var(--color-primary) 72%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--color-primary) 9%, var(--app-bg-dialog));
}

:global(html:not(.dark)) .overview-session-row.channel-member {
  background: transparent;
}

:global(html:not(.dark)) .overview-session-row.channel-member:hover {
  background: color-mix(in srgb, var(--app-text) 5%, transparent);
}

:global(html:not(.dark)) .overview-session-row.channel-member.selected {
  background: color-mix(in srgb, var(--color-primary) 9%, transparent);
}

:global(html:not(.dark)) .overview-hint {
  color: color-mix(in srgb, var(--app-text) 68%, transparent);
}

:global(html:not(.dark)) .overview-hint kbd {
  border-color: color-mix(in srgb, var(--app-text) 18%, var(--app-border-shadow));
  color: var(--app-text-secondary);
  background: color-mix(in srgb, var(--app-input-bg) 82%, var(--app-bg-dialog));
}

:global(html:not(.dark)) .overview-close-all {
  --icon-btn-color: color-mix(in srgb, var(--color-danger) 82%, var(--app-text));
  --icon-btn-hover-color: color-mix(in srgb, var(--color-danger) 88%, var(--app-text));
  --icon-btn-hover-bg: color-mix(in srgb, var(--color-danger) 15%, transparent);
}

.overview-enter-active { transition: opacity 160ms ease; }
.overview-leave-active { transition: opacity 120ms ease; }
.overview-enter-from,
.overview-leave-to { opacity: 0; }

@media (max-width: 700px) {
  .overview-backdrop { padding: 24px; }
  .overview-container { width: calc(100vw - 48px); }
  .session-name { width: 140px; flex-basis: 140px; }
  .session-meta,
  .channel-role { display: none; }
  .overview-hint { display: none; }
}

@media (prefers-reduced-motion: reduce) {
  .overview-enter-active,
  .overview-leave-active { transition-duration: 1ms; }
}
</style>
