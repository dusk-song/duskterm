import { computed } from 'vue';
import { useRedisStore } from '@/stores/redis';
import { useWorkspaceStore } from '@/stores/workspace';

export function useTerminalPanels(sshStore) {
  const workspace = useWorkspaceStore();
  const redisStore = useRedisStore();
  const activeKey = computed({
    get: () => workspace.activeId,
    set: value => { workspace.activeId = value; }
  });

  const visibleSessions = computed(() => {
    const terminals = new Map(sshStore.sessions.map(session => [session.id, session]));
    return workspace.panels.map(panel => panel.kind === 'redis'
      ? redisStore.sessions[panel.id]
      : terminals.get(panel.id)
    ).filter(Boolean);
  });

  const setActivePanel = (panelId) => {
    if (!panelId) return;
    activeKey.value = panelId;
  };

  const movePanel = (dragId, dropId, placement) => {
    if (!dragId || !dropId || dragId === dropId) return;
    workspace.move(dragId, dropId, placement);
  };

  return {
    activeKey,
    visibleSessions,
    setActivePanel,
    movePanel
  };
}
