<script setup>
import { computed, nextTick, ref, useId, watch } from 'vue';
import { ChevronDown, ChevronRight, Folder, History, Pencil, Pin, Plus } from '@lucide/vue';
import { PopoverContent, PopoverPortal, PopoverRoot, PopoverTrigger } from 'reka-ui';
import { Button } from '@/components/ui/button';
import Input from '@/components/ui/input/Input.vue';
import IconButton from '@/components/common/IconButton.vue';
import { useAiStore } from '@/stores/ai';

const open = defineModel({ type: Boolean, required: true });
const props = defineProps({ disabled: Boolean });
const emit = defineEmits(['open-conversation', 'new-conversation']);
const store = useAiStore();
const headingId = useId();
const expanded = ref(new Set());
const editor = ref(null);
const name = ref('');
const nameInput = ref(null);
const moving = ref(null);
const busy = ref(false);
const error = ref('');
let navigating = false;

const groups = computed(() => {
  const byWorkspace = new Map(store.workspaces.map(workspace => [workspace.id, []]));
  for (const conversation of store.conversations) {
    byWorkspace.get(conversation.workspaceId)?.push(conversation);
  }
  return byWorkspace;
});

watch(open, async visible => {
  editor.value = null;
  moving.value = null;
  error.value = '';
  if (!visible) return;
  navigating = false;
  expanded.value = new Set([store.workspaceId]);
  try {
    await store.loadList();
  } catch (err) {
    if (open.value) error.value = String(err);
  }
});

function toggle(id) {
  if (expanded.value.has(id)) expanded.value.delete(id);
  else expanded.value.add(id);
}

async function editWorkspace(workspace = null) {
  moving.value = null;
  editor.value = { id: workspace?.id ?? null };
  name.value = workspace?.name ?? '';
  await nextTick();
  nameInput.value?.$el?.focus();
}

async function update(operation) {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    await operation();
  } catch (err) {
    error.value = String(err);
  } finally {
    busy.value = false;
  }
}

function submitWorkspace() {
  if (!editor.value || !name.value.trim()) return;
  const id = editor.value.id;
  const title = name.value.trim();
  void update(async () => {
    if (id) await store.renameWorkspace(id, title);
    else expanded.value.add((await store.createWorkspace(title)).id);
    editor.value = null;
  });
}

function moveConversation(conversation, workspaceId) {
  if (!workspaceId) return;
  void update(async () => {
    await store.moveConversation(conversation.id, workspaceId);
    expanded.value.add(workspaceId);
    moving.value = null;
  });
}

function navigate(action, id) {
  navigating = true;
  if (action === 'open') emit('open-conversation', id);
  else emit('new-conversation', id);
}

function closeFocus(event) {
  if (navigating) event.preventDefault();
  navigating = false;
}
</script>

