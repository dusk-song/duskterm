<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useTheme } from '@/composables/useTheme';
import { normalizeBackgroundSettings, resolveBackgroundUrl } from '@/utils/background';
import { invokeCommand } from '@/utils/ipc';
import { isReducedPerformance } from '@/utils/performance';

const props = defineProps({ settings: { type: Object, default: () => ({}) } });
const emit = defineEmits(['availability-change']);
const { isDark } = useTheme();
const config = computed(() => normalizeBackgroundSettings(props.settings));
const mediaUrl = ref('');
const resolvedMediaType = ref('image');
const videoRef = ref(null);
const layoutSuspended = ref(false);
let resolveToken = 0;
let lastDisplayTarget = '';
let resizeTimer = null;
let available = false;
let imageProbe = null;

const displayTarget = () => {
  const scale = window.devicePixelRatio || 1;
  return {
    targetWidth: Math.round(window.screen.width * scale),
    targetHeight: Math.round(window.screen.height * scale),
  };
};

const setAvailability = (next) => {
  if (available === next) return;
  available = next;
  emit('availability-change', next);
};

const releaseVideo = () => {
  const video = videoRef.value;
  if (!video) return;
  video.pause();
  video.removeAttribute('src');
  video.load();
};

const releaseImageProbe = () => {
  if (!imageProbe) return;
  imageProbe.onload = null;
  imageProbe.onerror = null;
  imageProbe.src = '';
  imageProbe = null;
};

const versionedAssetUrl = (path, asset, targetSignature) => {
  const url = resolveBackgroundUrl(path);
  if (!url) return '';
  const version = encodeURIComponent(`${asset.resource_id}-${asset.width}x${asset.height}-${targetSignature}`);
  return `${url}${url.includes('?') ? '&' : '?'}background=${version}`;
};

const syncVideoPlayback = () => {
  const video = videoRef.value;
  if (!video) return;
  video.muted = true;
  if (document.hidden || layoutSuspended.value || isReducedPerformance() || !config.value.enabled || config.value.opacity <= 0) {
    video.pause();
    return;
  }
  void video.play().catch(() => {});
};

const resolveBackground = async () => {
  const token = ++resolveToken;
  releaseImageProbe();
  releaseVideo();
  mediaUrl.value = '';
  setAvailability(false);
  if (!config.value.enabled || !config.value.resourceId) return;

  const target = displayTarget();
  const targetSignature = `${target.targetWidth}x${target.targetHeight}`;
  lastDisplayTarget = targetSignature;
  try {
    const asset = await invokeCommand('ensure_background_image', {
      resourceId: config.value.resourceId,
      ...target,
    });
    if (token !== resolveToken) return;
    const mediaType = asset.media_type === 'video' ? 'video' : 'image';
    const url = versionedAssetUrl(asset.optimized_path, asset, targetSignature);
    if (!url) return;
    resolvedMediaType.value = mediaType;
    if (mediaType === 'video') {
      mediaUrl.value = url;
      await nextTick();
      syncVideoPlayback();
      return;
    }

    const probe = new Image();
    imageProbe = probe;
    probe.onload = () => {
      if (imageProbe === probe) imageProbe = null;
      if (token !== resolveToken) return;
      mediaUrl.value = url;
      setAvailability(true);
    };
    probe.onerror = () => {
      if (imageProbe === probe) imageProbe = null;
      if (token === resolveToken) setAvailability(false);
    };
    probe.src = url;
  } catch {
    if (token === resolveToken) setAvailability(false);
  }
};

const handleVideoReady = (event) => {
  if (event.currentTarget !== videoRef.value || resolvedMediaType.value !== 'video') return;
  setAvailability(true);
  syncVideoPlayback();
};

const handleVideoError = (event) => {
  if (event.currentTarget === videoRef.value) setAvailability(false);
};

