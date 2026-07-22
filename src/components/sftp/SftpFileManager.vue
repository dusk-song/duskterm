<script setup>
import Button from '@/components/ui/button/Button.vue';
import ContextMenu from '@/components/ui/context-menu/ContextMenu.vue';
import ContextMenuContent from '@/components/ui/context-menu/ContextMenuContent.vue';
import ContextMenuItem from '@/components/ui/context-menu/ContextMenuItem.vue';
import ContextMenuSeparator from '@/components/ui/context-menu/ContextMenuSeparator.vue';
import ContextMenuTrigger from '@/components/ui/context-menu/ContextMenuTrigger.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import LoadingSpinner from '@/components/ui/loading-spinner/LoadingSpinner.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import {
  ArrowLeft,
  Download,
  LocateFixed,
  RefreshCw,
  RotateCcw,
  Save as SaveIcon,
  Upload,
  X
} from '@lucide/vue';
import { open, save } from '@tauri-apps/plugin-dialog';
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useFileSelection } from '@/composables/useFileSelection';
import { useSftpPager } from '@/composables/useSftpPager';
import { useVirtualList } from '@/composables/useVirtualList';
import { useSftpTransfersStore } from '@/stores/sftpTransfers';
import { useSshStore } from '@/stores/ssh';
import { formatLocalTime, formatPermissions, formatSize, joinRemotePath } from '@/types/sftp';
import { invokeCommand } from '@/utils/ipc';
import CodeEditor from './CodeEditor.vue';
import FileIcon from '@/components/common/FileIcon.vue';
import IconButton from '@/components/common/IconButton.vue';

const props = defineProps({
  sessionId: String,
  followSessionId: String,
  visible: Boolean
});
const emit = defineEmits(['close']);

const sshStore = useSshStore();
const transferStore = useSftpTransfersStore();
const activeSession = computed(() => sshStore.getSession(props.sessionId));
const followedTerminalSession = computed(() => sshStore.getSession(props.followSessionId || props.sessionId));
const activeSessionStatus = computed(() => activeSession.value?.status || 'disconnected');
const currentPath = ref('/');
const connected = ref(false);
const initializing = ref(false);
const netError = ref('');
const followTerminalPath = ref(false);
const lastObservedTerminalCwd = ref('');
const showBatchActions = ref(false);
const showSelectionColumn = ref(false);

const headerScrollRef = ref(null);
const bodyScrollRef = ref(null);
const bottomScrollRef = ref(null);
const viewportRef = ref(null);

// Direct pixel column widths — simple and reliable for drag resize
const colPx = reactive({
  select: 0,
  name: 280,
  modified: 180,
  size: 90,
  permissions: 180,
  ownerGroup: 120,
});

// Container width tracking for auto-fit
const containerWidth = ref(800);
let _colResizeObserver = null;
let _colResizeFrame = null;
let _pendingContainerWidth = 800;

// Recompute column widths when container resizes
function recalcColumnWidths() {
  const cw = containerWidth.value;
  if (cw < 100) return;
  const selectW = showSelectionColumn.value ? 44 : 0;
  const avail = Math.max(200, cw - selectW);
  const totalFlex = 3 + 2 + 1 + 2 + 1.5; // name:3 modified:2 size:1 permissions:2 ownerGroup:1.5
  const u = avail / totalFlex;
  colPx.select = selectW;
  colPx.name = Math.round(3 * u);
  colPx.modified = Math.round(2 * u);
  colPx.size = Math.round(1 * u);
  colPx.permissions = Math.round(2 * u);
  colPx.ownerGroup = Math.round(1.5 * u);
}

const pager = useSftpPager({
  sessionIdRef: computed(() => props.sessionId),
  pathRef: currentPath,
  pageSize: 200
});

const draftFolderVisible = ref(false);
const draftFolderName = ref('');
const draftFolderSubmitting = ref(false);
const draftFolderInputRef = ref(null);
const draftFolderRecord = computed(() => (
  draftFolderVisible.value
    ? {
      name: draftFolderName.value,
      is_dir: true,
      modified: 0,
      size: 0,
      permissions: 0,
      owner: '',
      group: '',
      __draftFolder: true,
    }
    : null
));

const listItems = computed(() => {
  const rows = [];
  if (currentPath.value !== '/') {
    rows.push({ name: '..', is_dir: true, __parent: true });
  }
  if (draftFolderRecord.value) {
    rows.push(draftFolderRecord.value);
  }
  return rows.concat(pager.items.value);
});

const selection = useFileSelection(listItems);
const batchState = reactive({
  running: false,
  action: '',
  total: 0,
  success: 0,
  failed: 0
});

const ctxRecord = ref(null);

const dragState = reactive({
  draggingName: '',
  draggingIndex: -1
});
const navigatingDir = ref(false);
const resizingColumnKey = ref('');
const reconnectingSftp = ref(false);

const editorVisible = ref(false);
const editorLoading = ref(false);
const editorReadonly = ref(false);
const editorSaving = ref(false);
const editorRef = ref(null);
const editorDirty = ref(false);
const editorOpenRequestId = ref(0);
const editorFilePath = ref('');
const editorLanguage = ref('plaintext');
const editorContent = ref('');
const editorOriginalContent = ref('');
const editorReadFallback = ref(false);
const editorCasToken = ref('');
const INLINE_EDITOR_MAX_BYTES = 8 * 1024 * 1024;
const INLINE_EDITOR_LIGHTWEIGHT_BYTES = 1024 * 1024;
const editorMeta = reactive({
  size: 0,
  modified: 0,
  permissions: 0,
  owner: '-',
  group: '-'
});
const editorCursor = reactive({
  line: 1,
  column: 1
});
const editorCloseConfirmVisible = ref(false);

const listVirtual = useVirtualList({
  items: listItems,
  rowHeight: 32,
  overscan: 10
});

const visibleRows = computed(() => listVirtual.visibleItems.value.map((item, idx) => ({
  item,
  index: listVirtual.startIndex.value + idx
})));

const selectedRecords = computed(() => selection.selectedList.value.filter(item => !item.__parent));
const panelStateCache = new Map();
let sessionSyncGeneration = 0;
const initializingSessionIds = new Set();

function isCurrentSessionSync(sessionId, generation) {
  return props.sessionId === sessionId && sessionSyncGeneration === generation;
}

function scheduleColumnWidthRecalc(width) {
  const nextWidth = Math.round(Number(width) || 0);
  if (nextWidth <= 0) return;
  _pendingContainerWidth = nextWidth;
  if (_colResizeFrame) return;
  _colResizeFrame = requestAnimationFrame(() => {
    _colResizeFrame = null;
    if (containerWidth.value !== _pendingContainerWidth) {
      containerWidth.value = _pendingContainerWidth;
      recalcColumnWidths();
    }
  });
}
const confirmOverwrite = (remotePath) => new Promise((resolve) => {
  confirm({
    title: '文件已存在',
    content: `目标文件已存在，是否覆盖？\n${remotePath}`,
    okText: '覆盖',
    cancelText: '取消',
    centered: true,
    onOk: () => resolve(true),
    onCancel: () => resolve(false)
  });
});

const createTransferTask = (sessionId, direction, fileName, localPath, remotePath) => (
  transferStore.createTask({ sessionId, direction, fileName, localPath, remotePath })
);

const createUploadTask = (sessionId, filePath, remotePath) => {
  const fileName = filePath.split(/[/\\]/).pop() || 'unknown';
  return createTransferTask(sessionId, 'upload', fileName, filePath, remotePath);
};

const createDownloadTask = (sessionId, remotePath, localPath, fileName) => (
  createTransferTask(sessionId, 'download', fileName, localPath, remotePath)
);

const markUploadTaskFailed = (task, err) => {
  task.status = 'failed';
  task.error = String(err || '上传失败');
  task.rate = 0;
  task.etaSeconds = null;
  task.telemetrySamples = [];
};

const markTransferTaskFailed = (task, err, fallback) => {
  task.status = 'failed';
  task.error = String(err || fallback);
  task.rate = 0;
  task.etaSeconds = null;
  task.telemetrySamples = [];
};

const markTransferTaskCancelled = (task) => {
  task.status = 'cancelled';
  task.error = '已取消';
  task.rate = 0;
  task.etaSeconds = null;
  task.telemetrySamples = [];
};

const isCancelledTransfer = (task, error) => (
  task.status === 'cancelling'
  || task.status === 'cancelled'
  || /cancel|取消/i.test(String(error || ''))
);

const summarizeTransferTasks = (tasks) => ({
  success: tasks.filter((task) => task.status === 'success').length,
  cancelled: tasks.filter((task) => task.status === 'cancelled').length,
  failed: tasks.filter((task) => task.status === 'failed').length
});

const notifyTransferSummary = (action, tasks) => {
  const summary = summarizeTransferTasks(tasks);
  if (summary.failed > 0) {
    toast.warning(`${action}完成：成功 ${summary.success}，取消 ${summary.cancelled}，失败 ${summary.failed}`);
  } else if (summary.cancelled > 0) {
    toast.info(`${action}完成：成功 ${summary.success}，取消 ${summary.cancelled}`);
  } else {
    toast.success(`${action}完成：${summary.success} 个文件`);
  }
};

const verifyUploadedTask = async (task) => {
  try {
    const stat = await invokeCommand('sftp_stat', {
      sessionId: task.sessionId,
      path: task.remotePath
    });
    const remoteSize = Number(stat?.size || 0);
    const expectedSize = Number(task.total || 0);
    if (expectedSize > 0 && remoteSize !== expectedSize) {
      console.warn(`[SFTP] Upload Verify Mismatch: expected ${expectedSize}, got ${remoteSize}`);
    }
    task.total = expectedSize > 0 ? expectedSize : remoteSize;
    task.current = task.total;
  } catch (err) {
    console.warn(`[SFTP] Upload Verify Error: ${err}`);
  }
};

