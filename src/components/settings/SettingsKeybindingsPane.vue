<script setup>
import Input from '@/components/ui/input/Input.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { HelpCircle } from '@lucide/vue';

defineProps({
  keybindingItems: {
    type: Array,
    required: true
  },
  keybindings: {
    type: Object,
    required: true
  },
  keybindingConflictMap: {
    type: Object,
    required: true
  },
  bindingActionKey: {
    type: String,
    default: ''
  },
  keybindingConflictEntries: {
    type: Array,
    required: true
  },
  onKeybindingInputFocus: {
    type: Function,
    required: true
  },
  onKeybindingInputBlur: {
    type: Function,
    required: true
  },
  onKeybindingInputKeydown: {
    type: Function,
    required: true
  }
});
</script>

<template>
  <div class="settings-content">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">按键映射</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            点击输入框后，直接按键即可绑定；按 Backspace/Delete 可清空，留空即禁用该功能。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="kb-table">
        <div class="kb-header">
          <span class="kb-col-label">功能</span>
          <span class="kb-col-key">快捷键</span>
        </div>
        <div v-for="item in keybindingItems" :key="item.key" class="kb-row">
          <span class="kb-col-label">{{ item.label }}</span>
          <div class="kb-col-key">
            <Input :model-value="keybindings[item.key]" :placeholder="item.placeholder" readonly size="sm"
              class="cursor-pointer w-full" @focus="onKeybindingInputFocus(item.key)"
              @blur="onKeybindingInputBlur(item.key)" @keydown="onKeybindingInputKeydown(item.key, $event)" />
            <span v-if="keybindingConflictMap[item.key]" class="kb-hint error">冲突</span>
            <span v-else-if="!keybindings[item.key]" class="kb-hint muted">未设置</span>
          </div>
        </div>
      </div>
      <div v-if="keybindingConflictEntries.length" class="keybinding-conflict-list">
        <div v-for="entry in keybindingConflictEntries" :key="entry.combo" class="keybinding-conflict-item">
          {{ entry.labels.join('、') }} 使用了相同快捷键：{{ entry.combo }}
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

.kb-table {
  @apply border border-[hsl(var(--border))] rounded-[var(--radius-md)] overflow-hidden;
}

.kb-header {
  @apply flex items-center gap-2 px-2.5 py-[5px] text-[11px] font-semibold text-[hsl(var(--muted-foreground))] bg-[hsl(var(--secondary))] border-b border-[hsl(var(--border))] select-none;
}

.kb-row {
  @apply flex items-center gap-2 px-2.5 py-1 text-xs border-b border-[hsl(var(--border)/0.5)] transition-colors duration-100;
}

.kb-row:last-child {
  @apply border-b-0;
}

.kb-row:hover {
  background: hsl(var(--accent) / 0.06);
}

.kb-col-label {
  @apply flex-1 min-w-0 text-[12px] text-[var(--app-text)];
}

.kb-col-key {
  @apply flex-1 min-w-0 flex items-center gap-1.5;
}

.keybinding-conflict-list {
  @apply mt-2;
}

.keybinding-conflict-item {
  @apply text-xs text-[var(--color-danger)];
}

.kb-hint {
  @apply text-[11px] shrink-0;
}

.kb-hint.error {
  @apply text-[var(--color-danger)];
}

.kb-hint.muted {
  @apply text-[var(--app-text-muted)];
}
</style>
