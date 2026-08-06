import { onBeforeUnmount, onMounted, watch } from 'vue';

const VIEWPORT_MARGIN = 12;
const DRAG_HANDLE_SELECTOR = '[data-slot="dialog-header"]';
const INTERACTIVE_SELECTOR = 'button, a, input, textarea, select, label, [role="button"], [contenteditable="true"]';
const SMALL_VIEWPORT_MAX = 640;

const resolveElement = (value) => {
  if (value instanceof HTMLElement) return value;
  if (value?.$el instanceof HTMLElement) return value.$el;
  return null;
};

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export function useDialogDrag(elementRef, isEnabled) {
  let element = null;
  let observer = null;
  let frame = null;
  let activePointerId = null;
  let offsetX = 0;
  let offsetY = 0;
  let dragStart = null;

  const enabled = () => Boolean(isEnabled?.()) && window.innerWidth > SMALL_VIEWPORT_MAX;

  const applyOffset = () => {
    frame = null;
    if (!element) return;
    element.style.setProperty('--dialog-offset-x', `${offsetX}px`);
    element.style.setProperty('--dialog-offset-y', `${offsetY}px`);
  };

  const scheduleOffset = () => {
    if (frame !== null) return;
    frame = requestAnimationFrame(applyOffset);
  };

  const resetPosition = () => {
    offsetX = 0;
    offsetY = 0;
    dragStart = null;
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
    element?.removeAttribute('data-dialog-dragging');
    applyOffset();
  };

  const finishDragging = (event) => {
    if (activePointerId === null || (event && event.pointerId !== activePointerId)) return;
    if (element?.hasPointerCapture?.(activePointerId)) {
      element.releasePointerCapture(activePointerId);
    }
    activePointerId = null;
    dragStart = null;
    element?.removeAttribute('data-dialog-dragging');
  };

  const onPointerDown = (event) => {
    if (element !== event.currentTarget) observeElement(event.currentTarget);
    if (!enabled() || event.button !== 0 || activePointerId !== null) return;

    const target = event.target instanceof Element ? event.target : null;
    const handle = target?.closest(DRAG_HANDLE_SELECTOR);
    if (!handle || !element.contains(handle) || target.closest(INTERACTIVE_SELECTOR)) return;

    const rect = element.getBoundingClientRect();
    dragStart = {
      clientX: event.clientX,
      clientY: event.clientY,
      offsetX,
      offsetY,
      minDeltaX: VIEWPORT_MARGIN - rect.left,
      maxDeltaX: window.innerWidth - VIEWPORT_MARGIN - rect.right,
      minDeltaY: VIEWPORT_MARGIN - rect.top,
      maxDeltaY: window.innerHeight - VIEWPORT_MARGIN - rect.bottom,
    };
    activePointerId = event.pointerId;
    element.setPointerCapture?.(activePointerId);
    element.setAttribute('data-dialog-dragging', 'true');
    event.preventDefault();
  };

  const onPointerMove = (event) => {
    if (event.pointerId !== activePointerId || !dragStart) return;
    const deltaX = clamp(event.clientX - dragStart.clientX, dragStart.minDeltaX, dragStart.maxDeltaX);
    const deltaY = clamp(event.clientY - dragStart.clientY, dragStart.minDeltaY, dragStart.maxDeltaY);
    offsetX = dragStart.offsetX + deltaX;
    offsetY = dragStart.offsetY + deltaY;
    scheduleOffset();
    event.preventDefault();
  };

  const onDoubleClick = (event) => {
    if (element !== event.currentTarget) observeElement(event.currentTarget);
    if (!enabled()) return;
    const target = event.target instanceof Element ? event.target : null;
    const handle = target?.closest(DRAG_HANDLE_SELECTOR);
    if (!handle || !element?.contains(handle) || target.closest(INTERACTIVE_SELECTOR)) return;
    resetPosition();
  };

  const observeElement = (value) => {
    observer?.disconnect();
    observer = null;
    element = resolveElement(value);
    if (!element) return;
    resetPosition();
    observer = new MutationObserver(() => {
      if (element?.dataset.state === 'open') resetPosition();
    });
    observer.observe(element, { attributes: true, attributeFilter: ['data-state'] });
  };

  watch(elementRef, observeElement, { flush: 'post' });

  onMounted(() => window.addEventListener('resize', resetPosition));
  onBeforeUnmount(() => {
    finishDragging();
    observer?.disconnect();
    window.removeEventListener('resize', resetPosition);
    if (frame !== null) cancelAnimationFrame(frame);
  });

  return {
    onDoubleClick,
    onPointerCancel: finishDragging,
    onPointerDown,
    onPointerMove,
    onPointerUp: finishDragging,
    resetPosition,
  };
}
