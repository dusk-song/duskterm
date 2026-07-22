<script setup>
import Button from '@/components/ui/button/Button.vue';
import ContextMenu from '@/components/ui/context-menu/ContextMenu.vue';
import ContextMenuContent from '@/components/ui/context-menu/ContextMenuContent.vue';
import ContextMenuItem from '@/components/ui/context-menu/ContextMenuItem.vue';
import ContextMenuSeparator from '@/components/ui/context-menu/ContextMenuSeparator.vue';
import ContextMenuTrigger from '@/components/ui/context-menu/ContextMenuTrigger.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import { toast } from '@/composables/useToast';
import { save } from '@tauri-apps/plugin-dialog';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal } from '@xterm/xterm';
import {
  CaseSensitive,
  ChevronDown,
  ChevronUp,
  Regex,
  Search,
  WholeWord,
  X
} from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useTheme } from '@/composables/useTheme';
import { useCommandKnowledgeStore } from '@/stores/commandKnowledge';
import { useSecurityStore } from '@/stores/security';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand, listenEvent } from '@/utils/ipc';
import { findMatchedCommandInPayload, matchSensitiveCommand } from '@/utils/sensitiveCommand';
import { getSessionSyncBadgeState, SYNC_INPUT_CHANNELS_STORAGE_KEY } from '@/utils/syncInputChannels';
import {
  buildTerminalLineReplacementPayload,
  extractCommandFromTerminalLine,
  findCommandHistoryMatches,
  normalizeCommandHistory,
  recordCommandHistoryEntry,
} from '@/utils/terminalCommandHistory';
import { getTerminalTheme, loadTerminalThemeSettings } from '@/utils/terminalTheme';

const props = defineProps({
  sessionId: {
    type: String,
    required: true
  }
});

const terminalWrapperRef = ref(null);
const terminalContainer = ref(null);
const contextMenuOpen = ref(false);
const lineNumberGutterRef = ref(null);
// Line numbers: on by default, controlled globally via Settings → Terminal
const _termSettings = loadTerminalThemeSettings();
const lineNumbersEnabled = ref(_termSettings.showLineNumbers !== false); // default true unless explicitly false
const reconnectingAfterDisconnect = ref(false);
const reconnectPromptShown = ref(false);
const terminalTransferRequest = ref(null);
const lineNumberRows = ref([]);
const lineNumberGutterWidth = ref('4ch');
const lineNumberRowHeightPx = ref(18);
const showLineNumberGutter = computed(() => lineNumbersEnabled.value);
const sshStore = useSshStore();
const commandKnowledgeStore = useCommandKnowledgeStore();
const securityStore = useSecurityStore();
const { isDark } = useTheme();
const QUICK_HINT_DEBOUNCE_MS = 90;
const QUICK_HINT_MAX_ITEMS = 24;
const QUICK_HINT_PANEL_MAX_HEIGHT_PX = 200;
const QUICK_HINT_PANEL_MIN_WIDTH_PX = 320;
const QUICK_HINT_PANEL_MARGIN_PX = 10;
const QUICK_HINT_PANEL_GAP_PX = 8;
// --- Security Interceptor ---
const securityModalVisible = ref(false);
const blockedCommandContent = ref('');
const blockedCommandSeverity = ref('warning');
const pendingData = ref(null);
const confirmPassword = ref('');
const currentInputBuffer = ref('');
const quickHintVisible = ref(false);
const quickHintItems = ref([]);
const quickHintSelectedIndex = ref(0);
const quickHintFocused = ref(false);
const quickHintPanelRef = ref(null);
const quickHintPanelStyle = ref({});
const quickHintLastQuery = ref('');
const quickHintLastMatchedIndexes = ref([]);
const commandHistory = ref([]); // [{ cmd: string, count: number }]
const HISTORY_MAX = 200;
const HISTORY_MIN_LEN = 5;
const HISTORY_STORAGE_KEY = 'cmd-history-v1';
const HISTORY_CHANGED_EVENT = 'command-history-changed';
const SERIAL_RECEIVE_VISIBLE_KEY_PREFIX = 'serial-receive-visible-v1:';
const SERIAL_CAPTURE_MAX_CHARS = 5 * 1024 * 1024;
const SERIAL_SAVE_IPC_CHUNK_BYTES = 128 * 1024;
const knowledgeSensitiveRules = computed(() => commandKnowledgeStore.sensitiveRules || []);

const readStoredCommandHistory = () => {
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY);
    return normalizeCommandHistory(raw ? JSON.parse(raw) : []);
  } catch {
    return [];
  }
};

const loadCommandHistory = () => {
  commandHistory.value = readStoredCommandHistory();
};

const persistCommandHistory = () => {
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(commandHistory.value));
  } catch { /* ignore */ }
  window.dispatchEvent(new CustomEvent(HISTORY_CHANGED_EVENT, {
    detail: { history: commandHistory.value },
  }));
};

const onCommandHistoryChanged = (event) => {
  commandHistory.value = normalizeCommandHistory(event?.detail?.history || []);
};
const syncBadgeState = ref({
  visible: false,
  channelId: '',
  channelName: '',
  connectedCount: 0,
  isPrimary: false,
  sourceMode: 'all',
  sendMode: 'realtime',
  broadcastEnabled: false,
});
const nonPrimaryInputWarnAt = ref(0);
const serialReceiveVisible = ref(true);
const serialRawReceiveChunks = ref([]);
const serialIoLogChunks = ref([]);
let serialRawReceiveBytes = 0;
let serialIoLogChars = 0;
const serialTextEncoder = new TextEncoder();

const currentSession = computed(() => sshStore.sessions.find(s => s.id === props.sessionId) || null);
const sessionName = computed(() => currentSession.value?.name || 'Unknown');
const isSerialSession = computed(() => String(currentSession.value?.config?.protocol || '').toLowerCase() === 'serial');

const serialPreferenceKey = () => {
  const config = currentSession.value?.config || {};
  const stableKey = config.id || config.serial_path || props.sessionId;
  return `${SERIAL_RECEIVE_VISIBLE_KEY_PREFIX}${stableKey}`;
};

const loadSerialReceivePreference = () => {
  if (!isSerialSession.value) return;
  try {
    serialReceiveVisible.value = localStorage.getItem(serialPreferenceKey()) !== '0';
  } catch {
    serialReceiveVisible.value = true;
  }
};

const persistSerialReceivePreference = () => {
  if (!isSerialSession.value) return;
  try {
    localStorage.setItem(serialPreferenceKey(), serialReceiveVisible.value ? '1' : '0');
  } catch { /* ignore */ }
};

const appendLimitedSerialText = (chunksRef, text, sizeGetter, sizeSetter) => {
  if (!text) return;
  chunksRef.value.push(text);
  sizeSetter(sizeGetter() + text.length);
  while (sizeGetter() > SERIAL_CAPTURE_MAX_CHARS && chunksRef.value.length > 1) {
    const removed = chunksRef.value.shift() || '';
    sizeSetter(Math.max(0, sizeGetter() - removed.length));
  }
};

const appendLimitedSerialBytes = (chunksRef, bytes) => {
  if (!bytes?.length) return;
  const chunk = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  chunksRef.value.push(chunk);
  serialRawReceiveBytes += chunk.length;
  while (serialRawReceiveBytes > SERIAL_CAPTURE_MAX_CHARS && chunksRef.value.length > 1) {
    const removed = chunksRef.value.shift();
    serialRawReceiveBytes = Math.max(0, serialRawReceiveBytes - (removed?.length || 0));
  }
};

const padSerialTime = (value, width = 2) => String(value).padStart(width, '0');

const formatSerialLogTimestamp = (date = new Date()) => (
  `${date.getFullYear()}-${padSerialTime(date.getMonth() + 1)}-${padSerialTime(date.getDate())} `
  + `${padSerialTime(date.getHours())}:${padSerialTime(date.getMinutes())}:${padSerialTime(date.getSeconds())}.`
  + `${padSerialTime(date.getMilliseconds(), 3)}`
);

const appendSerialReceiveRaw = (bytes) => {
  appendLimitedSerialBytes(serialRawReceiveChunks, bytes);
};

const appendSerialIoLog = (direction, text, byteLength) => {
  const marker = direction === 'SEND' ? '>>>' : '<<<';
  const entry = `[${formatSerialLogTimestamp()}]# ${direction} ASCII/${byteLength} ${marker}\n${text}\n\n`;
  appendLimitedSerialText(
    serialIoLogChunks,
    entry,
    () => serialIoLogChars,
    (value) => { serialIoLogChars = value; }
  );
};

const recordSerialReceive = (text, byteLength, rawBytes = null) => {
  if (!isSerialSession.value || !text) return;
  appendSerialReceiveRaw(rawBytes || serialTextEncoder.encode(text));
  appendSerialIoLog('RECV', text, byteLength);
};

const recordSerialSend = (text, byteLength = serialTextEncoder.encode(text).length) => {
  if (!isSerialSession.value || !text) return;
  appendSerialIoLog('SEND', text, byteLength);
};

const toggleSerialReceiveVisible = () => {
  serialReceiveVisible.value = !serialReceiveVisible.value;
  persistSerialReceivePreference();
  toast.info(serialReceiveVisible.value ? '串口接收数据显示已开启' : '串口接收数据显示已隐藏');
};

const openSecurityModal = (matched, data) => {
  blockedCommandContent.value = matched.content;
  blockedCommandSeverity.value = matched.severity;
  pendingData.value = data;
  confirmPassword.value = '';
  currentInputBuffer.value = '';
  securityModalVisible.value = true;
};

function sendData(data) {
  const session = currentSession.value;
  if (session && (session.status === 'connected' || session.status === 'connecting')) {
    const command = session.isSplitChild ? 'write_ssh_shell_channel' : 'write_ssh';
    const payload = session.isSplitChild
      ? { rootSessionId: session.workspaceSessionId || session.parentId, channelId: props.sessionId, data }
      : { sessionId: props.sessionId, data };
    invokeCommand(command, payload).catch(console.error);
  }
}

const formatCloseReason = (reason) => {
  const text = String(reason || '').trim();
  return text ? `Connection closed by remote host (${text}).` : 'Connection closed by remote host.';
};

const handleTerminalTransferRequest = (request) => {
  terminalTransferRequest.value = request || null;
  if (request?.protocol === 'zmodem') {
    toast.info('暂不支持 ZMODEM，请使用 SFTP 文件面板上传或下载文件');
  }
};

const dismissTerminalTransferUnsupported = () => {
  terminalTransferRequest.value = null;
};

async function reconnectAfterDisconnect() {
  if (reconnectingAfterDisconnect.value) return;
  reconnectingAfterDisconnect.value = true;
  try {
    term?.write('\r\n\x1b[36m正在重连，请稍候...\x1b[0m\r\n');
    markSearchBufferChanged();
    const ok = await sshStore.reconnectSession(props.sessionId);
    if (ok) {
      term?.write('\r\n\x1b[32m已重新连接。\x1b[0m\r\n');
      markSearchBufferChanged();
      currentInputBuffer.value = '';
      closeQuickHint();
      reconnectPromptShown.value = false;
    } else {
      term?.write('\r\n\x1b[31m重连失败，请稍后再试。\x1b[0m\r\n');
      markSearchBufferChanged();
    }
  } finally {
    reconnectingAfterDisconnect.value = false;
  }
}

const isCurrentSessionSyncSource = () => {
  const state = syncBadgeState.value || {};
  if (!state.visible || !state.broadcastEnabled) return false;
  if (state.sourceMode === 'primary') {
    return !!state.isPrimary;
  }
  return true;
};

const shouldLockInputByPrimaryMode = () => {
  const state = syncBadgeState.value || {};
  if (!state.visible || !state.broadcastEnabled) return false;
  if (state.sourceMode !== 'primary') return false;
  return !state.isPrimary;
};

const notifyPrimaryLockIfNeeded = () => {
  const now = Date.now();
  if (now - nonPrimaryInputWarnAt.value < 1000) return;
  nonPrimaryInputWarnAt.value = now;
  toast.info('当前为同步输入主控模式，请在主控会话中输入');
};

const forwardTerminalInput = async (data) => {
  const detail = {
    panelId: props.sessionId,
    payload: data,
    handledByRouter: false,
    respond: null
  };

  let resolved = false;
  let handled = false;
  const waitHandled = new Promise((resolve) => {
    detail.respond = (result) => {
      if (resolved) return;
      resolved = true;
      handled = !!result?.handled;
      resolve(handled);
    };
  });

  window.dispatchEvent(new CustomEvent('terminal-input-route', { detail }));

  if (detail.handledByRouter) {
    await waitHandled;
    if (handled) return;
  }

  sendData(data);
};

const onSyncInputChanged = (event) => {
  const detail = event?.detail || {};
  syncBadgeState.value = getSessionSyncBadgeState(detail.syncChannels || [], props.sessionId);
};

const loadSyncInputState = () => {
  try {
    const raw = localStorage.getItem(SYNC_INPUT_CHANNELS_STORAGE_KEY);
    if (!raw) {
      syncBadgeState.value = getSessionSyncBadgeState([], props.sessionId);
      return;
    }

    const parsed = JSON.parse(raw) || {};
    syncBadgeState.value = getSessionSyncBadgeState(parsed.channels || [], props.sessionId);
  } catch {
    syncBadgeState.value = getSessionSyncBadgeState([], props.sessionId);
  }
};