const runSingleUploadTask = async (task) => {
  task.status = 'uploading';
  task.error = '';
  task.rate = 0;
  task.etaSeconds = null;
  task.telemetrySamples = [];
  const uploadPayload = {
    sessionId: task.sessionId,
    localPath: task.localPath,
    remotePath: task.remotePath,
    reqId: task.id
  };

  const invokeUpload = async () => {
    await invokeCommand('sftp_upload_file', uploadPayload);
  };

  try {
    await invokeUpload();
    // For 0 byte file or quick uploads, we don't throw verified error to avoid rollback.
    if (task.total > 0) {
      await verifyUploadedTask(task);
    }
    task.status = 'success';
    task.percent = task.total > 0 ? 100 : 0;
    if (task.total > 0) task.current = task.total;
  } catch (err) {
    if (isCancelledTransfer(task, err)) {
      markTransferTaskCancelled(task);
      throw err;
    }
    if (isRecoverableSftpError(err) && task.sessionId === props.sessionId) {
      const ready = await ensureSftpReady();
      if (ready) {
        try {
          await invokeUpload();
          if (task.total > 0) {
            await verifyUploadedTask(task);
          }
          task.status = 'success';
          task.percent = task.total > 0 ? 100 : 0;
          if (task.total > 0) task.current = task.total;
          return;
        } catch (retryErr) {
          if (isCancelledTransfer(task, retryErr)) {
            markTransferTaskCancelled(task);
            throw retryErr;
          }
          markUploadTaskFailed(task, retryErr);
          toast.error(`重试上传失败: ${task.fileName} - ${String(retryErr)}`);
          throw retryErr;
        }
      }
    }
    markUploadTaskFailed(task, err);
    toast.error(`上传失败: ${task.fileName} - ${String(err)}`);
    throw err;
  }
};

const runSingleDownloadTask = async (task) => {
  task.status = 'uploading';
  task.error = '';
  task.rate = 0;
  task.etaSeconds = null;
  task.telemetrySamples = [];
  try {
    await invokeCommand('sftp_download_file', {
      sessionId: task.sessionId,
      remotePath: task.remotePath,
      localPath: task.localPath,
      reqId: task.id
    });
    task.status = 'success';
    task.percent = task.total > 0 ? 100 : 0;
    if (task.total > 0) task.current = task.total;
    task.etaSeconds = 0;
  } catch (err) {
    if (isCancelledTransfer(task, err)) {
      markTransferTaskCancelled(task);
      throw err;
    }
    markTransferTaskFailed(task, err, '下载失败');
    toast.error(`下载失败: ${task.fileName} - ${String(err)}`);
    throw err;
  }
};

const runUploadTasksWithConcurrency = async (tasks, concurrency = 1) => {
  const queue = [...tasks];
  const workerCount = concurrency > 0
    ? Math.max(1, Math.min(concurrency, queue.length))
    : 1;
  const workers = Array.from({ length: workerCount }, async () => {
    while (queue.length) {
      const task = queue.shift();
      if (!task) return;
      if (task.status === 'cancelled') continue;
      try {
        await runSingleUploadTask(task);
      } catch {
      }
    }
  });
  await Promise.all(workers);
};

const runDownloadTasksWithConcurrency = async (tasks, concurrency = 1) => {
  const queue = [...tasks];
  const workerCount = concurrency > 0
    ? Math.max(1, Math.min(concurrency, queue.length))
    : 1;
  const workers = Array.from({ length: workerCount }, async () => {
    while (queue.length) {
      const task = queue.shift();
      if (!task) return;
      if (task.status === 'cancelled') continue;
      try {
        await runSingleDownloadTask(task);
      } catch {
      }
    }
  });
  await Promise.all(workers);
};

const formatEntrySize = (record) => (!record || record.is_dir ? '-' : formatSize(record.size || 0));
const localTime = (record) => formatLocalTime(record?.modified || 0);
const permissionText = (record) => formatPermissions(record?.permissions || 0);
const hasWritePermission = (permissions) => ((permissions || 0) & 0o222) !== 0;
const editorLargeFile = computed(() => Number(editorMeta.size || editorOriginalContent.value.length || 0) >= INLINE_EDITOR_LIGHTWEIGHT_BYTES);
const editorCanSave = computed(() => !editorReadonly.value && !editorSaving.value && !editorLoading.value && editorDirty.value);
const editorFileName = computed(() => editorFilePath.value.split('/').pop() || '');

function patchPagerItem(nextEntry) {
  if (!nextEntry?.name) return;
  const index = pager.items.value.findIndex((item) => item?.name === nextEntry.name);
  if (index < 0) return;
  pager.items.value.splice(index, 1, { ...pager.items.value[index], ...nextEntry });
  cachePanelState();
}

const EDITABLE_LANGUAGE_MAP = Object.freeze({
  txt: 'plaintext', log: 'plaintext', conf: 'plaintext', cfg: 'plaintext', ini: 'plaintext', env: 'shell',
  json: 'json', jsonc: 'json', yaml: 'yaml', yml: 'yaml', toml: 'ini', xml: 'xml', csv: 'plaintext',
  tsv: 'plaintext', md: 'markdown', markdown: 'markdown', sql: 'sql', gql: 'graphql', graphql: 'graphql',
  js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript', ts: 'typescript', tsx: 'typescript',
  vue: 'html', html: 'html', htm: 'html', css: 'css', scss: 'scss', less: 'less',
  rs: 'rust', py: 'python', sh: 'shell', bash: 'shell', zsh: 'shell', ps1: 'powershell', bat: 'bat',
  c: 'c', h: 'c', cpp: 'cpp', cc: 'cpp', cxx: 'cpp', hpp: 'cpp', hxx: 'cpp', java: 'java',
  go: 'go', php: 'php', rb: 'ruby', swift: 'swift', kt: 'kotlin'
});

const EDITABLE_FILE_NAMES = new Set([
  'dockerfile', 'makefile', 'readme', 'license', '.gitignore', '.gitattributes', '.editorconfig',
  '.npmrc', '.prettierrc', '.eslintrc', '.bashrc', '.zshrc', '.profile'
]);

function normalizeFileName(filename) {
  return String(filename || '').split('/').pop()?.trim().toLowerCase() || '';
}

function isEditableFileName(filename) {
  const normalized = normalizeFileName(filename);
  if (!normalized) return false;
  if (EDITABLE_FILE_NAMES.has(normalized) || normalized.startsWith('.env')) return true;
  const dotIndex = normalized.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === normalized.length - 1) return false;
  const ext = normalized.slice(dotIndex + 1);
  return Object.prototype.hasOwnProperty.call(EDITABLE_LANGUAGE_MAP, ext);
}

function getEditBlockedReason(filename, size = 0) {
  if (Number(size || 0) > INLINE_EDITOR_MAX_BYTES) {
    return `当前在线编辑仅支持 ${formatSize(INLINE_EDITOR_MAX_BYTES)} 以内的文本文件。请下载后编辑该大文件。`;
  }
  if (isEditableFileName(filename)) return '';
  return '当前编辑器仅支持文本、代码和配置类文件。该文件类型不支持在线编辑，请下载后处理。';
}

function getLanguageFromExt(filename) {
  const normalized = normalizeFileName(filename);
  if (EDITABLE_FILE_NAMES.has(normalized)) {
    if (normalized === 'dockerfile') return 'shell';
    return 'plaintext';
  }
  if (normalized.startsWith('.env')) return 'shell';
  const dotIndex = normalized.lastIndexOf('.');
  const ext = dotIndex > -1 ? normalized.slice(dotIndex + 1) : '';
  return EDITABLE_LANGUAGE_MAP[ext] || 'plaintext';
}

function isDisconnectError(err) {
  const text = String(err || '').toLowerCase();
  return text.includes('session not found')
    || text.includes('session closed')
    || text.includes('disconnect')
    || text.includes('channel')
    || text.includes('broken pipe');
}

function isRecoverableSftpError(err) {
  const text = String(err || '').toLowerCase();
  return isDisconnectError(err)
    || text.includes('timeout')
    || text.includes('timed out')
    || text.includes('connection reset')
    || text.includes('connection aborted')
    || text.includes('resource temporarily unavailable');
}

function throttle(fn, wait) {
  let pending = false;
  return (...args) => {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      fn(...args);
    }, wait);
  };
}

function cachePanelState() {
  cachePanelStateForSession(props.sessionId);
}

function cachePanelStateForSession(sessionId) {
  if (!sessionId) return;
  panelStateCache.set(sessionId, {
    path: currentPath.value,
    selected: Array.from(selection.selectedKeys.value),
    scrollTop: bodyScrollRef.value?.scrollTop || 0,
    items: (pager.items.value || []).map(item => ({ ...item })),
    total: Number(pager.total.value || 0),
    totalKnown: !!pager.totalKnown.value,
    offset: Number(pager.offset.value || 0),
    hasMore: !!pager.hasMore.value,
    followTerminalPath: followTerminalPath.value,
    lastObservedTerminalCwd: lastObservedTerminalCwd.value
  });
}

async function restorePanelState() {
  if (!props.sessionId) return;
  const state = panelStateCache.get(props.sessionId);
  if (!state) return;
  currentPath.value = normalizeSftpPath(state.path);
  followTerminalPath.value = !!state.followTerminalPath;
  lastObservedTerminalCwd.value = state.lastObservedTerminalCwd || '';
  if (Array.isArray(state.items)) {
    pager.items.value = state.items.map(item => ({ ...item }));
    pager.total.value = Number(state.total || state.items.length || 0);
    pager.totalKnown.value = !!state.totalKnown;
    pager.offset.value = Number(state.offset || state.items.length || 0);
    pager.hasMore.value = !!state.hasMore;
  } else {
    await loadFirstPage();
  }
  selection.clearSelection();
  const picked = new Set(state.selected || []);
  const next = new Set();
  listItems.value.forEach((item) => {
    if (picked.has(item.name)) next.add(item.name);
  });
  selection.selectedKeys.value = next;
  await nextTick();
  if (bodyScrollRef.value) bodyScrollRef.value.scrollTop = state.scrollTop || 0;
}

