import { ref } from 'vue';

const toasts = ref([]);
let _id = 0;
const keyMap = {};

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
  if (opts.key && keyMap[opts.key] != null) {
    const existingId = keyMap[opts.key];
    const idx = toasts.value.findIndex(t => t.id === existingId);
    if (idx !== -1) {
      toasts.value.splice(idx, 1);
    }
    clearTimeout(keyMap[opts.key + '_timer']);
  }

  const id = ++_id;
  toasts.value.push({ id, message: opts.message, type: opts.type, duration: opts.duration, leaving: false });
  if (opts.key) {
    keyMap[opts.key] = id;
  }

  if (opts.duration > 0) {
    const timer = setTimeout(() => removeToast(id), opts.duration);
    if (opts.key) keyMap[opts.key + '_timer'] = timer;
  }
  return id;
}

function removeToast(id) {
  const idx = toasts.value.findIndex(t => t.id === id);
  if (idx === -1) return;
  toasts.value[idx].leaving = true;
  setTimeout(() => {
    toasts.value = toasts.value.filter(t => t.id !== id);
    // Clean up key map
    for (const [k, v] of Object.entries(keyMap)) {
      if (v === id) delete keyMap[k];
    }
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
