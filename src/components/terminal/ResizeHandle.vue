<script setup>
const props = defineProps({
  orientation: {
    type: String,
    default: 'vertical'
  },
  active: {
    type: Boolean,
    default: false
  },
  disabled: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['mousedown']);

const handleMouseDown = (event) => {
  if (props.disabled) return;
  emit('mousedown', event);
};
</script>

<template>
  <div class="resize-handle" :class="[
    `is-${orientation}`,
    { 'is-active': active, 'is-disabled': disabled }
  ]" @mousedown="handleMouseDown">
    <span class="resize-handle__grip" />
  </div>
</template>

<style scoped>
.resize-handle {
  position: relative;
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  transition: background-color 0.15s ease, opacity 0.15s ease;
  user-select: none;
  touch-action: none;
  z-index: 6;
}

.resize-handle.is-vertical {
  width: 4px;
  cursor: col-resize;
}

.resize-handle.is-horizontal {
  height: 4px;
  cursor: row-resize;
}

.resize-handle__grip {
  position: absolute;
  inset: 0;
  border-radius: 999px;
  background: transparent;
  transition: background-color 0.15s ease, box-shadow 0.15s ease;
}

.resize-handle:hover .resize-handle__grip,
.resize-handle.is-active .resize-handle__grip {
  background: color-mix(in srgb, var(--app-text-muted) 24%, transparent);
}

html.dark .resize-handle:hover .resize-handle__grip,
html.dark .resize-handle.is-active .resize-handle__grip {
  background: color-mix(in srgb, var(--app-text) 20%, transparent);
}

.resize-handle.is-disabled {
  cursor: default;
  opacity: 0.35;
}

.resize-handle.is-vertical .resize-handle__grip {
  width: 4px;
}

.resize-handle.is-horizontal .resize-handle__grip {
  height: 4px;
}
</style>
