<script setup>
import { X } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import IconButton from '@/components/common/IconButton.vue';
import { computeSplitLayout } from '@/utils/splitTree';
import Terminal from './Terminal.vue';
import TerminalTitleBar from './TerminalTitleBar.vue';

const props = defineProps({
  panels: { type: Array, required: true },
  activePanelId: { type: String, default: null },
  splitTrees: { type: Object, required: true },
  focusedLeaf: { type: Object, required: true },
  resolveTree: { type: Function, required: true },
  onSplitDrag: { type: Function, required: true },
  onSetFocused: { type: Function, required: true }
});

const emit = defineEmits(['activate', 'close-panel', 'tab-drop', 'tab-context']);

const rootRef = ref(null);
const hasPanels = computed(() => props.panels.length > 0);
const activePanel = computed(() => props.panels.find((panel) => panel.id === props.activePanelId) || null);

const scrollIndex = ref(0);
const isTransitioning = ref(false);

const syncIndexFromActive = () => {
  const idx = props.panels.findIndex((panel) => panel.id === props.activePanelId);
  if (idx >= 0 && idx !== scrollIndex.value) {
    isTransitioning.value = true;
    scrollIndex.value = idx;
  }
  if (props.activePanelId) {
    window.dispatchEvent(new CustomEvent('terminal:focus', { detail: { sessionId: props.activePanelId } }));
  }
};

const scrollTo = (index) => {
  const count = props.panels.length;
  if (count === 0 || index < 0 || index >= count) return;
  if (index === scrollIndex.value) return;

  scrollIndex.value = index;
  isTransitioning.value = true;
  const target = props.panels[index];
  if (target && target.id !== props.activePanelId) {
    emit('activate', target.id);
    window.dispatchEvent(new CustomEvent('terminal:focus', { detail: { sessionId: target.id } }));
  }
};

const scrollNext = () => {
  const count = props.panels.length;
  if (count <= 1) return;
  scrollTo((scrollIndex.value + 1) % count);
};

const scrollPrev = () => {
  const count = props.panels.length;
  if (count <= 1) return;
  scrollTo((scrollIndex.value - 1 + count) % count);
};

const handleScrollTo = (event) => {
  const idx = event?.detail?.index;
  if (typeof idx === 'number') scrollTo(idx);
};

const isWithinRoot = (target) => rootRef.value?.contains(target);

const handleKeyDown = (event) => {
  if (event.isComposing) return;
  if (!isWithinRoot(event.target)) return;

  if (event.ctrlKey && !event.shiftKey && !event.altKey) {
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      scrollPrev();
      return;
    }
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      scrollNext();
      return;
    }
  }

  if (event.ctrlKey && !event.shiftKey && !event.altKey) {
    const num = parseInt(event.key, 10);
    if (num >= 1 && num <= 9) {
      event.preventDefault();
      scrollTo(num - 1);
    }
  }
};

const handleWheel = (event) => {
  if (!isWithinRoot(event.target)) return;
  if (event.shiftKey && (Math.abs(event.deltaX) > 5 || Math.abs(event.deltaY) > 5)) {
    event.preventDefault();
    if (event.deltaX > 0 || event.deltaY > 0) scrollNext();
    else scrollPrev();
  }
};

const onTransitionEnd = () => {
  isTransitioning.value = false;
};

watch(() => props.activePanelId, () => {
  syncIndexFromActive();
});

onMounted(() => {
  syncIndexFromActive();
  window.addEventListener('terminal-scroll-to', handleScrollTo);
  window.addEventListener('keydown', handleKeyDown);
  window.addEventListener('wheel', handleWheel, { passive: false });
});

onUnmounted(() => {
  window.removeEventListener('terminal-scroll-to', handleScrollTo);
  window.removeEventListener('keydown', handleKeyDown);
  window.removeEventListener('wheel', handleWheel);
});