async function initSftp(forceReconnect = false, options = {}) {
  const sessionId = options.sessionId || props.sessionId;
  const generation = options.generation ?? sessionSyncGeneration;
  if (!sessionId) return;
  const session = sshStore.getSession(sessionId);
  if (!session) return;

  if (initializingSessionIds.has(sessionId)) return;
  netError.value = '';
  if (session.status !== 'connected') {
    sshStore.markSftpDisconnected(sessionId);
    if (isCurrentSessionSync(sessionId, generation)) {
      connected.value = false;
      netError.value = session.status === 'connecting' ? '等待终端 SSH 会话建立...' : '终端会话未连接';
    }
    return;
  }

  if (!forceReconnect && sshStore.isSftpConnected(sessionId)) {
    if (!isCurrentSessionSync(sessionId, generation)) return;
    connected.value = true;
    await restorePanelState();
    if (isCurrentSessionSync(sessionId, generation) && !panelStateCache.get(sessionId)) await loadFirstPage();
    return;
  }

  if (forceReconnect) {
    await disconnectSftpSession(sessionId);
    if (!isCurrentSessionSync(sessionId, generation)) return;
  }

  initializingSessionIds.add(sessionId);
  initializing.value = true;
  try {
    const config = session.config || {};
    if (String(config.protocol || 'ssh').toLowerCase() !== 'ssh') {
      throw new Error('当前会话不是 SSH 协议，SFTP 仅支持 SSH 会话');
    }
    await invokeCommand('connect_sftp', {
      sessionId,
      config: {
        ...config,
        connect_timeout: config.connect_timeout ?? 10
      }
    });
    sshStore.markSftpConnected(sessionId);
    if (!isCurrentSessionSync(sessionId, generation)) return;
    connected.value = true;
    await restorePanelState();
    if (isCurrentSessionSync(sessionId, generation) && !panelStateCache.get(sessionId)) await loadFirstPage();
  } catch (err) {
    sshStore.markSftpDisconnected(sessionId);
    if (isCurrentSessionSync(sessionId, generation)) {
      connected.value = false;
      netError.value = `SFTP 连接失败：${String(err)}`;
    }
  } finally {
    initializingSessionIds.delete(sessionId);
    if (props.sessionId === sessionId) initializing.value = false;
    if (props.sessionId === sessionId && sessionSyncGeneration !== generation) {
      const latestGeneration = sessionSyncGeneration;
      queueMicrotask(() => syncVisibleSessionState({ sessionId, generation: latestGeneration }));
    }
  }
}

async function queryBackendSftpConnected(sessionId) {
  if (!sessionId) return false;
  try {
    return !!(await invokeCommand('sftp_is_connected', { sessionId }));
  } catch {
    return false;
  }
}

async function syncVisibleSessionState(options = {}) {
  const { restore = true, sessionId = props.sessionId, generation = sessionSyncGeneration } = options;

  if (!sessionId) {
    connected.value = false;
    netError.value = '暂无连接';
    return;
  }

  const session = sshStore.getSession(sessionId);
  if (!session) {
    connected.value = false;
    netError.value = '暂无连接';
    return;
  }

  if (session.status !== 'connected') {
    connected.value = false;
    netError.value = session.status === 'connecting' ? '等待终端 SSH 会话建立...' : '终端会话未连接';
    return;
  }

  let hasSftp = sshStore.isSftpConnected(sessionId);
  if (!hasSftp) {
    hasSftp = await queryBackendSftpConnected(sessionId);
    if (!isCurrentSessionSync(sessionId, generation)) return;
    if (hasSftp) {
      sshStore.markSftpConnected(sessionId);
    }
  }

  if (!hasSftp) {
    await initSftp(false, { sessionId, generation });
    if (!isCurrentSessionSync(sessionId, generation)) return;
    hasSftp = connected.value && sshStore.isSftpConnected(props.sessionId);
  }

  connected.value = hasSftp;
  netError.value = hasSftp ? '' : 'SFTP 未连接';

  if (hasSftp && restore) {
    await restorePanelState();
    if (!isCurrentSessionSync(sessionId, generation)) return;
    if (!panelStateCache.get(props.sessionId)) {
      await loadFirstPage();
    }
  }
}

async function ensureSftpReady() {
  if (!props.sessionId) return false;
  if (reconnectingSftp.value) return false;
  if (activeSessionStatus.value !== 'connected') {
    connected.value = false;
    sshStore.markSftpDisconnected(props.sessionId);
    netError.value = activeSessionStatus.value === 'connecting' ? '等待终端 SSH 会话建立...' : '终端会话未连接';
    return false;
  }

  if (!connected.value || !sshStore.isSftpConnected(props.sessionId)) {
    reconnectingSftp.value = true;
    try {
      await initSftp(true);
    } finally {
      reconnectingSftp.value = false;
    }
    return connected.value;
  }

  try {
    await invokeCommand('sftp_exists', {
      sessionId: props.sessionId,
      path: currentPath.value || '/'
    });
    return true;
  } catch (err) {
    if (!isRecoverableSftpError(err)) return false;
    reconnectingSftp.value = true;
    try {
      await initSftp(true);
    } finally {
      reconnectingSftp.value = false;
    }
    return connected.value;
  }
}

async function loadFirstPage() {
  const sessionId = props.sessionId;
  const path = currentPath.value;
  try {
    const loaded = await pager.loadFirstPage();
    if (loaded === false || props.sessionId !== sessionId || currentPath.value !== path) return false;
    netError.value = '';
    await nextTick();
    syncBottomScrollbar();
    cachePanelState();
    return true;
  } catch (err) {
    if (props.sessionId !== sessionId || currentPath.value !== path) return false;
    netError.value = `文件列表加载失败：${String(err)}`;
    if (isDisconnectError(err)) {
      connected.value = false;
      sshStore.markSftpDisconnected(props.sessionId);
    }
    return false;
  }
}

function focusDraftFolderInput() {
  nextTick(() => {
    draftFolderInputRef.value?.focus?.();
    draftFolderInputRef.value?.select?.();
  });
}

function setDraftFolderInputRef(element) {
  draftFolderInputRef.value = element;
}

function cancelCreateFolderDraft() {
  if (draftFolderSubmitting.value) return;
  draftFolderVisible.value = false;
  draftFolderName.value = '';
}

function createFolder() {
  if (draftFolderVisible.value) {
    focusDraftFolderInput();
    return;
  }
  draftFolderVisible.value = true;
  draftFolderName.value = '新建文件夹';
  focusDraftFolderInput();
}

async function submitCreateFolderDraft(options = {}) {
  const { cancelIfEmpty = false } = options;
  if (draftFolderSubmitting.value) return;

  const name = String(draftFolderName.value || '').trim();
  if (!name) {
    if (cancelIfEmpty) {
      cancelCreateFolderDraft();
      return;
    }
    toast.warning('请输入文件夹名称');
    focusDraftFolderInput();
    return;
  }

  draftFolderSubmitting.value = true;
  try {
    await invokeCommand('sftp_mkdir', {
      sessionId: props.sessionId,
      path: joinRemotePath(currentPath.value, name)
    });
    draftFolderVisible.value = false;
    draftFolderName.value = '';
    await loadFirstPage();
  } finally {
    draftFolderSubmitting.value = false;
  }
}

function onDraftFolderKeydown(event) {
  if (event.key === 'Enter') {
    event.preventDefault();
    submitCreateFolderDraft();
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelCreateFolderDraft();
  }
}

function isContextActionDisabled(action, record) {
  // Right-click on empty space: only allow general actions
  if (!record) {
    return action === 'edit' || action === 'download' || action === 'prop';
  }
  if (record.__draftFolder) {
    return true;
  }
  if (record.__parent) {
    return action === 'edit' || action === 'download' || action === 'prop';
  }
  if (action === 'edit') {
    return !!record.is_dir;
  }
  return false;
}

function closeEditorState() {
  editorOpenRequestId.value += 1;
  editorVisible.value = false;
  editorLoading.value = false;
  editorSaving.value = false;
  editorReadonly.value = false;
  editorDirty.value = false;
  editorReadFallback.value = false;
  editorRef.value = null;
  editorFilePath.value = '';
  editorContent.value = '';
  editorOriginalContent.value = '';
  editorCasToken.value = '';
  editorCursor.line = 1;
  editorCursor.column = 1;
  editorCloseConfirmVisible.value = false;
}

const onBodyScroll = throttle(async (event) => {
  listVirtual.onScroll(event);
  syncXFromBody();
  const el = event.target;
  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 120;
  if (nearBottom && pager.canLoadMore.value) {
    try {
      await pager.loadNextPage();
    } catch (err) {
      if (isDisconnectError(err)) {
        connected.value = false;
        sshStore.markSftpDisconnected(props.sessionId);
      }
    }
  }
  cachePanelState();
}, 80);

function onTableWheel(event) {
  const horizontalDelta = Math.abs(event.deltaX) > 0 ? event.deltaX : (event.shiftKey ? event.deltaY : 0);
  if (!horizontalDelta || !bodyScrollRef.value) return;
  const maxScrollLeft = bodyScrollRef.value.scrollWidth - bodyScrollRef.value.clientWidth;
  if (maxScrollLeft <= 0) return;

  const nextScrollLeft = Math.max(0, Math.min(maxScrollLeft, bodyScrollRef.value.scrollLeft + horizontalDelta));
  bodyScrollRef.value.scrollLeft = nextScrollLeft;
  syncXFromBody();

  if (event.cancelable) event.preventDefault();
}

function syncXFromBody() {
  const left = bodyScrollRef.value?.scrollLeft || 0;
  if (headerScrollRef.value) headerScrollRef.value.scrollLeft = left;
  if (bottomScrollRef.value) bottomScrollRef.value.scrollLeft = left;
}

