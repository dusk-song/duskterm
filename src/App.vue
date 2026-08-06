<script setup>
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { Code2, Server, Trash2, Usb } from '@lucide/vue';
import { computed, defineAsyncComponent, h, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

// ── Lazy-loaded heavy components for faster initial paint ──
import ConfirmDialog from '@/components/ui/confirm/ConfirmDialog.vue';
import ToastContainer from '@/components/ui/toast/ToastContainer.vue';
import { TooltipProvider } from '@/components/ui/tooltip';
import CustomTitlebar from './components/app-shell/CustomTitlebar.vue';
import GlobalBackground from './components/app-shell/GlobalBackground.vue';
import { useInputRouter } from './composables/useInputRouter';
const DesktopPet = defineAsyncComponent(() => import('./components/misc/DesktopPet.vue'));
const FileManager = defineAsyncComponent(() => import('./components/sftp/FileManager.vue'));
const TransferPanel = defineAsyncComponent(() => import('./components/app-shell/TransferPanel.vue'));
const LockScreen = defineAsyncComponent(() => import('./components/app-shell/LockScreen.vue'));
// ResponsiveMenuBar removed — native OS menu handles all menu bar functionality
const CommandKnowledgePanel = defineAsyncComponent(() => import('./components/knowledge/CommandKnowledgePanel.vue'));
const SessionList = defineAsyncComponent(() => import('./components/session/SessionList.vue'));
const SessionModal = defineAsyncComponent(() => import('./components/session/SessionModal.vue'));
const SessionOverview = defineAsyncComponent(() => import('./components/session/SessionOverview.vue'));
const SettingsModal = defineAsyncComponent(() => import('./components/settings/SettingsModal.vue'));
const SyncInputModal = defineAsyncComponent(() => import('./components/terminal/SyncInputModal.vue'));
const TerminalPanelManager = defineAsyncComponent(() => import('./components/terminal/TerminalPanelManager.vue'));
const TiledPanel = defineAsyncComponent(() => import('./components/terminal/TiledPanel.vue'));
const TunnelModal = defineAsyncComponent(() => import('./components/tunnel/TunnelModal.vue'));
// useMenuHandler removed — CustomTitlebar handles menus directly
import { usePanelLayout } from './composables/usePanelLayout';
import { useTerminalTransferCoordinator } from './composables/useTerminalTransferCoordinator';
import { useTerminalConnection } from './composables/useTerminalConnection';
import { useTerminalPanels } from './composables/useTerminalPanels';
import { useTheme } from './composables/useTheme';
import { useWindowInteraction } from './composables/useWindowInteraction';
import { createSessionBooleanState, resolveFocusedSessionId } from './utils/sftpPanelState';
import { useTransfersStore } from './stores/transfers';
import { useSshStore } from './stores/ssh';
import { invokeCommand, listenEvent } from './utils/ipc';
import { loadMainUiSettings, normalizeMainUiSettings, saveMainUiSettings } from './utils/mainUi';
import { getPreferenceDefaults, loadPreference } from './utils/preferences';

const sshStore = useSshStore();
const transferStore = useTransfersStore();
const { theme, setTheme } = useTheme();

// Native menu handling removed — CustomTitlebar handles menus directly

// ── Centralized event listener tracking for cleanup ──
const _appEvts = [];
const onApp = (event, handler) => {
  window.addEventListener(event, handler);
  _appEvts.push([event, handler]);
};

onApp('app:new-connection', () => { showSessionModal(); });
onApp('app:open-session-list', () => {
  showSessionPanel.value = true;
  sshStore.loadSavedSessions();
});
onApp('app:disconnect-current', () => {
  if (activeKey.value) {
    sshStore.removeSession(activeKey.value);
  }
});
onApp('app:disconnect-all', () => {
  window.dispatchEvent(new CustomEvent('app:sync-input-stop'));
  const ids = [...(sshStore.sessions || [])].map((session) => session.id);
  ids.forEach((id) => sshStore.removeSession(id));
});
onApp('app:open-settings', () => { isSettingsVisible.value = true; });
onApp('menu:file_prefs', () => { isSettingsVisible.value = true; });
onApp('app:theme-change', (e) => {
  setTheme(e.detail);
});
onApp('app:open-tunnel', () => {
  preferredTunnelSessionId.value = activeKey.value || '';
  isTunnelModalVisible.value = true;
});
onApp('app:open-tool-panels', () => {
  showSessionPanel.value = true;
  openSftpPanel();
});
onApp('app:open-tool-sessions', () => {
  showSessionPanel.value = true;
  sshStore.loadSavedSessions();
});
onApp('app:open-tool-sftp', () => {
  openSftpPanel();
});
onApp('app:toggle-tool-sessions', () => {
  showSessionPanel.value = !showSessionPanel.value;
  if (showSessionPanel.value) {
    sshStore.loadSavedSessions();
  }
});
onApp('app:toggle-tool-sftp', () => {
  toggleSftpPanel();
});
onApp('app:toggle-tool-knowledge', () => {
  toggleCommandKnowledgePanel();
});
onApp('app:toggle-transfer-panel', () => {
  toggleTransferPanelVisibility();
});
onApp('app:refresh-sessions', () => {
  sshStore.loadSavedSessions();
});
onApp('app:refresh-current-view', () => {
  sshStore.loadSavedSessions();
  const sessionId = getActiveSftpWorkspaceId();
  if (sessionId) {
    window.dispatchEvent(new CustomEvent('app:sftp-refresh-active', {
      detail: { sessionId }
    }));
  }
});
onApp('app:save-current-session', () => {
  void saveCurrentSessionFromMenu();
});
onApp('app:save-all-sessions', () => {
  void saveAllSessionsFromMenu();
});
onApp('app:reconnect-current', () => {
  void reconnectCurrentSessionFromMenu();
});
onApp('app:reconnect-all', () => {
  void reconnectAllSessionsFromMenu();
});
onApp('app:edit-active-session', () => {
  openActiveSessionEditor();
});
onApp('app:terminal-find', () => {
  if (!activeKey.value) {
    toast.info('当前没有激活会话');
    return;
  }
  window.dispatchEvent(new CustomEvent('term:find', {
    detail: { sessionId: activeKey.value }
  }));
});
onApp('app:close-current-panel', () => {
  closeCurrentPanel();
});

// ── Trackpad gesture events from Terminal ──
onApp('terminal-gesture-next', () => {
  keybindingActions.nextSession();
});
onApp('terminal-gesture-prev', () => {
  keybindingActions.prevSession();
});
// Unified quit handler — calls Rust to force process exit
const performQuit = async () => {
  try {
    await invokeCommand('exit_app');
  } catch {
    // Fallback: close the current window if the native quit command is unavailable.
    try { const w = await import('@tauri-apps/api/window').then(m => m.getCurrentWindow()); await w.close(); } catch { window.close(); }
  }
};
onApp('app:quit', () => { performQuit(); });

// Settings
const isSettingsVisible = ref(false);

// Session Panel
const showSessionModal = (sessionData = null) => {
  currentEditSession.value = sessionData;
  isSessionModalVisible.value = true;
};

async function openSavedSessionEditor(sessionData) {
  if (!sessionData?.id) {
    showSessionModal(sessionData);
    return;
  }

  try {
    const decrypted = await invokeCommand('get_decrypted_session', { id: sessionData.id });
    showSessionModal(decrypted);
  } catch (error) {
    console.error('Failed to load decrypted session for edit:', error);
    toast.error(`加载会话详情失败：${error}`);
  }
}
const isSessionModalVisible = ref(false);
const isTunnelModalVisible = ref(false);
const preferredTunnelSessionId = ref('');
const currentEditSession = ref(null);
const showSessionPanel = ref(false);
const sftpPanelVisibility = createSessionBooleanState(false);
let getActiveSftpWorkspaceId = () => '';
const activeToolPanel = ref(null);
const transferPanelVisible = ref(false);
const transferPanelExpanded = ref(false);
const toggleTransferPanelVisibility = () => {
  transferPanelVisible.value = !transferPanelVisible.value;
  if (transferPanelVisible.value) {
    transferPanelExpanded.value = true;
  } else {
    transferPanelExpanded.value = false;
  }
};
const closeTransferPanel = () => {
  transferPanelVisible.value = false;
  transferPanelExpanded.value = false;
};
watch(
  () => transferStore.panelOpenRequestVersion,
  (version, previousVersion) => {
    if (version <= previousVersion) return;
    transferPanelVisible.value = true;
    transferPanelExpanded.value = true;
  },
);
useTerminalTransferCoordinator({ transferStore });
const showSftpPanel = computed({
  get: () => activeToolPanel.value === 'sftp',
  set: (visible) => {
    if (visible) activeToolPanel.value = 'sftp';
    else if (activeToolPanel.value === 'sftp') activeToolPanel.value = null;
  }
});
const showCommandKnowledgePanel = computed({
  get: () => activeToolPanel.value === 'knowledge',
  set: (visible) => {
    if (visible) activeToolPanel.value = 'knowledge';
    else if (activeToolPanel.value === 'knowledge') activeToolPanel.value = null;
  }
});
const SFTP_PANEL_TRANSITION_MS = 220;
let sftpPanelTransitionTimer = null;

const notifyTerminalLayoutDragging = (dragging, options = {}) => {
  window.dispatchEvent(new CustomEvent('terminal-layout-dragging', {
    detail: {
      dragging,
      ...options
    }
  }));
};

const notifyTerminalLayoutResize = () => {
  window.dispatchEvent(new CustomEvent('terminal-layout-resize'));
};

watch(showSftpPanel, () => {
  const sessionId = getActiveSftpWorkspaceId();
  if (sessionId) sftpPanelVisibility.set(sessionId, showSftpPanel.value);

  if (sftpPanelTransitionTimer) {
    clearTimeout(sftpPanelTransitionTimer);
    sftpPanelTransitionTimer = null;
  }

  notifyTerminalLayoutDragging(true, { source: 'sftp-panel', deferFit: true });
  measureWorkspace();
  nextTick(() => window.dispatchEvent(new CustomEvent('app:sftp-layout-refresh')));

  sftpPanelTransitionTimer = setTimeout(() => {
    sftpPanelTransitionTimer = null;
    notifyTerminalLayoutDragging(false, { source: 'sftp-panel', deferFit: true });
    notifyTerminalLayoutResize();
    measureWorkspace();
    window.dispatchEvent(new CustomEvent('app:sftp-layout-refresh'));
  }, SFTP_PANEL_TRANSITION_MS);
});

const workspaceRef = ref(null);
const workspaceWidth = ref(0);
const leftColumnWidth = ref(360);
const toolPanelWidth = ref(440);
const isDraggingColumn = ref(false);
const workspaceSeparatorSize = 4;
const workspaceGridGap = 6;
const workspaceGridPaddingX = 8;
const leftColumnMinWidth = 280;
const rightColumnMinWidth = 320;
const toolPanelMinWidth = ref(360);

let _measureWorkspaceRaf = null;
const measureWorkspace = () => {
  const tracksWorkspaceWidth = (
    showSessionPanel.value
    || showCommandKnowledgePanel.value
    || showActiveSftpPanel.value
  );
  if (workspaceWidth.value > 0 && !tracksWorkspaceWidth) return;
  if (_measureWorkspaceRaf) return;
  _measureWorkspaceRaf = requestAnimationFrame(() => {
    _measureWorkspaceRaf = null;
    const element = workspaceRef.value;
    if (!element) return;
    const rect = element.getBoundingClientRect();
    workspaceWidth.value = Math.max(0, rect.width);
    clampWorkspaceLayout();
  });
};

const hasWorkspaceLeftPanels = computed(() => showSessionPanel.value);
const hasWorkspaceRightPanel = computed(() => (
  showCommandKnowledgePanel.value || showActiveSftpPanel.value
));

const workspaceContentWidth = (width = workspaceWidth.value) => {
  const trackCount = 1 + (hasWorkspaceLeftPanels.value ? 2 : 0) + (hasWorkspaceRightPanel.value ? 2 : 0);
  const gapWidth = Math.max(0, trackCount - 1) * workspaceGridGap;
  return Math.max(0, (width || 0) - workspaceGridPaddingX - gapWidth);
};

const getWorkspaceWidthState = (width = workspaceWidth.value) => {
  const hasLeft = hasWorkspaceLeftPanels.value;
  const hasRight = hasWorkspaceRightPanel.value;
  const separatorCount = Number(hasLeft) + Number(hasRight);
  const availableWidth = Math.max(
    0,
    workspaceContentWidth(width) - separatorCount * workspaceSeparatorSize
  );
  const leftWidth = hasLeft
    ? Math.max(leftColumnMinWidth, Number.isFinite(leftColumnWidth.value) ? leftColumnWidth.value : leftColumnMinWidth)
    : 0;
  const toolWidth = hasRight
    ? Math.max(toolPanelMinWidth.value, Number.isFinite(toolPanelWidth.value) ? toolPanelWidth.value : toolPanelMinWidth.value)
    : 0;
  const preferredWidth = leftWidth + rightColumnMinWidth + toolWidth;

  return {
    availableWidth,
    hasLeft,
    hasRight,
    leftWidth,
    proportional: availableWidth > 0 && availableWidth < preferredWidth,
    toolWidth,
  };
};

const clampWorkspaceLayout = () => {
  if (getWorkspaceWidthState().proportional) return;
  const availableWidth = workspaceContentWidth();

  if (availableWidth > 0 && hasWorkspaceLeftPanels.value) {
    const rightReserve = hasWorkspaceRightPanel.value
      ? toolPanelWidth.value + workspaceSeparatorSize
      : 0;
    const maxLeftWidth = Math.max(
      leftColumnMinWidth,
      availableWidth - rightColumnMinWidth - rightReserve - workspaceSeparatorSize
    );
    const nextLeftWidth = Math.min(maxLeftWidth, Math.max(leftColumnMinWidth, leftColumnWidth.value));
    leftColumnWidth.value = nextLeftWidth;
  }

  if (availableWidth > 0 && hasWorkspaceRightPanel.value) {
    const leftReserve = hasWorkspaceLeftPanels.value
      ? leftColumnWidth.value + workspaceSeparatorSize
      : 0;
    const maxRightWidth = Math.max(
      toolPanelMinWidth.value,
      availableWidth - leftReserve - rightColumnMinWidth - workspaceSeparatorSize
    );
    toolPanelWidth.value = Math.min(
      maxRightWidth,
      Math.max(toolPanelMinWidth.value, toolPanelWidth.value)
    );
  }

};

const startWorkspaceResize = (axis, event) => {
  if (axis === 'columns' && !hasWorkspaceLeftPanels.value) return;
  if (axis === 'tool' && !hasWorkspaceRightPanel.value) return;

  const startX = event.clientX;
  const startLeftWidth = leftColumnWidth.value;
  const startToolWidth = toolPanelWidth.value;
  let rafId = null;
  let pendingLeftWidth = startLeftWidth;
  let pendingToolWidth = startToolWidth;
  const workspaceEl = workspaceRef.value;

  isDraggingColumn.value = true;
  notifyTerminalLayoutDragging(true, { source: 'workspace-column', deferFit: true });
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';

  const updateLayout = () => {
    if (axis === 'columns') {
      leftColumnWidth.value = pendingLeftWidth;
    } else if (axis === 'tool') {
      toolPanelWidth.value = pendingToolWidth;
    }
    rafId = null;
  };

  const updateCssVariable = () => {
    if (!workspaceEl) return;

    const unit = getWorkspaceWidthState().proportional ? 'fr' : 'px';

    if (axis === 'columns') {
      workspaceEl.style.setProperty('--drag-left-width', `${pendingLeftWidth}${unit}`);
    } else if (axis === 'tool') {
      workspaceEl.style.setProperty('--drag-tool-width', `${pendingToolWidth}${unit}`);
    }
    rafId = null;
  };

  const onMouseMove = (moveEvent) => {
    const widthState = getWorkspaceWidthState(
      workspaceWidth.value || workspaceRef.value?.getBoundingClientRect().width || 0
    );
    if (widthState.proportional) {
      const preferenceScale = (
        widthState.leftWidth + rightColumnMinWidth + widthState.toolWidth
      ) / Math.max(1, widthState.availableWidth);
      if (axis === 'columns') {
        pendingLeftWidth = Math.max(
          leftColumnMinWidth,
          startLeftWidth + (moveEvent.clientX - startX) * preferenceScale
        );
      } else if (axis === 'tool') {
        pendingToolWidth = Math.max(
          toolPanelMinWidth.value,
          startToolWidth - (moveEvent.clientX - startX) * preferenceScale
        );
      }
    } else if (axis === 'columns') {
      const availableWidth = workspaceContentWidth(workspaceWidth.value || workspaceRef.value?.getBoundingClientRect().width || 0);
      const rightReserve = hasWorkspaceRightPanel.value
        ? toolPanelWidth.value + workspaceSeparatorSize
        : 0;
      const maxLeftWidth = Math.max(
        leftColumnMinWidth,
        availableWidth - rightColumnMinWidth - rightReserve - workspaceSeparatorSize
      );
      pendingLeftWidth = Math.min(maxLeftWidth, Math.max(leftColumnMinWidth, startLeftWidth + (moveEvent.clientX - startX)));
    } else if (axis === 'tool') {
      const availableWidth = workspaceContentWidth(workspaceWidth.value || workspaceRef.value?.getBoundingClientRect().width || 0);
      const leftReserve = hasWorkspaceLeftPanels.value
        ? leftColumnWidth.value + workspaceSeparatorSize
        : 0;
      const maxToolWidth = Math.max(
        toolPanelMinWidth.value,
        availableWidth - leftReserve - rightColumnMinWidth - workspaceSeparatorSize
      );
      pendingToolWidth = Math.min(
        maxToolWidth,
        Math.max(toolPanelMinWidth.value, startToolWidth - (moveEvent.clientX - startX))
      );
    }

    if (rafId === null) {
      rafId = requestAnimationFrame(updateCssVariable);
    }
  };

  const onMouseUp = () => {
    document.removeEventListener('mousemove', onMouseMove);
    document.removeEventListener('mouseup', onMouseUp);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    isDraggingColumn.value = false;

    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }

    if (workspaceEl) {
      workspaceEl.style.removeProperty('--drag-left-width');
      workspaceEl.style.removeProperty('--drag-tool-width');
    }

    updateLayout();
    clampWorkspaceLayout();
    notifyTerminalLayoutDragging(false, { source: 'workspace-column', deferFit: true });
    nextTick(() => requestAnimationFrame(notifyTerminalLayoutResize));
  };

  document.addEventListener('mousemove', onMouseMove);
  document.addEventListener('mouseup', onMouseUp);
};

