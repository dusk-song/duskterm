<script setup>
import IconButton from '@/components/common/IconButton.vue';
import Input from '@/components/ui/input/Input.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { useVirtualList } from '@/composables/useVirtualList';
import { useCommandKnowledgeStore } from '@/stores/commandKnowledge';
import {
  Copy,
  Download,
  Pencil,
  Play,
  Plus,
  Send,
  Star,
  Trash2,
  Upload,
  X
} from '@lucide/vue';
import { open, save } from '@tauri-apps/plugin-dialog';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import CommandKnowledgeDialog from './CommandKnowledgeDialog.vue';

const emit = defineEmits(['close', 'insert-command', 'execute-command']);

const commandKnowledgeStore = useCommandKnowledgeStore();
const searchText = ref('');
const debouncedSearchText = ref('');
const selectedEntryId = ref('');
const dialogOpen = ref(false);
const editingEntry = ref(null);
const viewportRef = ref(null);
const tagStyleCache = new Map();
let searchDebounceTimer = null;

const filteredEntries = computed(() => commandKnowledgeStore.search(debouncedSearchText.value, 2000));
const selectedEntry = computed(() =>
  filteredEntries.value.find((entry) => entry.id === selectedEntryId.value)
  || filteredEntries.value[0]
  || null
);

const listVirtual = useVirtualList({
  items: filteredEntries,
  rowHeight: 36,
  overscan: 12,
});

const visibleRows = computed(() => listVirtual.visibleItems.value.map((entry, offset) => ({
  entry,
  index: listVirtual.startIndex.value + offset,
})));

const tagColorPalette = [
  { bg: 'rgba(55, 120, 180, 0.14)', border: 'rgba(55, 120, 180, 0.35)', text: '#2f6f9f' },
  { bg: 'rgba(80, 145, 92, 0.14)', border: 'rgba(80, 145, 92, 0.34)', text: '#3d7d4e' },
  { bg: 'rgba(188, 126, 42, 0.15)', border: 'rgba(188, 126, 42, 0.36)', text: '#91601f' },
  { bg: 'rgba(172, 78, 94, 0.14)', border: 'rgba(172, 78, 94, 0.34)', text: '#9a3f51' },
  { bg: 'rgba(104, 94, 176, 0.14)', border: 'rgba(104, 94, 176, 0.34)', text: '#5f55a6' },
  { bg: 'rgba(42, 145, 140, 0.14)', border: 'rgba(42, 145, 140, 0.34)', text: '#267c78' },
];

const availableTags = computed(() => {
  const counts = new Map();
  for (const entry of commandKnowledgeStore.entries) {
    for (const tag of entry.tags || []) {
      const normalized = String(tag || '').trim();
      if (!normalized) continue;
      counts.set(normalized, (counts.get(normalized) || 0) + 1);
    }
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 24)
    .map(([tag]) => tag);
});

const hashTag = (tag) => {
  let hash = 0;
  for (const char of String(tag || '')) {
    hash = ((hash << 5) - hash + char.charCodeAt(0)) | 0;
  }
  return Math.abs(hash);
};

const getTagStyle = (tag) => {
  const cacheKey = String(tag || '');
  const cached = tagStyleCache.get(cacheKey);
  if (cached) return cached;
  const color = tagColorPalette[hashTag(tag) % tagColorPalette.length];
  const style = {
    '--tag-bg': color.bg,
    '--tag-border': color.border,
    '--tag-text': color.text,
  };
  tagStyleCache.set(cacheKey, style);
  return style;
};

const isTagFilterActive = (tag) => searchText.value.trim().toLowerCase() === String(tag || '').trim().toLowerCase();

const applyTagFilter = (tag) => {
  searchText.value = isTagFilterActive(tag) ? '' : tag;
  debouncedSearchText.value = searchText.value;
};

const syncViewportHeight = () => {
  const height = viewportRef.value?.clientHeight || 360;
  listVirtual.setViewportHeight(height);
};

watch(filteredEntries, (entries) => {
  if (!entries.some((entry) => entry.id === selectedEntryId.value)) {
    selectedEntryId.value = entries[0]?.id || '';
  }
  nextTick(syncViewportHeight);
});

watch(searchText, (value) => {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    debouncedSearchText.value = value;
    searchDebounceTimer = null;
  }, 80);
});