function syncXFromBottom() {
  const left = bottomScrollRef.value?.scrollLeft || 0;
  if (bodyScrollRef.value) bodyScrollRef.value.scrollLeft = left;
  if (headerScrollRef.value) headerScrollRef.value.scrollLeft = left;
}

function syncBottomScrollbar() {
  // No-op: flex columns fill the panel width, no horizontal scrollbar needed
}

function startColumnResize(key, event) {
  event.preventDefault();
  event.stopPropagation();
  if (!(key in colPx)) return;

  // Find the next column for bidirectional resize
  const order = ['name', 'modified', 'size', 'permissions', 'ownerGroup'];
  const idx = order.indexOf(key);
  if (idx < 0 || idx >= order.length - 1) return;
  const nextKey = order[idx + 1];

  const startX = event.clientX;
  const startA = colPx[key];
  const startB = colPx[nextKey];
  resizingColumnKey.value = key;

  const onMove = (e) => {
    const d = e.clientX - startX;
    colPx[key] = Math.max(40, startA + d);
    colPx[nextKey] = Math.max(40, startB - d);
  };
  const onUp = () => {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    document.body.style.cursor = '';
    resizingColumnKey.value = '';
  };

  document.body.style.cursor = 'col-resize';
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}

function rowClass(record) {
  if (record.__parent) return 'fm-row fm-row-parent';
  if (record.__draftFolder) return 'fm-row fm-row-draft';
  if (!connected.value) return 'fm-row fm-row-disabled';
  if (selection.isSelected(record)) return 'fm-row fm-row-selected';
  return 'fm-row';
}

function handleRowClick(record, index, event) {
  if (record.__draftFolder) return;
  if (record.__parent) {
    navigateTo('..');
    return;
  }
  selection.handleRowSelect(record, index, event);
  cachePanelState();
}

async function handleRowDoubleClick(record) {
  if (navigatingDir.value) return;
  if (record.__draftFolder) return;
  if (record.__parent) {
    await navigateTo('..');
    return;
  }
  if (record.is_dir) {
    await navigateTo(record.name);
    return;
  }
  await openEditor(record);
}

function openContextMenu(event, record) {
  if (record && !record.__parent && !selection.isSelected(record)) {
    const index = listItems.value.findIndex(item => item.name === record.name);
    if (index >= 0) selection.selectSingle(record, index);
  }
  ctxRecord.value = record || null;
}

function onPanelContextMenu(event) {
  ctxRecord.value = null;
}

function closeContextMenu() {
  ctxRecord.value = null;
}

async function navigateTo(segment) {
  if (navigatingDir.value) return;
  cancelCreateFolderDraft();
  let nextPath = currentPath.value;
  const prevPath = currentPath.value;
  if (segment === '..') {
    if (nextPath === '/') return;
    const parts = nextPath.replace(/\/$/, '').split('/');
    parts.pop();
    nextPath = parts.join('/') || '/';
  } else {
    nextPath = joinRemotePath(nextPath, segment);
  }
  navigatingDir.value = true;
  try {
    currentPath.value = nextPath;
    selection.clearSelection();
    const loaded = await loadFirstPage();
    if (!loaded) throw new Error(netError.value || '目录加载失败');
    cachePanelState();
  } catch (err) {
    currentPath.value = prevPath;
    toast.error(`进入目录失败：${String(err)}`);
  } finally {
    navigatingDir.value = false;
  }
}

function normalizeTerminalCwd(cwd) {
  const normalized = String(cwd || '').trim();
  return normalized.startsWith('/') ? normalized : '';
}

function normalizeSftpPath(path) {
  const normalized = String(path || '').trim();
  return normalized.startsWith('/') ? normalized : '/';
}

async function followCurrentTerminalPath({ notifyIfMissing = false } = {}) {
  const cwd = normalizeTerminalCwd(followedTerminalSession.value?.cwd);
  if (!cwd) {
    if (notifyIfMissing) toast.info('暂未获取到终端当前路径，收到路径后将自动跟随');
    return;
  }

  lastObservedTerminalCwd.value = cwd;
  if (cwd === currentPath.value || navigatingDir.value) return;

  const previousPath = currentPath.value;
  navigatingDir.value = true;
  try {
    currentPath.value = cwd;
    selection.clearSelection();
    const loaded = await loadFirstPage();
    if (!loaded) {
      currentPath.value = previousPath;
      toast.error(`跟随终端路径失败：${netError.value || cwd}`);
    } else {
      cachePanelState();
    }
  } finally {
    navigatingDir.value = false;
  }
}

async function toggleFollowTerminalPath() {
  followTerminalPath.value = !followTerminalPath.value;
  cachePanelState();
  if (followTerminalPath.value) {
    await followCurrentTerminalPath({ notifyIfMissing: true });
  }
}

function goUp() {
  navigateTo('..');
}

async function retryConnection() {
  await initSftp();
}

async function manualRefresh() {
  if (!props.sessionId) return;
  cancelCreateFolderDraft();

  const backendConnected = await queryBackendSftpConnected(props.sessionId);
  if (!backendConnected || !sshStore.isSftpConnected(props.sessionId) || !connected.value) {
    await initSftp(true);
    if (!connected.value) return;
  }

  await loadFirstPage();
}

async function runBatch(action, targets, worker) {
  if (!targets.length) return;
  batchState.running = true;
  batchState.action = action;
  batchState.total = targets.length;
  batchState.success = 0;
  batchState.failed = 0;

  for (const item of targets) {
    try {
      await worker(item);
      batchState.success += 1;
    } catch {
      batchState.failed += 1;
    }
  }

  toast.info(`${action} 完成：成功 ${batchState.success}，失败 ${batchState.failed}`);
  batchState.running = false;
  await loadFirstPage();
}

async function handleUpload() {
  const sessionId = props.sessionId;
  const uploadDirectory = currentPath.value;
  if (!sessionId) return;
  const ready = await ensureSftpReady();
  if (!ready) {
    toast.error('SFTP 会话不可用，请重试连接');
    return;
  }
  const selected = await open({ multiple: true, directory: false });
  if (!selected) return;
  const filePaths = Array.isArray(selected) ? selected : [selected];
  const files = filePaths.map(item => (typeof item === 'string' ? item : item?.path)).filter(Boolean);
  if (!files.length) return;

  const tasks = [];
  for (const path of files) {
    const name = path.split(/[/\\]/).pop() || 'unknown';
    const remotePath = joinRemotePath(uploadDirectory, name);

    let shouldUpload = true;
    try {
      const exists = await invokeCommand('sftp_exists', {
        sessionId,
        path: remotePath
      });
      if (exists) {
        shouldUpload = await confirmOverwrite(remotePath);
      }
    } catch (err) {
      console.warn('[SFTP] upload existence check failed:', err);
    }

    if (!shouldUpload) continue;
    tasks.push(createUploadTask(sessionId, path, remotePath));
  }

  if (!tasks.length) {
    toast.info('已取消上传');
    return;
  }

  await runUploadTasksWithConcurrency(tasks);
  notifyTransferSummary('上传', tasks);
  if (props.sessionId === sessionId) {
    await ensureSftpReady();
    await loadFirstPage();
  }
}

async function handleDownload(records = selectedRecords.value) {
  const sessionId = props.sessionId;
  const downloadDirectory = currentPath.value;
  if (!sessionId || !connected.value) return;
  const targets = records.filter(item => !item.is_dir);
  if (!targets.length) {
    toast.warning('请选择文件');
    return;
  }

  if (targets.length === 1) {
    const output = await save({ defaultPath: targets[0].name });
    if (!output) return;
    const targetPath = output.path || output;
    const task = createDownloadTask(
      sessionId,
      joinRemotePath(downloadDirectory, targets[0].name),
      targetPath,
      targets[0].name
    );
    await runDownloadTasksWithConcurrency([task]);
    if (task.status === 'success') {
      toast.success('下载成功');
    }
    return;
  }

  const folder = await open({ directory: true, multiple: false });
  if (!folder) return;
  const base = typeof folder === 'string' ? folder : folder.path;

  const tasks = targets.map((item) => createDownloadTask(
    sessionId,
    joinRemotePath(downloadDirectory, item.name),
    `${base}\\${item.name}`,
    item.name
  ));
  await runDownloadTasksWithConcurrency(tasks);
  notifyTransferSummary('下载', tasks);
}

async function batchDelete() {
  const targets = selectedRecords.value;
  if (!targets.length) {
    toast.warning('请先选择要删除的文件/目录');
    return;
  }

  confirm({
    title: '确认删除',
    content: `将删除 ${targets.length} 个项目（目录需为空），是否继续？`,
    okText: '删除',
    okType: 'danger',
    cancelText: '取消',
    async onOk() {
      await runBatch('删除', targets, async (item) => {
        await invokeCommand('sftp_remove', {
          sessionId: props.sessionId,
          path: joinRemotePath(currentPath.value, item.name),
          isDir: !!item.is_dir
        });
      });
      selection.clearSelection();
    }
  });
}

async function batchRename() {
  const targets = selectedRecords.value;
  if (!targets.length) {
    toast.warning('请先选择项目');
    return;
  }

  const template = window.prompt('输入重命名模板，支持 {name} 和 {index}，例：bak_{index}_{name}', '{name}');
  if (!template) return;

  await runBatch('重命名', targets, async (item) => {
    const i = targets.findIndex(it => it.name === item.name);
    const nextName = template.replaceAll('{name}', item.name).replaceAll('{index}', String(i + 1));
    if (!nextName || nextName === item.name) return;
    await invokeCommand('sftp_rename', {
      sessionId: props.sessionId,
      fromPath: joinRemotePath(currentPath.value, item.name),
      toPath: joinRemotePath(currentPath.value, nextName)
    });
  });
}

async function batchChmod() {
  const targets = selectedRecords.value;
  if (!targets.length) {
    toast.warning('请先选择项目');
    return;
  }

  const modeInput = window.prompt('输入权限（八进制），例如 755 或 644', '644');
  if (!modeInput) return;
  const mode = Number.parseInt(modeInput, 8);
  if (Number.isNaN(mode)) {
    toast.error('权限格式错误');
    return;
  }

  await runBatch('权限修改', targets, async (item) => {
    await invokeCommand('sftp_chmod', {
      sessionId: props.sessionId,
      path: joinRemotePath(currentPath.value, item.name),
      permissions: mode
    });
  });
}

