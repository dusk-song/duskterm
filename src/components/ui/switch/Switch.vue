<script setup>
import { cn } from "@/lib/utils";
import { reactiveOmit } from "@vueuse/core";
import { SwitchRoot, SwitchThumb, useForwardPropsEmits } from "reka-ui";

const props = defineProps({
  defaultValue: { type: null, required: false },
  modelValue: { type: null, required: false },
  disabled: { type: Boolean, required: false },
  id: { type: String, required: false },
  value: { type: String, required: false },
  trueValue: { type: null, required: false },
  falseValue: { type: null, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  name: { type: String, required: false },
  required: { type: Boolean, required: false },
  class: { type: null, required: false },
  size: { type: String, required: false, default: "default" },
});

const emits = defineEmits(["update:modelValue"]);

const delegatedProps = reactiveOmit(props, "class", "size");

const forwarded = useForwardPropsEmits(delegatedProps, emits);
</script>

<template>
  <SwitchRoot v-slot="slotProps" data-slot="switch" :data-size="size" v-bind="forwarded" :class="cn(
    'peer group/switch relative inline-flex shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent outline-none transition-colors duration-200 ease-in-out',
    'data-[state=checked]:bg-primary data-[state=unchecked]:bg-muted-foreground/30',
    'focus-visible:border-[var(--app-focus-border)] focus-visible:bg-[var(--app-focus-bg)] focus-visible:shadow-[var(--app-focus-shadow)]',
    'data-[disabled]:pointer-events-none data-[disabled]:opacity-50',
    'data-[size=default]:h-5 data-[size=default]:w-14',
    'data-[size=sm]:h-4 data-[size=sm]:w-7',
    'after:absolute after:-inset-x-3 after:-inset-y-2',
    props.class,
  )
    ">
    <SwitchThumb data-slot="switch-thumb" :class="cn(
      'pointer-events-none block rounded-full bg-white shadow-sm ring-0 transition-transform duration-200 ease-in-out',
      'group-data-[size=default]/switch:size-3.5',
      'group-data-[size=sm]/switch:size-2.5',
      'group-data-[size=default]/switch:data-[state=checked]:translate-x-[31px]',
      'group-data-[size=sm]/switch:data-[state=checked]:translate-x-[14px]',
      'group-data-[size=default]/switch:data-[state=unchecked]:translate-x-0',
      'group-data-[size=sm]/switch:data-[state=unchecked]:translate-x-0',
    )">
      <slot name="thumb" v-bind="slotProps" />
    </SwitchThumb>
  </SwitchRoot>
</template>
