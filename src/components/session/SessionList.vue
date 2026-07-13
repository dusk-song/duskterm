<script setup>
import ContextMenu from '@/components/ui/context-menu/ContextMenu.vue';
import ContextMenuContent from '@/components/ui/context-menu/ContextMenuContent.vue';
import ContextMenuItem from '@/components/ui/context-menu/ContextMenuItem.vue';
import ContextMenuSeparator from '@/components/ui/context-menu/ContextMenuSeparator.vue';
import ContextMenuTrigger from '@/components/ui/context-menu/ContextMenuTrigger.vue';
import Input from '@/components/ui/input/Input.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import {
  Code2,
  Download,
  Folder,
  Lock,
  Pin,
  Plus,
  Server,
  Upload,
  X,
  Usb,
  FolderOpen
} from '@lucide/vue';
import { open, save } from '@tauri-apps/plugin-dialog';
import { v4 as uuidv4 } from 'uuid';
import { computed, h, onMounted, onUnmounted, ref } from 'vue';
import { useVirtualList } from '@/composables/useVirtualList';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand } from '@/utils/ipc';
import IconButton from '@/components/common/IconButton.vue';

const props = defineProps({
  width: {
    type: [String, Number],
    default: '100%'
  },
  sftpActive: Boolean,
  sftpDisabled: Boolean
});
const emit = defineEmits(['open-create', 'open-edit', 'toggle-sftp', 'close']);

const sshStore = useSshStore();
const expandedKeys = ref([]);
const searchKeyword = ref('');
const panelContentRef = ref(null);
let resizeObserver = null;

const panelWidthStyle = computed(() => {
  if (props.width === undefined || props.width === null) {
    return { width: '100%' };
  }

  if (typeof props.width === 'number') {
    return { width: `${props.width}px` };
  }

  return { width: String(props.width) };
});

const normalizedKeyword = computed(() => searchKeyword.value.trim().toLowerCase());

const getProtocol = (session) => String(session?.protocol || 'ssh').toLowerCase();

const getSessionIcon = (session) => {
  switch (getProtocol(session)) {
    case 'telnet':
      return Server;
    case 'serial':
      return Usb;
    default:
      return Code2;
  }
};

const getSessionMeta = (session) => {
  switch (getProtocol(session)) {
    case 'serial':
      return session?.serial_path || '串口';
    case 'telnet':
      return session?.host ? `${session.host}:${session.port || 23}` : 'Telnet';
    default:
      return session?.host || 'SSH';
  }
};

const sessionSearchIndex = computed(() => sshStore.savedSessions.map((session) => {
  const searchText = [
    session?.name,
    session?.host,
    session?.username,
    session?.group,
    session?.serial_path,
    session?.protocol || 'ssh'
  ]
    .map((value) => String(value || '').toLowerCase())
    .join('\u0000');

  return {
    session,
    searchText
  };
}));

const filteredSessions = computed(() => {
  const kw = normalizedKeyword.value;
  if (!kw) return sshStore.savedSessions;
  return sessionSearchIndex.value
    .filter(({ searchText }) => searchText.includes(kw))
    .map(({ session }) => session);
});

