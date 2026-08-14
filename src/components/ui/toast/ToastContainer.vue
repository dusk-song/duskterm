<script setup>
import { useToast } from '@/composables/useToast';
import { Check, CircleAlert, Info, LoaderCircle, TriangleAlert } from '@lucide/vue';
import { computed } from 'vue';

const { toasts } = useToast();
const visibleToasts = computed(() => [...toasts.value].reverse());

const toastToneMap = {
  success: 'toast-card--success',
  error: 'toast-card--error',
  info: 'toast-card--info',
  warning: 'toast-card--warning',
  loading: 'toast-card--info',
};

const toastIconMap = {
  success: Check,
  error: CircleAlert,
  info: Info,
  warning: TriangleAlert,
  loading: LoaderCircle,
};
</script>

<template>
  <div v-if="visibleToasts.length" class="toast-viewport">
    <TransitionGroup name="toast-list" tag="div" class="toast-track">
      <div v-for="t in visibleToasts" :key="t.id"
        :class="['toast-card', toastToneMap[t.type] || toastToneMap.info, t.leaving ? 'toast-card--leaving' : '']"
        :role="t.type === 'error' || t.type === 'warning' ? 'alert' : 'status'">
        <component :is="toastIconMap[t.type] || toastIconMap.info" class="toast-icon"
          :class="{ 'toast-icon--loading': t.type === 'loading' }" :size="15" :stroke-width="2.15" />
        <span class="toast-message">{{ t.message }}</span>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-viewport {
  display: flex;
  width: 100%;
  max-width: 100%;
  max-height: 86px;
  align-items: flex-start;
  justify-content: center;
  pointer-events: none;
  overflow: hidden;
}

.toast-track {
  position: relative;
  display: flex;
  width: max-content;
  max-width: 100%;
  flex-direction: column;
  align-items: center;
  justify-content: flex-start;
  gap: 4px;
}

.toast-card {
  --toast-tone: var(--app-toast-info);
  pointer-events: none;
  position: relative;
  display: flex;
  width: max-content;
  min-width: 0;
  max-width: min(360px, calc(100vw - 32px));
  height: 26px;
  flex: 0 0 auto;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  border: 1px solid var(--app-toast-border);
  border-radius: var(--niri-radius-md, 8px);
  color: var(--app-toast-text);
  background: var(--app-toast-bg);
  box-shadow: var(--app-toast-shadow);
  box-sizing: border-box;
  padding: 0 8px;
  font-size: 12px;
  line-height: 16px;
  transform: translateY(0);
  opacity: 1;
  animation: toast-card-enter 160ms var(--app-motion-ease) both;
  transition:
    opacity 180ms var(--app-motion-ease),
    transform 180ms var(--app-motion-ease),
    border-color var(--app-motion-panel) var(--app-motion-ease),
    background-color var(--app-motion-panel) var(--app-motion-ease),
    color var(--app-motion-panel) var(--app-motion-ease);
}

.toast-card--leaving {
  transform: translateY(-4px) scale(0.98);
  opacity: 0;
}

.toast-icon {
  flex: 0 0 auto;
  color: var(--toast-tone);
}

.toast-icon--loading {
  animation: toast-icon-spin 900ms linear infinite;
}

.toast-message {
  min-width: 0;
  flex: 0 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toast-card--success { --toast-tone: var(--app-toast-success); }
.toast-card--error { --toast-tone: var(--app-toast-danger); }
.toast-card--warning { --toast-tone: var(--app-toast-warning); }
.toast-card--info { --toast-tone: var(--app-toast-info); }

@keyframes toast-card-enter {
  from {
    transform: translateY(-6px) scale(0.98);
    opacity: 0;
  }
}

@keyframes toast-icon-spin {
  to { transform: rotate(360deg); }
}

.toast-list-move {
  transition: transform 180ms var(--app-motion-ease);
}

@media (prefers-reduced-motion: reduce) {
  .toast-card,
  .toast-list-move,
  .toast-icon--loading {
    animation-duration: 1ms;
    transition-duration: 1ms;
  }
}
</style>
