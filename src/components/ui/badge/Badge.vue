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
    'inline-flex items-center rounded-full border border-transparent px-2.5 py-0.5 text-xs font-semibold',
    {
      'bg-primary text-primary-foreground': variant === 'default' || !variant,
      'bg-secondary text-secondary-foreground': variant === 'secondary',
      'bg-destructive text-destructive-foreground': variant === 'destructive',
      'text-foreground border-border': variant === 'outline',
      'bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-100': variant === 'success',
    },
    $props.class,
  )
    ">
    <slot />
  </span>
</template>
