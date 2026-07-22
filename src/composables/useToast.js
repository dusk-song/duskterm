import { ref } from 'vue';

const toasts = ref([]);
let _id = 0;
const keyToId = new Map();
const idToKey = new Map();
const idToTimer = new Map();

const DEFAULT_DURATION = 2500;

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

  // Key-based: replace existing toast with same key
  if (opts.key !== null && keyToId.has(opts.key)) {
    const existingId = keyToId.get(opts.key);
    removeToast(existingId, true);
  }

  const id = ++_id;
  toasts.value.push({ id, message: opts.message, type: opts.type, duration: opts.duration, leaving: false });
  if (opts.key !== null) {
    keyToId.set(opts.key, id);
    idToKey.set(id, opts.key);
  }

  if (opts.duration > 0) {
    const timer = setTimeout(() => removeToast(id), opts.duration);
    idToTimer.set(id, timer);
  }
  return id;
}

function removeToast(id, immediate = false) {
  const idx = toasts.value.findIndex(t => t.id === id);
  if (idx === -1) return;
  clearTimeout(idToTimer.get(id));
  idToTimer.delete(id);
  const key = idToKey.get(id);
  if (key !== undefined) {
    keyToId.delete(key);
    idToKey.delete(id);
  }
  if (immediate) {
    toasts.value.splice(idx, 1);
    return;
  }
  if (toasts.value[idx].leaving) return;
  toasts.value[idx].leaving = true;
  setTimeout(() => {
    toasts.value = toasts.value.filter(t => t.id !== id);
  }, 200);
}

export const toast = {
  success(msg, duration) { return addToast(msg, 'success', duration); },
  error(msg, duration) { return addToast(msg, 'error', duration); },
  info(msg, duration) { return addToast(msg, 'info', duration); },
  warning(msg, duration) { return addToast(msg, 'warning', duration); },
  loading(msg, duration) { return addToast(msg, 'loading', duration); },
  remove(id, immediate = false) { removeToast(id, immediate); },
};
