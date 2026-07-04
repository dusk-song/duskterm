<script setup>
import { Copy, Minus, Moon, Square, Sun, X } from '@lucide/vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, onUnmounted, ref } from 'vue';
import { executeMenuAction } from '@/composables/useMenu';
import { useTheme } from '@/composables/useTheme';
import { isTauriRuntime } from '@/utils/ipc';

const { isDark, toggleTheme } = useTheme();

const menus = [
  {
    key: 'file', label: '会话', items: [
      { key: 'file_new_conn', label: '新建连接', shortcut: 'Ctrl+N' },
      { key: 'file_open_session', label: '打开会话列表', shortcut: 'Ctrl+O' },
      // { key: 'file_save_session', label: '保存会话', shortcut: 'Ctrl+S' },
      // { key: 'file_save_all', label: '保存全部活动会话', shortcut: 'Ctrl+Shift+S' },
      // { type: 'divider' },
      { key: 'file_prefs', label: '首选项', shortcut: 'Ctrl+,' },
      { type: 'divider' },
      { key: 'file_quit', label: '退出', shortcut: 'Alt+F4' }
    ]
  },
  {
    key: 'edit', label: '编辑', items: [
      { key: 'edit_copy', label: '复制', shortcut: 'Ctrl+Shift+C' },
      { key: 'edit_paste', label: '粘贴', shortcut: 'Ctrl+Shift+V' },
      { key: 'edit_select_all', label: '全选', shortcut: 'Ctrl+A' },
      { type: 'divider' },
      { key: 'edit_clear', label: '清空屏幕', shortcut: 'Ctrl+Shift+L' },
      { key: 'edit_find', label: '查找...', shortcut: 'Ctrl+F' }
    ]
  },
  {
    key: 'view', label: '视图', items: [
      // { key: 'view_zoom_in', label: '放大', shortcut: 'Ctrl+Plus' },
      // { key: 'view_zoom_out', label: '缩小', shortcut: 'Ctrl+Minus' },
      // { key: 'view_zoom_reset', label: '重置缩放', shortcut: 'Ctrl+0' },
      // { type: 'divider' },
      { key: 'view_tool_sessions', label: '切换会话列表', shortcut: 'F8' },
      { key: 'view_tool_sftp', label: '切换文件管理', shortcut: 'F9' },
      { key: 'view_fullscreen', label: '切换全屏', shortcut: 'F11' },
      // { key: 'view_refresh', label: '刷新当前视图', shortcut: 'F5' }
    ]
  },
  {
    key: 'connection', label: '连接', items: [
      { key: 'conn_reconnect', label: '重连当前会话', shortcut: 'Ctrl+R' },
      { key: 'conn_disconnect', label: '断开当前会话', shortcut: 'Ctrl+D' },
      { type: 'divider' },
      { key: 'conn_sync_input', label: '同步输入...', shortcut: 'Ctrl+Shift+I' },
      { key: 'conn_tunnel', label: '隧道管理...', shortcut: 'Ctrl+Alt+T' },
      { type: 'divider' },
      { key: 'conn_reconnect_all', label: '重连所有', shortcut: 'Ctrl+Shift+R' },
      { key: 'conn_disconnect_all', label: '断开所有', shortcut: 'Ctrl+Shift+D' },
      { type: 'divider' },
      { key: 'conn_edit_session', label: '编辑当前会话...', shortcut: 'Ctrl+Alt+E' }
    ]
  },
  {
    key: 'help', label: '帮助', items: [
      { key: 'help_github', label: 'GitHub', shortcut: 'F1' }
    ]
  }
];

const openKey = ref('');
const dropdownPos = ref({ top: 0, left: 0 });

function closeMenu() { openKey.value = ''; }

function handleClick(key) { closeMenu(); executeMenuAction(key); }

function toggleMenu(key, event) {
  if (openKey.value === key) { closeMenu(); return; }
  const rect = event.currentTarget.getBoundingClientRect();
  dropdownPos.value = { top: rect.bottom, left: rect.left };
  openKey.value = key;
}

function onHover(key, event) {
  if (!openKey.value || openKey.value === key) return;
  openKey.value = key;
  const rect = event.currentTarget.getBoundingClientRect();
  dropdownPos.value = { top: rect.bottom, left: rect.left };
}

