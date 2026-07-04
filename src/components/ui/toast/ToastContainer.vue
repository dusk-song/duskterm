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
  <Teleport to="body">
    <div class="fixed top-7 left-1/2 -translate-x-1/2 z-[var(--z-alert)] flex flex-col items-center gap-2 pointer-events-none">
      <div v-for="t in toasts" :key="t.id" :class="['pointer-events-auto relative flex min-w-64 max-w-[min(420px,calc(100vw-2rem))] items-center gap-2.5 overflow-hidden rounded-[10px] border border-[var(--app-toast-border)] bg-[var(--app-toast-bg)] px-3.5 py-2.5 text-[var(--app-font-body-size)] text-[var(--app-toast-text)] shadow-[var(--app-toast-shadow)] transition-[opacity,transform,box-shadow] duration-[var(--app-motion-panel)] ease-[var(--app-motion-ease)]',
        t.leaving ? 'opacity-0 translate-y-2' : 'opacity-100 translate-y-0']">
        <div data-slot="toast-rail" :class="['absolute inset-y-0 left-0 w-1', (toastToneMap[t.type] || toastToneMap.info).rail]" />
        <div :class="['grid size-6 shrink-0 place-items-center rounded-[8px] border', (toastToneMap[t.type] || toastToneMap.info).icon]">
          <component :is="iconMap[t.type]" :size="15" :class="{ 'animate-spin': t.type === 'loading' }" />
        </div>
        <span class="min-w-0 flex-1 truncate pr-1">{{ t.message }}</span>
        <div :class="['absolute bottom-0 left-0 h-0.5 w-2/3 rounded-r-full opacity-70', (toastToneMap[t.type] || toastToneMap.info).progress]" />
      </div>
    </div>
  </Teleport>
</template>
