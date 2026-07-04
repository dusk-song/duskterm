<script setup>
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import Textarea from '@/components/ui/textarea/Textarea.vue';
import { computed, reactive, watch } from 'vue';

const props = defineProps({
  open: Boolean,
  entry: { type: Object, default: null },
});

const emit = defineEmits(['update:open', 'save']);

const form = reactive({
  id: '',
  title: '',
  command: '',
  trigger: '',
  tagsText: '',
  description: '',
  favorite: false,
  safetyLevel: 'normal',
  executionPolicy: 'insertOnly',
});

const executionPolicies = new Set(['insertOnly', 'confirmBeforeExecute', 'blockDirectExecute']);

const normalizePolicyForLevel = (policy, level) => {
  if (executionPolicies.has(policy)) return policy;
  if (level === 'dangerous') return 'blockDirectExecute';
  if (level === 'sensitive') return 'confirmBeforeExecute';
  return 'insertOnly';
};

const dialogOpen = computed({
  get: () => props.open,
  set: (value) => emit('update:open', value),
});

const resetForm = () => {
  const entry = props.entry || {};
  form.id = entry.id || '';
  form.title = entry.title || '';
  form.command = entry.command || '';
  form.trigger = entry.trigger || '';
  form.tagsText = Array.isArray(entry.tags) ? entry.tags.join(', ') : '';
  form.description = entry.description || '';
  form.favorite = !!entry.favorite;
  form.safetyLevel = entry.safetyLevel || 'normal';
  form.executionPolicy = normalizePolicyForLevel(entry.executionPolicy, form.safetyLevel);
};

watch(() => props.open, (open) => {
  if (open) resetForm();
});

watch(() => form.safetyLevel, (level) => {
  form.executionPolicy = normalizePolicyForLevel(form.executionPolicy, level);
});

const handleSubmit = () => {
  const title = form.title.trim();
  const command = form.command.trim();
  if (!title || !command) return;
  emit('save', {
    ...(props.entry || {}),
    id: form.id || undefined,
    title,
    command,
    trigger: form.trigger.trim(),
    tags: form.tagsText.split(',').map((tag) => tag.trim()).filter(Boolean),
    description: form.description.trim(),
    favorite: form.favorite,
    safetyLevel: form.safetyLevel,
    executionPolicy: form.executionPolicy,
  });
};
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent class="command-knowledge-dialog" show-close-button>
      <DialogHeader>
        <DialogTitle>{{ entry?.id ? '编辑命令条目' : '新增命令条目' }}</DialogTitle>
      </DialogHeader>

      <form class="knowledge-form" @submit.prevent="handleSubmit">
        <label class="knowledge-field">
          <span>标题</span>
          <Input v-model="form.title" required placeholder="例如：查看端口占用" />
        </label>

        <label class="knowledge-field">
          <span>命令内容</span>
          <Textarea v-model="form.command" required class="knowledge-command-input" placeholder="lsof -i :8080" />
        </label>

        <div class="knowledge-grid">
          <label class="knowledge-field">
            <span>快捷触发词</span>
            <Input v-model="form.trigger" placeholder="port" />
          </label>

          <label class="knowledge-field">
            <span>标签</span>
            <Input v-model="form.tagsText" placeholder="网络, 排障" />
          </label>
        </div>

        <label class="knowledge-field">
          <span>简短说明</span>
          <Textarea v-model="form.description" placeholder="用于快速定位本机端口占用进程" />
        </label>

        <div class="knowledge-grid">
          <label class="knowledge-field">
            <span>安全级别</span>
            <Select v-model="form.safetyLevel">
              <SelectTrigger size="sm" class="knowledge-select">
                <SelectValue />
              </SelectTrigger>
              <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                <SelectItem value="normal">普通</SelectItem>
                <SelectItem value="sensitive">敏感</SelectItem>
                <SelectItem value="dangerous">高危</SelectItem>
              </SelectContent>
            </Select>
          </label>

          <label class="knowledge-field">
            <span>执行策略</span>
            <Select v-model="form.executionPolicy">
              <SelectTrigger size="sm" class="knowledge-select">
                <SelectValue />
              </SelectTrigger>
              <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                <SelectItem value="insertOnly">仅插入</SelectItem>
                <SelectItem value="confirmBeforeExecute">执行前确认</SelectItem>
                <SelectItem value="blockDirectExecute">禁止直接执行</SelectItem>
              </SelectContent>
            </Select>
          </label>
        </div>

        <label class="knowledge-favorite">
          <Checkbox v-model="form.favorite" />
          <span>收藏</span>
        </label>
      </form>

      <DialogFooter>
        <Button variant="outline" size="sm" @click="dialogOpen = false">取消</Button>
        <Button size="sm" :disabled="!form.title.trim() || !form.command.trim()" @click="handleSubmit">保存</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.command-knowledge-dialog {
  width: min(720px, calc(100vw - 32px));
  max-width: min(720px, calc(100vw - 32px));
}

.knowledge-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

.knowledge-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
}

.knowledge-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--app-text-muted);
}

.knowledge-field :deep(input),
.knowledge-field :deep(textarea),
.knowledge-select {
  color: var(--app-text);
}

.knowledge-command-input {
  min-height: 92px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.knowledge-select {
  height: 34px;
  border: 1px solid var(--app-input-border, hsl(var(--border)));
  border-radius: 6px;
  background: var(--app-input-bg, hsl(var(--background)));
  padding: 0 10px;
  font-size: 13px;
  outline: none;
}

.knowledge-select:focus {
  border-color: var(--app-focus-border);
  box-shadow: var(--app-focus-shadow);
}

.knowledge-favorite {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  width: fit-content;
  font-size: 13px;
  color: var(--app-text);
}

@media (max-width: 640px) {
  .knowledge-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
