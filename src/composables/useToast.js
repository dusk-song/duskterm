import { ref } from 'vue';

const toasts = ref([]);
let _id = 0;
const keyToId = new Map();
const keyToTimer = new Map();
const idToKey = new Map();

const DEFAULT_DURATION = 2500;

export function useToast() {
  return { toasts, addToast, toast };
}

function normalizeOptions(options, defaultType = 'info') {
  if (typeof options === 'string') {
    return { message: options, type: defaultType, key: null, duration: DEFAULT_DURATION };
  }
  const msg = options?.content || options?.message || '';
  return {
    message: msg,
    type: options?.type || defaultType,
    key: options?.key || null,
    duration: options?.duration ?? DEFAULT_DURATION,
  };
}

function addToast(raw, type = 'info', duration) {
  const opts = normalizeOptions(raw, type);
  if (duration !== undefined) opts.duration = duration;

  // Key-based: replace existing toast with same key
  if (opts.key && keyToId.has(opts.key)) {
    const existingId = keyToId.get(opts.key);
    const idx = toasts.value.findIndex(t => t.id === existingId);
    if (idx !== -1) {
      toasts.value.splice(idx, 1);
    }
    clearTimeout(keyToTimer.get(opts.key));
    keyToTimer.delete(opts.key);
    keyToId.delete(opts.key);
    idToKey.delete(existingId);
  }

  const id = ++_id;
  toasts.value.push({ id, message: opts.message, type: opts.type, duration: opts.duration, leaving: false });
  if (opts.key) {
    keyToId.set(opts.key, id);
    idToKey.set(id, opts.key);
  }

  if (opts.duration > 0) {
    const timer = setTimeout(() => removeToast(id), opts.duration);
    if (opts.key) keyToTimer.set(opts.key, timer);
  }
  return id;
}

function removeToast(id) {
  const idx = toasts.value.findIndex(t => t.id === id);
  if (idx === -1) return;
  const key = idToKey.get(id);
  if (key) {
    clearTimeout(keyToTimer.get(key));
    keyToTimer.delete(key);
    keyToId.delete(key);
    idToKey.delete(id);
  }
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
  remove(id) { removeToast(id); },
};
