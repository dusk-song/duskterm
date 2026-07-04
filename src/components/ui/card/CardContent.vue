<script setup>
import { cn } from "@/lib/utils";

const props = defineProps({
  class: { type: null, required: false },
});
</script>

<template>
  <div
    data-slot="card-content"
    :class="cn('px-6 group-data-[size=sm]/card:px-4', props.class)"
  >
    <slot />
  </div>
</template>