function dropdownStyle(key) {
  if (openKey.value !== key) return { display: 'none' };
  return {
    position: 'fixed',
    top: dropdownPos.value.top + 'px',
    left: dropdownPos.value.left + 'px',
  };
}

// ── Click outside / Escape to close ──
function onDocClick(e) {
  if (!openKey.value) return;
  if (!e.target.closest('.tb-menu-item') && !e.target.closest('.tb-dropdown')) {
    closeMenu();
  }
}
function onDocKey(e) {
  if (e.key === 'Escape' && openKey.value) { closeMenu(); e.preventDefault(); }
}

// ── Window state ──
const isMaximized = ref(false);
let win = null;
let unlistenResize = null;
let removeFocusSync = null;

async function initWin() {
  if (!isTauriRuntime()) return;
  try {
    win = getCurrentWindow();
    // Initialize synchronously from actual window state
    isMaximized.value = await win.isMaximized();
    // Listen for resize to keep state in sync
    unlistenResize = await win.onResized(async () => {
      isMaximized.value = await win.isMaximized();
    });
    // Fallback: also sync on native window resize event (handles edge cases)
    const syncState = async () => {
      try { isMaximized.value = await win.isMaximized(); } catch { /* noop */ }
    };
    window.addEventListener('focus', syncState);
    removeFocusSync = () => window.removeEventListener('focus', syncState);
  } catch (error) {
    console.warn('Initialize titlebar window bindings failed:', error);
    win = null;
  }
}

async function winMin() { win?.minimize(); }
async function winMax() {
  if (!win) return;
  await win.toggleMaximize();
  isMaximized.value = await win.isMaximized();
}
async function winClose() { win?.close(); }

async function onDblClick(e) {
  if (e.target.closest('.tb-btn,.tb-menu-item')) return;
  win?.toggleMaximize();
}

// ── Keyboard shortcuts ──
const shortcuts = {};
menus.forEach(m => m.items.forEach(item => {
  if (item.shortcut && item.key) shortcuts[String(item.shortcut).replace(/\s+/g, '').toLowerCase()] = item.key;
}));
function onKeydown(e) {
  const tag = document.activeElement?.tagName?.toLowerCase();
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return;
  const p = [];
  if (e.ctrlKey || e.metaKey) p.push('Ctrl');
  if (e.shiftKey) p.push('Shift');
  if (e.altKey) p.push('Alt');
  let k = e.key; if (k === ' ') k = 'Space'; if (k.length === 1) k = k.toUpperCase();
  p.push(k);
  const a = shortcuts[p.join('+').toLowerCase()];
  if (a) { e.preventDefault(); executeMenuAction(a); }
}

onMounted(() => {
  initWin();
  document.addEventListener('keydown', onKeydown, true);
  document.addEventListener('click', onDocClick, true);
  document.addEventListener('keydown', onDocKey, true);
});
onUnmounted(() => {
  unlistenResize?.();
  removeFocusSync?.();
  unlistenResize = null;
  removeFocusSync = null;
  document.removeEventListener('keydown', onKeydown, true);
  document.removeEventListener('click', onDocClick, true);
  document.removeEventListener('keydown', onDocKey, true);
});
</script>

<template>
  <div data-tauri-drag-region class="dusk-titlebar" @dblclick="onDblClick">
    <div class="tb-left">
      <img src="/tauri.svg" class="tb-app-icon" alt="DuskTerm" />
      <div class="tb-menus">
        <div v-for="m in menus" :key="m.key" class="tb-menu-item" :class="{ open: openKey === m.key }"
          @click.stop="toggleMenu(m.key, $event)" @mouseenter="onHover(m.key, $event)">
          {{ m.label }}
          <Teleport to="body">
            <div v-if="openKey === m.key" class="tb-dropdown" :style="dropdownStyle(m.key)" @click.stop>
              <template v-for="(e, ei) in m.items" :key="e.key || `s${ei}`">
                <div v-if="e.type === 'divider'" class="tb-divider"></div>
                <div v-else class="tb-entry" @click="handleClick(e.key)">
                  <span>{{ e.label }}</span>
                  <span v-if="e.shortcut" class="tb-shortcut">{{ e.shortcut }}</span>
                </div>
              </template>
            </div>
          </Teleport>
        </div>
      </div>
    </div>
    <div class="tb-controls">
      <button class="tb-btn tb-theme-btn" @click="toggleTheme()" :title="isDark ? '切换亮色主题' : '切换暗色主题'">
        <Sun v-if="isDark" class="tb-theme-icon" :size="16" />
        <Moon v-else class="tb-theme-icon" :size="16" />
      </button>
      <button class="tb-btn" @click="winMin" title="最小化">
        <Minus class="tb-window-icon" :size="13" stroke-width="1.8" />
      </button>
      <button class="tb-btn" @click="winMax" :title="isMaximized ? '还原' : '最大化'">
        <Copy v-if="isMaximized" class="tb-window-icon" :size="12" stroke-width="1.8" />
        <Square v-else class="tb-window-icon" :size="12" stroke-width="1.8" />
      </button>
      <button class="tb-btn tb-close" @click="winClose" title="关闭">
        <X class="tb-window-icon" :size="13" stroke-width="1.9" />
      </button>
    </div>
  </div>