async function showProperties(record) {
  const target = record || ctxRecord.value;
  if (!target) return;
  const stat = await invokeCommand('sftp_stat', {
    sessionId: props.sessionId,
    path: joinRemotePath(currentPath.value, target.name)
  });

  confirm({
    title: `属性 - ${target.name}`,
    content: [
      target.is_dir ? '目录' : '文件',
      `大小: ${formatEntrySize(stat)}`,
      `修改时间: ${localTime(stat)}`,
      `权限: ${permissionText(stat)}`,
      `所属用户: ${stat.owner || '-'}`,
      `所属组: ${stat.group || '-'}`,
    ].join('\n'),
    okText: '关闭',
    cancelText: '',
  });
}

async function openEditor(record) {
  if (!record || record.is_dir) return;

  const blockedReason = getEditBlockedReason(record.name, record.size);
  if (blockedReason) {
    toast.warning(blockedReason);
    return;
  }

  const ready = await ensureSftpReady();
  if (!ready) {
    toast.error('SFTP 会话不可用，无法打开文件编辑器');
    return;
  }

  const requestId = editorOpenRequestId.value + 1;
  editorOpenRequestId.value = requestId;
  editorFilePath.value = joinRemotePath(currentPath.value, record.name);
  editorLanguage.value = getLanguageFromExt(record.name);
  editorReadonly.value = !hasWritePermission(record.permissions);
  editorReadFallback.value = false;
  editorVisible.value = true;
  editorDirty.value = false;
  editorContent.value = '';
  editorOriginalContent.value = '';
  editorCasToken.value = '';
  editorLoading.value = true;
  editorCursor.line = 1;
  editorCursor.column = 1;

  editorMeta.size = record.size || 0;
  editorMeta.modified = record.modified || 0;
  editorMeta.permissions = record.permissions || 0;
  editorMeta.owner = record.owner || '-';
  editorMeta.group = record.group || '-';

  try {
    const result = await invokeCommand('sftp_open_text_file', {
      sessionId: props.sessionId,
      path: editorFilePath.value
    });
    if (requestId !== editorOpenRequestId.value) return;
    const file = result?.file || {};
    editorContent.value = result?.content || '';
    editorOriginalContent.value = editorContent.value;
    editorDirty.value = false;
    editorCasToken.value = String(result?.cas_token || '');
    editorMeta.size = file?.size || 0;
    editorMeta.modified = file?.modified || 0;
    editorMeta.permissions = file?.permissions || 0;
    editorMeta.owner = file?.owner || '-';
    editorMeta.group = file?.group || '-';
    editorReadonly.value = !hasWritePermission(file?.permissions || record.permissions);
    patchPagerItem(file);
  } catch (err) {
    if (requestId !== editorOpenRequestId.value) return;
    const text = String(err || '');
    toast.error(`读取文件失败：${text}`);
    closeEditorState();
  } finally {
    if (requestId === editorOpenRequestId.value) {
      editorLoading.value = false;
    }
  }
}

function restoreEditorContent() {
  if (editorSaving.value || editorReadonly.value) return;
  editorRef.value?.setValue?.(editorOriginalContent.value, { clean: true });
  editorContent.value = editorOriginalContent.value;
  editorDirty.value = false;
  toast.info('已还原到上次保存内容');
}

function handleEditorDirtyChange(nextDirty) {
  editorDirty.value = !!nextDirty;
}

function handleEditorCursorChange(position) {
  editorCursor.line = Number(position?.line || 1);
  editorCursor.column = Number(position?.column || 1);
}

function handleEditorReady() {
  editorRef.value?.resize?.();
  editorRef.value?.focus?.();
}

async function confirmEmptyEditorSave() {
  try {
    await confirm({
      title: '保存为空文件？',
      content: '当前操作会把一个非空远程文件保存为空内容。此操作会覆盖服务器上的文件，请确认这是你想要的结果。',
      okText: '保存为空',
      cancelText: '取消',
      danger: true,
      zIndex: 2300
    });
    return true;
  } catch {
    return false;
  }
}

async function saveEditor(contentOverride) {
  if (!editorFilePath.value) return;
  if (editorLoading.value) {
    toast.warning('文件仍在读取中，请稍候...');
    return;
  }
  const blockedReason = getEditBlockedReason(editorFileName.value, editorMeta.size);
  if (blockedReason) {
    toast.warning(blockedReason);
    return;
  }
  if (editorReadonly.value) {
    toast.warning('当前文件为只读，无法保存');
    return;
  }
  if (editorSaving.value) return;

  const content = typeof contentOverride === 'string'
    ? contentOverride
    : (editorRef.value?.getValue?.() ?? editorContent.value);

  if (content === editorOriginalContent.value) {
    editorDirty.value = false;
    editorRef.value?.markClean?.(content);
    toast.info('内容未变化，无需保存');
    return;
  }

  if (content.length === 0 && editorOriginalContent.value.length > 0) {
    const confirmEmpty = await confirmEmptyEditorSave();
    if (!confirmEmpty) return;
  }

  const ready = await ensureSftpReady();
  if (!ready) {
    toast.error('SFTP 会话不可用，无法保存文件');
    return;
  }

  editorSaving.value = true;
  const toastKey = `editor-save-${editorFilePath.value}`;
  toast.loading({ content: '保存中...', key: toastKey, duration: 0 });
  try {
    const result = await invokeCommand('sftp_save_text_file', {
      sessionId: props.sessionId,
      path: editorFilePath.value,
      content,
      expectedCasToken: editorCasToken.value
    });

    const file = result?.file || {};
    editorContent.value = content;
    editorOriginalContent.value = content;
    editorDirty.value = false;
    editorRef.value?.markClean?.(content);
    editorReadFallback.value = false;
    editorCasToken.value = String(result?.cas_token || '');
    editorMeta.size = file?.size || content.length;
    editorMeta.modified = file?.modified || 0;
    editorMeta.permissions = file?.permissions || editorMeta.permissions;
    editorMeta.owner = file?.owner || editorMeta.owner;
    editorMeta.group = file?.group || editorMeta.group;
    editorReadonly.value = !hasWritePermission(editorMeta.permissions);
    patchPagerItem(file);
    toast.success({ content: '保存成功', key: toastKey });
  } catch (err) {
    toast.error({ content: `保存失败：${String(err)}`, key: toastKey, duration: 4 });
  } finally {
    editorSaving.value = false;
  }
}

function onEditorKeydown(event) {
  if (!editorVisible.value) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    handleEditorModalToggle(false);
    return;
  }
  const hitSave = (event.ctrlKey || event.metaKey) && String(event.key).toLowerCase() === 's';
  if (!hitSave) return;
  event.preventDefault();
  saveEditor();
}

function handleEditorModalToggle(nextOpen) {
  if (nextOpen) {
    editorVisible.value = true;
    return;
  }

  if (editorSaving.value) {
    toast.warning('正在保存中，请稍候...');
    return;
  }

  if (editorDirty.value) {
    editorCloseConfirmVisible.value = true;
    return;
  }

  closeEditorState();
}

function handleEditorModalCancel(event) {
  event?.preventDefault?.();
  handleEditorModalToggle(false);
}

function cancelEditorCloseConfirm() {
  editorCloseConfirmVisible.value = false;
}

function confirmEditorClose() {
  closeEditorState();
}

function onDragStart(record, index) {
  if (record.__parent || record.__draftFolder) return;
  dragState.draggingName = record.name;
  dragState.draggingIndex = index;
}

async function onDropRow(record, targetIndex) {
  if (!dragState.draggingName) return;
  const fromName = dragState.draggingName;
  const fromItem = pager.items.value.find(item => item.name === fromName);
  dragState.draggingName = '';
  dragState.draggingIndex = -1;
  if (!fromItem || fromName === record.name) return;

  if (record.is_dir) {
    await invokeCommand('sftp_rename', {
      sessionId: props.sessionId,
      fromPath: joinRemotePath(currentPath.value, fromName),
      toPath: joinRemotePath(joinRemotePath(currentPath.value, record.name), fromName)
    });
    toast.success(`已移动到 ${record.name}`);
    await loadFirstPage();
    return;
  }

  const source = pager.items.value.findIndex(item => item.name === fromName);
  const target = targetIndex
    - (currentPath.value === '/' ? 0 : 1)
    - (draftFolderVisible.value ? 1 : 0);
  if (source < 0 || target < 0 || source === target) return;
  const local = [...pager.items.value];
  const moved = local.splice(source, 1)[0];
  local.splice(Math.min(target, local.length), 0, moved);
  pager.items.value = local;
  toast.info('已调整当前视图顺序（仅本次会话）');
}

function onDropBlank() {
  dragState.draggingName = '';
  dragState.draggingIndex = -1;
}

function runContextAction(action, record) {
  if (isContextActionDisabled(action, record)) return;
  if (action === 'edit' && record) openEditor(record);
  if (action === 'download' && record) handleDownload([record]);
  if (action === 'upload') handleUpload();
  if (action === 'mkdir') createFolder();
  if (action === 'refresh') manualRefresh();
  if (action === 'prop' && record) showProperties(record);
}

async function disconnectSftpSession(sessionId) {
  if (!sessionId) return;
  try {
    await invokeCommand('sftp_disconnect', { sessionId });
  } catch {
  }
  sshStore.markSftpDisconnected(sessionId);
}

async function handleClosePanel() {
  emit('close');
}

