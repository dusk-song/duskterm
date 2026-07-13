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
  height: 30px;
  padding: 0 4px;
  border: 1px solid color-mix(in srgb, var(--app-border-light) 64%, transparent);
  border-radius: 7px;
  color: var(--tb-text, var(--app-text));
  background: color-mix(in srgb, var(--tb-bg, var(--app-bg-dialog)) 82%, transparent);
  box-shadow: 0 2px 9px color-mix(in srgb, var(--app-workspace-gap, #000) 28%, transparent);
  backdrop-filter: blur(8px) saturate(112%);
  -webkit-backdrop-filter: blur(8px) saturate(112%);
  pointer-events: none;
}

.dusk-dock--interactive :deep(> *) {
  pointer-events: auto;
}

.dusk-dock--compact {
  height: 24px;
  padding: 0 7px;
  border-radius: 6px;
}
</style>
