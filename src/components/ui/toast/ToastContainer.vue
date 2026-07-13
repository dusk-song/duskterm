<script setup>
import { useToast } from '@/composables/useToast';
import { AlertTriangle, CheckCircle, Info, Loader, XCircle } from '@lucide/vue';

const { toasts } = useToast();

const iconMap = {
  success: CheckCircle,
  error: XCircle,
  info: Info,
  warning: AlertTriangle,
  loading: Loader,
};

const toastToneMap = {
  success: {
    rail: 'bg-[var(--app-status-success)]',
    icon: 'bg-[var(--app-status-success-bg)] border-[var(--app-status-success-border)] text-[var(--app-status-success)]',
    progress: 'bg-[var(--app-status-success)]',
  },
  error: {
    rail: 'bg-[var(--app-risk-danger)]',
    icon: 'bg-[var(--app-risk-danger-bg)] border-[var(--app-risk-danger-border)] text-[var(--app-risk-danger)]',
    progress: 'bg-[var(--app-risk-danger)]',
  },
  info: {
    rail: 'bg-[var(--app-status-info)]',
    icon: 'bg-[var(--app-status-info-bg)] border-[var(--app-status-info-border)] text-[var(--app-status-info)]',
    progress: 'bg-[var(--app-status-info)]',
  },
  warning: {
    rail: 'bg-[var(--app-risk-warning)]',
    icon: 'bg-[var(--app-risk-warning-bg)] border-[var(--app-risk-warning-border)] text-[var(--app-risk-warning)]',
    progress: 'bg-[var(--app-risk-warning)]',
  },
  loading: {
    rail: 'bg-[var(--app-status-info)]',
    icon: 'bg-[var(--app-status-info-bg)] border-[var(--app-status-info-border)] text-[var(--app-status-info)]',
    progress: 'bg-[var(--app-status-info)]',
  },
};
</script>

<template>
  <div v-if="toasts.length" class="toast-viewport">
    <div v-for="t in toasts" :key="t.id" :class="['toast-card', t.leaving ? 'toast-card--leaving' : '']">
      <div data-slot="toast-rail" :class="['absolute inset-y-0 left-0 w-1', (toastToneMap[t.type] || toastToneMap.info).rail]" />
      <div :class="['toast-icon', (toastToneMap[t.type] || toastToneMap.info).icon]">
        <component :is="iconMap[t.type]" :size="15" :class="{ 'animate-spin': t.type === 'loading' }" />
      </div>
      <span class="toast-message">{{ t.message }}</span>
      <div :class="['absolute bottom-0 left-0 h-0.5 w-2/3 rounded-r-full opacity-70', (toastToneMap[t.type] || toastToneMap.info).progress]" />
    </div>
  </div>
</template>

<style scoped>
.toast-viewport {
  position: relative;
  z-index: var(--z-alert);
  display: flex;
  max-width: min(340px, 28vw);
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  pointer-events: none;
  overflow: visible;
}

.toast-card {
  pointer-events: none;
  position: relative;
  display: flex;
  min-width: 180px;
  max-width: min(340px, 28vw);
  height: 30px;
  align-items: center;
  gap: 8px;
  overflow: hidden;
  border: 1px solid var(--app-toast-border);
  border-radius: 999px;
  color: var(--app-toast-text);
  background: color-mix(in srgb, var(--app-toast-bg) 90%, transparent);
  box-shadow: var(--app-toast-shadow);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  padding: 0 12px 0 9px;
  font-size: var(--app-font-body-size);
  transform: translateY(0);
  opacity: 1;
  transition:
    opacity var(--app-motion-panel) var(--app-motion-ease),
    transform var(--app-motion-panel) var(--app-motion-ease),
    box-shadow var(--app-motion-panel) var(--app-motion-ease);
}

.toast-card--leaving {
  transform: translateY(-4px);
  opacity: 0;
}

.toast-icon {
  display: grid;
  width: 20px;
  height: 20px;
  flex: 0 0 auto;
  place-items: center;
  border-radius: 999px;
  border-width: 1px;
}

.toast-message {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  padding-right: 2px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 980px) {
  .toast-viewport {
    max-width: 220px;
  }

  .toast-card {
    min-width: 160px;
    max-width: 220px;
  }
}
</style>
