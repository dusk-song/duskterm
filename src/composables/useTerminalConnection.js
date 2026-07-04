export function useTerminalConnection({ sshStore, activeKey }) {
  const ensureSplitSession = async (sourceSessionId) => {
    const source = sshStore.getSession(sourceSessionId);
    if (!source?.config) return null;

    const config = {
      ...source.config,
      name: source.name ? `${source.name} (分屏)` : source.config.name
    };

    const splitId = await sshStore.connectLogicWithMeta(config, {
      isSplitChild: true,
      parentId: activeKey.value,
      noActivate: true
    });

    return splitId;
  };

  return {
    ensureSplitSession
  };
}
