<script setup>
import { cn } from "@/lib/utils";

const props = defineProps({
  class: { type: null, required: false },
});
</script>

<template>
  <fieldset
    data-slot="field-set"
    :class="
      cn(
        'gap-6 has-[>[data-slot=checkbox-group]]:gap-3 has-[>[data-slot=radio-group]]:gap-3 flex flex-col',
        props.class,
      )
    "
  >
    <slot />
  </fieldset>
</template>
