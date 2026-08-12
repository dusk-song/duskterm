import { toast } from '@/composables/useToast';

export function useTerminalConnection({ sshStore, activeKey }) {
  const ensureSplitSession = async (sourceSessionId) => {
    const source = sshStore.getSession(sourceSessionId);
    if (!source?.config) return null;
    const protocol = String(source.config.protocol || 'ssh').toLowerCase();
    if (protocol !== 'ssh' && protocol !== 'local') {
      toast.info(protocol === 'serial' ? '串口会话暂不支持分屏' : '当前会话类型暂不支持分屏');
      return null;
    }
    const workspaceSessionId = source.workspaceSessionId || source.parentId || activeKey.value;
    return sshStore.openSplitShell(sourceSessionId, workspaceSessionId);
  };

  return {
    ensureSplitSession
  };
}
