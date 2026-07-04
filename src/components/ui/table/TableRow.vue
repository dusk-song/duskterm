<script setup>
import { cn } from "@/lib/utils";

const props = defineProps({
  class: { type: null, required: false },
});
</script>

<template>
  <tr
    data-slot="table-row"
    :class="
      cn(
        'hover:bg-accent/50 data-[state=selected]:bg-primary/15 border-b border-border/70 transition-colors has-aria-expanded:bg-accent/50',
        props.class,
      )
    "
  >
    <slot />
  </tr>
</template>
