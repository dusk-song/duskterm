<script setup>
import { computed } from 'vue';
import Tooltip from './Tooltip.vue';
import TooltipContent from './TooltipContent.vue';
import TooltipTrigger from './TooltipTrigger.vue';

const props = defineProps({
  text: { type: [String, Number], default: '' },
  disabled: { type: Boolean, default: false },
  side: { type: String, default: 'top' },
  align: { type: String, default: 'center' },
  sideOffset: { type: Number, default: 8 },
  delayDuration: { type: Number, default: 200 },
});

const content = computed(() => String(props.text ?? '').trim());
</script>

<template>
  <Tooltip :disabled="disabled || !content" :delay-duration="delayDuration">
    <TooltipTrigger as-child>
      <slot />
    </TooltipTrigger>
    <TooltipContent
      class="tooltip-hint-content"
      :side="side"
      :align="align"
      :side-offset="sideOffset"
      :avoid-collisions="true"
      :collision-padding="8"
    >
      {{ content }}
    </TooltipContent>
  </Tooltip>
</template>