const summarizeCommand = (command) => {
  const text = String(command || '').replace(/\s+/g, ' ').trim();
  return text.length > 120 ? `${text.slice(0, 117)}...` : text;
};

const safetyLabel = (level) => ({
  normal: '普通',
  sensitive: '敏感',
  dangerous: '高危',
}[level] || '普通');

const policyLabel = (policy) => ({
  insertOnly: '仅插入',
  confirmBeforeExecute: '执行前确认',
  blockDirectExecute: '禁止直接执行',
}[policy] || '仅插入');

const confirmChoice = async (options) => {
  try {
    await confirm(options);
    return true;
  } catch {
    return false;
  }
};

const selectEntry = (entry) => {
  selectedEntryId.value = entry?.id || '';
};

const openCreateDialog = () => {
  editingEntry.value = null;
  dialogOpen.value = true;
};

const openEditDialog = (entry = selectedEntry.value) => {
  if (!entry) return;
  editingEntry.value = entry;
  dialogOpen.value = true;
};

const handleSaveEntry = async (entry) => {
  try {
    const saved = await commandKnowledgeStore.saveEntry(entry);
    selectedEntryId.value = saved.id;
    dialogOpen.value = false;
    toast.success('命令条目已保存');
  } catch (error) {
    toast.error(`保存失败：${error}`);
  }
};

const copyCommand = async (entry = selectedEntry.value) => {
  if (!entry?.command) return;
  try {
    await navigator.clipboard.writeText(entry.command);
    toast.success('命令已复制');
  } catch {
    toast.error('复制失败');
  }
};

const insertCommand = async (entry = selectedEntry.value) => {
  if (!entry?.command) return;
  emit('insert-command', { id: entry.id, command: entry.command });
};

const executeCommand = async (entry = selectedEntry.value) => {
  if (!entry?.command) return;
  if (entry.executionPolicy === 'blockDirectExecute' || entry.safetyLevel === 'dangerous') {
    toast.warning('高危命令禁止从知识库直接执行');
    return;
  }
  if (entry.executionPolicy === 'confirmBeforeExecute' || entry.safetyLevel === 'sensitive') {
    const ok = await confirmChoice({
      title: '执行敏感命令？',
      content: entry.command,
      okText: '执行',
      cancelText: '取消',
      centered: true,
    });
    if (!ok) return;
  }
  emit('execute-command', { id: entry.id, command: entry.command });
};

const removeEntry = async (entry = selectedEntry.value) => {
  if (!entry) return;
  const ok = await confirmChoice({
    title: '删除命令条目？',
    content: entry.title,
    okText: '删除',
    cancelText: '取消',
    centered: true,
  });
  if (!ok) return;
  try {
    await commandKnowledgeStore.deleteEntry(entry.id);
    toast.success('命令条目已删除');
  } catch (error) {
    toast.error(`删除失败：${error}`);
  }
};

const resolveDialogPath = (selected) => {
  if (!selected) return '';
  if (Array.isArray(selected)) return resolveDialogPath(selected[0]);
  return selected.path || selected || '';
};

const exportKnowledge = async () => {
  const targetPath = await save({
    defaultPath: 'command-knowledge.json',
    filters: [{ name: 'JSON', extensions: ['json'] }],
  });
  const path = resolveDialogPath(targetPath);
  if (!path) return;
  try {
    await commandKnowledgeStore.exportTo(path);
    toast.success('命令知识库已导出');
  } catch (error) {
    toast.error(`导出失败：${error}`);
  }
};

const importKnowledge = async () => {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'JSON', extensions: ['json'] }],
  });
  const path = resolveDialogPath(selected);
  if (!path) return;
  try {
    const imported = await commandKnowledgeStore.importFrom(path);
    toast.success(`已导入 ${imported.length || 0} 条命令`);
  } catch (error) {
    toast.error(`导入失败：${error}`);
  }
};

onMounted(async () => {
  await commandKnowledgeStore.loadEntries();
  selectedEntryId.value = filteredEntries.value[0]?.id || '';
  await nextTick();
  syncViewportHeight();
  window.addEventListener('resize', syncViewportHeight);
});

onUnmounted(() => {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  window.removeEventListener('resize', syncViewportHeight);
});
</script>

