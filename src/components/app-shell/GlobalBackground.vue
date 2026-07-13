<script setup>
import { computed, ref, watch } from 'vue';
import { useTheme } from '@/composables/useTheme';
import { normalizeBackgroundSettings, resolveBackgroundUrl } from '@/utils/background';
import { invokeCommand } from '@/utils/ipc';

const props = defineProps({ settings: { type: Object, default: () => ({}) } });
const emit = defineEmits(['availability-change']);
const { isDark } = useTheme();
const config = computed(() => normalizeBackgroundSettings(props.settings));
const imageUrl = ref('');
const loadedUrl = ref('');
let resolveToken = 0;
watch(() => config.value.resourceId, async (resourceId) => {
  const token = ++resolveToken;
  imageUrl.value = '';
  if (!resourceId) return;
  try {
    const scale = window.devicePixelRatio || 1;
    const asset = await invokeCommand('ensure_background_image', {
      resourceId,
      targetWidth: Math.round(window.screen.width * scale),
      targetHeight: Math.round(window.screen.height * scale),
    });
    if (token === resolveToken) imageUrl.value = resolveBackgroundUrl(asset.optimized_path);
  } catch { if (token === resolveToken) emit('availability-change', false); }
}, { immediate: true });
watch(imageUrl, (url) => {
  loadedUrl.value = '';
  emit('availability-change', false);
  if (!url) return;
  const probe = new Image();
  probe.onload = () => { loadedUrl.value = url; emit('availability-change', true); };
  probe.onerror = () => emit('availability-change', false);
  probe.src = url;
}, { immediate: true });
const fitStyle = computed(() => {
  const fit = config.value.fit;
  if (fit === 'tile') return { backgroundSize: 'auto', backgroundRepeat: 'repeat', backgroundPosition: 'center' };
  if (fit === 'center') return { backgroundSize: 'auto', backgroundRepeat: 'no-repeat', backgroundPosition: 'center' };
  return { backgroundSize: fit === 'stretch' ? '100% 100%' : fit, backgroundRepeat: 'no-repeat', backgroundPosition: 'center' };
});
const layerStyle = computed(() => ({
  ...fitStyle.value,
  inset: config.value.blur > 0 ? `${-config.value.blur * 2}px` : '0',
  backgroundImage: loadedUrl.value ? `url("${loadedUrl.value.replace(/"/g, '%22')}")` : 'none',
  filter: config.value.blur > 0 ? `blur(${config.value.blur}px)` : 'none',
  opacity: config.value.opacity,
}));
const overlayStyle = computed(() => ({
  background: isDark.value ? '#000' : '#fff',
  opacity: isDark.value ? config.value.darkOverlay : config.value.lightOverlay,
}));
</script>

<template>
  <div v-if="config.enabled && loadedUrl" class="global-background" aria-hidden="true">
    <div class="global-background__image" :style="layerStyle" />
    <div class="global-background__overlay" :style="overlayStyle" />
  </div>
</template>

<style scoped>
.global-background { position: fixed; inset: 0; overflow: hidden; pointer-events: none; z-index: 0; background: var(--app-workspace-bg); }
.global-background__image, .global-background__overlay { position: absolute; inset: 0; }
</style>
