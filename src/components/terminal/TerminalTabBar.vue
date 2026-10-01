<script setup>
import { X } from '@lucide/vue';
import { computed, nextTick, onBeforeUnmount, onBeforeUpdate, onMounted, ref, watch } from 'vue';
import ContextMenu from '@/components/ui/context-menu/ContextMenu.vue';
import ContextMenuContent from '@/components/ui/context-menu/ContextMenuContent.vue';
import ContextMenuItem from '@/components/ui/context-menu/ContextMenuItem.vue';
import ContextMenuSeparator from '@/components/ui/context-menu/ContextMenuSeparator.vue';
import ContextMenuTrigger from '@/components/ui/context-menu/ContextMenuTrigger.vue';
import { toast } from '@/composables/useToast';
import { buildSessionDisplayNameMap } from '@/utils/sessionOverview';

const props = defineProps({
  panels: { type: Array, required: true },
  activePanelId: { type: String, default: null }
});

const emit = defineEmits(['activate', 'close', 'duplicate', 'reconnect']);
const tabListRef = ref(null);
const tabItemRefs = new Map();
const tabButtonRefs = new Map();
let resizeObserver = null;

const displayNames = computed(() => buildSessionDisplayNameMap(props.panels));
const panelIds = computed(() => props.panels.map((panel) => panel.id).join('|'));

const getDisplayName = (panel) => displayNames.value.get(panel.id)
  || panel.name
  || panel.config?.name
  || '未命名会话';

const getClipboardValues = (panel) => {
  const config = panel?.config || {};
  const protocol = String(config.protocol || panel?.protocol || 'ssh').trim().toLowerCase();
  const host = String(config.host || panel?.host || '').trim();
  const username = String(config.username || panel?.username || '').trim();
  const port = Number(config.port || panel?.port || 0);
  const connectionHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
  let connection = '';

  if (host) {
    connection = `${username ? `${username}@` : ''}${connectionHost}${port > 0 ? `:${port}` : ''}`;
  } else if (protocol === 'serial') {
    connection = String(config.serial_path || panel?.serial_path || '').trim();
  } else if (protocol === 'local') {
    connection = String(config.local_shell_name || panel?.local_shell_name || '').trim();
  }

  return {
    name: getDisplayName(panel),
    host,
    username,
    connection,
    cwd: String(panel?.cwd || '').trim()
  };
};

const copyPanelValue = async (panel, key) => {
  const values = getClipboardValues(panel);
  const value = values[key];
  if (!value) return;
  const labels = {
    name: '会话名称',
    host: '主机地址',
    username: '用户名',
    connection: '连接信息',
    cwd: '当前目录'
  };
  const label = labels[key] || '内容';
  try {
    await navigator.clipboard.writeText(value);
    toast.success(`${label}已复制`);
  } catch {
    toast.error(`复制${label}失败`);
  }
};

const setTabItemRef = (panelId, element) => {
  if (element) tabItemRefs.set(panelId, element);
  else tabItemRefs.delete(panelId);
};

const setTabButtonRef = (panelId, element) => {
  if (element) tabButtonRefs.set(panelId, element);
  else tabButtonRefs.delete(panelId);
};

const scrollActiveIntoView = () => nextTick(() => {
  tabItemRefs.get(props.activePanelId)?.scrollIntoView({
    behavior: 'auto',
    block: 'nearest',
    inline: 'nearest'
  });
});

const activatePanel = (panelId) => {
  if (!panelId || panelId === props.activePanelId) return;
  emit('activate', panelId);
};

const closePanel = (panelId) => {
  if (!panelId) return;
  emit('close', panelId);
};

const focusRelativeTab = (panelId, offset) => {
  const currentIndex = props.panels.findIndex((panel) => panel.id === panelId);
  if (currentIndex < 0 || props.panels.length < 2) return;
  const targetIndex = (currentIndex + offset + props.panels.length) % props.panels.length;
  const target = props.panels[targetIndex];
  if (!target) return;
  emit('activate', target.id);
  nextTick(() => tabButtonRefs.get(target.id)?.focus());
};

const handleTabKeydown = (event, panelId) => {
  if (event.key === 'ArrowLeft') {
    event.preventDefault();
    focusRelativeTab(panelId, -1);
  } else if (event.key === 'ArrowRight') {
    event.preventDefault();
    focusRelativeTab(panelId, 1);
  } else if (event.key === 'Home') {
    event.preventDefault();
    const first = props.panels[0];
    if (first) {
      emit('activate', first.id);
      nextTick(() => tabButtonRefs.get(first.id)?.focus());
    }
  } else if (event.key === 'End') {
    event.preventDefault();
    const last = props.panels[props.panels.length - 1];
    if (last) {
      emit('activate', last.id);
      nextTick(() => tabButtonRefs.get(last.id)?.focus());
    }
  }
};

const handleWheel = (event) => {
  event.stopPropagation();
};

watch([() => props.activePanelId, panelIds], scrollActiveIntoView, { flush: 'post' });

onBeforeUpdate(() => {
  tabItemRefs.clear();
  tabButtonRefs.clear();
});

onMounted(() => {
  scrollActiveIntoView();
  if (typeof ResizeObserver === 'function' && tabListRef.value) {
    resizeObserver = new ResizeObserver(scrollActiveIntoView);
    resizeObserver.observe(tabListRef.value);
  }
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  resizeObserver = null;
});
</script>

