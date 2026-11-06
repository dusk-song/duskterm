<script setup>
import { cn } from '@/lib/utils';
import { DropdownMenuRoot, DropdownMenuTrigger, DropdownMenuPortal, DropdownMenuContent } from 'reka-ui';

defineOptions({ inheritAttrs: false });
const props = defineProps({
  class: { type: String, default: '' },
  hint: { type: String, default: '' },
  side: { type: String, default: 'bottom' },
  align: { type: String, default: 'start' },
  sideOffset: { type: Number, default: 4 },
  collisionPadding: { type: Number, default: 8 },
});
const open = defineModel('open', { type: Boolean, default: false });
const emit = defineEmits(['closeAutoFocus']);
</script>

<template>
  <DropdownMenuRoot v-model:open="open">
    <DropdownMenuTrigger as-child :title="props.hint || undefined"><slot name="trigger" /></DropdownMenuTrigger>
    <DropdownMenuPortal>
      <DropdownMenuContent v-bind="$attrs" data-slot="dropdown-menu-content"
        :side="side" :align="align" :side-offset="sideOffset" :collision-padding="collisionPadding"
        :class="cn('app-context-menu-content', props.class)"
        @close-auto-focus="emit('closeAutoFocus', $event)">
        <slot />
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>

<style src="../menu.css"></style>
