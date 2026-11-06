import { getOrCreateWorkerPoolSingleton } from '@pierre/diffs/worker';
import DiffsWorker from '@pierre/diffs/worker/worker.js?worker';
import { setStreamDiffsWorkerPool, clearStreamDiffsWorkerPool } from 'markstream-vue';

let initialized = false;
export function initializeAiHighlight() {
  if (initialized || typeof Worker === 'undefined') return;
  initialized = true;
  try {
    setStreamDiffsWorkerPool(getOrCreateWorkerPoolSingleton({
      poolOptions: {
        poolSize: Math.max(1, Math.min(2, Math.floor((navigator.hardwareConcurrency || 2) / 2))),
        workerFactory: () => new DiffsWorker(),
      },
      highlighterOptions: { preferredHighlighter: 'shiki-js', theme: { light: 'vitesse-light', dark: 'vitesse-dark' } },
    }));
  } catch {
    // The renderer keeps its plain-code fallback if a WebView cannot start workers.
    clearStreamDiffsWorkerPool();
  }
}
