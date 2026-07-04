<script setup>
import { Code2, Server, Usb } from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';

const props = defineProps({
  visible: Boolean,
  sessions: { type: Array, default: () => [] },
  activeSessionId: { type: String, default: '' }
});

const emit = defineEmits(['close', 'select']);

const sshStore = useSshStore();
const selectedIndex = ref(0);
const gridRef = ref(null);

const scrollToSelected = async () => {
  await nextTick();
  const card = gridRef.value?.children[selectedIndex.value];
  card?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
};

const gridItems = computed(() => {
  return (props.sessions || []).map((s, i) => ({
    ...s,
    _index: i,
    isActive: s.id === props.activeSessionId,
    isConnected: s.status === 'connected'
  }));
});

const totalItems = computed(() => gridItems.value.length);

const getProtocolIcon = (session) => {
  const p = String(session?.config?.protocol || 'ssh').toLowerCase();
  if (p === 'telnet') return Server;
  if (p === 'serial') return Usb;
  return Code2;
};

const clampIndex = () => {
  if (totalItems.value === 0) { selectedIndex.value = 0; return; }
  selectedIndex.value = Math.max(0, Math.min(selectedIndex.value, totalItems.value - 1));
};

const navigateTo = (dir) => {
  if (totalItems.value === 0) return;
  const cols = Math.max(1, Math.floor(Math.sqrt(totalItems.value)));
  if (dir === 'up') selectedIndex.value = Math.max(0, selectedIndex.value - cols);
  if (dir === 'down') selectedIndex.value = Math.min(totalItems.value - 1, selectedIndex.value + cols);
  if (dir === 'left') selectedIndex.value = Math.max(0, selectedIndex.value - 1);
  if (dir === 'right') selectedIndex.value = Math.min(totalItems.value - 1, selectedIndex.value + 1);
  clampIndex();
  scrollToSelected();
};

const confirmSelection = () => {
  const item = gridItems.value[selectedIndex.value];
  if (item) {
    emit('select', item.id);
    emit('close');
  }
};

const handleKeyDown = (e) => {
  if (!props.visible) return;
  switch (e.key) {
    case 'Escape': e.preventDefault(); e.stopPropagation(); emit('close'); break;
    case 'ArrowUp': e.preventDefault(); e.stopPropagation(); navigateTo('up'); break;
    case 'ArrowDown': e.preventDefault(); e.stopPropagation(); navigateTo('down'); break;
    case 'ArrowLeft': e.preventDefault(); e.stopPropagation(); navigateTo('left'); break;
    case 'ArrowRight': e.preventDefault(); e.stopPropagation(); navigateTo('right'); break;
    case 'Enter': e.preventDefault(); e.stopPropagation(); confirmSelection(); break;
  }
};

onMounted(() => {
  window.addEventListener('keydown', handleKeyDown, true);
  clampIndex();
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown, true);
});

// Scroll to active session when overview opens
watch(() => props.visible, (v) => {
  if (v) {
    const idx = gridItems.value.findIndex(item => item.id === props.activeSessionId);
    if (idx >= 0) selectedIndex.value = idx;
    scrollToSelected();
  }
});
</script>

