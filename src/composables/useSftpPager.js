import { computed, ref } from 'vue';
import { invokeCommand } from '../utils/ipc';

export function useSftpPager({ sessionIdRef, pathRef, pageSize = 200 }) {
  const items = ref([]);
  const loading = ref(false);
  const loadingMore = ref(false);
  const offset = ref(0);
  const hasMore = ref(true);
  const total = ref(0);
  const totalKnown = ref(false);
  const error = ref('');
  let firstPageRequestId = 0;
  let nextPageRequestId = 0;

  const canLoadMore = computed(() => hasMore.value && !loading.value && !loadingMore.value);

  const loadFirstPage = async () => {
    if (!sessionIdRef.value) return;
    const requestedSessionId = sessionIdRef.value;
    const requestedPath = pathRef.value;
    const requestId = ++firstPageRequestId;
    loading.value = true;
    error.value = '';
    try {
      const result = await invokeCommand('sftp_ls_paged', {
        sessionId: requestedSessionId,
        path: requestedPath,
        offset: 0,
        limit: pageSize
      });
      if (requestId !== firstPageRequestId || sessionIdRef.value !== requestedSessionId || pathRef.value !== requestedPath) return false;
      items.value = result.items || [];
      offset.value = result.next_offset || items.value.length;
      hasMore.value = !!result.has_more;
      total.value = Number(result.total || items.value.length);
      totalKnown.value = !!result.total_known;
      return true;
    } catch (err) {
      if (requestId === firstPageRequestId) error.value = String(err);
      throw err;
    } finally {
      if (requestId === firstPageRequestId) loading.value = false;
    }
  };

  const loadNextPage = async () => {
    if (!canLoadMore.value || !sessionIdRef.value) return;
    const requestedSessionId = sessionIdRef.value;
    const requestedPath = pathRef.value;
    const requestedOffset = offset.value;
    const requestId = ++nextPageRequestId;
    loadingMore.value = true;
    error.value = '';
    try {
      const result = await invokeCommand('sftp_ls_paged', {
        sessionId: requestedSessionId,
        path: requestedPath,
        offset: requestedOffset,
        limit: pageSize
      });
      if (requestId !== nextPageRequestId || sessionIdRef.value !== requestedSessionId || pathRef.value !== requestedPath || offset.value !== requestedOffset) return false;
      items.value = items.value.concat(result.items || []);
      offset.value = result.next_offset || items.value.length;
      hasMore.value = !!result.has_more;
      total.value = Number(result.total || items.value.length);
      totalKnown.value = !!result.total_known;
      return true;
    } catch (err) {
      if (requestId === nextPageRequestId) error.value = String(err);
      throw err;
    } finally {
      if (requestId === nextPageRequestId) loadingMore.value = false;
    }
  };

  return {
    items,
    loading,
    loadingMore,
    offset,
    total,
    totalKnown,
    hasMore,
    error,
    canLoadMore,
    loadFirstPage,
    loadNextPage
  };
}
