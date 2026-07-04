<script setup>
import { ChevronDownIcon } from "@lucide/vue";

import { reactiveOmit } from "@vueuse/core";
import { SelectIcon, SelectTrigger, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  disabled: { type: Boolean, required: false },
  reference: { type: null, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  size: { type: String, required: false, default: "default" },
});

const delegatedProps = reactiveOmit(props, "class", "size");
const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <SelectTrigger
    data-slot="select-trigger"
    :data-size="size"
    v-bind="forwardedProps"
    :class="
      cn(
        'border-input bg-[var(--app-input-bg)] data-placeholder:text-muted-foreground hover:bg-accent/50 focus-visible:border-[var(--app-focus-border)] focus-visible:shadow-[var(--app-focus-shadow)] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/30 aria-invalid:border-destructive gap-1.5 rounded-[8px] border py-1 pr-2 pl-2.5 text-sm shadow-none transition-[background,border-color,box-shadow] data-[size=default]:h-[30px] data-[size=sm]:h-7 *:data-[slot=select-value]:gap-1.5 [&_svg:not([class*=size-])]:size-3.5 flex w-fit items-center justify-between whitespace-nowrap outline-none disabled:cursor-not-allowed disabled:opacity-50 *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:flex *:data-[slot=select-value]:items-center [&_svg]:pointer-events-none [&_svg]:shrink-0',
        props.class,
      )
    "
  >
    <slot />
    <SelectIcon as-child>
      <ChevronDownIcon
        class="text-muted-foreground size-4 pointer-events-none"
      />
    </SelectIcon>
  </SelectTrigger>
</template>