<template>
  <Teleport to="body">
    <Transition name="overview">
      <div v-if="visible" class="overview-backdrop" @click.self="emit('close')">
        <div class="overview-container">
          <div class="overview-header">
            <span class="overview-title">会话总览</span>
            <span class="overview-count">{{ totalItems }} 个会话</span>
          </div>

          <div ref="gridRef" class="overview-grid" v-if="totalItems > 0">
            <div v-for="item in gridItems" :key="item.id" class="overview-card"
              :class="{ active: item._index === selectedIndex }"
              @click="emit('select', item.id); emit('close')"
              @mouseenter="selectedIndex = item._index">
              <div class="card-preview">
                <component :is="getProtocolIcon(item)" class="card-protocol-icon" />
                <!-- Status dot: green = connected, dim = disconnected -->
                <span class="card-dot" :class="{ live: item.isConnected, dead: !item.isConnected }" />
              </div>
              <div class="card-info">
                <div class="card-name">{{ item.name || item.config?.host || '未命名' }}</div>
                <div class="card-host">{{ item.config?.username || '' }}{{ item.config?.username ? '@' : '' }}{{ item.config?.host || item.host || '—' }}</div>
              </div>
            </div>
          </div>

          <div v-else class="overview-empty">暂无活跃会话</div>

          <div class="overview-hint">
            <kbd>↑↓←→</kbd> 导航 &nbsp; <kbd>Enter</kbd> 选择 &nbsp; <kbd>Esc</kbd> 关闭
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.overview-backdrop {
  position: fixed; inset: 0; z-index: var(--z-critical-overlay);
  background: rgba(0,0,0,0.72);
  display: flex; align-items: center; justify-content: center;
  padding: 40px;
}
.overview-container {
  width: 100%; max-width: 960px; max-height: 80vh;
  display: flex; flex-direction: column; gap: 16px;
}
.overview-header {
  display: flex; align-items: center; gap: 12px; padding: 0 4px;
}
.overview-title { font-size: 18px; font-weight: 700; color: var(--app-text); }
.overview-count {
  font-size: 12px; color: var(--app-text-muted);
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
  padding: 2px 10px; border-radius: 10px;
}

/* Grid */
.overview-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 10px; overflow-y: auto; padding: 4px;
}

/* Card */
.overview-card {
  background: color-mix(in srgb, var(--app-input-bg) 60%, var(--app-bg-dialog));
  border: 1px solid var(--app-border-shadow);
  border-radius: 8px; padding: 14px; cursor: pointer;
  transition: background 0.12s ease, border-color 0.12s ease;
  display: flex; flex-direction: column; gap: 10px; min-height: 120px;
}
.overview-card:hover {
  background: color-mix(in srgb, var(--app-input-bg) 80%, var(--app-bg-dialog));
  border-color: color-mix(in srgb, var(--app-text) 20%, transparent);
}
.overview-card.active {
  background: color-mix(in srgb, var(--color-primary) 12%, transparent);
  border-color: var(--color-primary);
}

/* Preview area */
.card-preview {
  flex: 1; display: flex; align-items: center; justify-content: center;
  background: color-mix(in srgb, var(--app-bg-dialog) 60%, var(--app-text));
  border-radius: 6px; position: relative; min-height: 56px;
}
.card-protocol-icon { font-size: 26px; color: var(--app-text-muted); opacity: 0.3; }

/* Status dot — top-right corner */
.card-dot {
  position: absolute; top: 6px; right: 6px;
  width: 7px; height: 7px; border-radius: 50%;
}
.card-dot.live { background: var(--color-success); }
.card-dot.dead { background: color-mix(in srgb, var(--app-text) 18%, transparent); }

/* Info */
.card-info { display: flex; flex-direction: column; gap: 2px; }
.card-name {
  font-size: 13px; font-weight: 600; color: var(--app-text);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.card-host {
  font-size: 11px; color: var(--app-text-muted);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}

/* Empty */
.overview-empty {
  display: flex; align-items: center; justify-content: center;
  min-height: 160px; color: var(--app-text-muted); font-size: 14px;
}

/* Hint bar */
.overview-hint {
  text-align: center; font-size: 11px; color: var(--app-text-muted);
}
.overview-hint kbd {
  display: inline-block; padding: 1px 6px; border-radius: 3px;
  background: color-mix(in srgb, var(--app-text) 8%, transparent);
  border: 1px solid var(--app-border-shadow);
  font-size: 10px; font-family: inherit;
}

/* Transition */
.overview-enter-active { transition: opacity 160ms ease; }
.overview-leave-active { transition: opacity 120ms ease; }
.overview-enter-from, .overview-leave-to { opacity: 0; }
</style>
