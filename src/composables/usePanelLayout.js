import { ref, watch } from 'vue';

const STORAGE_KEY = 'terminal-panel-layout-v1';
const MODE_KEY = 'terminal-layout-mode-v1';

export function usePanelLayout({ sshStore, activeKey, ensureSplitSession, visibleSessions }) {
  const splitTrees = ref({});
  const focusedLeaf = ref({});
  const tileTree = ref(null);
  const layoutMode = ref(localStorage.getItem(MODE_KEY) || 'tabs');
  const RATIO_EPSILON = 0.001;
  let isDraggingLayout = false;
  let hasPendingLayoutSave = false;
  let dragGestureActive = false;

  const notifyLayoutDragging = (dragging) => {
    window.dispatchEvent(new CustomEvent('terminal-layout-dragging', { detail: { dragging } }));
  };

  const createLeaf = (sessionId) => ({ type: 'leaf', sessionId });
  const createSplit = (direction, first, second) => ({
    type: 'split',
    direction,
    ratio: 0.5,
    first,
    second
  });

  const createTilePanel = (panelId) => ({
    id: `panel-${panelId}`,
    type: 'panel',
    panelId
  });

  const createTileSplit = (direction, first, second) => ({
    id: `split-${crypto.randomUUID()}`,
    type: 'split',
    direction,
    ratio: 0.5,
    first,
    second
  });

  const findFirstLeaf = (node) => {
    if (!node) return null;
    if (node.type === 'leaf') return node.sessionId;
    return findFirstLeaf(node.first) || findFirstLeaf(node.second);
  };

  const replaceLeaf = (node, targetId, replacement) => {
    if (!node) return null;
    if (node.type === 'leaf') {
      return node.sessionId === targetId ? replacement : node;
    }
    return {
      ...node,
      first: replaceLeaf(node.first, targetId, replacement),
      second: replaceLeaf(node.second, targetId, replacement)
    };
  };

  const removeLeaf = (node, targetId) => {
    if (!node) return null;
    if (node.type === 'leaf') {
      return node.sessionId === targetId ? null : node;
    }
    const first = removeLeaf(node.first, targetId);
    const second = removeLeaf(node.second, targetId);
    if (!first && !second) return null;
    if (!first) return second;
    if (!second) return first;
    return { ...node, first, second };
  };

  const getLeafIds = (node, acc = []) => {
    if (!node) return acc;
    if (node.type === 'leaf') {
      acc.push(node.sessionId);
      return acc;
    }
    getLeafIds(node.first, acc);
    getLeafIds(node.second, acc);
    return acc;
  };

  const ensureTree = (panelId) => {
    if (!panelId) return null;
    if (!splitTrees.value[panelId]) {
      splitTrees.value[panelId] = createLeaf(panelId);
    }
    return splitTrees.value[panelId];
  };

  const buildTileTree = (panelIds, depth = 0) => {
    if (!panelIds.length) return null;
    if (panelIds.length === 1) return createTilePanel(panelIds[0]);
    const mid = Math.ceil(panelIds.length / 2);
    const direction = depth % 2 === 0 ? 'vertical' : 'horizontal';
    const first = buildTileTree(panelIds.slice(0, mid), depth + 1);
    const second = buildTileTree(panelIds.slice(mid), depth + 1);
    return createTileSplit(direction, first, second);
  };

  const collectTilePanels = (node, acc = []) => {
    if (!node) return acc;
    if (node.type === 'panel') {
      acc.push(node.panelId);
      return acc;
    }
    collectTilePanels(node.first, acc);
    collectTilePanels(node.second, acc);
    return acc;
  };

  const collectTileRects = (node, rect, acc = []) => {
    if (!node) return acc;
    if (node.type === 'panel') {
      acc.push({
        panelId: node.panelId,
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        centerX: rect.x + rect.width / 2,
        centerY: rect.y + rect.height / 2
      });
      return acc;
    }

    const ratio = Number.isFinite(node.ratio) ? node.ratio : 0.5;
    if (node.direction === 'vertical') {
      const firstWidth = rect.width * ratio;
      const secondWidth = rect.width - firstWidth;
      collectTileRects(node.first, { x: rect.x, y: rect.y, width: firstWidth, height: rect.height }, acc);
      collectTileRects(node.second, { x: rect.x + firstWidth, y: rect.y, width: secondWidth, height: rect.height }, acc);
      return acc;
    }

    const firstHeight = rect.height * ratio;
    const secondHeight = rect.height - firstHeight;
    collectTileRects(node.first, { x: rect.x, y: rect.y, width: rect.width, height: firstHeight }, acc);
    collectTileRects(node.second, { x: rect.x, y: rect.y + firstHeight, width: rect.width, height: secondHeight }, acc);
    return acc;
  };

  const getTileRects = () => collectTileRects(tileTree.value, { x: 0, y: 0, width: 1, height: 1 }, []);

  const findNeighborTilePanel = (direction) => {
    const currentId = activeKey.value;
    if (!currentId) return null;

    const rects = getTileRects();
    const current = rects.find((rect) => rect.panelId === currentId);
    if (!current) return null;

    const candidates = rects.filter((rect) => {
      if (rect.panelId === currentId) return false;
      if (direction === 'left') return rect.centerX < current.centerX - 0.001;
      if (direction === 'right') return rect.centerX > current.centerX + 0.001;
      if (direction === 'up') return rect.centerY < current.centerY - 0.001;
      if (direction === 'down') return rect.centerY > current.centerY + 0.001;
      return false;
    });

    if (!candidates.length) return null;

    const scored = candidates.map((candidate) => {
      const dx = Math.abs(candidate.centerX - current.centerX);
      const dy = Math.abs(candidate.centerY - current.centerY);
      const primary = direction === 'left' || direction === 'right' ? dx : dy;
      const secondary = direction === 'left' || direction === 'right' ? dy : dx;
      return {
        panelId: candidate.panelId,
        score: primary * 10 + secondary
      };
    });

    scored.sort((a, b) => a.score - b.score);
    return scored[0]?.panelId || null;
  };

  const focusTileByDirection = (direction) => {
    if (layoutMode.value !== 'tile') return false;
    const targetId = findNeighborTilePanel(direction);
    if (!targetId) return false;
    activeKey.value = targetId;
    return true;
  };

  const syncTileTree = () => {
    const panelIds = (visibleSessions?.value || []).map((panel) => panel.id);
    if (!panelIds.length) {
      tileTree.value = null;
      return;
    }

    const existing = collectTilePanels(tileTree.value, []);
    const stale = existing.some((id) => !panelIds.includes(id));
    const missing = panelIds.some((id) => !existing.includes(id));

    if (!tileTree.value || stale || missing) {
      tileTree.value = buildTileTree(panelIds);
    }
  };

  const setFocused = (panelId, sessionId) => {
    focusedLeaf.value[panelId] = sessionId;
  };

  const splitActive = async (direction) => {
    if (!activeKey.value) return;
    const panelId = activeKey.value;
    const tree = ensureTree(panelId);
    const targetId = focusedLeaf.value[panelId] || findFirstLeaf(tree) || panelId;
    const splitId = await ensureSplitSession(targetId);
    if (!splitId) return;

    const newNode = createSplit(direction, createLeaf(targetId), createLeaf(splitId));
    splitTrees.value[panelId] = replaceLeaf(tree, targetId, newNode);
    setFocused(panelId, splitId);
  };

  const mergeToSingle = (panelId) => {
    const tree = ensureTree(panelId);
    const leafIds = getLeafIds(tree);
    leafIds.forEach((id) => {
      if (id !== panelId) {
        sshStore.removeSession(id);
      }
    });
    splitTrees.value[panelId] = createLeaf(panelId);
    setFocused(panelId, panelId);
  };

  const closeCurrentPanel = () => {
    if (!activeKey.value) return;
    const panelId = activeKey.value;
    const tree = ensureTree(panelId);
    const targetId = focusedLeaf.value[panelId] || findFirstLeaf(tree);
    if (!targetId) return;

    const leafIds = getLeafIds(tree);
    if (leafIds.length <= 1) return;

    if (targetId === panelId) {
      const siblingId = leafIds.find((id) => id !== panelId);
      if (!siblingId) return;

      const sibling = sshStore.getSession(siblingId);
      if (sibling) {
        sibling.isSplitChild = false;
        delete sibling.parentId;
      }
      sshStore.removeSession(panelId);

      const newTree = removeLeaf(tree, targetId) || createLeaf(siblingId);
      splitTrees.value[siblingId] = newTree;
      delete splitTrees.value[panelId];
      delete focusedLeaf.value[panelId];
      setFocused(siblingId, siblingId);
      activeKey.value = siblingId;
      return;
    }

    splitTrees.value[panelId] = removeLeaf(tree, targetId) || createLeaf(panelId);
    sshStore.removeSession(targetId);
    const remaining = getLeafIds(splitTrees.value[panelId]);
    setFocused(panelId, remaining[0] || panelId);
  };

  const cleanupSplitTrees = () => {
    const ids = new Set(sshStore.sessions.map((session) => session.id));
    Object.keys(splitTrees.value).forEach((panelId) => {
      if (!ids.has(panelId)) {
        const tree = splitTrees.value[panelId];
        getLeafIds(tree).forEach((id) => {
          if (ids.has(id)) sshStore.removeSession(id);
        });
        delete splitTrees.value[panelId];
        delete focusedLeaf.value[panelId];
      }
    });
  };

  const removePanelRoot = (panelId) => {
    const tree = splitTrees.value[panelId];
    if (tree) {
      getLeafIds(tree).forEach((id) => {
        if (id !== panelId) sshStore.removeSession(id);
      });
      delete splitTrees.value[panelId];
      delete focusedLeaf.value[panelId];
    }
    sshStore.removeSession(panelId);
    cleanupSplitTrees();
    syncTileTree();
  };

  const startSplitDrag = (e, node) => {
    e.preventDefault();
    e.stopPropagation();
    const container = e.currentTarget?.parentElement;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const isVertical = node.direction === 'vertical';
    dragGestureActive = true;
    notifyLayoutDragging(true);
    let rafId = null;
    let pendingRatio = node.ratio;

    const onMove = (evt) => {
      const clientPos = isVertical ? evt.clientX : evt.clientY;
      const offset = isVertical ? rect.left : rect.top;
      const size = isVertical ? rect.width : rect.height;
      if (size === 0) return;

      const pos = (clientPos - offset) / size;
      pendingRatio = Math.min(0.85, Math.max(0.15, pos));

      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          if (Math.abs(node.ratio - pendingRatio) > RATIO_EPSILON) {
            isDraggingLayout = true;
            node.ratio = pendingRatio;
          }
          rafId = null;
        });
      }
    };

    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';

      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }

      if (Math.abs(node.ratio - pendingRatio) > RATIO_EPSILON) {
        isDraggingLayout = true;
        node.ratio = pendingRatio;
      }

      if (isDraggingLayout || hasPendingLayoutSave) {
        isDraggingLayout = false;
        hasPendingLayoutSave = false;
        saveLayoutSnapshot();
      }

      if (dragGestureActive) {
        dragGestureActive = false;
        notifyLayoutDragging(false);
      }

      window.dispatchEvent(new CustomEvent('terminal-layout-resize'));
    };

    document.body.style.cursor = isVertical ? 'col-resize' : 'row-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

  const startTileSplitDrag = (e, node) => {
    e.preventDefault();
    e.stopPropagation();
    const container = e.currentTarget?.parentElement;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const isVertical = node.direction === 'vertical';
    dragGestureActive = true;
    notifyLayoutDragging(true);
    let rafId = null;
    let pendingRatio = node.ratio;

    const onMove = (evt) => {
      const clientPos = isVertical ? evt.clientX : evt.clientY;
      const offset = isVertical ? rect.left : rect.top;
      const size = isVertical ? rect.width : rect.height;
      if (size === 0) return;

      const pos = (clientPos - offset) / size;
      pendingRatio = Math.min(0.85, Math.max(0.15, pos));

      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          if (Math.abs(node.ratio - pendingRatio) > RATIO_EPSILON) {
            isDraggingLayout = true;
            node.ratio = pendingRatio;
          }
          rafId = null;
        });
      }
    };

    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';

      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }

      if (Math.abs(node.ratio - pendingRatio) > RATIO_EPSILON) {
        isDraggingLayout = true;
        node.ratio = pendingRatio;
      }

      if (isDraggingLayout || hasPendingLayoutSave) {
        isDraggingLayout = false;
        hasPendingLayoutSave = false;
        saveLayoutSnapshot();
      }

      if (dragGestureActive) {
        dragGestureActive = false;
        notifyLayoutDragging(false);
      }

      window.dispatchEvent(new CustomEvent('terminal-layout-resize'));
    };

    document.body.style.cursor = isVertical ? 'col-resize' : 'row-resize';
    document.body.style.userSelect = 'none';
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

  const loadLayoutSnapshot = () => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      splitTrees.value = parsed?.splitTrees || {};
      focusedLeaf.value = parsed?.focusedLeaf || {};
      tileTree.value = parsed?.tileTree || null;
    } catch {
      splitTrees.value = {};
      focusedLeaf.value = {};
      tileTree.value = null;
    }
  };

  const saveLayoutSnapshot = () => {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          splitTrees: splitTrees.value,
          focusedLeaf: focusedLeaf.value,
          tileTree: tileTree.value
        })
      );
    } catch {
      return;
    }
  };

  const setLayoutMode = (mode) => {
    layoutMode.value = mode === 'tile' ? 'tile' : 'tabs';
  };

  loadLayoutSnapshot();
  syncTileTree();

  watch(
    () => sshStore.sessions.length,
    () => {
      cleanupSplitTrees();
      syncTileTree();
    }
  );

  watch(
    () => visibleSessions?.value?.map((panel) => panel.id).join('|'),
    () => {
      syncTileTree();
    }
  );

  watch(layoutMode, (mode) => {
    localStorage.setItem(MODE_KEY, mode);
  });

  watch(
    [splitTrees, focusedLeaf, tileTree],
    () => {
      if (isDraggingLayout) {
        hasPendingLayoutSave = true;
        return;
      }
      saveLayoutSnapshot();
    },
    { deep: true }
  );

  return {
    splitTrees,
    focusedLeaf,
    tileTree,
    layoutMode,
    setLayoutMode,
    ensureTree,
    setFocused,
    splitActive,
    mergeToSingle,
    closeCurrentPanel,
    removePanelRoot,
    cleanupSplitTrees,
    startSplitDrag,
    startTileSplitDrag,
    focusTileByDirection
  };
}
