import { toast } from '@/composables/useToast';
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { invokeCommand } from '@/utils/ipc';
import {
  buildCommandKnowledgeIndex,
  deriveKnowledgeSensitiveRules,
  matchKnowledgeTriggers,
  migrateLegacyKnowledgeEntries,
  normalizeKnowledgeEntry,
  searchKnowledgeEntries,
} from '@/utils/commandKnowledge';

const LEGACY_QUICK_COMMANDS_KEY = 'quick-commands-v1';
const LEGACY_BLACKLIST_KEY = 'ssh-command-blacklist';
const MIGRATION_KEY = 'command-knowledge-migrated-v1';

const readLocalJson = (key, fallback) => {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw);
    return parsed ?? fallback;
  } catch {
    return fallback;
  }
};

const writeChangedEvent = () => {
  window.dispatchEvent(new CustomEvent('command-knowledge-changed'));
};

export const useCommandKnowledgeStore = defineStore('commandKnowledge', () => {
  const entries = ref([]);
  const loaded = ref(false);
  const loading = ref(false);
  const lastError = ref('');

  const index = computed(() => buildCommandKnowledgeIndex(entries.value));
  const sensitiveRules = computed(() => deriveKnowledgeSensitiveRules(entries.value));

  function setEntries(nextEntries) {
    entries.value = (nextEntries || []).map((entry) => normalizeKnowledgeEntry(entry));
    writeChangedEvent();
  }

  async function persistAll(nextEntries) {
    const saved = await invokeCommand('replace_command_knowledge_entries', { entries: nextEntries });
    setEntries(saved || nextEntries);
    return entries.value;
  }

  async function migrateLegacyIfNeeded() {
    if (localStorage.getItem(MIGRATION_KEY) === '1') return false;

    const quickCommands = readLocalJson(LEGACY_QUICK_COMMANDS_KEY, []);
    const blacklistRules = readLocalJson(LEGACY_BLACKLIST_KEY, []);
    if (!Array.isArray(quickCommands) && !Array.isArray(blacklistRules)) {
      localStorage.setItem(MIGRATION_KEY, '1');
      return false;
    }

    const migrated = migrateLegacyKnowledgeEntries({
      existingEntries: entries.value,
      quickCommands: Array.isArray(quickCommands) ? quickCommands : [],
      blacklistRules: Array.isArray(blacklistRules) ? blacklistRules : [],
    });

    if (migrated.length !== entries.value.length) {
      await persistAll(migrated);
    }
    localStorage.setItem(MIGRATION_KEY, '1');
    return true;
  }

  async function loadEntries() {
    if (loading.value) return entries.value;
    loading.value = true;
    lastError.value = '';
    try {
      const data = await invokeCommand('load_command_knowledge');
      setEntries(Array.isArray(data) ? data : []);
      loaded.value = true;
      await migrateLegacyIfNeeded();
      return entries.value;
    } catch (error) {
      lastError.value = String(error || '');
      console.error('Load command knowledge failed:', error);
      toast.error('加载命令知识库失败');
      return entries.value;
    } finally {
      loading.value = false;
    }
  }

  async function saveEntry(entry) {
    const normalized = normalizeKnowledgeEntry(entry);
    const saved = await invokeCommand('save_command_knowledge_entry', { entry: normalized });
    const next = [...entries.value];
    const index = next.findIndex((item) => item.id === saved.id);
    if (index >= 0) next[index] = saved;
    else next.unshift(saved);
    setEntries(next);
    return saved;
  }

  async function deleteEntry(id) {
    await invokeCommand('delete_command_knowledge_entry', { id });
    setEntries(entries.value.filter((entry) => entry.id !== id));
  }

  async function recordUsage(id) {
    const entry = entries.value.find((item) => item.id === id);
    if (!entry) return null;
    const next = {
      ...entry,
      usageCount: Number(entry.usageCount || 0) + 1,
      lastUsedAt: Date.now(),
    };
    return saveEntry(next);
  }

  function search(query, limit) {
    return searchKnowledgeEntries(index.value, query, limit);
  }

  function matchTriggers(query, limit) {
    return matchKnowledgeTriggers(index.value, query, limit);
  }

  async function exportTo(targetPath) {
    await invokeCommand('export_command_knowledge_to', { targetPath });
  }

  async function importFrom(sourcePath) {
    const imported = await invokeCommand('import_command_knowledge_from', { sourcePath });
    await loadEntries();
    return imported || [];
  }

  return {
    entries,
    loaded,
    loading,
    lastError,
    index,
    sensitiveRules,
    loadEntries,
    saveEntry,
    deleteEntry,
    recordUsage,
    search,
    matchTriggers,
    exportTo,
    importFrom,
    migrateLegacyIfNeeded,
  };
});
