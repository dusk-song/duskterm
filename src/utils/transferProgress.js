// Reserve 100% for confirmed success: writing all bytes can still be followed by remote finalization.
export function transferProgressTarget(item) {
  if (item?.status === 'success') return 100;
  const loaded = Number(item?.loaded);
  const total = Number(item?.total);
  const percent = Number.isFinite(loaded) && Number.isFinite(total) && total > 0
    ? loaded / total * 100
    : Number(item?.progress);
  return Number.isFinite(percent) ? Math.max(0, Math.min(99, percent)) : 0;
}

// Time-based easing with a speed limit prevents a first large sample from flashing across the track.
export function advanceTransferProgress(current, target, elapsedMs) {
  const value = Number.isFinite(current) ? Math.max(0, Math.min(100, current)) : 0;
  const goal = Number.isFinite(target) ? Math.max(0, Math.min(100, target)) : value;
  const delta = goal - value;
  if (delta <= 0) return value;
  const elapsed = Number.isFinite(elapsedMs) ? Math.max(0, Math.min(64, elapsedMs)) : 0;
  if (!elapsed) return value;
  if (delta < 0.05) return goal;
  return value + Math.min(delta * (1 - Math.exp(-elapsed / 120)), elapsed * 0.125);
}
