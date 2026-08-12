<script setup>
import { Copy, FolderOpen, Minus, Moon, Square, Sun, X } from '@lucide/vue';
import { onMounted, onUnmounted, ref } from 'vue';
import { executeMenuAction } from '@/composables/useMenu';
import { useTheme } from '@/composables/useTheme';
import { useWindowChrome } from '@/composables/useWindowChrome';
import DuskDock from './DuskDock.vue';
import SessionDock from './SessionDock.vue';
import TransferDock from './TransferDock.vue';

const { isDark, toggleTheme } = useTheme();
const { isMaximized, minimize, tauriDragRegion, toggleMaximize } = useWindowChrome();
const props = defineProps({
  sftpActive: Boolean,
  sftpDisabled: Boolean,
  knowledgeActive: Boolean,
  sessionPanelActive: Boolean,
  transferVisible: Boolean,
  keybindings: {
    type: Object,
    default: () => ({})
  },
});
const emit = defineEmits(['toggle-sftp', 'toggle-transfer']);
const menus = [
  { key: 'file', label: '会话', items: [
    { key: 'file_new_conn', label: '新建连接', shortcut: 'Ctrl+N' },
    { key: 'file_prefs', label: '首选项', shortcut: 'Ctrl+,' }, { type: 'divider' },
    { key: 'file_quit', label: '退出', shortcut: 'Alt+F4' },
  ] },
  { key: 'edit', label: '编辑', items: [
    { key: 'edit_copy', label: '复制', shortcut: 'Ctrl+Shift+C' },
    { key: 'edit_paste', label: '粘贴', shortcut: 'Ctrl+Shift+V' },
    { key: 'edit_select_all', label: '全选', shortcut: 'Ctrl+A' }, { type: 'divider' },
    { key: 'edit_clear', label: '清空屏幕', shortcut: 'Ctrl+Shift+L' },
    { key: 'edit_find', label: '查找...', bindingKey: 'toggleFind' },
  ] },
  { key: 'view', label: '视图', items: [
    { key: 'view_tool_sessions', label: '会话列表', bindingKey: 'sessionList' },
    { key: 'view_tool_sftp', label: '文件管理', bindingKey: 'sftpPanel' },
    { key: 'view_tool_knowledge', label: '命令知识库', bindingKey: 'commandKnowledge' },
    { key: 'view_transfer_list', label: '传输列表', bindingKey: 'transferList' },
  ] },
  { key: 'connection', label: '连接', items: [
    { key: 'conn_reconnect', label: '重连当前会话', shortcut: 'Ctrl+R' },
    { key: 'conn_disconnect', label: '断开当前会话', shortcut: 'Ctrl+D' }, { type: 'divider' },
    { key: 'conn_sync_input', label: '同步输入...', shortcut: 'Ctrl+Shift+I' },
    { key: 'conn_tunnel', label: '隧道管理...', shortcut: 'Ctrl+Alt+T' }, { type: 'divider' },
    { key: 'conn_reconnect_all', label: '重连所有', shortcut: 'Ctrl+Shift+R' },
    { key: 'conn_disconnect_all', label: '断开所有', shortcut: 'Ctrl+Shift+D' },
    { key: 'conn_edit_session', label: '编辑当前会话...', shortcut: 'Ctrl+Alt+E' },
  ] },
  { key: 'help', label: '帮助', items: [{ key: 'help_github', label: 'GitHub', shortcut: 'F1' }] },
];

const openKey = ref('');
const dropdownPos = ref({ top: 0, left: 0 });
const titlebarLeftRef = ref(null);
const titlebarRightRef = ref(null);
const titlebarSideClearance = ref(480);
let titlebarSidesObserver = null;

const updateTitlebarSideClearance = () => {
  const leftWidth = titlebarLeftRef.value?.getBoundingClientRect().width || 0;
  const rightWidth = titlebarRightRef.value?.getBoundingClientRect().width || 0;
  // Includes the 5px titlebar edge padding and the 6px gap around the centered dock.
  const next = Math.ceil((Math.max(leftWidth, rightWidth) + 11) * 2);
  if (next > 0 && next !== titlebarSideClearance.value) titlebarSideClearance.value = next;
};

