export function resolveFocusedSessionId(activePanelId, focusedLeaf = {}) {
  if (!activePanelId) return '';
  return focusedLeaf?.[activePanelId] || activePanelId;
}

export function createSessionBooleanState(defaultValue = false) {
  const values = new Map();

  return {
    get(sessionId) {
      if (!sessionId) return Boolean(defaultValue);
      return values.has(sessionId) ? values.get(sessionId) : Boolean(defaultValue);
    },
    set(sessionId, value) {
      if (!sessionId) return Boolean(defaultValue);
      const next = Boolean(value);
      values.set(sessionId, next);
      return next;
    },
    toggle(sessionId) {
      return this.set(sessionId, !this.get(sessionId));
    },
    delete(sessionId) {
      values.delete(sessionId);
    }
  };
}
