<script setup>
defineProps({ compact: Boolean, interactive: Boolean });
</script>

<template>
  <div class="dusk-dock" :class="{ 'dusk-dock--compact': compact, 'dusk-dock--interactive': interactive }">
    <slot />
  </div>
</template>

<style scoped>
.dusk-dock {
  display: inline-flex;
  align-items: center;
  min-width: 0;
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--tb-border, color-mix(in srgb, var(--app-border-light) 42%, transparent));
  border-radius: var(--niri-radius-md, 8px);
  color: var(--tb-text, var(--app-text));
  background: var(--tb-bg, var(--app-bg-dialog));
  box-shadow: var(--tb-shadow, 0 2px 10px color-mix(in srgb, var(--app-workspace-gap, #000) 30%, transparent));
  pointer-events: none;
}

.dusk-dock--interactive :deep(> *) {
  pointer-events: auto;
}

.dusk-dock--compact {
  height: 34px;
  padding: 0 9px;
  border-radius: var(--niri-radius-md, 8px);
}

</style>
