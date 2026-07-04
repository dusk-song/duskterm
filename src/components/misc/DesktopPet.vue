<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  defaultDesktopPetAssetSrc,
  normalizeDesktopPetSettings,
  resolveDesktopPetAssetUrl
} from '@/utils/desktopPet';
import { isReducedPerformance } from '@/utils/performance';

const props = defineProps({
  settings: {
    type: Object,
    default: () => ({})
  },
  suspend: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['settings-change']);

const settings = computed(() => normalizeDesktopPetSettings(props.settings || {}));
const shouldHide = computed(() => props.suspend && settings.value.autoHideOnModal);
const isVisible = computed(() => settings.value.enabled && !shouldHide.value && !isReducedPerformance());
const enabledNodes = computed(() => {
  const nodes = Array.isArray(settings.value.nodes) ? settings.value.nodes : [];
  const filtered = nodes.filter((node) => node.enabled !== false);
  return filtered.length ? filtered : nodes;
});

const activeNodeIndex = ref(0);
const renderNonce = ref(0);
const petFracTop = ref(0.01);
const petFracRight = ref(0.01);
const isDragging = ref(false);
const probeEdge = ref('');
const petHostRef = ref(null);
const SNAP_TOP = 10;
const SNAP_RIGHT = 12;
const SNAP_THRESHOLD = 22;
let nextNodeTimer = 0;
let dragState = null;
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
  return allNodes.value.find((n) => n.id === targetId) || null;
};
const displayNode = computed(() => {
  if (probeEdge.value && edgeProbeEnabled.value) {
    const probeNode = resolveProbeNode(probeEdge.value);
    if (probeNode) return probeNode;
  }
  return activeNode.value || null;
});
const activeNodeAssetUrl = computed(() => resolveDesktopPetAssetUrl(displayNode.value?.src || defaultDesktopPetAssetSrc));
const activeNodeKey = computed(() => `${displayNode.value?.id || 'node'}-${renderNonce.value}`);
const stageWidth = computed(() => Math.round(156 * Number(settings.value.scale || 1)));
const stageHeight = computed(() => Math.round(156 * Number(settings.value.scale || 1)));

const nodeOffsetX = computed(() => Number(displayNode.value?.offsetX ?? 0));
const nodeOffsetY = computed(() => Number(displayNode.value?.offsetY ?? 0));
const nodeOwnScale = computed(() => Number(displayNode.value?.scale ?? 1));
const nodeFinalScale = computed(() => (nodeOwnScale.value * probeScaleFactor.value).toFixed(3));

const imageAlt = computed(() => displayNode.value?.name || '桌宠');
const dragHintVisible = computed(() => isVisible.value && !props.suspend);
const dragButtonTitle = computed(() => isDragging.value ? '拖拽中' : '拖拽桌宠位置');

const transitionMs = computed(() => settings.value.transitionMs);
const transStr = computed(() => `${transitionMs.value}ms ease`);

