import {
  getPreferenceDefaults,
  getPreferenceStorageKey,
  loadPreference,
  savePreference
} from './preferences';

const LIGHTBAR_SETTINGS_KEY = getPreferenceStorageKey('lightbar');
const defaultLightbarSettings = getPreferenceDefaults('lightbar');

function loadLightbarSettings() {
  return loadPreference('lightbar');
}

function saveLightbarSettings(settings) {
  return savePreference('lightbar', settings);
}

export {
  LIGHTBAR_SETTINGS_KEY,
  defaultLightbarSettings,
  loadLightbarSettings,
  saveLightbarSettings
};
