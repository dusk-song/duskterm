import { open } from '@tauri-apps/plugin-dialog';
import { onMounted, onUnmounted } from 'vue';
import { toast } from '@/composables/useToast';
import { invokeCommand, listenEvent } from '@/utils/ipc';

export function useTerminalTransferCoordinator({ transferStore }) {
  let selectionBusy = false;
  let disposed = false;
  const activeRequestIds = new Set();
  const reportedFailureRequestIds = new Set();
  const unlisteners = [];

  const rejectRequest = (request) => invokeCommand('reject_terminal_transfer', {
    workspaceSessionId: request.workspaceSessionId,
    channelId: request.channelId ?? null,
    requestId: request.requestId,
  });

  const handleEnded = (payload = {}) => {
    const requests = Array.isArray(transferStore.terminalRequests)
      ? transferStore.terminalRequests
      : [];
    const matchingRequests = requests.filter((request) => (
      (payload.requestId && request.requestId === payload.requestId)
      || (!payload.requestId && payload.sessionId && request.sessionId === payload.sessionId)
    ));
    const alreadyReported = (
      (payload.requestId && reportedFailureRequestIds.has(payload.requestId))
      || matchingRequests.some((request) => reportedFailureRequestIds.has(request.requestId))
    );
    matchingRequests.forEach((request) => {
      reportedFailureRequestIds.delete(request.requestId);
      activeRequestIds.delete(request.requestId);
    });
    if (payload.requestId) {
      reportedFailureRequestIds.delete(payload.requestId);
      activeRequestIds.delete(payload.requestId);
    }
    const hasFailedTask = transferStore.tasks.some((task) => (
      task.protocol === 'zmodem'
      && task.status === 'failed'
      && ((payload.operationId && task.operationId === payload.operationId)
        || (!payload.operationId && payload.sessionId && task.sessionId === payload.sessionId))
    ));
    if (payload.error && payload.error !== '会话已关闭' && !hasFailedTask && !alreadyReported) {
      toast.error(`ZMODEM 传输失败：${payload.error}`);
    }
    transferStore.finishTerminalTransfer(payload);
  };

  const handleRequest = async (request = {}) => {
    if (!request.requestId || !request.workspaceSessionId) return;
    if (activeRequestIds.has(request.requestId)) return;
    activeRequestIds.add(request.requestId);
    transferStore.registerTerminalRequest(request);

    if (selectionBusy) {
      try {
        await rejectRequest(request);
      } catch (error) {
        handleEnded(request);
        toast.error(`拒绝 ZMODEM 请求失败：${error}`);
      }
      toast.warning('已有文件选择窗口，新的 ZMODEM 请求已拒绝');
      return;
    }

    selectionBusy = true;
    try {
      const upload = request.direction === 'upload';
      const selected = await open(upload
        ? { title: '选择要通过 ZMODEM 上传的文件', multiple: true, directory: false }
        : { title: '选择 ZMODEM 下载目录', multiple: false, directory: true });
      const paths = upload
        ? (Array.isArray(selected) ? selected : (selected ? [selected] : []))
        : [];

      if ((!upload && !selected) || (upload && paths.length === 0)) {
        await rejectRequest(request);
        return;
      }

      await invokeCommand('accept_terminal_transfer', {
        workspaceSessionId: request.workspaceSessionId,
        channelId: request.channelId ?? null,
        requestId: request.requestId,
        selection: upload
          ? { kind: 'upload', paths }
          : { kind: 'download', directory: selected, collisionPolicy: 'autoRename' },
      });
      transferStore.requestPanelOpen();
    } catch (error) {
      reportedFailureRequestIds.add(request.requestId);
      try {
        await rejectRequest(request);
      } catch {
        // The backend may already be recovering from a failed accept and will
        // emit the authoritative ended event shortly. Clear the visible request
        // now, but keep the report marker to avoid a duplicate error toast.
        activeRequestIds.delete(request.requestId);
        transferStore.finishTerminalTransfer(request);
      }
      toast.error(`启动 ZMODEM 传输失败：${error}`);
    } finally {
      selectionBusy = false;
    }
  };

  onMounted(async () => {
    const results = await Promise.allSettled([
      listenEvent('transfer-progress', transferStore.applyProgress),
      listenEvent('terminal-transfer-request', handleRequest),
      listenEvent('terminal-transfer-ended', handleEnded),
    ]);
    const listeners = results
      .filter((result) => result.status === 'fulfilled')
      .map((result) => result.value);
    if (results.some((result) => result.status === 'rejected')) {
      listeners.forEach((unlisten) => unlisten?.());
      if (!disposed) toast.error('初始化终端传输监听失败');
      return;
    }
    if (disposed) {
      listeners.forEach((unlisten) => unlisten?.());
    } else {
      unlisteners.push(...listeners);
    }
  });

  onUnmounted(() => {
    disposed = true;
    unlisteners.splice(0).forEach((unlisten) => unlisten?.());
    activeRequestIds.clear();
    reportedFailureRequestIds.clear();
  });
}
