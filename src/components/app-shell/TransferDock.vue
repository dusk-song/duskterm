<script setup>
import { ListChecks } from '@lucide/vue';
import DuskDock from './DuskDock.vue';

defineProps({
  embedded: Boolean,
  expanded: Boolean,
});
const emit = defineEmits(['toggle']);
</script>

<template>
  <div class="transfer-dock-root" @dblclick.stop>
    <button v-if="embedded" type="button" class="transfer-dock transfer-dock--embedded"
      :class="{ active: expanded }" title="传输列表" @pointerdown.stop @click.stop="emit('toggle')">
      <ListChecks :size="14" />
    </button>
    <DuskDock v-else class="transfer-dock" :class="{ active: expanded }" interactive @click.stop="emit('toggle')">
      <ListChecks :size="14" />
    </DuskDock>
  </div>
</template>

<style scoped>
.transfer-dock-root {
  position: relative;
  pointer-events: auto;
}

.transfer-dock {
  gap: 6px;
  padding: 0 9px;
  font-size: 11px;
  cursor: pointer;
  white-space: nowrap;
}

.transfer-dock.active {
  border-color: color-mix(in srgb, var(--color-primary) 65%, transparent);
}

.transfer-dock--embedded {
  display: inline-flex;
  min-width: 29px;
  width: 29px;
  flex: 0 0 29px;
  height: 24px;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 999px;
  color: var(--tb-text, var(--app-text));
  background: transparent;
  opacity: .78;
}

.transfer-dock--embedded:hover,
.transfer-dock--embedded.active {
  background: var(--tb-hover-bg, color-mix(in srgb, var(--app-text) 8%, transparent));
  opacity: 1;
}
</style>