watch(() => props.sessionId, async (nextSessionId, prevSessionId) => {
  const generation = ++sessionSyncGeneration;
  if (prevSessionId && prevSessionId !== nextSessionId) {
    cachePanelStateForSession(prevSessionId);
  }
  cancelCreateFolderDraft();
  closeContextMenu();
  if (editorVisible.value) {
    closeEditorState();
  }
  selection.clearSelection();
  connected.value = false;
  const nextState = panelStateCache.get(nextSessionId);
  currentPath.value = normalizeSftpPath(nextState?.path);
  followTerminalPath.value = !!nextState?.followTerminalPath;
  lastObservedTerminalCwd.value = nextState?.lastObservedTerminalCwd || '';
  if (!props.visible) {
    netError.value = '';
    return;
  }
  await syncVisibleSessionState({ sessionId: nextSessionId, generation });
  if (!isCurrentSessionSync(nextSessionId, generation)) return;
  if (followTerminalPath.value) await followCurrentTerminalPath();
}, { immediate: true });

watch(() => props.visible, async (visible) => {
  const sessionId = props.sessionId;
  if (!sessionId) return;
  if (!visible) {
    cachePanelStateForSession(sessionId);
    ++sessionSyncGeneration;
    return;
  }
  const generation = ++sessionSyncGeneration;
  await syncVisibleSessionState({ sessionId, generation });
  if (isCurrentSessionSync(sessionId, generation) && followTerminalPath.value) {
    await followCurrentTerminalPath();
  }
});

watch(() => props.followSessionId, () => {
  lastObservedTerminalCwd.value = normalizeTerminalCwd(followedTerminalSession.value?.cwd);
});

watch(() => followedTerminalSession.value?.cwd, async (cwd, previousCwd) => {
  const normalized = normalizeTerminalCwd(cwd);
  if (!followTerminalPath.value || !normalized || normalized === normalizeTerminalCwd(previousCwd)) return;
  if (normalized === lastObservedTerminalCwd.value) return;
  await followCurrentTerminalPath();
});

watch(activeSessionStatus, async (status, prevStatus) => {
  if (!props.sessionId) return;

  if (status !== 'connected') {
    connected.value = false;
    netError.value = status === 'connecting' ? '等待终端 SSH 会话建立...' : '终端会话未连接';
    if (sshStore.isSftpConnected(props.sessionId)) {
      await disconnectSftpSession(props.sessionId);
    }
    return;
  }

  if (prevStatus !== 'connected') {
    await syncVisibleSessionState({ restore: false });
  }
});

watch([() => pager.items.value.length, () => connected.value], () => nextTick(syncBottomScrollbar));

let resizeHandler = null;
let resizeFrame = null;
let scheduleResizeHandler = null;
let activeSftpRefreshHandler = null;
let sftpLayoutRefreshHandler = null;
onMounted(() => {
  resizeHandler = () => {
    const height = viewportRef.value?.clientHeight || 400;
    listVirtual.setViewportHeight(height);
  };
  scheduleResizeHandler = () => {
    if (resizeFrame) return;
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = null;
      resizeHandler?.();
    });
  };
  resizeHandler();
  activeSftpRefreshHandler = (event) => {
    const targetSessionId = event?.detail?.sessionId;
    if (!targetSessionId || targetSessionId !== props.sessionId) return;
    manualRefresh();
  };
  sftpLayoutRefreshHandler = () => nextTick(() => {
    resizeHandler?.();
    syncBottomScrollbar();
    recalcColumnWidths();
  });
  window.addEventListener('resize', scheduleResizeHandler);
  window.addEventListener('app:sftp-refresh-active', activeSftpRefreshHandler);
  window.addEventListener('app:sftp-layout-refresh', sftpLayoutRefreshHandler);
  window.addEventListener('keydown', onEditorKeydown);

  // Track container width for auto-fit column widths
  if (viewportRef.value) {
    _colResizeObserver = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect?.width;
      scheduleColumnWidthRecalc(w);
    });
    _colResizeObserver.observe(viewportRef.value);
    recalcColumnWidths();
  }
});

onUnmounted(() => {
  cachePanelState();
  cancelCreateFolderDraft();
  if (scheduleResizeHandler) {
    window.removeEventListener('resize', scheduleResizeHandler);
    scheduleResizeHandler = null;
  }
  if (resizeFrame) {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = null;
  }
  if (activeSftpRefreshHandler) {
    window.removeEventListener('app:sftp-refresh-active', activeSftpRefreshHandler);
    activeSftpRefreshHandler = null;
  }
  if (sftpLayoutRefreshHandler) {
    window.removeEventListener('app:sftp-layout-refresh', sftpLayoutRefreshHandler);
    sftpLayoutRefreshHandler = null;
  }
  window.removeEventListener('keydown', onEditorKeydown);
  if (_colResizeFrame) {
    cancelAnimationFrame(_colResizeFrame);
    _colResizeFrame = null;
  }
  if (_colResizeObserver) { _colResizeObserver.disconnect(); _colResizeObserver = null; }
});
</script>

