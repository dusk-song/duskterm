export function useTerminalConnection({ sshStore, activeKey }) {
  const ensureSplitSession = async (sourceSessionId) => {
    const source = sshStore.getSession(sourceSessionId);
    if (!source?.config) return null;
    const workspaceSessionId = source.workspaceSessionId || source.parentId || activeKey.value;
    return sshStore.openSplitShell(sourceSessionId, workspaceSessionId);
  };

  return {
    ensureSplitSession
  };
}
