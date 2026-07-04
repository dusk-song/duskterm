<script setup>
import { reactiveOmit } from "@vueuse/core";
import { AlertDialogCancel, useForwardPropsEmits } from "reka-ui";
import { cn } from "@/lib/utils";
import { buttonVariants } from '@/components/ui/button';

const props = defineProps({
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  variant: { type: null, required: false, default: "outline" },
  size: { type: null, required: false, default: "default" },
});
const emits = defineEmits(["click"]);

const delegatedProps = reactiveOmit(props, "class", "variant", "size");
const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <AlertDialogCancel
    data-slot="alert-dialog-cancel"
    v-bind="{ ...$attrs, ...forwarded }"
    :class="cn('', buttonVariants({ variant, size }), props.class)"
  >
    <slot />
  </AlertDialogCancel>
</template>
