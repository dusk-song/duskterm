import { convertFileSrc } from '@tauri-apps/api/core';

export const defaultBackgroundSettings = Object.freeze({
  enabled: false,
  resourceId: '',
  fileName: '',
  fit: 'cover',
  blur: 0,
  opacity: 1,
  darkOverlay: 0.35,
  lightOverlay: 0.18,
});

const fits = new Set(['cover', 'contain', 'stretch', 'center', 'tile']);
const clamp = (value, min, max, fallback) => Number.isFinite(Number(value))
  ? Math.min(max, Math.max(min, Number(value)))
  : fallback;

export function normalizeBackgroundSettings(value = {}) {
  return {
    enabled: value?.enabled === true,
    resourceId: String(value?.resourceId || ''),
    fileName: String(value?.fileName || ''),
    fit: fits.has(value?.fit) ? value.fit : 'cover',
    blur: clamp(value?.blur, 0, 40, 0),
    opacity: clamp(value?.opacity, 0, 1, 1),
    darkOverlay: clamp(value?.darkOverlay, 0, 1, 0.35),
    lightOverlay: clamp(value?.lightOverlay, 0, 1, 0.18),
  };
}

export function resolveBackgroundUrl(path = '') {
  if (!path) return '';
  try { return convertFileSrc(path); } catch { return ''; }
}
