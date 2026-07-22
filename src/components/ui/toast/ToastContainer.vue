<script setup>
import { useToast } from '@/composables/useToast';
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

const { toasts, toast } = useToast();
const viewportRef = ref(null);
let resizeObserver = null;
let trimFrame = null;

async function trimOverflowingToasts() {
  await nextTick();
  const viewport = viewportRef.value;
  if (!viewport) return;
  while (toasts.value.length > 1 && viewport.scrollWidth > viewport.clientWidth + 1) {
    toast.remove(toasts.value[0].id, true);
    await nextTick();
  }
}

function scheduleOverflowTrim() {
  if (trimFrame !== null) return;
  trimFrame = requestAnimationFrame(async () => {
    trimFrame = null;
    await trimOverflowingToasts();
  });
}

watch(
  () => toasts.value.map((item) => `${item.id}:${item.message}`).join('|'),
  scheduleOverflowTrim,
  { flush: 'post' }
);

onMounted(() => {
  resizeObserver = new ResizeObserver(scheduleOverflowTrim);
  if (viewportRef.value) resizeObserver.observe(viewportRef.value);
  window.addEventListener('resize', scheduleOverflowTrim);
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  if (trimFrame !== null) cancelAnimationFrame(trimFrame);
  window.removeEventListener('resize', scheduleOverflowTrim);
});

const toastToneMap = {
  success: 'toast-card--success',
  error: 'toast-card--error',
  info: 'toast-card--info',
  warning: 'toast-card--warning',
  loading: 'toast-card--info',
};
</script>

<template>
  <div v-if="toasts.length" ref="viewportRef" class="toast-viewport">
    <div v-for="t in toasts" :key="t.id"
      :class="['toast-card', toastToneMap[t.type] || toastToneMap.info, t.leaving ? 'toast-card--leaving' : '']">
      <span class="toast-message">{{ t.message }}</span>
    </div>
  </div>
</template>

<style scoped>
.toast-viewport {
  position: relative;
  z-index: var(--z-alert);
  display: flex;
  width: 100%;
  max-width: 100%;
  flex-direction: row-reverse;
  align-items: center;
  gap: 4px;
  pointer-events: none;
  overflow: hidden;
}

.toast-card {
  pointer-events: none;
  position: relative;
  display: flex;
  width: max-content;
  min-width: 96px;
  max-width: none;
  height: 19px;
  flex: 0 0 auto;
  align-items: center;
  overflow: visible;
  border: 1px solid color-mix(in srgb, var(--app-toast-border) 72%, transparent);
  border-radius: 999px;
  color: var(--app-toast-text);
  background: color-mix(in srgb, var(--app-toast-bg) 76%, transparent);
  box-shadow: none;
  padding: 0 8px;
  font-size: 12px;
  line-height: 17px;
  transform: translateX(0);
  opacity: 1;
  transition:
    opacity var(--app-motion-panel) var(--app-motion-ease),
    transform var(--app-motion-panel) var(--app-motion-ease),
    border-color var(--app-motion-panel) var(--app-motion-ease);
}

.toast-card--leaving {
  transform: translateX(-4px);
  opacity: 0;
}

.toast-message {
  min-width: 0;
  flex: 0 0 auto;
  overflow: visible;
  text-overflow: clip;
  white-space: nowrap;
}

.toast-card--success {
  border-color: color-mix(in srgb, var(--app-status-success) 36%, transparent);
}

.toast-card--error {
  border-color: color-mix(in srgb, var(--app-risk-danger) 38%, transparent);
}

.toast-card--warning {
  border-color: color-mix(in srgb, var(--app-risk-warning) 40%, transparent);
}

.toast-card--info {
  border-color: color-mix(in srgb, var(--app-status-info) 32%, transparent);
}

@media (max-width: 980px) {
  .toast-card {
    min-width: 90px;
    max-width: none;
  }
}
</style>
