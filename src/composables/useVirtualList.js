import { computed, ref } from 'vue';

export function useVirtualList({ items, rowHeight = 34, overscan = 8 }) {
  const scrollTop = ref(0);
  const viewportHeight = ref(360);

  const totalHeight = computed(() => items.value.length * rowHeight);
  const visibleCount = computed(() => Math.ceil(viewportHeight.value / rowHeight));

  const startIndex = computed(() => {
    const raw = Math.floor(scrollTop.value / rowHeight) - overscan;
    return raw > 0 ? raw : 0;
  });

  const endIndex = computed(() => {
    const raw = startIndex.value + visibleCount.value + overscan * 2;
    return Math.min(items.value.length, raw);
  });

  const visibleItems = computed(() => items.value.slice(startIndex.value, endIndex.value));
  const translateY = computed(() => startIndex.value * rowHeight);

  const onScroll = (event) => {
    scrollTop.value = event.target.scrollTop;
  };

  const setViewportHeight = (height) => {
    viewportHeight.value = Math.max(120, Math.floor(height));
  };

  return {
    rowHeight,
    scrollTop,
    viewportHeight,
    totalHeight,
    startIndex,
    endIndex,
    visibleItems,
    translateY,
    onScroll,
    setViewportHeight
  };
}
