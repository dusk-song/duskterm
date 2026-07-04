<script setup>
import { cn } from "@/lib/utils";

const props = defineProps({
  class: { type: null, required: false },
});
</script>

<template>
  <span
    data-slot="context-menu-shortcut"
    :class="
      cn(
        'text-muted-foreground group-focus/context-menu-item:text-accent-foreground ml-auto text-xs tracking-widest',
        props.class,
      )
    "
  >
    <slot />
  </span>
</template>
