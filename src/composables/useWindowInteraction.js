import { nextTick, onMounted, onUnmounted, ref } from 'vue';

const WINDOW_RESIZE_SETTLE_DELAY = 240;
const WINDOW_RESIZE_STABLE_FRAMES = 4;
const WINDOW_RESIZE_MIN_QUIET_MS = 48;

function notifyTerminalLayoutDragging(dragging) {
  window.dispatchEvent(new CustomEvent('terminal-layout-dragging', {
    detail: {
      dragging,
      deferFit: true,
      source: 'window-resize',
    },
  }));
}

export function useWindowInteraction({ onResize, settleDelay = WINDOW_RESIZE_SETTLE_DELAY } = {}) {
  const isResizing = ref(false);
  let settleTimer = null;
  let settleFrame = null;
  let finishFrame = null;
  let finishGeneration = 0;
  let awaitingSettledLayout = false;
  let stableFrameCount = 0;
  let lastResizeAt = 0;
  let lastWidth = 0;
  let lastHeight = 0;

  const cancelSettleWatch = () => {
    if (settleFrame) cancelAnimationFrame(settleFrame);
    settleFrame = null;
    stableFrameCount = 0;
  };

  const cancelPendingFinish = () => {
    finishGeneration += 1;
    if (finishFrame) cancelAnimationFrame(finishFrame);
    finishFrame = null;
  };

  const notifySettledLayout = async () => {
    const generation = ++finishGeneration;
    onResize?.();
    await nextTick();

    finishFrame = requestAnimationFrame(() => {
      finishFrame = requestAnimationFrame(() => {
        finishFrame = null;
        if (generation !== finishGeneration || isResizing.value) return;
        awaitingSettledLayout = false;
        notifyTerminalLayoutDragging(false);
        window.dispatchEvent(new CustomEvent('terminal-layout-resize', {
          detail: { source: 'window-resize' },
        }));
      });
    });
  };

  const finishResize = () => {
    clearTimeout(settleTimer);
    settleTimer = null;
    cancelSettleWatch();
    if (!isResizing.value) return;

    isResizing.value = false;
    awaitingSettledLayout = true;
    void notifySettledLayout();
  };

  const watchForStableSize = () => {
    if (settleFrame) return;
    const checkSize = () => {
      settleFrame = null;
      if (!isResizing.value) return;

      const width = window.innerWidth;
      const height = window.innerHeight;
      if (width === lastWidth && height === lastHeight) {
        stableFrameCount += 1;
      } else {
        lastWidth = width;
        lastHeight = height;
        stableFrameCount = 0;
        onResize?.();
      }

      if (
        stableFrameCount >= WINDOW_RESIZE_STABLE_FRAMES
        && performance.now() - lastResizeAt >= WINDOW_RESIZE_MIN_QUIET_MS
      ) {
        finishResize();
        return;
      }
      settleFrame = requestAnimationFrame(checkSize);
    };
    settleFrame = requestAnimationFrame(checkSize);
  };

  const beginResize = () => {
    if (isResizing.value) return;
    isResizing.value = true;
    notifyTerminalLayoutDragging(true);
  };

  const handleResize = () => {
    cancelPendingFinish();
    awaitingSettledLayout = false;
    beginResize();

    lastResizeAt = performance.now();
    lastWidth = window.innerWidth;
    lastHeight = window.innerHeight;
    stableFrameCount = 0;
    onResize?.();
    clearTimeout(settleTimer);
    settleTimer = setTimeout(finishResize, settleDelay);
    watchForStableSize();
  };

  const handleWindowResume = () => {
    if (document.hidden) return;
    cancelPendingFinish();
    awaitingSettledLayout = false;
    beginResize();
    onResize?.();
    finishResize();
  };

  const handleVisibilityChange = () => {
    if (!document.hidden) handleWindowResume();
  };

  onMounted(() => {
    onResize?.();
    window.addEventListener('resize', handleResize, true);
    window.addEventListener('focus', handleWindowResume);
    window.addEventListener('window-chrome-state-changed', handleResize);
    document.addEventListener('visibilitychange', handleVisibilityChange);
  });

  onUnmounted(() => {
    const shouldReleaseLayout = isResizing.value || awaitingSettledLayout;
    window.removeEventListener('resize', handleResize, true);
    window.removeEventListener('focus', handleWindowResume);
    window.removeEventListener('window-chrome-state-changed', handleResize);
    document.removeEventListener('visibilitychange', handleVisibilityChange);
    clearTimeout(settleTimer);
    cancelSettleWatch();
    cancelPendingFinish();
    if (shouldReleaseLayout) notifyTerminalLayoutDragging(false);
  });

  return {
    finishResize,
    isResizing,
  };
}
