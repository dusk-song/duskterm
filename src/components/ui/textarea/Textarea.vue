<script setup>
import { useVModel } from "@vueuse/core";
import { cn } from "@/lib/utils";

const props = defineProps({
  class: { type: null, required: false },
  defaultValue: { type: [String, Number], required: false },
  modelValue: { type: [String, Number], required: false },
});

const emits = defineEmits(["update:modelValue"]);

const modelValue = useVModel(props, "modelValue", emits, {
  passive: true,
  defaultValue: props.defaultValue,
});
</script>

<template>
  <textarea
    v-model="modelValue"
    data-slot="textarea"
    :class="
      cn(
        'border-input dark:bg-input/30 focus-visible:border-[var(--app-focus-border)] focus-visible:shadow-[var(--app-focus-shadow)] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 rounded-md border bg-transparent px-2.5 py-2 text-base shadow-xs transition-[color,border-color,box-shadow] aria-invalid:ring-3 md:text-sm flex field-sizing-content min-h-16 w-full resize-y outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50',
        props.class,
      )
    "
  />
</template>

<style scoped>
textarea[data-slot='textarea']::-webkit-resizer {
  background:
    linear-gradient(135deg, transparent 0 46%, var(--app-input-border) 47% 53%, transparent 54%),
    linear-gradient(135deg, transparent 0 62%, color-mix(in srgb, var(--app-text-muted) 36%, transparent) 63% 69%, transparent 70%);
  background-color: var(--app-input-bg);
  border: 1px solid var(--app-input-border);
  border-bottom-right-radius: calc(var(--radius) - 2px);
}

textarea[data-slot='textarea']::-webkit-scrollbar-corner {
  background: transparent;
}
</style>
