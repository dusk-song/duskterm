<script setup>
import { ListChecks } from '@lucide/vue';
import IconButton from '@/components/common/IconButton.vue';

defineProps({
  expanded: Boolean,
});
const emit = defineEmits(['toggle']);

function handleToggle(event) {
  event?.stopPropagation?.();
  emit('toggle');
}
</script>

<template>
  <div class="transfer-dock-root" @pointerdown.stop @dblclick.stop>
    <IconButton :icon="ListChecks" size="sm" tone="primary" :active="expanded"
      aria-label="传输列表" :action="handleToggle" />
  </div>
</template>

<style scoped>
.transfer-dock-root {
  position: relative;
  pointer-events: auto;
}
</style>