<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <Button variant="ghost" size="icon-sm" :disabled="props.disabled" aria-label="历史对话" title="历史对话">
        <History />
      </Button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent
        class="ai-history-popover"
        :aria-labelledby="headingId"
        side="bottom"
        align="end"
        :side-offset="6"
        :collision-padding="10"
        @close-auto-focus="closeFocus"
      >
        <div class="history-heading">
          <strong :id="headingId">历史对话</strong>
          <Button variant="ghost" size="xs" :disabled="busy" @click="editWorkspace()"><Plus /> 工作区</Button>
        </div>

        <form v-if="editor" class="workspace-editor" @submit.prevent="submitWorkspace">
          <Input ref="nameInput" v-model="name" maxlength="80" size="sm" aria-label="工作区名称" placeholder="工作区名称" />
          <Button type="submit" size="xs" :disabled="busy || !name.trim()">保存</Button>
          <Button type="button" variant="ghost" size="xs" @click="editor = null">取消</Button>
        </form>
        <p v-if="error" class="history-error" role="alert">{{ error }}</p>

        <section v-for="workspace in store.workspaces" :key="workspace.id" class="history-group">
          <div class="workspace-row" :class="{ active: workspace.id === store.workspaceId }">
            <button class="workspace-toggle" type="button" :aria-expanded="expanded.has(workspace.id)" @click="toggle(workspace.id)">
              <component :is="expanded.has(workspace.id) ? ChevronDown : ChevronRight" :size="13" />
              <Folder :size="14" />
              <span class="history-label" :title="workspace.name">{{ workspace.name }}</span>
            </button>
            <IconButton :icon="Plus" size="sm" :aria-label="`在「${workspace.name}」新建对话`" :action="() => navigate('new', workspace.id)" />
            <IconButton :icon="Pencil" size="sm" :disabled="busy" :aria-label="`重命名「${workspace.name}」`" :action="() => editWorkspace(workspace)" />
            <IconButton v-if="store.workspaces[0].id !== workspace.id" :icon="Pin" size="sm" :disabled="busy"
              :aria-label="`将「${workspace.name}」移到第一位`" :action="() => update(() => store.putWorkspaceFirst(workspace.id))" />
          </div>
          <div v-if="expanded.has(workspace.id)" class="history-entries">
            <p v-if="!groups.get(workspace.id)?.length" class="history-empty">暂无对话</p>
            <div v-for="conversation in groups.get(workspace.id)" :key="conversation.id" class="history-entry">
              <div class="conversation-row">
                <button type="button" class="conversation-title" :class="{ active: conversation.id === store.current?.id }"
                  :title="conversation.title" @click="navigate('open', conversation.id)">{{ conversation.title }}</button>
                <Button v-if="store.workspaces.length > 1" variant="ghost" size="xs" :disabled="busy"
                  :aria-label="`移动「${conversation.title}」`"
                  @click="moving = moving === conversation.id ? null : conversation.id">移动</Button>
              </div>
              <select v-if="moving === conversation.id" class="workspace-picker" :disabled="busy" value="" aria-label="移动到工作区"
                @change="moveConversation(conversation, $event.target.value)">
                <option disabled value="">移动到工作区</option>
                <option v-for="target in store.workspaces.filter(item => item.id !== workspace.id)" :key="target.id" :value="target.id">{{ target.name }}</option>
              </select>
            </div>
          </div>
        </section>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<style scoped>
.ai-history-popover{z-index:var(--z-select);width:min(360px,calc(100vw - 20px));max-height:min(440px,var(--reka-popover-content-available-height));overflow-y:auto;padding:8px;border:1px solid var(--app-border-dark);border-radius:10px;background:var(--app-panel-bg,var(--app-bg));color:var(--app-text);box-shadow:var(--niri-shadow-dialog);outline:none;font-size:12px}
.history-heading,.workspace-row,.conversation-row,.workspace-editor{display:flex;align-items:center;gap:4px;min-width:0}
.history-heading{justify-content:space-between;padding:0 4px 4px}
.workspace-editor{margin:4px 0 8px}
.workspace-editor :deep([data-slot="input"]){flex:1;min-width:0}
.history-group+.history-group{margin-top:4px}
.workspace-row,.conversation-title{border-radius:6px}
.workspace-row.active,.conversation-title.active{background:var(--app-btn-hover)}
.workspace-toggle,.conversation-title{min-width:0;flex:1;text-align:left;padding:5px 6px}
.workspace-toggle{display:flex;align-items:center;gap:5px;font-weight:600}
.workspace-toggle svg{flex-shrink:0}
.history-label,.conversation-title{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.workspace-toggle:focus-visible,.conversation-title:focus-visible{outline:none;box-shadow:var(--app-focus-shadow)}
.history-entries{padding:1px 0 3px 22px}
.conversation-title:hover{background:var(--app-btn-hover)}
.history-empty,.history-error{padding:4px 6px;color:var(--app-text-muted)}
.history-error{color:var(--color-destructive,#e16b6b)}
.workspace-picker{width:100%;margin:4px 0 6px;padding:4px 6px;border:1px solid var(--app-border-light);border-radius:5px;background:var(--app-input-bg);color:var(--app-text)}
</style>
