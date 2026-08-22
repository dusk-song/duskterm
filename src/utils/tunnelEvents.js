export const TUNNELS_CHANGED_EVENT = 'duskterm:tunnels-changed';

export function notifyTunnelsChanged(detail = null) {
  window.dispatchEvent(new CustomEvent(TUNNELS_CHANGED_EVENT, { detail }));
}