<template>
  <aside class="command-knowledge-panel" aria-label="命令知识库">
    <div class="knowledge-toolbar">
      <Input v-model="searchText" placeholder="搜索标题、命令、触发词、标签、说明" />
      <div class="knowledge-actions">
        <IconButton :icon="Upload" size="sm" aria-label="导入" :action="importKnowledge" />
        <IconButton :icon="Download" size="sm" aria-label="导出" :action="exportKnowledge" />
        <IconButton :icon="Plus" size="sm" aria-label="新增" :action="openCreateDialog" />
        <IconButton :icon="X" size="sm" aria-label="关闭" :action="() => emit('close')" />
      </div>
    </div>

    <div class="knowledge-search">
      <div v-if="availableTags.length" class="knowledge-tag-filters" aria-label="标签筛选">
        <button v-for="tag in availableTags" :key="tag" type="button" class="knowledge-tag-chip"
          :class="{ active: isTagFilterActive(tag) }" :style="getTagStyle(tag)" @click="applyTagFilter(tag)">
          {{ tag }}
        </button>
      </div>
    </div>

    <div ref="viewportRef" class="knowledge-list" @scroll="listVirtual.onScroll">
      <div class="knowledge-list-spacer" :style="{ height: `${listVirtual.totalHeight.value}px` }">
        <div class="knowledge-list-window" :style="{ transform: `translateY(${listVirtual.translateY.value}px)` }">
          <button v-for="{ entry, index } in visibleRows" :key="entry.id" type="button" class="knowledge-row"
            :class="{ active: entry.id === selectedEntry?.id }" :data-index="index" @click="selectEntry(entry)"
            @dblclick="insertCommand(entry)">
            <span v-if="entry.trigger" class="knowledge-trigger">{{ entry.trigger }}</span>
            <span class="knowledge-row-title">{{ entry.title }}</span>
            <span class="knowledge-row-command">{{ summarizeCommand(entry.command) }}</span>
            <Star v-if="entry.favorite" class="knowledge-star" />
          </button>
        </div>
      </div>
      <div v-if="!filteredEntries.length" class="knowledge-empty">没有匹配的命令条目</div>
    </div>

    <footer v-if="selectedEntry" class="knowledge-detail">
      <div class="detail-title-line">
        <strong>{{ selectedEntry.title }}</strong>
        <span class="detail-badge" :class="`risk-${selectedEntry.safetyLevel}`">
          {{ safetyLabel(selectedEntry.safetyLevel) }}
        </span>
      </div>
      <pre class="detail-command">{{ selectedEntry.command }}</pre>
      <div v-if="selectedEntry.description" class="detail-description">{{ selectedEntry.description }}</div>
      <div class="detail-meta">
        <span>{{ policyLabel(selectedEntry.executionPolicy) }}</span>
        <span v-if="selectedEntry.trigger">#{{ selectedEntry.trigger }}</span>
        <span v-for="tag in selectedEntry.tags" :key="tag" class="detail-tag" :style="getTagStyle(tag)">{{ tag }}</span>
        <span>使用 {{ selectedEntry.usageCount || 0 }}</span>
      </div>
      <div class="detail-actions">
        <IconButton :icon="Copy" size="sm" aria-label="复制" :action="() => copyCommand(selectedEntry)" />
        <IconButton :icon="Send" size="sm" aria-label="发送" :action="() => insertCommand(selectedEntry)" />
        <IconButton :icon="Play" size="sm" aria-label="执行" :action="() => executeCommand(selectedEntry)" />
        <IconButton :icon="Pencil" size="sm" aria-label="编辑" :action="() => openEditDialog(selectedEntry)" />
        <IconButton :icon="Trash2" size="sm" aria-label="删除" :action="() => removeEntry(selectedEntry)" />
      </div>
    </footer>

    <CommandKnowledgeDialog v-model:open="dialogOpen" :entry="editingEntry" @save="handleSaveEntry" />
  </aside>
</template>

<style scoped>
.command-knowledge-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  box-sizing: border-box;
  padding: 6px;
  overflow: hidden;
  color: var(--app-text);
  background: transparent;
}

.knowledge-toolbar,
.knowledge-actions,
.detail-title-line,
.detail-meta,
.detail-actions {
  display: flex;
  align-items: center;
}