const getSecurityModalContainer = () => {
  try {
    const doc = globalThis?.document;
    if (doc?.body) {
      return doc.body;
    }
  } catch (error) {
    console.error('Resolve modal container failed:', error);
  }
  return false;
};

async function handleSecurityConfirm() {
  if (blockedCommandSeverity.value === 'critical' && securityStore.hasPassword) {
    if (!confirmPassword.value) {
      toast.error('请输入密码');
      return;
    }
    if (!(await securityStore.verifyPassword(confirmPassword.value))) {
      toast.error('密码错误');
      return;
    }
  }

  if (pendingData.value) {
    recordCommandHistory(blockedCommandContent.value);
    forwardTerminalInput(pendingData.value);
    pendingData.value = null;
  }
  securityModalVisible.value = false;
  confirmPassword.value = '';
  currentInputBuffer.value = '';
  term?.focus();
}

function handleSecurityCancel() {
  pendingData.value = null;
  securityModalVisible.value = false;
  confirmPassword.value = '';
  currentInputBuffer.value = '';
  sendData('\x03');
  term?.focus();
}

function openSettings() {
  pendingData.value = null;
  confirmPassword.value = '';
  currentInputBuffer.value = '';
  sendData('\x03');
  securityModalVisible.value = false;
  window.dispatchEvent(new CustomEvent('app:open-settings'));
}

const terminalCache = globalThis.__sshTerminalCache || (globalThis.__sshTerminalCache = new Map());

let term = null;
let fitAddon = null;
let unicode11Addon = null;
let unlistenData = null;
let unlistenDebug = null;
let unlistenConnected = null;
let unlistenClosed = null;
let unlistenError = null;
let unlistenTerminalTransferRequest = null;
let unlistenSerialDataSent = null;
let resizeObserver = null;
let textDecoder = new TextDecoder('utf-8'); // Default
let quickCommandHandler = null;
let terminalFocusHandler = null;
let quickHintDebounceTimer = null;
let shellCompletionSyncTimer = null;
let shellCompletionSyncPending = false;
let quickHintPositionRafId = null;
let quickHintSearchToken = 0;
let isLayoutDragging = false;
let dragFitRafId = null;
let dragFitTimerId = null;
let lastDragFitAt = 0;
let lastFittedContainerWidth = 0;
let lastFittedContainerHeight = 0;
let lastFitAt = 0;
let deferLayoutFit = false;
let lastProposedCols = 0;
let lastProposedRows = 0;
let lastSentCols = 0;
let lastSentRows = 0;
const DRAG_FIT_MIN_INTERVAL = 30;
const SEARCH_AUTO_REFRESH_DEBOUNCE_MS = 200;
const SEARCH_SELECTION_MAX_LENGTH = 512;
const PHYSICAL_LINE_CHECKPOINT_STEP = 128;
const PHYSICAL_LINE_CHECKPOINT_STEP_MEDIUM = 256;
const PHYSICAL_LINE_CHECKPOINT_STEP_LARGE = 512;
const PHYSICAL_LINE_CHECKPOINT_STEP_HUGE = 1024;
const PHYSICAL_LINE_THRESHOLD_MEDIUM = 20000;
const PHYSICAL_LINE_THRESHOLD_LARGE = 100000;
const PHYSICAL_LINE_THRESHOLD_HUGE = 200000;
const TERMINAL_IMMEDIATE_WRITE_MAX_CHARS = 4096;
const terminalThemeSettings = ref(loadTerminalThemeSettings());
const CJK_MONO_FALLBACK_FONTS = '"Dusk Noto Sans SC", "Sarasa Mono SC", "Noto Sans Mono CJK SC", "Microsoft YaHei Mono", "SimSun", monospace';
const TERMINAL_DEFAULT_FONT = '"Consolas"';
const TERMINAL_FONT_FALLBACKS = '"Cascadia Mono", "Courier New", ' + CJK_MONO_FALLBACK_FONTS;
let metricsDirty = false;
let metricsRafId = null;
let lastLineMetrics = null;
let lastLineNumberRowsSignature = '';
let searchBufferVersion = 0;
let searchCountCacheSignature = '';
let searchCountCacheValue = 0;

const focusTerminalSurface = () => {
  if (!term) return;
  requestAnimationFrame(() => {
    term?.focus();
    requestAnimationFrame(() => {
      term?.focus();
    });
  });
};

let writeFlushRafId = null;
let pendingOutputChunks = [];
let viewportElement = null;
let viewportScrollHandler = null;
let termTitleDisposable = null;
let termDataDisposable = null;
let termResizeDisposable = null;
let termCursorMoveDisposable = null;
let termSelectionDisposable = null;
let termScrollDisposable = null;
let physicalLineCheckpoints = [{ index: -1, count: 0 }];
let physicalLineScannedUntil = -1;
let physicalLineTotal = 0;
// Cached last-non-empty scan: only re-scan when buffer grows
let _lastBufLen = -1;
let _cachedLastNonEmpty = -1;

// ── Trackpad gesture detection ──
let gestureDeltaX = 0;
let gestureTimerX = null;
let gestureCooldown = 0;
const GESTURE_WINDOW_MS = 350;
const GESTURE_COOLDOWN_MS = 1200;
const SWIPE_THRESHOLD_X = 100;

const resetGestureX = () => {
  clearTimeout(gestureTimerX);
  gestureTimerX = null;
  gestureDeltaX = 0;
};

const isGestureCooldown = () => {
  return Date.now() - gestureCooldown < GESTURE_COOLDOWN_MS;
};

const handleTerminalWheel = (e) => {
  const absX = Math.abs(e.deltaX);

  // Horizontal trackpad gestures switch sessions. Vertical wheel input remains terminal scroll only.
  if (absX > 2 && !isGestureCooldown()) {
    gestureDeltaX += e.deltaX;
    if (gestureTimerX) clearTimeout(gestureTimerX);
    gestureTimerX = setTimeout(resetGestureX, GESTURE_WINDOW_MS);

    if (gestureDeltaX > SWIPE_THRESHOLD_X) {
      window.dispatchEvent(new CustomEvent('terminal-gesture-next'));
      gestureCooldown = Date.now();
      resetGestureX();
      return;
    }
    if (gestureDeltaX < -SWIPE_THRESHOLD_X) {
      window.dispatchEvent(new CustomEvent('terminal-gesture-prev'));
      gestureCooldown = Date.now();
      resetGestureX();
      return;
    }
  }
};

let physicalLineCheckpointStep = PHYSICAL_LINE_CHECKPOINT_STEP;

const safeUnlisten = (unlisten) => {
  try {
    if (typeof unlisten === 'function') unlisten();
  } catch (error) {
    console.error('Terminal event cleanup failed:', error);
  }
};

// Sync line-number state from global Settings → Terminal pref
const onTerminalThemeChanged = () => {
  const settings = loadTerminalThemeSettings();
  lineNumbersEnabled.value = settings.showLineNumbers === true;
  scheduleLineMetrics();
};

const normalizeTerminalFontFamily = (configuredFontFamily) => {
  const value = String(configuredFontFamily || '').trim();
  if (!value) return TERMINAL_DEFAULT_FONT;
  if (value.includes(',') || value.startsWith('"') || value.startsWith("'")) return value;
  return /\s/.test(value) ? `"${value.replace(/"/g, '\\"')}"` : value;
};

const buildTerminalFontFamily = (configuredFontFamily) => {
  const primary = normalizeTerminalFontFamily(configuredFontFamily);
  return `${primary}, ${TERMINAL_FONT_FALLBACKS}`;
};

const refreshTerminalSurface = (fit = true, repaint = false) => {
  if (!term) return;
  const run = () => {
    if (!term) return;
    if (fit && fitAddon && terminalContainer.value?.clientWidth > 2 && terminalContainer.value?.clientHeight > 2) {
      try {
        doFit({ force: true });
      } catch (error) {
        console.error('Terminal font/layout refresh failed:', error);
      }
    }
    if (repaint && term.rows > 0) {
      term.refresh(0, term.rows - 1);
    }
    updateLineNumberRowHeight();
    scheduleLineMetrics();
  };

  requestAnimationFrame(run);
  setTimeout(run, 80);
  document.fonts?.ready?.then(run).catch(() => {});
};

const applyTerminalTextRendering = (config = {}) => {
  if (!term) return;
  const fontFamily = buildTerminalFontFamily(config.font_family);
  term.options.fontFamily = fontFamily;
  terminalWrapperRef.value?.style.setProperty('--terminal-font-family', fontFamily);
  refreshTerminalSurface(true, true);
};

const resolveCssColor = (value, fallback) => {
  if (typeof document === 'undefined') return fallback;
  const probe = document.createElement('span');
  probe.style.color = value;
  probe.style.position = 'absolute';
  probe.style.pointerEvents = 'none';
  probe.style.visibility = 'hidden';
  (terminalWrapperRef.value || document.body || document.documentElement).appendChild(probe);
  const resolved = getComputedStyle(probe).color;
  probe.remove();
  return resolved || fallback;
};

const applyTerminalTheme = () => {
  if (!term) return;
  const themeKey = terminalThemeSettings.value.theme || 'default';
  const baseTheme = getTerminalTheme(themeKey, isDark.value);
  const hasFloatingSurfaces = !!terminalWrapperRef.value?.closest('.has-floating-surfaces');
  const opaqueThemeBackground = resolveCssColor('var(--app-bg-dialog)', baseTheme.background || '#1e1e1e');
  const themeBackground = hasFloatingSurfaces ? 'rgba(0, 0, 0, 0)' : opaqueThemeBackground;
  const selectionBackground = resolveCssColor(
    baseTheme.selectionBackground || 'var(--app-selection-bg)',
    'rgba(192,132,47,0.28)'
  );
  const theme = {
    ...baseTheme,
    background: themeBackground,
    cursorAccent: baseTheme.cursorAccent || opaqueThemeBackground,
    selectionBackground,
    selectionInactiveBackground: baseTheme.selectionInactiveBackground || selectionBackground
  };
  term.options.theme = theme;

  const wrapper = terminalWrapperRef.value;
  if (wrapper) {
    wrapper.style.setProperty('--terminal-theme-bg', themeBackground);
    wrapper.style.setProperty(
      '--terminal-surface-bg',
      hasFloatingSurfaces
        ? 'color-mix(in srgb, var(--app-bg-dialog) 52%, transparent)'
        : opaqueThemeBackground
    );
    wrapper.style.setProperty('--terminal-theme-fg', theme.foreground || '#d4d4d4');
  }

  if (typeof term.refresh === 'function' && term.rows > 0) {
    term.refresh(0, term.rows - 1);
  }
};

const handleTerminalThemeChanged = () => {
  terminalThemeSettings.value = loadTerminalThemeSettings();
  applyTerminalTheme();
  // Sync line-number state from global pref
  onTerminalThemeChanged();
};

const handleMainUiChanged = () => requestAnimationFrame(applyTerminalTheme);

watch(isDark, () => {
  applyTerminalTheme();
});

const recordCommandHistory = (command) => {
  const latestHistory = readStoredCommandHistory();
  commandHistory.value = recordCommandHistoryEntry(latestHistory, command, {
    max: HISTORY_MAX,
    minLength: HISTORY_MIN_LEN,
  });
  persistCommandHistory();
};

const getCursorLogicalLineText = () => {
  const buffer = term?.buffer?.active;
  if (!buffer) return '';

  const cursorVisualIndex = Math.max(0, Number(buffer.baseY || 0) + Number(buffer.cursorY || 0));
  let startIndex = cursorVisualIndex;
  while (startIndex > 0 && buffer.getLine(startIndex)?.isWrapped) {
    startIndex -= 1;
  }

  const parts = [];
  for (let index = startIndex; index <= cursorVisualIndex; index += 1) {
    const line = buffer.getLine(index);
    if (!line) continue;
    const text = line.translateToString(false);
    if (index === cursorVisualIndex) {
      parts.push(text.slice(0, Math.max(0, Number(buffer.cursorX || 0))));
    } else {
      parts.push(text);
    }
  }

  return parts.join('').replace(/\u00a0/g, ' ').trimEnd();
};

const stripPromptFromCommandLine = (line, fallbackInput = '') => {
  return extractCommandFromTerminalLine(line, fallbackInput);
};

const getSubmittedCommandText = () => {
  const fallback = currentInputBuffer.value;
  const commandFromTerminal = stripPromptFromCommandLine(getCursorLogicalLineText(), fallback);
  if (commandFromTerminal) return commandFromTerminal;
  return String(fallback || '').trim();
};

const syncInputBufferFromTerminal = ({ refreshHints = true } = {}) => {
  const command = stripPromptFromCommandLine(getCursorLogicalLineText(), currentInputBuffer.value);
  if (!command) return '';
  if (command !== currentInputBuffer.value) {
    currentInputBuffer.value = command;
    if (refreshHints) scheduleQuickHintUpdate(command);
  }
  return command;
};

