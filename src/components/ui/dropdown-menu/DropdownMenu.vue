<script setup lang="ts">
import { cn } from "@/lib/utils";
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";

defineProps<{
  class?: string;
  align?: "start" | "center" | "end";
}>();

const open = ref(false);
const model = defineModel<boolean>("open");
const triggerRef = ref<HTMLElement | null>(null);
const menuRef = ref<HTMLElement | null>(null);
const menuStyle = ref<Record<string, string>>({});

function updatePosition() {
  if (!triggerRef.value || !menuRef.value) return;
  const triggerRect = triggerRef.value.getBoundingClientRect();
  const style: Record<string, string> = {
    position: 'fixed',
    top: `${triggerRect.bottom + 4}px`,
  };
  // Align to trigger's left edge by default
  style.left = `${triggerRect.left}px`;
  menuStyle.value = style;
}

function toggleOpen() {
  open.value = !open.value;
}

function closeMenu() {
  open.value = false;
}

function onClickOutside(e: MouseEvent) {
  const target = e.target as Node;
  if (
    open.value &&
    menuRef.value && !menuRef.value.contains(target) &&
    triggerRef.value && !triggerRef.value.contains(target)
  ) {
    closeMenu();
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) {
    closeMenu();
  }
}

watch(open, async (val) => {
  if (val) {
    document.addEventListener('mousedown', onClickOutside, true);
    document.addEventListener('keydown', onKeydown, true);
    await nextTick();
    updatePosition();
  } else {
    document.removeEventListener('mousedown', onClickOutside, true);
    document.removeEventListener('keydown', onKeydown, true);
  }
});

onUnmounted(() => {
  document.removeEventListener('mousedown', onClickOutside, true);
  document.removeEventListener('keydown', onKeydown, true);
});
</script>

<template>
  <div class="relative inline-block" ref="triggerRef">
    <div @click.stop="toggleOpen">
      <slot name="trigger" />
    </div>
    <Teleport to="body">
      <div v-if="open" ref="menuRef" :style="menuStyle" :class="cn(
        'z-[var(--z-dropdown)] min-w-[10rem] overflow-hidden rounded-[var(--radius-md)] border border-border bg-popover p-1 text-popover-foreground shadow-md',
        $props.class,
      )
        " role="menu">
        <slot />
      </div>
    </Teleport>
  </div>
</template>
