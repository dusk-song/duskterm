import {
  getPreferenceDefaults,
  getPreferenceStorageKey,
  loadPreference,
  savePreference
} from './preferences';

const TERMINAL_THEME_KEY = getPreferenceStorageKey('terminalTheme');

const terminalThemes = {
  duskWarm: {
    name: 'DuskTerm Warm Auto',
    theme: {
      background: '#111113',
      foreground: '#e4e0d8',
      cursor: '#d1b16b',
      cursorAccent: '#111113',
      selectionBackground: 'rgba(192,132,47,0.46)',
      selectionInactiveBackground: 'rgba(192,132,47,0.28)',
      black: '#111113',
      red: '#d17a72',
      green: '#b8a06a',
      yellow: '#d1b16b',
      blue: '#6ca6d9',
      magenta: '#b59a7a',
      cyan: '#a59b8f',
      white: '#e4e0d8',
      brightBlack: '#6f6860',
      brightRed: '#e18b82',
      brightGreen: '#c7ad78',
      brightYellow: '#e0bd78',
      brightBlue: '#8fc7ff',
      brightMagenta: '#c6aa8a',
      brightCyan: '#b8aea3',
      brightWhite: '#f1ece4'
    }
  },
  duskWarmLight: {
    name: 'DuskTerm Warm Light',
    theme: {
      background: '#f1ece3',
      foreground: '#25221f',
      cursor: '#8a5a16',
      cursorAccent: '#f1ece3',
      selectionBackground: '#d8b86f',
      selectionForeground: '#211c16',
      selectionInactiveBackground: '#ead9b4',
      black: '#25221f',
      red: '#a3483f',
      green: '#6f613a',
      yellow: '#8a5a16',
      blue: '#5f564e',
      magenta: '#795d42',
      cyan: '#625b52',
      white: '#f7f3eb',
      brightBlack: '#6f6860',
      brightRed: '#b75a51',
      brightGreen: '#807047',
      brightYellow: '#9d6b22',
      brightBlue: '#71675e',
      brightMagenta: '#8c6d4e',
      brightCyan: '#766d64',
      brightWhite: '#fffaf2'
    }
  },
  default: {
    name: 'DuskTerm Warm Dark',
    theme: {
      background: '#1e1e1e',
      foreground: '#d4d4d4',
      cursor: '#d4d4d4',
      cursorAccent: '#1e1e1e',
      selectionBackground: 'rgba(255,255,255,0.42)',
      selectionInactiveBackground: 'rgba(255,255,255,0.24)',
      black: '#000000',
      red: '#ff5f5f',
      green: '#5fff87',
      yellow: '#ffd75f',
      blue: '#5f87ff',
      magenta: '#af87ff',
      cyan: '#5fffff',
      white: '#ffffff',
      brightBlack: '#5c6370',
      brightRed: '#ff6c6b',
      brightGreen: '#98be65',
      brightYellow: '#ecbe7b',
      brightBlue: '#51afef',
      brightMagenta: '#c678dd',
      brightCyan: '#46d9ff',
      brightWhite: '#d7d7d7'
    }
  }
};

const defaultTerminalThemeSettings = getPreferenceDefaults('terminalTheme');

function resolveTerminalThemeKey(themeKey) {
  const fallback = defaultTerminalThemeSettings.theme || 'duskWarm';
  const raw = String(themeKey || '').trim();
  if (!raw) return fallback;
  if (terminalThemes[raw]) return raw;

  const lower = raw.toLowerCase();
  return Object.keys(terminalThemes).find((key) => key.toLowerCase() === lower) || fallback;
}

function loadTerminalThemeSettings() {
  const settings = loadPreference('terminalTheme');
  return {
    ...settings,
    theme: resolveTerminalThemeKey(settings.theme)
  };
}

function saveTerminalThemeSettings(settings) {
  return savePreference('terminalTheme', {
    ...settings,
    theme: resolveTerminalThemeKey(settings?.theme)
  });
}

function normalizeXtermTheme(theme = {}) {
  const selectionBackground = theme.selectionBackground || theme.selection || 'rgba(255,255,255,0.42)';
  const selectionInactiveBackground = theme.selectionInactiveBackground || selectionBackground;
  const cursor = theme.cursor || theme.foreground || '#d4d4d4';
  const cursorAccent = theme.cursorAccent || theme.background || '#1e1e1e';
  const { selection, ...rest } = theme;

  return {
    ...rest,
    cursor,
    cursorAccent,
    selectionBackground,
    selectionInactiveBackground
  };
}

function getTerminalTheme(themeKey, isDark = true) {
  const resolvedKey = resolveTerminalThemeKey(themeKey);
  const effectiveKey = !isDark && resolvedKey === 'duskWarm'
    ? 'duskWarmLight'
    : resolvedKey;
  return normalizeXtermTheme(terminalThemes[effectiveKey]?.theme || terminalThemes.duskWarm.theme);
}

function getTerminalThemeOptions() {
  return Object.entries(terminalThemes).map(([key, value]) => ({
    key,
    name: value.name
  }));
}

export {
  TERMINAL_THEME_KEY,
  terminalThemes,
  defaultTerminalThemeSettings,
  loadTerminalThemeSettings,
  saveTerminalThemeSettings,
  getTerminalTheme,
  getTerminalThemeOptions
};