<template>
  <div class="file-manager" @click="closeContextMenu">
    <div class="fm-address fm-address-row">
      <Input v-model="currentPath" @keyup.enter="manualRefresh" size="sm" class="flex-1" />
      <div class="fm-actions">
        <IconButton :icon="ArrowLeft" size="sm" :disabled="currentPath === '/'" aria-label="返回上级目录"
          :action="goUp" />
        <IconButton :icon="LocateFixed" size="sm" :active="followTerminalPath" aria-label="跟随终端当前路径"
          :action="toggleFollowTerminalPath" />
        <IconButton :icon="RefreshCw" size="sm" :disabled="pager.loading.value" aria-label="刷新"
          :action="manualRefresh" />
        <IconButton :icon="Upload" size="sm" aria-label="上传" :action="handleUpload" />
        <IconButton :icon="Download" size="sm" aria-label="下载" :action="() => handleDownload()" />
        <IconButton :icon="X" size="sm" aria-label="关闭面板" :action="handleClosePanel" />
      </div>
    </div>

    <div class="fm-toolbar" v-if="false">
      <Button v-if="showBatchActions" size="sm" variant="outline" @click="batchDelete"
        :disabled="batchState.running">批量删除</Button>
      <Button v-if="showBatchActions" size="sm" variant="outline" @click="batchRename"
        :disabled="batchState.running">批量重命名</Button>
      <Button v-if="showBatchActions" size="sm" variant="outline" @click="batchChmod"
        :disabled="batchState.running">批量权限</Button>
      <Button v-if="showBatchActions" size="sm" variant="outline" @click="createFolder"
        :disabled="batchState.running">新建文件夹</Button>
      <span class="fm-stat" v-if="batchState.running">
        {{ batchState.action }}中 {{ batchState.success + batchState.failed }}/{{ batchState.total }}
      </span>
      <span class="fm-stat" v-else>
        {{ pager.totalKnown.value ? `共 ${pager.items.value.length}/${pager.total.value} 条` : `已加载 ${pager.items.value.length} 条` }}
      </span>
    </div>

    <div class="fm-content" ref="viewportRef" @contextmenu="onPanelContextMenu">
      <div v-if="!connected" class="fm-state">
        <div>{{ initializing ? '正在连接 SFTP...' : (netError || '暂无连接') }}</div>
        <!-- <a-button size="small" @click="retryConnection" :loading="initializing">重试</a-button> -->
      </div>

      <div v-else class="fm-table">
        <div class="fm-table-header" ref="headerScrollRef">
          <div class="fm-grid"
            :style="{ width: (colPx.select + colPx.name + colPx.modified + colPx.size + colPx.permissions + colPx.ownerGroup) + 'px' }">
            <div class="fm-cell fm-head" :class="{ 'fm-cell-hidden': !showSelectionColumn }"
              :style="{ width: colPx.select + 'px' }"></div>
            <div class="fm-cell fm-head fm-head-resizable" :style="{ width: colPx.name + 'px' }">
              文件名
              <span class="fm-col-resizer" :class="{ 'is-active': resizingColumnKey === 'name' }"
                @mousedown="(event) => startColumnResize('name', event)"></span>
            </div>
            <div class="fm-cell fm-head fm-head-resizable" :style="{ width: colPx.modified + 'px' }">
              修改时间
              <span class="fm-col-resizer" :class="{ 'is-active': resizingColumnKey === 'modified' }"
                @mousedown="(event) => startColumnResize('modified', event)"></span>
            </div>
            <div class="fm-cell align-right fm-head-resizable" :style="{ width: colPx.size + 'px' }">
              大小
              <span class="fm-col-resizer" :class="{ 'is-active': resizingColumnKey === 'size' }"
                @mousedown="(event) => startColumnResize('size', event)"></span>
            </div>
            <div class="fm-cell fm-head fm-head-resizable" :style="{ width: colPx.permissions + 'px' }">
              权限
              <span class="fm-col-resizer" :class="{ 'is-active': resizingColumnKey === 'permissions' }"
                @mousedown="(event) => startColumnResize('permissions', event)"></span>
            </div>
            <div class="fm-cell fm-head" :style="{ width: colPx.ownerGroup + 'px' }">
              用户/组
            </div>
          </div>
        </div>

        <div class="fm-table-body" ref="bodyScrollRef" @scroll="onBodyScroll" @wheel="onTableWheel" @dragover.prevent
          @drop.prevent="onDropBlank">
          <div v-if="navigatingDir" class="fm-nav-loading-mask">
            <LoadingSpinner tip="正在加载目录..." />
          </div>
          <div class="fm-body-inner"
            :style="{ width: (colPx.select + colPx.name + colPx.modified + colPx.size + colPx.permissions + colPx.ownerGroup) + 'px', height: listVirtual.totalHeight.value + 'px' }">
            <div class="fm-virtual" :style="{ transform: `translateY(${listVirtual.translateY.value}px)` }">
              <ContextMenu v-for="row in visibleRows" :key="row.item.__draftFolder ? '__draft-folder-row' : (row.item.name + '-' + row.index)">
                <ContextMenuTrigger as-child>
                  <div :class="rowClass(row.item)" :draggable="!row.item.__parent && !row.item.__draftFolder"
                    @dragstart="onDragStart(row.item, row.index)"
                    @dragover.prevent @drop.prevent="onDropRow(row.item, row.index)"
                    @click="(e) => handleRowClick(row.item, row.index, e)"
                    @dblclick="() => handleRowDoubleClick(row.item)">
                    <div class="fm-cell" :class="{ 'fm-cell-hidden': !showSelectionColumn }"
                      :style="{ width: colPx.select + 'px' }">
                      <input v-if="showSelectionColumn" type="checkbox" :checked="selection.isSelected(row.item)"
                        :disabled="row.item.__parent" @click.stop
                        @change="(e) => handleRowClick(row.item, row.index, e)" />
                    </div>
                    <div class="fm-cell" :style="{ width: colPx.name + 'px' }">
                      <div class="fm-file-name">
                        <FileIcon :name="row.item.name" :path="joinRemotePath(currentPath, row.item.name)"
                          :is-directory="!!row.item.is_dir" :expanded="false" :size="16" />
                        <template v-if="row.item.__draftFolder">
                          <input :ref="setDraftFolderInputRef" v-model="draftFolderName" class="fm-inline-create-input"
                            type="text" spellcheck="false" maxlength="255" aria-label="新建文件夹名称"
                            @click.stop @keydown="onDraftFolderKeydown"
                            @blur="() => submitCreateFolderDraft({ cancelIfEmpty: true })" />
                        </template>
                        <span v-else class="fm-file-label">{{ row.item.__parent ? '..' : row.item.name }}</span>
                      </div>
                    </div>
                    <div class="fm-cell" :style="{ width: colPx.modified + 'px' }">
                      {{ row.item.__draftFolder ? '' : localTime(row.item) }}
                    </div>
                    <div class="fm-cell align-right" :style="{ width: colPx.size + 'px' }">
                      {{ row.item.__draftFolder ? '' : formatEntrySize(row.item) }}
                    </div>
                    <div class="fm-cell fm-mono" :style="{ width: colPx.permissions + 'px' }">
                      {{ row.item.__draftFolder ? '' : permissionText(row.item) }}
                    </div>
                    <div class="fm-cell" :style="{ width: colPx.ownerGroup + 'px' }">
                      {{ row.item.__draftFolder ? '' : ((row.item.owner || '-') + '/' + (row.item.group || '-')) }}
                    </div>
                  </div>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  <ContextMenuItem :disabled="isContextActionDisabled('edit', row.item)"
                    @select="runContextAction('edit', row.item)">编辑</ContextMenuItem>
                  <ContextMenuItem :disabled="isContextActionDisabled('download', row.item)"
                    @select="runContextAction('download', row.item)">下载</ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem :disabled="isContextActionDisabled('upload', row.item)"
                    @select="runContextAction('upload', row.item)">上传</ContextMenuItem>
                  <ContextMenuItem :disabled="isContextActionDisabled('mkdir', row.item)"
                    @select="runContextAction('mkdir', row.item)">新建文件夹</ContextMenuItem>
                  <ContextMenuItem :disabled="isContextActionDisabled('refresh', row.item)"
                    @select="runContextAction('refresh', row.item)">刷新</ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem :disabled="isContextActionDisabled('prop', row.item)"
                    @select="runContextAction('prop', row.item)">属性</ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
            </div>
          </div>
          <div class="fm-load-more" v-if="pager.loadingMore.value">加载更多中...</div>
        </div>
      </div>
    </div>

    <div class="fm-bottom-scrollbar" ref="bottomScrollRef" @scroll="syncXFromBottom" style="display:none">
      <div class="fm-bottom-scrollbar-inner"></div>
    </div>


    <Dialog :open="editorVisible" @update:open="(v) => { if (!v) handleEditorModalCancel(); }">
      <DialogContent :show-close-button="false"
        class="sftp-editor-dialog !max-w-[96vw] !w-[min(1120px,96vw)] !p-0 !bg-[var(--app-bg-dialog)]">
        <DialogHeader class="sr-only">
          <DialogTitle>{{ editorFilePath || editorFileName || '远程文件编辑' }}
          </DialogTitle>
        </DialogHeader>
        <div class="editor-shell">
          <div class="editor-toolbar">
            <div class="editor-title-block">
              <div class="editor-modal-name" :title="editorFileName || editorFilePath || '远程文件编辑'">
                {{ editorFileName || '远程文件编辑' }}
              </div>
              <div class="editor-modal-path" :title="editorFilePath">{{ editorFilePath || '-' }}</div>
            </div>
            <div class="editor-toolbar-meta">
              <span class="editor-chip" v-if="editorReadonly">只读</span>
              <span class="editor-chip editor-chip-warning" v-if="editorDirty">未保存</span>
              <span class="editor-chip editor-chip-muted" v-if="editorSaving">保存中</span>
              <span class="editor-chip editor-chip-muted" v-if="editorLargeFile">轻量模式</span>
              <span class="editor-chip editor-chip-muted" v-if="editorReadFallback">保护模式</span>
            </div>
            <div class="editor-toolbar-actions">
              <Button variant="ghost" class="editor-action editor-action-subtle"
                :disabled="!editorDirty || editorSaving || editorReadonly || editorLoading"
                @click="restoreEditorContent">
                <RotateCcw :size="14" />
                还原
              </Button>
              <Button class="editor-action editor-action-primary" :disabled="!editorCanSave" @click="() => saveEditor()">
                <SaveIcon :size="14" />
                保存
              </Button>
            </div>
            <div class="editor-toolbar-close">
              <IconButton :icon="X" size="sm" aria-label="关闭编辑弹窗" class="editor-title-close"
                :action="handleEditorModalCancel" />
            </div>
          </div>
          <div v-if="editorLoading" class="big-file-loading">正在读取文件...</div>
          <div v-else class="editor-surface">
            <CodeEditor ref="editorRef" :model-value="editorContent" :language="editorLanguage"
              :readonly="editorReadonly" :large-file="editorLargeFile" class="editor-ace"
              @dirty-change="handleEditorDirtyChange" @cursor-change="handleEditorCursorChange"
              @ready="handleEditorReady" @save="saveEditor" />
          </div>
          <div class="editor-statusbar">
            <span class="editor-status-item">大小 {{ formatSize(editorMeta.size || 0) }}</span>
            <span class="editor-status-item">修改 {{ localTime(editorMeta) }}</span>
            <span class="editor-status-item">权限 {{ permissionText(editorMeta) }}</span>
            <span class="editor-status-item">用户 {{ editorMeta.owner || '-' }}/{{ editorMeta.group || '-' }}</span>
            <span class="editor-status-item">语言 {{ editorLargeFile ? 'plaintext' : editorLanguage }}</span>
            <span class="editor-status-item">行 {{ editorCursor.line }}，列 {{ editorCursor.column }}</span>
            <span class="editor-status-item" v-if="editorReadonly">只读</span>
            <span class="editor-status-item" v-if="editorLargeFile">轻量渲染</span>
          </div>
        </div>
      </DialogContent>
    </Dialog>

    <Dialog :open="editorCloseConfirmVisible" @update:open="(v) => { if (!v) cancelEditorCloseConfirm(); }">
      <DialogContent class="max-w-md !bg-[var(--app-bg-dialog)]">
        <div class="editor-confirm-shell">
          <div class="editor-confirm-title">关闭未保存编辑？</div>
          <div class="editor-confirm-text">当前文件仍有未保存改动。关闭后，本次编辑内容将丢失。</div>
          <div class="editor-confirm-actions">
            <Button variant="ghost" @click="cancelEditorCloseConfirm">继续编辑</Button>
            <Button variant="destructive" @click="confirmEditorClose">仍然关闭</Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>

<style scoped>
.file-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  min-width: 260px;
  background: var(--terminal-surface-bg, var(--app-bg-dialog));
}

.fm-address,
.fm-toolbar {
  flex-shrink: 0;
  padding: 6px 16px 8px;
}

.fm-address-row {
  display: flex;
  align-items: center;
  gap: 8px;
  /* keep actions fixed to right */
  background: transparent;
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-shadow) 64%, transparent);
}

.fm-address-row :deep(.ant-input),
.fm-address-row :deep(input),
.fm-address-input {
  /* allow the input to grow and match session-list top behavior */
  flex: 1 1 auto;
  width: auto;
  min-width: 140px;
  max-width: none;
  height: 30px;
  border: 1px solid color-mix(in srgb, var(--app-border-shadow) 82%, transparent) !important;
  border-radius: 8px;
  background: color-mix(in srgb, var(--app-input-bg) 72%, transparent) !important;
  box-shadow: inset 0 1px 0 color-mix(in srgb, var(--app-text) 4%, transparent) !important;
  padding: 4px 10px;
  color: var(--app-text);
}

.fm-actions {
  margin-left: auto;
  display: flex;
  gap: 4px;
  align-items: center;
}

.fm-actions {
  display: flex;
  gap: 4px;
}

.fm-actions :deep(.icon-button) {
  --icon-btn-size: 28px;
  --icon-btn-color: color-mix(in srgb, var(--app-text) 70%, transparent);
  --icon-btn-hover-color: var(--app-text);
  --icon-btn-hover-bg: color-mix(in srgb, var(--app-text) 8%, transparent);
  --icon-btn-active-bg: color-mix(in srgb, var(--color-primary) 16%, transparent);
  --icon-btn-active-color: var(--app-text);
  border-radius: 7px;
  box-shadow: none;
}

.fm-actions :deep(.ant-btn) {
  width: 24px;
  min-width: 24px;
  height: 24px;
  padding: 0;
  color: color-mix(in srgb, var(--app-text) 82%, transparent);
  border: 1px solid transparent;
  border-radius: 6px;
  cursor: pointer;
  background: transparent !important;
  box-shadow: none !important;
}

.fm-actions :deep(.ant-btn-default),
.fm-actions :deep(.ant-btn-text) {
  background: transparent !important;
  box-shadow: none !important;
  border: 1px solid transparent !important;
}

.fm-actions :deep(.ant-btn::before) {
  display: none !important;
}

