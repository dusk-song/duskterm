<script setup>
import { reactiveOmit } from "@vueuse/core";
import {
  ContextMenuContent,
  ContextMenuPortal,
  useForwardPropsEmits,
} from "reka-ui";
import { cn } from "@/lib/utils";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  loop: { type: Boolean, required: false },
  sideFlip: { type: Boolean, required: false },
  alignOffset: { type: Number, required: false },
  alignFlip: { type: Boolean, required: false },
  avoidCollisions: { type: Boolean, required: false },
  collisionBoundary: { type: null, required: false },
  collisionPadding: { type: [Number, Object], required: false, default: 8 },
  hideShiftedArrow: { type: Boolean, required: false },
  sticky: { type: String, required: false },
  hideWhenDetached: { type: Boolean, required: false },
  positionStrategy: { type: String, required: false, default: "fixed" },
  disableUpdateOnLayoutShift: { type: Boolean, required: false, default: true },
  prioritizePosition: { type: Boolean, required: false },
  reference: { type: null, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});
const emits = defineEmits([
  "escapeKeyDown",
  "pointerDownOutside",
  "focusOutside",
  "interactOutside",
  "closeAutoFocus",
]);

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <ContextMenuPortal>
    <ContextMenuContent
      data-slot="context-menu-content"
      v-bind="{ ...$attrs, ...forwarded }"
      :class="
        cn(
          'app-context-menu-content',
          props.class,
        )
      "
    >
      <slot />
    </ContextMenuContent>
  </ContextMenuPortal>
</template>

<style>
@keyframes app-context-menu-in {
  from { opacity: 0; }
  to { opacity: 1; }
}

@keyframes app-context-menu-out {
  from { opacity: 1; }
  to { opacity: 0; }
}

.app-context-menu-content {
  z-index: var(--z-dropdown);
  min-width: 136px;
  max-height: var(--reka-context-menu-content-available-height);
  padding: 4px;
  overflow-x: hidden;
  overflow-y: auto;
  border: 1px solid var(--app-border-shadow);
  border-radius: var(--niri-radius-md, 8px);
  background: var(--app-bg-dialog);
  box-shadow: 0 8px 22px rgba(0, 0, 0, 0.22);
  color: var(--app-text);
  font-family: var(--app-font-family);
  font-size: var(--app-font-caption-size, 12px);
  line-height: 1.35;
}

.app-context-menu-content[data-state="open"] {
  animation: app-context-menu-in 70ms linear both;
}

.app-context-menu-content[data-state="closed"] {
  animation: app-context-menu-out 45ms linear both;
}

@media (prefers-reduced-motion: reduce) {
  .app-context-menu-content[data-state] {
    animation: none;
  }
}
</style>
