<script setup>
import { cn } from "@/lib/utils";
import { useVModel } from "@vueuse/core";
import {
  SliderRange,
  SliderRoot,
  SliderThumb,
  SliderTrack,
} from "reka-ui";
import { computed } from "vue";

const props = defineProps({
  defaultValue: { type: [Number, Array], required: false },
  modelValue: { type: [Number, Array, null], required: false },
  disabled: { type: Boolean, required: false },
  orientation: { type: String, required: false },
  dir: { type: String, required: false },
  inverted: { type: Boolean, required: false },
  min: { type: Number, required: false },
  max: { type: Number, required: false },
  step: { type: Number, required: false },
  minStepsBetweenThumbs: { type: Number, required: false },
  thumbAlignment: { type: String, required: false },
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  name: { type: String, required: false },
  required: { type: Boolean, required: false },
  class: { type: null, required: false },
});
const emits = defineEmits(["update:modelValue", "valueCommit"]);

const modelValue = useVModel(props, "modelValue", emits, {
  passive: true,
  defaultValue: props.defaultValue,
});

// Normalize to array for reka-ui SliderRoot
const arrayModel = computed({
  get: () => {
    const v = modelValue.value;
    if (v == null) return [props.min ?? 0];
    return Array.isArray(v) ? v : [Number(v)];
  },
  set: (val) => {
    if (!Array.isArray(modelValue.value) && modelValue.value != null) {
      modelValue.value = val[0] ?? (props.min ?? 0);
    } else {
      modelValue.value = val;
    }
  },
});

const isVertical = computed(() => props.orientation === 'vertical');
</script>

<template>
  <SliderRoot v-slot="{ modelValue }" v-model="arrayModel" data-slot="slider"
    :orientation="props.orientation" :dir="props.dir" :inverted="props.inverted"
    :min="props.min" :max="props.max" :step="props.step"
    :min-steps-between-thumbs="props.minStepsBetweenThumbs"
    :thumb-alignment="props.thumbAlignment"
    :as-child="props.asChild" :as="props.as"
    :name="props.name" :required="props.required" :disabled="props.disabled"
    :class="cn(
      'relative flex w-full touch-none items-center select-none disabled:opacity-50',
      isVertical ? 'min-h-40 h-full w-auto flex-col' : '',
      props.class,
    )
  ">
    <SliderTrack data-slot="slider-track" :class="cn(
      'bg-muted-foreground/25 rounded-full relative grow overflow-hidden',
      isVertical ? 'w-1.5 h-full' : 'h-1.5 w-full',
    )">
      <SliderRange data-slot="slider-range" :class="cn(
        'bg-primary absolute select-none',
        isVertical ? 'w-full' : 'h-full',
      )" />
    </SliderTrack>

    <SliderThumb v-for="(_, key) in modelValue" :key="key" data-slot="slider-thumb"
      class="border-primary ring-ring/50 size-4 rounded-full border bg-white shadow-sm transition-[color,box-shadow] hover:ring-4 focus-visible:shadow-[var(--app-focus-shadow)] focus-visible:outline-none block shrink-0 select-none disabled:pointer-events-none disabled:opacity-50" />
  </SliderRoot>
</template>
