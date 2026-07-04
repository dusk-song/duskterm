<script setup>
import { cn } from "@/lib/utils";
import { useVModel } from "@vueuse/core";

const props = defineProps({
  defaultValue: { type: [String, Number], required: false },
  modelValue: { type: [String, Number], required: false },
  class: { type: null, required: false },
  size: { type: String, required: false },
});

const emits = defineEmits(["update:modelValue"]);

const modelValue = useVModel(props, "modelValue", emits, {
  passive: true,
  defaultValue: props.defaultValue,
});
</script>

<template>
  <input v-model="modelValue" data-slot="input" :class="cn(
    'border-input bg-[var(--app-input-bg)] focus-visible:border-[var(--app-focus-border)] focus-visible:shadow-[var(--app-focus-shadow)] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/30 aria-invalid:border-destructive h-[30px] rounded-[8px] border px-2.5 py-1 text-sm shadow-none transition-[border-color,box-shadow] file:h-7 file:text-sm file:font-medium w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent file:text-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50',
    props.class,
  )
    " />
</template>
