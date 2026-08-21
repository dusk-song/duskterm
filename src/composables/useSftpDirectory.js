import { ref } from 'vue';
import { invokeCommand } from '../utils/ipc';

export function useSftpDirectory({ sessionIdRef, pathRef }) {
  const items = ref([]);
  const loading = ref(false);
  const error = ref('');
  let requestId = 0;

  const reset = () => {
    ++requestId;
    items.value = [];
    loading.value = false;
    error.value = '';
  };

  const load = async () => {
    if (!sessionIdRef.value) return false;
    const requestedSessionId = sessionIdRef.value;
    const requestedPath = pathRef.value;
    const currentRequestId = ++requestId;
    loading.value = true;
    error.value = '';
    try {
      const result = await invokeCommand('sftp_ls', {
        sessionId: requestedSessionId,
        path: requestedPath
      });
      if (
        currentRequestId !== requestId
        || sessionIdRef.value !== requestedSessionId
        || pathRef.value !== requestedPath
      ) return false;
      items.value = Array.isArray(result) ? result : [];
      return true;
    } catch (err) {
      if (currentRequestId === requestId) error.value = String(err);
      throw err;
    } finally {
      if (currentRequestId === requestId) loading.value = false;
    }
  };

  return {
    items,
    loading,
    error,
    reset,
    load
  };
}
