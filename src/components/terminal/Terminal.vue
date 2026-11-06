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
import { TooltipHint } from '@/components/ui/tooltip';
import { toast } from '@/composables/useToast';
import { open, save } from '@tauri-apps/plugin-dialog';
import { FitAddon } from '@xterm/addon-fit';
import { describeLargeTerminalPaste } from '@/utils/terminalPaste';
import { buildTerminalContext, supportsTerminalTextContext } from '@/utils/aiContext';
import { captureTerminalSnapshot } from '@/utils/terminalContextSnapshot';
import { registerTerminalContextReader } from '@/utils/terminalContextReaders';
import { useAiStore } from '@/stores/ai';
import { TerminalSearch } from '@/utils/terminalSearch';
import { attachTerminalOutput, createTerminalOutput } from '@/utils/terminalOutput';
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
import { useCommandHistoryStore } from '@/stores/commandHistory';
import { useCommandKnowledgeStore } from '@/stores/commandKnowledge';
import { useSecurityStore } from '@/stores/security';
import { useSshStore } from '@/stores/ssh';
import { useTransfersStore } from '@/stores/transfers';
import { invokeCommand, listenEvent, isTauriRuntime } from '@/utils/ipc';
import { getPreferenceDefaults, loadPreference } from '@/utils/preferences';
import { findMatchedCommandInPayload, matchSensitiveCommand } from '@/utils/sensitiveCommand';
import { getSessionSyncBadgeState, SYNC_INPUT_CHANNELS_STORAGE_KEY } from '@/utils/syncInputChannels';
import {
  buildTerminalLineReplacementPayload,
  createTerminalInputState,
  extractAnchoredTerminalInput,
  replaceTerminalInputState,
  updateTerminalInputState,
} from '@/utils/terminalCommandHistory';
import { getTerminalTheme, loadTerminalThemeSettings } from '@/utils/terminalTheme';

const props = defineProps({
  sessionId: {
    type: String,
    required: true
  },
  active: {
    type: Boolean,
    default: true
  }
});

const terminalWrapperRef = ref(null);
const terminalContainer = ref(null);
const contextMenuOpen = ref(false);
const hasAiSelection = ref(false);
const aiImporting = ref(false);
let aiCaptureController;
let unregisterContextReader;
let aiMenuSelected = false;
function handleContextMenuCloseFocus(event) {
  if (aiMenuSelected) { event.preventDefault(); aiMenuSelected = false; }
}
const reconnectingAfterDisconnect = ref(false);
const reconnectPromptShown = ref(false);
const sshStore = useSshStore();
const transferStore = useTransfersStore();
const commandHistoryStore = useCommandHistoryStore();
const commandKnowledgeStore = useCommandKnowledgeStore();
const securityStore = useSecurityStore();
const { isDark } = useTheme();
const QUICK_HINT_DEBOUNCE_MS = 90;
const QUICK_HINT_MAX_ITEMS = 24;
const QUICK_HINT_PANEL_MAX_HEIGHT_PX = 200;
const QUICK_HINT_PANEL_MIN_WIDTH_PX = 320;
const QUICK_HINT_PANEL_MARGIN_PX = 10;
const QUICK_HINT_PANEL_GAP_PX = 8;
const defaultKeybindings = getPreferenceDefaults('keybindings');
const quickHintActivateBinding = ref(defaultKeybindings.selectTerminalSuggestion || 'Alt+ArrowDown');
const quickHintActivateLabel = computed(() => String(quickHintActivateBinding.value || '')
  .replace(/ArrowDown/gi, '↓')
  .replace(/ArrowUp/gi, '↑')
  .replace(/ArrowLeft/gi, '←')
  .replace(/ArrowRight/gi, '→'));

const refreshQuickHintActivateBinding = () => {
  try {
    const keybindings = loadPreference('keybindings');
    quickHintActivateBinding.value = String(
      keybindings.selectTerminalSuggestion ?? defaultKeybindings.selectTerminalSuggestion ?? ''
    ).trim();
  } catch (error) {
    quickHintActivateBinding.value = defaultKeybindings.selectTerminalSuggestion || 'Alt+ArrowDown';
  }
};
refreshQuickHintActivateBinding();
// --- Security Interceptor ---
const securityModalVisible = ref(false);
const blockedCommandContent = ref('');
const blockedCommandSeverity = ref('warning');
const pendingData = ref(null);
let pendingHistorySnapshot = null;
const confirmPassword = ref('');
const currentInputState = ref(createTerminalInputState());
const currentInputBuffer = computed({
  get: () => currentInputState.value.text,
  set: (value) => {
    currentInputState.value = replaceTerminalInputState(value);
  },
});
let inputLinePrefix = '';
const quickHintVisible = ref(false);
const quickHintItems = ref([]);
const quickHintSelectedIndex = ref(0);
const quickHintFocused = ref(false);
const quickHintPanelRef = ref(null);
const quickHintPanelStyle = ref({});
let quickHintDismissedInput = '';
let quickHintHistoryNavigation = false;
let pendingKnowledgeUsageId = '';
const SERIAL_RECEIVE_VISIBLE_KEY_PREFIX = 'serial-receive-visible-v1:';
const SERIAL_CAPTURE_MAX_CHARS = 5 * 1024 * 1024;
const SERIAL_SAVE_IPC_CHUNK_BYTES = 128 * 1024;
const knowledgeSensitiveRules = computed(() => commandKnowledgeStore.sensitiveRules || []);

const resetCurrentInputState = () => {
  currentInputState.value = createTerminalInputState();
  inputLinePrefix = '';
  quickHintHistoryNavigation = false;
};

const captureInputLinePrefix = () => {
  if (currentInputState.value.text) return;
  inputLinePrefix = getCursorLogicalLineText();
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
const openSyncSettings = () => window.dispatchEvent(new CustomEvent('app:open-sync-input'));
const pauseSyncChannel = () => window.dispatchEvent(new CustomEvent('app:pause-sync-channel', {
  detail: { channelId: syncBadgeState.value.channelId },
}));
let terminalWriteErrorToastAt = 0;
const serialReceiveVisible = ref(true);
const serialRawReceiveChunks = ref([]);
const serialIoLogChunks = ref([]);
const serialPanelVisible = ref(true);
const serialDisplayMode = ref('ascii');
const serialSendMode = ref('text');
const serialSendText = ref('');
const serialSendLineEnding = ref('none');
const serialPeriodicInterval = ref(1000);
const serialPeriodicSending = ref(false);
const serialDtrEnabled = ref(false);
const serialRtsEnabled = ref(false);
const serialBreakEnabled = ref(false);
const createSerialStatus = () => ({
  rxBytes: 0,
  txBytes: 0,
  rxRate: 0,
  txRate: 0,
  cts: null,
  dsr: null,
  ri: null,
  dcd: null,
  capturing: false,
  sendingFile: false
});
const serialStatus = ref(createSerialStatus());
let serialRawReceiveBytes = 0;
let serialIoLogChars = 0;
let serialPendingReceiveLogBytes = 0;
let serialRawCaptureTruncated = false;
let serialIoLogTruncated = false;
let serialHexColumn = 0;
let serialReceivePendingCr = false;
let serialReceivePendingCrTimer = null;
let serialPeriodicTimer = null;
let serialPeriodicWritePending = false;
let serialAutoReconnectTimer = null;
let serialAutoReconnectAttempt = 0;
const serialTextEncoder = new TextEncoder();

const currentSession = computed(() => sshStore.sessions.find(s => s.id === props.sessionId) || null);
const sessionName = computed(() => currentSession.value?.name || 'Unknown');
const isSerialSession = computed(() => String(currentSession.value?.config?.protocol || '').toLowerCase() === 'serial');
const isLocalSession = computed(() => String(currentSession.value?.config?.protocol || '').toLowerCase() === 'local');
const canShareContext = computed(() => !!currentSession.value && supportsTerminalTextContext(currentSession.value.config?.protocol));
function updateContextMenu(open) {
  contextMenuOpen.value = open;
  hasAiSelection.value = !!term?.hasSelection();
}
function terminalContextSource() {
  const source = currentSession.value;
  return { id: props.sessionId, name: source?.name, protocol: source?.config?.protocol,
    shell: source?.config?.local_shell_name || source?.config?.local_profile };
}
async function importTerminalContext(prepare) {
  if (!canShareContext.value || aiImporting.value || securityStore.isLocked) return;
  const origin = document.activeElement;
  const source = terminalContextSource();
  const targetTerm = term;
  const controller = new AbortController();
  aiCaptureController = controller;
  aiImporting.value = true;
  try {
    const ai = useAiStore();
    window.dispatchEvent(new CustomEvent('app:open-tool-ai'));
    await ai.importTerminalText(async () => {
      // Yield once before a bounded atomic read so Cancel/navigation can run.
      await new Promise(resolve => setTimeout(resolve, 0));
      if (controller.signal.aborted || targetTerm !== term) throw new DOMException('带入已取消', 'AbortError');
      return prepare(targetTerm, source);
    }, origin, controller.signal);
    toast.success('已带入输入框，可编辑后发送');
  } catch (error) { if (error && error.name !== 'AbortError') toast.error(String(error.message || error)); }
  finally { aiImporting.value = false; aiCaptureController = null; }
}
function bringSelectionToAi() {
  try {
    const context = buildTerminalContext(term?.getSelection(), terminalContextSource());
    return importTerminalContext(() => context);
  } catch (error) { toast.error(String(error.message || error)); }
}
function bringOutputToAi(mode, lines = 200) {
  return importTerminalContext((terminal, source) => {
    const snapshot = captureTerminalSnapshot(terminal, { mode, lines });
    return buildTerminalContext(snapshot.text, source, snapshot.capturedAt, snapshot.range);
  });
}
watch(() => [props.active, props.sessionId, currentSession.value?.status, currentSession.value?.config?.protocol, securityStore.isLocked], () => {
  aiCaptureController?.abort();
});
const serialLocalEchoEnabled = computed(() => currentSession.value?.config?.serial_local_echo === true);
const serialReceiveLineEnding = computed(() => String(
  currentSession.value?.config?.serial_receive_line_ending || 'none'
).toLowerCase());
const terminalTransferOwned = computed(() => transferStore.isTerminalOwned(props.sessionId));

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
  if (!text) return false;
  if (text.length >= SERIAL_CAPTURE_MAX_CHARS) {
    chunksRef.value = [text.slice(-SERIAL_CAPTURE_MAX_CHARS)];
    sizeSetter(SERIAL_CAPTURE_MAX_CHARS);
    return true;
  }
  let truncated = false;
  chunksRef.value.push(text);
  sizeSetter(sizeGetter() + text.length);
  while (sizeGetter() > SERIAL_CAPTURE_MAX_CHARS && chunksRef.value.length > 1) {
    const removed = chunksRef.value.shift() || '';
    sizeSetter(Math.max(0, sizeGetter() - removed.length));
    truncated = true;
  }
  return truncated;
};