const closeMenu = () => { openKey.value = ''; };
const itemChecked = (key) => ({
  view_tool_sessions: props.sessionPanelActive,
  view_tool_sftp: props.sftpActive,
  view_tool_knowledge: props.knowledgeActive,
  view_transfer_list: props.transferVisible,
}[key] === true);
const itemDisabled = (key) => key === 'view_tool_sftp' && props.sftpDisabled;
const handleClick = (key) => {
  if (itemDisabled(key)) return;
  closeMenu();
  executeMenuAction(key);
};
function openMenu(key, event) {
  if (openKey.value === key) return closeMenu();
  const rect = event.currentTarget.getBoundingClientRect();
  dropdownPos.value = { top: rect.bottom + 4, left: rect.left };
  openKey.value = key;
}
function hoverMenu(key, event) {
  if (!openKey.value || openKey.value === key) return;
  const rect = event.currentTarget.getBoundingClientRect();
  dropdownPos.value = { top: rect.bottom + 4, left: rect.left };
  openKey.value = key;
}
const dropdownStyle = () => ({ position: 'fixed', top: `${dropdownPos.value.top}px`, left: `${dropdownPos.value.left}px` });
const itemShortcut = (item) => item?.bindingKey
  ? String(props.keybindings[item.bindingKey] || '')
  : String(item?.shortcut || '');
const shortcuts = {};
menus.forEach((menu) => menu.items.forEach((item) => {
  if (item.key && item.shortcut) {
    shortcuts[item.shortcut.replace(/\s+/g, '').toLowerCase()] = item.key;
  }
}));
function onKeydown(event) {
  if (['input', 'textarea', 'select'].includes(document.activeElement?.tagName?.toLowerCase())) return;
  const parts = [];
  if (event.ctrlKey || event.metaKey) parts.push('Ctrl');
  if (event.shiftKey) parts.push('Shift');
  if (event.altKey) parts.push('Alt');
  parts.push(event.key.length === 1 ? event.key.toUpperCase() : event.key);
  const action = shortcuts[parts.join('+').toLowerCase()];
  if (action) { event.preventDefault(); executeMenuAction(action); }
  if (event.key === 'Escape') closeMenu();
}
function onDocumentClick(event) {
  if (!event.target.closest('.tb-menu-item') && !event.target.closest('.tb-dropdown')) closeMenu();
}
onMounted(() => {
  document.addEventListener('keydown', onKeydown, true);
  document.addEventListener('click', onDocumentClick, true);
  titlebarSidesObserver = new ResizeObserver(updateTitlebarSideClearance);
  if (titlebarLeftRef.value) titlebarSidesObserver.observe(titlebarLeftRef.value);
  if (titlebarRightRef.value) titlebarSidesObserver.observe(titlebarRightRef.value);
  updateTitlebarSideClearance();
});
onUnmounted(() => {
  document.removeEventListener('keydown', onKeydown, true);
  document.removeEventListener('click', onDocumentClick, true);
  titlebarSidesObserver?.disconnect();
  titlebarSidesObserver = null;
});
</script>

<template>
  <header class="dusk-titlebar window-drag-region"
    :style="{ '--titlebar-side-clearance': `${titlebarSideClearance}px` }"
    :data-tauri-drag-region="tauriDragRegion">
    <div ref="titlebarLeftRef" class="titlebar-left">
      <DuskDock class="menu-dock" interactive>
        <img src="/tauri.svg" class="app-icon" alt="DuskTerm" draggable="false" />
        <button v-for="menu in menus" :key="menu.key" class="tb-menu-item" :class="{ open: openKey === menu.key }"
          @click.stop="openMenu(menu.key, $event)" @mouseenter="hoverMenu(menu.key, $event)">{{ menu.label }}</button>
      </DuskDock>
    </div>
    <div class="titlebar-center">
      <SessionDock />
    </div>
    <div ref="titlebarRightRef" class="titlebar-right">
      <DuskDock class="utility-dock" interactive>
        <TransferDock embedded :expanded="props.transferVisible"
          @toggle="emit('toggle-transfer')" />
        <button class="tb-btn" :class="{ active: props.sftpActive }" :disabled="props.sftpDisabled"
          title="文件管理（F9）" @click="emit('toggle-sftp')"><FolderOpen :size="14" /></button>
        <button class="tb-btn" @click="toggleTheme" :title="isDark ? '切换亮色主题' : '切换暗色主题'"><Sun v-if="isDark" :size="15" /><Moon v-else :size="15" /></button>
        <button class="tb-btn" @click="minimize" title="最小化"><Minus :size="13" /></button>
        <button class="tb-btn" @click="toggleMaximize" :title="isMaximized ? '还原' : '最大化'"><Copy v-if="isMaximized" :size="12" /><Square v-else :size="12" /></button>
        <button class="tb-btn close" @click="executeMenuAction('file_quit')" title="关闭"><X :size="14" /></button>
      </DuskDock>
    </div>
    <Teleport to="body">
      <div v-if="openKey" class="tb-dropdown" :style="dropdownStyle()" @click.stop>
        <template v-for="(item, index) in menus.find((menu) => menu.key === openKey)?.items" :key="item.key || index">
          <div v-if="item.type === 'divider'" class="tb-divider" />
          <button v-else class="tb-entry" :class="{ checked: itemChecked(item.key) }"
            :disabled="itemDisabled(item.key)" @click="handleClick(item.key)">
            <span class="tb-entry-label"><span class="tb-check">{{ itemChecked(item.key) ? '✓' : '' }}</span>{{ item.label }}</span>
            <span v-if="itemShortcut(item)" class="tb-shortcut">{{ itemShortcut(item) }}</span>
          </button>
        </template>
      </div>
    </Teleport>
  </header>