const workspaceGridStyle = computed(() => {
  const widthState = getWorkspaceWidthState();
  const { hasLeft, hasRight } = widthState;

  if (!hasLeft && !hasRight) {
    return {
      gridTemplateColumns: 'minmax(0, 1fr)',
      gridTemplateRows: '1fr'
    };
  }

  if (widthState.proportional) {
    const proportionalColumns = [];
    if (hasLeft) {
      proportionalColumns.push(
        `minmax(0, var(--drag-left-width, ${widthState.leftWidth}fr))`,
        `${workspaceSeparatorSize}px`
      );
    }
    proportionalColumns.push(`minmax(0, ${rightColumnMinWidth}fr)`);
    if (hasRight) {
      proportionalColumns.push(
        `${workspaceSeparatorSize}px`,
        `minmax(0, var(--drag-tool-width, ${widthState.toolWidth}fr))`
      );
    }
    return {
      gridTemplateColumns: proportionalColumns.join(' '),
      gridTemplateRows: '1fr'
    };
  }

  const availableWidth = workspaceContentWidth();
  const columns = [];
  if (hasLeft) {
    const rightReserve = hasRight ? toolPanelWidth.value + workspaceSeparatorSize : 0;
    const maxLeftWidth = Math.max(
      leftColumnMinWidth,
      availableWidth - rightColumnMinWidth - rightReserve - workspaceSeparatorSize
    );
    const leftWidth = Math.max(
      leftColumnMinWidth,
      Math.min(Number.isFinite(leftColumnWidth.value) ? leftColumnWidth.value : leftColumnMinWidth, maxLeftWidth)
    );
    columns.push(`var(--drag-left-width, ${leftWidth}px)`, `${workspaceSeparatorSize}px`);
  }

  columns.push(`minmax(${rightColumnMinWidth}px, 1fr)`);

  if (hasRight) {
    const leftReserve = hasLeft ? leftColumnWidth.value + workspaceSeparatorSize : 0;
    const maxRightWidth = Math.max(
      toolPanelMinWidth.value,
      availableWidth - leftReserve - rightColumnMinWidth - workspaceSeparatorSize
    );
    const currentToolWidth = Math.max(
      toolPanelMinWidth.value,
      Math.min(toolPanelWidth.value, maxRightWidth)
    );
    columns.push(`${workspaceSeparatorSize}px`, `var(--drag-tool-width, ${currentToolWidth}px)`);
  }

  return {
    gridTemplateColumns: columns.join(' '),
    gridTemplateRows: '1fr'
  };
});

