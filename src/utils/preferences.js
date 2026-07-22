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
      sftpPanel: 'Ctrl+Alt+2',
      commandKnowledge: 'Ctrl+Alt+3',
      overview: 'Ctrl+`',
      copySession: 'Ctrl+P',
      toggleLineNumbers: 'Ctrl+Alt+L',
      toggleFind: 'Ctrl+Shift+F'
    }
  },
  monitor: {
    storageKey: 'monitor-settings-v1',
    defaults: {
      showMonitor: true,
      showCpu: true,
      showMemory: true,
      showDisk: true,
      showNet: true,
      refreshIntervalMs: 1000,
      diskIntervalMs: 5000,
      localColor: '#b8a06a',
      remoteColor: '#c0842f',
      labelColor: '#8c8c8c',
      valueColor: 'inherit'
    }
  },
  terminalTheme: {
    storageKey: 'terminal-theme-v1',
    defaults: {
      theme: 'duskWarm',
      showLineNumbers: false
    }
  },
  mainUi: {
    storageKey: 'main-ui-settings-v1',
    defaults: {
      background: defaultBackgroundSettings,
      desktopPet: defaultDesktopPetSettings
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

