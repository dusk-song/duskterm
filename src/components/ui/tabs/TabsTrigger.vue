<script setup>
import { reactiveOmit } from "@vueuse/core";
import { TabsTrigger, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  value: { type: [String, Number], required: true },
  disabled: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});

const delegatedProps = reactiveOmit(props, "class");

const forwardedProps = useForwardProps(delegatedProps);
</script>

<template>
  <TabsTrigger
    data-slot="tabs-trigger"
    :class="
      cn(
        'gap-1.5 rounded-[8px] border border-transparent px-2.5 py-1 text-xs font-semibold group-data-[variant=default]/tabs-list:data-[state=active]:shadow-sm group-data-[variant=line]/tabs-list:data-[state=active]:shadow-none [&_svg:not([class*=size-])]:size-3.5 has-[[data-icon=inline-end]]:pr-1.5 has-[[data-icon=inline-start]]:pl-1.5 relative inline-flex h-[28px] flex-1 items-center justify-center whitespace-nowrap text-muted-foreground transition-[background,border-color,color,box-shadow] group-data-[orientation=vertical]/tabs:w-full group-data-[orientation=vertical]/tabs:justify-start hover:text-foreground focus-visible:border-[var(--app-focus-border)] focus-visible:[box-shadow:var(--app-focus-shadow)] disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0',
        'group-data-[variant=line]/tabs-list:bg-transparent group-data-[variant=line]/tabs-list:data-[state=active]:bg-transparent group-data-[variant=line]/tabs-list:data-[state=active]:border-transparent',
        'data-[state=active]:border-primary/30 data-[state=active]:bg-primary/15 data-[state=active]:text-foreground',
        'after:absolute after:bg-primary after:opacity-0 after:transition-opacity group-data-[orientation=horizontal]/tabs:after:inset-x-2 group-data-[orientation=horizontal]/tabs:after:bottom-[-4px] group-data-[orientation=horizontal]/tabs:after:h-0.5 group-data-[orientation=vertical]/tabs:after:inset-y-1 group-data-[orientation=vertical]/tabs:after:-right-1 group-data-[orientation=vertical]/tabs:after:w-0.5 group-data-[variant=line]/tabs-list:data-[state=active]:after:opacity-100',
        props.class,
      )
    "
    v-bind="forwardedProps"
  >
    <slot />
  </TabsTrigger>
</template>
