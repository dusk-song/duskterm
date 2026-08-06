export const WINDOW_PLATFORM_WINDOWS = 'windows';
export const WINDOW_PLATFORM_MACOS = 'macos';
export const WINDOW_PLATFORM_LINUX = 'linux';
export const WINDOW_PLATFORM_UNKNOWN = 'unknown';

export const WINDOW_DRAG_MODE_NATIVE_REGION = 'native-region';
export const WINDOW_DRAG_MODE_TAURI = 'tauri';

export function resolveWindowPlatform({
  userAgentDataPlatform = '',
  navigatorPlatform = '',
  userAgent = '',
} = {}) {
  const platform = `${userAgentDataPlatform} ${navigatorPlatform} ${userAgent}`.toLowerCase();

  if (/windows|win32|win64/.test(platform)) return WINDOW_PLATFORM_WINDOWS;
  if (/macintosh|macintel|macos|mac os/.test(platform)) return WINDOW_PLATFORM_MACOS;
  if (/linux|x11|wayland/.test(platform)) return WINDOW_PLATFORM_LINUX;
  return WINDOW_PLATFORM_UNKNOWN;
}

export function supportsNativeWindowRegion(cssSupports = globalThis.CSS?.supports?.bind(globalThis.CSS)) {
  if (typeof cssSupports !== 'function') return false;

  try {
    return cssSupports('app-region', 'drag') || cssSupports('-webkit-app-region', 'drag');
  } catch {
    return false;
  }
}

export function resolveWindowDragMode(platform, nativeRegionSupported = false) {
  return platform === WINDOW_PLATFORM_WINDOWS && nativeRegionSupported
    ? WINDOW_DRAG_MODE_NATIVE_REGION
    : WINDOW_DRAG_MODE_TAURI;
}