const workspaceLeftStackStyle = computed(() => {
  return {
    gridTemplateRows: 'minmax(0, 1fr)'
  };
});

function openSftpPanel() {
  if (!activeSessionSupportsSftp.value || visibleSessions.value.length === 0) return;
  showSftpPanel.value = true;
}

function toggleSftpPanel() {
  if (showSftpPanel.value) {
    showSftpPanel.value = false;
    return;
  }
  openSftpPanel();
}

function toggleCommandKnowledgePanel() {
  showCommandKnowledgePanel.value = !showCommandKnowledgePanel.value;
  if (showCommandKnowledgePanel.value) {
    isSettingsVisible.value = false;
  }
}

function buildActiveSessionDraft() {
  const activeSession = activeKey.value ? sshStore.getSession(activeKey.value) : null;
  if (!activeSession?.config) return null;

  const draft = JSON.parse(JSON.stringify(activeSession.config));
  draft.id = draft.id || activeSession.id || crypto.randomUUID();
  draft.name = draft.name || activeSession.name || `${draft.username || ''}@${draft.host || ''}`;
  return draft;
}

async function saveCurrentSessionFromMenu() {
  const draft = buildActiveSessionDraft();
  if (!draft) {
    toast.info('当前没有可保存的会话');
    return;
  }
  await sshStore.saveSessionToStorage(draft);
}