const appendLimitedSerialBytes = (chunksRef, bytes) => {
  if (!bytes?.length) return;
  const chunk = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  chunksRef.value.push(chunk);
  serialRawReceiveBytes += chunk.length;
  while (serialRawReceiveBytes > SERIAL_CAPTURE_MAX_CHARS && chunksRef.value.length > 1) {
    const removed = chunksRef.value.shift();
    serialRawReceiveBytes = Math.max(0, serialRawReceiveBytes - (removed?.length || 0));
    serialRawCaptureTruncated = true;
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
  const truncated = appendLimitedSerialText(
    serialIoLogChunks,
    entry,
    () => serialIoLogChars,
    (value) => { serialIoLogChars = value; }
  );
  if (truncated) serialIoLogTruncated = true;
};

const recordSerialReceive = (text, byteLength, rawBytes = null) => {
  if (!isSerialSession.value) return;
  const bytes = rawBytes || (text ? serialTextEncoder.encode(text) : null);
  if (bytes?.length) appendSerialReceiveRaw(bytes);
  serialPendingReceiveLogBytes += Math.max(0, Number(byteLength) || 0);
  if (text) {
    appendSerialIoLog('RECV', text, serialPendingReceiveLogBytes);
    serialPendingReceiveLogBytes = 0;
  }
};

const serialBytesToHex = (bytes) => Array.from(bytes || [])
  .map((byte) => Number(byte).toString(16).padStart(2, '0').toUpperCase())
  .join(' ');

const recordSerialSend = (text, byteLength = serialTextEncoder.encode(text).length, rawBytes = null) => {
  if (!isSerialSession.value || (!text && !rawBytes?.length)) return;
  appendSerialIoLog('SEND', text || `[HEX] ${serialBytesToHex(rawBytes)}`, byteLength);
};

const formatSerialHex = (bytes) => {
  let output = '';
  for (const byte of bytes || []) {
    if (serialHexColumn > 0) output += ' ';
    output += Number(byte).toString(16).padStart(2, '0').toUpperCase();
    serialHexColumn += 1;
    if (serialHexColumn >= 16) {
      output += '\r\n';
      serialHexColumn = 0;
    }
  }
  return output;
};

const clearSerialPendingCr = () => {
  if (serialReceivePendingCrTimer) clearTimeout(serialReceivePendingCrTimer);
  serialReceivePendingCrTimer = null;
  serialReceivePendingCr = false;
};

const flushSerialPendingCr = () => {
  if (!serialReceivePendingCr) return;
  clearSerialPendingCr();
  enqueueTerminalOutput('\r\n');
};

const normalizeSerialReceiveText = (text) => {
  const mode = serialReceiveLineEnding.value;
  if (mode === 'none') return text;
  if (mode === 'cr') return text.replace(/\r/g, '\r\n');
  if (mode === 'lf') return text.replace(/\n/g, '\r\n');
  if (mode !== 'auto') return text;

  if (serialReceivePendingCrTimer) clearTimeout(serialReceivePendingCrTimer);
  serialReceivePendingCrTimer = null;
  let value = `${serialReceivePendingCr ? '\r' : ''}${text}`;
  serialReceivePendingCr = value.endsWith('\r');
  if (serialReceivePendingCr) {
    value = value.slice(0, -1);
    serialReceivePendingCrTimer = setTimeout(() => {
      serialReceivePendingCrTimer = null;
      if (!serialReceivePendingCr) return;
      serialReceivePendingCr = false;
      enqueueTerminalOutput('\r\n');
    }, 30);
  }
  return value.replace(/\r\n|\r|\n/g, '\r\n');
};

const renderSerialReceive = (bytes, decoded) => (
  serialDisplayMode.value === 'hex'
    ? formatSerialHex(bytes)
    : normalizeSerialReceiveText(decoded)
);

const renderSerialLocalEcho = (bytes, decoded) => {
  if (!serialLocalEchoEnabled.value) return;
  const output = serialDisplayMode.value === 'hex'
    ? formatSerialHex(bytes)
    : decoded.replace(/\r\n|\r|\n/g, '\r\n');
  enqueueTerminalOutput(output);
};

const parseSerialHexInput = (value) => {
  const withoutPrefixes = String(value || '').replace(/0x/gi, '');
  const compact = withoutPrefixes.replace(/[\s,;:_-]+/g, '');
  if (!compact || /[^0-9a-f]/i.test(compact) || compact.length % 2 !== 0) {
    throw new Error('HEX 数据必须由完整字节组成，例如：01 03 00 00 00 02');
  }
  const bytes = [];
  for (let index = 0; index < compact.length; index += 2) {
    bytes.push(Number.parseInt(compact.slice(index, index + 2), 16));
  }
  return bytes;
};

const formatSerialByteCount = (value) => {
  const bytes = Math.max(0, Number(value) || 0);
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
};

const sendSerialPanelData = async () => {
  if (!isSerialSession.value) return;
  if (!serialSendText.value) throw new Error('请输入要发送的数据');
  if (serialStatus.value.sendingFile) throw new Error('串口文件发送期间不能插入其他数据');
  if (serialSendMode.value === 'hex') {
    const bytes = parseSerialHexInput(serialSendText.value);
    await invokeCommand('serial_write_bytes', { sessionId: props.sessionId, data: bytes });
    return;
  }
  await invokeCommand('serial_write_text', {
    sessionId: props.sessionId,
    text: serialSendText.value,
    encoding: currentSession.value?.config?.encoding || 'UTF-8',
    lineEnding: serialSendLineEnding.value
  });
};

const handleSerialSendClick = async () => {
  try {
    await sendSerialPanelData();
  } catch (error) {
    toast.error(`发送失败：${error}`);
  }
};

watch(serialDisplayMode, () => {
  serialHexColumn = 0;
  clearSerialPendingCr();
});

const stopSerialPeriodicSend = () => {
  if (serialPeriodicTimer) clearInterval(serialPeriodicTimer);
  serialPeriodicTimer = null;
  serialPeriodicSending.value = false;
};

const toggleSerialPeriodicSend = async () => {
  if (serialPeriodicSending.value) {
    stopSerialPeriodicSend();
    return;
  }
  const interval = Math.max(20, Number(serialPeriodicInterval.value) || 1000);
  serialPeriodicInterval.value = interval;
  try {
    await sendSerialPanelData();
    serialPeriodicSending.value = true;
    serialPeriodicTimer = setInterval(() => {
      if (serialPeriodicWritePending) return;
      serialPeriodicWritePending = true;
      sendSerialPanelData()
        .catch((error) => {
          stopSerialPeriodicSend();
          toast.error(`周期发送已停止：${error}`);
        })
        .finally(() => { serialPeriodicWritePending = false; });
    }, interval);
  } catch (error) {
    toast.error(`发送失败：${error}`);
  }
};

const sendSerialFile = async () => {
  if (serialStatus.value.sendingFile) {
    toast.info('已有串口文件正在发送');
    return;
  }
  const selected = await open({ multiple: false, directory: false });
  const path = selected?.path || selected;
  if (!path) return;
  try {
    await invokeCommand('serial_send_file', { sessionId: props.sessionId, path });
    serialStatus.value = { ...serialStatus.value, sendingFile: true };
    stopSerialPeriodicSend();
    toast.success('串口文件已开始发送');
  } catch (error) {
    toast.error(`文件发送失败：${error}`);
  }
};

const setSerialControlLine = async (line) => {
  const stateRef = line === 'dtr'
    ? serialDtrEnabled
    : line === 'rts'
      ? serialRtsEnabled
      : serialBreakEnabled;
  const next = !stateRef.value;
  try {
    await invokeCommand('serial_set_control_line', {
      sessionId: props.sessionId,
      line,
      enabled: next
    });
    stateRef.value = next;
  } catch (error) {
    toast.error(`${line.toUpperCase()} 设置失败：${error}`);
  }
};

const clearSerialBuffer = async (target = 'all') => {
  try {
    await invokeCommand('serial_clear_buffer', { sessionId: props.sessionId, target });
    toast.success('串口缓冲区已清理');
  } catch (error) {
    toast.error(`清理串口缓冲区失败：${error}`);
  }
};

const toggleSerialCapture = async () => {
  try {
    if (serialStatus.value.capturing) {
      await invokeCommand('serial_stop_capture', { sessionId: props.sessionId });
      serialStatus.value = { ...serialStatus.value, capturing: false };
      toast.success('串口直接抓取已停止');
      return;
    }
    const path = await save({
      title: '选择串口直接抓取文件',
      defaultPath: buildLogFilename('direct-capture', 'dat')
    });
    if (!path) return;
    await invokeCommand('serial_start_capture', { sessionId: props.sessionId, path, append: false });
    serialStatus.value = { ...serialStatus.value, capturing: true };
    toast.success('串口直接抓取已开始');
  } catch (error) {
    toast.error(`串口抓取操作失败：${error}`);
  }
};

const toggleSerialReceiveVisible = () => {
  serialReceiveVisible.value = !serialReceiveVisible.value;
  persistSerialReceivePreference();
  toast.info(serialReceiveVisible.value ? '串口接收数据显示已开启' : '串口接收数据显示已隐藏');
};

const openSecurityModal = (matched, data, historySnapshot = null, knowledgeUsageId = '') => {
  blockedCommandContent.value = matched.content;
  blockedCommandSeverity.value = matched.severity;
  pendingData.value = data;
  pendingHistorySnapshot = historySnapshot;
  pendingKnowledgeUsageId = knowledgeUsageId;
  confirmPassword.value = '';
  resetCurrentInputState();
  securityModalVisible.value = true;
};

// Development diagnostics contain timings and counters only, never input text.
const terminalPerf = import.meta.env.DEV ? {
  inputs: 0, lastInputAt: 0, inputDispatchMs: 0, inputToNextOutputMs: null,
} : null;
if (terminalPerf) {
  (globalThis.__dusktermTerminalPerformance ||= new Map()).set(props.sessionId, terminalPerf);
}
const pendingLargePaste = ref(null);
let processTerminalInput = null;
const cancelLargePaste = () => { pendingLargePaste.value = null; };
const confirmLargePaste = () => {
  const pending = pendingLargePaste.value;
  if (!pending || pending.info.tooLarge) return;
  pendingLargePaste.value = null;
  nextTick(() => processTerminalInput?.(pending.data, { pasteConfirmed: true }));
};
const handlePasteDialogCloseFocus = (event) => {
  event.preventDefault();
  nextTick(() => {
    if (!securityModalVisible.value && !pendingLargePaste.value && !terminalDisposed) term?.focus();
  });
};
async function sendData(data) {
  if (terminalTransferOwned.value) return false;
  const session = currentSession.value;
  if (session && (session.status === 'connected' || session.status === 'connecting')) {
    const command = session.isSplitChild ? 'write_ssh_shell_channel' : 'write_ssh';
    const payload = session.isSplitChild
      ? { rootSessionId: session.workspaceSessionId || session.parentId, channelId: props.sessionId, data }
      : { sessionId: props.sessionId, data };
    try {
      const started = performance.now();
      if (terminalPerf) { terminalPerf.inputs++; terminalPerf.lastInputAt = started; }
      await invokeCommand(command, payload);
      if (terminalPerf) terminalPerf.inputDispatchMs = performance.now() - started;
      return true;
    } catch (error) {
      console.error(error);
      const now = Date.now();
      if (now - terminalWriteErrorToastAt < 1500) return;
      terminalWriteErrorToastAt = now;
      toast.error(`${isSerialSession.value ? '串口发送失败' : '输入未发送'}：${error}`);
      return false;
    }
  }
  return false;
}

const formatCloseReason = (reason) => {
  const text = String(reason || '').trim();
  if (isLocalSession.value) {
    return text || '本地 Shell 已关闭。';
  }
  if (isSerialSession.value) {
    return text ? `串口已关闭（${text}）。` : '串口已关闭。';
  }
  return text ? `Connection closed by remote host (${text}).` : 'Connection closed by remote host.';
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
      resetCurrentInputState();
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

const clearSerialAutoReconnect = () => {
  if (serialAutoReconnectTimer) clearTimeout(serialAutoReconnectTimer);
  serialAutoReconnectTimer = null;
};

const scheduleSerialAutoReconnect = () => {
  if (!isSerialSession.value || currentSession.value?.config?.serial_auto_reconnect === false) return;
  if (serialAutoReconnectTimer || reconnectingAfterDisconnect.value) return;
  const delay = Math.min(10000, 1000 * (2 ** Math.min(serialAutoReconnectAttempt, 3)));
  serialAutoReconnectTimer = setTimeout(async () => {
    serialAutoReconnectTimer = null;
    const session = sshStore.sessions.find((item) => item.id === props.sessionId);
    if (!session?.config || session.status === 'connected') return;
    serialAutoReconnectAttempt += 1;
    const ok = await sshStore.reconnectSession(props.sessionId);
    if (!ok) scheduleSerialAutoReconnect();
  }, delay);
};

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
  let routeResult = { handled: false, sent: false };
  const waitHandled = new Promise((resolve) => {
    detail.respond = (result) => {
      if (resolved) return;
      resolved = true;
      routeResult = {
        handled: !!result?.handled,
        sent: !!result?.sent,
      };
      resolve(routeResult);
    };
  });

  window.dispatchEvent(new CustomEvent('terminal-input-route', { detail }));

  if (detail.handledByRouter) {
    await waitHandled;
    if (routeResult.handled) return routeResult.sent;
  }

  return sendData(data);
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
    const sent = await forwardTerminalInput(pendingData.value);
    if (sent && pendingHistorySnapshot) {
      scheduleSubmittedCommandRecord(pendingHistorySnapshot);
    }
    if (sent && pendingKnowledgeUsageId) {
      recordKnowledgeUsage(pendingKnowledgeUsageId);
    }
  }
  pendingData.value = null;
  pendingHistorySnapshot = null;
  pendingKnowledgeUsageId = '';
  securityModalVisible.value = false;
  confirmPassword.value = '';
  resetCurrentInputState();
  term?.focus();
}

