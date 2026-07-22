<script setup>
import { Check, ChevronDown } from '@lucide/vue';
import { computed, nextTick, onMounted, onUnmounted, ref, useId } from 'vue';

const props = defineProps({
  modelValue: { type: String, default: '' },
  mode: { type: String, default: 'auto' },
  options: { type: Array, default: () => [] },
  placeholder: { type: String, default: '' },
  automaticLabel: { type: String, default: '自动检测' },
  customLabel: { type: String, default: '自定义' },
  ariaLabel: { type: String, default: '' },
  disabled: Boolean,
});

const emit = defineEmits(['update:modelValue', 'update:mode']);
const rootRef = ref(null);
const inputRef = ref(null);
const open = ref(false);
const listboxId = useId();

const displayValue = computed(() => {
  if (props.mode === 'auto') {
    const detected = props.options.find((item) => item.value === props.modelValue);
    return detected ? `${props.automaticLabel}（${detected.label}）` : props.automaticLabel;
  }
  return props.options.find((item) => item.value === props.modelValue)?.label || props.modelValue;
});

function selectAutomatic() {
  emit('update:mode', 'auto');
  emit('update:modelValue', props.options[0]?.value || '');
  open.value = false;
}

function selectOption(value) {
  emit('update:mode', 'detected');
  emit('update:modelValue', value);
  open.value = false;
}

async function selectCustom() {
  emit('update:mode', 'custom');
  if (props.options.some((item) => item.value === props.modelValue)) emit('update:modelValue', '');
  open.value = false;
  await nextTick();
  inputRef.value?.focus();
}

function updateCustomValue(event) {
  emit('update:modelValue', event.target.value);
}

function toggleOpen() {
  if (props.disabled) return;
  open.value = !open.value;
}

function onKeydown(event) {
  if (event.key === 'Escape' && open.value) {
    event.preventDefault();
    open.value = false;
    inputRef.value?.focus();
    return;
  }
  if (event.key === 'ArrowDown' && !open.value) {
    event.preventDefault();
    open.value = true;
    return;
  }
  if (
    event.target === inputRef.value
    && props.mode !== 'custom'
    && (event.key === 'Enter' || event.key === ' ')
  ) {
    event.preventDefault();
    toggleOpen();
  }
}

function onDocumentPointerDown(event) {
  if (!rootRef.value?.contains(event.target)) open.value = false;
}

onMounted(() => document.addEventListener('pointerdown', onDocumentPointerDown));
onUnmounted(() => document.removeEventListener('pointerdown', onDocumentPointerDown));
</script>

<template>
  <div ref="rootRef" class="editable-select" :class="{ 'is-open': open, 'is-disabled': disabled }" @keydown="onKeydown">
    <div class="editable-select-trigger" :class="{ 'is-custom': mode === 'custom' }">
      <input ref="inputRef" class="editable-select-input" :value="mode === 'custom' ? modelValue : displayValue"
        :readonly="mode !== 'custom'" :disabled="disabled" :placeholder="placeholder" autocomplete="off"
        role="combobox" aria-haspopup="listbox" :aria-label="ariaLabel || undefined" :aria-controls="listboxId"
        :aria-expanded="open" :aria-readonly="mode !== 'custom'" :aria-autocomplete="mode === 'custom' ? 'list' : 'none'"
        @input="updateCustomValue" @click="mode === 'custom' ? null : toggleOpen()" />
      <button type="button" class="editable-select-toggle" :disabled="disabled" aria-label="展开选项"
        aria-haspopup="listbox" :aria-controls="listboxId" :aria-expanded="open" @click="toggleOpen">
        <ChevronDown :size="14" />
      </button>
    </div>

    <div v-if="open" :id="listboxId" class="editable-select-content" role="listbox">
      <button type="button" class="editable-select-item" :class="{ selected: mode === 'auto' }"
        role="option" :aria-selected="mode === 'auto'" @click="selectAutomatic">
        <span>{{ automaticLabel }}</span><Check v-if="mode === 'auto'" :size="14" />
      </button>
      <button v-for="item in options" :key="item.value" type="button" class="editable-select-item"
        :class="{ selected: mode === 'detected' && modelValue === item.value }" role="option"
        :aria-selected="mode === 'detected' && modelValue === item.value" @click="selectOption(item.value)">
        <span>{{ item.label }}</span><Check v-if="mode === 'detected' && modelValue === item.value" :size="14" />
      </button>
      <button type="button" class="editable-select-item" :class="{ selected: mode === 'custom' }"
        role="option" :aria-selected="mode === 'custom'" @click="selectCustom">
        <span>{{ customLabel }}</span><Check v-if="mode === 'custom'" :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.editable-select {
  position: relative;
  width: 100%;
}

.editable-select-trigger {
  display: flex;
  height: 28px;
  align-items: center;
  overflow: hidden;
  border: 1px solid var(--app-border-shadow);
  border-radius: 8px;
  background: var(--app-input-bg);
  transition: border-color var(--app-motion-control), box-shadow var(--app-motion-control);
}

.editable-select-trigger:focus-within,
.editable-select.is-open .editable-select-trigger {
  border-color: var(--app-focus-border);
  box-shadow: var(--app-focus-shadow);
}

.editable-select-input {
  min-width: 0;
  height: 100%;
  flex: 1;
  padding: 0 10px;
  border: 0;
  outline: 0;
  color: var(--app-text);
  background: transparent;
  font-size: 13px;
}

.editable-select-input[readonly] {
  cursor: default;
}

.editable-select-toggle {
  display: grid;
  width: 30px;
  height: 100%;
  flex: 0 0 auto;
  place-items: center;
  border: 0;
  color: var(--app-text-muted);
  background: transparent;
}

.editable-select-toggle:hover {
  color: var(--app-text);
  background: color-mix(in srgb, var(--app-text) 6%, transparent);
}

.editable-select-content {
  position: absolute;
  z-index: var(--z-select);
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  max-height: 220px;
  overflow-y: auto;
  padding: 4px;
  border: 1px solid var(--app-border-dark);
  border-radius: 10px;
  background: var(--app-bg-dialog);
  box-shadow: var(--niri-shadow-dialog);
}

.editable-select-item {
  display: flex;
  width: 100%;
  min-height: 28px;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 4px 8px;
  border: 0;
  border-radius: 6px;
  color: var(--app-text);
  background: transparent;
  font-size: 13px;
  text-align: left;
}

.editable-select-item:hover,
.editable-select-item:focus-visible {
  outline: none;
  background: hsl(var(--accent) / 0.16);
}

.editable-select-item.selected {
  color: hsl(var(--primary));
  background: hsl(var(--accent) / 0.12);
}

.editable-select.is-disabled {
  pointer-events: none;
  opacity: .5;
}
</style>
