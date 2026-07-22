<script setup>
import { Code2, Server, Usb } from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { buildSessionOverviewItems } from '@/utils/sessionOverview';

const props = defineProps({
  visible: Boolean,
  sessions: { type: Array, default: () => [] },
  syncChannels: { type: Array, default: () => [] },
  activeSessionId: { type: String, default: '' }
});

const emit = defineEmits(['close', 'select']);

const selectedIndex = ref(0);
const gridRef = ref(null);

const scrollToSelected = async () => {
  await nextTick();
  const card = gridRef.value?.children[selectedIndex.value];
  card?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
};

const gridItems = computed(() => {
  return buildSessionOverviewItems(props.sessions, props.syncChannels, props.activeSessionId).map((item, i) => ({
    ...item,
    _index: i,
    isActive: item.sessions.some((session) => session.id === props.activeSessionId),
  }));
});

const totalItems = computed(() => gridItems.value.length);
const channelCount = computed(() => gridItems.value.filter((item) => item.type === 'channel').length);

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
  if (item?.selectSessionId) {
    emit('select', item.selectSessionId);
    emit('close');
  }
};

const selectItem = (item) => {
  if (!item?.selectSessionId) return;
  emit('select', item.selectSessionId);
  emit('close');
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
    const idx = gridItems.value.findIndex(item => item.isActive);
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
            <span class="overview-count">{{ props.sessions.length }} 个会话<span v-if="channelCount"> · {{ channelCount }} 个同步频道</span></span>
          </div>

          <div ref="gridRef" class="overview-grid" v-if="totalItems > 0">
            <div v-for="item in gridItems" :key="item.id" class="overview-card"
              :class="{ active: item._index === selectedIndex, channel: item.type === 'channel' }"
              @click="selectItem(item)"
              @mouseenter="selectedIndex = item._index">
              <div v-if="item.type === 'channel'" class="card-preview channel-preview">
                <div class="channel-mini-grid">
                  <div v-for="member in item.sessions.slice(0, 6)" :key="member.id" class="channel-mini-session"
                    :title="member.name || member.config?.host || member.id">
                    <component :is="getProtocolIcon(member)" class="channel-mini-icon" />
                    <span class="channel-mini-name">{{ member.name || member.config?.host || '会话' }}</span>
                    <span class="channel-mini-dot" :class="{ live: member.status === 'connected' }" />
                  </div>
                  <div v-if="item.sessions.length > 6" class="channel-mini-more">+{{ item.sessions.length - 6 }}</div>
                </div>
              </div>
              <div v-else class="card-preview">
                <component :is="getProtocolIcon(item.session)" class="card-protocol-icon" />
                <span class="card-dot"
                  :class="{ live: item.session.status === 'connected', dead: item.session.status !== 'connected' }" />
              </div>
              <div class="card-info">
                <template v-if="item.type === 'channel'">
                  <div class="card-name">{{ item.name }}</div>
                  <div class="card-host">{{ item.description }}</div>
                </template>
                <template v-else>
                  <div class="card-name">{{ item.session.name || item.session.config?.host || '未命名' }}</div>
                  <div class="card-host">{{ item.session.config?.username || '' }}{{ item.session.config?.username ? '@' : '' }}{{ item.session.config?.host || item.session.host || '—' }}</div>
                </template>
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
.overview-card.channel { min-height: 146px; }

/* Preview area */
.card-preview {
  flex: 1; display: flex; align-items: center; justify-content: center;
  background: color-mix(in srgb, var(--app-bg-dialog) 60%, var(--app-text));
  border-radius: 6px; position: relative; min-height: 56px;
}
.card-protocol-icon { font-size: 26px; color: var(--app-text-muted); opacity: 0.3; }
.channel-preview { padding: 7px; align-items: stretch; }
.channel-mini-grid {
  width: 100%; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 5px;
}
.channel-mini-session {
  position: relative; min-width: 0; height: 28px; padding: 0 17px 0 6px;
  display: flex; align-items: center; gap: 5px;
  border: 1px solid color-mix(in srgb, var(--app-text) 10%, transparent);
  border-radius: 4px; background: color-mix(in srgb, var(--app-input-bg) 76%, transparent);
}
.channel-mini-icon { flex: 0 0 auto; width: 12px; height: 12px; color: var(--app-text-muted); opacity: .65; }
.channel-mini-name {
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: var(--app-text); font-size: 9px;
}
.channel-mini-dot {
  position: absolute; top: 5px; right: 5px; width: 5px; height: 5px; border-radius: 50%;
  background: color-mix(in srgb, var(--app-text) 18%, transparent);
}
.channel-mini-dot.live { background: var(--color-success); }
.channel-mini-more {
  height: 28px; display: flex; align-items: center; justify-content: center;
  border-radius: 4px; color: var(--app-text-muted); font-size: 10px;
  background: color-mix(in srgb, var(--app-text) 6%, transparent);
}

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
