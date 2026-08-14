import { ref } from 'vue';

const toasts = ref([]);
let _id = 0;
const keyToId = new Map();
const idToKey = new Map();
const idToTimer = new Map();
const idToLeaveTimer = new Map();

const DEFAULT_DURATION = 2500;
const MAX_TOASTS = 3;

export function useToast() {
  return { toasts, addToast, toast };
}

function normalizeOptions(options, defaultType = 'info') {
  if (typeof options === 'string') {
    return { message: options, type: defaultType, key: null, duration: DEFAULT_DURATION };
  }
  const message = options?.content ?? options?.message ?? '';
  const requestedDuration = Number(options?.duration ?? DEFAULT_DURATION);
  return {
    message: String(message),
    type: options?.type || defaultType,
    key: options?.key ?? null,
    duration: Number.isFinite(requestedDuration) ? Math.max(0, requestedDuration) : DEFAULT_DURATION,
  };
}

function addToast(raw, type = 'info', duration) {
  const opts = normalizeOptions(raw, type);
  if (duration !== undefined) {
    const requestedDuration = Number(duration);
    opts.duration = Number.isFinite(requestedDuration) ? Math.max(0, requestedDuration) : DEFAULT_DURATION;
  }

  // Key-based notifications (for example loading → success) update in place.
  if (opts.key !== null && keyToId.has(opts.key)) {
    const existingId = keyToId.get(opts.key);
    const existing = toasts.value.find((item) => item.id === existingId);
    if (existing) {
      clearTimeout(idToLeaveTimer.get(existingId));
      idToLeaveTimer.delete(existingId);
      existing.message = opts.message;
      existing.type = opts.type;
      existing.duration = opts.duration;
      existing.leaving = false;
      const existingIndex = toasts.value.findIndex((item) => item.id === existingId);
      if (existingIndex >= 0) {
        toasts.value.splice(existingIndex, 1);
        toasts.value.push(existing);
      }
      scheduleToastRemoval(existingId, opts.duration);
      return existingId;
    }
    keyToId.delete(opts.key);
    idToKey.delete(existingId);
  }

  // Repeated identical feedback refreshes one card instead of filling the row.
  const duplicateIndex = opts.key === null
    ? toasts.value.findIndex((item) => (
      !item.leaving
      && !idToKey.has(item.id)
      && item.type === opts.type
      && item.message === opts.message
    ))
    : -1;
  if (duplicateIndex >= 0) {
    const [existing] = toasts.value.splice(duplicateIndex, 1);
    existing.duration = opts.duration;
    toasts.value.push(existing);
    scheduleToastRemoval(existing.id, opts.duration);
    return existing.id;
  }

  const id = ++_id;
  toasts.value.push({ id, message: opts.message, type: opts.type, duration: opts.duration, leaving: false });
  if (opts.key !== null) {
    keyToId.set(opts.key, id);
    idToKey.set(id, opts.key);
  }

  while (toasts.value.length > MAX_TOASTS) {
    removeToast(toasts.value[0].id, true);
  }
  scheduleToastRemoval(id, opts.duration);
  return id;
}

function scheduleToastRemoval(id, duration) {
  clearTimeout(idToTimer.get(id));
  idToTimer.delete(id);
  if (duration <= 0) return;
  const timer = setTimeout(() => removeToast(id), duration);
  idToTimer.set(id, timer);
}

function clearToastKey(id) {
  const key = idToKey.get(id);
  if (key === undefined) return;
  keyToId.delete(key);
  idToKey.delete(id);
}

function removeToast(id, immediate = false) {
  const idx = toasts.value.findIndex(t => t.id === id);
  if (idx === -1) return;
  clearTimeout(idToTimer.get(id));
  idToTimer.delete(id);
  if (immediate) {
    clearTimeout(idToLeaveTimer.get(id));
    idToLeaveTimer.delete(id);
    clearToastKey(id);
    toasts.value.splice(idx, 1);
    return;
  }
  if (toasts.value[idx].leaving) return;
  toasts.value[idx].leaving = true;
  const leaveTimer = setTimeout(() => {
    idToLeaveTimer.delete(id);
    clearToastKey(id);
    toasts.value = toasts.value.filter(t => t.id !== id);
  }, 200);
  idToLeaveTimer.set(id, leaveTimer);
}

export const toast = {
  success(msg, duration) { return addToast(msg, 'success', duration); },
  error(msg, duration) { return addToast(msg, 'error', duration); },
  info(msg, duration) { return addToast(msg, 'info', duration); },
  warning(msg, duration) { return addToast(msg, 'warning', duration); },
  loading(msg, duration) { return addToast(msg, 'loading', duration); },
  remove(id, immediate = false) { removeToast(id, immediate); },
};
