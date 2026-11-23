<script setup>
import { Textarea } from '@/components/ui/textarea';
import SettingsSelect from '@/components/settings/SettingsSelect.vue';
import { toast } from '@/composables/useToast';
import { formatStringInput } from './redisBytes';

const value = defineModel({ type: Object, required: true });
const props = defineProps({ result: { type: Object, required: true }, disabled: Boolean });
const formats = [
  { value: 'utf8', label: 'UTF-8' },
  { value: 'hex', label: '十六进制' },
  { value: 'base64', label: 'Base64' },
];

function switchFormat(format) {
  if (format === value.value.format || props.result.error) return;
  const text = formatStringInput(props.result, format);
  if (text === null) {
    toast.warning('该值无法用 UTF-8 无损编辑，可使用十六进制或 Base64');
    return;
  }
  value.value = { format, text };
}
</script>

<template>
  <section class="grid min-w-0 gap-2">
    <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <span>编辑格式</span>
      <SettingsSelect item-class="text-xs" :model-value="value.format" :options="formats" aria-label="String 编辑格式" class="!w-36"
        :disabled="disabled || !!result.error" @update:model-value="switchFormat" />
      <span v-if="!result.error">{{ result.length }} B · 上限 64 KiB</span>
    </div>
    <Textarea :model-value="value.text" aria-label="String 值" :aria-invalid="!!result.error" :disabled="disabled"
      autocomplete="off" spellcheck="false" class="min-h-28 font-mono"
      :placeholder="value.format === 'hex' ? '例如 00 FF 2A' : value.format === 'base64' ? '例如 AP8q' : '输入 UTF-8 文本'"
      @update:model-value="text => value = { format: value.format, text }" />
    <p v-if="result.error" class="text-xs text-destructive" role="alert">{{ result.error }}</p>
  </section>
</template>
