<script setup>
import { ListChecks } from '@lucide/vue';
import { storeToRefs } from 'pinia';
import { computed } from 'vue';
import { useSftpTransfersStore } from '@/stores/sftpTransfers';
import DuskDock from './DuskDock.vue';

defineProps({
  embedded: Boolean,
  expanded: Boolean,
});
const emit = defineEmits(['toggle']);

const transferStore = useSftpTransfersStore();
const { dockStatus: status } = storeToRefs(transferStore);
const transferCount = computed(() => status.value.active || status.value.total || 0);
</script>

<template>
  <div class="transfer-dock-root" @dblclick.stop>
    <button v-if="embedded" type="button" class="transfer-dock transfer-dock--embedded"
      :class="{ active: expanded }" title="传输列表" @pointerdown.stop @click.stop="emit('toggle')">
      <ListChecks :size="14" />
      <span v-if="transferCount" class="transfer-badge" :class="{ busy: status.active }">{{ transferCount }}</span>
    </button>
    <DuskDock v-else class="transfer-dock" :class="{ active: expanded }" interactive @click.stop="emit('toggle')">
      <ListChecks :size="14" />
      <span v-if="transferCount" class="transfer-badge" :class="{ busy: status.active }">{{ transferCount }}</span>
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
  height: 24px;
  align-items: center;
  justify-content: center;
  padding: 0 7px;
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

.transfer-badge {
  display: inline-flex;
  min-width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  padding: 0 5px;
  border-radius: 999px;
  color: var(--tb-text, var(--app-text));
  background: color-mix(in srgb, var(--app-text) 12%, transparent);
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
}

.transfer-badge.busy {
  color: #fff;
  background: var(--color-primary);
}
</style>
