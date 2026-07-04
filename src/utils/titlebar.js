/**
 * Injects a custom HTML menu into the decorum overlay titlebar.
 * Decorated by tauri-plugin-decorum, [data-tauri-decorum-tb] is created
 * by Rust before the webview loads. This script finds it and inserts menus.
 */
import { executeMenuAction } from '../composables/useMenu';

const menus = [
  {
    key: 'file', label: '文件', items: [
      { key: 'file_new_conn', label: '新建连接', shortcut: 'Ctrl+N' },
      { key: 'file_open_session', label: '打开会话列表', shortcut: 'Ctrl+O' },
      { key: 'file_save_session', label: '保存会话', shortcut: 'Ctrl+S' },
      { key: 'file_save_all', label: '保存全部活动会话', shortcut: 'Ctrl+Shift+S' },
      { type: 'divider' },
      { key: 'file_prefs', label: '首选项', shortcut: 'Ctrl+,' },
      { type: 'divider' },
      { key: 'file_quit', label: '退出', shortcut: 'Alt+F4' }
    ]
  },
  {
    key: 'edit', label: '编辑', items: [
      { key: 'edit_copy', label: '复制', shortcut: 'Ctrl+C' },
      { key: 'edit_paste', label: '粘贴', shortcut: 'Ctrl+V' },
      { key: 'edit_select_all', label: '全选', shortcut: 'Ctrl+A' },
      { type: 'divider' },
      { key: 'edit_clear', label: '清空屏幕', shortcut: 'Ctrl+Shift+L' },
      { key: 'edit_find', label: '查找...', shortcut: 'Ctrl+F' }
    ]
  },
  {
    key: 'view', label: '视图', items: [
      { key: 'view_zoom_in', label: '放大', shortcut: 'Ctrl+Plus' },
      { key: 'view_zoom_out', label: '缩小', shortcut: 'Ctrl+Minus' },
      { key: 'view_zoom_reset', label: '重置缩放', shortcut: 'Ctrl+0' },
      { type: 'divider' },
      { key: 'view_tool_sessions', label: '切换会话列表', shortcut: 'F8' },
      { key: 'view_tool_sftp', label: '切换文件管理', shortcut: 'F9' },
      { key: 'view_fullscreen', label: '切换全屏', shortcut: 'F11' },
      { key: 'view_refresh', label: '刷新当前视图', shortcut: 'F5' }
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

let openKey = '';

function closeAll() {
  openKey = '';
  document.querySelectorAll('.dm-dropdown').forEach(d => d.remove());
  document.querySelectorAll('.dm-item.open').forEach(el => el.classList.remove('open'));
}

function buildDropdown(menu) {
  const div = document.createElement('div');
  div.className = 'dm-dropdown';
  menu.items.forEach(entry => {
    if (entry.type === 'divider') {
      const sep = document.createElement('div');
      sep.className = 'dm-divider';
      div.appendChild(sep);
    } else {
      const row = document.createElement('div');
      row.className = 'dm-entry';
      const span = document.createElement('span');
      span.textContent = entry.label;
      row.appendChild(span);
      if (entry.shortcut) {
        const sc = document.createElement('span');
        sc.className = 'dm-shortcut';
        sc.textContent = entry.shortcut;
        row.appendChild(sc);
      }
      row.addEventListener('click', (e) => {
        e.stopPropagation();
        closeAll();
        executeMenuAction(entry.key);
      });
      div.appendChild(row);
    }
  });
  return div;
}

function onMenuClick(menu, el, e) {
  e.stopPropagation();
  if (openKey === menu.key) {
    closeAll();
    return;
  }
  closeAll();
  openKey = menu.key;
  el.classList.add('open');
  const dd = buildDropdown(menu);
  el.appendChild(dd);
}

function onMenuHover(menu, el) {
  if (openKey && openKey !== menu.key) {
    closeAll();
    openKey = menu.key;
    el.classList.add('open');
    el.appendChild(buildDropdown(menu));
  }
}

// ── Inject into decorum titlebar ──
function injectMenus() {
  const tb = document.querySelector('[data-tauri-decorum-tb]');
  if (!tb) return setTimeout(injectMenus, 50);

  // Prevent duplicate injection
  if (tb.querySelector('.dm-menus')) return;

  // App label
  const label = document.createElement('span');
  label.className = 'dm-app-label';
  label.textContent = 'DuskTerm';
  label.style.cssText = 'font-size:12px;font-weight:600;color:rgba(255,255,255,0.45);margin-right:8px;flex-shrink:0;';
  tb.insertBefore(label, tb.firstChild);

  // Menu container
  const container = document.createElement('div');
  container.className = 'dm-menus';
  container.style.cssText = 'display:flex;align-items:center;height:100%;';

  menus.forEach(menu => {
    const el = document.createElement('div');
    el.className = 'dm-item';
    el.textContent = menu.label;
    el.style.cssText = 'position:relative;padding:3px 8px;font-size:13px;color:#fff;cursor:default;border-radius:4px;user-select:none;';
    el.addEventListener('mouseenter', () => {
      el.style.background = 'rgba(255,255,255,0.1)';
      onMenuHover(menu, el);
    });
    el.addEventListener('mouseleave', () => {
      if (openKey !== menu.key) el.style.background = '';
    });
    el.addEventListener('click', (e) => onMenuClick(menu, el, e));
    container.appendChild(el);
  });

  tb.insertBefore(container, tb.querySelector('button') || tb.lastChild);

  // Close menus when clicking outside
  document.addEventListener('click', (e) => {
    if (!tb.contains(e.target)) closeAll();
  });
}

// ── Keyboard shortcuts ──
const shortcutMap = {};
menus.forEach(m => {
  m.items.forEach(item => {
    if (item.shortcut && item.key) {
      const norm = String(item.shortcut).trim().replace(/\s+/g, '').toLowerCase();
      shortcutMap[norm] = item.key;
    }
  });
});

document.addEventListener('keydown', (e) => {
  const tag = document.activeElement?.tagName?.toLowerCase();
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return;
  if (document.activeElement?.isContentEditable) return;

  const parts = [];
  if (e.ctrlKey || e.metaKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  let key = e.key;
  if (key === ' ') key = 'Space';
  if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  const combo = parts.join('+').toLowerCase();

  const actionKey = shortcutMap[combo];
  if (actionKey) {
    e.preventDefault();
    executeMenuAction(actionKey);
  }
}, true);

// ── Start injection ──
export function initTitlebar() {
  injectMenus();
}
