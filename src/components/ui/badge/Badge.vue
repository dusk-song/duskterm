<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

export type BadgeVariant = "default" | "secondary" | "destructive" | "outline" | "success";

defineProps<{
  variant?: BadgeVariant;
  class?: HTMLAttributes["class"];
}>();
</script>

<template>
  <span :class="cn(
    'inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold transition-colors',
    {
      'bg-primary text-primary-foreground hover:bg-primary/80': variant === 'default' || !variant,
      'bg-secondary text-secondary-foreground hover:bg-secondary/80': variant === 'secondary',
      'bg-destructive text-destructive-foreground hover:bg-destructive/80': variant === 'destructive',
      'text-foreground border border-border': variant === 'outline',
      'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-100': variant === 'success',
    },
    $props.class,
  )
    ">
    <slot />
  </span>
</template>
