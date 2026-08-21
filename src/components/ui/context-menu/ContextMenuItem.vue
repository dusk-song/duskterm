<script setup>
import { reactiveOmit } from "@vueuse/core";
import { ContextMenuItem, useForwardPropsEmits } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  disabled: { type: Boolean, required: false },
  textValue: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  inset: { type: Boolean, required: false },
  variant: { type: String, required: false, default: "default" },
});
const emits = defineEmits(["select"]);

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <ContextMenuItem
    data-slot="context-menu-item"
    :data-inset="inset ? '' : undefined"
    :data-variant="variant"
    v-bind="forwarded"
    :class="
      cn(
        'app-context-menu-item',
        props.class,
      )
    "
  >
    <slot />
  </ContextMenuItem>
</template>

<style>
.app-context-menu-item {
  position: relative;
  display: flex;
  min-height: 27px;
  align-items: center;
  gap: 8px;
  padding: 0 8px;
  border-radius: var(--niri-radius-sm, 6px);
  outline: none;
  color: var(--app-text);
  font-size: var(--app-font-caption-size, 12px);
  line-height: 1.35;
  cursor: default;
  user-select: none;
}

.app-context-menu-item[data-inset] {
  padding-left: 28px;
}

.app-context-menu-item:focus {
  background: var(--tb-entry-hover, color-mix(in srgb, var(--app-text) 8%, transparent));
  color: var(--app-text);
}

.app-context-menu-item[data-disabled] {
  opacity: 0.4;
  pointer-events: none;
}

.app-context-menu-item[data-variant="destructive"] {
  color: var(--color-danger);
}

.app-context-menu-item[data-variant="destructive"]:focus {
  background: var(--app-risk-danger-bg);
  color: var(--color-danger);
}

.app-context-menu-item svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  pointer-events: none;
}
</style>