const treeData = computed(() => {
  const list = filteredSessions.value;
  const groups = {};
  const ungrouped = [];

  list.forEach(s => {
    if (s.group && s.group.trim() !== '') {
      if (!groups[s.group]) groups[s.group] = [];
      groups[s.group].push(s);
    } else {
      ungrouped.push(s);
    }
  });

  const treeNodes = [];

  const order = (sshStore.groupOrder || []).filter(Boolean);
  const existing = Object.keys(groups).sort();
  const baseGroups = [...order.filter(g => existing.includes(g)), ...existing.filter(g => !order.includes(g))];
  const prefs = sshStore.groupPrefs || {};
  const pinnedGroups = baseGroups.filter(g => prefs[g]?.pinned);
  const normalGroups = baseGroups.filter(g => !prefs[g]?.pinned);
  const orderedGroups = [...pinnedGroups, ...normalGroups];

  if (ungrouped.length > 0 || !normalizedKeyword.value) {
    treeNodes.push({
      title: '未分组',
      key: 'group-__ungrouped__',
      icon: () => h(Folder),
      selectable: false,
      isLeaf: false,
      groupName: '',
      children: ungrouped.map(s => ({
        title: s.name,
        key: s.id,
        icon: () => h(getSessionIcon(s)),
        isLeaf: true,
        data: s
      }))
    });
  }

  const rootMap = new Map();
  const ensureRoot = (part, fullPath) => {
    if (!rootMap.has(part)) {
      const locked = !!prefs[fullPath]?.locked;
      const pinned = !!prefs[fullPath]?.pinned;
      const node = {
        title: part,
        key: 'group-' + fullPath,
        icon: () => h(Folder),
        selectable: false,
        isLeaf: false,
        data: { groupName: fullPath, draggable: !locked, locked, pinned },
        children: []
      };
      node.__childMap = new Map();
      rootMap.set(part, node);
      treeNodes.push(node);
    }
    return rootMap.get(part);
  };

  const ensureChild = (parentNode, part, fullPath) => {
    const map = parentNode.__childMap || (parentNode.__childMap = new Map());
    if (!map.has(part)) {
      const locked = !!prefs[fullPath]?.locked;
      const pinned = !!prefs[fullPath]?.pinned;
      const node = {
        title: part,
        key: 'group-' + fullPath,
        icon: () => h(Folder),
        selectable: false,
        isLeaf: false,
        data: { groupName: fullPath, draggable: !locked, locked, pinned },
        children: []
      };
      node.__childMap = new Map();
      map.set(part, node);
      parentNode.children.push(node);
    }
    return map.get(part);
  };

  orderedGroups.forEach(groupName => {
    const parts = String(groupName).split('/').filter(Boolean);
    if (parts.length === 0) return;

    let currentNode = null;
    let fullPath = '';

    parts.forEach((part, index) => {
      fullPath = fullPath ? `${fullPath}/${part}` : part;
      currentNode = currentNode ? ensureChild(currentNode, part, fullPath) : ensureRoot(part, fullPath);

      if (index === parts.length - 1) {
        const leafSessions = (groups[groupName] || []);
        if (leafSessions.length === 0 && normalizedKeyword.value) return;
        currentNode.children.push(
          ...leafSessions.map(s => ({
            title: s.name,
            key: s.id,
            icon: () => h(getSessionIcon(s)),
            isLeaf: true,
            data: s
          }))
        );
      }
    });
  });

  return treeNodes;
});

// Flatten treeData into visible items (respect expandedKeys)
const flatList = computed(() => {
  const result = [];
  const walk = (nodes, depth) => {
    for (const node of nodes) {
      const key = node.key;
      result.push({ ...node, _depth: depth, _isGroup: !node.isLeaf });
      if (node.children && node.children.length > 0 && expandedKeys.value.includes(key)) {
        walk(node.children, depth + 1);
      }
    }
  };
  walk(treeData.value, 0);
  return result;
});

// Virtual scroll
const viewportRef = ref(null);
const { visibleItems, totalHeight, translateY, onScroll: onVirtualScroll, setViewportHeight } = useVirtualList({
  items: flatList,
  rowHeight: 34,
});

