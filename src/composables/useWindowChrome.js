import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, onUnmounted, ref } from 'vue';
import { isTauriRuntime } from '@/utils/ipc';
import {
  resolveWindowDragMode,
  resolveWindowPlatform,
  supportsNativeWindowRegion,
  WINDOW_DRAG_MODE_TAURI,
} from '@/utils/windowChrome';

const MAXIMIZED_SYNC_DELAY = 60;

function detectPlatform() {
  if (typeof navigator === 'undefined') return resolveWindowPlatform();

  return resolveWindowPlatform({
    userAgentDataPlatform: navigator.userAgentData?.platform,
    navigatorPlatform: navigator.platform,
    userAgent: navigator.userAgent,
  });
}

export function useWindowChrome() {
  const isMaximized = ref(false);
  const platform = detectPlatform();
  const dragMode = isTauriRuntime()
    ? resolveWindowDragMode(platform, supportsNativeWindowRegion())
    : WINDOW_DRAG_MODE_TAURI;
  const tauriDragRegion = dragMode === WINDOW_DRAG_MODE_TAURI ? 'deep' : undefined;

  let appWindow = null;
  let unlistenResize = null;
  let syncTimer = null;

  const syncMaximized = () => {
    clearTimeout(syncTimer);
    syncTimer = setTimeout(async () => {
      if (!appWindow) return;
      try {
        isMaximized.value = await appWindow.isMaximized();
      } catch (error) {
        console.warn('Synchronize window maximized state failed:', error);
      }
    }, MAXIMIZED_SYNC_DELAY);
  };

  const minimize = async () => {
    try {
      await appWindow?.minimize();
    } catch (error) {
      console.warn('Minimize window failed:', error);
    }
  };

  const toggleMaximize = async () => {
    try {
      await appWindow?.toggleMaximize();
      syncMaximized();
      window.dispatchEvent(new CustomEvent('window-chrome-state-changed'));
    } catch (error) {
      console.warn('Toggle window maximized state failed:', error);
    }
  };

  onMounted(async () => {
    document.documentElement.dataset.windowPlatform = platform;
    document.documentElement.dataset.windowDragMode = dragMode;

    if (!isTauriRuntime()) return;

    try {
      appWindow = getCurrentWindow();
      isMaximized.value = await appWindow.isMaximized();
      unlistenResize = await appWindow.onResized(syncMaximized);
    } catch (error) {
      console.warn('Initialize window chrome failed:', error);
    }
  });

  onUnmounted(() => {
    clearTimeout(syncTimer);
    unlistenResize?.();
    delete document.documentElement.dataset.windowPlatform;
    delete document.documentElement.dataset.windowDragMode;
  });

  return {
    dragMode,
    isMaximized,
    minimize,
    platform,
    tauriDragRegion,
    toggleMaximize,
  };
}
