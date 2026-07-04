import { computed, onMounted, onUnmounted, ref, unref } from 'vue';

const STACK_BREAKPOINT = 800;
const HANDLE_SIZE = 4;
const SESSION_MIN_WIDTH = 260;
const FILE_MIN_WIDTH = 320;
const MAIN_MIN_WIDTH = 520;
const SESSION_DEFAULT_WIDTH = 320;
const FILE_DEFAULT_WIDTH = 400;

export function useTiledWorkspaceLayout({ showSessionPanel, showSftpPanel }) {
  const workspaceRef = ref(null);
  const workspaceWidth = ref(0);
  const isStacked = ref(false);
  const sessionPanelWidth = ref(SESSION_DEFAULT_WIDTH);
  const filePanelWidth = ref(FILE_DEFAULT_WIDTH);
  const activeResizeHandle = ref(null);
  let resizeObserver = null;
  let resizeFrame = 0;
  let dragState = null;

  const sessionVisible = computed(() => Boolean(unref(showSessionPanel)));
  const fileVisible = computed(() => Boolean(unref(showSftpPanel)));

  const getWorkspaceWidth = () => {
    const element = workspaceRef.value;
    if (!element) return 0;
    return Math.max(0, element.getBoundingClientRect().width);
  };

  const clampWidths = () => {
    const available = workspaceWidth.value || getWorkspaceWidth();
    if (available <= 0) return;

    const handleSpace = isStacked.value ? 0 : HANDLE_SIZE * 2;
    const usable = Math.max(0, available - handleSpace);
    const maxSession = Math.max(SESSION_MIN_WIDTH, usable - FILE_MIN_WIDTH - MAIN_MIN_WIDTH);
    const maxFile = Math.max(FILE_MIN_WIDTH, usable - SESSION_MIN_WIDTH - MAIN_MIN_WIDTH);

    sessionPanelWidth.value = Math.min(maxSession, Math.max(SESSION_MIN_WIDTH, sessionPanelWidth.value));
    filePanelWidth.value = Math.min(maxFile, Math.max(FILE_MIN_WIDTH, filePanelWidth.value));
  };

  const measureWorkspace = () => {
    const width = getWorkspaceWidth();
    workspaceWidth.value = width;
    isStacked.value = width < STACK_BREAKPOINT;
    clampWidths();
  };

  const startResize = (handle, event) => {
    if (isStacked.value) return;
    if (handle !== 'session' && handle !== 'file') return;

    const element = workspaceRef.value;
    if (!element) return;

    const rect = element.getBoundingClientRect();
    const startX = event.clientX;
    const startSession = sessionPanelWidth.value;
    const startFile = filePanelWidth.value;
    activeResizeHandle.value = handle;
    dragState = {
      rect,
      startX,
      startSession,
      startFile,
      frameId: 0,
      latestX: startX
    };

    event.preventDefault();
    event.stopPropagation();
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    // Add a class to the workspace to hint the browser about upcoming changes
    try {
      element.classList.add('resizing');
    } catch (e) {
      /* ignore */
    }

    const getBounds = () => {
      const usable = Math.max(0, rect.width - HANDLE_SIZE * 2);
      return {
        sessionMax: Math.max(SESSION_MIN_WIDTH, usable - FILE_MIN_WIDTH - MAIN_MIN_WIDTH),
        fileMax: Math.max(FILE_MIN_WIDTH, usable - SESSION_MIN_WIDTH - MAIN_MIN_WIDTH)
      };
    };

    const applyDrag = () => {
      if (!dragState) return;
      const delta = dragState.latestX - dragState.startX;
      const { sessionMax, fileMax } = getBounds();

      // Compute target widths but avoid writing to reactive refs on every frame.
      const targetSession = handle === 'session'
        ? Math.min(sessionMax, Math.max(SESSION_MIN_WIDTH, dragState.startSession + delta))
        : sessionPanelWidth.value;
      const targetFile = handle === 'file'
        ? Math.min(fileMax, Math.max(FILE_MIN_WIDTH, dragState.startFile + delta))
        : filePanelWidth.value;

      // If workspace element exists, write inline style directly to avoid Vue re-render cost.
      const el = workspaceRef.value;
      if (el) {
        // Build the gridTemplateColumns string similar to workspaceGridStyle computed result.
        if (!isStacked.value && sessionVisible.value && fileVisible.value) {
          el.style.gridTemplateColumns = `${Math.round(targetSession)}px ${HANDLE_SIZE}px ${Math.round(targetFile)}px ${HANDLE_SIZE}px minmax(${MAIN_MIN_WIDTH}px, 1fr)`;
        } else if (!isStacked.value && sessionVisible.value) {
          el.style.gridTemplateColumns = `${Math.round(targetSession)}px ${HANDLE_SIZE}px minmax(0, 1fr)`;
        } else if (!isStacked.value && fileVisible.value) {
          el.style.gridTemplateColumns = `minmax(0, 1fr) ${HANDLE_SIZE}px ${Math.round(targetFile)}px`;
        }
      }

      // Save last computed values on dragState so we can commit them on mouseup.
      dragState.computedSession = targetSession;
      dragState.computedFile = targetFile;
    };

    const scheduleApply = (clientX) => {
      if (!dragState) return;
      dragState.latestX = clientX;
      if (dragState.frameId) return;
      dragState.frameId = window.requestAnimationFrame(() => {
        dragState.frameId = 0;
        applyDrag();
      });
    };

    const onMove = (moveEvent) => {
      scheduleApply(moveEvent.clientX);
    };

    const onUp = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      if (dragState?.frameId) {
        window.cancelAnimationFrame(dragState.frameId);
      }

      // Commit final sizes into reactive state and clear inline styles
      const el = workspaceRef.value;
      const finalSession = dragState?.computedSession ?? sessionPanelWidth.value;
      const finalFile = dragState?.computedFile ?? filePanelWidth.value;
      if (typeof finalSession === 'number') sessionPanelWidth.value = Math.round(finalSession);
      if (typeof finalFile === 'number') filePanelWidth.value = Math.round(finalFile);
      if (el) {
        el.style.gridTemplateColumns = '';
        try { el.classList.remove('resizing'); } catch (e) { /* ignore */ }
      }

      dragState = null;
      activeResizeHandle.value = null;
      clampWidths();
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  };

  const workspaceGridStyle = computed(() => {
    if (isStacked.value) {
      const rows = [];
      if (sessionVisible.value) rows.push('auto');
      if (fileVisible.value) rows.push('auto');
      rows.push('minmax(0, 1fr)');
      return {
        gridTemplateColumns: '1fr',
        gridTemplateRows: rows.join(' ')
      };
    }

    if (sessionVisible.value && fileVisible.value) {
      return {
        gridTemplateColumns: `${sessionPanelWidth.value}px ${HANDLE_SIZE}px ${filePanelWidth.value}px ${HANDLE_SIZE}px minmax(${MAIN_MIN_WIDTH}px, 1fr)`,
        gridTemplateRows: '1fr'
      };
    }

    if (sessionVisible.value) {
      return {
        gridTemplateColumns: `${sessionPanelWidth.value}px ${HANDLE_SIZE}px minmax(0, 1fr)`,
        gridTemplateRows: '1fr'
      };
    }

    if (fileVisible.value) {
      return {
        gridTemplateColumns: `minmax(0, 1fr) ${HANDLE_SIZE}px ${filePanelWidth.value}px`,
        gridTemplateRows: '1fr'
      };
    }

    return {
      gridTemplateColumns: 'minmax(0, 1fr)',
      gridTemplateRows: '1fr'
    };
  });

  onMounted(() => {
    measureWorkspace();
    resizeObserver = new ResizeObserver(() => {
      if (resizeFrame) return;
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = 0;
        measureWorkspace();
      });
    });
    if (workspaceRef.value) {
      resizeObserver.observe(workspaceRef.value);
    }
    window.addEventListener('resize', measureWorkspace);
  });

  onUnmounted(() => {
    if (resizeObserver && workspaceRef.value) {
      resizeObserver.unobserve(workspaceRef.value);
    }
    if (resizeFrame) window.cancelAnimationFrame(resizeFrame);
    window.removeEventListener('resize', measureWorkspace);
  });

  return {
    workspaceRef,
    workspaceWidth,
    isStacked,
    sessionPanelWidth,
    filePanelWidth,
    activeResizeHandle,
    workspaceGridStyle,
    startResize,
    clampWidths
  };
}
