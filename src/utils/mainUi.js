import {
  getPreferenceDefaults,
  getPreferenceStorageKey,
  loadPreference,
  savePreference
} from './preferences';
import { normalizeDesktopPetSettings } from './desktopPet';
import { normalizeBackgroundSettings } from './background';

const MAIN_UI_SETTINGS_KEY = getPreferenceStorageKey('mainUi');
const defaultMainUiSettings = getPreferenceDefaults('mainUi');

function normalizeMainUiSettings(settings = {}) {
  const next = {
    ...defaultMainUiSettings,
    ...(settings || {})
  };
  next.desktopPet = normalizeDesktopPetSettings(next.desktopPet || defaultMainUiSettings.desktopPet);
  next.background = normalizeBackgroundSettings(next.background || defaultMainUiSettings.background);
  const recentSessions = {
    ...defaultMainUiSettings.recentSessions,
    ...(next.recentSessions || {})
  };
  next.recentSessions = {
    enabled: recentSessions.enabled !== false,
    limit: Math.min(100, Math.max(1, Math.round(Number(recentSessions.limit) || defaultMainUiSettings.recentSessions.limit)))
  };
  return next;
}

function loadMainUiSettings() {
  return normalizeMainUiSettings(loadPreference('mainUi'));
}

function saveMainUiSettings(settings) {
  return savePreference('mainUi', normalizeMainUiSettings(settings));
}

export {
  MAIN_UI_SETTINGS_KEY,
  defaultMainUiSettings,
  normalizeMainUiSettings,
  loadMainUiSettings,
  saveMainUiSettings
};
