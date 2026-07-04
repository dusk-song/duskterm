<script setup>
import { CheckIcon } from "@lucide/vue";

import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import { CheckboxIndicator, CheckboxRoot, useForwardPropsEmits } from "reka-ui";

const props = defineProps({
  defaultValue: { type: null, required: false },
  modelValue: { type: null, required: false },
  disabled: { type: Boolean, required: false },
  value: { type: null, required: false },
  id: { type: String, required: false },
  trueValue: { type: null, required: false },
  falseValue: { type: null, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  name: { type: String, required: false },
  required: { type: Boolean, required: false },
  class: { type: null, required: false },
});
const emits = defineEmits(["update:modelValue"]);

const delegatedProps = reactiveOmit(props, "class");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <CheckboxRoot v-slot="slotProps" data-slot="checkbox" v-bind="forwarded" :class="cn(
    'border-foreground/80 bg-transparent dark:border-foreground/80 dark:bg-transparent data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground dark:data-[state=checked]:bg-primary data-[state=checked]:border-primary aria-invalid:aria-checked:border-primary aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 focus-visible:border-[var(--app-focus-border)] focus-visible:shadow-[var(--app-focus-shadow)] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 flex size-4 items-center justify-center rounded-[4px] border shadow-xs transition-shadow group-has-disabled/field:opacity-50 aria-invalid:ring-3 peer relative shrink-0 outline-none after:absolute after:-inset-x-3 after:-inset-y-2 disabled:cursor-not-allowed disabled:opacity-50',
    props.class,
  )
    ">
    <CheckboxIndicator data-slot="checkbox-indicator"
      class="[&>svg]:size-3.5 grid place-content-center text-current transition-none">
      <slot v-bind="slotProps">
        <CheckIcon />
      </slot>
    </CheckboxIndicator>
  </CheckboxRoot>
</template>
