<script setup>
import Button from '@/components/ui/button/Button.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import Textarea from '@/components/ui/textarea/Textarea.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { Pencil, Plus, Trash2 } from '@lucide/vue';
import { ref } from 'vue';

const STORAGE_KEY = 'quick-commands-v1';

const props = defineProps({
  embedded: {
    type: Boolean,
    default: false
  }
});

const commands = ref(loadCommands());
const modalVisible = ref(false);
const modalTitle = ref('新增命令');
const form = ref({ id: '', name: '', command: '' });

function loadCommands() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch (e) {
    return [];
  }
}

function saveCommands() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(commands.value));
    window.dispatchEvent(new CustomEvent('quick-commands-changed'));
  } catch (e) {
    toast.error('保存失败');
  }
}

function openAdd() {
  modalTitle.value = '新增命令';
  form.value = { id: crypto.randomUUID(), name: '', command: '' };
  modalVisible.value = true;
}

function openEdit(cmd) {
  modalTitle.value = '编辑命令';
  form.value = { ...cmd };
  modalVisible.value = true;
}

function handleSave() {
  const name = form.value.name?.trim();
  const command = form.value.command?.trim();
  if (!name || !command) {
    toast.warning('请填写名称和命令');
    return;
  }
  const index = commands.value.findIndex(c => c.id === form.value.id);
  if (index >= 0) {
    commands.value[index] = { ...form.value, name, command };
  } else {
    commands.value.unshift({ ...form.value, name, command });
  }
  saveCommands();
  modalVisible.value = false;
}

function handleDelete(cmd) {
  confirm({
    title: '删除命令',
    content: `确认删除 “${cmd.name}” 吗？`,
    okText: '删除',
    cancelText: '取消',
    onOk() {
      commands.value = commands.value.filter(c => c.id !== cmd.id);
      saveCommands();
    }
  });
}

function handleInsert(cmd) {
  window.dispatchEvent(new CustomEvent('quick-command-insert', { detail: cmd.command }));
}
</script>

<template>
  <div class="qc-panel" :class="{ embedded: props.embedded }">
    <div class="qc-actions">
      <Button size="sm" @click="openAdd">
        <Plus :size="14" /> 新增
      </Button>
    </div>

    <div v-if="commands.length === 0" class="qc-empty">暂无命令</div>

    <div v-else class="qc-table">
      <div class="qc-header">
        <span class="qc-col-index">#</span>
        <span class="qc-col-name">名称</span>
        <span class="qc-col-cmd">命令</span>
        <span class="qc-col-action">操作</span>
      </div>
      <div v-for="(cmd, index) in commands" :key="cmd.id" class="qc-row" @dblclick="handleInsert(cmd)">
        <span class="qc-col-index">{{ index + 1 }}</span>
        <span class="qc-col-name">{{ cmd.name }}</span>
        <code class="qc-col-cmd">{{ cmd.command }}</code>
        <span class="qc-col-action">
          <Button variant="default" size="icon" @click="openEdit(cmd)" aria-label="编辑">
            <Pencil :size="14" />
          </Button>
          <Button variant="destructive" size="icon" @click="handleDelete(cmd)" aria-label="删除">
            <Trash2 :size="14" />
          </Button>
        </span>
      </div>
    </div>

    <Dialog :open="modalVisible" @update:open="(v) => { if (!v) modalVisible = false; }">
      <DialogContent class="max-w-md">
        <DialogHeader>
          <DialogTitle>{{ modalTitle }}</DialogTitle>
        </DialogHeader>
        <div class="flex flex-col gap-3">
          <div class="form-item">
            <label class="block text-xs text-muted-foreground mb-1">名称</label>
            <Input v-model="form.name" placeholder="如：查看磁盘" size="sm" class="w-full" />
          </div>
          <div class="form-item">
            <label class="block text-xs text-muted-foreground mb-1">命令</label>
            <Textarea v-model="form.command" :rows="3" placeholder="df -h" class="qc-textarea" />
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" @click="modalVisible = false">取消</Button>
          <Button @click="handleSave">保存</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<style scoped>
@import '../settings/settingsPaneShared.css';

.qc-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: transparent;
}

.qc-actions {
  @apply flex gap-1.5 mb-2;
}

.qc-table {
  @apply border border-[hsl(var(--border))] rounded-[var(--radius-md)] overflow-hidden;
}

.qc-header {
  @apply flex items-center gap-2 px-2.5 py-[5px] text-[11px] font-semibold text-[hsl(var(--muted-foreground))] bg-[hsl(var(--secondary))] border-b border-[hsl(var(--border))] select-none;
}

.qc-row {
  @apply flex items-center gap-2 px-2.5 py-1 text-xs border-b border-[hsl(var(--border)/0.5)] transition-colors duration-100;
}

.qc-row:last-child {
  @apply border-b-0;
}

.qc-row:hover {
  background: hsl(var(--accent) / 0.06);
}

.qc-col-index {
  @apply w-7 shrink-0 text-center text-[11px] text-[hsl(var(--muted-foreground))];
}

.qc-col-name {
  @apply w-[100px] shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-[12px] text-[hsl(var(--foreground))];
}

.qc-col-cmd {
  @apply flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[12px] text-[hsl(var(--foreground))];
  font-family: var(--font-mono);
}

.qc-header .qc-col-cmd {
  font-family: inherit;
}

.qc-col-action {
  @apply w-[52px] shrink-0 flex justify-center gap-3;
}

.qc-empty {
  @apply py-4 text-[hsl(var(--muted-foreground))] text-center text-xs;
}

.qc-textarea {
  @apply w-full resize-y min-h-[72px] rounded-[var(--radius-md)] px-2.5 py-1.5 text-xs outline-none transition-colors;
  background: var(--app-input-bg);
  border: 1px solid var(--app-border-shadow);
  color: var(--app-text);
}

.qc-textarea:focus {
  border-color: var(--color-primary);
}

.qc-textarea::placeholder {
  color: var(--app-text-muted);
}
</style>
