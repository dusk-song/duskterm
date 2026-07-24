import { computed, ref } from 'vue';

export function useVirtualList({ items, rowHeight = 34, overscan = 8 }) {
  const scrollTop = ref(0);
  const viewportHeight = ref(360);

  const totalHeight = computed(() => items.value.length * rowHeight);
  const visibleCount = computed(() => Math.ceil(viewportHeight.value / rowHeight));
  const maxScrollTop = computed(() => Math.max(0, totalHeight.value - viewportHeight.value));
  const effectiveScrollTop = computed(() => Math.min(scrollTop.value, maxScrollTop.value));

  const startIndex = computed(() => {
    const raw = Math.floor(effectiveScrollTop.value / rowHeight) - overscan;
    return raw > 0 ? raw : 0;
  });

  const endIndex = computed(() => {
    const raw = startIndex.value + visibleCount.value + overscan * 2;
    return Math.min(items.value.length, raw);
  });

  const visibleItems = computed(() => items.value.slice(startIndex.value, endIndex.value));
  const translateY = computed(() => startIndex.value * rowHeight);

  const setScrollTop = (value) => {
    const normalized = Number.isFinite(Number(value)) ? Math.max(0, Number(value)) : 0;
    scrollTop.value = Math.min(normalized, maxScrollTop.value);
    return scrollTop.value;
  };

  const resetScroll = () => setScrollTop(0);
  const clampScroll = () => setScrollTop(scrollTop.value);

  const onScroll = (event) => {
    setScrollTop(event.target.scrollTop);
  };

  const setViewportHeight = (height) => {
    viewportHeight.value = Math.max(120, Math.floor(height));
    clampScroll();
  };

  return {
    rowHeight,
    scrollTop,
    viewportHeight,
    totalHeight,
    maxScrollTop,
    startIndex,
    endIndex,
    visibleItems,
    translateY,
    onScroll,
    setScrollTop,
    resetScroll,
    clampScroll,
    setViewportHeight
  };
}
