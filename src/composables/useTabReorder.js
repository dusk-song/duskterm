import { onBeforeUnmount, ref, shallowRef } from 'vue';
import { findTabDropTarget } from '../utils/tabReorder.js';

export function useTabReorder({ listRef, itemRefs, onMove }) {
  const draggingId = ref(null);
  const dropTarget = shallowRef(null);
  const ghost = shallowRef(null);
  const ghostRef = ref(null);
  let gesture = null;
  let frame = 0;
  let suppressClick = false;
  let lastFrameTime = 0;

  const clear = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    const previous = gesture;
    gesture = null;
    if (previous?.element.hasPointerCapture?.(previous.pointerId)) {
      previous.element.releasePointerCapture(previous.pointerId);
    }
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', end);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('blur', cancel);
    window.removeEventListener('keydown', keydown, true);
    document.removeEventListener('visibilitychange', visibilityChange);
    draggingId.value = null;
    dropTarget.value = null;
    ghost.value = null;
  };

  const cancel = () => clear();
  const keydown = (event) => {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    cancel();
  };
  const visibilityChange = () => {
    if (document.hidden) cancel();
  };

  const measure = () => {
    const list = listRef.value;
    if (!gesture || !list) return;
    const bounds = list.getBoundingClientRect();
    gesture.bounds = bounds;
    gesture.rects = Array.from(itemRefs, ([id, element]) => {
      const rect = element.getBoundingClientRect();
      return {
        id,
        left: rect.left - bounds.left + list.scrollLeft,
        right: rect.right - bounds.left + list.scrollLeft,
        top: rect.top - bounds.top + list.scrollTop,
        bottom: rect.bottom - bounds.top + list.scrollTop
      };
    });
  };

  const renderFrame = (time) => {
    frame = 0;
    const current = gesture;
    const list = listRef.value;
    if (!current || !draggingId.value || !list) return;
    const { bounds, x, y } = current;
    const elapsed = Math.min(time - lastFrameTime || 16, 32);
    lastFrameTime = time;
    // Keep scrolling only near the list; dragging away from it cancels the drop target.
    const inside = x >= bounds.left - 20 && x <= bounds.right + 20
      && y >= bounds.top - 28 && y <= bounds.bottom + 28;
    const edge = Math.min(22, bounds.height / 3);
    const speed = inside ? y < bounds.top + edge
      ? -Math.min(1, (bounds.top + edge - y) / edge)
      : y > bounds.bottom - edge ? Math.min(1, (y - bounds.bottom + edge) / edge) : 0 : 0;
    if (speed) list.scrollTop += speed * elapsed * 0.5;
    const target = inside ? findTabDropTarget(current.rects, current.id,
      x - bounds.left + list.scrollLeft, y - bounds.top + list.scrollTop) : null;
    if (target?.id !== dropTarget.value?.id || target?.placement !== dropTarget.value?.placement) {
      dropTarget.value = target;
    }
    if (ghostRef.value) {
      ghostRef.value.style.transform = `translate3d(${x - current.offsetX}px, ${y - current.offsetY}px, 0)`;
    }
    if (speed) frame = requestAnimationFrame(renderFrame);
  };

  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(renderFrame);
  };

  function move(event) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (event.buttons === 0) { cancel(); return; }
    gesture.x = event.clientX;
    gesture.y = event.clientY;
    if (!draggingId.value) {
      if (Math.hypot(gesture.x - gesture.startX, gesture.y - gesture.startY) < 6) return;
      suppressClick = true;
      draggingId.value = gesture.id;
      ghost.value = { panel: gesture.panel, width: gesture.width };
      measure();
      lastFrameTime = 0;
      gesture.element.setPointerCapture?.(gesture.pointerId);
    }
    event.preventDefault();
    schedule();
  }

  function end(event) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (draggingId.value) {
      gesture.x = event.clientX;
      gesture.y = event.clientY;
      cancelAnimationFrame(frame);
      renderFrame(performance.now());
      const sourceId = gesture.id;
      const target = dropTarget.value;
      clear();
      if (target) onMove(sourceId, target.id, target.placement);
    } else clear();
  }

  const start = (event, panel) => {
    if (event.button !== 0 || !event.isPrimary) return;
    clear();
    // A fresh press, including on close, must never inherit the previous drag's click guard.
    suppressClick = false;
    if (event.target.closest('.terminal-tab-close')) return;
    const element = event.currentTarget;
    const rect = element.getBoundingClientRect();
    gesture = {
      id: panel.id, panel, element, pointerId: event.pointerId,
      x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY,
      offsetX: event.clientX - rect.left, offsetY: event.clientY - rect.top, width: rect.width
    };
    window.addEventListener('pointermove', move, { passive: false });
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('blur', cancel);
    window.addEventListener('keydown', keydown, true);
    document.addEventListener('visibilitychange', visibilityChange);
  };

  const captureClick = (event) => {
    if (!suppressClick) return;
    suppressClick = false;
    if (event.detail === 0) return;
    event.preventDefault();
    event.stopPropagation();
  };

  onBeforeUnmount(clear);
  return { draggingId, dropTarget, ghost, ghostRef, start, cancel, captureClick };
}
