import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, onUnmounted, ref } from 'vue';

export function useSftpNativeDrop({
  enabled,
  resolveTarget,
  onDrop,
  onError
}) {
  const active = ref(false);
  const targetPath = ref('');
  const pathCount = ref(0);
  let scaleFactor = 1;
  let unlisten = null;
  let disposed = false;

  const reset = () => {
    active.value = false;
    targetPath.value = '';
    pathCount.value = 0;
  };

  const toLogicalPosition = (position) => {
    if (!position) return null;
    if (typeof position.toLogical === 'function') {
      return position.toLogical(scaleFactor);
    }
    return {
      x: Number(position.x || 0) / scaleFactor,
      y: Number(position.y || 0) / scaleFactor
    };
  };

  const updateTarget = (position) => {
    if (!enabled.value) {
      reset();
      return '';
    }
    const logical = toLogicalPosition(position);
    const target = logical ? resolveTarget(logical) : '';
    active.value = !!target;
    targetPath.value = target || '';
    return targetPath.value;
  };

  onMounted(async () => {
    try {
      scaleFactor = await getCurrentWindow().scaleFactor();
      if (disposed) return;
      unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === 'leave') {
          reset();
          return;
        }

        const target = updateTarget(payload.position);
        if (payload.type === 'enter') {
          pathCount.value = payload.paths?.length || 0;
          return;
        }
        if (payload.type !== 'drop') return;

        const paths = Array.isArray(payload.paths) ? payload.paths : [];
        reset();
        if (!target || !paths.length) return;
        Promise.resolve(onDrop({ paths, targetPath: target })).catch((error) => {
          onError?.(error);
        });
      });
    } catch (error) {
      onError?.(error);
    }
  });

  onUnmounted(() => {
    disposed = true;
    unlisten?.();
    unlisten = null;
    reset();
  });

  return {
    active,
    targetPath,
    pathCount,
    reset
  };
}