function handleSecurityCancel() {
  pendingData.value = null;
  pendingHistorySnapshot = null;
  pendingKnowledgeUsageId = '';
  securityModalVisible.value = false;
  confirmPassword.value = '';
  resetCurrentInputState();
  sendData('\x03');
  term?.focus();
}

function openSettings() {
  pendingData.value = null;
  pendingHistorySnapshot = null;
  pendingKnowledgeUsageId = '';
  confirmPassword.value = '';
  resetCurrentInputState();
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
let unlistenSerialDataSent = null;
let unlistenSerialStatus = null;
let unlistenSerialOperationError = null;
let resizeObserver = null;
let textDecoder = new TextDecoder('utf-8'); // Default
let serialSendLogDecoder = new TextDecoder('utf-8');
let quickCommandHandler = null;
let terminalFocusHandler = null;
let quickHintDebounceTimer = null;
let shellCompletionSyncTimer = null;
let shellCompletionSyncPending = false;
let quickHintPositionRafId = null;
let quickHintSearchToken = 0;
let isLayoutDragging = false;
const layoutDragSources = new Map();
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
const SEARCH_INPUT_DEBOUNCE_MS = 100;
const SEARCH_REFRESH_MS = 250;
const SEARCH_SELECTION_MAX_LENGTH = 512;
const terminalThemeSettings = ref(loadTerminalThemeSettings());
const CJK_MONO_FALLBACK_FONTS = '"Sarasa Mono SC", "Microsoft YaHei Mono", "SimSun", monospace';
const TERMINAL_DEFAULT_FONT = '"Maple Mono"';
const TERMINAL_FONT_FALLBACKS = '"Cascadia Mono", "Courier New", ' + CJK_MONO_FALLBACK_FONTS;

const focusTerminalSurface = () => {
  if (!term) return;
  searchFocusToken++;
  requestAnimationFrame(() => {
    term?.focus();
    requestAnimationFrame(() => {
      term?.focus();
    });
  });
};

let termTitleDisposable = null;
let termDataDisposable = null;
let termResizeDisposable = null;
let termCursorMoveDisposable = null;
let termScrollDisposable = null;

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

const safeUnlisten = (unlisten) => {
  try {
    if (typeof unlisten === 'function') unlisten();
  } catch (error) {
    console.error('Terminal event cleanup failed:', error);
  }
};

const normalizeTerminalFontFamily = (configuredFontFamily) => {
  const value = String(configuredFontFamily || '').trim();
  if (!value || value === 'Consolas' || value === '"Consolas"' || value === "'Consolas'") {
    return TERMINAL_DEFAULT_FONT;
  }
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
  const appShell = terminalWrapperRef.value?.closest('.app-shell');
  const hasGlobalBackground = !!appShell?.classList.contains('has-global-background');
  const opaqueThemeBackground = resolveCssColor(
    baseTheme.background || 'var(--app-bg-dialog)',
    baseTheme.background || '#1e1e1e'
  );
  const themeBackground = hasGlobalBackground ? 'rgba(0, 0, 0, 0)' : opaqueThemeBackground;
  const selectionBackground = resolveCssColor(
    baseTheme.selectionBackground || 'var(--app-selection-bg)',
    'rgba(192,132,47,0.46)'
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
    wrapper.style.setProperty('--terminal-theme-fg', theme.foreground || '#d4d4d4');
  }

  if (typeof term.refresh === 'function' && term.rows > 0) {
    term.refresh(0, term.rows - 1);
  }
};

const handleTerminalThemeChanged = (event) => {
  terminalThemeSettings.value = event?.detail?.settings
    ? { ...event.detail.settings }
    : loadTerminalThemeSettings();
  applyTerminalTheme();
};

const handleGlobalBackgroundAvailabilityChanged = () => requestAnimationFrame(applyTerminalTheme);

watch(isDark, () => {
  applyTerminalTheme();
});

const recordCommandHistory = (command, source = 'terminal') => {
  const session = currentSession.value;
  void commandHistoryStore.record(command, {
    source,
    protocol: session?.config?.protocol || 'ssh',
    host: session?.config?.host || null,
    username: session?.config?.username || null,
  });
};

const getCursorLogicalLineText = ({ fullLine = false } = {}) => {
  const buffer = term?.buffer?.active;
  if (!buffer) return '';

  const cursorVisualIndex = Math.max(0, Number(buffer.baseY || 0) + Number(buffer.cursorY || 0));
  let startIndex = cursorVisualIndex;
  while (startIndex > 0 && buffer.getLine(startIndex)?.isWrapped) {
    startIndex -= 1;
  }

  const parts = [];
  let endIndex = cursorVisualIndex;
  if (fullLine) {
    while (endIndex + 1 < buffer.length && buffer.getLine(endIndex + 1)?.isWrapped) {
      endIndex += 1;
    }
  }

  for (let index = startIndex; index <= endIndex; index += 1) {
    const line = buffer.getLine(index);
    if (!line) continue;
    const text = line.translateToString(false);
    if (!fullLine && index === cursorVisualIndex) {
      parts.push(text.slice(0, Math.max(0, Number(buffer.cursorX || 0))));
    } else {
      parts.push(text);
    }
  }

  const text = parts.join('').replace(/\u00a0/g, ' ');
  return fullLine ? text.trimEnd() : text;
};

const getSubmittedCommandText = () => {
  const local = currentInputState.value;
  if (!local.reliable || !inputLinePrefix) return null;
  const command = String(local.text || '').trim();
  if (!command || /[\r\n]/.test(command)) return null;
  return {
    command,
    echoedText: local.text.trimEnd(),
    linePrefix: inputLinePrefix,
    source: 'terminal',
  };
};

const terminalContainsSubmittedCommand = (snapshot) => {
  const buffer = term?.buffer?.active;
  if (!buffer || !snapshot?.linePrefix || !snapshot?.echoedText) return false;
  const expected = `${snapshot.linePrefix}${snapshot.echoedText}`;
  const end = Math.max(0, Number(buffer.baseY || 0) + Number(buffer.cursorY || 0));
  for (let index = end; index >= Math.max(0, end - 12); index -= 1) {
    let start = index;
    while (start > 0 && buffer.getLine(start)?.isWrapped) start -= 1;
    const parts = [];
    let cursor = start;
    do {
      const line = buffer.getLine(cursor);
      if (!line) break;
      parts.push(line.translateToString(false));
      cursor += 1;
    } while (cursor < buffer.length && buffer.getLine(cursor)?.isWrapped);
    if (parts.join('').replace(/\u00a0/g, ' ').trimEnd() === expected) return true;
    index = start;
  }
  return false;
};

const scheduleSubmittedCommandRecord = (snapshot) => {
  if (!snapshot?.command) return;
  let attempt = 0;
  const verify = () => {
    if (terminalContainsSubmittedCommand(snapshot)) {
      recordCommandHistory(snapshot.command, snapshot.source);
      return;
    }
    attempt += 1;
    if (attempt < 20) setTimeout(verify, 100);
  };
  setTimeout(verify, 0);
};

const syncInputBufferFromTerminal = ({ refreshHints = true } = {}) => {
  if (!inputLinePrefix) return '';
  if (currentInputState.value.reliable) return currentInputState.value.text;
  const extracted = extractAnchoredTerminalInput(getCursorLogicalLineText(), inputLinePrefix);
  if (!extracted.reliable) return '';
  currentInputState.value = replaceTerminalInputState(extracted.text);
  if (refreshHints) scheduleQuickHintUpdate(extracted.text);
  return extracted.text;
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
    syncInputBufferFromTerminal({ refreshHints: !quickHintHistoryNavigation });
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
};

const cancelQuickHintDebounce = () => {
  if (!quickHintDebounceTimer) return;
  clearTimeout(quickHintDebounceTimer);
  quickHintDebounceTimer = null;
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
  const historyItems = query.length >= 2
    ? commandHistoryStore.matches(query, {
      excludedCommands: seenCmds,
      limit: 10,
    })
    : [];

  return { knowledgeItems, historyItems };
};

const updateQuickHintMatches = async (rawInput) => {
  if (quickHintHistoryNavigation) {
    closeQuickHint();
    return;
  }
  const query = normalizeQuickHintQuery(rawInput);
  if (!query) {
    closeQuickHint();
    return;
  }
  if (!inputLinePrefix || currentInputState.value.reliable === false) {
    closeQuickHint();
    return;
  }
  const echoed = extractAnchoredTerminalInput(
    getCursorLogicalLineText({ fullLine: true }),
    inputLinePrefix,
  );
  if (!echoed.reliable || echoed.text.trimEnd() !== currentInputBuffer.value.trimEnd()) {
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
  if (quickHintHistoryNavigation) {
    closeQuickHint();
    return;
  }
  const input = String(rawInput ?? '');
  if (quickHintDismissedInput) {
    if (input === quickHintDismissedInput) {
      closeQuickHint();
      return;
    }
    quickHintDismissedInput = '';
  }
  const query = normalizeQuickHintQuery(rawInput);
  cancelQuickHintDebounce();
  quickHintDebounceTimer = setTimeout(() => {
    quickHintDebounceTimer = null;
    updateQuickHintMatches(rawInput).catch((error) => {
      console.error('Quick hint async match failed:', error);
    });
  }, resolveQuickHintDebounceMs(query));
};

const activateQuickHintSelection = async () => {
  if (isSerialSession.value || securityModalVisible.value) return false;

  quickHintHistoryNavigation = false;
  quickHintDismissedInput = '';
  if (quickHintVisible.value && quickHintItems.value.length > 0) {
    quickHintFocused.value = true;
    quickHintSelectedIndex.value = Math.min(
      quickHintSelectedIndex.value,
      quickHintItems.value.length - 1,
    );
    ensureQuickHintItemVisible();
    return true;
  }

  captureInputLinePrefix();
  const synchronizedInput = syncInputBufferFromTerminal({ refreshHints: false });
  const input = synchronizedInput || currentInputBuffer.value;
  if (!normalizeQuickHintQuery(input)) return false;

  await updateQuickHintMatches(input);
  if (!quickHintVisible.value || quickHintItems.value.length === 0) return false;
  quickHintFocused.value = true;
  quickHintSelectedIndex.value = Math.min(
    quickHintSelectedIndex.value,
    quickHintItems.value.length - 1,
  );
  ensureQuickHintItemVisible();
  return true;
};

const replaceCurrentTerminalLine = (command) => {
  const text = String(command || '').trim();
  if (!text) return null;

  const currentInput = currentInputBuffer.value;
  quickHintHistoryNavigation = false;
  quickHintDismissedInput = text;
  closeQuickHint();
  const sent = forwardTerminalInput(buildTerminalLineReplacementPayload(text, currentInput));
  currentInputState.value = replaceTerminalInputState(text);
  term?.focus();
  return sent;
};

const recordKnowledgeUsage = (id) => {
  if (!id) return;
  void commandKnowledgeStore.recordUsage(id).catch((error) => {
    console.error('Record command knowledge usage failed:', error);
  });
};

const applyQuickHintSelection = () => {
  if (!quickHintVisible.value || quickHintItems.value.length === 0) return false;
  const selected = quickHintItems.value[quickHintSelectedIndex.value];
  const command = String(selected?.command || '').trim();
  if (!command) {
    closeQuickHint();
    return false;
  }

  const sent = replaceCurrentTerminalLine(command);
  if (!sent) return false;
  if (selected?._source === 'knowledge' && selected?.id) {
    void sent.then((wasSent) => {
      if (wasSent) recordKnowledgeUsage(selected.id);
    });
  }
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
async function copyTerminalText(text) {
  const selection = String(text || '');
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
      try {
        textarea.select();
        if (!document.execCommand('copy')) {
          toast.error('复制失败');
          return false;
        }
        toast.success('已复制');
      } catch {
        toast.error('复制失败');
        return false;
      } finally {
        textarea.remove();
      }
    }
    return true;
  }
  return false;
}

async function handleCopy() {
  if (!term) return;
  await copyTerminalText(term.getSelection());
}

function handleTerminalContextMenu(event) {
  if (terminalThemeSettings.value.rightClickBehavior !== 'smart-copy') return;
  const selection = term?.getSelection?.() || '';
  if (!selection) return;

  event.preventDefault();
  event.stopImmediatePropagation();
  void handleCopyAndSafeInsert(selection);
}

async function handlePaste() {
  if (!term) return;
  const text = await navigator.clipboard.readText();
  term.paste(text);
}

async function handleCopyAndSafeInsert(selection) {
  const targetTerm = term;
  if (!targetTerm || !(await copyTerminalText(selection)) || term !== targetTerm) return;

  // Do not forward Enter or any terminal control sequence from a right-click insert.
  targetTerm.paste(String(selection).replace(/[\u0000-\u001F\u007F-\u009F\u2028\u2029]/g, ' '));
}
function handleSelectAll() {
  term?.selectAll();
}
function handleClear() {
  if (!term) return;
  term.write('\x1b[2J\x1b[H');
  markSearchBufferChanged();
  term.scrollToBottom();

}

function clearScrollback() {
  if (!term) return;
  term.write('\x1b[3J\x1b[2J\x1b[H');
  term.clear();
  markSearchBufferChanged();
  term.scrollToBottom();

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
    toast.success(serialRawCaptureTruncated ? '串口接收数据已保存（仅包含最近 5 MB）' : '串口接收数据已保存');
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
    toast.success(serialIoLogTruncated ? '串口收发日志已保存（较早记录已被淘汰）' : '串口收发日志已保存');
  } catch (e) {
    toast.error(`保存失败: ${e}`);
  }
}