const scheduleDisplayRefresh = () => {
  if (config.value.mediaType === 'video') return;
  if (resizeTimer) clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    resizeTimer = null;
    const target = displayTarget();
    const targetSignature = `${target.targetWidth}x${target.targetHeight}`;
    if (targetSignature !== lastDisplayTarget) void resolveBackground();
  }, 250);
};

watch(
  () => [config.value.enabled, config.value.resourceId, config.value.mediaType],
  () => { void resolveBackground(); },
  { immediate: true }
);
watch(() => config.value.opacity, syncVideoPlayback);
watch(isReducedPerformance, syncVideoPlayback);
watch(layoutSuspended, syncVideoPlayback);
const handleLayoutInteraction = () => {
  layoutSuspended.value = document.documentElement.hasAttribute('data-layout-interacting');
};

const fitStyle = computed(() => {
  const fit = config.value.fit;
  if (fit === 'tile') return { backgroundSize: 'auto', backgroundRepeat: 'repeat', backgroundPosition: 'center' };
  if (fit === 'center') return { backgroundSize: 'auto', backgroundRepeat: 'no-repeat', backgroundPosition: 'center' };
  return { backgroundSize: fit === 'stretch' ? '100% 100%' : fit, backgroundRepeat: 'no-repeat', backgroundPosition: 'center' };
});

const mediaLayerStyle = computed(() => ({
  inset: config.value.blur > 0 ? `${-config.value.blur * 2}px` : '0',
  filter: config.value.blur > 0 ? `blur(${config.value.blur}px)` : 'none',
  opacity: config.value.opacity,
}));

const imageLayerStyle = computed(() => ({
  ...fitStyle.value,
  ...mediaLayerStyle.value,
  backgroundImage: mediaUrl.value ? `url("${mediaUrl.value.replace(/"/g, '%22')}")` : 'none',
}));

const videoStyle = computed(() => ({
  objectFit: config.value.fit === 'stretch'
    ? 'fill'
    : config.value.fit === 'contain'
      ? 'contain'
      : config.value.fit === 'center'
        ? 'none'
        : 'cover',
  objectPosition: 'center',
}));

const overlayStyle = computed(() => ({
  background: isDark.value ? '#000' : '#f1ece3',
  opacity: isDark.value ? config.value.darkOverlay : config.value.lightOverlay,
}));

onMounted(() => {
  window.addEventListener('terminal-layout-dragging', handleLayoutInteraction);
  document.addEventListener('visibilitychange', syncVideoPlayback);
  window.addEventListener('resize', scheduleDisplayRefresh);
});

onBeforeUnmount(() => {
  window.removeEventListener('terminal-layout-dragging', handleLayoutInteraction);
  resolveToken += 1;
  if (resizeTimer) clearTimeout(resizeTimer);
  document.removeEventListener('visibilitychange', syncVideoPlayback);
  window.removeEventListener('resize', scheduleDisplayRefresh);
  releaseImageProbe();
  releaseVideo();
});
</script>

<template>
  <div v-if="config.enabled && mediaUrl" class="global-background" aria-hidden="true">
    <div v-if="resolvedMediaType === 'image'" class="global-background__image" :style="imageLayerStyle" />
    <div v-else class="global-background__video-layer" :style="mediaLayerStyle">
      <video ref="videoRef" :key="mediaUrl" class="global-background__video" :style="videoStyle" :src="mediaUrl"
        autoplay loop muted playsinline preload="metadata" disablepictureinpicture
        @loadeddata="handleVideoReady" @canplay="handleVideoReady" @error="handleVideoError" />
    </div>
    <div class="global-background__overlay" :style="overlayStyle" />
  </div>
</template>

<style scoped>
.global-background { position: fixed; inset: 0; overflow: hidden; pointer-events: none; z-index: 0; background: var(--app-workspace-bg); }
.global-background__image, .global-background__video-layer, .global-background__overlay { position: absolute; inset: 0; }
.global-background__video { display: block; width: 100%; height: 100%; }
</style>
