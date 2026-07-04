<script setup lang="ts">
import { cn } from "@/lib/utils";
import { inject, ref, type Ref } from "vue";

defineProps<{
  value: string;
  disabled?: boolean;
  class?: string;
}>();

const model = inject<Ref<string>>("radioGroupModel", ref(""));
</script>

<template>
  <button type="button" role="radio" :aria-checked="model === value" :disabled="disabled"
    :data-state="model === value ? 'checked' : 'unchecked'" :class="cn(
      'inline-flex items-center justify-center rounded-md px-3 py-1 text-sm font-medium transition-[background,color,box-shadow] focus-visible:outline-none focus-visible:bg-[var(--app-focus-bg)] focus-visible:shadow-[var(--app-focus-shadow)] disabled:pointer-events-none disabled:opacity-50',
      model === value
        ? 'bg-primary text-primary-foreground'
        : 'bg-secondary text-secondary-foreground hover:bg-accent',
      $props.class,
    )
      " @click="model = value">
    <slot />
  </button>
</template>
