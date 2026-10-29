import assert from 'node:assert/strict';
import test from 'node:test';
import { createPinia, setActivePinia } from 'pinia';
import { useTransfersStore } from './transfers.js';

test('one transfer update preserves all unchanged row identities', () => {
  setActivePinia(createPinia());
  const store = useTransfersStore();
  store.createTask({ sessionId: 'demo', id: 'a', fileName: 'a.txt', direction: 'upload' });
  store.createTask({ sessionId: 'demo', id: 'b', fileName: 'b.txt', direction: 'upload' });
  const before = store.dockStatus.items;
  store.applyProgress({ sessionId: 'demo', id: 'a', status: 'uploading', current: 50, total: 100, percent: 50 });
  const after = store.dockStatus.items;
  assert.equal(after[0], before[0]);
  assert.notEqual(after[1], before[1]);
  assert.equal(after[1].loaded, 50);
  assert.equal(after[1].progress, 50);
});

test('real terminal state and cleanup remain independent of presentation animation', () => {
  setActivePinia(createPinia());
  const store = useTransfersStore();
  store.createTask({ sessionId: 'demo', id: 'done', fileName: 'empty.txt', direction: 'upload' });
  store.applyProgress({ sessionId: 'demo', id: 'done', status: 'finalizing', current: 0, total: 0, percent: 0 });
  assert.equal(store.activeCount, 1);
  store.applyProgress({ sessionId: 'demo', id: 'done', status: 'success', current: 0, total: 0, percent: 0 });
  assert.equal(store.activeCount, 0);
  assert.equal(store.dockStatus.items[0].status, 'success');
  assert.equal(store.dockStatus.items[0].progress, 0);
  store.clearFinishedTasks();
  assert.equal(store.dockStatus.items.length, 0);
});
