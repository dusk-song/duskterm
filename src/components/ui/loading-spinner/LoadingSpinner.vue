<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

defineProps<{
  class?: HTMLAttributes["class"];
  tip?: string;
}>();
</script>

<template>
  <div :class="cn('flex flex-col items-center justify-center gap-2 py-8', $props.class)">
    <div class="h-7 w-7 animate-spin rounded-full border-2 border-muted-foreground/15 border-t-primary" role="status"
      aria-label="加载中" />
    <span v-if="tip" class="text-xs text-muted-foreground">{{ tip }}</span>
  </div>
</template>
