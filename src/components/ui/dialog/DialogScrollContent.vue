<script setup>
import { XIcon } from "@lucide/vue";

import { reactiveOmit } from "@vueuse/core";
import {
  DialogClose,
  DialogContent,
  DialogOverlay,
  DialogPortal,
  useForwardPropsEmits,
} from "reka-ui";
import { cn } from "@/lib/utils";

defineOptions({
  inheritAttrs: false,
});

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  disableOutsidePointerEvents: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
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

const handlePointerDownOutside = (event) => {
  if (!props.closeOnOutsideInteract) {
    event.preventDefault();
    return;
  }

  const originalEvent = event.detail.originalEvent;
  const target = originalEvent.target;
  if (
    originalEvent.offsetX > target.clientWidth ||
    originalEvent.offsetY > target.clientHeight
  ) {
    event.preventDefault();
  }
};

const handleInteractOutside = (event) => {
  if (!props.closeOnOutsideInteract) {
    event.preventDefault();
  }
};
</script>

<template>
  <DialogPortal>
    <DialogOverlay
      class="fixed inset-0 z-[var(--z-dialog-overlay)] grid place-items-center overflow-y-auto bg-black/80 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0"
    >
      <DialogContent
        :class="
          cn(
            'relative z-[var(--z-dialog-content)] my-4 grid w-full max-w-lg min-h-0 max-h-[calc(100vh-3rem)] gap-4 overflow-y-auto border border-border bg-background p-6 shadow-lg duration-200 sm:my-8 sm:rounded-lg md:w-full',
            props.class,
          )
        "
        v-bind="{ ...$attrs, ...forwarded }"
        @pointer-down-outside="handlePointerDownOutside"
        @interact-outside="handleInteractOutside"
      >
        <slot />

        <DialogClose
          class="absolute top-4 right-4 p-0.5 transition-colors rounded-md hover:bg-secondary"
        >
          <XIcon class="w-4 h-4" />
          <span class="sr-only">Close</span>
        </DialogClose>
      </DialogContent>
    </DialogOverlay>
  </DialogPortal>
</template>