</template>

<style scoped>
.dusk-titlebar { position: relative; display: flex; align-items: center; justify-content: space-between; height: 46px; padding: 0 5px; box-sizing: border-box; flex: 0 0 auto; gap: 6px; background: transparent; user-select: none; z-index: var(--z-chrome); }
.titlebar-left, .titlebar-right { position: relative; z-index: 1; display: flex; align-items: center; min-width: max-content; gap: 6px; }
.titlebar-left { justify-content: flex-start; }
.titlebar-right { justify-content: flex-end; }
.titlebar-center { position: absolute; z-index: 0; left: 50%; display: flex; width: min(260px, max(0px, calc(100% - var(--titlebar-side-clearance, 480px)))); min-width: 0; align-items: center; justify-content: center; overflow: hidden; transform: translateX(-50%); pointer-events: none; }
.menu-dock { width: auto; max-width: none; flex: 0 0 auto; padding: 0 8px; }
.app-icon { width: 17px; height: 17px; margin-right: 4px; flex: 0 0 auto; }
.tb-menu-item, .tb-btn { height: 24px; border: 0; border-radius: 999px; color: var(--tb-text, var(--app-text)); background: transparent; cursor: default; }
.tb-menu-item { flex: 0 0 auto; padding: 0 7px; font-size: 12px; white-space: nowrap; }
.tb-btn { display: inline-flex; width: 29px; flex: 0 0 29px; align-items: center; justify-content: center; padding: 0; opacity: .78; }
.tb-menu-item:hover, .tb-menu-item.open, .tb-btn:hover, .tb-btn.active { background: var(--tb-hover-bg, color-mix(in srgb, var(--app-text) 8%, transparent)); opacity: 1; }
.tb-btn.active { color: var(--color-primary); }
.tb-btn:disabled { opacity: .3; cursor: not-allowed; }
.tb-btn.close:hover { color: var(--tb-close-hover-text, var(--color-danger-foreground)); background: var(--tb-close-hover, var(--color-danger)); }
.utility-dock { min-width: max-content; flex: 0 0 auto; padding: 0 4px; }
.window-dock { padding: 0 4px; }
</style>

<style>
html[data-window-drag-mode="native-region"] .window-drag-region {
  -webkit-app-region: drag;
  app-region: drag;
}
html[data-window-drag-mode="native-region"] .window-drag-region :is(button, a, input, select, textarea, label, [role="button"], [contenteditable="true"]),
html[data-window-drag-mode="native-region"] .window-no-drag {
  -webkit-app-region: no-drag;
  app-region: no-drag;
}
.tb-dropdown { min-width: 220px; padding: 5px; border: 1px solid var(--tb-dropdown-border, var(--app-border-shadow)); border-radius: 9px; background: color-mix(in srgb, var(--tb-dropdown-bg, var(--app-bg-dialog)) 95%, transparent); box-shadow: var(--niri-shadow-dialog); backdrop-filter: blur(12px); z-index: var(--z-dropdown); }
.tb-entry { display: flex; width: 100%; min-height: 28px; align-items: center; justify-content: space-between; padding: 0 9px; border: 0; border-radius: 6px; color: var(--tb-text, var(--app-text)); background: transparent; font-size: 12px; }
.tb-entry:hover { background: var(--tb-entry-hover, color-mix(in srgb, var(--app-text) 8%, transparent)); }
.tb-entry:disabled { opacity: .38; }
.tb-entry-label { display: inline-flex; align-items: center; }
.tb-check { display: inline-block; width: 16px; color: var(--color-primary); }
.tb-shortcut { margin-left: 22px; color: var(--tb-text-muted, var(--app-text-muted)); font-size: 11px; }
.tb-divider { height: 1px; margin: 3px 7px; background: var(--tb-divider, var(--app-border-shadow)); }
</style>