// Search calculations and exact counts share one worker result index.
const searchVisible = ref(false);
const searchText = ref('');
const searchOptions = ref({ matchCase: false, regex: false, wholeWord: false, incremental: true });
const searchInput = ref(null);
const searchBarRef = ref(null);
const searchInputFocused = ref(false);
const searchMatchCount = ref(0);
const searchCurrentMatch = ref(0);
const searchResultsPending = ref(false);
const searchError = ref('');
let searchInputDebounceTimer = null;
let searchRefreshTimer = null;
let searchUiToken = 0;
let searchFocusToken = 0;
let searchAddon = null;
const searchDecorations = {
  matchBackground: '#38516c',
  activeMatchBackground: '#65568d',
};
const hasValidSearchKeyword = () => String(searchText.value ?? '').trim().length > 0;
const searchCountLabel = computed(() => searchError.value || (
  searchResultsPending.value
    ? `${searchCurrentMatch.value > 0 ? searchCurrentMatch.value : '…'}/…`
    : `${searchCurrentMatch.value}/${searchMatchCount.value}`
));
const resetSearchStats = () => {
  searchMatchCount.value = 0;
  searchCurrentMatch.value = 0;
  searchResultsPending.value = false;
  searchError.value = '';
};
const cancelSearchInputDebounce = () => {
  clearTimeout(searchInputDebounceTimer);
  searchInputDebounceTimer = null;
};
const cancelSearchScheduling = () => {
  cancelSearchInputDebounce();
  clearTimeout(searchRefreshTimer);
  searchRefreshTimer = null;
  searchUiToken++;
};
const scheduleSearchRefresh = () => {
  if (!props.active || !searchVisible.value || !hasValidSearchKeyword()) return;
  searchResultsPending.value = true;
  // Throttle rather than waiting indefinitely for a continuously printing shell.
  if (searchRefreshTimer) return;
  searchRefreshTimer = setTimeout(() => {
    searchRefreshTimer = null;
    void performSearch(0, { reveal: false });
  }, SEARCH_REFRESH_MS);
};
const markSearchBufferChanged = () => searchAddon?.invalidate(true);
const scheduleSearchFromInput = () => {
  cancelSearchScheduling();
  searchAddon?.clearDecorations();
  resetSearchStats();
  if (!hasValidSearchKeyword()) return;
  searchResultsPending.value = true;
  searchInputDebounceTimer = setTimeout(() => {
    searchInputDebounceTimer = null;
    void performSearch();
  }, SEARCH_INPUT_DEBOUNCE_MS);
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

let webglAddon = null;
function releaseWebglRenderer() {
  if (!webglAddon) return;
  const canvases = [...(term?.element?.querySelectorAll('.xterm-screen canvas') || [])];
  const addon = webglAddon;
  webglAddon = null;
  addon.dispose();
  for (const canvas of canvases) {
    if (!canvas.isConnected) canvas.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
  }
}
let webglLoading = false;
let webglUnavailable = false;
async function setTerminalRenderer(active) {
  if (!active) { releaseWebglRenderer(); return; }
  if (!term?.element || webglAddon || webglLoading || webglUnavailable || terminalDisposed) return;
  webglLoading = true;
  let addon;
  try {
    const { WebglAddon } = await import('@xterm/addon-webgl');
    if (!props.active || terminalDisposed || !term) return;
    addon = new WebglAddon();
    addon.onContextLoss(() => {
      webglUnavailable = true;
      addon.dispose();
      if (webglAddon === addon) webglAddon = null;
      term?.refresh(0, term.rows - 1);
    });
    term.loadAddon(addon);
    webglAddon = addon;
  } catch (error) {
    webglUnavailable = true;
    try { addon?.dispose(); } catch { /* Keep the default renderer. */ }
    console.debug('Terminal uses DOM rendering:', error);
  } finally { webglLoading = false; }
}

let outputWriter = null;
let detachBinaryOutput = null;
let terminalDisposed = false;
const enqueueTerminalOutput = (chunk) => {
  if (!term || terminalDisposed || !chunk) return;
  if (!outputWriter) {
    outputWriter = createTerminalOutput({ write: (text, done) => term.write(text, done) });
    if (terminalPerf) terminalPerf.output = outputWriter.stats;
  }
  outputWriter.enqueue(chunk);
};

function focusSearchInputSoon({ selectText = false } = {}) {
  const token = ++searchFocusToken;
  const previousFocus = document.activeElement;
  nextTick(() => {
    requestAnimationFrame(() => {
      if (token !== searchFocusToken || !searchVisible.value || !props.active) return;
      const currentFocus = document.activeElement;
      if (currentFocus !== previousFocus && currentFocus !== searchInput.value && currentFocus !== document.body) return;
      searchInput.value?.focus({ preventScroll: true });
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
  if (hasValidSearchKeyword()) scheduleSearchFromInput();
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
  searchFocusToken++;
  cancelSearchScheduling();
  searchInputFocused.value = false;
  searchInput.value?.blur();
  searchVisible.value = false;
  searchAddon?.clearDecorations();
  resetSearchStats();
  nextTick(() => {
    requestAnimationFrame(() => term?.focus());
  });
  setTimeout(() => handleResize(), 100);
}

function handleSearchInputFocus() {
  searchInputFocused.value = true;
}

function handleSearchInputBlur() {
  searchFocusToken++;
  searchInputFocused.value = false;
}

function handleSearchInput() {
  if (!hasValidSearchKeyword()) {
    cancelSearchScheduling();
    searchAddon?.clearDecorations();
    resetSearchStats();
    return;
  }
  if (searchOptions.value.incremental) {
    scheduleSearchFromInput();
  }
}

function toggleSearchOption(optionKey) {
  searchOptions.value[optionKey] = !searchOptions.value[optionKey];
  if (searchVisible.value && hasValidSearchKeyword()) {
    scheduleSearchFromInput();
  }
}

async function performSearch(direction = 0, { reveal = true } = {}) {
  if (!searchAddon || !hasValidSearchKeyword()) return;
  const token = ++searchUiToken;
  searchResultsPending.value = true;
  searchError.value = '';
  try {
    const result = await searchAddon.find(searchText.value, {
      caseSensitive: searchOptions.value.matchCase,
      regex: searchOptions.value.regex,
      wholeWord: searchOptions.value.wholeWord,
      decorations: searchDecorations,
      reveal: () => reveal && props.active && searchVisible.value
        && !!searchBarRef.value?.contains(document.activeElement),
    }, direction);
    if (token !== searchUiToken || !searchVisible.value) return;
    if (!result) { scheduleSearchRefresh(); return; }
    searchMatchCount.value = result.count;
    searchCurrentMatch.value = result.current;
    searchResultsPending.value = !!result.stale;
    if (result.stale) scheduleSearchRefresh();
  } catch (error) {
    if (token !== searchUiToken) return;
    resetSearchStats();
    searchError.value = error.message === 'invalid-regex' ? '正则无效' : error.message;
  }
}
function findNext() { cancelSearchInputDebounce(); void performSearch(1); }
function findPrev() { cancelSearchInputDebounce(); void performSearch(-1); }

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

function normalizeTerminalShortcutEvent(event) {
  const parts = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.shiftKey) parts.push('Shift');
  if (event.altKey) parts.push('Alt');
  if (event.metaKey) parts.push('Meta');

  if (['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) return '';
  let key = event.key === ' ' ? 'Space' : String(event.key || '');
  if (key === 'Esc') key = 'Escape';
  if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  return parts.join('+');
}

function matchesTerminalShortcut(event, binding) {
  const expected = String(binding || '').trim().replace(/\s+/g, '').toLowerCase();
  if (!expected) return false;
  return normalizeTerminalShortcutEvent(event).replace(/\s+/g, '').toLowerCase() === expected;
}

function handleTerminalCustomKeyEvent(event) {
  if (event.type !== 'keydown' || event.isComposing) return true;

  if (matchesTerminalShortcut(event, quickHintActivateBinding.value)) {
    event.preventDefault();
    event.stopPropagation();
    void activateQuickHintSelection().catch((error) => {
      console.error('Activate terminal suggestion failed:', error);
    });
    return false;
  }

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
let layoutFitRafId = null;
let needsFitOnActivation = false;
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

          scheduleQuickHintPositionUpdate();
          return;
        }

        lastFittedContainerWidth = width;
        lastFittedContainerHeight = height;
        lastFitAt = now;
        lastProposedCols = dims.cols;
        lastProposedRows = dims.rows;

        fitAddon.fit();


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

function handleResize() {
  if (!props.active) {
    needsFitOnActivation = true;
    return;
  }
  if (resizeTimeout) clearTimeout(resizeTimeout);
  if (isLayoutDragging) {
    if (deferLayoutFit) {
      return;
    }
    scheduleDragFit();
    return;
  }
  resizeTimeout = setTimeout(() => {
    resizeTimeout = null;
    doFit();
  }, 80);
}

function handleLayoutResize() {
  if (isLayoutDragging && deferLayoutFit) {
    needsFitOnActivation = true;
    return;
  }
  if (!props.active) {
    needsFitOnActivation = true;
    return;
  }
  if (resizeTimeout) {
    clearTimeout(resizeTimeout);
    resizeTimeout = null;
  }
  if (layoutFitRafId) return;
  layoutFitRafId = requestAnimationFrame(() => {
    layoutFitRafId = null;
    if (!props.active) {
      needsFitOnActivation = true;
      return;
    }
    doFit();
  });
}

function handleLayoutDragging(event) {
  const source = event?.detail?.source || 'layout';
  if (event?.detail?.dragging) {
    layoutDragSources.set(source, event?.detail?.deferFit !== false);
  } else {
    layoutDragSources.delete(source);
  }
  isLayoutDragging = layoutDragSources.size > 0;
  deferLayoutFit = isLayoutDragging && [...layoutDragSources.values()].some(Boolean);
  if (isLayoutDragging && deferLayoutFit) {
    // A resize callback queued before the gesture must not reflow the terminal mid-drag.
    if (resizeTimeout) clearTimeout(resizeTimeout);
    resizeTimeout = null;
    if (layoutFitRafId) cancelAnimationFrame(layoutFitRafId);
    layoutFitRafId = null;
    if (dragFitRafId) cancelAnimationFrame(dragFitRafId);
    dragFitRafId = null;
    if (dragFitTimerId) clearTimeout(dragFitTimerId);
    dragFitTimerId = null;
  }
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

watch(() => props.active, (active) => {
  void setTerminalRenderer(active);
  if (active) { scheduleSearchRefresh(); searchAddon?.scheduleDecorations(); }
  if (!active) {
    if (resizeTimeout) {
      clearTimeout(resizeTimeout);
      resizeTimeout = null;
    }
    return;
  }

  if (!needsFitOnActivation) return;
  needsFitOnActivation = false;
  nextTick(() => {
    requestAnimationFrame(() => {
      doFit();
    });
  });
});

// --- Context Menu ---
const handleMenuSelect = async (key) => {
  if (key.startsWith('ai-')) aiMenuSelected = true;
  switch (key) {
    case 'ai-text': await bringSelectionToAi(); break;
    case 'ai-visible': await bringOutputToAi('visible'); break;
    case 'ai-recent': await bringOutputToAi('recent'); break;
    case 'ai-recent-500': await bringOutputToAi('recent', 500); break;
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
  if (key !== 'find' && !key.startsWith('ai-')) {
    requestAnimationFrame(() => { term?.focus(); });
  }
};

async function executeKnowledgeCommand(detail, command) {
  const text = String(command || '').trim();
  if (!text) return;
  captureInputLinePrefix();
  syncInputBufferFromTerminal({ refreshHints: false });
  const payload = `${buildTerminalLineReplacementPayload(text, currentInputBuffer.value)}\r`;
  const historySnapshot = {
    command: text,
    echoedText: text,
    linePrefix: inputLinePrefix,
    source: 'knowledge',
  };

  closeQuickHint();
  resetCurrentInputState();

  const matched = findMatchedCommandInPayload(payload, knowledgeSensitiveRules.value);
  const requiresTerminalConfirmation = matched
    && (!detail?.securityConfirmed || matched.severity === 'critical');
  if (requiresTerminalConfirmation) {
    openSecurityModal(matched, payload, historySnapshot, detail?.id);
    return;
  }

  const sent = await forwardTerminalInput(payload);
  if (sent) {
    scheduleSubmittedCommandRecord(historySnapshot);
    recordKnowledgeUsage(detail?.id);
  }
  term?.focus();
}

function handleKnowledgeCommandEvent(event) {
  const detail = event?.detail;
  if (detail?.sessionId && detail.sessionId !== props.sessionId) return;

  const command = typeof detail === 'string' ? detail : detail?.command;
  if (typeof command !== 'string' || command.length === 0) return;

  if (detail?.execute) {
    void executeKnowledgeCommand(detail, command);
    return;
  }

  captureInputLinePrefix();
  syncInputBufferFromTerminal({ refreshHints: false });
  const sent = replaceCurrentTerminalLine(command);
  if (sent && detail?.id) {
    void sent.then((wasSent) => {
      if (wasSent) recordKnowledgeUsage(detail.id);
    });
  }
}

onMounted(async () => {
  loadSyncInputState();
  void commandKnowledgeStore.loadEntries();
  void commandHistoryStore.loadEntries();
  loadSerialReceivePreference();

  const cacheKey = props.sessionId;
  const cached = terminalCache.get(cacheKey);
  const session = sshStore.sessions.find(s => s.id === props.sessionId);
  const config = session?.config || {};

  if (cached) {
    safeUnlisten(cached.unlistenTerminalTransferRequest);
    term = cached.term;
    fitAddon = cached.fitAddon;
    searchAddon = cached.searchAddon;
    termTitleDisposable = cached.termTitleDisposable || null;
    termDataDisposable = cached.termDataDisposable || null;
    termResizeDisposable = cached.termResizeDisposable || null;
    termCursorMoveDisposable = cached.termCursorMoveDisposable || null;
    termScrollDisposable = cached.termScrollDisposable || null;
    unlistenData = cached.unlistenData;
    unlistenDebug = cached.unlistenDebug;
    unlistenConnected = cached.unlistenConnected;
    unlistenClosed = cached.unlistenClosed;
    unlistenError = cached.unlistenError;
    unlistenSerialDataSent = cached.unlistenSerialDataSent;
    unlistenSerialStatus = cached.unlistenSerialStatus;
    unlistenSerialOperationError = cached.unlistenSerialOperationError;
    textDecoder = cached.textDecoder || textDecoder;
    serialSendLogDecoder = cached.serialSendLogDecoder || serialSendLogDecoder;

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

    applyTerminalTextRendering(config);
    applyTerminalTheme();
    term?.attachCustomKeyEventHandler?.(handleTerminalCustomKeyEvent);

  }

  const isCached = !!cached;

  if (!isCached) {
    // 1. Get Session Config

    // 2. Configure Decoder
    if (config.encoding && config.encoding !== 'UTF-8') {
      try {
        textDecoder = new TextDecoder(config.encoding);
        serialSendLogDecoder = new TextDecoder(config.encoding);
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
    searchAddon = new TerminalSearch({
      onInvalidate: scheduleSearchRefresh,
      visible: () => props.active && searchVisible.value,
    });

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
      void setTerminalRenderer(props.active);
      applyTerminalTextRendering(config);
      focusTerminalSurface();
      applyTerminalTheme();

      refreshTerminalSurface(true);

    // Wait a tick for layout to settle before fitting
    setTimeout(() => {
      doFit({ force: true });
    }, 50);

    // Handle user input
    processTerminalInput = (data, { pasteConfirmed = false } = {}) => {
      try {
        if (terminalTransferOwned.value) return;
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

          return;
        }

        if (shouldLockInputByPrimaryMode()) {
          notifyPrimaryLockIfNeeded();
          return;
        }

        if (isSerialSession.value) {
          closeQuickHint();
          resetCurrentInputState();
          forwardTerminalInput(data);
          return;
        }

        if (securityModalVisible.value || pendingLargePaste.value) {
          return;
        }

        if (!pasteConfirmed) {
          const info = describeLargeTerminalPaste(data);
          if (info) {
            closeQuickHint();
            pendingLargePaste.value = { data, info };
            return;
          }
        }

        if (quickHintVisible.value && quickHintFocused.value) {
          if (data === '\x1b[A') {
            moveQuickHintSelection(-1);
            return;
          }
          if (data === '\x1b[B') {
            moveQuickHintSelection(1);
            return;
          }
          if (data === '\x1b') {
            closeQuickHint();
            return;
          }
          if (data === '\r' || data === '\n' || data === '\t') {
            applyQuickHintSelection();
            return;
          }

          // 选择模式下输入其他按键时，退出弹层并将按键继续交给终端。
          closeQuickHint();
        } else if (quickHintVisible.value && data === '\x1b') {
          closeQuickHint();
          return;
        }

        if (data === '\x1b[A' || data === '\x1b[B') {
          // 未通过快捷键进入选择模式时，方向键始终属于 shell history。
          quickHintHistoryNavigation = true;
          closeQuickHint();
        }

        if (data === '\t') {
          captureInputLinePrefix();
          currentInputState.value = updateTerminalInputState(currentInputState.value, data);
          shellCompletionSyncPending = true;
          forwardTerminalInput(data);
          return;
        }

        const isEnter = data === '\r' || data === '\n';
        const isPasteWithNewline = pasteConfirmed || (data.length > 1 && (data.includes('\r') || data.includes('\n')));
        const routedBySync = isCurrentSessionSyncSource();

        if (routedBySync && isPasteWithNewline) {
          resetCurrentInputState();
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (routedBySync && isEnter) {
          syncInputBufferFromTerminal({ refreshHints: false });
          cancelShellCompletionSync();
          const historySnapshot = getSubmittedCommandText();
          resetCurrentInputState();
          closeQuickHint();
          void forwardTerminalInput(data).then((sent) => {
            if (sent && historySnapshot) scheduleSubmittedCommandRecord(historySnapshot);
          });
          return;
        }

        if (!routedBySync && isPasteWithNewline) {
          const matched = findMatchedCommandInPayload(data, knowledgeSensitiveRules.value);
          if (matched) {
            openSecurityModal(matched, data);
            return;
          }
          resetCurrentInputState();
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        if (!routedBySync && isEnter) {
          syncInputBufferFromTerminal({ refreshHints: false });
          cancelShellCompletionSync();
          const historySnapshot = getSubmittedCommandText();
          const matched = matchSensitiveCommand(historySnapshot?.command || '', knowledgeSensitiveRules.value);
          if (matched) {
            openSecurityModal(matched, data, historySnapshot);
            return;
          }
          resetCurrentInputState();
          closeQuickHint();
          void forwardTerminalInput(data).then((sent) => {
            if (sent && historySnapshot) scheduleSubmittedCommandRecord(historySnapshot);
          });
          return;
        }

        if (data === '\u007f' || data === '\b') {
          quickHintHistoryNavigation = false;
          captureInputLinePrefix();
          currentInputState.value = updateTerminalInputState(currentInputState.value, data);
          scheduleQuickHintUpdate(currentInputBuffer.value);
          forwardTerminalInput(data);
          return;
        }

        if (data === '\u0003') {
          cancelShellCompletionSync();
          resetCurrentInputState();
          closeQuickHint();
          forwardTerminalInput(data);
          return;
        }

        const isControlSequence = data.startsWith('\x1b') || /^[\u0000-\u001F\u007F]$/.test(data);
        if (!isControlSequence) {
          quickHintHistoryNavigation = false;
          captureInputLinePrefix();
          currentInputState.value = updateTerminalInputState(currentInputState.value, data);
          scheduleQuickHintUpdate(currentInputBuffer.value);
        } else if (data === '\x1b') {
          currentInputState.value = updateTerminalInputState(currentInputState.value, data);
          closeQuickHint();
        } else {
          captureInputLinePrefix();
          currentInputState.value = updateTerminalInputState(currentInputState.value, data);
          if (data === '\x1b[A' || data === '\x1b[B') {
            shellCompletionSyncPending = true;
          }
          scheduleQuickHintUpdate(currentInputBuffer.value);
        }

        forwardTerminalInput(data);
      } catch (error) {
        console.error('Security interceptor fallback:', error);
        closeQuickHint();
        resetCurrentInputState();
        forwardTerminalInput(data);
      }
    };
    termDataDisposable = term.onData(data => processTerminalInput?.(data));

    // Handle resize
    termResizeDisposable = term.onResize(({ cols, rows }) => {
      if (cols < 2 || rows < 2) return; // Ignore invalid sizes

      sendResizeIfNeeded(cols, rows);

    });

    termCursorMoveDisposable = term.onCursorMove(() => {
      scheduleQuickHintPositionUpdate();
      scheduleShellCompletionSync();
    });

    termScrollDisposable = term.onScroll(() => {
      scheduleQuickHintPositionUpdate();
    });

    // Listen for backend data
    const receiveData = (payload) => {
      if (terminalDisposed) return;
      if (terminalPerf?.lastInputAt) {
        // The next output is a latency hint, not proof of a specific key echo.
        terminalPerf.inputToNextOutputMs = performance.now() - terminalPerf.lastInputAt;
        terminalPerf.lastInputAt = 0;
      }
      // console.log('Terminal Data:', payload);
      if (Array.isArray(payload) || payload instanceof Uint8Array) {
        const rawBytes = payload instanceof Uint8Array ? payload : new Uint8Array(payload);
        const decoded = textDecoder.decode(rawBytes, { stream: true });
        recordSerialReceive(decoded, payload.length, rawBytes);
        if (!isSerialSession.value || serialReceiveVisible.value) {
          enqueueTerminalOutput(isSerialSession.value ? renderSerialReceive(rawBytes, decoded) : decoded);
          if (!isSerialSession.value && shellCompletionSyncPending) {
            scheduleShellCompletionSync();
          }
          if (!isSerialSession.value && currentInputBuffer.value.length >= 1) {
            scheduleQuickHintUpdate(currentInputBuffer.value);
          }
        }
      } else if (typeof payload === 'string') {
        const rawBytes = serialTextEncoder.encode(payload);
        recordSerialReceive(payload, rawBytes.length, rawBytes);
        if (!isSerialSession.value || serialReceiveVisible.value) {
          enqueueTerminalOutput(isSerialSession.value ? renderSerialReceive(rawBytes, payload) : payload);
          if (!isSerialSession.value && shellCompletionSyncPending) {
            scheduleShellCompletionSync();
          }
          if (!isSerialSession.value && currentInputBuffer.value.length >= 1) {
            scheduleQuickHintUpdate(currentInputBuffer.value);
          }
        }
      }
    };
    unlistenData = await listenEvent(`ssh-data-${props.sessionId}`, receiveData);
    if ((isLocalSession.value || !config.protocol || config.protocol.toLowerCase() === 'ssh') && isTauriRuntime()) {
      const detach = await attachTerminalOutput({
        sessionId: props.sessionId, invoke: invokeCommand, listen: listenEvent,
        consume: async (bytes) => {
          receiveData(bytes);
          await outputWriter?.idle();
        },
      });
      if (terminalDisposed) {
        void detach();
        safeUnlisten(unlistenData);
        return;
      }
      detachBinaryOutput = detach;
    }

    // Debug listener
    unlistenDebug = await listenEvent(`ssh-debug-${props.sessionId}`, (msg) => {
      console.log(`[SSH-DEBUG]`, msg);
    });

    // Force a resize after the container settles
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

    }, 200);

    unlistenConnected = await listenEvent(`ssh-connected-${props.sessionId}`, () => {
      clearSerialAutoReconnect();
      serialAutoReconnectAttempt = 0;
      if (isSerialSession.value) serialStatus.value = createSerialStatus();
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

    unlistenClosed = await listenEvent(`ssh-closed-${props.sessionId}`, async (reason) => {
      flushSerialPendingCr();
      stopSerialPeriodicSend();
      serialStatus.value = { ...serialStatus.value, rxRate: 0, txRate: 0, capturing: false, sendingFile: false };
      sshStore.setSessionStatus(props.sessionId, 'disconnected');
      reconnectPromptShown.value = false;
      await detachBinaryOutput?.flush();
      await outputWriter?.idle();
      if (terminalDisposed || !term) return;
      term.write(`\r\n\x1b[31m${formatCloseReason(reason)}\x1b[0m\r\n`);
      term.write(`\x1b[33m按 Enter 键尝试${isLocalSession.value ? '重新启动' : '重连'}。\x1b[0m\r\n`);
      markSearchBufferChanged();

      scheduleSerialAutoReconnect();
    });

    // Listen for errors
    unlistenError = await listenEvent(`ssh-error-${props.sessionId}`, (err) => {
      // A rejected input is not a transport close. Only ssh-closed may end an
      // established session; startup errors still update a connecting session.
      if (currentSession.value?.status === 'connected') {
        toast.error(`终端提示：${err}`);
        return;
      }
      flushSerialPendingCr();
      stopSerialPeriodicSend();
      serialStatus.value = { ...serialStatus.value, rxRate: 0, txRate: 0, capturing: false, sendingFile: false };
      sshStore.setSessionStatus(props.sessionId, 'disconnected');
      reconnectPromptShown.value = false;
      term.write(`\r\n\x1b[31mError: ${err}\x1b[0m\r\n`);
      term.write('\x1b[33m按 Enter 键尝试重连。\x1b[0m\r\n');
      markSearchBufferChanged();
      toast.error(`会话错误：${err}`);

      scheduleSerialAutoReconnect();
    });

    unlistenSerialDataSent = await listenEvent(`serial-data-sent-${props.sessionId}`, (payload) => {
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const decoded = serialSendLogDecoder.decode(bytes, { stream: true });
        recordSerialSend(decoded, payload.length, bytes);
        renderSerialLocalEcho(bytes, decoded);
      } else if (typeof payload === 'string') {
        recordSerialSend(payload, serialTextEncoder.encode(payload).length);
        renderSerialLocalEcho(serialTextEncoder.encode(payload), payload);
      }
    });

    unlistenSerialStatus = await listenEvent(`serial-status-${props.sessionId}`, (payload) => {
      if (!payload || typeof payload !== 'object') return;
      serialStatus.value = { ...serialStatus.value, ...payload };
      if (payload.sendingFile) stopSerialPeriodicSend();
    });

    unlistenSerialOperationError = await listenEvent(`serial-operation-error-${props.sessionId}`, (error) => {
      toast.error(`串口后台操作失败：${error}`);
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

    terminalCache.set(cacheKey, {
      term,
      fitAddon,
      searchAddon,
      termTitleDisposable,
      termDataDisposable,
      termResizeDisposable,
      termCursorMoveDisposable,
      termScrollDisposable,
      unlistenData,
      unlistenDebug,
      unlistenConnected,
      unlistenClosed,
      unlistenError,
      unlistenSerialDataSent,
      unlistenSerialStatus,
      unlistenSerialOperationError,
      textDecoder,
      serialSendLogDecoder
    });
  }

  unregisterContextReader = registerTerminalContextReader(props.sessionId, async ({ mode, lines, signal }) => {
    const targetTerm = term;
    const source = terminalContextSource();
    const status = currentSession.value?.status;
    await new Promise(resolve => setTimeout(resolve, 0));
    if (signal?.aborted) throw new DOMException('带入已取消', 'AbortError');
    if (terminalDisposed || !props.active || targetTerm !== term || status !== currentSession.value?.status
      || !canShareContext.value || securityStore.isLocked) throw new Error('活动终端已变化，请重新采集');
    if (mode === 'selection') return buildTerminalContext(targetTerm.getSelection(), source);
    const snapshot = captureTerminalSnapshot(targetTerm, { mode, lines });
    return buildTerminalContext(snapshot.text, source, snapshot.capturedAt, snapshot.range);
  });
  window.addEventListener('keydown', handleKeydown);
  quickCommandHandler = handleKnowledgeCommandEvent;
  window.addEventListener('command-knowledge-insert', quickCommandHandler);
  window.addEventListener('terminal-theme-changed', handleTerminalThemeChanged);
  window.addEventListener('global-background-availability-changed', handleGlobalBackgroundAvailabilityChanged);
  window.addEventListener('terminal-layout-resize', handleLayoutResize);
  window.addEventListener('terminal-layout-dragging', handleLayoutDragging);
  window.addEventListener('keybindings-changed', refreshQuickHintActivateBinding);
  window.addEventListener('mousedown', handleQuickHintPointerDown, true);
  window.addEventListener('sync-input-changed', onSyncInputChanged);

  if (resizeObserver) resizeObserver.disconnect();
  resizeObserver = new ResizeObserver(() => handleResize());
  if (terminalContainer.value) {
    resizeObserver.observe(terminalContainer.value);
  }

  // Trackpad gesture detection on the terminal wrapper
  terminalWrapperRef.value?.addEventListener('wheel', handleTerminalWheel, { passive: true });

  const readyDimensions = fitAddon?.proposeDimensions?.();
  window.dispatchEvent(
    new CustomEvent('terminal-ready', {
      detail: {
        sessionId: props.sessionId,
        cols: readyDimensions?.cols || term?.cols || 80,
        rows: readyDimensions?.rows || term?.rows || 24
      }
    })
  );
});

onUnmounted(() => {
  aiCaptureController?.abort();
  unregisterContextReader?.();
  terminalDisposed = true;
  pendingLargePaste.value = null;
  processTerminalInput = null;
  globalThis.__dusktermTerminalPerformance?.delete(props.sessionId);
  releaseWebglRenderer();
  outputWriter?.dispose();
  if (detachBinaryOutput) void detachBinaryOutput().catch(console.error);
  stopSerialPeriodicSend();
  clearSerialAutoReconnect();
  clearSerialPendingCr();
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

  if (resizeObserver) resizeObserver.disconnect();
  layoutDragSources.clear();
  isLayoutDragging = false;
  deferLayoutFit = false;
  if (layoutFitRafId) {
    cancelAnimationFrame(layoutFitRafId);
    layoutFitRafId = null;
  }
  if (dragFitRafId) {
    cancelAnimationFrame(dragFitRafId);
    dragFitRafId = null;
  }
  if (dragFitTimerId) {
    clearTimeout(dragFitTimerId);
    dragFitTimerId = null;
  }
  cancelSearchScheduling();
  cancelQuickHintPositionUpdate();
  lastSentCols = 0;
  lastSentRows = 0;
  lastProposedCols = 0;
  lastProposedRows = 0;
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
  if (termScrollDisposable) {
    termScrollDisposable.dispose();
    termScrollDisposable = null;
  }
  safeUnlisten(unlistenData);
  safeUnlisten(unlistenDebug);
  safeUnlisten(unlistenConnected);
  safeUnlisten(unlistenClosed);
  safeUnlisten(unlistenError);
  safeUnlisten(unlistenSerialDataSent);
  safeUnlisten(unlistenSerialStatus);
  safeUnlisten(unlistenSerialOperationError);
  unlistenData = null;
  unlistenDebug = null;
  unlistenConnected = null;
  unlistenClosed = null;
  unlistenError = null;
  unlistenSerialDataSent = null;
  unlistenSerialStatus = null;
  unlistenSerialOperationError = null;
  // Always clear cache on unmount so next mount creates fresh bindings.
  // KeepAlive page switches use onDeactivated/onActivated, not onUnmounted.
  terminalCache.delete(props.sessionId);
  window.removeEventListener('keydown', handleKeydown);
  window.removeEventListener('terminal-theme-changed', handleTerminalThemeChanged);
  window.removeEventListener('global-background-availability-changed', handleGlobalBackgroundAvailabilityChanged);
  window.removeEventListener('terminal-layout-resize', handleLayoutResize);
  window.removeEventListener('terminal-layout-dragging', handleLayoutDragging);
  window.removeEventListener('keybindings-changed', refreshQuickHintActivateBinding);
  window.removeEventListener('mousedown', handleQuickHintPointerDown, true);
  window.removeEventListener('sync-input-changed', onSyncInputChanged);

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

});

</script>

<template>
  <div ref="terminalWrapperRef" class="terminal-wrapper">
    <div v-if="syncBadgeState.visible" class="terminal-sync-status" :class="{ broadcasting: syncBadgeState.broadcastEnabled }">
      <button type="button" class="sync-status-label" @click="openSyncSettings">
        {{ syncBadgeState.channelName }} · {{ syncBadgeState.broadcastEnabled ? '同步中' : '已暂停' }} · {{ syncBadgeState.connectedCount }} 个终端
        <span v-if="syncBadgeState.sourceMode === 'primary'"> · {{ syncBadgeState.isPrimary ? '主控' : '受控' }}</span>
      </button>
      <button v-if="syncBadgeState.broadcastEnabled" type="button" @click="pauseSyncChannel">暂停同步</button>
    </div>
    <div class="terminal-main">
        <div v-if="terminalTransferOwned" class="terminal-transfer-overlay">
          ZMODEM 传输中 · 终端输入已暂停
        </div>
      <ContextMenu @update:open="updateContextMenu">
        <ContextMenuTrigger class="terminal-container-wrap">
          <div ref="terminalContainer" class="terminal-container" @mousedown="focusTerminalSurface"
            @contextmenu.capture="handleTerminalContextMenu"></div>
        </ContextMenuTrigger>
        <ContextMenuContent @close-auto-focus="handleContextMenuCloseFocus">
          <ContextMenuItem v-if="canShareContext" :disabled="!hasAiSelection || aiImporting" @select="handleMenuSelect('ai-text')">选区带入智能助手</ContextMenuItem>
          <template v-if="canShareContext">
            <ContextMenuItem :disabled="aiImporting" @select="handleMenuSelect('ai-visible')">带入当前可见区域</ContextMenuItem>
            <ContextMenuItem :disabled="aiImporting" @select="handleMenuSelect('ai-recent')">带入最近 200 行</ContextMenuItem>
            <ContextMenuItem :disabled="aiImporting" @select="handleMenuSelect('ai-recent-500')">带入最近 500 行</ContextMenuItem>
          </template>
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

    <div v-if="isSerialSession" class="serial-console-panel">
      <div class="serial-status-row">
        <span>RX {{ formatSerialByteCount(serialStatus.rxBytes) }} · {{ formatSerialByteCount(serialStatus.rxRate) }}/s</span>
        <span>TX {{ formatSerialByteCount(serialStatus.txBytes) }} · {{ formatSerialByteCount(serialStatus.txRate) }}/s</span>
        <span :class="{ active: serialStatus.cts }">CTS</span>
        <span :class="{ active: serialStatus.dsr }">DSR</span>
        <span :class="{ active: serialStatus.ri }">RI</span>
        <span :class="{ active: serialStatus.dcd }">DCD</span>
        <span>{{ serialLocalEchoEnabled ? '本地回显' : '设备回显' }}</span>
        <div class="serial-status-spacer"></div>
        <label>显示</label>
        <select v-model="serialDisplayMode" class="serial-compact-select">
          <option value="ascii">ASCII</option>
          <option value="hex">HEX</option>
        </select>
        <Button type="button" variant="ghost" size="sm" @click="serialPanelVisible = !serialPanelVisible">
          {{ serialPanelVisible ? '收起发送区' : '展开发送区' }}
        </Button>
      </div>
      <div v-show="serialPanelVisible" class="serial-send-area">
        <textarea v-model="serialSendText" class="serial-send-input" rows="2" :disabled="serialStatus.sendingFile"
          :placeholder="serialSendMode === 'hex' ? '01 03 00 00 00 02' : '输入要发送的文本，Ctrl+Enter 发送'"
          @keydown.ctrl.enter.prevent="handleSerialSendClick"></textarea>
        <div class="serial-send-controls">
          <select v-model="serialSendMode" class="serial-compact-select">
            <option value="text">文本</option>
            <option value="hex">HEX</option>
          </select>
          <select v-model="serialSendLineEnding" class="serial-compact-select" :disabled="serialSendMode === 'hex'">
            <option value="none">无行尾</option>
            <option value="cr">CR</option>
            <option value="lf">LF</option>
            <option value="crlf">CRLF</option>
          </select>
          <Button type="button" size="sm" :disabled="serialStatus.sendingFile" @click="handleSerialSendClick">发送</Button>
          <Button type="button" variant="outline" size="sm" :disabled="serialStatus.sendingFile" @click="sendSerialFile">
            {{ serialStatus.sendingFile ? '文件发送中' : '发送文件' }}
          </Button>
          <TooltipHint text="周期发送间隔（毫秒）">
            <Input v-model="serialPeriodicInterval" class="serial-interval-input" aria-label="周期发送间隔" />
          </TooltipHint>
          <span class="serial-unit">ms</span>
          <Button type="button" :variant="serialPeriodicSending ? 'destructive' : 'outline'" size="sm"
            :disabled="serialStatus.sendingFile"
            @click="toggleSerialPeriodicSend">{{ serialPeriodicSending ? '停止周期' : '周期发送' }}</Button>
          <Button type="button" :variant="serialDtrEnabled ? 'default' : 'outline'" size="sm"
            @click="setSerialControlLine('dtr')">DTR</Button>
          <Button type="button" :variant="serialRtsEnabled ? 'default' : 'outline'" size="sm"
            @click="setSerialControlLine('rts')">RTS</Button>
          <Button type="button" :variant="serialBreakEnabled ? 'destructive' : 'outline'" size="sm"
            @click="setSerialControlLine('break')">BREAK</Button>
          <Button type="button" variant="outline" size="sm" @click="clearSerialBuffer('input')">清 RX</Button>
          <Button type="button" variant="outline" size="sm" @click="clearSerialBuffer('output')">清 TX</Button>
          <Button type="button" :variant="serialStatus.capturing ? 'destructive' : 'outline'" size="sm"
            @click="toggleSerialCapture">{{ serialStatus.capturing ? '停止抓取' : '直接抓取' }}</Button>
        </div>
      </div>
    </div>

    <!-- Search Bar -->
    <div v-show="searchVisible" ref="searchBarRef" class="search-bar">
      <div class="search-input-wrapper">
        <Search class="search-icon" />
        <input ref="searchInput" v-model="searchText" type="text" placeholder="查找..."
          class="terminal-search-input" autocomplete="off" spellcheck="false" @mousedown.stop @click.stop
          @input="handleSearchInput" @focus="handleSearchInputFocus" @blur="handleSearchInputBlur"
          @keydown="handleSearchKeydown" />
      </div>
      <span class="search-count"
        :class="{ empty: !searchResultsPending && !searchError && searchMatchCount === 0 }">
        {{ searchCountLabel }}
      </span>
      <TooltipHint text="区分大小写" side="bottom">
        <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
          :class="{ active: searchOptions.matchCase }" aria-label="区分大小写"
          :aria-pressed="searchOptions.matchCase"
          @click="toggleSearchOption('matchCase')">
          <CaseSensitive :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
      <TooltipHint text="全词匹配" side="bottom">
        <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
          :class="{ active: searchOptions.wholeWord }" aria-label="全词匹配"
          :aria-pressed="searchOptions.wholeWord"
          @click="toggleSearchOption('wholeWord')">
          <WholeWord :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
      <TooltipHint text="正则表达式" side="bottom">
        <button type="button" class="terminal-find-button terminal-find-icon-button option-button"
          :class="{ active: searchOptions.regex }" aria-label="正则表达式"
          :aria-pressed="searchOptions.regex"
          @click="toggleSearchOption('regex')">
          <Regex :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
      <span class="terminal-find-divider"></span>
      <TooltipHint text="上一个匹配（Shift+J）" side="bottom">
        <button type="button" class="terminal-find-button terminal-find-icon-button"
          aria-label="上一个匹配" @click="findPrev">
          <ChevronUp :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
      <TooltipHint text="下一个匹配（Enter / Shift+K）" side="bottom">
        <button type="button" class="terminal-find-button terminal-find-icon-button"
          aria-label="下一个匹配" @click="findNext">
          <ChevronDown :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
      <TooltipHint text="关闭查找（Esc）" side="bottom">
        <button type="button" class="terminal-find-close terminal-find-icon-button"
          aria-label="关闭查找" @click="closeSearch">
          <X :size="15" stroke-width="1.9" />
        </button>
      </TooltipHint>
    </div>

    <Dialog :open="!!pendingLargePaste" @update:open="(open) => { if (!open) cancelLargePaste(); }">
      <DialogContent class="max-w-lg" @close-auto-focus="handlePasteDialogCloseFocus">
        <DialogHeader>
          <DialogTitle>{{ pendingLargePaste?.info.tooLarge ? '粘贴内容过大' : '确认粘贴大量内容' }}</DialogTitle>
        </DialogHeader>
        <div v-if="pendingLargePaste" class="min-h-0 space-y-3 text-sm">
          <p>将向当前终端粘贴约 {{ (pendingLargePaste.info.bytes / 1024).toFixed(1) }} KiB、{{ pendingLargePaste.info.lines }} 行内容。</p>
          <p v-if="pendingLargePaste.info.tooLarge">单次输入上限为 4 MiB，请分批粘贴或使用文件传输。本次内容尚未发送。</p>
          <p v-else>多行内容可能被 Shell 直接执行，请确认这不是误粘贴。</p>
          <pre class="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md bg-secondary p-3 font-mono text-xs">{{ pendingLargePaste.info.preview }}{{ pendingLargePaste.info.truncated ? '\n…（仅预览开头）' : '' }}</pre>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="cancelLargePaste">取消</Button>
          <Button v-if="pendingLargePaste && !pendingLargePaste.info.tooLarge" @click="confirmLargePaste">确认粘贴</Button>
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
            当前会话: <span style="color: var(--app-text); font-weight: bold;">{{ sessionName }}</span><br>
            如果您确认该操作无误，请点击<span style="color: var(--color-danger)">红色按钮</span>继续。
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
      <div class="quick-hint-guide" role="presentation">
        <template v-if="quickHintFocused">↑/↓ 选择 · Enter 填入 · Esc 返回</template>
        <template v-else>{{ quickHintActivateLabel || '未设置快捷键' }} 进入选择</template>
      </div>
    </div>

  </div>
</template>

<style scoped>
.terminal-sync-status { display: flex; flex-shrink: 0; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 3px 10px; border-bottom: 1px solid var(--app-panel-border); color: var(--app-text-muted); background: var(--app-panel-bg); font: 12px var(--app-font-family); }
.terminal-sync-status.broadcasting { color: var(--app-risk-warning); background: var(--app-risk-warning-bg); }
.terminal-sync-status button { padding: 3px 5px; border: 0; border-radius: 4px; color: inherit; background: transparent; white-space: nowrap; }
.terminal-sync-status .sync-status-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; text-align: left; }
.terminal-sync-status button:hover, .terminal-sync-status button:focus-visible { background: var(--app-btn-hover); outline: 1px solid var(--app-focus-border); }
.terminal-wrapper {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  box-sizing: border-box;
  background-color: var(--terminal-theme-bg, transparent);
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

.terminal-transfer-overlay {
  position: absolute;
  z-index: 5;
  top: 8px;
  right: 50px;
  padding: 5px 9px;
  border: 1px solid color-mix(in srgb, var(--color-primary) 38%, var(--app-border-shadow));
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-bg-dialog) 92%, transparent);
  color: var(--app-text);
  box-shadow: var(--niri-shadow-panel);
  font-size: 12px;
  pointer-events: none;
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
  top: 42px;
  right: 22px;
  z-index: var(--z-floating);
  display: flex;
  align-items: center;
  padding: 4px 8px;
  background: var(--app-bg-dialog);
  border: 1px solid var(--app-border-shadow);
  border-radius: 6px;
  gap: 4px;
  width: min(680px, calc(100% - 44px));
  max-width: calc(100% - 16px);
  box-sizing: border-box;
  pointer-events: auto;
  isolation: isolate;
}

.search-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1 1 auto;
  min-width: 80px;
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
  width: 100%;
  min-width: 0;
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
  min-width: 7ch;
  padding: 0 4px;
  box-sizing: content-box;
  flex: 0 0 auto;
  font-variant-numeric: tabular-nums;
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

.quick-hint-guide {
  position: sticky;
  bottom: 0;
  padding: 4px 10px;
  background: hsl(var(--popover));
  color: hsl(var(--muted-foreground));
  font-size: 10px;
  line-height: 16px;
  text-align: right;
  white-space: nowrap;
  pointer-events: none;
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

.serial-console-panel {
  flex: 0 0 auto;
  border-top: 1px solid var(--app-border-shadow);
  background: color-mix(in srgb, var(--app-bg-dialog) 94%, var(--app-bg));
  color: var(--app-text);
  font-size: 12px;
}

.serial-status-row,
.serial-send-controls {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  min-width: 0;
}

.serial-status-row > span {
  white-space: nowrap;
  color: var(--app-text-muted);
}

.serial-status-row > span.active {
  color: var(--color-success, #4caf50);
  font-weight: 600;
}

.serial-status-spacer {
  flex: 1;
}

.serial-send-area {
  display: grid;
  grid-template-columns: minmax(220px, 1fr) auto;
  gap: 6px;
  padding: 0 8px 7px;
}

.serial-send-input {
  min-height: 54px;
  resize: vertical;
  padding: 6px 8px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 5px;
  outline: none;
  background: var(--app-input-bg);
  color: var(--app-text);
  font: 12px/1.35 var(--font-mono);
}

.serial-send-controls {
  max-width: 560px;
  padding: 0;
  align-content: flex-start;
  flex-wrap: wrap;
}

.serial-compact-select {
  height: 30px;
  padding: 0 6px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 5px;
  background: var(--app-input-bg);
  color: var(--app-text);
  font-size: 12px;
}

.serial-interval-input {
  width: 72px;
  height: 30px;
  font-size: 12px;
}

.serial-unit {
  margin-left: -4px;
  color: var(--app-text-muted);
}

@media (max-width: 900px) {
  .serial-send-area {
    grid-template-columns: 1fr;
  }
  .serial-status-row > span:nth-child(n+3):nth-child(-n+6) {
    display: none;
  }
}

</style>
