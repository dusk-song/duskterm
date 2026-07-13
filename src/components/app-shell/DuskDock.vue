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
  padding: 0 8px;
  border: 1px solid color-mix(in srgb, var(--app-border-light) 42%, transparent);
  border-radius: 999px;
  color: var(--tb-text, var(--app-text));
  background: color-mix(in srgb, var(--tb-bg, var(--app-bg-dialog)) 86%, transparent);
  box-shadow: 0 2px 10px color-mix(in srgb, var(--app-workspace-gap, #000) 30%, transparent);
  backdrop-filter: blur(12px) saturate(116%);
  -webkit-backdrop-filter: blur(12px) saturate(116%);
  pointer-events: none;
}

.dusk-dock--interactive :deep(> *) {
  pointer-events: auto;
}

.dusk-dock--compact {
  height: 24px;
  padding: 0 9px;
  border-radius: 999px;
}
</style>
