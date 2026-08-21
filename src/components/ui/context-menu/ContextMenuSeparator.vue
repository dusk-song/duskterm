<script setup>
import { reactiveOmit } from "@vueuse/core";
import { ContextMenuSeparator } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps({
  asChild: { type: Boolean, required: false },
  as: { type: null, required: false },
  class: { type: null, required: false },
});

const delegatedProps = reactiveOmit(props, "class");
</script>

<template>
  <ContextMenuSeparator
    data-slot="context-menu-separator"
    v-bind="delegatedProps"
    :class="cn('app-context-menu-separator', props.class)"
  />
</template>

<style>
.app-context-menu-separator {
  height: 1px;
  margin: 3px 4px;
  background: var(--app-border-shadow);
}
</style>
