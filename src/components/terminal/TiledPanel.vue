<script setup>
const props = defineProps({
  dense: {
    type: Boolean,
    default: false
  },
  padded: {
    type: Boolean,
    default: true
  },
  ariaLabel: {
    type: String,
    default: ''
  }
});
</script>

<template>
  <section class="tiled-panel" :class="{ 'is-dense': dense, 'is-padded': padded }" role="region"
    :aria-label="ariaLabel || undefined">
    <div class="tiled-panel__body">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.tiled-panel {
  position: relative;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  height: 100%;
  border-radius: var(--niri-radius-lg, 14px);
  background: var(--app-panel-bg);
  border: var(--niri-border-width, 1px) solid var(--app-panel-border);
  box-shadow: var(--app-panel-shadow);
  overflow: hidden;
}

html.dark .tiled-panel {
  background: var(--app-panel-bg);
  box-shadow: var(--app-panel-shadow);
}

.tiled-panel.is-padded .tiled-panel__body {
  padding: 12px;
}

.tiled-panel.is-dense .tiled-panel__body {
  padding: 8px;
}

.tiled-panel__body {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}
</style>