</template>

<style scoped>
/* ── Optimized titlebar ── */
.dusk-titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 38px;
  flex-shrink: 0;
  background: var(--tb-bg, #282C34);
  border-bottom: 1px solid var(--tb-divider, rgba(255, 255, 255, 0.08));
  user-select: none;
}

.tb-left {
  display: flex;
  align-items: center;
  height: 100%;
}

.tb-app-icon {
  width: 18px;
  height: 18px;
  margin: 0 8px 0 10px;
  flex-shrink: 0;
  pointer-events: none;
}

/* ── Menus ── */
.tb-menus {
  display: flex;
  align-items: center;
  height: 100%;
}

.tb-menu-item {
  position: relative;
  display: inline-flex;
  align-items: center;
  height: 24px;
  padding: 0 8px;
  font-size: 13px;
  color: var(--tb-text, #383A42);
  cursor: default;
  border-radius: 7px;
}

.tb-menu-item:hover,
.tb-menu-item.open {
  background: var(--tb-hover-bg, rgba(0, 0, 0, 0.06));
}

/* Dropdown — teleported to body, use :global() so styles apply outside scoped component */
:global(.tb-dropdown) {
  min-width: 220px;
  background: var(--tb-dropdown-bg, #2C313A);
  border: 1px solid var(--tb-dropdown-border, rgba(255, 255, 255, 0.08));
  border-radius: 10px;
  padding: 5px;
  z-index: var(--z-dropdown);
  box-shadow: var(--niri-shadow-dialog);
}

:global(.tb-entry) {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 28px;
  padding: 0 10px;
  font-size: 13px;
  color: var(--tb-text, rgba(255, 255, 255, 0.9));
  cursor: default;
  border-radius: 7px;
}

:global(.tb-entry:hover) {
  background: var(--tb-entry-hover, rgba(228, 224, 216, 0.08));
  color: var(--tb-text, rgba(255, 255, 255, 0.9));
}

:global(.tb-shortcut) {
  font-size: 12px;
  color: var(--tb-text-muted, rgba(255, 255, 255, 0.4));
  margin-left: 24px;
}

:global(.tb-divider) {
  height: 1px;
  background: var(--tb-divider, rgba(255, 255, 255, 0.08));
  margin: 3px 8px;
}

/* ── Window controls ── */
.tb-controls {
  display: flex;
  align-items: center;
  height: 100%;
}

.tb-btn {
  width: 34px;
  height: 28px;
  margin: 5px 2px;
  border: none;
  background: transparent;
  cursor: default;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border-radius: 7px;
}

.tb-window-icon {
  color: var(--tb-text, rgba(255, 255, 255, 0.6));
  opacity: 0.72;
  pointer-events: none;
}

.tb-btn:hover {
  background: var(--tb-hover-bg, rgba(255, 255, 255, 0.08));
}

.tb-btn:hover .tb-window-icon {
  opacity: 0.9;
  color: var(--tb-text, #C8D2E1);
}

.tb-theme-btn {
  width: 36px;
}

.tb-theme-icon {
  color: var(--tb-text, rgba(255, 255, 255, 0.6));
  transition: color 0.2s;
}

.tb-theme-btn:hover .tb-theme-icon {
  color: var(--tb-text, #C8D2E1);
  opacity: 1;
}

.tb-close:hover {
  background: var(--tb-close-hover, #E06C75);
}

.tb-close:hover .tb-window-icon {
  opacity: 1;
  color: #fff;
}
</style>
