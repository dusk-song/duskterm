<script setup>
import { ChevronRightIcon } from "@lucide/vue";

import { reactiveOmit } from "@vueuse/core";
import { ContextMenuSubTrigger, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  disabled: { type: Boolean, required: false },
  textValue: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  inset: { type: Boolean, required: false },
});

const delegatedProps = reactiveOmit(props, "class");

const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <ContextMenuSubTrigger
    data-slot="context-menu-sub-trigger"
    :data-inset="inset ? '' : undefined"
    v-bind="forwardedProps"
    :class="
      cn(
        'app-context-menu-item',
        props.class,
      )
    "
  >
    <slot />
    <ChevronRightIcon class="cn-rtl-flip ml-auto" />
  </ContextMenuSubTrigger>
</template>
