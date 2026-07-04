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

  const canLoadMore = computed(() => hasMore.value && !loading.value && !loadingMore.value);

  const loadFirstPage = async () => {
    if (!sessionIdRef.value) return;
    loading.value = true;
    error.value = '';
    try {
      const result = await invokeCommand('sftp_ls_paged', {
        sessionId: sessionIdRef.value,
        path: pathRef.value,
        offset: 0,
        limit: pageSize
      });
      items.value = result.items || [];
      offset.value = result.next_offset || items.value.length;
      hasMore.value = !!result.has_more;
      total.value = Number(result.total || items.value.length);
      totalKnown.value = !!result.total_known;
    } catch (err) {
      error.value = String(err);
      throw err;
    } finally {
      loading.value = false;
    }
  };

  const loadNextPage = async () => {
    if (!canLoadMore.value || !sessionIdRef.value) return;
    loadingMore.value = true;
    error.value = '';
    try {
      const result = await invokeCommand('sftp_ls_paged', {
        sessionId: sessionIdRef.value,
        path: pathRef.value,
        offset: offset.value,
        limit: pageSize
      });
      items.value = items.value.concat(result.items || []);
      offset.value = result.next_offset || items.value.length;
      hasMore.value = !!result.has_more;
      total.value = Number(result.total || items.value.length);
      totalKnown.value = !!result.total_known;
    } catch (err) {
      error.value = String(err);
      throw err;
    } finally {
      loadingMore.value = false;
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
