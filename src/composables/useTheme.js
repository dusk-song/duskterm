import { useDark, useToggle } from '@vueuse/core';
import { computed, ref } from 'vue';

// ── Singleton — ensures only one useDark instance across the app ──
const isDark = useDark({
  selector: 'html',
  attribute: 'class',
  valueDark: 'dark',
  valueLight: '',
  storageKey: 'duskterm-theme',
});

const _rawToggle = useToggle(isDark);

// Wrapped toggle that marks manual interaction
const toggleTheme = () => {
  _rawToggle();
  isFollowingSystem.value = false;
};

// Track whether the user is following system preference
const _stored = (() => { try { return localStorage.getItem('duskterm-theme'); } catch { return null; } })();
const isFollowingSystem = ref(_stored === null);

// Sync legacy data-theme attribute for backward compat
const _syncDataTheme = () => {
  if (isDark.value) {
    document.documentElement.setAttribute('data-theme', 'dark');
  } else {
    document.documentElement.removeAttribute('data-theme');
  }
};

// Watch for changes
import { watch } from 'vue';
watch(isDark, _syncDataTheme, { immediate: true });

export function useTheme() {
  const theme = computed(() => isDark.value ? 'dark' : 'light');

  const setTheme = (mode) => {
    if (mode === 'dark') {
      isDark.value = true;
    } else if (mode === 'light') {
      isDark.value = false;
    } else if (mode === 'system') {
      followSystem();
      return;
    }
    isFollowingSystem.value = false;
  };

  const followSystem = () => {
    try { localStorage.removeItem('duskterm-theme'); } catch { /* noop */ }
    isFollowingSystem.value = true;
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    isDark.value = prefersDark;
  };

  return {
    isDark,
    theme,
    toggleTheme,
    setTheme,
    followSystem,
    isFollowingSystem,
  };
}
