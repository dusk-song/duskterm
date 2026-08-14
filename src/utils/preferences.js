import { defaultDesktopPetSettings } from './desktopPet';
import { defaultBackgroundSettings } from './background';

const PREFERENCE_DEFINITIONS = {
  appTheme: {
    storageKey: 'duskterm-theme',
    defaults: {
      mode: 'system',  // 'light' | 'dark' | 'system'
    }
  },
  keybindings: {
    storageKey: 'keybindings-v1',
    defaults: {
      splitHorizontal: 'Ctrl+Shift+U',
      splitVertical: 'Ctrl+Alt+I',
      closeSession: 'Ctrl+Shift+W',
      closeSplitTerminal: 'Ctrl+Alt+W',
      nextSession: 'Ctrl+Tab',
      prevSession: 'Ctrl+Shift+Tab',
      sessionList: 'Ctrl+Alt+1',
      sftpPanel: 'Alt+2',
      commandKnowledge: 'Ctrl+Alt+3',
      transferList: 'Alt+4',
      overview: 'Ctrl+`',
      copySession: 'Ctrl+P',
      toggleLineNumbers: 'Ctrl+Alt+L',
      toggleFind: 'Ctrl+Shift+F',
      selectTerminalSuggestion: 'Alt+ArrowDown'
    }
  },
  terminalTheme: {
    storageKey: 'terminal-theme-v1',
    defaults: {
      theme: 'duskWarm',
      showLineNumbers: false
    }
  },
  commandHistory: {
    storageKey: 'command-history-settings-v1',
    defaults: {
      enabled: true
    }
  },
  mainUi: {
    storageKey: 'main-ui-settings-v1',
    defaults: {
      background: defaultBackgroundSettings,
      desktopPet: defaultDesktopPetSettings,
      recentSessions: {
        enabled: true,
        limit: 6
      }
    }
  }
};

const getPreferenceMeta = (name) => PREFERENCE_DEFINITIONS[name] || null;

function getPreferenceStorageKey(name) {
  return getPreferenceMeta(name)?.storageKey || '';
}

function getPreferenceDefaults(name) {
  const defaults = getPreferenceMeta(name)?.defaults || {};
  return { ...defaults };
}

function loadPreference(name) {
  const meta = getPreferenceMeta(name);
  if (!meta) return {};
  try {
    const raw = localStorage.getItem(meta.storageKey);
    if (!raw) return { ...meta.defaults };
    const parsed = JSON.parse(raw);
    if (name === 'keybindings' && parsed?.commandKnowledge === 'Ctrl+Shift+3') {
      parsed.commandKnowledge = meta.defaults.commandKnowledge;
    }
    return { ...meta.defaults, ...(parsed || {}) };
  } catch (e) {
    return { ...meta.defaults };
  }
}

function savePreference(name, value) {
  const meta = getPreferenceMeta(name);
  if (!meta) return { ...(value || {}) };
  const next = { ...meta.defaults, ...(value || {}) };
  localStorage.setItem(meta.storageKey, JSON.stringify(next));
  return next;
}

export {
  getPreferenceDefaults, getPreferenceStorageKey, loadPreference, PREFERENCE_DEFINITIONS, savePreference
};