.knowledge-toolbar {
  gap: 6px;
  min-width: 0;
  padding: 0;
}

.knowledge-toolbar :deep(input) {
  min-width: 120px;
  height: 30px;
  flex: 1 1 140px;
}

.knowledge-actions,
.detail-actions {
  gap: 4px;
}

.knowledge-search {
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  gap: 6px;
  min-width: 0;
}

.knowledge-tag-filters {
  display: flex;
  gap: 5px;
  min-width: 0;
  overflow-x: auto;
  padding-bottom: 2px;
  scrollbar-width: thin;
}

.knowledge-tag-chip {
  flex: 0 0 auto;
  height: 22px;
  max-width: 112px;
  padding: 0 7px;
  border: 1px solid var(--tag-border);
  border-radius: 4px;
  background: var(--tag-bg);
  color: var(--tag-text);
  font-size: 11px;
  line-height: 20px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.knowledge-tag-chip:hover,
.knowledge-tag-chip.active {
  filter: saturate(1.18);
  box-shadow: inset 0 0 0 1px var(--tag-border);
}

.knowledge-list {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  border-top: 1px solid var(--app-border-light);
  border-bottom: 1px solid var(--app-border-light);
}

.knowledge-list-spacer {
  position: relative;
  min-height: 100%;
}

.knowledge-list-window {
  position: absolute;
  inset: 0 0 auto 0;
}

.knowledge-row {
  display: grid;
  grid-template-columns: minmax(42px, auto) minmax(86px, 0.52fr) minmax(0, 1fr) 16px;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: 36px;
  padding: 0 8px;
  border: 0;
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-light) 72%, transparent);
  background: transparent;
  color: var(--app-text);
  text-align: left;
  cursor: pointer;
}

.knowledge-row:hover {
  background: color-mix(in srgb, var(--app-selection-bg) 40%, transparent);
}

.knowledge-row.active {
  background: var(--app-selection-bg);
  color: var(--app-selection-text);
}

.knowledge-trigger,
.knowledge-row-title,
.knowledge-row-command {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.knowledge-trigger {
  padding: 1px 5px;
  border: 1px solid var(--app-border-light);
  border-radius: 4px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--app-text-muted);
}

.knowledge-row-title {
  font-size: 12px;
  font-weight: 600;
}

.knowledge-row-command {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--app-text-muted);
}

.knowledge-row.active .knowledge-row-command,
.knowledge-row.active .knowledge-trigger {
  color: color-mix(in srgb, var(--app-selection-text) 72%, transparent);
}

.knowledge-star {
  width: 14px;
  height: 14px;
  color: #c28b2c;
  fill: currentColor;
}

.knowledge-empty {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: var(--app-text-muted);
  font-size: 12px;
}

.knowledge-detail {
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  gap: 7px;
  min-height: 150px;
  max-height: 42%;
  padding: 8px 2px 2px;
}

.detail-title-line {
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.detail-title-line strong {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 13px;
}

.detail-badge {
  flex: 0 0 auto;
  border-radius: 4px;
  padding: 1px 6px;
  font-size: 11px;
  border: 1px solid var(--app-border-light);
  color: var(--app-text-muted);
}

.detail-badge.risk-sensitive {
  color: #9a6a00;
  border-color: color-mix(in srgb, #d9a520 50%, transparent);
}

.detail-badge.risk-dangerous {
  color: #b83232;
  border-color: color-mix(in srgb, #b83232 50%, transparent);
}

.detail-command {
  min-height: 46px;
  max-height: 108px;
  margin: 0;
  overflow: auto;
  padding: 8px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-input-bg) 80%, transparent);
  border: 1px solid var(--app-border-light);
  color: var(--app-text);
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-word;
}

.detail-description {
  color: var(--app-text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.detail-meta {
  flex-wrap: wrap;
  gap: 4px;
  color: var(--app-text-muted);
  font-size: 11px;
}

.detail-meta span {
  border: 1px solid var(--app-border-light);
  border-radius: 4px;
  padding: 1px 5px;
}

.detail-meta .detail-tag {
  border-color: var(--tag-border);
  background: var(--tag-bg);
  color: var(--tag-text);
}

.detail-actions {
  justify-content: flex-end;
}
</style>
