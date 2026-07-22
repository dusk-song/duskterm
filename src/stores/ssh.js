import { toast } from '@/composables/useToast';
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invokeCommand } from '../utils/ipc';

export const useSshStore = defineStore('ssh', () => {
  const TERMINAL_READY_EVENT = 'terminal-ready';
  const TERMINAL_READY_TIMEOUT_MS = 1200;

  const sessions = ref([]); // Active tabs
  const activeSessionId = ref(null);
  const sftpConnectedSessionIds = ref([]);

  const savedSessions = ref([]); // Loaded from storage
  const groups = ref([]); // Loaded from storage
  const groupOrder = ref(loadGroupOrder());
  const groupPrefs = ref(loadGroupPrefs());

  // --- Legacy blacklist ---
  // Default rules now live in the command knowledge base only when users create/import them.
  const defaultBlacklist = [];

  const commandBlacklist = ref(loadBlacklist());

  function loadBlacklist() {
    try {
      const raw = localStorage.getItem('ssh-command-blacklist');
      if (!raw) return [...defaultBlacklist];

      const parsed = JSON.parse(raw);
      // Migration: Convert string[] to object[]
      if (Array.isArray(parsed) && parsed.length > 0 && typeof parsed[0] === 'string') {
        return parsed.map(p => ({ pattern: p, severity: 'warning' })); // Default migrated to warning for safety or matching old behavior
      }
      return parsed;
    } catch (e) {
      return [...defaultBlacklist];
    }
  }

  function loadGroupOrder() {
    try {
      const raw = localStorage.getItem('ssh-group-order');
      const parsed = raw ? JSON.parse(raw) : [];
      return Array.isArray(parsed) ? parsed : [];
    } catch (e) {
      return [];
    }
  }

  function saveGroupOrder(order) {
    groupOrder.value = order;
    try {
      localStorage.setItem('ssh-group-order', JSON.stringify(order));
    } catch (e) {
      console.error('Failed to persist group order:', e);
    }
  }

  function loadGroupPrefs() {
    try {
      const raw = localStorage.getItem('ssh-group-prefs');
      const parsed = raw ? JSON.parse(raw) : {};
      return parsed && typeof parsed === 'object' ? parsed : {};
    } catch (e) {
      return {};
    }
  }

  function saveGroupPrefs(nextPrefs) {
    groupPrefs.value = nextPrefs;
    try {
      localStorage.setItem('ssh-group-prefs', JSON.stringify(nextPrefs));
    } catch (e) {
      console.error('Failed to persist group prefs:', e);
    }
  }

  function getGroupNames() {
    const set = new Set();
    savedSessions.value.forEach(s => {
      const g = (s.group || '').trim();
      if (g) set.add(g);
    });
    return Array.from(set);
  }

  function syncGroupOrder() {
    const existing = getGroupNames();
    const current = groupOrder.value || [];
    const next = current.filter(g => existing.includes(g));
    const missing = existing.filter(g => !next.includes(g)).sort();
    const merged = next.concat(missing);
    if (JSON.stringify(merged) !== JSON.stringify(current)) {
      saveGroupOrder(merged);
    }
  }

  function syncGroupPrefs() {
    const existing = getGroupNames();
    const current = groupPrefs.value || {};
    const next = {};
    existing.forEach(g => {
      next[g] = current[g] || { pinned: false, locked: false };
    });
    if (JSON.stringify(next) !== JSON.stringify(current)) {
      saveGroupPrefs(next);
    }
  }

  function addSession(session) {
    // Ensure cwd property exists
    if (!session.cwd) session.cwd = '';
    sessions.value.push(session);
    if (!session.noActivate) {
      activeSessionId.value = session.id;
    }
  }
  function setSessionStatus(id, status) {
    const session = sessions.value.find((item) => item.id === id);
    if (!session) return;
    session.status = status;
    if (status !== 'connected') {
      markSftpDisconnected(id);
    }
  }

  function updateSessionCwd(id, cwd) {
    const s = sessions.value.find(x => x.id === id);
    if (s) {
      s.cwd = cwd;
    }
  }

  function removeSession(id) {
    const idx = sessions.value.findIndex(s => s.id === id);
    if (idx !== -1) {
      const session = sessions.value[idx];
      if (session.isSplitChild && session.workspaceSessionId) {
        invokeCommand('close_ssh_shell_channel', {
          rootSessionId: session.workspaceSessionId,
          channelId: id
        }).catch(() => { });
      } else {
        invokeCommand('disconnect_ssh', { sessionId: id }).catch(() => { });
      }
      if (activeSessionId.value === id) {
        // Workspace navigation must skip split child runtimes.
        let next = null;
        for (let i = idx + 1; i < sessions.value.length; i += 1) {
          if (!sessions.value[i].isSplitChild) { next = sessions.value[i]; break; }
        }
        if (!next) {
          for (let i = idx - 1; i >= 0; i -= 1) {
            if (!sessions.value[i].isSplitChild) { next = sessions.value[i]; break; }
          }
        }
        activeSessionId.value = next ? next.id : null;
      }
      markSftpDisconnected(id);
      sessions.value.splice(idx, 1);
    }
  }

  function isSftpConnected(sessionId) {
    return (sftpConnectedSessionIds.value || []).includes(sessionId);
  }

  function markSftpConnected(sessionId) {
    if (!sessionId) return;
    if (!isSftpConnected(sessionId)) {
      sftpConnectedSessionIds.value = [...(sftpConnectedSessionIds.value || []), sessionId];
    }
  }

  function markSftpDisconnected(sessionId) {
    if (!sessionId) return;
    sftpConnectedSessionIds.value = (sftpConnectedSessionIds.value || []).filter(id => id !== sessionId);
  }

  function moveSession(dragId, dropId) {
    const dragIdx = sessions.value.findIndex(s => s.id === dragId);
    const dropIdx = sessions.value.findIndex(s => s.id === dropId);

    if (dragIdx > -1 && dropIdx > -1 && dragIdx !== dropIdx) {
      const item = sessions.value.splice(dragIdx, 1)[0];
      // If we removed an item before the drop target, the drop index shifts
      // We want to insert *before* or *after*? 
      // Simplified: insert at the new index.
      // Re-find drop index because array changed
      const newDropIdx = sessions.value.findIndex(s => s.id === dropId);

      // If we are moving forward/backward matters, but usually "insert at drop location" is fine.
      // However, usually detailed drag logic requires knowing "after" or "before".
      // For tabs, if I drop ON a tab, typically I want to place it *before* it, or swap.
      // Let's implement "insert before dropId".

      sessions.value.splice(newDropIdx, 0, item);
    }
  }

  function getSession(id) {
    return sessions.value.find(s => s.id === id);
  }

  // --- Storage Actions ---

  async function loadSavedSessions() {
    try {
      const data = await invokeCommand('load_sessions');
      savedSessions.value = data;
      syncGroupOrder();
      syncGroupPrefs();
    } catch (e) {
      console.error('Failed to load sessions:', e);
      toast.error('加载会话列表失败');
    }
  }

  async function saveSessionToStorage(sessionConfig) {
    try {
      await invokeCommand('save_session', { session: sessionConfig });
      await loadSavedSessions(); // Reload
      toast.success('会话已保存');
      return true;
    } catch (e) {
      console.error('Failed to save session:', e);
      toast.error('保存会话失败');
      return false;
    }
  }

  async function updateSessionGroup(sessionId, groupName) {
    try {
      const config = await invokeCommand('get_decrypted_session', { id: sessionId });
      config.group = groupName && groupName.trim() ? groupName.trim() : '';
      await invokeCommand('save_session', { session: config });
      await loadSavedSessions();
    } catch (e) {
      console.error('Failed to update group:', e);
      toast.error('分组更新失败');
    }
  }

  function setGroupOrder(order) {
    const cleaned = (order || []).filter(Boolean);
    saveGroupOrder(cleaned);
  }

  function setGroupPinned(groupName, pinned) {
    const next = { ...(groupPrefs.value || {}) };
    next[groupName] = { ...(next[groupName] || { pinned: false, locked: false }), pinned: !!pinned };
    saveGroupPrefs(next);
  }

  function setGroupLocked(groupName, locked) {
    const next = { ...(groupPrefs.value || {}) };
    next[groupName] = { ...(next[groupName] || { pinned: false, locked: false }), locked: !!locked };
    saveGroupPrefs(next);
  }

  async function renameGroup(oldName, newName) {
    const trimmed = newName?.trim();
    if (!trimmed || trimmed === oldName) return;
    const targets = savedSessions.value.filter(s => (s.group || '').trim() === oldName);
    try {
      for (const s of targets) {
        const config = await invokeCommand('get_decrypted_session', { id: s.id });
        config.group = trimmed;
        await invokeCommand('save_session', { session: config });
      }
      const reordered = (groupOrder.value || []).map(g => (g === oldName ? trimmed : g));
      saveGroupOrder(reordered);
      const prefs = { ...(groupPrefs.value || {}) };
      if (prefs[oldName]) {
        prefs[trimmed] = prefs[oldName];
        delete prefs[oldName];
        saveGroupPrefs(prefs);
      }
      await loadSavedSessions();
      toast.success('分组已重命名');
    } catch (e) {
      console.error('Failed to rename group:', e);
      toast.error('分组重命名失败');
    }
  }

  async function removeGroup(groupName) {
    const targets = savedSessions.value.filter(s => (s.group || '').trim() === groupName);
    try {
      for (const s of targets) {
        const config = await invokeCommand('get_decrypted_session', { id: s.id });
        config.group = '';
        await invokeCommand('save_session', { session: config });
      }
      const reordered = (groupOrder.value || []).filter(g => g !== groupName);
      saveGroupOrder(reordered);
      const prefs = { ...(groupPrefs.value || {}) };
      if (prefs[groupName]) {
        delete prefs[groupName];
        saveGroupPrefs(prefs);
      }
      await loadSavedSessions();
      toast.success('分组已移除（会话已移到未分组）');
    } catch (e) {
      console.error('Failed to remove group:', e);
      toast.error('分组移除失败');
    }
  }

  async function deleteSessionFromStorage(id) {
    try {
      await invokeCommand('delete_session', { id });
      await loadSavedSessions();
      toast.success('会话已删除');
    } catch (e) {
      console.error(e);
      toast.error('删除失败');
    }
  }

  async function connectStoredSession(id) {
    try {
      // Get decrypted config
      const config = await invokeCommand('get_decrypted_session', { id });
      console.log("Connecting to:", config.host);

      // Update last_connected so recent sessions sort correctly
      config.last_connected = Date.now();
      await invokeCommand('save_session', { session: config });
      await loadSavedSessions();

      // Connect using existing logic
      const connectConfig = { ...config };
      await connectLogic(connectConfig);
    } catch (e) {
      toast.error(`连接失败：${e}`);
    }
  }

  const buildSessionDisplayName = (config = {}) => {
    if (config.name) return config.name;
    const protocol = String(config.protocol || 'ssh').toLowerCase();
    if (protocol === 'serial') {
      return config.serial_path || '串口会话';
    }
    if (protocol === 'telnet') {
      return config.host ? `Telnet ${config.host}` : 'Telnet 会话';
    }
    return config.host ? `${config.username || 'user'}@${config.host}` : 'SSH 会话';
  };

  const waitForTerminalReady = (sessionId, timeoutMs = TERMINAL_READY_TIMEOUT_MS) =>
    new Promise((resolve) => {
      let done = false;
      let timer = null;

      const cleanup = () => {
        window.removeEventListener(TERMINAL_READY_EVENT, onReady);
        if (timer) {
          clearTimeout(timer);
          timer = null;
        }
      };

      const finish = () => {
        if (done) return;
        done = true;
        cleanup();
        resolve();
      };

      const onReady = (event) => {
        if (event?.detail?.sessionId !== sessionId) return;
        finish();
      };

      window.addEventListener(TERMINAL_READY_EVENT, onReady);
      timer = setTimeout(finish, Math.max(200, Number(timeoutMs) || TERMINAL_READY_TIMEOUT_MS));
    });

  const connectSessionById = async (sessionId, config) => {
    await waitForTerminalReady(sessionId);
    await invokeCommand('connect_ssh', { id: sessionId, config });
  };

  async function reconnectSession(sessionId) {
    const session = getSession(sessionId);
    if (!session?.config) {
      toast.warning('未找到可重连的会话配置');
      return false;
    }

    try {
      session.status = 'connecting';
      if (session.isSplitChild && session.workspaceSessionId) {
        const root = getSession(session.workspaceSessionId);
        await invokeCommand('close_ssh_shell_channel', {
          rootSessionId: session.workspaceSessionId,
          channelId: sessionId
        }).catch(() => { });
        await invokeCommand('open_ssh_shell_channel', {
          rootSessionId: session.workspaceSessionId,
          channelId: sessionId,
          termType: root?.config?.term_type || null,
          loginScript: root?.config?.login_script || null
        });
        return true;
      }
      markSftpDisconnected(sessionId);
      await invokeCommand('disconnect_ssh', { sessionId }).catch(() => { });
      await connectSessionById(sessionId, { ...session.config });
      return true;
    } catch (e) {
      session.status = 'disconnected';
      console.error('Failed to reconnect session:', e);
      toast.error(`重连失败：${session.name || sessionId}`);
      return false;
    }
  }

  async function reconnectAllSessions() {
    const candidates = [...sessions.value].filter((session) => session?.config && !session.isSplitChild);
    if (candidates.length === 0) {
      toast.info('当前没有可重连的会话');
      return 0;
    }

    let successCount = 0;
    for (const session of candidates) {
      const ok = await reconnectSession(session.id);
      if (ok) successCount += 1;
    }

    if (successCount > 0) {
      toast.success(`已重连 ${successCount}/${candidates.length} 个会话`);
    }
    return successCount;
  }

  async function connectLogic(config) {
    const sessionId = crypto.randomUUID();

    // 1. Add session PRE-CONNECT to ensure UI is ready
    addSession({
      id: sessionId,
      name: buildSessionDisplayName(config),
      status: 'connecting',
      config: config
    });

    try {
      await connectSessionById(sessionId, config);
    } catch (err) {
      removeSession(sessionId);
      console.error(err);
      toast.error('连接请求失败');
    }
  }

  // Connect logic with metadata (e.g., split child sessions)
  async function connectLogicWithMeta(config, meta = {}) {
    const sessionId = crypto.randomUUID();
    const independentConfig = JSON.parse(JSON.stringify(config || {}));

    addSession({
      id: sessionId,
      name: buildSessionDisplayName(independentConfig),
      status: 'connecting',
      config: independentConfig,
      ...meta,
      noActivate: meta?.noActivate ?? false
    });

    try {
      await connectSessionById(sessionId, independentConfig);
    } catch (err) {
      removeSession(sessionId);
      console.error(err);
      toast.error('连接请求失败');
    }

    return sessionId;
  }

  async function clearRecentSessions() {
    try {
      await invokeCommand('clear_recent_sessions');
      await loadSavedSessions();
      toast.success('最近会话已清空');
    } catch (e) {
      console.error('Failed to clear recent sessions:', e);
      toast.error('清空最近会话失败');
    }
  }

  async function openSplitShell(sourceSessionId, workspaceSessionId) {
    const source = getSession(sourceSessionId);
    const root = getSession(workspaceSessionId);
    if (!source || !root?.config) return null;
    const channelId = crypto.randomUUID();
    addSession({
      id: channelId,
      name: root.name,
      status: 'connecting',
      config: root.config,
      cwd: source.cwd || '',
      isSplitChild: true,
      parentId: workspaceSessionId,
      workspaceSessionId,
      noActivate: true
    });
    void (async () => {
      try {
        await waitForTerminalReady(channelId);
        await invokeCommand('open_ssh_shell_channel', {
          rootSessionId: workspaceSessionId,
          channelId,
          termType: root.config.term_type || null,
          loginScript: root.config.login_script || null
        });
        setSessionStatus(channelId, 'connected');
      } catch (error) {
        setSessionStatus(channelId, 'error');
        console.error('Failed to open shared SSH shell channel:', error);
        toast.error('创建分屏 Shell Channel 失败');
      }
    })();
    return channelId;
  }

  return {
    sessions,
    activeSessionId,
    savedSessions,
    groups,
    groupOrder,
    groupPrefs,
    sftpConnectedSessionIds,

    addSession,
    removeSession,
    getSession,
    isSftpConnected,
    markSftpConnected,
    markSftpDisconnected,

    loadSavedSessions,
    saveSessionToStorage,
    deleteSessionFromStorage,
    clearRecentSessions,
    updateSessionGroup,
    setGroupOrder,
    setGroupPinned,
    setGroupLocked,
    renameGroup,
    removeGroup,
    connectStoredSession,
    setSessionStatus,
    connectLogic,
    connectLogicWithMeta,
    openSplitShell,
    reconnectSession,
    reconnectAllSessions,
    updateSessionCwd,

    commandBlacklist,
    defaultBlacklist,
    moveSession // Export moveSession
  };
});
