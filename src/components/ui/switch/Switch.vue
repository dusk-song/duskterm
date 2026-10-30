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
    'app-switch peer relative inline-flex shrink-0 cursor-pointer items-center rounded-full border-0 outline-none transition-colors duration-[var(--app-motion-control)] ease-[var(--app-motion-ease)]',
    'data-[state=checked]:bg-primary data-[state=unchecked]:bg-muted-foreground/30',
    'focus-visible:[box-shadow:var(--app-focus-shadow)]',
    'data-[disabled]:pointer-events-none data-[disabled]:opacity-50',
    'after:absolute after:-inset-x-3 after:-inset-y-2',
    props.class,
  )
    ">
    <SwitchThumb data-slot="switch-thumb" :class="cn(
      'pointer-events-none block shrink-0 rounded-full bg-white shadow-sm ring-0 transition-transform duration-[var(--app-motion-control)] ease-[var(--app-motion-ease)]',
    )">
      <slot name="thumb" v-bind="slotProps" />
    </SwitchThumb>
  </SwitchRoot>
</template>

<style scoped>
/* Padding keeps fractional zoom from rounding the inset differently from the thumb travel. */
.app-switch {
  --switch-width: 36px;
  --switch-height: 20px;
  --switch-thumb-size: 14px;
  width: var(--switch-width);
  height: var(--switch-height);
  padding: 2px;
}

.app-switch[data-size="sm"] {
  --switch-width: 28px;
  --switch-height: 16px;
  --switch-thumb-size: 10px;
}

.app-switch :deep([data-slot="switch-thumb"]) {
  width: var(--switch-thumb-size);
  height: var(--switch-thumb-size);
  transform: translateX(0);
}

.app-switch[data-state="checked"] :deep([data-slot="switch-thumb"]) {
  transform: translateX(calc(var(--switch-width) - var(--switch-thumb-size) - 4px));
}
</style>