<template>
  <div class="terminal-tab-bar">
    <div ref="tabListRef" class="terminal-tab-list" role="tablist" aria-label="终端会话"
      @wheel="handleWheel">
      <ContextMenu v-for="panel in panels" :key="panel.id">
        <ContextMenuTrigger as-child>
          <div :ref="(element) => setTabItemRef(panel.id, element)"
            class="terminal-tab-item" :class="{ active: panel.id === activePanelId }">
          <button :ref="(element) => setTabButtonRef(panel.id, element)" type="button"
            class="terminal-tab-main" role="tab" :aria-selected="panel.id === activePanelId"
            :tabindex="panel.id === activePanelId ? 0 : -1" @click="activatePanel(panel.id)"
            @keydown="handleTabKeydown($event, panel.id)">
            <span class="terminal-tab-status" :class="panel.status" aria-hidden="true" />
            <span class="terminal-tab-label">{{ getDisplayName(panel) }}</span>
          </button>
          <button type="button" class="terminal-tab-close" :aria-label="`关闭 ${getDisplayName(panel)}`"
            @mousedown.stop @click.stop="closePanel(panel.id)">
            <X :size="13" stroke-width="1.9" />
          </button>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem :disabled="!panel.config" @select="emit('duplicate', panel.id)">
            复制会话
          </ContextMenuItem>
          <ContextMenuItem :disabled="!panel.config" @select="emit('reconnect', panel.id)">
            重新连接
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem @select="copyPanelValue(panel, 'name')">复制会话名称</ContextMenuItem>
          <ContextMenuItem :disabled="!getClipboardValues(panel).host"
            @select="copyPanelValue(panel, 'host')">复制主机地址</ContextMenuItem>
          <ContextMenuItem :disabled="!getClipboardValues(panel).username"
            @select="copyPanelValue(panel, 'username')">复制用户名</ContextMenuItem>
          <ContextMenuItem :disabled="!getClipboardValues(panel).connection"
            @select="copyPanelValue(panel, 'connection')">复制连接信息</ContextMenuItem>
          <ContextMenuItem :disabled="!getClipboardValues(panel).cwd"
            @select="copyPanelValue(panel, 'cwd')">复制当前目录</ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem variant="destructive" @select="closePanel(panel.id)">关闭会话</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </div>
  </div>
</template>

<style scoped>
.terminal-tab-bar {
  position: relative;
  z-index: 19;
  display: flex;
  flex: 0 0 auto;
  min-width: 0;
  min-height: 26px;
  align-items: stretch;
  padding: 0;
  border-bottom: 1px solid var(--app-border-shadow);
  background: color-mix(in srgb, var(--app-tab-bg-inactive) 48%, transparent);
}

.terminal-tab-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
  flex: 1 1 auto;
  min-width: 0;
  max-height: 78px;
  align-content: flex-start;
  gap: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior-y: contain;
  scrollbar-color: var(--app-border-shadow) transparent;
  scrollbar-width: thin;
}

.terminal-tab-list::-webkit-scrollbar {
  width: 5px;
}

.terminal-tab-list::-webkit-scrollbar-thumb {
  border-radius: 999px;
  background: var(--app-border-shadow);
}

.terminal-tab-item {
  display: inline-flex;
  position: relative;
  min-width: 0;
  height: 26px;
  align-items: center;
  overflow: hidden;
  border-right: 1px solid var(--app-border-shadow);
  border-bottom: 1px solid var(--app-border-shadow);
  color: var(--app-text-muted);
  background: transparent;
  transition: color var(--app-motion-control), background var(--app-motion-control), border-color var(--app-motion-control);
}

.terminal-tab-item::after {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  height: 1px;
  content: '';
  background: hsl(var(--primary));
  opacity: 0;
  transition: opacity var(--app-motion-control);
}

.terminal-tab-item:hover {
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-tab-bg-active) 54%, transparent);
}

.terminal-tab-item.active {
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-tab-bg-active) 82%, transparent);
}

.terminal-tab-item.active::after {
  opacity: 1;
}

.terminal-tab-main {
  display: inline-flex;
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  align-items: center;
  gap: 6px;
  padding: 0 4px 0 9px;
  border: 0;
  outline: none;
  color: inherit;
  background: transparent;
  text-align: left;
}

.terminal-tab-main:focus-visible {
  box-shadow: inset 0 0 0 1px var(--app-focus-border);
}

.terminal-tab-status {
  width: 5px;
  height: 5px;
  flex: 0 0 auto;
  border-radius: 999px;
  background: var(--app-connection-offline);
}

.terminal-tab-status.connected {
  background: var(--app-connection-online);
}

.terminal-tab-status.connecting {
  background: var(--app-status-info);
}

.terminal-tab-status.error {
  background: var(--app-status-error);
}

.terminal-tab-label {
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  font-weight: 500;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.terminal-tab-close {
  display: inline-flex;
  width: 20px;
  height: 20px;
  flex: 0 0 20px;
  align-items: center;
  justify-content: center;
  margin-right: 4px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  outline: none;
  color: var(--app-terminal-close-color);
  background: transparent;
  opacity: 0;
  transition: color var(--app-motion-control), background var(--app-motion-control), opacity var(--app-motion-control);
}

.terminal-tab-item:hover .terminal-tab-close,
.terminal-tab-item.active .terminal-tab-close,
.terminal-tab-close:focus-visible {
  opacity: .72;
}

.terminal-tab-close:hover,
.terminal-tab-close:focus-visible {
  color: var(--app-terminal-close-hover-color);
  background: var(--app-terminal-close-hover-bg);
  opacity: 1;
}

.terminal-tab-close:focus-visible {
  box-shadow: var(--app-focus-shadow);
}

@media (max-width: 720px) {
  .terminal-tab-list {
    grid-template-columns: repeat(auto-fill, minmax(108px, 1fr));
  }
}
</style>
