<script setup>
import { computed } from 'vue';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';

defineOptions({ inheritAttrs: false });
const props = defineProps({ modelValue: { default: null }, options: { type: Array, required: true }, disabled: Boolean, itemClass: { type: String, default: '' }, triggerLabel: { type: String, default: null } });
const emit = defineEmits(['update:modelValue']);
const key = value => value == null || value === '' ? '__default__' : String(value);
const selected = computed({ get: () => key(props.modelValue), set: value => {
  const option = props.options.find(o => key(o.value) === value);
  if (option) emit('update:modelValue', option.value);
} });
</script>

<template>
  <Select v-model="selected" :disabled="disabled">
    <SelectTrigger v-bind="$attrs" size="sm" class="w-full min-w-0"><SelectValue v-if="triggerLabel != null">{{ triggerLabel }}</SelectValue><SelectValue v-else /></SelectTrigger>
    <SelectContent position="popper"><SelectItem v-for="option in options" :key="key(option.value)" :value="key(option.value)" :disabled="option.disabled" :class="itemClass">{{ option.label }}</SelectItem></SelectContent>
  </Select>
</template>