const cancelShellCompletionSync = () => {
  if (shellCompletionSyncTimer) {
    clearTimeout(shellCompletionSyncTimer);
    shellCompletionSyncTimer = null;
  }
  shellCompletionSyncPending = false;
};

const scheduleShellCompletionSync = () => {
  if (!shellCompletionSyncPending) return;
  if (shellCompletionSyncTimer) clearTimeout(shellCompletionSyncTimer);
  shellCompletionSyncTimer = setTimeout(() => {
    shellCompletionSyncTimer = null;
    if (!shellCompletionSyncPending) return;
    syncInputBufferFromTerminal();
    shellCompletionSyncPending = false;
  }, 80);
};

const closeQuickHint = () => {
  cancelQuickHintDebounce();
  quickHintSearchToken += 1;
  quickHintVisible.value = false;
  quickHintItems.value = [];
  quickHintSelectedIndex.value = 0;
  quickHintFocused.value = false;
  quickHintPanelStyle.value = {};
  quickHintLastQuery.value = '';
  quickHintLastMatchedIndexes.value = [];
};

const cancelQuickHintDebounce = () => {
  if (!quickHintDebounceTimer) return;
  clearTimeout(quickHintDebounceTimer);
  quickHintDebounceTimer = null;
};

const loadCommandKnowledgeCatalog = () => {
  commandKnowledgeStore.loadEntries().catch((error) => {
    console.error('Load command knowledge failed:', error);
  });
};

const cancelQuickHintPositionUpdate = () => {
  if (!quickHintPositionRafId) return;
  cancelAnimationFrame(quickHintPositionRafId);
  quickHintPositionRafId = null;
};

const ensureQuickHintItemVisible = () => {
  nextTick(() => {
    const panel = quickHintPanelRef.value;
    if (!panel) return;
    const current = panel.querySelector(`.quick-hint-item[data-index="${quickHintSelectedIndex.value}"]`);
    current?.scrollIntoView({ block: 'nearest' });
  });
};

const updateQuickHintPosition = () => {
  if (!quickHintVisible.value) return;
  const wrapper = terminalWrapperRef.value;
  const container = terminalContainer.value;
  if (!wrapper || !container || !term?.textarea) return;

  const panel = quickHintPanelRef.value;
  const wrapperRect = wrapper.getBoundingClientRect();
  const containerRect = container.getBoundingClientRect();
  const caretRect = term.textarea.getBoundingClientRect();

  const availableWidth = Math.max(240, containerRect.width - QUICK_HINT_PANEL_MARGIN_PX * 2);
  const targetWidth = Math.floor(containerRect.width * (1 / 3));
  const panelWidth = Math.max(260, Math.min(availableWidth, targetWidth));
  const panelHeight = Math.min(
    QUICK_HINT_PANEL_MAX_HEIGHT_PX,
    panel?.offsetHeight || QUICK_HINT_PANEL_MAX_HEIGHT_PX
  );

  let left = caretRect.left - wrapperRect.left;
  left = Math.max(
    containerRect.left - wrapperRect.left + QUICK_HINT_PANEL_MARGIN_PX,
    Math.min(
      left,
      containerRect.right - wrapperRect.left - panelWidth - QUICK_HINT_PANEL_MARGIN_PX
    )
  );

  let top = caretRect.bottom - wrapperRect.top + QUICK_HINT_PANEL_GAP_PX;
  const maxBottom = containerRect.bottom - wrapperRect.top - QUICK_HINT_PANEL_MARGIN_PX;
  if (top + panelHeight > maxBottom) {
    top = caretRect.top - wrapperRect.top - panelHeight - QUICK_HINT_PANEL_GAP_PX;
  }

  const minTop = containerRect.top - wrapperRect.top + QUICK_HINT_PANEL_MARGIN_PX;
  top = Math.max(minTop, top);

  quickHintPanelStyle.value = {
    left: `${Math.round(left)}px`,
    top: `${Math.round(top)}px`,
    width: `${Math.round(panelWidth)}px`,
    maxHeight: `${QUICK_HINT_PANEL_MAX_HEIGHT_PX}px`,
    minWidth: `${Math.min(QUICK_HINT_PANEL_MIN_WIDTH_PX, panelWidth)}px`
  };
};

const scheduleQuickHintPositionUpdate = () => {
  if (!quickHintVisible.value) return;
  if (quickHintPositionRafId) return;
  quickHintPositionRafId = requestAnimationFrame(() => {
    quickHintPositionRafId = null;
    updateQuickHintPosition();
  });
};

const moveQuickHintSelection = (offset) => {
  if (!quickHintVisible.value || quickHintItems.value.length === 0) return;
  const size = quickHintItems.value.length;
  const next = (quickHintSelectedIndex.value + offset + size) % size;
  quickHintSelectedIndex.value = next;
  ensureQuickHintItemVisible();
};

const areQuickHintItemsSame = (nextItems) => {
  const prev = quickHintItems.value || [];
  if (prev.length !== nextItems.length) return false;
  for (let index = 0; index < prev.length; index += 1) {
    const prevItem = prev[index] || {};
    const nextItem = nextItems[index] || {};
    if (
      String(prevItem.id || '') !== String(nextItem.id || '') ||
      String(prevItem.title || prevItem.name || '') !== String(nextItem.title || nextItem.name || '') ||
      String(prevItem.command || '') !== String(nextItem.command || '')
    ) {
      return false;
    }
  }
  return true;
};

const normalizeQuickHintQuery = (rawInput) => String(rawInput ?? '').trim().toLowerCase();

const resolveQuickHintDebounceMs = (query) => {
  if (query.length <= 2) return 60;
  return QUICK_HINT_DEBOUNCE_MS;
};

const collectQuickHintMatchesAsync = async (query, token) => {
  if (!commandKnowledgeStore.loaded) {
    await commandKnowledgeStore.loadEntries();
  }
  if (token !== quickHintSearchToken) return null;
  const knowledgeItems = commandKnowledgeStore.matchTriggers(query, QUICK_HINT_MAX_ITEMS)
    .map((entry) => ({
      ...entry,
      _source: 'knowledge',
      name: entry.title,
    }));

  const seenCmds = new Set(knowledgeItems.map(item => String(item.command || '')));
  const historyItems = findCommandHistoryMatches(commandHistory.value, query, {
    excludedCommands: seenCmds,
    limit: 10,
  });

  return { knowledgeItems, historyItems };
};

const updateQuickHintMatches = async (rawInput) => {
  const query = normalizeQuickHintQuery(rawInput);
  if (!query || query.length < 2) {
    closeQuickHint();
    return;
  }

  const token = ++quickHintSearchToken;
  const result = await collectQuickHintMatchesAsync(query, token);
  if (!result || token !== quickHintSearchToken) return;

  const { knowledgeItems, historyItems } = result;

  // Build items: knowledge trigger matches first, then history
  const histItems = historyItems.map((entry) => ({
    id: `hist-${entry.cmd}`,
    command: entry.cmd,
    _source: 'history'
  }));
  const nextItems = [...knowledgeItems, ...histItems];

  if (nextItems.length === 0) {
    closeQuickHint();
    return;
  }

  quickHintLastQuery.value = query;
  quickHintLastMatchedIndexes.value = knowledgeItems.map((item) => item.id);

  const sameItems = areQuickHintItemsSame(nextItems);
  if (!sameItems) {
    quickHintItems.value = nextItems;
    if (quickHintSelectedIndex.value >= nextItems.length) {
      quickHintSelectedIndex.value = 0;
    }
  }

  quickHintVisible.value = true;
  quickHintFocused.value = false;
  scheduleQuickHintPositionUpdate();
  if (!sameItems) {
    ensureQuickHintItemVisible();
  }
};

const scheduleQuickHintUpdate = (rawInput) => {
  const query = normalizeQuickHintQuery(rawInput);
  cancelQuickHintDebounce();
  quickHintDebounceTimer = setTimeout(() => {
    quickHintDebounceTimer = null;
    updateQuickHintMatches(rawInput).catch((error) => {
      console.error('Quick hint async match failed:', error);
    });
  }, resolveQuickHintDebounceMs(query));
};

const applyQuickHintSelection = () => {
  if (!quickHintVisible.value || quickHintItems.value.length === 0) return false;
  const selected = quickHintItems.value[quickHintSelectedIndex.value];
  const command = String(selected?.command || '').trim();
  if (!command) {
    closeQuickHint();
    return false;
  }

  const matched = matchSensitiveCommand(command, knowledgeSensitiveRules.value);
  if (matched) {
    openSecurityModal(matched, `${buildTerminalLineReplacementPayload(command)}\r`);
    closeQuickHint();
    return true;
  }

  forwardTerminalInput(buildTerminalLineReplacementPayload(command));
  currentInputBuffer.value = command;
  if (selected?._source === 'knowledge' && selected?.id) {
    commandKnowledgeStore.recordUsage(selected.id);
  }
  closeQuickHint();
  term?.focus();
  return true;
};

const handleQuickHintPointerDown = (event) => {
  if (!quickHintVisible.value) return;
  const panel = quickHintPanelRef.value;
  if (panel?.contains(event.target)) return;
  closeQuickHint();
};

const handleQuickHintItemClick = (index) => {
  quickHintSelectedIndex.value = index;
  applyQuickHintSelection();
};

// --- Menu Handlers ---
function handleZoomIn() {
  if (!term) return;
  term.options.fontSize = (term.options.fontSize || 14) + 2;
  doFit({ force: true });
}
function handleZoomOut() {
  if (!term) return;
  term.options.fontSize = Math.max(10, (term.options.fontSize || 14) - 2);
  doFit({ force: true });
}
function handleZoomReset() {
  if (!term) return;
  term.options.fontSize = 14;
  doFit({ force: true });
}
async function handleCopy() {
  if (!term) return;
  const selection = term.getSelection();
  if (selection) {
    try {
      await navigator.clipboard.writeText(selection);
      toast.success('已复制');
    } catch {
      const textarea = document.createElement('textarea');
      textarea.value = selection;
      textarea.setAttribute('readonly', 'true');
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      textarea.style.pointerEvents = 'none';
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      toast.success('已复制');
    }
  }
}
async function handlePaste() {
  if (!term) return;
  const text = await navigator.clipboard.readText();
  term.paste(text);
}
function handleSelectAll() {
  term?.selectAll();
}
function handleClear() {
  if (!term) return;
  term.write('\x1b[2J\x1b[H');
  markSearchBufferChanged();
  term.scrollToBottom();
  resetPhysicalLineCache();
  scheduleLineMetrics();
}

function clearScrollback() {
  if (!term) return;
  term.write('\x1b[3J\x1b[2J\x1b[H');
  term.clear();
  markSearchBufferChanged();
  term.scrollToBottom();
  resetPhysicalLineCache();
  scheduleLineMetrics();
  // Send empty newline to trigger shell to redraw the prompt
  sendData('\r');
}

