<script setup>
import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import {
  AlertDialogContent,
  AlertDialogOverlay,
  AlertDialogPortal,
  useForwardPropsEmits,
} from "reka-ui";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  disableOutsidePointerEvents: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  size: { type: String, required: false, default: "default" },
  zIndex: { type: Number, default: undefined },
});
const emits = defineEmits([
  "escapeKeyDown",
  "pointerDownOutside",
  "focusOutside",
  "interactOutside",
  "openAutoFocus",
  "closeAutoFocus",
]);

const delegatedProps = reactiveOmit(props, "class", "size");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <AlertDialogPortal>
    <AlertDialogOverlay data-slot="alert-dialog-overlay"
      class="data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 bg-black/10 duration-100 supports-backdrop-filter:backdrop-blur-xs fixed inset-0 z-[var(--z-dialog-overlay)]"
      :style="props.zIndex ? { zIndex: Math.max(0, props.zIndex - 1) } : undefined" />
    <AlertDialogContent data-slot="alert-dialog-content" :data-size="size" v-bind="{ ...$attrs, ...forwarded }" :class="cn(
      'pointer-events-auto data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 group/alert-dialog-content fixed top-1/2 left-1/2 grid w-full max-w-[calc(100%-2rem)] max-h-[calc(100vh-3rem)] -translate-x-1/2 -translate-y-1/2 gap-6 overflow-y-auto rounded-xl border border-[var(--app-border-dark)] bg-popover p-6 text-[var(--app-font-body-size)] text-popover-foreground shadow-[var(--niri-shadow-dialog)] duration-[var(--app-motion-panel)] ease-[var(--app-motion-ease)] outline-none data-[size=default]:max-w-xs data-[size=sm]:max-w-xs data-[size=default]:sm:max-w-lg',
      props.class,
    )
      " :style="props.zIndex ? { zIndex: props.zIndex } : { zIndex: 'var(--z-alert)' }">
      <slot />
    </AlertDialogContent>
  </AlertDialogPortal>
</template>