// Drag reorder for groups only
const draggingGroupName = ref('');
const dragOverGroupName = ref('');
const onGroupDragStart = (groupName, e) => {
  draggingGroupName.value = groupName;
  e.dataTransfer.effectAllowed = 'move';
};
const onGroupDragOver = (groupName, e) => {
  e.preventDefault();
  if (draggingGroupName.value && draggingGroupName.value !== groupName) {
    dragOverGroupName.value = groupName;
  }
};
const onGroupDragLeave = () => { dragOverGroupName.value = ''; };
const onGroupDrop = (targetGroupName) => {
  const src = draggingGroupName.value;
  if (!src || src === targetGroupName) { draggingGroupName.value = ''; dragOverGroupName.value = ''; return; }
  const order = (sshStore.groupOrder || []).filter(Boolean);
  const allGroups = [...new Set((sshStore.savedSessions || []).map(s => s.group).filter(Boolean))];
  const base = [...order.filter(g => allGroups.includes(g)), ...allGroups.filter(g => !order.includes(g))];
  const dragIdx = base.indexOf(src);
  const targetIdx = base.indexOf(targetGroupName);
  if (dragIdx === -1 || targetIdx === -1) { draggingGroupName.value = ''; dragOverGroupName.value = ''; return; }
  const next = base.filter(g => g !== src);
  next.splice(targetIdx, 0, src);
  sshStore.setGroupOrder(next);
  draggingGroupName.value = '';
  dragOverGroupName.value = '';
};

const onCtxMenuClick = (action, node) => {
  if (!node) return;
  if (action === 'connect') sshStore.connectStoredSession(node.key);
  else if (action === 'edit') emit('open-edit', node.data);
  else if (action === 'duplicate') duplicateSession(node.data);
  else if (action === 'delete') handleDelete(node.key);
  else if (action === 'rename') startRenameGroup(node.key, node.data.groupName);
  else if (action === 'pin') togglePinned(node.data.groupName);
  else if (action === 'lock') toggleLocked(node.data.groupName);
  else if (action === 'delete-group') handleRemoveGroup(node.data.groupName);
};

const handleDelete = (id) => {
  confirm({
    title: '确认删除',
    content: '确定要删除这个会话吗？此操作不可撤销。',
    okText: '删除',
    cancelText: '取消',
    onOk() {
      sshStore.deleteSessionFromStorage(id);
    }
  });
};

const duplicateSession = async (session) => {
  if (!session?.id) return;
  try {
    const decrypted = await invokeCommand('get_decrypted_session', { id: session.id });
    const host = decrypted.host || 'unknown';
    // Count existing sessions with the same IP to generate suffix (1), (2)...
    const sameHostCount = (sshStore.savedSessions || []).filter(s => s.host === host).length;
    const clone = {
      ...decrypted,
      id: uuidv4(),
      name: `${host} (${sameHostCount + 1})`,
      last_connected: null,
      local_forward: null,
      remote_forward: null,
      login_script: null,
      remarks: null,
    };
    await invokeCommand('save_session', { session: clone });
    await sshStore.loadSavedSessions();
    toast.success('会话已复制，请在编辑中修改 IP');
  } catch (e) {
    toast.error(`复制失败: ${e}`);
  }
};

const splitTitle = (text) => {
  const kw = normalizedKeyword.value;
  if (!kw) return [{ text, match: false }];
  const lower = text.toLowerCase();
  const idx = lower.indexOf(kw);
  if (idx === -1) return [{ text, match: false }];
  return [
    { text: text.slice(0, idx), match: false },
    { text: text.slice(idx, idx + kw.length), match: true },
    { text: text.slice(idx + kw.length), match: false }
  ];
};

onMounted(() => {
  resizeObserver = new ResizeObserver(() => {
    if (panelContentRef.value) {
      setViewportHeight(Math.max(200, panelContentRef.value.clientHeight - 6));
    }
  });
  if (panelContentRef.value) resizeObserver.observe(panelContentRef.value);
});

onUnmounted(() => {
  if (resizeObserver) resizeObserver.disconnect();
});

const togglePinned = (groupName) => {
  const prefs = sshStore.groupPrefs || {};
  const current = !!prefs[groupName]?.pinned;
  sshStore.setGroupPinned(groupName, !current);
};

const toggleLocked = (groupName) => {
  const prefs = sshStore.groupPrefs || {};
  const current = !!prefs[groupName]?.locked;
  sshStore.setGroupLocked(groupName, !current);
};

