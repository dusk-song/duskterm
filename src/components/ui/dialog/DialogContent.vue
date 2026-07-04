<script setup>
import { XIcon } from "@lucide/vue";

import { Button } from '@/components/ui/button';
import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import {
  DialogClose,
  DialogContent,
  DialogPortal,
  useForwardPropsEmits,
} from "reka-ui";
import DialogOverlay from "./DialogOverlay.vue";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  disableOutsidePointerEvents: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  showCloseButton: { type: Boolean, required: false, default: true },
  closeOnOutsideInteract: { type: Boolean, required: false, default: false },
});
const emits = defineEmits([
  "escapeKeyDown",
  "pointerDownOutside",
  "focusOutside",
  "interactOutside",
  "openAutoFocus",
  "closeAutoFocus",
]);

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);

const handleOutsideInteract = (event) => {
  if (!props.closeOnOutsideInteract) {
    event.preventDefault();
  }
};
</script>

<template>
  <DialogPortal>
    <DialogOverlay />
    <DialogContent data-slot="dialog-content" v-bind="{ ...$attrs, ...forwarded }" :class="cn(
      'bg-popover text-popover-foreground border border-[var(--app-border-dark)] shadow-[var(--niri-shadow-dialog)] data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 fixed top-1/2 left-1/2 z-[var(--z-dialog-content)] grid w-full max-w-[calc(100%-2rem)] min-h-0 max-h-[calc(100vh-3rem)] -translate-x-1/2 -translate-y-1/2 gap-6 overflow-hidden rounded-[14px] p-6 text-[var(--app-font-body-size)] duration-[var(--app-motion-panel)] ease-[var(--app-motion-ease)] outline-none focus-visible:outline-none sm:max-w-md',
      props.class,
    )
      "
      @pointer-down-outside="handleOutsideInteract"
      @interact-outside="handleOutsideInteract">
      <slot />

      <DialogClose v-if="showCloseButton" data-slot="dialog-close" as-child>
        <Button variant="ghost" class="absolute top-4 right-4" size="icon-sm">
          <XIcon />
          <span class="sr-only">Close</span>
        </Button>
      </DialogClose>
    </DialogContent>
  </DialogPortal>
</template>