async function saveAllSessionsFromMenu() {
  const drafts = [];
  const seenIds = new Set();

  for (const session of sshStore.sessions || []) {
    if (!session?.config) continue;
    const draft = JSON.parse(JSON.stringify(session.config));
    draft.id = draft.id || session.id || crypto.randomUUID();
    draft.name = draft.name || session.name || `${draft.username || ''}@${draft.host || ''}`;
    if (seenIds.has(draft.id)) continue;
    seenIds.add(draft.id);
    drafts.push(draft);
  }

  if (drafts.length === 0) {
    toast.info('当前没有可保存的活动会话');
    return;
  }

  let successCount = 0;
  for (const draft of drafts) {
    try {
      await invokeCommand('save_session', { session: draft });
      successCount += 1;
    } catch (error) {
      console.error('Save session from menu failed:', error);
    }
  }

  await sshStore.loadSavedSessions();

  if (successCount === drafts.length) {
    toast.success(`已保存 ${successCount} 个活动会话`);
  } else if (successCount > 0) {
    toast.warning(`已保存 ${successCount}/${drafts.length} 个活动会话`);
  } else {
    toast.error('保存活动会话失败');
  }
}

async function reconnectCurrentSessionFromMenu() {
  if (!activeKey.value) {
    toast.info('当前没有激活会话');
    return;
  }
  await sshStore.reconnectSession(activeKey.value);
}

async function reconnectAllSessionsFromMenu() {
  await sshStore.reconnectAllSessions();
}

function openActiveSessionEditor() {
  const draft = buildActiveSessionDraft();
  if (!draft) {
    toast.info('当前没有可编辑的会话');
    return;
  }
  showSessionModal(draft);
}

function showHelpDialog() {
  confirm({
    title: 'DuskTerm 使用说明',
    content: h('div', { class: 'menu-help-dialog' }, [
      h('p', 'DuskTerm 提供 SSH 会话管理、分屏终端、SFTP 文件管理、端口隧道、安全拦截与状态监控。'),
      h('ul', [
        h('li', '“文件”用于新建连接、打开会话列表、保存活动会话和进入首选项。'),
        h('li', '“编辑”用于操作当前激活终端：复制、粘贴、全选、清屏和查找。'),
        h('li', '“视图”用于切换会话列表、文件管理、缩放和全屏。'),
        h('li', '“连接”用于重连、断开、同步输入、隧道管理以及编辑当前会话配置。')
      ])
    ]),
    okText: '知道了',
    cancelText: '',
  });
}

function showAboutDialog() {
  confirm({
    title: '关于 DuskTerm',
    content: h('div', { class: 'menu-help-dialog' }, [
      h('p', 'DuskTerm'),
      h('p', '基于 Tauri 2 + Vue 3 的桌面 SSH / SFTP 运维工具。'),
      h('p', '当前项目包含会话管理、分屏终端、SFTP、隧道、桌宠与状态栏监控能力。')
    ]),
    okText: '关闭',
    cancelText: '',
  });
}

// Defer saved-session loading to after first paint (prevents blocking the initial render)
const _deferLoadSessions = () => {
  sshStore.loadSavedSessions();
};
// Use requestIdleCallback if available, otherwise setTimeout(0)
if (typeof requestIdleCallback === 'function') {
  requestIdleCallback(_deferLoadSessions);
} else {
  setTimeout(_deferLoadSessions, 0);
}

const { activeKey, visibleSessions, setActivePanel, movePanel } = useTerminalPanels(sshStore);
const recentSessionSettings = computed(() => mainUiSettings.value.recentSessions);
const recentSessions = computed(() => {
  if (!recentSessionSettings.value.enabled) return [];
  const list = (sshStore.savedSessions || []).filter((session) => Number(session?.last_connected || 0) > 0);
  list.sort((a, b) => (b.last_connected || 0) - (a.last_connected || 0));
  return list.slice(0, recentSessionSettings.value.limit);
});

const openRecentSession = (session) => {
  if (session?.id) {
    sshStore.connectStoredSession(session.id);
  }
};

const recentClearing = ref(false);
const getRecentSessionIcon = (session) => {
  const protocol = String(session?.protocol || 'ssh').toLowerCase();
  if (protocol === 'telnet') return Server;
  if (protocol === 'serial') return Usb;
  return Code2;
};
const clearRecentSessionHistory = async () => {
  if (recentClearing.value || recentSessions.value.length === 0) return;
  recentClearing.value = true;
  try {
    await sshStore.clearRecentSessions();
  } finally {
    recentClearing.value = false;
  }
};

const { ensureSplitSession } = useTerminalConnection({ sshStore, activeKey });

const {
  splitTrees,
  focusedLeaf,
  ensureTree,
  setFocused,
  splitActive,
  mergeToSingle,
  closeCurrentPanel,
  closeLeaf,
  removePanelRoot,
  startSplitDrag
} = usePanelLayout({
  sshStore,
  activeKey,
  ensureSplitSession,
  visibleSessions
});

const focusedTerminalRuntimeId = computed(() => resolveFocusedSessionId(activeKey.value, focusedLeaf.value));
const activeSftpWorkspaceId = computed(() => activeKey.value || '');
const activeSessionSupportsSftp = computed(() => {
  const session = sshStore.getSession(activeKey.value);
  return String(session?.config?.protocol || 'ssh').toLowerCase() === 'ssh';
});
const showActiveSftpPanel = computed(
  () => showSftpPanel.value && visibleSessions.value.length > 0 && activeSessionSupportsSftp.value
);
watch(
  [showSessionPanel, showCommandKnowledgePanel, showActiveSftpPanel],
  ([showLeft, showKnowledge, showSftp]) => {
    if (showLeft || showKnowledge || showSftp) nextTick(measureWorkspace);
  }
);
getActiveSftpWorkspaceId = () => activeSftpWorkspaceId.value;
watch(activeSftpWorkspaceId, async (sessionId) => {
  showSftpPanel.value = sftpPanelVisibility.get(sessionId);
  await nextTick();
  if (showSftpPanel.value) {
    measureWorkspace();
    window.dispatchEvent(new CustomEvent('app:sftp-layout-refresh'));
  }
}, { immediate: true });

function dispatchCommandKnowledgeToActiveTerminal(mode, detail = {}) {
  const targetSessionId = focusedLeaf.value?.[activeKey.value] || activeKey.value;
  if (!targetSessionId) {
    toast.info('当前没有可用的终端会话');
    return;
  }

  window.dispatchEvent(new CustomEvent('command-knowledge-insert', {
    detail: {
      ...detail,
      sessionId: targetSessionId,
      execute: mode === 'execute',
    }
  }));
}

const {
  syncChannels,
  selectedSyncChannelId,
  replaceSyncChannels,
  clearSyncChannels,
  setSelectedSyncChannelId,
} = useInputRouter({ sshStore });

const isSyncInputVisible = ref(false);

