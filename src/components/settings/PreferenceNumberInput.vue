<script setup>
import { ref, watch } from 'vue';
import Input from '@/components/ui/input/Input.vue';
import { toast } from '@/composables/useToast';

const props = defineProps({
  modelValue: { type: [Number, String], required: true },
  min: Number, max: Number, step: Number,
  integer: { type: Boolean, default: false },
  label: { type: String, default: '数值' },
});
const emit = defineEmits(['update:modelValue']);
const draft = ref(String(props.modelValue));
watch(() => props.modelValue, value => { draft.value = String(value); });
function commit() {
  const text = draft.value.trim();
  const value = Number(text);
  if (!/^-?(?:\d+\.?\d*|\.\d+)$/.test(text) || !Number.isFinite(value)
    || (props.integer && !Number.isInteger(value))
    || (props.min != null && value < props.min) || (props.max != null && value > props.max)) {
    toast.error(`${props.label}须为${props.min != null && props.max != null ? ` ${props.min}–${props.max} 范围内的` : '有效'}${props.integer ? '整数' : '数值'}`);
    draft.value = String(props.modelValue);
    return;
  }
  draft.value = String(value);
  if (value !== Number(props.modelValue)) emit('update:modelValue', value);
}
</script>

<template>
  <Input v-model="draft" type="text" :inputmode="integer ? 'numeric' : 'decimal'" :aria-label="label" @blur="commit" @keydown.enter.prevent="$event.target.blur()" />
</template>