function buildLogFilename(suffix = '', extension = 'log') {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  const timestamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  const rawName = sessionName.value || 'terminal';
  const safeName = rawName.replace(/[\\/:*?"<>|\s]+/g, '_');
  const suffixPart = suffix ? `_${suffix}` : '';
  return `${safeName}${suffixPart}_${timestamp}.${extension}`;
}

async function saveTerminalOutput() {
  if (!term) return;
  const path = await save({
    title: '保存终端输出',
    defaultPath: buildLogFilename()
  });
  if (!path) return;

  const buffer = term.buffer.active;
  const lines = [];
  for (let i = 0; i < buffer.length; i += 1) {
    const line = buffer.getLine(i);
    lines.push(line ? line.translateToString(true) : '');
  }
  const content = lines.join('\n');
  try {
    await invokeCommand('save_text_file', { path, content });
    toast.success('终端输出已保存');
  } catch (e) {
    toast.error(`保存失败: ${e}`);
  }
}

async function saveSerialReceiveData() {
  if (!isSerialSession.value) return;
  if (!serialRawReceiveBytes || serialRawReceiveChunks.value.length === 0) {
    toast.info('暂无串口接收数据');
    return;
  }
  const captureChunks = serialRawReceiveChunks.value.slice();
  const captureByteLength = captureChunks.reduce((total, chunk) => total + chunk.length, 0);
  const path = await save({
    title: '保存串口接收数据',
    defaultPath: buildLogFilename('recv-data', 'dat')
  });
  if (!path) return;
  try {
    const buffer = new Uint8Array(Math.min(SERIAL_SAVE_IPC_CHUNK_BYTES, captureByteLength));
    let bufferLength = 0;
    let isFirstChunk = true;

    const flushBuffer = async () => {
      if (bufferLength === 0) return;
      const command = isFirstChunk ? 'save_binary_file' : 'append_binary_file';
      const content = Array.from(buffer.subarray(0, bufferLength));
      await invokeCommand(command, { path, content });
      isFirstChunk = false;
      bufferLength = 0;
    };

    for (const sourceChunk of captureChunks) {
      let sourceOffset = 0;
      while (sourceOffset < sourceChunk.length) {
        const copyLength = Math.min(buffer.length - bufferLength, sourceChunk.length - sourceOffset);
        buffer.set(sourceChunk.subarray(sourceOffset, sourceOffset + copyLength), bufferLength);
        bufferLength += copyLength;
        sourceOffset += copyLength;
        if (bufferLength === buffer.length) {
          await flushBuffer();
        }
      }
    }
    await flushBuffer();
    toast.success('串口接收数据已保存');
  } catch (e) {
    toast.error(`保存失败: ${e}`);
  }
}

async function saveSerialIoLog() {
  if (!isSerialSession.value) return;
  const content = serialIoLogChunks.value.join('');
  if (!content) {
    toast.info('暂无串口收发日志');
    return;
  }
  const path = await save({
    title: '保存串口收发日志',
    defaultPath: buildLogFilename('io-log', 'log')
  });
  if (!path) return;
  try {
    await invokeCommand('save_text_file', { path, content });
    toast.success('串口收发日志已保存');
  } catch (e) {
    toast.error(`保存失败: ${e}`);
  }
}

// --- Search Implementation ---
const searchVisible = ref(false);
const searchText = ref('');
const searchOptions = ref({
  matchCase: false,
  regex: false,
  wholeWord: false,
  incremental: true // Search as you type
});
const searchInput = ref(null);
const searchInputFocused = ref(false);
const searchMatchCount = ref(0);
const searchCurrentMatch = ref(0);
let searchAutoRefreshTimer = null;
let searchAddon = null;

const searchDecorations = {
  matchBackground: 'rgba(59, 130, 246, 0.20)',
  activeMatchBackground: 'rgba(99, 102, 241, 0.30)',
  matchBorder: 'rgba(96, 165, 250, 0.45)',
  activeMatchBorder: 'rgba(129, 140, 248, 0.65)',
  matchOverviewRuler: 'rgba(99, 102, 241, 0.72)'
};

const hasValidSearchKeyword = () => String(searchText.value ?? '').trim().length > 0;

const resetSearchStats = () => {
  searchMatchCount.value = 0;
  searchCurrentMatch.value = 0;
};

const markSearchBufferChanged = () => {
  searchBufferVersion += 1;
  searchCountCacheSignature = '';
  scheduleSearchAutoRefresh();
};

const normalizeSearchSelection = (selection) => {
  const text = String(selection || '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .join(' ')
    .trim();
  if (!text) return '';
  return text.slice(0, SEARCH_SELECTION_MAX_LENGTH);
};

const getSearchTextFromSelection = () => {
  if (term?.hasSelection && !term.hasSelection()) return '';
  return normalizeSearchSelection(term?.getSelection?.());
};

const seedSearchTextFromSelection = () => {
  const selectedText = getSearchTextFromSelection();
  if (!selectedText) return false;
  if (searchText.value !== selectedText) {
    searchText.value = selectedText;
    resetSearchStats();
  }
  return true;
};

const buildSearchRegex = () => {
  if (!hasValidSearchKeyword()) return null;
  const source = searchOptions.value.regex
    ? searchText.value
    : searchText.value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const wrapped = searchOptions.value.wholeWord ? `\\b${source}\\b` : source;
  const flags = searchOptions.value.matchCase ? 'g' : 'gi';

  try {
    return new RegExp(wrapped, flags);
  } catch {
    return null;
  }
};

const countSearchMatches = () => {
  if (!term || !hasValidSearchKeyword()) return 0;
  const regex = buildSearchRegex();
  if (!regex) return 0;

  const buffer = term.buffer.active;
  const signature = [
    searchBufferVersion,
    buffer.length,
    buffer.baseY,
    searchText.value,
    searchOptions.value.matchCase ? 1 : 0,
    searchOptions.value.regex ? 1 : 0,
    searchOptions.value.wholeWord ? 1 : 0
  ].join('|');

  if (signature === searchCountCacheSignature) {
    return searchCountCacheValue;
  }

  let count = 0;

  for (let index = 0; index < buffer.length; index += 1) {
    const line = buffer.getLine(index)?.translateToString(true) || '';
    regex.lastIndex = 0;
    let match = regex.exec(line);
    while (match) {
      count += 1;
      if ((match[0] || '').length === 0) {
        regex.lastIndex += 1;
      }
      match = regex.exec(line);
    }
  }

  searchCountCacheSignature = signature;
  searchCountCacheValue = count;
  return count;
};

const updateSearchStats = ({ resetCurrent = false } = {}) => {
  searchMatchCount.value = countSearchMatches();
  if (resetCurrent) {
    searchCurrentMatch.value = searchMatchCount.value > 0 ? 1 : 0;
    return;
  }
  if (searchMatchCount.value === 0) {
    searchCurrentMatch.value = 0;
  } else if (searchCurrentMatch.value > searchMatchCount.value || searchCurrentMatch.value <= 0) {
    searchCurrentMatch.value = 1;
  }
};

const getSearchFindOptions = (incremental = false) => ({
  matchCase: searchOptions.value.matchCase,
  regex: searchOptions.value.regex,
  wholeWord: searchOptions.value.wholeWord,
  incremental,
  decorations: searchDecorations
});

const runAutoSearchRefresh = () => {
  if (!searchVisible.value || !searchAddon || !searchInputFocused.value) return;
  if (!hasValidSearchKeyword()) {
    searchAddon.clearDecorations();
    resetSearchStats();
    return;
  }
  performSearch();
};

const scheduleSearchAutoRefresh = () => {
  if (!searchVisible.value || !searchInputFocused.value) return;
  if (searchAutoRefreshTimer) {
    clearTimeout(searchAutoRefreshTimer);
  }
  searchAutoRefreshTimer = setTimeout(() => {
    searchAutoRefreshTimer = null;
    runAutoSearchRefresh();
  }, SEARCH_AUTO_REFRESH_DEBOUNCE_MS);
};

const cancelSearchAutoRefresh = () => {
  if (!searchAutoRefreshTimer) return;
  clearTimeout(searchAutoRefreshTimer);
  searchAutoRefreshTimer = null;
};

const updateLineNumberRowHeight = () => {
  if (!term || !viewportElement) return;
  const rows = Math.max(1, Number(term.rows || 0));
  const height = Number(viewportElement.clientHeight || 0);
  if (height > 0) {
    lineNumberRowHeightPx.value = Math.max(12, Math.floor(height / rows));
  }
};

const attachViewportScrollListener = () => {
  if (!term?.element) return;
  const nextViewport = term.element.querySelector('.xterm-viewport');
  if (nextViewport === viewportElement) return;

  if (viewportElement && viewportScrollHandler) {
    viewportElement.removeEventListener('scroll', viewportScrollHandler);
  }

  viewportElement = nextViewport;
  viewportScrollHandler = () => {
    syncGutterScrollTop();
    scheduleLineMetrics();
  };

  if (viewportElement) {
    viewportElement.addEventListener('scroll', viewportScrollHandler, { passive: true });
    if (lineNumberGutterRef.value) {
      lineNumberGutterRef.value.scrollTop = viewportElement.scrollTop;
    }
    updateLineNumberRowHeight();
  }
};

const detachViewportScrollListener = () => {
  if (viewportElement && viewportScrollHandler) {
    viewportElement.removeEventListener('scroll', viewportScrollHandler);
  }
  viewportElement = null;
  viewportScrollHandler = null;
};

const toggleLineNumbers = (nextValue) => {
  const settings = loadTerminalThemeSettings();
  const globalOn = settings.showLineNumbers === true;

  // Block toggle when global setting is off (must enable via Settings first)
  if (!globalOn) {
    toast.info('行号功能已全局关闭，请在 首选项 → 终端 → 行号显示 中开启');
    return;
  }

  const nextEnabled = typeof nextValue === 'boolean' ? nextValue : !lineNumbersEnabled.value;
  if (lineNumbersEnabled.value === nextEnabled) return;
  lineNumbersEnabled.value = nextEnabled;
  scheduleLineMetrics();
  toast.success(`行号显示已${nextEnabled ? '开启' : '关闭'}`);
};

const handleExternalLineNumberToggle = (event) => {
  const detail = event?.detail;
  if (typeof detail === 'boolean') {
    toggleLineNumbers(detail);
    return;
  }
  if (detail && typeof detail.enabled === 'boolean') {
    toggleLineNumbers(detail.enabled);
  }
};

const resetPhysicalLineCache = () => {
  physicalLineCheckpoints = [{ index: -1, count: 0 }];
  physicalLineScannedUntil = -1;
  physicalLineTotal = 0;
  physicalLineCheckpointStep = PHYSICAL_LINE_CHECKPOINT_STEP;
  _lastBufLen = -1;
  _cachedLastNonEmpty = -1;
};

const resolveCheckpointStep = (targetLength) => {
  if (targetLength >= PHYSICAL_LINE_THRESHOLD_HUGE) return PHYSICAL_LINE_CHECKPOINT_STEP_HUGE;
  if (targetLength >= PHYSICAL_LINE_THRESHOLD_LARGE) return PHYSICAL_LINE_CHECKPOINT_STEP_LARGE;
  if (targetLength >= PHYSICAL_LINE_THRESHOLD_MEDIUM) return PHYSICAL_LINE_CHECKPOINT_STEP_MEDIUM;
  return PHYSICAL_LINE_CHECKPOINT_STEP;
};

const rebuildPhysicalLineCache = (targetLength) => {
  const buffer = term?.buffer?.active;
  if (!buffer || targetLength <= 0) {
    resetPhysicalLineCache();
    return;
  }

  const step = resolveCheckpointStep(targetLength);
  physicalLineCheckpoints = [{ index: -1, count: 0 }];
  physicalLineCheckpointStep = step;
  let count = 0;

  for (let index = 0; index < targetLength; index += 1) {
    const wrapped = !!buffer.getLine(index)?.isWrapped;
    if (!wrapped) count += 1;
    if ((index + 1) % step === 0) {
      physicalLineCheckpoints.push({ index, count });
    }
  }

  physicalLineScannedUntil = targetLength - 1;
  physicalLineTotal = count;
};

const extendPhysicalLineCache = (targetLength) => {
  const buffer = term?.buffer?.active;
  if (!buffer || targetLength <= 0) {
    resetPhysicalLineCache();
    return;
  }

  if (physicalLineScannedUntil < 0) {
    rebuildPhysicalLineCache(targetLength);
    return;
  }

  const step = physicalLineCheckpointStep;

  let count = physicalLineTotal;
  for (let index = physicalLineScannedUntil + 1; index < targetLength; index += 1) {
    const wrapped = !!buffer.getLine(index)?.isWrapped;
    if (!wrapped) count += 1;
    if ((index + 1) % step === 0) {
      physicalLineCheckpoints.push({ index, count });
    }
  }

  physicalLineScannedUntil = targetLength - 1;
  physicalLineTotal = count;
};

const ensurePhysicalLineCache = (targetLength, forceRebuild = false) => {
  if (!term) return;

  const desiredStep = resolveCheckpointStep(targetLength);

  if (targetLength <= 0) {
    resetPhysicalLineCache();
    return;
  }

  if (
    forceRebuild ||
    physicalLineScannedUntil >= targetLength ||
    physicalLineScannedUntil < 0 ||
    desiredStep !== physicalLineCheckpointStep
  ) {
    rebuildPhysicalLineCache(targetLength);
    return;
  }

  if (physicalLineScannedUntil < targetLength - 1) {
    extendPhysicalLineCache(targetLength);
  }
};

const findCheckpointIndex = (targetVisualIndex) => {
  let left = 0;
  let right = physicalLineCheckpoints.length - 1;
  let answer = 0;

  while (left <= right) {
    const middle = (left + right) >> 1;
    const item = physicalLineCheckpoints[middle];
    if (item.index <= targetVisualIndex) {
      answer = middle;
      left = middle + 1;
    } else {
      right = middle - 1;
    }
  }

  return answer;
};

const getPhysicalLineAtVisualIndex = (visualIndex) => {
  if (!term || visualIndex < 0) return 0;

  ensurePhysicalLineCache(visualIndex + 1);
  const buffer = term.buffer.active;
  const checkpointIndex = findCheckpointIndex(visualIndex);
  const checkpoint = physicalLineCheckpoints[checkpointIndex] || { index: -1, count: 0 };
  let count = checkpoint.count;

  for (let index = checkpoint.index + 1; index <= visualIndex; index += 1) {
    const wrapped = !!buffer.getLine(index)?.isWrapped;
    if (!wrapped) count += 1;
  }

  return count;
};

const collectLineMetrics = () => {
  if (!term) return;
  const buffer = term.buffer.active;
  const length = Math.max(0, Number(buffer.length || 0));
  const viewportY = Math.max(0, Number(buffer.viewportY || 0));
  const rows = Math.max(1, Number(term.rows || 0));
  const selection = term.getSelectionPosition?.();
  const selectedLine = Number.isInteger(selection?.end?.y) ? selection.end.y : null;
  const fallbackVisualLine = Math.max(0, Number(buffer.baseY || 0) + Number(buffer.cursorY || 0));

  // Fast path: when line numbers are off, skip expensive buffer scan and physical-line cache
  const needsLineNumbers = lineNumbersEnabled.value;

  // Cached reverse scan — only rescan when buffer grew or was cleared.
  // Caps scan to last 500 lines to avoid O(n) on large buffers.
  let lastNonEmptyVisualLine = _cachedLastNonEmpty;
  if (length !== _lastBufLen || _cachedLastNonEmpty < 0) {
    _lastBufLen = length;
    lastNonEmptyVisualLine = -1;
    const scanLimit = Math.max(0, length - 500);
    for (let index = length - 1; index >= scanLimit; index -= 1) {
      const line = buffer.getLine(index);
      if (!line) continue;
      if (line.translateToString(true).length > 0) {
        lastNonEmptyVisualLine = index;
        break;
      }
    }
    if (lastNonEmptyVisualLine < 0 && length > 0) {
      for (let index = scanLimit - 1; index >= 0; index -= 1) {
        const line = buffer.getLine(index);
        if (!line) continue;
        if (line.translateToString(true).length > 0) {
          lastNonEmptyVisualLine = index;
          break;
        }
      }
    }
    _cachedLastNonEmpty = lastNonEmptyVisualLine;
  }

  const effectiveLastVisualLine = Math.max(lastNonEmptyVisualLine, fallbackVisualLine);
  const effectiveLength = Math.max(0, effectiveLastVisualLine + 1);
  const cursorVisualLine = Math.max(0, Math.min(effectiveLength > 0 ? effectiveLength - 1 : 0, selectedLine ?? fallbackVisualLine));

  // Visual-line count (each row gets a number, no isWrapped skip)
  const totalVisualLines = effectiveLength;
  const cursorVisualLineNum = cursorVisualLine + 1;

  const visibleRows = needsLineNumbers ? [] : null;
  const visibleStart = Math.max(0, viewportY);
  const visibleEnd = Math.min(effectiveLength, viewportY + rows);

  if (visibleRows) {
    let runningPhysical = getPhysicalLineAtVisualIndex(visibleStart - 1);
    for (let index = visibleStart; index < visibleEnd; index += 1) {
      runningPhysical += 1;
      visibleRows.push(String(runningPhysical));
    }
    while (visibleRows.length < rows) {
      visibleRows.push('');
    }
  }

  return {
    cursorLine: cursorVisualLineNum,
    totalLines: totalVisualLines,
    visibleRows,
    lineNumberDigits: String(Math.max(totalVisualLines, 1)).length
  };
};

const dispatchLineMetrics = (metrics) => {
  if (!metrics) return;
  if (
    lastLineMetrics &&
    lastLineMetrics.cursorLine === metrics.cursorLine &&
    lastLineMetrics.totalLines === metrics.totalLines
  ) {
    return;
  }
  lastLineMetrics = metrics;

  window.dispatchEvent(
    new CustomEvent('terminal-line-metrics', {
      detail: {
        sessionId: props.sessionId,
        cursorLine: metrics.cursorLine,
        totalLines: metrics.totalLines
      }
    })
  );
};

// ── Line-number gutter ──
const syncGutterScrollTop = () => {
  if (lineNumbersEnabled.value && lineNumberGutterRef.value && viewportElement) {
    lineNumberGutterRef.value.scrollTop = viewportElement.scrollTop;
  }
};

// RAF-gated line metrics: coalesce all triggers into at most one update per frame
const scheduleLineMetrics = () => {
  syncGutterScrollTop();
  if (metricsDirty) return;
  metricsDirty = true;
  if (metricsRafId) return;
  metricsRafId = requestAnimationFrame(() => {
    metricsRafId = null;
    metricsDirty = false;
    if (!term) return;
    const metrics = collectLineMetrics();
    dispatchLineMetrics(metrics);
    if (lineNumbersEnabled.value) {
      const nextRows = metrics?.visibleRows || [];
      const nextWidth = `${Math.max(3, Number(metrics?.lineNumberDigits || 1) + 1)}ch`;
      const nextSignature = `${nextWidth}|${nextRows.join('\n')}`;
      if (nextSignature !== lastLineNumberRowsSignature) {
        lineNumberRows.value = nextRows;
        lineNumberGutterWidth.value = nextWidth;
        lastLineNumberRowsSignature = nextSignature;
      }
      updateLineNumberRowHeight();
    } else {
      if (lineNumberRows.value.length > 0) {
        lineNumberRows.value = [];
      }
      lastLineNumberRowsSignature = '';
    }
  });
};

const flushTerminalOutput = () => {
  writeFlushRafId = null;
  if (!term || pendingOutputChunks.length === 0) return;
  const merged = pendingOutputChunks.join('');
  pendingOutputChunks = [];
  term.write(merged);
  markSearchBufferChanged();
  scheduleLineMetrics();
};

const enqueueTerminalOutput = (chunk) => {
  if (!chunk) return;
  if (!writeFlushRafId && pendingOutputChunks.length === 0 && chunk.length <= TERMINAL_IMMEDIATE_WRITE_MAX_CHARS) {
    term?.write(chunk);
    markSearchBufferChanged();
    scheduleLineMetrics();
    return;
  }
  pendingOutputChunks.push(chunk);
  if (writeFlushRafId) return;
  writeFlushRafId = requestAnimationFrame(flushTerminalOutput);
};

function focusSearchInputSoon({ selectText = false } = {}) {
  nextTick(() => {
    requestAnimationFrame(() => {
      searchInput.value?.focus();
      if (selectText) searchInput.value?.select?.();
    });
  });
}

function openSearch({ seedFromSelection = true } = {}) {
  const wasVisible = searchVisible.value;
  const seeded = seedFromSelection ? seedSearchTextFromSelection() : false;
  searchVisible.value = true;
  searchInputFocused.value = true;
  focusSearchInputSoon({ selectText: seeded });
  if (hasValidSearchKeyword()) performSearch();
  else resetSearchStats();
  if (!wasVisible) {
    setTimeout(() => handleResize(), 100);
  }
}

function openSearchFromMenu(event) {
  const targetSessionId = event?.detail?.sessionId;
  if (targetSessionId && targetSessionId !== props.sessionId) return;
  openSearch();
}

function closeSearch() {
  cancelSearchAutoRefresh();
  searchInputFocused.value = false;
  searchVisible.value = false;
  term?.focus();
  searchAddon?.clearDecorations();
  resetSearchStats();
  setTimeout(() => handleResize(), 100);
}

function handleSearchInputFocus() {
  searchInputFocused.value = true;
}

function handleSearchInputBlur() {
  searchInputFocused.value = false;
}

function handleSearchInput() {
  if (!hasValidSearchKeyword()) {
    searchAddon?.clearDecorations();
    resetSearchStats();
    return;
  }
  if (searchOptions.value.incremental) {
    performSearch();
  }
}

function toggleSearchOption(optionKey) {
  searchOptions.value[optionKey] = !searchOptions.value[optionKey];
  if (searchVisible.value && hasValidSearchKeyword()) {
    performSearch();
  }
}

function performSearch() {
  if (!searchAddon || !hasValidSearchKeyword() || !buildSearchRegex()) {
    searchAddon?.clearDecorations();
    resetSearchStats();
    return;
  }

  updateSearchStats({ resetCurrent: true });
  const found = searchAddon.findNext(searchText.value, getSearchFindOptions(searchOptions.value.incremental));
  searchCurrentMatch.value = found ? 1 : 0;
}

function findNext() {
  if (!searchAddon || !hasValidSearchKeyword() || !buildSearchRegex()) return;
  updateSearchStats();
  const found = searchAddon.findNext(searchText.value, getSearchFindOptions(false));
  if (found && searchMatchCount.value > 0) {
    searchCurrentMatch.value = searchCurrentMatch.value >= searchMatchCount.value ? 1 : searchCurrentMatch.value + 1;
  }
}

function findPrev() {
  if (!searchAddon || !hasValidSearchKeyword() || !buildSearchRegex()) return;
  updateSearchStats();
  const found = searchAddon.findPrevious(searchText.value, getSearchFindOptions(false));
  if (found && searchMatchCount.value > 0) {
    searchCurrentMatch.value = searchCurrentMatch.value <= 1 ? searchMatchCount.value : searchCurrentMatch.value - 1;
  }
}

function handleSearchKeydown(e) {
  if (e.isComposing) return;

  const key = String(e.key || '').toLowerCase();
  if (key === 'enter') {
    e.preventDefault();
    findNext();
    return;
  }

  if (e.key === 'Escape') {
    e.preventDefault();
    closeSearch();
    return;
  }

  if (e.shiftKey && key === 'j') {
    e.preventDefault();
    findPrev();
    return;
  }

  if (e.shiftKey && key === 'k') {
    e.preventDefault();
    findNext();
  }
}

function isSearchInputActive() {
  return !!searchInput.value && document.activeElement === searchInput.value;
}

function isTerminalFocused() {
  const activeElement = document.activeElement;
  return !!activeElement && !!terminalContainer.value?.contains(activeElement);
}

function handleTerminalCustomKeyEvent(event) {
  if (event.type !== 'keydown' || event.isComposing) return true;

  const key = String(event.key || '').toLowerCase();
  if (event.ctrlKey && event.shiftKey && key === 'c') {
    event.preventDefault();
    event.stopPropagation();
    void handleCopy();
    return false;
  }

  return true;
}

function handleKeydown(e) {
  const searchInputActive = isSearchInputActive();
  const terminalFocused = isTerminalFocused();
  const ownsKeyboardContext = searchInputActive || terminalFocused || contextMenuOpen.value;

  if (!ownsKeyboardContext) return;

  if (e.ctrlKey && e.altKey && (e.key === 'l' || e.key === 'L')) {
    e.preventDefault();
    toggleLineNumbers();
    return;
  }

  if (e.ctrlKey && e.shiftKey && (e.key === 'c' || e.key === 'C') && terminalFocused && !searchInputActive) {
    e.preventDefault();
    e.stopPropagation();
    void handleCopy();
    return;
  }

  if (searchVisible.value && searchInputActive && e.shiftKey && (e.key === 'j' || e.key === 'J')) {
    e.preventDefault();
    findPrev();
    return;
  }

  if (searchVisible.value && searchInputActive && e.shiftKey && (e.key === 'k' || e.key === 'K')) {
    e.preventDefault();
    findNext();
    return;
  }

  // Ctrl+Shift+F to toggle search
  if (e.ctrlKey && e.shiftKey && (e.key === 'F' || e.key === 'f')) {
    e.preventDefault();
    openSearch();
  }
  else if (e.key === 'Escape' && searchVisible.value) {
    e.preventDefault();
    closeSearch();
  }
}

function sendResizeIfNeeded(cols, rows, options = {}) {
  if (cols < 2 || rows < 2) return;
  const force = options.force === true;
  const session = sshStore.sessions.find(s => s.id === props.sessionId);
  if (session?.status !== 'connected') return;
  if (!force && cols === lastSentCols && rows === lastSentRows) return;
  lastSentCols = cols;
  lastSentRows = rows;
  const command = session.isSplitChild ? 'resize_ssh_shell_channel' : 'resize_ssh';
  const payload = session.isSplitChild
    ? { rootSessionId: session.workspaceSessionId || session.parentId, channelId: props.sessionId, cols, rows }
    : { sessionId: props.sessionId, cols, rows };
  invokeCommand(command, payload).catch(() => { });
}

let resizeTimeout = null;
function doFit(options = {}) {
  if (fitAddon && term?.element) {
    if (terminalContainer.value && terminalContainer.value.clientHeight > 2 && terminalContainer.value.clientWidth > 2) {
      try {
        const force = options.force === true;
        const now = performance.now();
        const width = terminalContainer.value.clientWidth;
        const height = terminalContainer.value.clientHeight;
        if (
          !force &&
          width === lastFittedContainerWidth &&
          height === lastFittedContainerHeight &&
          now - lastFitAt < 120
        ) {
          return;
        }

        const dims = fitAddon.proposeDimensions();
        if (!dims || dims.rows <= 1 || dims.cols <= 1) return;

        if (
          !force &&
          dims.cols === lastProposedCols &&
          dims.rows === lastProposedRows &&
          dims.cols === term.cols &&
          dims.rows === term.rows
        ) {
          lastFittedContainerWidth = width;
          lastFittedContainerHeight = height;
          lastFitAt = now;
          updateLineNumberRowHeight();
          scheduleQuickHintPositionUpdate();
          return;
        }

        lastFittedContainerWidth = width;
        lastFittedContainerHeight = height;
        lastFitAt = now;
        lastProposedCols = dims.cols;
        lastProposedRows = dims.rows;

        fitAddon.fit();
        resetPhysicalLineCache();

        sendResizeIfNeeded(dims.cols, dims.rows);
        scheduleLineMetrics();
        updateLineNumberRowHeight();
        scheduleQuickHintPositionUpdate();
      } catch (e) {
        console.error('Fit error:', e);
      }
    }
  }
}

function runDragFit() {
  lastDragFitAt = performance.now();
  doFit();
}

function scheduleDragFit() {
  if (dragFitRafId) return;

  dragFitRafId = requestAnimationFrame(() => {
    dragFitRafId = null;
    const now = performance.now();
    const elapsed = now - lastDragFitAt;

    if (elapsed >= DRAG_FIT_MIN_INTERVAL) {
      runDragFit();
      return;
    }

    const wait = DRAG_FIT_MIN_INTERVAL - elapsed;
    if (dragFitTimerId) clearTimeout(dragFitTimerId);
    dragFitTimerId = setTimeout(() => {
      dragFitTimerId = null;
      runDragFit();
    }, wait);
  });
}

function handleResize(immediate = false) {
  if (resizeTimeout) clearTimeout(resizeTimeout);
  if (!immediate && isLayoutDragging) {
    if (deferLayoutFit) {
      updateLineNumberRowHeight();
      scheduleQuickHintPositionUpdate();
      return;
    }
    scheduleDragFit();
    return;
  }
  if (immediate) {
    doFit({ force: true });
    return;
  }

  resizeTimeout = setTimeout(() => {
    doFit();
  }, 80);
}

function handleLayoutResize() {
  handleResize(true);
}

function handleLayoutDragging(event) {
  isLayoutDragging = !!event?.detail?.dragging;
  deferLayoutFit = isLayoutDragging && event?.detail?.deferFit !== false;
  if (!isLayoutDragging) {
    deferLayoutFit = false;
    if (dragFitRafId) {
      cancelAnimationFrame(dragFitRafId);
      dragFitRafId = null;
    }
    if (dragFitTimerId) {
      clearTimeout(dragFitTimerId);
      dragFitTimerId = null;
    }
  }
}

// --- Context Menu ---
const handleMenuSelect = async (key) => {
  switch (key) {
    case 'copy': await handleCopy(); break;
    case 'paste': await handlePaste(); break;
    case 'select-all': handleSelectAll(); break;
    case 'find': openSearch(); break;
    case 'clear': handleClear(); break;
    case 'clear-scrollback': clearScrollback(); break;
    case 'save-log': await saveTerminalOutput(); break;
    case 'serial-toggle-receive': toggleSerialReceiveVisible(); break;
    case 'serial-save-data': await saveSerialReceiveData(); break;
    case 'serial-save-log': await saveSerialIoLog(); break;
  }
  // Re-focus terminal after menu closes (except find which opens search bar)
  if (key !== 'find') {
    requestAnimationFrame(() => { term?.focus(); });
  }
};

function executeKnowledgeCommand(detail, command) {
  const text = String(command || '').trim();
  if (!text) return;
  const payload = text.endsWith('\r') || text.endsWith('\n') ? text : `${text}\r`;

  closeQuickHint();
  currentInputBuffer.value = '';
  recordCommandHistory(text);
  if (detail?.id) {
    commandKnowledgeStore.recordUsage(detail.id);
  }

  const matched = findMatchedCommandInPayload(payload, knowledgeSensitiveRules.value);
  if (matched) {
    openSecurityModal(matched, payload);
    return;
  }

  forwardTerminalInput(payload);
  term?.focus();
}

function handleKnowledgeCommandEvent(event) {
  const detail = event?.detail;
  if (detail?.sessionId && detail.sessionId !== props.sessionId) return;

  const command = typeof detail === 'string' ? detail : detail?.command;
  if (typeof command !== 'string' || command.length === 0) return;

  if (detail?.execute) {
    executeKnowledgeCommand(detail, command);
    return;
  }

  closeQuickHint();
  term?.paste(command);
  currentInputBuffer.value = command.trim();
  if (detail?.id) {
    commandKnowledgeStore.recordUsage(detail.id);
  }
  term?.focus();
}

onMounted(async () => {
  loadSyncInputState();
  loadCommandKnowledgeCatalog();
  loadCommandHistory();
  loadSerialReceivePreference();

  const cacheKey = props.sessionId;
  const cached = terminalCache.get(cacheKey);
  const session = sshStore.sessions.find(s => s.id === props.sessionId);
  const config = session?.config || {};

  if (cached) {
    term = cached.term;
    fitAddon = cached.fitAddon;
    searchAddon = cached.searchAddon;
    termTitleDisposable = cached.termTitleDisposable || null;
    termDataDisposable = cached.termDataDisposable || null;
    termResizeDisposable = cached.termResizeDisposable || null;
    termCursorMoveDisposable = cached.termCursorMoveDisposable || null;
    termSelectionDisposable = cached.termSelectionDisposable || null;
    termScrollDisposable = cached.termScrollDisposable || null;
    unlistenData = cached.unlistenData;
    unlistenDebug = cached.unlistenDebug;
    unlistenConnected = cached.unlistenConnected;
    unlistenClosed = cached.unlistenClosed;
    unlistenError = cached.unlistenError;
    unlistenTerminalTransferRequest = cached.unlistenTerminalTransferRequest;
    unlistenSerialDataSent = cached.unlistenSerialDataSent;
    textDecoder = cached.textDecoder || textDecoder;

    if (terminalContainer.value) {
      terminalContainer.value.innerHTML = '';
      if (term?.element) {
        terminalContainer.value.appendChild(term.element);
        refreshTerminalSurface(true);
      } else {
        term.open(terminalContainer.value);
      }
      setTimeout(() => {
        if (terminalContainer.value && terminalContainer.value.clientHeight > 10) {
          refreshTerminalSurface(true);
        }
      }, 50);
    }
    attachViewportScrollListener();
    applyTerminalTextRendering(config);
    applyTerminalTheme();
    term?.attachCustomKeyEventHandler?.(handleTerminalCustomKeyEvent);
    scheduleLineMetrics();
  }

  const isCached = !!cached;

  if (!isCached) {
    // 1. Get Session Config

    // 2. Configure Decoder
    if (config.encoding && config.encoding !== 'UTF-8') {
      try {
        textDecoder = new TextDecoder(config.encoding);
      } catch (e) {
        console.error('Invalid encoding:', config.encoding);
        toast.warn(`编码 '${config.encoding}' 不受支持，已使用 UTF-8`);
      }
    }

    // 3. Initialize Terminal with Config
    term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'bar',
      cursorWidth: 2,
      cursorInactiveStyle: 'outline',
      fontSize: config.font_size || 14,
      fontFamily: buildTerminalFontFamily(config.font_family),
      fontWeight: 'normal',
      fontWeightBold: 'bold',
      lineHeight: 1.0,
      letterSpacing: 0,
      customGlyphs: true,
      theme: getTerminalTheme(terminalThemeSettings.value.theme || 'default', isDark.value),
      allowProposedApi: true,
      scrollback: 50000,
      cols: 120,
      rows: 40,
      // iGPU optimizations: skip transparency blending, skip bold-bright conversion
      allowTransparency: true,
      drawBoldTextInBrightColors: false
    });

    fitAddon = new FitAddon();
    unicode11Addon = new Unicode11Addon();
    searchAddon = new SearchAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(unicode11Addon);
    term.unicode.activeVersion = '11';
    term.loadAddon(searchAddon);
    term.loadAddon(new WebLinksAddon());

    term.attachCustomKeyEventHandler(handleTerminalCustomKeyEvent);

    // Intercept Title Changes for Directory Tracking
    termTitleDisposable = term.onTitleChange((title) => {
      // Heuristic: Many shells set title to "user@host: /path" or just "/path"
      // We look for a pattern starting with / or ~
      // Also handle "root@host:~" where path is ~
      let path = '';
      if (title.startsWith('/')) {
        path = title;
      } else if (title.includes(':')) {
        // Try extracting after colon
        const parts = title.split(':');
        const last = parts[parts.length - 1].trim();
        // Check for common path indicators
        if (last.startsWith('/') || last.startsWith('~')) {
          path = last;
        } else if (last === 'root') {
          // Edge case: some configs just set title to user?
        }
      }

      // Fix: Normalize path via simple string ops if needed
      if (path && path.includes(' ')) {
        // Sometimes title includes other info? Assume path is first valid token?
        // Actually, paths can have spaces. Let's trust the title for now.
      }

      if (path) {
        sshStore.updateSessionCwd(props.sessionId, path);
      }
    });

      term.open(terminalContainer.value);
      applyTerminalTextRendering(config);
      focusTerminalSurface();
      applyTerminalTheme();
      attachViewportScrollListener();
      scheduleLineMetrics();
      refreshTerminalSurface(true);

    // Wait a tick for layout to settle before fitting
    setTimeout(() => {
      doFit({ force: true });
    }, 50);

    // Handle user input
    termDataDisposable = term.onData((data) => {
      try {
        const session = sshStore.sessions.find(s => s.id === props.sessionId);
        const isConnected = session?.status === 'connected';

        if (!isConnected) {
          if (session?.status === 'connecting' && session?.isSplitChild) {
            sendData(data);
            return;
          }
          const isEnter = data === '\r' || data === '\n';
          if (isEnter) {
            reconnectAfterDisconnect();
            return;
          }
          if (!reconnectPromptShown.value) {
            term.write('\r\n\x1b[33m当前会话已断开，按 Enter 键重连。\x1b[0m\r\n');
            markSearchBufferChanged();
            reconnectPromptShown.value = true;
          }
          scheduleLineMetrics();
          return;
        }

        if (shouldLockInputByPrimaryMode()) {
          notifyPrimaryLockIfNeeded();
          return;
        }

        if (securityModalVisible.value) {
          return;
        }

        if (quickHintVisible.value) {
          // Arrow Up
          if (data === '\x1b[A') {
            if (!quickHintFocused.value) {
              quickHintFocused.value = true;
              quickHintSelectedIndex.value = quickHintItems.value.length - 1;
              ensureQuickHintItemVisible();
            } else if (quickHintSelectedIndex.value === 0) {
              quickHintFocused.value = false;
            } else {
              moveQuickHintSelection(-1);
            }
            return;
          }
          // Arrow Down
          if (data === '\x1b[B') {
            if (!quickHintFocused.value) {
              quickHintFocused.value = true;
              quickHintSelectedIndex.value = 0;
              ensureQuickHintItemVisible();
            } else if (quickHintSelectedIndex.value >= quickHintItems.value.length - 1) {
              // At bottom: stay, don't wrap
            } else {
              moveQuickHintSelection(1);
            }
            return;
          }
          // Escape
          if (data === '\x1b') {
            closeQuickHint();
            return;
          }
          // Enter: apply only when hint is focused
          if (data === '\r' || data === '\n') {
            if (quickHintFocused.value && applyQuickHintSelection()) {
              return;
            }
            // Not focused: Enter falls through to normal terminal input
          }
          // Tab: apply when hint is focused
          if (data === '\t') {
            if (quickHintFocused.value && applyQuickHintSelection()) {
              return;
            }
          }
        }

        if (data === '\t') {
          shellCompletionSyncPending = true;
          forwardTerminalInput(data);
          return;
        }

        const isEnter = data === '\r' || data === '\n';
        const isPasteWithNewline = data.length > 1 && (data.includes('\r') || data.includes('\n'));
        const routedBySync = isCurrentSessionSyncSource();

        if (routedBySync && isPasteWithNewline) {
          currentInputBuffer.value = '';
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (routedBySync && isEnter) {
          syncInputBufferFromTerminal({ refreshHints: false });
          cancelShellCompletionSync();
          recordCommandHistory(getSubmittedCommandText());
          currentInputBuffer.value = '';
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (!routedBySync && isPasteWithNewline) {
          const matched = findMatchedCommandInPayload(data, knowledgeSensitiveRules.value);
          if (matched) {
            openSecurityModal(matched, data);
            return;
          }
          currentInputBuffer.value = '';
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (!routedBySync && isEnter) {
          syncInputBufferFromTerminal({ refreshHints: false });
          cancelShellCompletionSync();
          const submittedCommand = getSubmittedCommandText();
          const matched = matchSensitiveCommand(submittedCommand, knowledgeSensitiveRules.value);
          if (matched) {
            openSecurityModal(matched, data);
            return;
          }
          recordCommandHistory(submittedCommand);
          currentInputBuffer.value = '';
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (data === '\u007f' || data === '\b') {
          currentInputBuffer.value = currentInputBuffer.value.slice(0, -1);
          scheduleQuickHintUpdate(currentInputBuffer.value);
          forwardTerminalInput(data);
          return;
        }

        if (data === '\u0003') {
          cancelShellCompletionSync();
          currentInputBuffer.value = '';
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        const isControlSequence = data.startsWith('\x1b') || /^[\u0000-\u001F\u007F]$/.test(data);
        if (!isControlSequence) {
          currentInputBuffer.value += data;
          scheduleQuickHintUpdate(currentInputBuffer.value);
        } else if (data === '\x1b') {
          closeQuickHint();
        }

        forwardTerminalInput(data);
      } catch (error) {
        console.error('Security interceptor fallback:', error);
        closeQuickHint();
        currentInputBuffer.value = '';
        forwardTerminalInput(data);
      }
    });

    // Handle resize
    termResizeDisposable = term.onResize(({ cols, rows }) => {
      if (cols < 2 || rows < 2) return; // Ignore invalid sizes
      resetPhysicalLineCache();
      sendResizeIfNeeded(cols, rows);
      scheduleLineMetrics();
    });

    termCursorMoveDisposable = term.onCursorMove(() => {
      scheduleQuickHintPositionUpdate();
      scheduleShellCompletionSync();
    });

    termSelectionDisposable = term.onSelectionChange(() => {
      scheduleLineMetrics();
    });

    termScrollDisposable = term.onScroll(() => {
      // Immediate scroll sync for zero-lag gutter tracking
      syncGutterScrollTop();
      // Defer content rebuild to RAF
      scheduleLineMetrics();
      scheduleQuickHintPositionUpdate();
    });

    // Initial resize - delayed slightly to ensure container is ready
    setTimeout(() => {
      /* Handled by the forced resize above */
    }, 100);

    // Listen for backend data
    unlistenData = await listenEvent(`ssh-data-${props.sessionId}`, (payload) => {
      // console.log('Terminal Data:', payload);
      if (Array.isArray(payload)) {
        const rawBytes = new Uint8Array(payload);
        const decoded = textDecoder.decode(rawBytes);
        recordSerialReceive(decoded, payload.length, rawBytes);
        if (!isSerialSession.value || serialReceiveVisible.value) {
          enqueueTerminalOutput(decoded);
        }
      } else if (typeof payload === 'string') {
        const rawBytes = serialTextEncoder.encode(payload);
        recordSerialReceive(payload, rawBytes.length, rawBytes);
        if (!isSerialSession.value || serialReceiveVisible.value) {
          enqueueTerminalOutput(payload);
        }
      }
    });

    // Debug listener
    unlistenDebug = await listenEvent(`ssh-debug-${props.sessionId}`, (msg) => {
      console.log(`[SSH-DEBUG]`, msg);
    });

    // Force a resize + line-metrics refresh after short delay
    setTimeout(() => {
      if (fitAddon) {
        doFit({ force: true });
        const dims = fitAddon.proposeDimensions();
        if (dims && dims.rows && dims.rows > 1) {
          sendResizeIfNeeded(dims.cols, dims.rows);
        } else {
          term.resize(80, 24);
          sendResizeIfNeeded(80, 24);
        }
      }
      // Force initial line-number gutter render — no events fire on a fresh terminal
      scheduleLineMetrics();
    }, 200);

    unlistenConnected = await listenEvent(`ssh-connected-${props.sessionId}`, () => {
      sshStore.setSessionStatus(props.sessionId, 'connected');
      refreshTerminalSurface(true);
      focusTerminalSurface();
      setTimeout(() => {
        if (!fitAddon || !terminalContainer.value) return;
        try {
          doFit({ force: true });
          const dims = fitAddon.proposeDimensions();
          if (dims && dims.rows > 1 && dims.cols > 1) {
            sendResizeIfNeeded(dims.cols, dims.rows, { force: true });
          }
        } catch (error) {
          console.error('Terminal connected resize failed:', error);
        }
      }, 0);
    });

    unlistenClosed = await listenEvent(`ssh-closed-${props.sessionId}`, (reason) => {
      sshStore.setSessionStatus(props.sessionId, 'disconnected');
      reconnectPromptShown.value = false;
      term.write(`\r\n\x1b[31m${formatCloseReason(reason)}\x1b[0m\r\n`);
      term.write('\x1b[33m按 Enter 键尝试重连。\x1b[0m\r\n');
      markSearchBufferChanged();
      scheduleLineMetrics();
    });

    // Listen for errors
    unlistenError = await listenEvent(`ssh-error-${props.sessionId}`, (err) => {
      sshStore.setSessionStatus(props.sessionId, 'disconnected');
      reconnectPromptShown.value = false;
      term.write(`\r\n\x1b[31mError: ${err}\x1b[0m\r\n`);
      term.write('\x1b[33m按 Enter 键尝试重连。\x1b[0m\r\n');
      markSearchBufferChanged();
      toast.error(`会话错误：${err}`);
      scheduleLineMetrics();
    });

    unlistenTerminalTransferRequest = await listenEvent(
      `terminal-transfer-request-${props.sessionId}`,
      handleTerminalTransferRequest
    );

    unlistenSerialDataSent = await listenEvent(`serial-data-sent-${props.sessionId}`, (payload) => {
      if (Array.isArray(payload)) {
        recordSerialSend(textDecoder.decode(new Uint8Array(payload)), payload.length);
      } else if (typeof payload === 'string') {
        recordSerialSend(payload, serialTextEncoder.encode(payload).length);
      }
    });

    // Listen for Global Menu Global Events
    window.addEventListener('term:zoom-in', handleZoomIn);
    window.addEventListener('term:zoom-out', handleZoomOut);
    window.addEventListener('term:zoom-reset', handleZoomReset);
    window.addEventListener('term:copy', handleCopy);
    window.addEventListener('term:paste', handlePaste);
    window.addEventListener('term:select-all', handleSelectAll);

    // Focus this terminal when session becomes active
    terminalFocusHandler = (e) => {
      if (e?.detail?.sessionId === props.sessionId) {
        nextTick(() => {
          requestAnimationFrame(() => {
            term?.focus();
            // Double-tap: some browsers need a second focus after paint
            requestAnimationFrame(() => term?.focus());
          });
        });
      }
    };
    window.addEventListener('terminal:focus', terminalFocusHandler);
    window.addEventListener('term:clear', handleClear);
    window.addEventListener('term:find', openSearchFromMenu);

    window.addEventListener('command-knowledge-changed', loadCommandKnowledgeCatalog);
    loadCommandKnowledgeCatalog();

    terminalCache.set(cacheKey, {
      term,
      fitAddon,
      searchAddon,
      termTitleDisposable,
      termDataDisposable,
      termResizeDisposable,
      termCursorMoveDisposable,
      termSelectionDisposable,
      termScrollDisposable,
      unlistenData,
      unlistenDebug,
      unlistenConnected,
      unlistenClosed,
      unlistenError,
      unlistenTerminalTransferRequest,
      unlistenSerialDataSent,
      textDecoder
    });
  }

  window.addEventListener('keydown', handleKeydown);
  quickCommandHandler = handleKnowledgeCommandEvent;
  window.addEventListener('command-knowledge-insert', quickCommandHandler);
  window.addEventListener('terminal-theme-changed', handleTerminalThemeChanged);
  window.addEventListener('main-ui-settings-changed', handleMainUiChanged);
  window.addEventListener('terminal-layout-resize', handleLayoutResize);
  window.addEventListener('terminal-layout-dragging', handleLayoutDragging);
  window.addEventListener('terminal:toggle-line-numbers', handleExternalLineNumberToggle);
  window.addEventListener('mousedown', handleQuickHintPointerDown, true);
  window.addEventListener('sync-input-changed', onSyncInputChanged);
  window.addEventListener(HISTORY_CHANGED_EVENT, onCommandHistoryChanged);

  if (resizeObserver) resizeObserver.disconnect();
  resizeObserver = new ResizeObserver(() => handleResize());
  if (terminalContainer.value) {
    resizeObserver.observe(terminalContainer.value);
  }

  // Trackpad gesture detection on the terminal wrapper
  terminalWrapperRef.value?.addEventListener('wheel', handleTerminalWheel, { passive: true });

  window.addEventListener('resize', handleResize);
  window.dispatchEvent(
    new CustomEvent('terminal-ready', {
      detail: {
        sessionId: props.sessionId
      }
    })
  );
});

onUnmounted(() => {
  // Clean up listeners
  window.removeEventListener('term:zoom-in', handleZoomIn);
  window.removeEventListener('term:zoom-out', handleZoomOut);
  window.removeEventListener('term:zoom-reset', handleZoomReset);
  window.removeEventListener('term:copy', handleCopy);
  window.removeEventListener('term:paste', handlePaste);
  window.removeEventListener('term:select-all', handleSelectAll);
  window.removeEventListener('term:clear', handleClear);
  window.removeEventListener('term:find', openSearchFromMenu);
  if (quickCommandHandler) {
    window.removeEventListener('command-knowledge-insert', quickCommandHandler);
    quickCommandHandler = null;
  }
  if (terminalFocusHandler) {
    window.removeEventListener('terminal:focus', terminalFocusHandler);
    terminalFocusHandler = null;
  }
  window.removeEventListener('command-knowledge-changed', loadCommandKnowledgeCatalog);

  if (resizeObserver) resizeObserver.disconnect();
  if (dragFitRafId) {
    cancelAnimationFrame(dragFitRafId);
    dragFitRafId = null;
  }
  if (dragFitTimerId) {
    clearTimeout(dragFitTimerId);
    dragFitTimerId = null;
  }
  cancelSearchAutoRefresh();
  if (metricsRafId) {
    cancelAnimationFrame(metricsRafId);
    metricsRafId = null;
  }
  cancelQuickHintPositionUpdate();
  lastSentCols = 0;
  lastSentRows = 0;
  lastProposedCols = 0;
  lastProposedRows = 0;
  if (writeFlushRafId) {
    cancelAnimationFrame(writeFlushRafId);
    writeFlushRafId = null;
  }
  if (termTitleDisposable) {
    termTitleDisposable.dispose();
    termTitleDisposable = null;
  }
  if (termDataDisposable) {
    termDataDisposable.dispose();
    termDataDisposable = null;
  }
  if (termResizeDisposable) {
    termResizeDisposable.dispose();
    termResizeDisposable = null;
  }
  if (termCursorMoveDisposable) {
    termCursorMoveDisposable.dispose();
    termCursorMoveDisposable = null;
  }
  if (termSelectionDisposable) {
    termSelectionDisposable.dispose();
    termSelectionDisposable = null;
  }
  if (termScrollDisposable) {
    termScrollDisposable.dispose();
    termScrollDisposable = null;
  }
  pendingOutputChunks = [];
  lastLineMetrics = null;
  lastLineNumberRowsSignature = '';
  safeUnlisten(unlistenData);
  safeUnlisten(unlistenDebug);
  safeUnlisten(unlistenConnected);
  safeUnlisten(unlistenClosed);
  safeUnlisten(unlistenError);
  safeUnlisten(unlistenTerminalTransferRequest);
  safeUnlisten(unlistenSerialDataSent);
  unlistenData = null;
  unlistenDebug = null;
  unlistenConnected = null;
  unlistenClosed = null;
  unlistenError = null;
  unlistenTerminalTransferRequest = null;
  unlistenSerialDataSent = null;
  window.removeEventListener('resize', handleResize);
  // Always clear cache on unmount so next mount creates fresh bindings.
  // KeepAlive page switches use onDeactivated/onActivated, not onUnmounted.
  terminalCache.delete(props.sessionId);
  window.removeEventListener('keydown', handleKeydown);
  window.removeEventListener('terminal-theme-changed', handleTerminalThemeChanged);
  window.removeEventListener('main-ui-settings-changed', handleMainUiChanged);
  window.removeEventListener('terminal-layout-resize', handleLayoutResize);
  window.removeEventListener('terminal-layout-dragging', handleLayoutDragging);
  window.removeEventListener('terminal:toggle-line-numbers', handleExternalLineNumberToggle);
  window.removeEventListener('mousedown', handleQuickHintPointerDown, true);
  window.removeEventListener('sync-input-changed', onSyncInputChanged);
  window.removeEventListener(HISTORY_CHANGED_EVENT, onCommandHistoryChanged);
  detachViewportScrollListener();
  cancelQuickHintDebounce();
  cancelShellCompletionSync();
  closeQuickHint();
  resetGestureX();

  terminalWrapperRef.value?.removeEventListener('wheel', handleTerminalWheel);
  if (term) {
    term.dispose();
    term = null;
  }
  fitAddon = null;
  unicode11Addon = null;
  searchAddon = null;

  window.dispatchEvent(
    new CustomEvent('terminal-line-metrics', {
      detail: {
        sessionId: props.sessionId,
        cursorLine: 0,
        totalLines: 0
      }
    })
  );
});

</script>

<template>
  <div ref="terminalWrapperRef" class="terminal-wrapper">
    <div class="terminal-main">
        <div v-if="showLineNumberGutter" ref="lineNumberGutterRef" class="line-number-gutter"
          :style="{ width: lineNumberGutterWidth }">
        <div v-for="(lineNo, index) in lineNumberRows" :key="`line-no-${index}-${lineNo || 'wrap'}`"
          class="line-number-row"
          :style="{ height: `${lineNumberRowHeightPx}px`, lineHeight: `${lineNumberRowHeightPx}px` }">
          {{ lineNo }}
        </div>
      </div>
      <ContextMenu @update:open="(v) => contextMenuOpen = v">
        <ContextMenuTrigger class="terminal-container-wrap">
          <div ref="terminalContainer" class="terminal-container" @mousedown="focusTerminalSurface"></div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem @select="handleMenuSelect('copy')">复制</ContextMenuItem>
          <ContextMenuItem @select="handleMenuSelect('paste')">粘贴</ContextMenuItem>
          <ContextMenuItem @select="handleMenuSelect('select-all')">全选</ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem @select="handleMenuSelect('find')">查找</ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem @select="handleMenuSelect('clear')">清屏</ContextMenuItem>
          <ContextMenuItem @select="handleMenuSelect('clear-scrollback')">清空滚动缓冲区</ContextMenuItem>
          <ContextMenuItem @select="handleMenuSelect('save-log')">保存终端输出...</ContextMenuItem>
          <template v-if="isSerialSession">
            <ContextMenuSeparator />
            <ContextMenuItem @select="handleMenuSelect('serial-toggle-receive')">
              {{ serialReceiveVisible ? '隐藏接收数据' : '显示接收数据' }}
            </ContextMenuItem>
            <ContextMenuItem @select="handleMenuSelect('serial-save-data')">保存接收数据...</ContextMenuItem>
            <ContextMenuItem @select="handleMenuSelect('serial-save-log')">保存收发日志...</ContextMenuItem>
          </template>
        </ContextMenuContent>
      </ContextMenu>
    </div>

    <!-- Search Bar -->
    <div v-show="searchVisible" class="search-bar">
      <div class="search-input-wrapper">
        <Search class="search-icon" />
        <input ref="searchInput" v-model="searchText" type="text" placeholder="查找..."
          class="terminal-search-input" autocomplete="off" spellcheck="false" @mousedown.stop @click.stop
          @input="handleSearchInput" @focus="handleSearchInputFocus" @blur="handleSearchInputBlur"
          @keydown="handleSearchKeydown" />
      </div>
      <span class="search-count" :class="{ empty: searchMatchCount === 0 }">{{ searchCurrentMatch }}/{{ searchMatchCount
      }}</span>
      <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
        :class="{ active: searchOptions.matchCase }" aria-label="区分大小写"
        @click="toggleSearchOption('matchCase')">
        <CaseSensitive :size="15" stroke-width="1.9" />
      </button>
      <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
        :class="{ active: searchOptions.wholeWord }" aria-label="全词匹配"
        @click="toggleSearchOption('wholeWord')">
        <WholeWord :size="15" stroke-width="1.9" />
      </button>
      <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
        :class="{ active: searchOptions.regex }" aria-label="正则表达式" @click="toggleSearchOption('regex')">
        <Regex :size="15" stroke-width="1.9" />
      </button>
      <span class="terminal-find-divider"></span>
      <button type="button" class="terminal-find-button terminal-find-icon-button" aria-label="上一个"
        @click="findPrev">
        <ChevronUp :size="15" stroke-width="1.9" />
      </button>
      <button type="button" class="terminal-find-button terminal-find-icon-button" aria-label="下一个"
        @click="findNext">
        <ChevronDown :size="15" stroke-width="1.9" />
      </button>
      <button type="button" class="terminal-find-close terminal-find-icon-button" aria-label="关闭查找"
        @click="closeSearch">
        <X :size="15" stroke-width="1.9" />
      </button>
    </div>

    <Dialog :open="!!terminalTransferRequest" @update:open="(v) => { if (!v) dismissTerminalTransferUnsupported(); }">
      <DialogContent class="max-w-md">
        <DialogHeader>
          <DialogTitle>暂不支持 ZMODEM</DialogTitle>
        </DialogHeader>
        <div class="px-6 pb-4 text-sm terminal-transfer-dialog">
          <div class="terminal-transfer-kind">
            {{ terminalTransferRequest?.direction === 'sendToRemote' ? '检测到 rz 上传请求' : '检测到 sz 下载请求' }}
          </div>
          <p class="terminal-transfer-message">
            当前版本暂不支持 ZMODEM 文件传输。请使用 SFTP 文件面板上传或下载文件。
          </p>
        </div>
        <DialogFooter>
          <Button @click="dismissTerminalTransferUnsupported">知道了</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="securityModalVisible" @update:open="(v) => { if (!v) handleSecurityCancel(); }">
      <DialogContent class="max-w-lg">
        <DialogHeader>
          <DialogTitle>⚠️ 敏感命令二次确认</DialogTitle>
        </DialogHeader>
        <div class="px-6 pb-4 text-sm">
          <p>系统检测到您正在尝试执行以下高危命令：</p>
          <div
            style="background: hsl(var(--secondary)); padding: 12px; border-radius: 6px; font-family: var(--font-mono); color: var(--color-danger, #E45649); word-break: break-all; margin: 10px 0;">
            {{ blockedCommandContent }}
          </div>

          <!-- Critical Severity Handling -->
          <div v-if="blockedCommandSeverity === 'critical'">
            <div v-if="!securityStore.hasPassword"
              style="margin-top: 16px; padding: 12px; background: hsl(var(--destructive)/0.15); border-radius: 4px; border: 1px solid hsl(var(--destructive));">
              <p style="color: var(--app-text, #C8D2E1); margin-bottom: 8px;">⛔ 此命令已被标记为"严重"，必须验证应用密码才能执行。</p>
              <p style="color: var(--app-text-muted, #ABB2BF); margin-bottom: 8px; font-size: 12px">当前未设置应用密码。</p>
              <Button size="sm" @click="openSettings">前往设置密码</Button>
            </div>
            <div v-else style="margin-top: 16px;">
              <p style="margin-bottom: 8px">🔒 此操作需要验证应用密码：</p>
              <Input type="password" v-model="confirmPassword" placeholder="输入密码确认" size="sm" />
            </div>
          </div>

          <p v-if="!(blockedCommandSeverity === 'critical' && !securityStore.hasPassword)"
            style="margin-top: 16px; color: var(--app-text-muted, #ABB2BF);">
            当前会话: <span style="color: #fff; font-weight: bold;">{{ sessionName }}</span><br>
            如果您确认该操作无误，请点击<span style="color: #ff4d4f">红色按钮</span>继续。
          </p>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="handleSecurityCancel">取消</Button>
          <Button variant="destructive" @click="handleSecurityConfirm"
            :disabled="blockedCommandSeverity === 'critical' && !securityStore.hasPassword">
            确认执行
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <div v-if="quickHintVisible" ref="quickHintPanelRef" class="quick-hint-panel" :style="quickHintPanelStyle"
      role="listbox" aria-label="快捷指令建议" :aria-expanded="quickHintVisible">
      <div v-for="(item, index) in quickHintItems" :key="item.id || `${item.title || item.name}-${index}`" class="quick-hint-item"
        :class="{ active: quickHintFocused && index === quickHintSelectedIndex }" role="option"
        :aria-selected="quickHintFocused && index === quickHintSelectedIndex" :data-index="index" @mousedown.prevent
        @click="handleQuickHintItemClick(index)">
        <div v-if="item._source === 'history'" class="quick-hint-history-command">{{ item.command }}</div>
        <div v-else class="quick-hint-main">
          <div class="quick-hint-title">
            <span class="quick-hint-trigger">{{ item.trigger }}</span>
            {{ item.title || item.name || item.command }}
          </div>
          <div class="quick-hint-command">{{ item.command }}</div>
        </div>
      </div>
    </div>

  </div>
</template>

<style scoped>
.terminal-wrapper {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  box-sizing: border-box;
  background-color: var(--terminal-surface-bg, var(--app-bg-dialog));
  position: relative;
  overflow: hidden;
  border-radius: 0;
  --terminal-cursor-color: var(--terminal-theme-fg, #d4d4d4);
}

.terminal-main {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  background: transparent;
}

.line-number-gutter {
  flex: 0 0 auto;
  min-width: 3ch;
  background: transparent;
  color: var(--app-text-muted);
  user-select: none;
  pointer-events: none;
  overflow: hidden;
  text-align: right;
  padding-right: 6px;
  box-sizing: border-box;
  font-size: 11px;
  font-family: var(--terminal-font-family, var(--font-mono));
  contain: layout style paint;
}

.line-number-row {
  font-size: inherit;
  font-family: inherit;
  white-space: nowrap;
}

:deep(.terminal-container-wrap) {
  display: flex !important;
  flex: 1 !important;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.terminal-container {
  width: auto;
  flex: 1;
  min-width: 0;
  min-height: 0;
  box-sizing: border-box;
  padding: 0;
  overflow: hidden;
  display: flex;
  background: transparent;
  border-radius: 0;
}

.terminal-container :deep(.xterm) {
  flex: 1 1 auto;
  height: 100% !important;
  width: 100% !important;
  min-height: 0 !important;
  box-sizing: border-box !important;
  padding: 6px 8px;
  overflow: hidden !important;
  font-family: var(--terminal-font-family, var(--font-mono)) !important;
  font-variant-ligatures: none;
  font-feature-settings: "liga" 0, "calt" 0;
  font-kerning: none;
  letter-spacing: 0 !important;
  line-height: normal;
  background: transparent !important;
}

.terminal-container :deep(.xterm *) {
  box-sizing: content-box;
}

.terminal-container :deep(.xterm-rows) {
  font-family: inherit !important;
  font-variant-ligatures: none;
  font-feature-settings: "liga" 0, "calt" 0;
  font-kerning: none;
  letter-spacing: 0 !important;
  line-height: normal !important;
  white-space: pre !important;
}

.terminal-container :deep(.xterm-rows span) {
  font-family: inherit !important;
  line-height: normal !important;
  letter-spacing: inherit;
}

.terminal-container :deep(.xterm-viewport) {
  background-color: transparent !important;
}

.terminal-container :deep(.xterm-screen),
.terminal-container :deep(.xterm-screen canvas),
.terminal-container :deep(.xterm-helpers),
.terminal-container :deep(.xterm-rows) {
  background: transparent !important;
}

/* removed local scrollbar rule to allow global scrollbar styling */

.terminal-container :deep(.xterm-screen canvas) {
  display: block;
}

.terminal-container :deep(.xterm .xterm-cursor.xterm-cursor-bar) {
  box-shadow: inset 2px 0 0 var(--terminal-cursor-color);
}

.terminal-container :deep(.xterm.focus .xterm-cursor.xterm-cursor-blink) {
  animation: duskterm-xterm-cursor-blink 1s steps(1, end) infinite;
}

@keyframes duskterm-xterm-cursor-blink {
  0%,
  50% {
    opacity: 1;
  }

  50.01%,
  100% {
    opacity: 0;
  }
}

.search-bar {
  position: absolute;
  top: 8px;
  right: 8px;
  z-index: var(--z-floating);
  display: flex;
  align-items: center;
  padding: 4px 8px;
  background: var(--app-bg-dialog);
  border: 1px solid var(--app-border-shadow);
  border-radius: 6px;
  gap: 4px;
  max-width: calc(100% - 16px);
  pointer-events: auto;
  isolation: isolate;
}

.search-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 6px;
  color: var(--app-text-muted);
  pointer-events: none;
  font-size: 12px;
  z-index: 1;
}

.terminal-search-input {
  width: 260px;
  height: 28px;
  box-sizing: border-box;
  padding: 3px 8px 3px 28px;
  border: 1px solid var(--app-border-shadow);
  border-radius: var(--niri-radius-sm, 6px);
  background: transparent;
  color: var(--app-text);
  outline: none;
  font-size: 13px;
  pointer-events: auto;
}

.terminal-search-input:focus {
  border-color: var(--app-focus-border);
  box-shadow: var(--app-focus-shadow);
}

.terminal-search-input::placeholder {
  color: var(--app-text-muted);
}

.search-count {
  font-size: 12px;
  color: var(--app-text-muted);
  font-weight: 600;
  white-space: nowrap;
  min-width: 36px;
  text-align: center;
}

.search-count.empty {
  opacity: 0.5;
}

.terminal-find-button,
.terminal-find-close {
  flex: 0 0 auto;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: var(--niri-radius-sm, 5px);
  background: transparent;
  color: var(--app-text-muted);
  padding: 0;
  line-height: 1;
  cursor: pointer;
}

.terminal-find-button {
  padding: 0 9px;
}

.terminal-find-icon-button {
  width: 28px;
  padding: 0;
}

.terminal-find-icon-button svg {
  width: 15px;
  height: 15px;
  fill: none;
  stroke: currentColor;
}

.terminal-find-divider {
  flex: 0 0 auto;
  width: 1px;
  height: 20px;
  background: var(--app-border-shadow);
}

.option-button {
  color: color-mix(in srgb, var(--app-text-muted) 86%, transparent);
}

.terminal-find-button:hover,
.terminal-find-close:hover {
  border-color: var(--app-border-shadow);
  background: var(--app-btn-hover);
  color: var(--app-text);
}

.terminal-find-button.active {
  border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
  background: color-mix(in srgb, var(--color-primary) 14%, transparent);
  color: var(--app-text);
}

.terminal-transfer-dialog {
  color: var(--app-text);
}

.terminal-transfer-kind {
  display: inline-flex;
  align-items: center;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--app-border);
  border-radius: 6px;
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-text) 6%, transparent);
  font-weight: 600;
}

.terminal-transfer-message {
  margin-top: 10px;
  color: var(--app-text-muted);
  line-height: 1.6;
}

.quick-hint-panel {
  position: absolute;
  overflow-y: auto;
  background: hsl(var(--popover));
  border: 1px solid hsl(var(--border));
  border-radius: 6px;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.5);
  z-index: var(--z-popover);
}

.quick-hint-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 10px;
  cursor: pointer;
  border-bottom: 1px solid hsl(var(--border));
}

.quick-hint-item:last-child {
  border-bottom: none;
}

.quick-hint-item.active {
  background: hsl(var(--accent));
}

.quick-hint-main {
  flex: 1 1 auto;
  min-width: 0;
}

.quick-hint-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: hsl(var(--foreground));
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.quick-hint-trigger {
  flex: 0 0 auto;
  max-width: 92px;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--color-primary);
  font-family: var(--font-mono);
}

.quick-hint-command {
  margin-top: 2px;
  font-size: 11px;
  font-weight: 500;
  color: hsl(var(--muted-foreground));
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
}
.quick-hint-history-command {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: hsl(var(--foreground));
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 500;
}

/* Context menu styling provided by shadcn-vue ContextMenu component */
</style>