// merged style computeds to reduce object allocation per tick
const petLayerStyle = computed(() => ({
  opacity: isVisible.value ? settings.value.opacity : 0,
  top: `${petTopPx.value}px`,
  right: `${petRightPx.value}px`,
  transition: isDragging.value
    ? `opacity ${transStr.value}`
    : `top 140ms ease, right 140ms ease, opacity ${transStr.value}`
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
  if (!isVisible.value) return;
  const total = enabledNodes.value.length;
  if (total <= 1) return;
  const durationMs = Math.max(200, Number(activeNode.value?.durationMs || 2000));
  nextNodeTimer = window.setTimeout(() => {
    const latestTotal = enabledNodes.value.length || 1;
    activeNodeIndex.value = (activeNodeIndex.value + 1) % latestTotal;
    renderNonce.value += 1;
    scheduleNextNode();
  }, durationMs);
}

function syncPetPositionFromSettings() {
  const rawTop = Number(settings.value.positionTop ?? 10);
  const rawRight = Number(settings.value.positionRight ?? 12);
  // If value is < 1, treat as fraction (new format); otherwise legacy absolute px
  petFracTop.value = rawTop < 1 ? rawTop : rawTop / Math.max(1, lastHostH.value);
  petFracRight.value = rawRight < 1 ? rawRight : rawRight / Math.max(1, lastHostW.value);
  lastHostW.value = window.innerWidth;
  lastHostH.value = window.innerHeight;
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

  return nextPosition;
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

function stopDragging() {
  if (!dragState) return;
  window.removeEventListener('mousemove', handleDragMove);
  window.removeEventListener('mouseup', handleDragEnd);
  dragState = null;
  if (isDragging.value) {
    isDragging.value = false;
    emitPositionSettings();
  }
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
  petFracTop.value = nextPosition.top / Math.max(1, lastHostH.value);
  petFracRight.value = nextPosition.right / Math.max(1, lastHostW.value);
  probeEdge.value = resolveProbeEdge({ top: petTopPx.value, right: petRightPx.value });
  isDragging.value = true;
}

function handleDragEnd() {
  stopDragging();
}

function handleDragStart(event) {
  if (event.button !== 0 || props.suspend) return;
  event.preventDefault();
  dragState = {
    startClientX: event.clientX,
    startClientY: event.clientY,
    startTopPx: petTopPx.value,
    startRightPx: petRightPx.value
  };
  isDragging.value = false;
  window.addEventListener('mousemove', handleDragMove);
  window.addEventListener('mouseup', handleDragEnd);
}

onMounted(() => {
  window.addEventListener('resize', handleWindowResize);
});

watch(
  nodeIdsHash,
  () => {
    normalizeActiveNodeIndex();
    renderNonce.value += 1;
    scheduleNextNode();
  },
  { immediate: true }
);

watch(
  () => [settings.value.enabled, settings.value.transitionMs, settings.value.scale, props.suspend].join('|'),
  () => {
    scheduleNextNode();
  },
  { immediate: true }
);

watch(
  () => [settings.value.positionTop, settings.value.positionRight, settings.value.scale].join('|'),
  () => {
    if (dragState) return;
    syncPetPositionFromSettings();
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  clearNodeTimer();
  stopDragging();
  window.removeEventListener('resize', handleWindowResize);
  if (resizeHandle) cancelAnimationFrame(resizeHandle);
});

</script>

<template>
  <div ref="petHostRef" v-if="displayNode" class="desktop-pet-layer" :style="petLayerStyle">
    <div class="desktop-pet-stage" :style="petStageStyle">
      <div class="desktop-pet-shell" :style="petShellStyle">
        <Transition name="pet-fade" mode="out-in">
          <button v-if="dragHintVisible" :key="`${activeNodeKey}-drag`" class="desktop-pet-drag-surface" type="button"
            :title="dragButtonTitle" @mousedown="handleDragStart">
            <img class="desktop-pet-image" :style="activeNodeAssetStyle" :src="activeNodeAssetUrl" :alt="imageAlt"
              draggable="false" />
          </button>
          <img v-else :key="`${activeNodeKey}-img`" class="desktop-pet-image" :style="activeNodeAssetStyle"
            :src="activeNodeAssetUrl" :alt="imageAlt" draggable="false" />
        </Transition>
      </div>
    </div>
  </div>
</template>

<style scoped>
.desktop-pet-layer {
  position: fixed;
  z-index: 24;
  will-change: top, right, opacity;
  pointer-events: none;
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

.desktop-pet-image {
  width: 132px;
  height: 132px;
  object-fit: contain;
  user-select: none;
  pointer-events: none;
  filter: drop-shadow(0 10px 18px rgba(0, 0, 0, 0.16));
}

/* crossfade transition for pet image swap */
.pet-fade-enter-active,
.pet-fade-leave-active {
  transition: opacity 0.22s ease;
}

.pet-fade-enter-from,
.pet-fade-leave-to {
  opacity: 0;
}
</style>
