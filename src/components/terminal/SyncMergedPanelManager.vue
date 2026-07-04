<script setup>
import Button from '@/components/ui/button/Button.vue';
import { ChevronLeft, ChevronRight, X } from '@lucide/vue';
import { computed, KeepAlive, nextTick, ref } from 'vue';
import IconButton from '@/components/common/IconButton.vue';
import Terminal from './Terminal.vue';
import TerminalTitleBar from './TerminalTitleBar.vue';

const props = defineProps({
  channelName: {
    type: String,
    default: '',
  },
  sessions: {
    type: Array,
    required: true
  },
  activePanelId: {
    type: String,
    default: ''
  },
  pageSize: {
    type: Number,
    default: 4
  },
  pageIndex: {
    type: Number,
    default: 0
  }
});

const emit = defineEmits(['activate', 'close-panel', 'change-page']);

const paneRefs = ref([]);

const pagedSessions = computed(() => {
  const list = Array.isArray(props.sessions) ? props.sessions : [];
  const size = Math.max(1, Number(props.pageSize || 4));
  const pages = [];
  for (let i = 0; i < list.length; i += size) {
    pages.push(list.slice(i, i + size));
  }
  return pages;
});

const totalPages = computed(() => pagedSessions.value.length);

const safePageIndex = computed(() => {
  if (!totalPages.value) return 0;
  return Math.max(0, Math.min(props.pageIndex, totalPages.value - 1));
});

const currentSessions = computed(() => pagedSessions.value[safePageIndex.value] || []);

const gridTemplateColumns = computed(() => {
  const count = currentSessions.value.length;
  if (count <= 1) return '1fr';
  return '1fr 1fr';
});

const gridTemplateRows = computed(() => {
  const count = currentSessions.value.length;
  if (count <= 2) return '1fr';
  return '1fr 1fr';
});

const changePage = (next) => {
  if (!totalPages.value) return;
  const target = Math.max(0, Math.min(next, totalPages.value - 1));
  emit('change-page', target);
  nextTick(() => {
    paneRefs.value?.[0]?.focus?.();
  });
};

const focusPane = (index) => {
  paneRefs.value?.[index]?.focus?.();
};

const onPaneKeydown = (event, paneIndex, sessionId) => {
  const count = currentSessions.value.length;
  if (!count) return;

  const cols = count <= 1 ? 1 : 2;
  let nextIndex = paneIndex;

  if (event.key === 'ArrowRight') {
    nextIndex = Math.min(count - 1, paneIndex + 1);
  } else if (event.key === 'ArrowLeft') {
    nextIndex = Math.max(0, paneIndex - 1);
  } else if (event.key === 'ArrowDown') {
    nextIndex = Math.min(count - 1, paneIndex + cols);
  } else if (event.key === 'ArrowUp') {
    nextIndex = Math.max(0, paneIndex - cols);
  } else if (event.key === 'Enter' || event.key === ' ') {
    emit('activate', sessionId);
    return;
  } else {
    return;
  }

  event.preventDefault();
  if (nextIndex !== paneIndex) {
    emit('activate', currentSessions.value[nextIndex]?.id || sessionId);
    focusPane(nextIndex);
  }
};
</script>

<template>
  <div class="sync-merged-manager">
    <div class="sync-merged-header">
      <div class="sync-merged-title">同步输入 · {{ channelName || '合并视图' }}</div>
      <div v-if="totalPages > 1" class="sync-merged-pager">
        <Button variant="ghost" size="sm" :disabled="safePageIndex <= 0" @click="changePage(safePageIndex - 1)">
          <ChevronLeft :size="14" />
        </Button>
        <span class="sync-merged-page-text">{{ safePageIndex + 1 }} / {{ totalPages }}</span>
        <Button variant="ghost" size="sm" :disabled="safePageIndex >= totalPages - 1"
          @click="changePage(safePageIndex + 1)">
          <ChevronRight :size="14" />
        </Button>
      </div>
    </div>

    <div class="sync-merged-grid" :style="{ gridTemplateColumns, gridTemplateRows }">
      <div v-for="(session, index) in currentSessions" :key="session.id" class="sync-merged-pane split-pane"
        :class="{ active: activePanelId === session.id }" tabindex="0" :ref="(el) => { paneRefs[index] = el; }"
        @click="emit('activate', session.id)" @keydown="onPaneKeydown($event, index, session.id)">
        <div class="sync-merged-pane-title">
          <TerminalTitleBar :session-id="session.id" :session-name="session.name || ''">
            <template #actions>
              <IconButton :icon="X" size="sm" aria-label="关闭连接" class="sync-merged-pane-close"
                :action="(event) => { event.stopPropagation(); emit('close-panel', session.id); }" />
            </template>
          </TerminalTitleBar>
        </div>
        <div class="sync-merged-pane-body">
          <KeepAlive>
            <Terminal :sessionId="session.id" :key="session.id" />
          </KeepAlive>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sync-merged-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 4px;
  gap: 6px;
}

.sync-merged-header {
  height: 28px;
  flex-shrink: 0;
  border: 1px solid var(--app-border-shadow);
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-bg-dialog) 96%, var(--app-input-bg));
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px;
}

.sync-merged-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--app-text-muted);
}

.sync-merged-pager {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.sync-merged-page-text {
  font-size: 12px;
  color: var(--app-text-muted);
  min-width: 46px;
  text-align: center;
}

.sync-merged-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  gap: 6px;
}

.sync-merged-pane {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid color-mix(in srgb, var(--app-border-shadow) 82%, transparent);
  border-radius: 6px;
  overflow: hidden;
  background: color-mix(in srgb, var(--app-input-bg) 95%, var(--app-bg-dialog));
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12);
  padding: 0;
}

.sync-merged-pane.active {
  outline: 1px solid var(--app-selection-bg);
  outline-offset: -1px;
}

.sync-merged-pane:focus-visible {
  outline: 1px solid var(--app-selection-bg);
  outline-offset: -1px;
}

.sync-merged-pane-title {
  height: 28px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px;
  border-bottom: 1px solid var(--app-border-shadow);
  background: color-mix(in srgb, var(--app-bg-dialog) 94%, var(--app-input-bg));
}

.sync-merged-pane-close {
  flex: 0 0 auto;
  background: transparent;
  --icon-btn-color: color-mix(in srgb, var(--app-text-muted) 94%, var(--app-text));
  --icon-btn-hover-color: var(--app-text);
  --icon-btn-hover-bg: color-mix(in srgb, var(--app-text-muted) 14%, transparent);
}

.sync-merged-pane-close:hover {
  color: var(--app-text);
}

.sync-merged-pane-body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.sync-merged-pane-body > * {
  flex: 1;
  min-width: 0;
  min-height: 0;
}
</style>
