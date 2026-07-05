<script setup>
import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import { DialogOverlay } from "reka-ui";

const props = defineProps({
  forceMount: { type: Boolean, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});

const delegatedProps = reactiveOmit(props, "class");
</script>

<template>
  <DialogOverlay data-slot="dialog-overlay" v-bind="delegatedProps" :class="cn(
    'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 bg-black/10 duration-100 fixed inset-0 isolate z-[var(--z-dialog-overlay)]',
    props.class,
  )
    ">
    <slot />
  </DialogOverlay>
</template>