const handleRemoveGroup = (groupName) => {
  confirm({
    title: '删除分组',
    content: `分组下的会话将移到“未分组”。确认删除 ${groupName} 吗？`,
    okText: '确认',
    cancelText: '取消',
    onOk() {
      return sshStore.removeGroup(groupName);
    }
  });
};

// --- Renaming Logic ---
const editingNodeKey = ref(null);
const editingName = ref('');
const editingOldName = ref('');
const renameInputRef = ref(null);
const isRenaming = ref(false);

const startRenameGroup = (nodeKey, groupName) => {
  // 模拟点击 body 关闭可能存在的 Dropdown
  document.body.click();

  // 延时执行，确保 Dropdown 有时间完成关闭动作，避免 DOM 瞬间变化导致 Dropdown 状态卡住
  setTimeout(() => {
    editingNodeKey.value = nodeKey;
    editingOldName.value = groupName;
    editingName.value = groupName;
    nextTick(() => {
      if (renameInputRef.value) {
        renameInputRef.value.focus();
        renameInputRef.value.select();
      }
    });
  }, 0);
};

const confirmRename = async () => {
  if (!editingNodeKey.value || isRenaming.value) return;
  isRenaming.value = true;

  try {
    const newName = editingName.value.trim();
    const oldName = editingOldName.value;

    if (newName && newName !== oldName) {
      await sshStore.renameGroup(oldName, newName);
    }
    cancelRename();
  } finally {
    isRenaming.value = false;
  }
};

const cancelRename = () => {
  editingNodeKey.value = null;
  editingName.value = '';
  editingOldName.value = '';
};

const toggleGroup = (key) => {
  const idx = expandedKeys.value.indexOf(key);
  if (idx > -1) expandedKeys.value.splice(idx, 1);
  else expandedKeys.value.push(key);
};

const handleExportSessions = async () => {
  try {
    const output = await save({
      defaultPath: 'duskterm-sessions.json',
      filters: [{ name: 'JSON', extensions: ['json'] }]
    });
    if (!output) return;
    const targetPath = typeof output === 'string' ? output : output.path;
    await invokeCommand('export_sessions_to', { targetPath });
    toast.success('会话已导出');
  } catch (err) {
    toast.error(`导出失败: ${err}`);
  }
};

const handleImportSessions = async () => {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'JSON', extensions: ['json'] }]
    });
    if (!selected) return;
    const sourcePath = typeof selected === 'string' ? selected : selected.path;
    await invokeCommand('import_sessions_from', { sourcePath });
    await sshStore.loadSavedSessions();
    toast.success('会话已导入');
  } catch (err) {
    toast.error(`导入失败: ${err}`);
  }
};

</script>

