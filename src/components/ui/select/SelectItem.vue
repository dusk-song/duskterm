<script setup>
import { CheckIcon } from "@lucide/vue";

import { reactiveOmit } from "@vueuse/core";
import {
  SelectItem,
  SelectItemIndicator,
  SelectItemText,
  useForwardProps,
} from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  value: { type: null, required: true },
  disabled: { type: Boolean, required: false },
  textValue: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});

const delegatedProps = reactiveOmit(props, "class");

const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <SelectItem
    data-slot="select-item"
    v-bind="forwardedProps"
    :class="
      cn(
        'data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground data-[state=checked]:bg-primary/10 data-[state=checked]:text-foreground data-[state=checked]:data-[highlighted]:bg-primary/15 active:bg-primary/20 gap-2 rounded-[5px] py-1.5 pr-8 pl-2 text-sm [&_svg:not([class*=size-])]:size-4 [&>[data-slot=select-item-text]]:inline-flex [&>[data-slot=select-item-text]]:items-center [&>[data-slot=select-item-text]]:gap-2 relative flex w-full cursor-default items-center outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0',
        props.class,
      )
    "
  >
    <span
      class="pointer-events-none absolute right-2 flex size-4 items-center justify-center"
    >
      <SelectItemIndicator>
        <slot name="indicator-icon">
          <CheckIcon class="pointer-events-none" />
        </slot>
      </SelectItemIndicator>
    </span>

    <SelectItemText data-slot="select-item-text">
      <slot />
    </SelectItemText>
  </SelectItem>
</template>