const panelLayout = (panelId) => computeSplitLayout(props.resolveTree(panelId));
const leafStyle = (leaf) => ({ left: `${leaf.x}%`, top: `${leaf.y}%`, width: `${leaf.width}%`, height: `${leaf.height}%` });
const dividerStyle = (divider) => divider.direction === 'vertical'
  ? { left: `${divider.x}%`, top: `${divider.y}%`, height: `${divider.height}%` }
  : { left: `${divider.x}%`, top: `${divider.y}%`, width: `${divider.width}%` };
</script>

<template>
  <div ref="rootRef" class="terminal-panel-manager">
    <div v-if="hasPanels && activePanel" class="session-name-bar">
      <TerminalTitleBar :session-id="activePanel.id" :session-name="activePanel.name || ''">
        <template #actions>
          <IconButton :icon="X" size="26px" aria-label="关闭会话" class="session-close-btn"
            :action="() => emit('close-panel', activePanel.id)" />
        </template>
      </TerminalTitleBar>
    </div>

    <div v-if="hasPanels" class="panel-scroll-track">
      <div class="panel-scroll-strip" :style="{ transform: `translateX(-${scrollIndex * 100}%)` }"
        :class="{ transitioning: isTransitioning }" @transitionend="onTransitionEnd">
        <div v-for="panel in panels" :key="panel.id" class="scroll-pane">
          <div v-for="leaf in panelLayout(panel.id).leaves" :key="leaf.sessionId" class="split-leaf"
            :class="{ 'split-focused': panelLayout(panel.id).leaves.length > 1 && focusedLeaf[panel.id] === leaf.sessionId }"
            :style="leafStyle(leaf)"
            @mousedown="onSetFocused(panel.id, leaf.sessionId)">
            <Terminal :session-id="leaf.sessionId" />
          </div>
          <div v-for="(divider, index) in panelLayout(panel.id).dividers" :key="index" class="split-divider"
            :class="divider.direction === 'vertical' ? 'divider-vertical' : 'divider-horizontal'"
            :style="dividerStyle(divider)" @mousedown="onSplitDrag($event, divider.node, divider.bounds)" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-panel-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  border-radius: var(--niri-radius-lg, 14px);
  background: var(--terminal-surface-bg, var(--app-bg-dialog));
}

.session-name-bar {
  height: 28px;
  display: flex;
  align-items: center;
  flex-shrink: 0;
  background: color-mix(in srgb, var(--terminal-surface-bg, var(--app-bg-dialog)) 88%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--app-border-shadow, rgba(255,255,255,0.08)) 62%, transparent);
  padding: 0 8px;
}

.session-close-btn {
  flex: 0 0 auto;
  z-index: 2;
  --icon-btn-size: 26px;
  --icon-btn-color: var(--app-terminal-close-color);
  --icon-btn-hover-color: var(--app-terminal-close-hover-color);
  --icon-btn-hover-bg: var(--app-terminal-close-hover-bg);
  background: transparent;
  opacity: 1;
  box-shadow: none;
}

.panel-scroll-track {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  position: relative;
  background: transparent;
}

.panel-scroll-strip {
  display: flex;
  height: 100%;
  will-change: transform;
}

.panel-scroll-strip.transitioning {
  transition: transform 150ms cubic-bezier(0.4, 0, 0.2, 1);
}

.scroll-pane {
  flex: 0 0 100%;
  min-width: 0;
  min-height: 0;
  height: 100%;
  position: relative;
  overflow: hidden;
  background: transparent;
}

.panel-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  color: var(--app-text-muted);
}

.split-leaf {
  position: absolute;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  padding: 0;
  box-sizing: border-box;
  overflow: hidden;
  background: transparent;
  border-radius: 0;
  transition: box-shadow 120ms ease;
}

.split-divider {
  position: absolute;
  z-index: 10;
  background: transparent;
  transition: opacity 120ms ease;
}

.divider-vertical {
  width: 8px;
  transform: translateX(-50%);
  cursor: col-resize;
}

.divider-horizontal {
  height: 8px;
  transform: translateY(-50%);
  cursor: row-resize;
}

.split-focused {
  box-shadow: inset 0 0 0 2px color-mix(
    in srgb,
    var(--app-bg-dialog) 82%,
    var(--app-text) 18%
  );
}
</style>
