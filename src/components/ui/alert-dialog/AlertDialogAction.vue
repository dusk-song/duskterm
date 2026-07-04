<script setup>
import { reactiveOmit } from "@vueuse/core";
import { AlertDialogAction, useForwardPropsEmits } from "reka-ui";
import { cn } from "@/lib/utils";
import { buttonVariants } from '@/components/ui/button';

const props = defineProps({
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
  variant: { type: null, required: false, default: "default" },
  size: { type: null, required: false, default: "default" },
});
const emits = defineEmits(["click"]);

const delegatedProps = reactiveOmit(props, "class", "variant", "size");
const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <AlertDialogAction
    data-slot="alert-dialog-action"
    v-bind="{ ...$attrs, ...forwarded }"
    :class="cn('', buttonVariants({ variant, size }), props.class)"
  >
    <slot />
  </AlertDialogAction>
</template>
