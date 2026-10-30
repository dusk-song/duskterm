import { computed } from 'vue';

export function useTerminalPanels(sshStore) {
  const activeKey = computed({
    get: () => sshStore.activeSessionId,
    set: (value) => {
      sshStore.activeSessionId = value;
    }
  });

  const visibleSessions = computed(() => sshStore.sessions.filter((session) => !session.isSplitChild));

  const setActivePanel = (panelId) => {
    if (!panelId) return;
    activeKey.value = panelId;
  };

  const movePanel = (dragId, dropId, placement) => {
    if (!dragId || !dropId || dragId === dropId) return;
    sshStore.moveSession(dragId, dropId, placement);
  };

  return {
    activeKey,
    visibleSessions,
    setActivePanel,
    movePanel
  };
}
