<script setup>
import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import {
  TooltipContent,
  TooltipPortal,
  useForwardPropsEmits,
} from "reka-ui";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  ariaLabel: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  side: { type: null, required: false },
  sideOffset: { type: Number, required: false, default: 0 },
  align: { type: null, required: false },
  alignOffset: { type: Number, required: false },
  avoidCollisions: { type: Boolean, required: false },
  collisionBoundary: { type: null, required: false },
  collisionPadding: { type: [Number, Object], required: false },
  arrowPadding: { type: Number, required: false },
  sticky: { type: String, required: false },
  hideWhenDetached: { type: Boolean, required: false },
  positionStrategy: { type: String, required: false },
  updatePositionStrategy: { type: String, required: false },
  class: { type: null, required: false },
});

const emits = defineEmits(["escapeKeyDown", "pointerDownOutside"]);

const delegatedProps = reactiveOmit(props, "class");
const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <TooltipPortal>
    <TooltipContent
      data-slot="tooltip-content"
      v-bind="{ ...forwarded, ...$attrs }"
      :class="cn(
        'app-tooltip-content data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 inline-flex items-center gap-1.5 has-data-[slot=kbd]:pr-1.5 **:data-[slot=kbd]:relative **:data-[slot=kbd]:isolate **:data-[slot=kbd]:z-[1] **:data-[slot=kbd]:rounded-sm z-[var(--z-popover)] w-fit origin-(--reka-tooltip-content-transform-origin)',
        props.class,
      )"
    >
      <slot />
    </TooltipContent>
  </TooltipPortal>
</template>

<style>
.app-tooltip-content {
  max-width: min(260px, calc(100vw - 16px));
  padding: 5px 8px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 6px;
  background: var(--app-bg-dialog);
  box-shadow: none;
  color: var(--app-text);
  font-family: var(--app-font-family);
  font-size: 12px;
  line-height: 1.35;
  overflow-wrap: anywhere;
  white-space: normal;
  pointer-events: none;
  user-select: none;
}

.app-tooltip-content.icon-button-tooltip {
  white-space: nowrap;
}

.app-tooltip-content.tooltip-hint-content {
  white-space: pre-line;
}
</style>
