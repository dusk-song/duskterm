import { computed, ref } from 'vue';

export function useFileSelection(itemsRef) {
  const selectedKeys = ref(new Set());
  const anchorIndex = ref(-1);

  const selectedList = computed(() => {
    const keys = selectedKeys.value;
    return itemsRef.value.filter(item => keys.has(item.name));
  });

  const isSelected = (item) => selectedKeys.value.has(item.name);

  const clearSelection = () => {
    selectedKeys.value = new Set();
    anchorIndex.value = -1;
  };

  const selectSingle = (item, index) => {
    const next = new Set();
    next.add(item.name);
    selectedKeys.value = next;
    anchorIndex.value = index;
  };

  const toggleSelect = (item, index) => {
    const next = new Set(selectedKeys.value);
    if (next.has(item.name)) {
      next.delete(item.name);
    } else {
      next.add(item.name);
    }
    selectedKeys.value = next;
    anchorIndex.value = index;
  };

  const selectRange = (index) => {
    const start = anchorIndex.value >= 0 ? anchorIndex.value : index;
    const [left, right] = start < index ? [start, index] : [index, start];
    const next = new Set(selectedKeys.value);
    for (let i = left; i <= right; i += 1) {
      const item = itemsRef.value[i];
      if (item) next.add(item.name);
    }
    selectedKeys.value = next;
  };

  const handleRowSelect = (item, index, event) => {
    if (event.shiftKey) {
      selectRange(index);
      return;
    }
    if (event.ctrlKey || event.metaKey) {
      toggleSelect(item, index);
      return;
    }
    selectSingle(item, index);
  };

  return {
    selectedKeys,
    selectedList,
    isSelected,
    clearSelection,
    selectSingle,
    handleRowSelect
  };
}