const mainUiSettings = ref(loadMainUiSettings());
const backgroundAvailable = ref(false);
const backgroundActive = computed(() => mainUiSettings.value.background?.enabled && backgroundAvailable.value);
watch(backgroundActive, () => {
  requestAnimationFrame(() => {
    window.dispatchEvent(new CustomEvent('main-ui-settings-changed', {
      detail: {
        preview: true,
        settings: mainUiSettings.value
      }
    }));
  });
});
const desktopPetSettings = computed(() => mainUiSettings.value.desktopPet || {});
const isAnyModalOpen = computed(() =>
  isSettingsVisible.value
  || isSessionModalVisible.value
  || isTunnelModalVisible.value
  || isSyncInputVisible.value
  || isModalVisible.value
  || isOverviewVisible.value
);
const isOverviewVisible = ref(false);
let mainUiSettingsPersistTimer = null;

const persistMainUiSettings = (nextSettings, dispatchEvent = true) => {
  mainUiSettings.value = nextSettings;
  if (mainUiSettingsPersistTimer) clearTimeout(mainUiSettingsPersistTimer);
  mainUiSettingsPersistTimer = setTimeout(() => {
    saveMainUiSettings(mainUiSettings.value);
    if (dispatchEvent) {
      window.dispatchEvent(new CustomEvent('main-ui-settings-changed'));
    }
  }, 180);
};

const handleDesktopPetSettingsChange = (nextDesktopPetSettings) => {
  persistMainUiSettings({
    ...mainUiSettings.value,
    desktopPet: nextDesktopPetSettings
  }, false);
};

onApp('app:open-sync-input', () => {
  isSyncInputVisible.value = true;
});

// --- Modal State ---
const isModalVisible = ref(false);
const confirmLoading = ref(false);
// Hostkey prompt — uses AlertDialog directly (not imperative confirm())
const hostkeyPrompt = ref({
  visible: false,
  title: '',
  host: '',
  port: 22,
  algorithm: '',
  fingerprint: '',
  warning: '',
  confirmCommand: '',
  sessionId: '',
});
const formState = ref({
  host: '',
  port: 22,
  username: 'root',
  password: ''
});

// --- Actions ---
const showModal = () => {
  isModalVisible.value = true;
};

const handleConnect = async () => {
  confirmLoading.value = true;
  try {
    const config = { ...formState.value, port: Number(formState.value.port) };
    await sshStore.connectLogic(config);
    isModalVisible.value = false;
  } catch (err) {
    toast.error(`连接失败：${err}`);
  } finally {
    confirmLoading.value = false;
  }
};

// --- Keybindings ---
const normalizeCombo = (combo) => String(combo || '').trim().replace(/\s+/g, '').toLowerCase();

const keybindingDefault = getPreferenceDefaults('keybindings');
const keybindings = ref({ ...keybindingDefault });

const loadKeybindings = () => {
  try {
    keybindings.value = { ...keybindingDefault, ...loadPreference('keybindings') };
  } catch (e) {
    keybindings.value = { ...keybindingDefault };
  }
};

// Action handlers mapped to SettingsModal key names
const keybindingActions = {
  splitHorizontal: () => splitActive('horizontal'),
  splitVertical: () => splitActive('vertical'),
  closeSession: () => { if (activeKey.value) removePanelRoot(activeKey.value); },
  closeSplitTerminal: () => closeCurrentPanel(),
  nextSession: () => {
    const list = visibleSessions.value;
    if (list.length > 1) {
      const idx = list.findIndex(s => s.id === activeKey.value);
      const next = list[(idx + 1) % list.length];
      if (next) activeKey.value = next.id;
    }
  },
  prevSession: () => {
    const list = visibleSessions.value;
    if (list.length > 1) {
      const idx = list.findIndex(s => s.id === activeKey.value);
      const prev = list[(idx - 1 + list.length) % list.length];
      if (prev) activeKey.value = prev.id;
    }
  },
  sessionList: () => {
    showSessionPanel.value = !showSessionPanel.value;
    if (showSessionPanel.value) sshStore.loadSavedSessions();
  },
  sftpPanel: () => { toggleSftpPanel(); },
  commandKnowledge: () => { toggleCommandKnowledgePanel(); },
  transferList: () => { toggleTransferPanelVisibility(); },
  overview: () => { isOverviewVisible.value = !isOverviewVisible.value; },
  copySession: () => {
    const active = sshStore.getSession(activeKey.value);
    if (active?.config) sshStore.connectLogicWithMeta(active.config);
  },
  toggleLineNumbers: () => {
    window.dispatchEvent(new CustomEvent('terminal:toggle-line-numbers'));
  },
  toggleFind: () => {
    window.dispatchEvent(new CustomEvent('app:terminal-find'));
  },
};

// Build normalized combo → action key lookup, reactively updated when keybindings change
const comboActionMap = computed(() => {
  const map = new Map();
  for (const [actionKey, combo] of Object.entries(keybindings.value)) {
    if (!combo || !String(combo).trim()) continue;
    const normalized = normalizeCombo(combo);
    if (normalized && keybindingActions[actionKey]) {
      map.set(normalized, actionKey);
    }
  }
  return map;
});

const normalizeKeyEvent = (e) => {
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');

  const modifierOnlyKeys = ['Control', 'Shift', 'Alt', 'Meta'];
  if (modifierOnlyKeys.includes(e.key)) {
    return '';
  }

  let key = e.key === ' ' ? 'Space' : e.key;
  if (key === 'Esc') key = 'Escape';
  if (key === '+' || key === '=') {
    key = '+';
    const shiftIndex = parts.indexOf('Shift');
    if (shiftIndex >= 0) parts.splice(shiftIndex, 1);
  }
  if (key === '_' || key === '-') {
    key = '-';
    const shiftIndex = parts.indexOf('Shift');
    if (shiftIndex >= 0) parts.splice(shiftIndex, 1);
  }
  if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  return parts.join('+');
};

const handleGlobalKeydown = (e) => {
  // 设置面板打开时不执行全局快捷键，避免录制快捷键时被已保存的绑定拦截
  if (isSettingsVisible.value) return;

  // Fast-path: skip unmodified single keys — vast majority of keystrokes
  if (!e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) return;

  // Check user-configured keybindings
  const normalized = normalizeKeyEvent(e).toLowerCase();
  if (normalized) {
    const actionKey = comboActionMap.value.get(normalized);
    if (actionKey && keybindingActions[actionKey]) {
      e.preventDefault();
      e.stopPropagation();
      keybindingActions[actionKey]();
      return;
    }
  }
};

let unlistenHostkey = null;
let unlistenSftpProgress = null;

const showHostkeyPrompt = (payload) => {
  if (!payload || !payload.sessionId) return;
  hostkeyPrompt.value = {
    visible: true,
    title: payload.warning ? '⚠️ 主机指纹不匹配' : '首次连接主机指纹确认',
    host: payload.host || '',
    port: payload.port || 22,
    algorithm: payload.algorithm || '',
    fingerprint: payload.fingerprint || '',
    warning: payload.warning || '',
    confirmCommand: payload.confirmCommand || 'confirm_hostkey',
    sessionId: payload.sessionId,
  };
};

const onHostkeyAccept = async () => {
  const p = hostkeyPrompt.value;
  hostkeyPrompt.value.visible = false;
  try {
    await invokeCommand(p.confirmCommand, { sessionId: p.sessionId, accept: true });
  } catch (e) {
    toast.error(`确认失败: ${e}`);
  }
};

