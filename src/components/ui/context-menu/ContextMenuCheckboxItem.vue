<script setup>
import { CheckIcon } from "@lucide/vue";

import { reactiveOmit } from "@vueuse/core";
import {
  ContextMenuCheckboxItem,
  ContextMenuItemIndicator,
  useForwardPropsEmits,
} from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  modelValue: { type: [Boolean, String], required: false },
  disabled: { type: Boolean, required: false },
  textValue: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});
const emits = defineEmits(["select", "update:modelValue"]);

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <ContextMenuCheckboxItem
    data-slot="context-menu-checkbox-item"
    v-bind="forwarded"
    :class="
      cn(
        'app-context-menu-item',
        props.class,
      )
    "
  >
    <span class="absolute right-2 pointer-events-none">
      <ContextMenuItemIndicator>
        <slot name="indicator-icon">
          <CheckIcon />
        </slot>
      </ContextMenuItemIndicator>
    </span>
    <slot />
  </ContextMenuCheckboxItem>
</template>
