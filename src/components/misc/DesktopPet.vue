<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  defaultDesktopPetAssetSrc,
  normalizeDesktopPetSettings,
  resolveDesktopPetAssetUrl
} from '@/utils/desktopPet';
import { isReducedPerformance } from '@/utils/performance';
import { TooltipHint } from '@/components/ui/tooltip';

const props = defineProps({
  settings: {
    type: Object,
    default: () => ({})
  },
  suspend: {
    type: Boolean,
    default: false
  },
  paused: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['settings-change']);

const settings = computed(() => normalizeDesktopPetSettings(props.settings || {}));
const shouldHide = computed(() => props.suspend && settings.value.autoHideOnModal);
const pageVisible = ref(!document.hidden);
const enabledNodes = computed(() => {
  const nodes = Array.isArray(settings.value.nodes) ? settings.value.nodes : [];
  return nodes.filter((node) => node.enabled !== false);
});
const isVisible = computed(() => settings.value.enabled && enabledNodes.value.length > 0
  && !shouldHide.value && !props.paused && pageVisible.value && !isReducedPerformance());

const activeNodeIndex = ref(0);
const renderedAssetUrl = ref('');
const isAssetLoading = ref(false);
const petFracTop = ref(0.01);
const petFracRight = ref(0.01);
const isDragging = ref(false);
const altHeld = ref(false);
const probeEdge = ref('');
const petHostRef = ref(null);
const SNAP_TOP = 10;
const SNAP_RIGHT = 12;
const SNAP_THRESHOLD = 22;
let nextNodeTimer = 0;
let dragState = null;
let dragFrame = 0;
let pendingDragPosition = null;
let disposed = false;
const lastHostW = ref(window.innerWidth);
const lastHostH = ref(window.innerHeight);

const petTopPx = computed(() => Math.round(petFracTop.value * lastHostH.value));
const petRightPx = computed(() => Math.round(petFracRight.value * lastHostW.value));

const activeNode = computed(() => enabledNodes.value[activeNodeIndex.value] || enabledNodes.value[0] || null);
const edgeProbeEnabled = computed(() => settings.value.edgeProbeEnabled !== false);
const edgeProbeMargin = computed(() => Math.max(8, Number(settings.value.edgeProbeMargin || 28)));
const probeScaleFactor = computed(() => (probeEdge.value && edgeProbeEnabled.value ? 0.82 : 1));
const allNodes = computed(() => Array.isArray(settings.value.nodes) ? settings.value.nodes : []);

const nodeIdsHash = computed(() => {
  const nodes = allNodes.value;
  if (!nodes.length) return '';
  return nodes.map((n) => `${n.id}|${n.enabled ? 1 : 0}`).join(',');
});

const resolveProbeNode = (edge) => {
  const map = {
    top: settings.value.edgeProbeNodeTop,
    right: settings.value.edgeProbeNodeRight,
    bottom: settings.value.edgeProbeNodeBottom,
    left: settings.value.edgeProbeNodeLeft
  };
  const targetId = (map[edge] || '').trim();
  if (!targetId) return null;
  return allNodes.value.find((n) => n.id === targetId && n.enabled !== false) || null;
};
const displayNode = computed(() => {
  if (probeEdge.value && edgeProbeEnabled.value) {
    const probeNode = resolveProbeNode(probeEdge.value);
    if (probeNode) return probeNode;
  }
  return activeNode.value || null;
});
const activeNodeAssetUrl = computed(() => resolveDesktopPetAssetUrl(displayNode.value?.src || defaultDesktopPetAssetSrc));
const stageWidth = computed(() => Math.round(156 * Number(settings.value.scale || 1)));
const stageHeight = computed(() => Math.round(156 * Number(settings.value.scale || 1)));

const nodeOffsetX = computed(() => Number(displayNode.value?.offsetX ?? 0));
const nodeOffsetY = computed(() => Number(displayNode.value?.offsetY ?? 0));
const nodeOwnScale = computed(() => Number(displayNode.value?.scale ?? 1));
const nodeFinalScale = computed(() => (nodeOwnScale.value * probeScaleFactor.value).toFixed(3));

const imageAlt = computed(() => displayNode.value?.name || '桌宠');
const dragHintVisible = computed(() => isVisible.value && !props.suspend
  && (!settings.value.clickThrough || altHeld.value || isDragging.value));
const dragButtonTitle = computed(() => isDragging.value ? '拖拽中' : '拖拽桌宠位置');

const transitionMs = computed(() => settings.value.transitionMs);
const transStr = computed(() => `${transitionMs.value}ms ease`);

// merged style computeds to reduce object allocation per tick
const petLayerStyle = computed(() => ({
  opacity: isVisible.value ? settings.value.opacity : 0,
  top: `${petTopPx.value}px`,
  right: `${petRightPx.value}px`,
  transition: `opacity ${transStr.value}`
}));

const petStageStyle = computed(() => ({
  transform: `scale(${settings.value.scale})`,
  transformOrigin: 'top right',
  transition: `transform ${transStr.value}, opacity ${transStr.value}`
}));

const petShellStyle = computed(() => ({
  transform: `translate(${nodeOffsetX.value}px, ${nodeOffsetY.value}px)`,
  transformOrigin: 'top right',
  transition: `transform ${transStr.value}`
}));

const activeNodeAssetStyle = computed(() => ({
  transform: `scale(${nodeFinalScale.value})`,
  transformOrigin: 'top right',
  transition: `transform ${transStr.value}, opacity ${transStr.value}`
}));

function clearNodeTimer() {
  if (nextNodeTimer) {
    clearTimeout(nextNodeTimer);
    nextNodeTimer = 0;
  }
}

function normalizeActiveNodeIndex() {
  const total = enabledNodes.value.length;
  if (!total) {
    activeNodeIndex.value = 0;
    return;
  }
  if (activeNodeIndex.value >= total) {
    activeNodeIndex.value = 0;
  }
}

function scheduleNextNode() {
  clearNodeTimer();
  if (!isVisible.value || isAssetLoading.value || isDragging.value || displayNode.value !== activeNode.value) return;
  const total = enabledNodes.value.length;
  if (total <= 1) return;
  const durationMs = Math.max(200, Number(activeNode.value?.durationMs || 2000));
  nextNodeTimer = window.setTimeout(() => {
    const latestTotal = enabledNodes.value.length || 1;
    activeNodeIndex.value = (activeNodeIndex.value + 1) % latestTotal;
  }, durationMs);
}

function syncPetPositionFromSettings() {
  lastHostW.value = window.innerWidth;
  lastHostH.value = window.innerHeight;
  const rawTop = Number(settings.value.positionTop ?? 10);
  const rawRight = Number(settings.value.positionRight ?? 12);
  // If value is < 1, treat as fraction (new format); otherwise legacy absolute px
  petFracTop.value = rawTop < 1 ? rawTop : rawTop / Math.max(1, lastHostH.value);
  petFracRight.value = rawRight < 1 ? rawRight : rawRight / Math.max(1, lastHostW.value);
  // Clamp before the first paint so the bottom default fits small windows and larger pet scales.
  const position = clampDragPosition(petTopPx.value, petRightPx.value);
  petFracTop.value = position.top / Math.max(1, lastHostH.value);
  petFracRight.value = position.right / Math.max(1, lastHostW.value);
}

function clampDragPosition(nextTop, nextRight) {
  const maxTop = Math.max(0, lastHostH.value - stageHeight.value - 8);
  const maxRight = Math.max(0, lastHostW.value - stageWidth.value - 8);
  return {
    top: Math.min(maxTop, Math.max(0, Math.round(nextTop))),
    right: Math.min(maxRight, Math.max(0, Math.round(nextRight)))
  };
}

function applyEdgeSnap(position) {
  const maxTop = Math.max(0, lastHostH.value - stageHeight.value - 8);
  const maxRight = Math.max(0, lastHostW.value - stageWidth.value - 8);
  const nextPosition = { ...position };

  if (Math.abs(nextPosition.top - SNAP_TOP) <= SNAP_THRESHOLD) {
    nextPosition.top = SNAP_TOP;
  } else if (Math.abs(nextPosition.top - maxTop) <= SNAP_THRESHOLD) {
    nextPosition.top = maxTop;
  }

  if (Math.abs(nextPosition.right - SNAP_RIGHT) <= SNAP_THRESHOLD) {
    nextPosition.right = SNAP_RIGHT;
  } else if (Math.abs(nextPosition.right - maxRight) <= SNAP_THRESHOLD) {
    nextPosition.right = maxRight;
  }

  return clampDragPosition(nextPosition.top, nextPosition.right);
}

function resolveProbeEdge(position) {
  if (!edgeProbeEnabled.value) return '';
  const left = lastHostW.value - position.right - stageWidth.value;
  const top = position.top;
  const right = position.right;
  const bottom = lastHostH.value - position.top - stageHeight.value;
  const distances = [
    ['top', top],
    ['right', right],
    ['bottom', bottom],
    ['left', left]
  ];
  const nearest = distances.reduce((acc, item) => (item[1] < acc[1] ? item : acc), distances[0]);
  return nearest[1] <= edgeProbeMargin.value ? nearest[0] : '';
}

function emitPositionSettings() {
  emit('settings-change', {
    ...settings.value,
    positionTop: petFracTop.value,
    positionRight: petFracRight.value
  });
}

let resizeHandle = 0;
function handleWindowResize() {
  if (!isVisible.value) return;
  if (resizeHandle) cancelAnimationFrame(resizeHandle);
  resizeHandle = requestAnimationFrame(() => {
    resizeHandle = 0;
    lastHostW.value = window.innerWidth;
    lastHostH.value = window.innerHeight;
    // Recalculate px from fraction, then clamp/snap for safety
    const nextPosition = applyEdgeSnap(clampDragPosition(petTopPx.value, petRightPx.value));
    petFracTop.value = nextPosition.top / Math.max(1, lastHostH.value);
    petFracRight.value = nextPosition.right / Math.max(1, lastHostW.value);
    probeEdge.value = resolveProbeEdge({ top: petTopPx.value, right: petRightPx.value });
  });
}

function stopDragging(commit = true) {
  if (!dragState) return;
  window.removeEventListener('mousemove', handleDragMove);
  window.removeEventListener('mouseup', handleDragEnd);
  if (dragFrame) {
    cancelAnimationFrame(dragFrame);
    dragFrame = 0;
  }
  const finalPosition = pendingDragPosition;
  const moved = dragState.moved;
  dragState = null;
  pendingDragPosition = null;
  if (isDragging.value && commit && moved && !disposed) {
    if (finalPosition) {
      petFracTop.value = finalPosition.top / Math.max(1, lastHostH.value);
      petFracRight.value = finalPosition.right / Math.max(1, lastHostW.value);
      probeEdge.value = resolveProbeEdge(finalPosition);
    }
    emitPositionSettings();
    nextTick(() => {
      if (disposed || dragState) return;
      petHostRef.value?.style.removeProperty('transform');
      isDragging.value = false;
    });
  } else {
    petHostRef.value?.style.removeProperty('transform');
    isDragging.value = false;
  }
}

function applyDragTransform() {
  dragFrame = 0;
  if (!dragState || !pendingDragPosition || !petHostRef.value) return;
  const nextProbeEdge = resolveProbeEdge(pendingDragPosition);
  if (nextProbeEdge !== probeEdge.value) probeEdge.value = nextProbeEdge;
  const translateX = dragState.startRightPx - pendingDragPosition.right;
  const translateY = pendingDragPosition.top - dragState.startTopPx;
  petHostRef.value.style.transform = `translate3d(${translateX}px, ${translateY}px, 0)`;
}

function handleDragMove(event) {
  if (!dragState) return;
  const deltaX = event.clientX - dragState.startClientX;
  const deltaY = event.clientY - dragState.startClientY;
  const nextPosition = applyEdgeSnap(
    clampDragPosition(
      dragState.startTopPx + deltaY,
      dragState.startRightPx - deltaX
    )
  );
  pendingDragPosition = nextPosition;
  dragState.moved = dragState.moved || deltaX !== 0 || deltaY !== 0;
  if (!dragFrame) dragFrame = requestAnimationFrame(applyDragTransform);
}

function handleDragEnd() {
  stopDragging();
}

function handleDragStart(event) {
  if (event.button !== 0 || !dragHintVisible.value || dragState) return;
  event.preventDefault();
  dragState = {
    startClientX: event.clientX,
    startClientY: event.clientY,
    startTopPx: petTopPx.value,
    startRightPx: petRightPx.value,
    moved: false
  };
  pendingDragPosition = {
    top: dragState.startTopPx,
    right: dragState.startRightPx
  };
  isDragging.value = true;
  window.addEventListener('mousemove', handleDragMove);
  window.addEventListener('mouseup', handleDragEnd);
}

function handleModifierKey(event) {
  if (event.key !== 'Alt') return;
  altHeld.value = event.type === 'keydown' && !event.ctrlKey && !event.metaKey;
}

function handleWindowBlur() {
  altHeld.value = false;
  stopDragging();
}

function handleVisibilityChange() {
  pageVisible.value = !document.hidden;
  if (!pageVisible.value) altHeld.value = false;
}

onMounted(() => {
  window.addEventListener('resize', handleWindowResize);
  window.addEventListener('keydown', handleModifierKey, true);
  window.addEventListener('keyup', handleModifierKey, true);
  window.addEventListener('blur', handleWindowBlur);
  document.addEventListener('visibilitychange', handleVisibilityChange);
});

watch(nodeIdsHash, normalizeActiveNodeIndex, { immediate: true });

watch([isVisible, nodeIdsHash, activeNodeIndex, isDragging, isAssetLoading,
  () => displayNode.value?.id, () => activeNode.value?.durationMs], scheduleNextNode, { immediate: true });

watch([isVisible, dragHintVisible], ([visible, canDrag], [wasVisible]) => {
  if (!canDrag) stopDragging();
  if (visible && !wasVisible) handleWindowResize();
});

watch([activeNodeAssetUrl, isVisible], async ([url, visible], previous, onCleanup) => {
  if (!visible) {
    renderedAssetUrl.value = '';
    isAssetLoading.value = false;
    return;
  }
  if (url === renderedAssetUrl.value) {
    isAssetLoading.value = false;
    return;
  }
  const image = new Image();
  let cancelled = false;
  onCleanup(() => {
    cancelled = true;
    image.src = '';
  });
  isAssetLoading.value = true;
  image.decoding = 'async';
  image.src = url;
  try {
    // Keep the current frame visible until the next asset is ready, without remounting the GIF.
    await image.decode();
    if (!cancelled) renderedAssetUrl.value = url;
  } catch {
    // A broken user asset must not blank an already usable pet or stop the remaining nodes.
  } finally {
    if (!cancelled) isAssetLoading.value = false;
  }
}, { immediate: true });

watch(
  () => [settings.value.positionTop, settings.value.positionRight, settings.value.scale].join('|'),
  () => {
    if (dragState) return;
    syncPetPositionFromSettings();
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  disposed = true;
  clearNodeTimer();
  stopDragging(false);
  window.removeEventListener('resize', handleWindowResize);
  window.removeEventListener('keydown', handleModifierKey, true);
  window.removeEventListener('keyup', handleModifierKey, true);
  window.removeEventListener('blur', handleWindowBlur);
  document.removeEventListener('visibilitychange', handleVisibilityChange);
  if (resizeHandle) cancelAnimationFrame(resizeHandle);
  if (dragFrame) cancelAnimationFrame(dragFrame);
});

</script>

<template>
  <div ref="petHostRef" v-if="displayNode && isVisible" class="desktop-pet-layer" :class="{ 'is-dragging': isDragging }"
    :style="petLayerStyle">
    <div class="desktop-pet-stage" :style="petStageStyle">
      <TooltipHint :text="dragHintVisible ? dragButtonTitle : ''" :disabled="isDragging || !dragHintVisible">
        <div class="desktop-pet-shell" :style="petShellStyle">
          <button v-if="renderedAssetUrl" class="desktop-pet-drag-surface" :class="{ 'is-pass-through': !dragHintVisible }"
            :style="activeNodeAssetStyle" :tabindex="dragHintVisible ? 0 : -1" type="button"
            aria-label="拖动桌宠（穿透开启时按住 Alt）" @mousedown="handleDragStart">
            <img class="desktop-pet-image" :src="renderedAssetUrl" :alt="imageAlt"
              decoding="async" draggable="false" />
          </button>
        </div>
      </TooltipHint>
    </div>
  </div>
</template>

<style scoped>
.desktop-pet-layer {
  position: fixed;
  z-index: 24;
  pointer-events: none;
}

.desktop-pet-layer.is-dragging {
  will-change: transform;
}

.desktop-pet-stage {
  position: relative;
  width: 156px;
  min-height: 156px;
}

.desktop-pet-shell {
  position: relative;
  display: flex;
  justify-content: flex-end;
  align-items: flex-start;
  min-height: 156px;
}

.desktop-pet-drag-surface {
  display: inline-flex;
  position: relative;
  z-index: 1;
  align-items: flex-start;
  justify-content: flex-end;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: grab;
  pointer-events: auto;
}

.desktop-pet-drag-surface:active {
  cursor: grabbing;
}

.desktop-pet-drag-surface.is-pass-through {
  pointer-events: none;
}

.desktop-pet-image {
  position: relative;
  z-index: 1;
  width: 132px;
  height: 132px;
  object-fit: contain;
  user-select: none;
  pointer-events: none;
}
</style>
