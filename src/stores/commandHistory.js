import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invokeCommand } from '@/utils/ipc';
import { loadPreference } from '@/utils/preferences';
import {
  findCommandHistoryMatches,
  isRecordableCommandHistory,
  normalizeCommandHistory,
  recordCommandHistoryEntry,
} from '@/utils/terminalCommandHistory';

const LEGACY_HISTORY_STORAGE_KEY = 'cmd-history-v1';
const DEFAULT_SCOPE_KEY = 'global';
const DEFAULT_HISTORY_LIMIT = 1000;
const DEFAULT_COMMAND_MAX_CHARS = 4096;
const DEFAULT_TOTAL_MAX_CHARS = 1_000_000;

export const useCommandHistoryStore = defineStore('commandHistory', () => {
  const entries = ref([]);
  const loaded = ref(false);
  const loading = ref(false);
  const lastError = ref('');
  const enabled = ref(loadPreference('commandHistory').enabled !== false);
  let loadPromise = null;
  let mutationQueue = Promise.resolve();

  function clearLegacyHistory() {
    try {
      localStorage.removeItem(LEGACY_HISTORY_STORAGE_KEY);
    } catch {
      // SQLite remains the source of truth even when WebView storage is unavailable.
    }
  }

  function capEntries(nextEntries) {
    const normalized = normalizeCommandHistory(nextEntries).slice(-DEFAULT_HISTORY_LIMIT);
    let total = 0;
    let start = normalized.length;
    while (start > 0) {
      const length = Array.from(normalized[start - 1].cmd || '').length;
      if (total + length > DEFAULT_TOTAL_MAX_CHARS) break;
      total += length;
      start -= 1;
    }
    return normalized.slice(start);
  }

  function replaceEntry(savedEntry) {
    const current = entries.value.find((entry) => entry.cmd === savedEntry.cmd);
    const next = entries.value.filter((entry) => entry.cmd !== savedEntry.cmd);
    next.push({
      ...savedEntry,
      count: Math.max(Number(savedEntry.count || 1), Number(current?.count || 1)),
      lastUsedAt: Math.max(Number(savedEntry.lastUsedAt || 0), Number(current?.lastUsedAt || 0)),
    });
    entries.value = capEntries(next);
  }

  function enqueueMutation(task) {
    const operation = mutationQueue.then(task, task);
    mutationQueue = operation.catch(() => {});
    return operation;
  }

  async function loadEntries({ force = false } = {}) {
    if (loadPromise) {
      await loadPromise;
      if (!force) return entries.value;
    }
    if (loaded.value && !force) return entries.value;

    loading.value = true;
    lastError.value = '';
    clearLegacyHistory();
    loadPromise = invokeCommand('load_command_history', {
      scopeKey: DEFAULT_SCOPE_KEY,
      limit: DEFAULT_HISTORY_LIMIT,
    })
      .then((data) => {
        entries.value = capEntries(Array.isArray(data) ? data : []);
        loaded.value = true;
        return entries.value;
      })
      .catch((error) => {
        lastError.value = String(error || '');
        console.error('Load command history failed:', error);
        return entries.value;
      })
      .finally(() => {
        loading.value = false;
        loadPromise = null;
      });
    return loadPromise;
  }

  function matches(query, { excludedCommands = [], limit = 10 } = {}) {
    if (!enabled.value) return [];
    return findCommandHistoryMatches(entries.value, query, { excludedCommands, limit });
  }

  function record(command, context = {}) {
    if (!enabled.value) return null;
    const text = String(command || '').trim();
    if (!isRecordableCommandHistory(text, DEFAULT_COMMAND_MAX_CHARS)) {
      return null;
    }

    return enqueueMutation(async () => {
      await loadEntries();
      entries.value = capEntries(recordCommandHistoryEntry(entries.value, text, {
        max: DEFAULT_HISTORY_LIMIT,
        minLength: 1,
      }));

      try {
        const saved = await invokeCommand('record_command_history', {
          command: text,
          source: context.source || 'terminal',
          protocol: context.protocol || null,
          host: context.host || null,
          username: context.username || null,
          scopeKey: DEFAULT_SCOPE_KEY,
          max: DEFAULT_HISTORY_LIMIT,
        });
        if (saved) replaceEntry(saved);
        return saved || null;
      } catch (error) {
        lastError.value = String(error || '');
        console.error('Record command history failed:', error);
        await loadEntries({ force: true });
        return null;
      }
    });
  }

  function clear() {
    return enqueueMutation(async () => {
      await loadEntries();
      await invokeCommand('clear_command_history', { scopeKey: DEFAULT_SCOPE_KEY });
      entries.value = [];
    });
  }

  function setEnabled(value) {
    enabled.value = value !== false;
  }

  return {
    entries,
    loaded,
    loading,
    lastError,
    enabled,
    loadEntries,
    matches,
    record,
    clear,
    setEnabled,
  };
});