.fm-actions :deep(.ant-btn:hover),
.fm-actions :deep(.ant-btn:focus),
.fm-actions :deep(.ant-btn:active) {
  color: var(--app-text) !important;
  border-color: color-mix(in srgb, var(--app-text) 18%, transparent) !important;
  background: color-mix(in srgb, var(--app-text) 8%, transparent) !important;
  box-shadow: none !important;
}

.fm-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  border-bottom: 1px solid var(--app-border-shadow);
  flex-wrap: wrap;
}

.fm-stat {
  color: var(--app-text);
  opacity: 0.85;
  font-size: 12px;
}

.fm-content {
  flex: 1;
  min-height: 0;
  margin: 0 16px 12px;
  display: flex;
  background: transparent;
}

.fm-state {
  margin: auto;
  min-width: 260px;
  min-height: 92px;
  justify-content: center;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: center;
  border: 1px solid color-mix(in srgb, var(--app-border-light) 62%, transparent);
  border-radius: var(--niri-radius-lg, 14px);
  background: color-mix(in srgb, var(--app-bg-dialog) 30%, transparent);
  color: color-mix(in srgb, var(--app-text) 88%, transparent);
  font-weight: 650;
  backdrop-filter: blur(8px) saturate(110%);
  -webkit-backdrop-filter: blur(8px) saturate(110%);
}

.fm-table {
  width: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.fm-table-header {
  overflow: hidden;
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-shadow) 62%, transparent);
  background: var(--terminal-surface-bg, var(--app-bg-dialog));
}

.fm-grid,
.fm-row {
  display: flex;
  align-items: center;
}

.fm-head {
  color: var(--app-text);
  font-size: 13px;
  font-weight: 500;
  height: 28px;
}

.fm-head-resizable {
  position: relative;
}

.fm-col-resizer {
  position: absolute;
  top: 0;
  right: -3px;
  width: 6px;
  height: 100%;
  cursor: col-resize;
  z-index: 2;
}

.fm-col-resizer::before {
  content: '';
  position: absolute;
  left: 2px;
  top: 6px;
  bottom: 6px;
  width: 1px;
  background: var(--app-border-shadow);
  opacity: 0.9;
}

.fm-head-resizable:hover .fm-col-resizer::before,
.fm-col-resizer.is-active::before {
  background: var(--app-selection-bg);
  width: 2px;
  left: 1px;
  opacity: 1;
}

.fm-table-body {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  background: var(--terminal-surface-bg, var(--app-bg-dialog));
  scrollbar-width: thin;
  scrollbar-color: var(--app-btn-border) transparent;
}

.fm-nav-loading-mask {
  position: absolute;
  inset: 0;
  z-index: 3;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--app-bg-dialog) 34%, transparent);
  backdrop-filter: blur(4px);
  pointer-events: all;
}

.fm-body-inner {
  position: relative;
  width: 100%;
}

.fm-virtual {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
}

.fm-row {
  height: 32px;
  border-bottom: none;
  cursor: default;
  user-select: none;
  color: var(--app-input-text);
  font-size: 13px;
}

.fm-row:hover {
  background: color-mix(in srgb, var(--app-text) 7%, transparent);
  color: var(--app-text);
}

.fm-row-draft {
  background: color-mix(in srgb, var(--app-selection-bg) 68%, transparent);
}

.fm-row-draft:hover {
  background: color-mix(in srgb, var(--app-selection-bg) 78%, transparent);
}

.fm-row-selected {
  background: var(--app-selection-bg);
  color: var(--app-selection-text);
}

.fm-row-selected:hover {
  background: var(--app-selection-bg);
  color: var(--app-selection-text);
}

.fm-row-disabled {
  opacity: .5;
}

.fm-row-parent {
  font-weight: 600;
}

.fm-cell {
  height: 32px;
  display: flex;
  align-items: center;
  padding: 0 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}

.fm-cell-hidden {
  padding: 0;
  min-width: 0;
  overflow: hidden;
}

.fm-row,
.fm-cell {
  transition: none;
}

.fm-mono {
  font-family: var(--font-mono);
}

.align-right {
  justify-content: flex-end;
}

.fm-file-name {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.fm-inline-create-input {
  width: 100%;
  min-width: 0;
  height: 24px;
  border: 1px solid color-mix(in srgb, var(--app-selection-bg) 72%, var(--app-border-shadow));
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-input-bg) 92%, var(--app-bg-dialog));
  color: var(--app-text);
  padding: 0 8px;
  outline: none;
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--app-selection-bg) 18%, transparent);
}

.fm-inline-create-input:focus {
  border-color: color-mix(in srgb, var(--color-primary) 82%, white);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-primary) 18%, transparent);
}

.fm-file-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fm-load-more {
  position: sticky;
  bottom: 0;
  left: 0;
  right: 0;
  text-align: center;
  padding: 6px;
  background: color-mix(in srgb, var(--app-bg-dialog) 42%, transparent);
  color: var(--app-text-muted);
  font-size: 12px;
}

.fm-bottom-scrollbar {
  height: 14px;
  margin: 0 8px 8px;
  overflow-x: auto;
  overflow-y: hidden;
  background: transparent;
  flex-shrink: 0;
  scrollbar-width: thin;
  scrollbar-color: var(--app-btn-border) transparent;
}

.fm-bottom-scrollbar-inner {
  height: 1px;
}

.big-file-loading {
  flex: 1;
  min-height: 360px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--app-text-muted);
  font-size: 14px;
}

.editor-shell {
  height: min(76vh, 760px);
  min-height: min(620px, calc(100vh - 5rem));
  display: flex;
  flex-direction: column;
  gap: 0;
  padding: 0;
  overflow: hidden;
}

.editor-toolbar {
  min-height: 50px;
  padding: 8px 12px 8px 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
  font-family: var(--app-font-family);
  background: color-mix(in srgb, var(--app-bg-dialog) 96%, var(--app-text));
}

.editor-title-block {
  min-width: 0;
  flex: 1 1 280px;
}

.editor-toolbar-meta,
.editor-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.editor-toolbar-meta {
  flex: 0 1 auto;
  min-width: 0;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.editor-toolbar-actions {
  flex: 0 0 auto;
  margin-left: 12px;
  margin-right: 18px;
}

.editor-toolbar-close {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  padding-left: 10px;
  border-left: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
}

.editor-surface {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.editor-ace {
  height: 100%;
}

.editor-ace :deep(.ace_editor) {
  background: var(--app-bg-dialog) !important;
}

.editor-ace :deep(.ace_gutter) {
  background: color-mix(in srgb, var(--app-bg-dialog) 96%, var(--app-text)) !important;
  color: var(--app-text-muted) !important;
}

.editor-ace :deep(.ace_gutter-active-line) {
  background: color-mix(in srgb, var(--app-text) 4%, transparent) !important;
}

.editor-ace :deep(.ace_scroller) {
  background: var(--app-bg-dialog) !important;
}

.editor-ace :deep(.ace_scrollbar-v) {
  background-color: var(--app-bg-dialog) !important;
}

.editor-ace :deep(.ace_scrollbar-h) {
  background-color: var(--app-bg-dialog) !important;
}

.editor-ace :deep(.ace_scroller::-webkit-scrollbar-corner) {
  background: var(--app-bg-dialog) !important;
}

.editor-statusbar {
  min-height: 34px;
  padding: 6px 12px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  border-top: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
  color: var(--app-text-muted);
  font-size: 12px;
  font-family: var(--app-font-family);
  background: color-mix(in srgb, var(--app-bg-dialog) 97%, var(--app-text));
}

.editor-modal-name {
  color: var(--app-text);
  font-size: 15px;
  font-weight: 700;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.editor-modal-path {
  margin-top: 3px;
  color: var(--app-text-muted);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.editor-title-close {
  flex: 0 0 auto;
  --icon-btn-color: var(--app-text-muted);
  --icon-btn-hover-color: var(--app-text);
  --icon-btn-hover-bg: color-mix(in srgb, var(--app-text-muted, #ABB2BF) 11%, transparent);
  background: transparent;
}

.editor-title-close:hover {
  background: color-mix(in srgb, var(--app-text-muted, #ABB2BF) 11%, transparent);
  color: var(--app-text);
}

/* .editor-title-close:focus-visible {
  outline: 1px solid var(--app-selection-bg);
  outline-offset: 2px;
} */

.editor-chip,
.editor-tip,
.editor-meta-item,
.editor-status-item {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 0 8px;
  border-radius: var(--niri-radius-sm, 4px);
  background: color-mix(in srgb, var(--app-text-muted, #ABB2BF) 10%, transparent);
  color: var(--app-text-muted);
  font-size: 11px;
}

.editor-chip {
  color: var(--app-text);
}

.editor-chip-warning {
  background: color-mix(in srgb, var(--color-warning) 18%, transparent);
  color: var(--color-warning);
}

.editor-chip-muted {
  background: color-mix(in srgb, var(--app-text-muted) 18%, transparent);
  color: var(--app-text-muted);
}

.editor-action-subtle {
  border-radius: var(--niri-radius-md, 8px);
}

.editor-action-primary {
  border-radius: var(--niri-radius-md, 8px);
  box-shadow: none;
}

.editor-confirm-danger {
  border-radius: var(--niri-radius-md, 8px);
}

.editor-confirm-shell {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 4px 2px;
  color: var(--app-text);
  font-family: var(--app-font-family);
}

.editor-confirm-title {
  font-size: 16px;
  font-weight: 700;
}

.editor-confirm-text {
  color: var(--app-text-muted);
  font-size: 13px;
  line-height: 1.6;
}

.editor-confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

/* removed local scrollbar rule to allow global scrollbar styling */
</style>

<!-- Portal-rendered dialog overrides (unscoped — must reach teleported shadcn DialogContent) -->
<style>
/* Editor dialog — border & shadow to match app theme */
[data-slot="dialog-content"].sftp-editor-dialog {
  border: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
  box-shadow: var(--niri-shadow-dialog);
}
</style>