<template>
  <div class="session-panel" :style="panelWidthStyle">
    <div class="panel-content" ref="panelContentRef">
      <div class="search-bar">
        <Input v-model="searchKeyword" placeholder="搜索…" size="sm" class="session-search-input" />
        <div class="search-actions">
          <IconButton :icon="FolderOpen" size="sm" aria-label="切换 SFTP 文件面板"
            :active="props.sftpActive" :disabled="props.sftpDisabled"
            :action="() => emit('toggle-sftp')" />
          <IconButton :icon="Plus" size="sm" aria-label="新建会话" :action="() => $emit('open-create')" />
          <IconButton :icon="Upload" size="sm" aria-label="导入" :action="handleImportSessions" />
          <IconButton :icon="Download" size="sm" aria-label="导出" :action="handleExportSessions" />
          <IconButton :icon="X" size="sm" aria-label="关闭面板" :action="() => $emit('close')" />
        </div>
      </div>
      <div v-if="sshStore.savedSessions.length > 0" class="session-tree-viewport" @scroll="onVirtualScroll">
        <div :style="{ height: totalHeight + 'px', position: 'relative' }">
          <div :style="{ transform: `translateY(${translateY}px)` }">
            <ContextMenu v-for="item in visibleItems" :key="item.key">
              <ContextMenuTrigger as-child>
                <div class="tree-row"
                  :class="{ 'drag-over': dragOverGroupName === (item.data?.groupName || item.title) }"
                  :style="{ paddingLeft: (item._depth * 16 + 4) + 'px', height: '34px' }"
                  @dblclick="item.isLeaf ? sshStore.connectStoredSession(item.key) : null">

                  <span v-if="item._isGroup" class="tree-arrow" :class="{ expanded: expandedKeys.includes(item.key) }"
                    @click.stop="toggleGroup(item.key)">▶</span>
                  <span v-else class="tree-arrow" style="visibility:hidden">▶</span>

                  <component :is="item.isLeaf ? getSessionIcon(item.data) : Folder"
                    style="margin-right: 5px; flex-shrink: 0" />

                  <span v-if="editingNodeKey === item.key" @click.stop class="flex-1 min-w-0">
                    <Input ref="renameInputRef" v-model="editingName" @blur="confirmRename"
                      @keydown.enter="confirmRename" @keydown.esc="cancelRename" size="sm"
                      style="width: 120px; height: 22px; font-size: 13px" />
                  </span>
                  <span v-else class="tree-node-title flex-1 min-w-0">
                    <template v-if="typeof item.title === 'string'">
                      <template v-for="(part, idx) in splitTitle(item.title)" :key="idx">
                        <mark v-if="part.match" class="match-mark">{{ part.text }}</mark>
                        <span v-else>{{ part.text }}</span>
                      </template>
                    </template>
                    <template v-else>{{ item.title }}</template>

                    <Lock v-if="item._isGroup && item.data?.locked"
                      style="margin-left: 6px; color: var(--app-text-muted, #ABB2BF); flex-shrink: 0" />
                    <Pin v-if="item._isGroup && item.data?.pinned"
                      style="margin-left: 6px; color: var(--app-text-muted, #ABB2BF); flex-shrink: 0" />

                    <span v-if="item.isLeaf" class="node-meta">({{ getSessionMeta(item.data) }})</span>
                  </span>

                  <span v-if="item._isGroup && item.key !== 'group-__ungrouped__'" class="tree-drag-handle"
                    draggable="true" @dragstart="onGroupDragStart(item.data?.groupName || item.title, $event)"
                    @dragover="onGroupDragOver(item.data?.groupName || item.title, $event)"
                    @dragleave="onGroupDragLeave" @drop="onGroupDrop(item.data?.groupName || item.title)" @click.stop
                    title="拖拽排序">⠿</span>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <template v-if="item.isLeaf">
                  <ContextMenuItem @select="onCtxMenuClick('connect', item)">连接</ContextMenuItem>
                  <ContextMenuItem @select="onCtxMenuClick('edit', item)">编辑</ContextMenuItem>
                  <ContextMenuItem @select="onCtxMenuClick('duplicate', item)">复制会话</ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem class="!text-destructive" @select="onCtxMenuClick('delete', item)">删除
                  </ContextMenuItem>
                </template>
                <template v-else>
                  <ContextMenuItem @select="onCtxMenuClick('rename', item)">重命名</ContextMenuItem>
                  <ContextMenuItem @select="onCtxMenuClick('pin', item)">{{ item.data?.pinned ? '取消置顶' : '置顶' }}
                  </ContextMenuItem>
                  <ContextMenuItem @select="onCtxMenuClick('lock', item)">{{ item.data?.locked ? '解除锁定' : '锁定' }}
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem class="!text-destructive" @select="onCtxMenuClick('delete-group', item)">删除分组
                  </ContextMenuItem>
                </template>
              </ContextMenuContent>
            </ContextMenu>
          </div>
        </div>
      </div>
      <div v-else class="empty-tip">
        暂无会话
      </div>

    </div>
  </div>
</template>

<style scoped>
.session-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 260px;
  background: transparent;
  overflow: hidden;
}

.panel-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 0;
  background: transparent;
}

