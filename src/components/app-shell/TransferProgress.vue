<script setup>
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { ACTIVE_TRANSFER_STATUSES } from '@/utils/transferPanel';
import { advanceTransferProgress, transferProgressTarget } from '@/utils/transferProgress';
import { isReducedPerformance } from '@/utils/performance';

const props = defineProps({ item: { type: Object, required: true } });
const trackRef = ref(null);
const fillRef = ref(null);
const labelRef = ref(null);
const visible = ref(ACTIVE_TRANSFER_STATUSES.has(props.item.status));
const reducedMotion = computed(isReducedPerformance);
let wasActive = visible.value;
let completionSettled = props.item.status === 'success';
const startedAt = props.item.transferStartedAt;
const fresh = Number.isFinite(startedAt) && performance.now() - startedAt < 300;
let displayed = fresh ? 0 : transferProgressTarget(props.item);
let frame = null;
let hideTimer = null;
let previousFrameAt = null;
let renderedPercent = -1;

function stopAnimation() {
  if (frame !== null) cancelAnimationFrame(frame);
  frame = null;
  previousFrameAt = null;
}

function clearHideTimer() {
  if (hideTimer !== null) clearTimeout(hideTimer);
  hideTimer = null;
}

function draw() {
  if (fillRef.value) fillRef.value.style.transform = `scaleX(${displayed / 100})`;
  const percent = Math.round(displayed);
  if (percent === renderedPercent) return;
  renderedPercent = percent;
  if (labelRef.value) labelRef.value.textContent = `${percent}%`;
  trackRef.value?.setAttribute('aria-valuenow', String(percent));
}

function finishCompletion() {
  if (hideTimer !== null || completionSettled) return;
  // Keep the last fill visible briefly, without delaying the actual success status or actions.
  hideTimer = setTimeout(() => {
    hideTimer = null;
    completionSettled = true;
    visible.value = false;
  }, 180);
}

function animate(stamp) {
  frame = null;
  if (document.hidden) { sync(true); return; }
  const target = transferProgressTarget(props.item);
  displayed = advanceTransferProgress(displayed, target, previousFrameAt === null ? 16 : stamp - previousFrameAt);
  previousFrameAt = stamp;
  draw();
  if (displayed < target) frame = requestAnimationFrame(animate);
  else {
    previousFrameAt = null;
    if (props.item.status === 'success') finishCompletion();
  }
}

function sync(snap = false) {
  const status = props.item.status;
  const target = transferProgressTarget(props.item);
  const active = ACTIVE_TRANSFER_STATUSES.has(status);
  if (!active && status !== 'success') {
    stopAnimation();
    clearHideTimer();
    visible.value = false;
    return;
  }
  if (status === 'success' && (completionSettled || !wasActive || snap || reducedMotion.value || document.hidden)) {
    stopAnimation();
    clearHideTimer();
    displayed = 100;
    draw();
    visible.value = false;
    completionSettled = true;
    return;
  }
  visible.value = true;
  wasActive = true;
  if (status === 'paused' || status === 'cancelling') {
    stopAnimation();
    draw();
    return;
  }
  if (snap || reducedMotion.value || document.hidden) {
    stopAnimation();
    displayed = Math.max(displayed, target);
    draw();
    return;
  }
  if (displayed < target && frame === null) frame = requestAnimationFrame(animate);
  else if (status === 'success' && displayed === 100) finishCompletion();
}

const onVisibilityChange = () => sync(true);
watch([() => props.item, reducedMotion], () => sync(), { flush: 'post' });
onMounted(() => {
  draw();
  sync();
  if (wasActive) document.addEventListener('visibilitychange', onVisibilityChange);
});
onUnmounted(() => {
  stopAnimation();
  clearHideTimer();
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div v-show="visible" class="transfer-progress-row">
    <div ref="trackRef" class="transfer-progress" role="progressbar" :aria-label="`${item.name}传输进度`"
      aria-valuemin="0" aria-valuemax="100" aria-valuenow="0">
      <i ref="fillRef" />
    </div>
    <span ref="labelRef" aria-hidden="true">0%</span>
  </div>
</template>

<style scoped>
.transfer-progress-row { display: grid; grid-template-columns: minmax(0, 1fr) 34px; align-items: center; gap: 8px; margin-top: 7px; }
.transfer-progress-row > span { color: var(--app-text-muted); font-size: 11px; text-align: right; font-variant-numeric: tabular-nums; }
.transfer-progress { height: 3px; overflow: hidden; border-radius: 999px; background: color-mix(in srgb, var(--app-text) 9%, transparent); }
.transfer-progress i { display: block; width: 100%; height: 100%; border-radius: inherit; background: var(--color-primary); transform: scaleX(0); transform-origin: left center; }
</style>