const onHostkeyReject = async () => {
  const p = hostkeyPrompt.value;
  hostkeyPrompt.value.visible = false;
  try {
    await invokeCommand(p.confirmCommand, { sessionId: p.sessionId, accept: false });
  } catch (e) {
    toast.error(`确认失败: ${e}`);
  }
};

const refreshMainUiSettings = (event) => {
  const previewSettings = event?.detail?.settings;
  mainUiSettings.value = previewSettings
    ? normalizeMainUiSettings(previewSettings)
    : loadMainUiSettings();
};

onMounted(async () => {
  const unlistenSshHostkey = await listenEvent('ssh-hostkey-request', showHostkeyPrompt);
  const unlistenSftpHostkey = await listenEvent('sftp-hostkey-request', showHostkeyPrompt);
  unlistenSftpProgress = await listenEvent('sftp-progress', transferStore.applyProgress);
  unlistenHostkey = () => {
    unlistenSshHostkey?.();
    unlistenSftpHostkey?.();
  };
  sshStore.loadSavedSessions();
  loadKeybindings();
  refreshMainUiSettings();
  // Scope storage events to relevant keys to avoid unnecessary work
  const _onStorage = (e) => {
    if (!e.key || e.key.startsWith('main-ui') || e.key.startsWith('keybinding')) {
      refreshMainUiSettings();
    }
  };
  window.addEventListener('storage', _onStorage);
  _appEvts.push(['storage', _onStorage]);
  window.addEventListener('keybindings-changed', loadKeybindings);
  window.addEventListener('main-ui-settings-changed', refreshMainUiSettings);
  window.addEventListener('keydown', handleGlobalKeydown, true);

});

onUnmounted(() => {
  if (unlistenHostkey) unlistenHostkey();
  if (unlistenSftpProgress) unlistenSftpProgress();
  if (sftpPanelTransitionTimer) {
    clearTimeout(sftpPanelTransitionTimer);
    sftpPanelTransitionTimer = null;
  }
  window.removeEventListener('keybindings-changed', loadKeybindings);
  window.removeEventListener('main-ui-settings-changed', refreshMainUiSettings);
  window.removeEventListener('keydown', handleGlobalKeydown, true);
  if (mainUiSettingsPersistTimer) clearTimeout(mainUiSettingsPersistTimer);
  // Cleanup all app:* / gesture / storage event listeners
  for (const [event, handler] of _appEvts) {
    window.removeEventListener(event, handler);
  }
  _appEvts.length = 0;
});

useWindowInteraction({ onResize: measureWorkspace });

// --- Panel Bar DnD & Context Menu ---
const onPanelDrop = ({ dragId, dropId }) => {
  movePanel(dragId, dropId);
};

const onPanelContext = ({ key, panel }) => {
  handleTabContext(key, panel);
};

const copyToClipboard = async (text) => {
  try {
    await navigator.clipboard.writeText(text);
    toast.success('已复制');
  } catch (err) {
    toast.error('复制失败');
  }
};

const handleTabContext = (key, session) => {
  if (key === 'copy-session') {
    if (session.config) {
      sshStore.connectLogicWithMeta(session.config);
    }
  } else if (key === 'copy-name') {
    copyToClipboard(session.name);
  } else if (key === 'copy-ip') {
    const host = session.host || session.config?.host;
    if (host) copyToClipboard(host);
  } else if (key === 'close') {
    onEdit(session.id, 'remove');
  }
};

</script>

