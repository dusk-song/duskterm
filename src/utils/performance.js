/**
 * Performance mode manager for low-power / iGPU scenarios.
 *
 * Detects:
 *  - prefers-reduced-motion (user accessibility preference)
 *  - Battery vs AC power (via Navigator.getBattery where available)
 *
 * Applies a data-performance attribute to <html> so CSS can reduce
 * expensive transitions, shadows, and animations.
 *
 * Exported flags allow JS logic (e.g. DesktopPet, xterm) to
 * adapt rendering behaviour accordingly.
 */

const PERF_ATTR = 'data-performance';

/** @type {'auto'|'reduced'} */
let currentMode = 'auto';

/**
 * Re-evaluate and apply the performance mode.
 * Call this whenever conditions change (e.g. battery status flips).
 */
function applyPerformanceMode() {
  const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const batterySaver = window._duskBatterySaver === true;

  const nextMode = prefersReducedMotion || batterySaver ? 'reduced' : 'auto';
  if (nextMode !== currentMode) {
    currentMode = nextMode;
    document.documentElement.setAttribute(PERF_ATTR, currentMode);
  }
  return currentMode;
}

/**
 * Try to detect battery status and apply performance mode when on battery.
 * Falls back silently on platforms that don't support getBattery().
 */
function watchBattery() {
  // Navigator.getBattery is Chrome-only; gracefully degrade elsewhere
  if (typeof navigator.getBattery !== 'function') {
    applyPerformanceMode();
    return;
  }

  navigator.getBattery().then((manager) => {
    const update = () => {
      window._duskBatterySaver = !manager.charging;
      applyPerformanceMode();
    };
    update();
    manager.addEventListener('chargingchange', update);
  }).catch(() => {
    applyPerformanceMode();
  });
}

/**
 * Initialize all performance-related observers.
 * Call once at app startup.
 */
export function initPerformanceMode() {
  // Listen for OS reduced-motion changes
  window.matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', applyPerformanceMode);

  // Watch battery status
  watchBattery();

  // Initial apply
  applyPerformanceMode();
}

/**
 * Returns true when the user is currently on battery power
 * (best-effort — returns false on unsupported browsers).
 */
export function isOnBattery() {
  return window._duskBatterySaver === true;
}

/**
 * Returns true when reduced rendering is active
 * (reduced-motion preference OR battery saver mode).
 */
export function isReducedPerformance() {
  return currentMode === 'reduced';
}

export { PERF_ATTR };
