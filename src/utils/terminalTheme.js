import {
  getPreferenceDefaults,
  getPreferenceStorageKey,
  loadPreference,
  savePreference
} from './preferences';

const TERMINAL_THEME_KEY = getPreferenceStorageKey('terminalTheme');

const terminalThemes = {
  duskWarm: {
    name: 'DuskTerm Warm',
    theme: {
      background: '#111113',
      foreground: '#e4e0d8',
      cursor: '#d1b16b',
      cursorAccent: '#111113',
      selection: 'rgba(192,132,47,0.22)',
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
      background: '#f7f3eb',
      foreground: '#25221f',
      cursor: '#8a5a16',
      cursorAccent: '#f7f3eb',
      selection: 'rgba(192,132,47,0.24)',
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
    name: 'DuskTerm Dark',
    theme: {
      background: '#1e1e1e',
      foreground: '#d4d4d4',
      cursor: '#d4d4d4',
      selection: 'rgba(255,255,255,0.2)',
      black: '#000000', red: '#ff5f5f', green: '#5fff87', yellow: '#ffd75f',
      blue: '#5f87ff', magenta: '#af87ff', cyan: '#5fffff', white: '#ffffff',
      brightBlack: '#5c6370', brightRed: '#ff6c6b', brightGreen: '#98be65',
      brightYellow: '#ecbe7b', brightBlue: '#51afef', brightMagenta: '#c678dd',
      brightCyan: '#46d9ff', brightWhite: '#d7d7d7'
    }
  },
  dracula: {
    name: 'Dracula',
    theme: {
      background: '#282a36', foreground: '#f8f8f2', cursor: '#f8f8f2',
      selection: 'rgba(189,147,249,0.3)',
      black: '#21222c', red: '#ff5555', green: '#50fa7b', yellow: '#f1fa8c',
      blue: '#bd93f9', magenta: '#ff79c6', cyan: '#8be9fd', white: '#f8f8f2',
      brightBlack: '#6272a4', brightRed: '#ff6e6e', brightGreen: '#69ff94',
      brightYellow: '#ffffa5', brightBlue: '#d6acff', brightMagenta: '#ff92df',
      brightCyan: '#a4ffff', brightWhite: '#ffffff'
    }
  },
  nord: {
    name: 'Nord',
    theme: {
      background: '#2e3440', foreground: '#d8dee9', cursor: '#d8dee9',
      selection: 'rgba(136,192,208,0.3)',
      black: '#3b4252', red: '#bf616a', green: '#a3be8c', yellow: '#ebcb8b',
      blue: '#81a1c1', magenta: '#b48ead', cyan: '#88c0d0', white: '#e5e9f0',
      brightBlack: '#4c566a', brightRed: '#bf616a', brightGreen: '#a3be8c',
      brightYellow: '#ebcb8b', brightBlue: '#81a1c1', brightMagenta: '#b48ead',
      brightCyan: '#8fbcbb', brightWhite: '#eceff4'
    }
  },
  gruvbox: {
    name: 'Gruvbox Dark',
    theme: {
      background: '#282828', foreground: '#ebdbb2', cursor: '#ebdbb2',
      selection: 'rgba(146,131,116,0.3)',
      black: '#282828', red: '#cc241d', green: '#98971a', yellow: '#d79921',
      blue: '#458588', magenta: '#b16286', cyan: '#689d6a', white: '#a89984',
      brightBlack: '#928374', brightRed: '#fb4934', brightGreen: '#b8bb26',
      brightYellow: '#fabd2f', brightBlue: '#83a598', brightMagenta: '#d3869b',
      brightCyan: '#8ec07c', brightWhite: '#ebdbb2'
    }
  },
  tokyoNight: {
    name: 'Tokyo Night',
    theme: {
      background: '#1a1b26', foreground: '#c0caf5', cursor: '#c0caf5',
      selection: 'rgba(51,70,100,0.4)',
      black: '#15161e', red: '#f7768e', green: '#9ece6a', yellow: '#e0af68',
      blue: '#7aa2f7', magenta: '#bb9af7', cyan: '#7dcfff', white: '#a9b1d6',
      brightBlack: '#414868', brightRed: '#f7768e', brightGreen: '#9ece6a',
      brightYellow: '#e0af68', brightBlue: '#7aa2f7', brightMagenta: '#bb9af7',
      brightCyan: '#7dcfff', brightWhite: '#c0caf5'
    }
  },
  solarizedDark: {
    name: 'Solarized Dark',
    theme: {
      background: '#002b36', foreground: '#839496', cursor: '#839496',
      selection: 'rgba(6,63,77,0.5)',
      black: '#073642', red: '#dc322f', green: '#859900', yellow: '#b58900',
      blue: '#268bd2', magenta: '#d33682', cyan: '#2aa198', white: '#eee8d5',
      brightBlack: '#002b36', brightRed: '#cb4b16', brightGreen: '#586e75',
      brightYellow: '#657b83', brightBlue: '#839496', brightMagenta: '#6c71c4',
      brightCyan: '#93a1a1', brightWhite: '#fdf6e3'
    }
  },
  oneDark: {
    name: 'One Dark',
    theme: {
      background: '#282c34', foreground: '#abb2bf', cursor: '#528bff',
      selection: 'rgba(62,68,81,0.5)',
      black: '#282c34', red: '#e06c75', green: '#98c379', yellow: '#e5c07b',
      blue: '#61afef', magenta: '#c678dd', cyan: '#56b6c2', white: '#abb2bf',
      brightBlack: '#545862', brightRed: '#e06c75', brightGreen: '#98c379',
      brightYellow: '#e5c07b', brightBlue: '#61afef', brightMagenta: '#c678dd',
      brightCyan: '#56b6c2', brightWhite: '#c8ccd4'
    }
  },
  catppuccin: {
    name: 'Catppuccin Mocha',
    theme: {
      background: '#1e1e2e', foreground: '#cdd6f4', cursor: '#f5e0dc',
      selection: 'rgba(108,112,134,0.3)',
      black: '#45475a', red: '#f38ba8', green: '#a6e3a1', yellow: '#f9e2af',
      blue: '#89b4fa', magenta: '#f5c2e7', cyan: '#94e2d5', white: '#bac2de',
      brightBlack: '#585b70', brightRed: '#f38ba8', brightGreen: '#a6e3a1',
      brightYellow: '#f9e2af', brightBlue: '#89b4fa', brightMagenta: '#f5c2e7',
      brightCyan: '#94e2d5', brightWhite: '#a6adc8'
    }
  },
  monokai: {
    name: 'Monokai',
    theme: {
      background: '#010101ff', foreground: '#F8F8F2', cursor: '#F8F8F2',
      cursorAccent: '#272822', selection: '#49483E',
      black: '#272822', red: '#F92672', green: '#A6E22E', yellow: '#E6DB74',
      blue: '#66D9EF', magenta: '#AE81FF', cyan: '#66D9EF', white: '#F8F8F2',
      brightBlack: '#75715E', brightRed: '#FF5C93', brightGreen: '#B4EC45',
      brightYellow: '#FD971F', brightBlue: '#7DDCF2', brightMagenta: '#C2A1FF',
      brightCyan: '#8DE8F6', brightWhite: '#FFFFFF'
    }
  },
  ohMyZsh: {
    name: 'oh-my-zsh',
    theme: {
      background: '#0f172a', foreground: '#f8fafc', cursor: '#f8fafc',
      selection: 'rgba(255,255,255,0.2)',
      black: '#0b1220', red: '#f87171', green: '#4ade80', yellow: '#fbbf24',
      blue: '#60a5fa', magenta: '#c084fc', cyan: '#22d3ee', white: '#e2e8f0',
      brightBlack: '#475569', brightRed: '#fb7185', brightGreen: '#86efac',
      brightYellow: '#fde047', brightBlue: '#93c5fd', brightMagenta: '#d8b4fe',
      brightCyan: '#67e8f9', brightWhite: '#f8fafc'
    }
  }
};

const defaultTerminalThemeSettings = getPreferenceDefaults('terminalTheme');

function loadTerminalThemeSettings() {
  return loadPreference('terminalTheme');
}

function saveTerminalThemeSettings(settings) {
  return savePreference('terminalTheme', settings);
}

function resolveTerminalThemeKey(themeKey) {
  const fallback = 'default';
  const raw = String(themeKey || '').trim();
  if (!raw) return fallback;
  if (terminalThemes[raw]) return raw;

  const lower = raw.toLowerCase();
  const matched = Object.keys(terminalThemes).find((key) => key.toLowerCase() === lower);
  return matched || fallback;
}

function getTerminalTheme(themeKey, isDark = true) {
  const resolvedKey = resolveTerminalThemeKey(themeKey);
  const effectiveKey = !isDark && resolvedKey === 'duskWarm'
    ? 'duskWarmLight'
    : resolvedKey;
  return terminalThemes[effectiveKey]?.theme || terminalThemes.default.theme;
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
