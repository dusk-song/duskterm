<script setup>
import { Copy, Minus, Moon, Square, Sun, X } from '@lucide/vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { executeMenuAction } from '@/composables/useMenu';
import { useTheme } from '@/composables/useTheme';
import { isTauriRuntime } from '@/utils/ipc';
import { resolveTitlebarVisibility } from '@/utils/titlebarLayout';
import DuskDock from './DuskDock.vue';
import MonitorDock from './MonitorDock.vue';
import SessionDock from './SessionDock.vue';
import TransferDock from './TransferDock.vue';

const { isDark, toggleTheme } = useTheme();
const menus = [
  { key: 'file', label: '会话', items: [
    { key: 'file_new_conn', label: '新建连接', shortcut: 'Ctrl+N' },
    { key: 'file_open_session', label: '打开会话列表', shortcut: 'Ctrl+O' },
    { key: 'file_prefs', label: '首选项', shortcut: 'Ctrl+,' }, { type: 'divider' },
    { key: 'file_quit', label: '退出', shortcut: 'Alt+F4' },
  ] },
  { key: 'edit', label: '编辑', items: [
    { key: 'edit_copy', label: '复制', shortcut: 'Ctrl+Shift+C' },
    { key: 'edit_paste', label: '粘贴', shortcut: 'Ctrl+Shift+V' },
    { key: 'edit_select_all', label: '全选', shortcut: 'Ctrl+A' }, { type: 'divider' },
    { key: 'edit_clear', label: '清空屏幕', shortcut: 'Ctrl+Shift+L' },
    { key: 'edit_find', label: '查找...', shortcut: 'Ctrl+F' },
  ] },
  { key: 'view', label: '视图', items: [
    { key: 'view_tool_sessions', label: '切换会话列表', shortcut: 'F8' },
    { key: 'view_tool_sftp', label: '切换文件管理', shortcut: 'F9' },
    { key: 'view_fullscreen', label: '切换全屏', shortcut: 'F11' },
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

const titlebarRef = ref(null);
const width = ref(window.innerWidth);
const visibility = computed(() => resolveTitlebarVisibility(width.value));
const visibleMenus = computed(() => menus.filter((menu) => visibility.value.menus.includes(menu.key)));
const openKey = ref('');
const dropdownPos = ref({ top: 0, left: 0 });
const isMaximized = ref(false);
let win = null;
let resizeObserver = null;
let unlistenResize = null;
let syncTimer = null;

const closeMenu = () => { openKey.value = ''; };
const handleClick = (key) => { closeMenu(); executeMenuAction(key); };
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
const syncMaximized = () => {
  clearTimeout(syncTimer);
  syncTimer = setTimeout(async () => { if (win) isMaximized.value = await win.isMaximized(); }, 60);
};
async function initWindow() {
  if (!isTauriRuntime()) return;
  win = getCurrentWindow();
  isMaximized.value = await win.isMaximized();
  unlistenResize = await win.onResized(syncMaximized);
}
const winMax = async () => { await win?.toggleMaximize(); syncMaximized(); };
const onDoubleClick = (event) => { if (!event.target.closest('button,.tb-menu-item')) winMax(); };
const shortcuts = {};
menus.forEach((menu) => menu.items.forEach((item) => { if (item.key && item.shortcut) shortcuts[item.shortcut.replace(/\s+/g, '').toLowerCase()] = item.key; }));
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
  initWindow().catch((error) => console.warn('Initialize titlebar window bindings failed:', error));
  resizeObserver = new ResizeObserver(([entry]) => { width.value = entry.contentRect.width; });
  if (titlebarRef.value) resizeObserver.observe(titlebarRef.value);
  document.addEventListener('keydown', onKeydown, true);
  document.addEventListener('click', onDocumentClick, true);
});
onUnmounted(() => {
  resizeObserver?.disconnect(); unlistenResize?.(); clearTimeout(syncTimer);
  document.removeEventListener('keydown', onKeydown, true);
  document.removeEventListener('click', onDocumentClick, true);
});
</script>

<template>
  <header ref="titlebarRef" data-tauri-drag-region class="dusk-titlebar" @dblclick="onDoubleClick">
    <div class="titlebar-drag-layer" data-tauri-drag-region aria-hidden="true" />
    <div class="titlebar-left">
      <DuskDock class="menu-dock" interactive>
        <img src="/tauri.svg" class="app-icon" alt="DuskTerm" />
        <button v-for="menu in visibleMenus" :key="menu.key" class="tb-menu-item" :class="{ open: openKey === menu.key }"
          @click.stop="openMenu(menu.key, $event)" @mouseenter="hoverMenu(menu.key, $event)">{{ menu.label }}</button>
      </DuskDock>
    </div>
    <div class="titlebar-center"><SessionDock /></div>
    <div class="titlebar-right">
      <MonitorDock v-if="visibility.monitor" />
      <TransferDock v-show="visibility.transfer" />
      <DuskDock class="window-dock" interactive>
        <button class="tb-btn" @click="toggleTheme" :title="isDark ? '切换亮色主题' : '切换暗色主题'"><Sun v-if="isDark" :size="15" /><Moon v-else :size="15" /></button>
        <button class="tb-btn" @click="win?.minimize()" title="最小化"><Minus :size="13" /></button>
        <button class="tb-btn" @click="winMax" :title="isMaximized ? '还原' : '最大化'"><Copy v-if="isMaximized" :size="12" /><Square v-else :size="12" /></button>
        <button class="tb-btn close" @click="win?.close()" title="关闭"><X :size="14" /></button>
      </DuskDock>
    </div>
    <Teleport to="body">
      <div v-if="openKey" class="tb-dropdown" :style="dropdownStyle()" @click.stop>
        <template v-for="(item, index) in menus.find((menu) => menu.key === openKey)?.items" :key="item.key || index">
          <div v-if="item.type === 'divider'" class="tb-divider" />
          <button v-else class="tb-entry" @click="handleClick(item.key)"><span>{{ item.label }}</span><span class="tb-shortcut">{{ item.shortcut }}</span></button>
        </template>
      </div>
    </Teleport>
  </header>
</template>

<style scoped>
.dusk-titlebar { position: relative; display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr); align-items: center; height: 62px; padding: 0 7px; box-sizing: border-box; flex: 0 0 auto; background: transparent; user-select: none; z-index: var(--z-chrome); }
.titlebar-drag-layer { position: absolute; inset: 0; z-index: 0; }
.titlebar-left, .titlebar-center, .titlebar-right { transform: translateY(-10px); }
.titlebar-left, .titlebar-right { position: relative; z-index: 1; display: flex; align-items: center; min-width: 0; gap: 6px; }
.titlebar-left { justify-content: flex-start; }
.titlebar-right { justify-content: flex-end; }
.titlebar-center { position: relative; z-index: 1; justify-self: center; pointer-events: none; min-width: 0; }
.menu-dock { padding-left: 7px; }
.app-icon { width: 17px; height: 17px; margin-right: 4px; pointer-events: none; }
.tb-menu-item, .tb-btn { height: 24px; border: 0; border-radius: 5px; color: var(--tb-text, var(--app-text)); background: transparent; cursor: default; }
.tb-menu-item { padding: 0 7px; font-size: 12px; }
.tb-btn { display: inline-flex; width: 29px; align-items: center; justify-content: center; padding: 0; opacity: .78; }
.tb-menu-item:hover, .tb-menu-item.open, .tb-btn:hover { background: var(--tb-hover-bg, color-mix(in srgb, var(--app-text) 8%, transparent)); opacity: 1; }
.tb-btn.close:hover { color: #fff; background: var(--tb-close-hover, var(--color-danger)); }
.window-dock { padding: 0 2px; }
</style>

<style>
.tb-dropdown { min-width: 220px; padding: 5px; border: 1px solid var(--tb-dropdown-border, var(--app-border-shadow)); border-radius: 9px; background: color-mix(in srgb, var(--tb-dropdown-bg, var(--app-bg-dialog)) 95%, transparent); box-shadow: var(--niri-shadow-dialog); backdrop-filter: blur(12px); z-index: var(--z-dropdown); }
.tb-entry { display: flex; width: 100%; min-height: 28px; align-items: center; justify-content: space-between; padding: 0 9px; border: 0; border-radius: 6px; color: var(--tb-text, var(--app-text)); background: transparent; font-size: 12px; }
.tb-entry:hover { background: var(--tb-entry-hover, color-mix(in srgb, var(--app-text) 8%, transparent)); }
.tb-shortcut { margin-left: 22px; color: var(--tb-text-muted, var(--app-text-muted)); font-size: 11px; }
.tb-divider { height: 1px; margin: 3px 7px; background: var(--tb-divider, var(--app-border-shadow)); }
</style>