.search-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  background: transparent;
  border-bottom: 0;
  flex-wrap: nowrap;
  overflow-x: hidden;
}

.search-bar :deep(.session-search-input) {
  flex: 1 1 140px;
  min-width: 120px;
  height: 30px;
  border-radius: 8px;
  background: var(--app-input-bg) !important;
  border: 1px solid var(--app-border-shadow) !important;
  font-size: 13px;
  color: var(--app-text);
}

.search-bar :deep(.session-search-input::placeholder) {
  color: var(--app-text-muted);
}

.search-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 0 0 auto;
  /* 固定到右侧 */
  margin-left: auto;
}


.empty-tip {
  color: var(--app-text-muted);
  text-align: center;
  padding: 20px;
  font-size: 11px;
  font-family: var(--app-font-family);
}

/* ── Virtual tree list ── */
.session-tree-viewport {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  background: transparent;
}

.tree-row {
  display: flex;
  align-items: center;
  gap: 2px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background-color 0.1s;
  font-family: var(--app-font-family);
  font-size: var(--app-font-size);
  color: var(--app-text);
  box-sizing: border-box;
}

.tree-row:hover {
  background-color: hsl(var(--accent) / 0.15);
  border: 1px dotted hsl(var(--border));
}

html.dark .tree-row:hover {
  background-color: hsl(var(--accent) / 0.2);
  border: 1px solid hsl(var(--border));
}

.tree-row.drag-over {
  border-top: 2px solid hsl(var(--primary));
}

.tree-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  font-size: 8px;
  color: var(--app-text-muted);
  transition: transform 0.15s;
  flex-shrink: 0;
}

.tree-arrow.expanded {
  transform: rotate(90deg);
}

.tree-drag-handle {
  cursor: grab;
  color: var(--app-text-muted);
  font-size: 14px;
  padding: 0 4px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.1s;
}

.tree-row:hover .tree-drag-handle {
  opacity: 0.6;
}

.tree-drag-handle:active {
  cursor: grabbing;
}

/* ── Existing tree node styling ── */

.tree-node-title {
  display: flex;
  align-items: center;
  width: 100%;
}

.node-meta {
  color: var(--app-text-muted);
  font-size: 0.9em;
  margin-left: 5px;
}

.match-mark {
  background: hsl(var(--primary) / 0.15);
  color: hsl(var(--foreground));
  padding: 0 1px;
  border-radius: 2px;
}

:deep(.ant-tree-node-content-wrapper) {
  border-radius: 0 !important;
  padding: 2px 4px !important;
  transition: none !important;
  /* Fast response */
  border: 1px solid transparent;
  /* Always have a border width to prevent layout jump */
  animation: none !important;
}

/* Keep hover/active/selected consistent grey */
</style>

<style>
/* Global styles for Context Menu — theme-aware via app tokens */
.custom-context-menu .ant-dropdown-menu {
  border-radius: 4px;
  border: 1px solid var(--app-border-shadow);
  padding: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  background: var(--app-bg-dialog);
}

.custom-context-menu .ant-dropdown-menu-item {
  font-size: 14px !important;
  padding: 4px 12px !important;
  border-radius: 3px;
  color: var(--app-text);
  transition: none;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif !important;
}

/* Hover — brand color highlight */
.custom-context-menu .ant-dropdown-menu-item:hover,
.custom-context-menu .ant-dropdown-menu-submenu-title:hover {
  background-color: var(--color-primary) !important;
  color: var(--color-primary-foreground) !important;
}

/* Danger items */
.custom-context-menu .ant-dropdown-menu-item-danger {
  color: var(--color-danger);
}

.custom-context-menu .ant-dropdown-menu-item-danger:hover {
  background-color: var(--color-danger) !important;
  color: var(--color-danger-foreground) !important;
}

/* Eliminate Wave Effect / Slow transitions */
[ant-click-animating-without-extra-node]:after {
  animation: none !important;
  display: none !important;
}

.ant-wave {
  display: none !important;
}
</style>
