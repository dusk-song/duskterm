<script setup>
import { reactiveOmit } from "@vueuse/core";
import { ContextMenuTrigger, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  disabled: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});

const delegatedProps = reactiveOmit(props, "class");
const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <ContextMenuTrigger
    data-slot="context-menu-trigger"
    v-bind="forwardedProps"
    :class="cn('select-none', props.class)"
  >
    <slot />
  </ContextMenuTrigger>
</template>
