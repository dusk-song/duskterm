<script setup>
import { X } from '@lucide/vue';
import { computed, defineComponent, h, KeepAlive, onMounted, onUnmounted, ref, watch } from 'vue';
import IconButton from '@/components/common/IconButton.vue';
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

const SplitNode = defineComponent({
  name: 'TerminalSplitNode',
  props: {
    node: { type: Object, required: true },
    panelId: { type: String, required: true },
    focusedLeaf: { type: Object, required: true },
    onFocus: { type: Function, required: true },
    onSplitDrag: { type: Function, required: true }
  },
  setup(splitProps) {
    return () => {
      const node = splitProps.node;
      if (!node) return h('div', { class: 'panel-empty' }, '无可用面板');

      if (node.type === 'leaf') {
        const isFocused = splitProps.focusedLeaf?.[splitProps.panelId] === node.sessionId;
        return h('div', {
          class: ['split-pane', 'split-leaf', isFocused ? 'split-focused' : ''],
          onClick: () => splitProps.onFocus(splitProps.panelId, node.sessionId)
        }, [
          h(KeepAlive, null, {
            default: () => h(Terminal, { sessionId: node.sessionId, key: node.sessionId })
          })
        ]);
      }

      const isVertical = node.direction === 'vertical';
      const firstStyle = { flex: `${node.ratio} 1 0` };
      const secondStyle = { flex: `${1 - node.ratio} 1 0` };

      return h('div', { class: ['terminal-split', isVertical ? 'split-vertical' : 'split-horizontal'] }, [
        h('div', { class: 'split-pane', style: firstStyle }, [
          h(SplitNode, {
            node: node.first,
            panelId: splitProps.panelId,
            focusedLeaf: splitProps.focusedLeaf,
            onFocus: splitProps.onFocus,
            onSplitDrag: splitProps.onSplitDrag
          })
        ]),
        h('div', {
          class: ['split-divider', isVertical ? 'divider-vertical' : 'divider-horizontal'],
          onMousedown: (event) => splitProps.onSplitDrag(event, node)
        }),
        h('div', { class: 'split-pane', style: secondStyle }, [
          h(SplitNode, {
            node: node.second,
            panelId: splitProps.panelId,
            focusedLeaf: splitProps.focusedLeaf,
            onFocus: splitProps.onFocus,
            onSplitDrag: splitProps.onSplitDrag
          })
        ])
      ]);
    };
  }
});
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
          <component :is="SplitNode" :node="resolveTree(panel.id)" :panel-id="panel.id" :focused-leaf="focusedLeaf"
            :on-focus="onSetFocused" :on-split-drag="onSplitDrag" />
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
}

.session-name-bar {
  height: 28px;
  display: flex;
  align-items: center;
  flex-shrink: 0;
  background: var(--app-bg-dialog);
  border-bottom: 1px solid var(--app-border-shadow, rgba(255,255,255,0.08));
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
  display: flex;
  flex-direction: column;
}

.panel-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  color: var(--app-text-muted);
}

.terminal-split {
  height: 100%;
  display: flex;
  gap: var(--niri-gap-md, 6px);
  width: 100%;
}

.terminal-split.split-vertical {
  flex-direction: row;
}

.terminal-split.split-horizontal {
  flex-direction: column;
}

.terminal-split .split-pane {
  flex: 0 0 auto;
  min-width: 0;
  min-height: 0;
  display: flex;
  border: var(--niri-border-width, 2px) solid var(--niri-border-color-idle, transparent);
  border-radius: var(--niri-radius-md, 8px);
  overflow: hidden;
  background: color-mix(in srgb, var(--app-input-bg) 95%, var(--app-bg-dialog));
  box-shadow: var(--niri-shadow-idle);
  padding: var(--niri-gap-sm, 4px);
  box-sizing: border-box;
  transition:
    border-color var(--niri-transition-normal, 180ms ease),
    box-shadow var(--niri-transition-normal, 180ms ease);
}

.terminal-split .split-pane:hover {
  border-color: var(--niri-border-color-hover, rgba(192, 132, 47, 0.32));
}

.terminal-split .split-pane > * {
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.split-leaf {
  flex: 1 1 auto;
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
}

.split-divider {
  background: transparent;
  position: relative;
  z-index: 5;
  flex: 0 0 auto;
  opacity: 0.4;
  transition: opacity 120ms ease;
}

.divider-vertical {
  width: var(--niri-gap-md, 6px);
  cursor: col-resize;
  margin: 0;
}

.divider-horizontal {
  height: var(--niri-gap-md, 6px);
  cursor: row-resize;
  margin: 0;
}

.split-divider:hover {
  opacity: 0.8;
}

.split-divider::before {
  content: '';
  position: absolute;
  background: var(--color-primary-muted, var(--color-primary));
  border-radius: 1px;
  opacity: 0;
  transition: opacity 120ms ease;
}

.divider-vertical::before {
  top: 12px;
  bottom: 12px;
  left: 50%;
  width: 2px;
  transform: translateX(-50%);
}

.divider-horizontal::before {
  left: 12px;
  right: 12px;
  top: 50%;
  height: 2px;
  transform: translateY(-50%);
}

.split-divider:hover::before {
  opacity: 0.6;
}

.split-focused {
  border-color: var(--niri-border-color-focus, var(--color-primary)) !important;
  box-shadow: var(--niri-shadow-focus) !important;
}
</style>
