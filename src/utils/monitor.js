import {
  getPreferenceDefaults,
  getPreferenceStorageKey,
  loadPreference,
  savePreference
} from './preferences';

const MONITOR_SETTINGS_KEY = getPreferenceStorageKey('monitor');
const defaultMonitorSettings = getPreferenceDefaults('monitor');

function loadMonitorSettings() {
  return loadPreference('monitor');
}

function saveMonitorSettings(settings) {
  return savePreference('monitor', settings);
}

export {
  MONITOR_SETTINGS_KEY,
  defaultMonitorSettings,
  loadMonitorSettings,
  saveMonitorSettings
};
