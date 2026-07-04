<script setup lang="ts">
import { cn } from "@/lib/utils";

defineProps<{
  class?: string;
  disabled?: boolean;
}>();

const model = defineModel<string>("modelValue");
</script>

<template>
  <div :class="cn('flex items-center gap-2', $props.class)">
    <div role="radiogroup" class="flex flex-wrap gap-2">
      <slot />
    </div>
  </div>
</template>
