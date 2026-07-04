<script setup>
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { useConfirm } from '@/composables/useConfirm';
import { AlertTriangle, Info } from '@lucide/vue';
import { computed, ref, watch } from 'vue';

const { confirmState, onOk, onCancel } = useConfirm();

// Force fresh VNode mount each time dialog opens to prevent unmount stale VNode errors
const contentKey = ref(0);
watch(() => confirmState.value.visible, (v) => { if (v) contentKey.value++; });

const confirmVisualMap = {
  danger: {
    icon: AlertTriangle,
    class: 'bg-[var(--app-risk-danger-bg)] border-[var(--app-risk-danger-border)] text-[var(--app-risk-danger)] shadow-[var(--app-risk-danger-glow)]',
  },
  warning: {
    icon: Info,
    class: 'bg-[var(--app-risk-warning-bg)] border-[var(--app-risk-warning-border)] text-[var(--app-risk-warning)] shadow-[var(--app-risk-warning-glow)]',
  },
};

const confirmVisualTone = computed(() => confirmState.value.danger ? 'danger' : 'warning');
const confirmVisual = computed(() => confirmVisualMap[confirmVisualTone.value]);
</script>

<template>
  <AlertDialog v-model:open="confirmState.visible">
    <AlertDialogContent :z-index="confirmState.zIndex" aria-describedby="confirm-dialog-desc">
      <AlertDialogHeader class="grid grid-cols-[auto_1fr] grid-rows-[auto_1fr] place-items-start gap-x-3 gap-y-1.5 text-left">
        <div
          data-slot="confirm-visual"
          :class="['row-span-2 grid size-9 place-items-center rounded-[10px] border', confirmVisual.class]"
          aria-hidden="true">
          <component :is="confirmVisual.icon" :size="18" />
        </div>
        <AlertDialogTitle class="col-start-2">{{ confirmState.title }}</AlertDialogTitle>
        <AlertDialogDescription v-if="typeof confirmState.content === 'string'" id="confirm-dialog-desc"
          class="col-start-2 whitespace-pre-line break-words text-[var(--app-font-body-size)] leading-[var(--app-line-height-ui)]">
          {{ confirmState.content }}
        </AlertDialogDescription>
        <div v-else :key="contentKey" id="confirm-dialog-desc" class="col-start-2 text-[var(--app-font-body-size)] text-muted-foreground leading-[var(--app-line-height-ui)]">
          <component :is="confirmState.content" />
        </div>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel v-if="confirmState.cancelText" @click="onCancel">
          {{ confirmState.cancelText }}
        </AlertDialogCancel>
        <AlertDialogAction :variant="confirmState.danger ? 'destructive' : 'default'" @click="onOk">
          {{ confirmState.okText }}
        </AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
