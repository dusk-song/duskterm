<script setup>
import Button from '@/components/ui/button/Button.vue';
import { ChevronDown, ChevronRight, Folder, Server } from '@lucide/vue';
import { computed, ref, watch } from 'vue';
import { PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger } from 'reka-ui';

const props = defineProps({
  modelValue: {
    type: String,
    default: '',
  },
  sessions: {
    type: Array,
    default: () => [],
  },
  groupOrder: {
    type: Array,
    default: () => [],
  },
  disabled: Boolean,
});

const emit = defineEmits(['update:modelValue']);

const open = ref(false);
const expandedGroups = ref(new Set());

function createGroupNode(title, path) {
  return {
    title,
    path,
    groups: new Map(),
    sessions: [],
  };
}

const treeRoot = computed(() => {
  const root = createGroupNode('', '');
  const order = new Map((props.groupOrder || []).map((group, index) => [String(group), index]));
  const sessions = [...(props.sessions || [])].sort((left, right) => {
    const leftGroup = String(left.group || '');
    const rightGroup = String(right.group || '');
    const leftOrder = order.has(leftGroup) ? order.get(leftGroup) : Number.MAX_SAFE_INTEGER;
    const rightOrder = order.has(rightGroup) ? order.get(rightGroup) : Number.MAX_SAFE_INTEGER;
    if (leftOrder !== rightOrder) return leftOrder - rightOrder;
    if (leftGroup !== rightGroup) return leftGroup.localeCompare(rightGroup, 'zh-CN');
    return String(left.name || '').localeCompare(String(right.name || ''), 'zh-CN');
  });

  for (const session of sessions) {
    const parts = String(session.group || '').split('/').map((part) => part.trim()).filter(Boolean);
    let parent = root;
    let path = '';
    for (const part of parts) {
      path = path ? `${path}/${part}` : part;
      if (!parent.groups.has(part)) {
        parent.groups.set(part, createGroupNode(part, path));
      }
      parent = parent.groups.get(part);
    }
    parent.sessions.push(session);
  }

  return root;
});

const groupPaths = computed(() => {
  const paths = [];
  const walk = (node) => {
    for (const group of node.groups.values()) {
      paths.push(group.path);
      walk(group);
    }
  };
  walk(treeRoot.value);
  return paths;
});

watch(
  groupPaths,
  (paths) => {
    const availablePaths = new Set(paths);
    expandedGroups.value = new Set(
      [...expandedGroups.value].filter((path) => availablePaths.has(path)),
    );
  },
  { immediate: true },
);

const visibleRows = computed(() => {
  const rows = [];
  const walk = (node, depth) => {
    for (const group of node.groups.values()) {
      rows.push({ type: 'group', key: `group:${group.path}`, depth, group });
      if (expandedGroups.value.has(group.path)) {
        walk(group, depth + 1);
      }
    }

    for (const session of node.sessions) {
      rows.push({ type: 'session', key: session.id, depth, session });
    }
  };
  walk(treeRoot.value, 0);
  return rows;
});

const selectedSession = computed(() =>
  (props.sessions || []).find((session) => session.id === props.modelValue) || null,
);

const selectedLabel = computed(() => {
  if (!selectedSession.value) return '选择已保存的 SSH 会话';
  const name = selectedSession.value.name
    || `${selectedSession.value.username || 'user'}@${selectedSession.value.host || 'host'}`;
  const group = String(selectedSession.value.group || '').trim();
  return group ? `${group} / ${name}` : name;
});

function toggleGroup(path) {
  const next = new Set(expandedGroups.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expandedGroups.value = next;
}

function selectSession(sessionId) {
  emit('update:modelValue', sessionId);
  open.value = false;
}
</script>

<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <Button
        type="button"
        size="default"
        variant="outline"
        class="w-full justify-between font-normal"
        :disabled="disabled"
        aria-label="选择已保存的 SSH 会话"
      >
        <span class="truncate">{{ selectedLabel }}</span>
        <ChevronDown class="size-4 shrink-0 text-muted-foreground" />
      </Button>
    </PopoverTrigger>

    <PopoverPortal>
      <PopoverContent
        side="bottom"
        align="start"
        :side-offset="4"
        :collision-padding="16"
        class="z-[var(--z-select)] max-h-[320px] w-[var(--reka-popover-trigger-width)] min-w-[280px] overflow-y-auto rounded-[10px] border border-[var(--app-border-dark)] bg-popover p-1 text-popover-foreground shadow-[var(--niri-shadow-dialog)] outline-none"
      >
        <div v-if="visibleRows.length" class="space-y-0.5">
          <button
            v-for="row in visibleRows"
            :key="row.key"
            type="button"
            :class="[
              'flex h-8 w-full items-center gap-2 rounded-md pr-2 text-left text-sm transition-colors hover:bg-accent hover:text-accent-foreground',
              row.type === 'session' && row.session.id === modelValue ? 'bg-accent text-accent-foreground' : '',
            ]"
            :style="{ paddingLeft: `${row.depth * 16 + 8}px` }"
            :aria-expanded="row.type === 'group' ? expandedGroups.has(row.group.path) : undefined"
            :aria-selected="row.type === 'session' ? row.session.id === modelValue : undefined"
            @click="row.type === 'group' ? toggleGroup(row.group.path) : selectSession(row.session.id)"
          >
            <template v-if="row.type === 'group'">
              <ChevronRight
                :class="['size-3.5 shrink-0 transition-transform', expandedGroups.has(row.group.path) ? 'rotate-90' : '']"
              />
              <Folder class="size-4 shrink-0 text-muted-foreground" />
              <span class="truncate">{{ row.group.title }}</span>
            </template>
            <template v-else>
              <span class="w-3.5 shrink-0" />
              <Server class="size-4 shrink-0 text-muted-foreground" />
              <span class="truncate">
                {{ row.session.name || `${row.session.username || 'user'}@${row.session.host || 'host'}` }}
              </span>
            </template>
          </button>
        </div>
        <div v-else class="px-3 py-5 text-center text-xs text-muted-foreground">
          还没有已保存的 SSH 会话。
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>