<template>
  <TooltipProvider :delay-duration="200">
    <ConfirmDialog />

    <!-- Hostkey fingerprint confirmation (declarative, not imperative confirm) -->
    <AlertDialog v-model:open="hostkeyPrompt.visible">
      <AlertDialogContent :z-index="2400">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ hostkeyPrompt.title }}</AlertDialogTitle>
          <AlertDialogDescription>
            <p>主机: {{ hostkeyPrompt.host }}:{{ hostkeyPrompt.port }}</p>
            <p>算法: {{ hostkeyPrompt.algorithm }}</p>
            <p style="word-break:break-all">指纹(SHA256): {{ hostkeyPrompt.fingerprint }}</p>
            <p v-if="hostkeyPrompt.warning"
              style="color: hsl(var(--destructive)); margin-top: 8px; white-space: pre-wrap;">{{ hostkeyPrompt.warning
              }}</p>
            <p v-else style="color: hsl(var(--muted-foreground));">请确认指纹是否可信。接受后将保存到本地 known_hosts。</p>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel @click="onHostkeyReject">拒绝</AlertDialogCancel>
          <AlertDialogAction @click="onHostkeyAccept">
            {{ hostkeyPrompt.warning ? '仍接受（更新密钥）' : '接受并保存' }}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
    <div class="app-shell has-floating-surfaces" :class="{ 'has-global-background': backgroundActive }">
      <GlobalBackground :settings="mainUiSettings.background" @availability-change="backgroundAvailable = $event" />
      <CustomTitlebar :sftp-active="showActiveSftpPanel"
        :sftp-disabled="visibleSessions.length === 0 || !activeSessionSupportsSftp"
        :knowledge-active="showCommandKnowledgePanel"
        :session-panel-active="showSessionPanel"
        :transfer-visible="transferPanelVisible"
        :keybindings="keybindings"
        @toggle-sftp="toggleSftpPanel"
        @toggle-transfer="toggleTransferPanelVisibility" />
      <div class="global-toast-layer" aria-live="polite" aria-atomic="false">
        <ToastContainer />
      </div>
      <div ref="workspaceRef" class="workspace-grid main-only"
        :class="{ 'has-left-panels': hasWorkspaceLeftPanels, 'has-right-panel': hasWorkspaceRightPanel }"
        :style="workspaceGridStyle">
        <!-- Left: Session List (inline panel, not overlay) -->
        <div v-if="hasWorkspaceLeftPanels" class="workspace-left-stack" :style="workspaceLeftStackStyle">
          <TiledPanel class="workspace-panel workspace-panel-session" :dense="true" :padded="false">
            <SessionList :width="'100%'"
              @open-create="() => { showSessionModal(null); }" @open-edit="(s) => { openSavedSessionEditor(s); }"
              @close="showSessionPanel = false" />
          </TiledPanel>
        </div>

        <!-- Column resize handle -->
        <div v-if="hasWorkspaceLeftPanels" class="workspace-separator workspace-separator-vertical"
          @mousedown="startWorkspaceResize('columns', $event)" />

        <TiledPanel class="workspace-panel workspace-panel-main"
          :class="{ 'is-empty': visibleSessions.length === 0 }"
          :dense="true" :padded="false" aria-label="主界面面板">
          <div class="main-panel-body">
            <div v-if="visibleSessions.length === 0" class="empty-state">
              <div class="empty-left">
                <div v-if="recentSessionSettings.enabled" class="recent-sessions-tree-container">
                  <div class="tree-header">
                    <span>最近会话</span>
                    <button v-if="recentSessions.length" type="button" class="recent-clear-button"
                      :disabled="recentClearing" title="清空最近会话" aria-label="清空最近会话"
                      @click.stop="clearRecentSessionHistory">
                      <Trash2 :size="14" />
                    </button>
                  </div>
                  <div v-if="recentSessions.length === 0" class="no-recent">无记录</div>
                  <div v-else class="tree-list">
                    <button v-for="s in recentSessions" :key="s.id" type="button" class="tree-item"
                      @click="openRecentSession(s)">
                      <component :is="getRecentSessionIcon(s)" class="tree-icon" />
                      <span class="tree-text">{{ s.name || `${s.username}@${s.host}` }}</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
            <TerminalPanelManager v-else :panels="visibleSessions" :active-panel-id="activeKey"
              :split-trees="splitTrees" :focused-leaf="focusedLeaf" :resolve-tree="ensureTree"
              :on-split-drag="startSplitDrag" :on-set-focused="setFocused" @activate="setActivePanel"
              @close-panel="removePanelRoot" @tab-drop="onPanelDrop" @tab-context="onPanelContext" />

            <DesktopPet :settings="desktopPetSettings" :suspend="isAnyModalOpen || hasWorkspaceRightPanel"
              @settings-change="handleDesktopPetSettingsChange" />
          </div>
        </TiledPanel>

        <div v-if="hasWorkspaceRightPanel" class="workspace-separator workspace-separator-vertical"
          @mousedown="startWorkspaceResize('tool', $event)" />

        <TiledPanel v-show="hasWorkspaceRightPanel" class="workspace-panel workspace-panel-tool" :dense="true"
          :padded="false" :aria-label="showActiveSftpPanel ? 'SFTP 文件管理' : '命令知识库'">
          <KeepAlive>
            <FileManager v-if="showActiveSftpPanel"
              :sessionId="activeSftpWorkspaceId" :follow-session-id="focusedTerminalRuntimeId"
              :visible="showActiveSftpPanel" @close="showSftpPanel = false" />
            <CommandKnowledgePanel v-else-if="showCommandKnowledgePanel" @close="showCommandKnowledgePanel = false"
              @insert-command="(detail) => dispatchCommandKnowledgeToActiveTerminal('insert', detail)"
              @execute-command="(detail) => dispatchCommandKnowledgeToActiveTerminal('execute', detail)" />
          </KeepAlive>
        </TiledPanel>
      </div>

      <TransferPanel v-if="transferPanelVisible" :expanded="transferPanelExpanded"
        @toggle="transferPanelExpanded = !transferPanelExpanded"
        @close="closeTransferPanel" />

      <!-- Session Modal -->
      <SessionModal v-model:visible="isSessionModalVisible" :sessionData="currentEditSession" />

      <TunnelModal v-model:visible="isTunnelModalVisible" :preferred-session-id="preferredTunnelSessionId" />

      <!-- Settings Modal -->
      <SettingsModal v-model:visible="isSettingsVisible" />

      <SyncInputModal v-model:visible="isSyncInputVisible" :active-key="activeKey" :sync-channels="syncChannels"
        :selected-channel-id="selectedSyncChannelId"
        :replace-sync-channels="replaceSyncChannels" :clear-sync-channels="clearSyncChannels"
        :set-selected-sync-channel-id="setSelectedSyncChannelId" />

      <!-- Security Lock Screen -->
      <LockScreen />

      <!-- Session Overview (Ctrl+`) -->
      <SessionOverview :visible="isOverviewVisible" :sessions="visibleSessions" :sync-channels="syncChannels"
        :active-session-id="activeKey"
        @close="isOverviewVisible = false" @select="(id) => { setActivePanel(id); }" />

    </div>

  </TooltipProvider>
</template>

<style>
:root {
  --app-gutter: 16px;
  --command-bar-height: 52px;
  --sider-min-width: 260px;
}

.logo {
  height: 64px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-bottom: 1px solid var(--app-border-shadow, #E4E4E6);
}

.logo h2 {
  margin: 0;
  font-size: 18px;
  font-weight: bold;
  color: var(--color-primary);
}

.sider-content {
  padding: 16px;
}

.workspace-grid {
  flex: 1;
  display: grid;
  grid-template-columns: minmax(320px, 30vw) minmax(0, 1fr);
  gap: 6px;
  min-height: 0;
  padding: 12px;
  box-sizing: border-box;
  align-items: stretch;
  background: var(--app-workspace-bg);
  box-shadow: inset 0 0 0 1px var(--app-workspace-gap);
  contain: layout style;
}

.app-shell {
  position: relative;
  z-index: 0;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: hsl(var(--background));
  isolation: isolate;
}

.global-toast-layer {
  position: relative;
  z-index: var(--z-alert);
  display: flex;
  width: 100%;
  height: 21px;
  flex: 0 0 21px;
  align-items: center;
  justify-content: flex-end;
  padding: 0 4px;
  box-sizing: border-box;
  pointer-events: none;
  overflow: hidden;
}

.app-shell > :not(.global-background) { position: relative; z-index: 1; }
.app-shell.has-floating-surfaces {
  --terminal-surface-bg: color-mix(in srgb, var(--app-bg-dialog) 52%, transparent);
  --workspace-side-panel-top-offset: 0px;
  --tb-bg: color-mix(in srgb, var(--app-bg-dialog) 78%, transparent);
  --tb-border: var(--app-panel-border);
  --tb-shadow: var(--app-panel-shadow);
}
.app-shell.has-global-background {
  background: transparent;
}
.app-shell.has-floating-surfaces .workspace-grid {
  padding: 0 4px 4px;
  background: transparent;
  box-shadow: none;
}
.app-shell.has-floating-surfaces .workspace-panel.tiled-panel:not(.workspace-panel-main) {
  background: color-mix(in srgb, var(--app-bg-dialog) 78%, transparent);
}
.app-shell.has-floating-surfaces .workspace-panel-main.tiled-panel {
  background: transparent;
  border-color: var(--app-panel-border);
  box-shadow: var(--app-panel-shadow);
}
.app-shell.has-floating-surfaces .workspace-panel-main.tiled-panel.is-empty {
  border-color: transparent;
  box-shadow: none;
}
.app-shell.has-floating-surfaces .workspace-panel-main :deep(.tiled-panel__body) {
  padding: 0;
}
.app-shell.has-floating-surfaces .workspace-panel-session,
.app-shell.has-floating-surfaces .workspace-panel-tool {
  margin-top: var(--workspace-side-panel-top-offset);
  height: calc(100% - var(--workspace-side-panel-top-offset));
}
.app-shell.has-floating-surfaces .file-manager,
.app-shell.has-floating-surfaces .fm-table-header,
.app-shell.has-floating-surfaces .fm-load-more {
  background: color-mix(in srgb, var(--app-bg-dialog) 80%, transparent) !important;
}
/* will-change removed: triggers excessive GPU layering on iGPU,
   causing re-rasterization of the entire tiled workspace on every drag frame */

.workspace-grid:not(.has-left-panels) {
  grid-template-columns: minmax(0, 1fr);
}

.workspace-grid.has-right-panel {
  grid-auto-rows: minmax(0, 1fr);
}

.workspace-grid.has-right-panel .desktop-pet-layer {
  display: none;
}

.workspace-left-stack {
  display: grid;
  gap: 6px;
  min-width: 0;
  min-height: 0;
  height: 100%;
  contain: layout style;
}

.workspace-grid.has-left-panels {
  grid-auto-rows: minmax(0, 1fr);
}

.workspace-panel {
  min-width: 0;
  min-height: 0;
  height: 100%;
}

.workspace-panel-main {
  min-width: 0;
  overflow: hidden;
}

.workspace-panel-tool {
  min-width: 0;
  overflow: hidden;
  position: relative;
  z-index: 30;
}

.workspace-panel-tool :deep(.tiled-panel__body) {
  padding: 0;
  overflow: hidden;
}

.workspace-panel-session :deep(.session-panel),
.workspace-panel-tool :deep(.command-knowledge-panel),
.workspace-panel-tool :deep(.file-manager) {
  width: 100% !important;
  height: 100%;
}

.workspace-panel-session :deep(.session-panel > *) {
  width: 100%;
}

.workspace-separator {
  background: transparent;
  border: 0;
  margin: 0;
  padding: 0;
  opacity: 0;
  user-select: none;
  touch-action: none;
}

.workspace-separator:hover,
.workspace-separator:active,
.workspace-separator:focus {
  background: transparent;
  opacity: 0;
}

.workspace-separator-vertical {
  width: 4px;
  cursor: col-resize;
}

.workspace-separator-horizontal {
  height: 4px;
  cursor: row-resize;
}

.workspace-separator:focus-visible {
  outline: none;
}

.main-panel-body {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  height: 100%;
}

.empty-state {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: center;
  gap: 80px;
  overflow: hidden;
}

.empty-left {
  width: min(340px, calc(100% - 24px));
  min-width: 0;
  height: 260px;
  background: transparent;
  border-right: none;
  display: flex;
  flex-direction: column;
  padding: 0;
  overflow-y: hidden;
}

/* Tree Styles */
.recent-sessions-tree-container {
  display: flex;
  flex-direction: column;
  gap: 4px;
  height: 100%;
  padding: 12px;
  border: 1px solid color-mix(in srgb, var(--app-border-light) 70%, transparent);
  border-radius: var(--niri-radius-lg, 14px);
  background: color-mix(in srgb, var(--app-bg-dialog) 54%, transparent);
  box-shadow: 0 8px 28px color-mix(in srgb, var(--app-workspace-gap, #000) 18%, transparent);
  box-sizing: border-box;
}

.tree-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: bold;
  font-size: 14px;
  color: var(--app-text-muted);
  border-bottom: 1px solid var(--app-border-light);
  padding-bottom: 8px;
  margin-bottom: 8px;
}

.recent-clear-button {
  display: inline-flex;
  width: 24px;
  height: 24px;
  align-items: center;
  justify-content: center;
  margin: -4px -4px -4px 8px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  color: var(--app-text-muted);
  background: transparent;
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--app-motion-control), color var(--app-motion-control), background var(--app-motion-control);
}

.recent-sessions-tree-container:hover .recent-clear-button,
.recent-clear-button:focus-visible {
  opacity: 1;
}

.recent-clear-button:hover {
  color: var(--color-danger);
  background: color-mix(in srgb, var(--color-danger) 12%, transparent);
}

.recent-clear-button:disabled {
  cursor: wait;
  opacity: .45;
}

.tree-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  /* Fill height */
  overflow-y: auto;
}

.tree-item {
  display: flex;
  width: 100%;
  align-items: center;
  padding: 4px 8px;
  border: 0;
  cursor: pointer;
  border-radius: 2px;
  color: var(--app-text);
  background: transparent;
  font-size: 13px;
  text-align: left;
  transition: background 0.1s;
}

.tree-item:hover,
.tree-item:focus-visible {
  outline: none;
  background: var(--app-selection-bg);
  color: var(--app-selection-text);
}

.tree-icon {
  margin-right: 8px;
  font-size: 14px;
  opacity: 0.7;
}

.tree-text {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tree-time {
  font-size: 11px;
  opacity: 0.5;
  margin-left: 8px;
}

.no-recent {
  font-style: italic;
  color: var(--app-text-muted);
  padding: 8px;
}

.welcome-text {
  text-align: center;
  color: var(--app-text-muted);
  font-family: var(--font-mono);
}

.welcome-text h3 {
  margin: 0 0 8px 0;
  font-size: 24px;
  text-transform: uppercase;
  letter-spacing: 2px;
  color: var(--color-success);
  text-shadow: 0 0 10px rgba(82, 196, 26, 0.5);
}

/* Dark mode adjustments for empty state parts that rely on XP vars */
html.dark .empty-left {
  background: transparent;
  border-right: none;
}

/* Remove old empty state styles that conflict */
/* .empty-content, .recent-sessions, etc. are replaced */

.terminal-tabs {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  /* Prevent double scrollbars */
}

.custom-tab-label {
  /* Increase hit area for drag */
  display: inline-block;
  width: 100%;
  height: 100%;
  cursor: grab;
}

.custom-tab-label:active {
  cursor: grabbing;
}

.terminal-split {
  height: 100%;
  display: flex;
  gap: 0;
}

.terminal-split.split-vertical {
  flex-direction: row;
}

.terminal-split.split-horizontal {
  flex-direction: column;
}

.terminal-split .split-pane {
  flex: 0 0 auto;
  min-width: 0;
  min-height: 0;
  border-right: 1px solid var(--app-border-shadow);
  border-bottom: 1px solid var(--app-border-shadow);
}

.split-leaf {
  flex: 1 1 auto;
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
}

.terminal-split.split-vertical .split-pane:last-child {
  border-right: none;
}

.terminal-split.split-horizontal .split-pane:last-child {
  border-bottom: none;
}

.split-divider {
  background: var(--app-bg-dialog);
  position: relative;
  z-index: 5;
  flex: 0 0 auto;
}

.divider-vertical {
  width: 6px;
  cursor: col-resize;
  border-left: 1px solid var(--app-border-shadow);
  border-right: 1px solid var(--app-border-light);
}

.divider-horizontal {
  height: 6px;
  cursor: row-resize;
  border-top: 1px solid var(--app-border-shadow);
  border-bottom: 1px solid var(--app-border-light);
}

.split-focused {
  /* outline: 1px solid var(--app-selection-bg); */
  outline-offset: -1px;
}

.resize-handle {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 4px;
  cursor: col-resize;
  z-index: 100;
  background: var(--app-bg-dialog);
  border-left: 1px solid var(--app-border-shadow);
  transition: background 0.2s;
}

.resize-handle:hover {
  background: var(--app-btn-hover);
}

html.dark .resize-handle {
  background: var(--app-bg-dialog, #282C34);
  border-left: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
}

html.dark .resize-handle:hover {
  background: var(--app-border-shadow, rgba(255, 255, 255, 0.08));
}

.session-list-resize-handle {
  position: absolute;
  right: 0;
  top: 0;
  width: 100%;
  height: 5px;
  cursor: row-resize;
  z-index: 10;
  background: transparent;
  transition: background 0.2s;
}

.session-list-resize-handle:hover,
.session-list-resize-handle:active {
  background: var(--mac-accent);
}

.h-resize-handle {
  height: 4px;
  /* Slightly thinner */
  background: var(--app-bg-dialog);
  border-top: 1px solid var(--app-border-shadow);
  border-bottom: 1px solid var(--app-border-light);
  cursor: row-resize;
  transition: background 0.2s;
  z-index: 10;
}

.h-resize-handle:hover,
.h-resize-handle:active {
  background: var(--app-btn-hover);
}

html.dark .h-resize-handle {
  background: var(--app-bg-dialog, #282C34);
  border-top: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
  border-bottom: 1px solid #000;
}

html.dark .h-resize-handle:hover {
  background: var(--app-border-shadow, rgba(255, 255, 255, 0.08));
}

.cmd-dropdown-btn {
  gap: 6px;
}

.cmd-down {
  font-size: 10px;
  margin-left: 2px;
}

.cmd-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-width: 260px;
}

/* ── Workspace panel session (left sidebar) ── */
.workspace-panel-session {
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
}

.workspace-panel-session :deep(.tiled-panel__body) {
  padding: 0;
  overflow: hidden;
}
</style>
