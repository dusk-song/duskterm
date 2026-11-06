<script setup>
import { reactiveOmit } from '@vueuse/core';
import { DropdownMenuItem, useForwardPropsEmits } from 'reka-ui';
import { cn } from '@/lib/utils';
const props = defineProps({
  disabled: { type: Boolean, required: false },
  textValue: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: String, default: '' },
  variant: { type: String, default: 'default' },
});
const emit = defineEmits(['select']);
const forwarded = useForwardPropsEmits(reactiveOmit(props, 'class'), emit);
</script>

<template>
  <DropdownMenuItem v-bind="forwarded" data-slot="dropdown-menu-item" :data-variant="variant"
    :class="cn('app-context-menu-item', props.class)"><slot /></DropdownMenuItem>
</template>
