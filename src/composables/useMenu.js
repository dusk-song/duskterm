import { openUrl } from '@tauri-apps/plugin-opener';
import { onMounted, onUnmounted } from 'vue';
import { listenEvent } from '../utils/ipc';

const toggleFullscreen = async () => {
  if (!document.fullscreenElement) {
    await document.documentElement.requestFullscreen();
    return;
  }
  await document.exitFullscreen();
};

const menuHandlers = {
  // File
  'file_new_conn': () => {
    window.dispatchEvent(new CustomEvent('app:new-connection'));
  },
  'file_open_session': () => window.dispatchEvent(new CustomEvent('app:open-session-list')),
  'file_prefs': () => window.dispatchEvent(new CustomEvent('menu:file_prefs')),
  'file_save_session': () => window.dispatchEvent(new CustomEvent('app:save-current-session')),
  'file_save_all': () => window.dispatchEvent(new CustomEvent('app:save-all-sessions')),
  'file_quit': () => window.dispatchEvent(new CustomEvent('app:quit')),

  // View
  'view_toggle_sftp': () => window.dispatchEvent(new CustomEvent('app:toggle-tool-sftp')),
  'view_zoom_in': () => window.dispatchEvent(new CustomEvent('term:zoom-in')),
  'view_zoom_out': () => window.dispatchEvent(new CustomEvent('term:zoom-out')),
  'view_zoom_reset': () => window.dispatchEvent(new CustomEvent('term:zoom-reset')),
  'view_tool_sessions': () => window.dispatchEvent(new CustomEvent('app:toggle-tool-sessions')),
  'view_tool_sftp': () => window.dispatchEvent(new CustomEvent('app:toggle-tool-sftp')),
  'view_refresh': () => window.dispatchEvent(new CustomEvent('app:refresh-current-view')),
  'view_fullscreen': () => toggleFullscreen().catch(() => { }),

  // Edit (Most need to go to active terminal)
  'edit_copy': () => window.dispatchEvent(new CustomEvent('term:copy')),
  'edit_paste': () => window.dispatchEvent(new CustomEvent('term:paste')),
  'edit_select_all': () => window.dispatchEvent(new CustomEvent('term:select-all')),
  'edit_clear': () => window.dispatchEvent(new CustomEvent('term:clear')),
  'edit_find': () => window.dispatchEvent(new CustomEvent('app:terminal-find')),

  // Connection
  'conn_disconnect': () => window.dispatchEvent(new CustomEvent('app:disconnect-current')),
  'conn_disconnect_all': () => window.dispatchEvent(new CustomEvent('app:disconnect-all')),
  'conn_sync_input': () => window.dispatchEvent(new CustomEvent('app:open-sync-input')),
  'conn_tunnel': () => window.dispatchEvent(new CustomEvent('app:open-tunnel')),
  'conn_reconnect': () => window.dispatchEvent(new CustomEvent('app:reconnect-current')),
  'conn_reconnect_all': () => window.dispatchEvent(new CustomEvent('app:reconnect-all')),
  'conn_edit_session': () => window.dispatchEvent(new CustomEvent('app:edit-active-session')),

  // Help
  'help_github': () => { openUrl('https://github.com/dusk-song/DuskTerm').catch(() => { }); },
};

export function executeMenuAction(id) {
  if (menuHandlers[id]) {
    menuHandlers[id]();
    return true;
  }
  console.warn('Unhandled menu id:', id);
  return false;
}

export function useMenuHandler() {
  let unlisten = null;

  onMounted(async () => {
    unlisten = await listenEvent('menu-event', (id) => {
      console.log('Menu Action:', id);
      executeMenuAction(id);
    });
  });

  onUnmounted(() => {
    if (unlisten) unlisten();
  });
}
