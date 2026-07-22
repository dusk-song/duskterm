import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { createPinia, setActivePinia } from 'pinia';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

async function loadTransferStore() {
  try {
    return await import('./sftpTransfers.js');
  } catch (error) {
    assert.fail(`全局 SFTP 传输 Store 尚未实现: ${error}`);
  }
}

test('transfer progress is isolated by session id and request id', async () => {
  const { useSftpTransfersStore } = await loadTransferStore();
  setActivePinia(createPinia());
  const store = useSftpTransfersStore();

  const first = store.createTask({
    id: 'same-request',
    sessionId: 'session-a',
    direction: 'upload',
    fileName: 'a.bin',
    localPath: 'C:\\a.bin',
    remotePath: '/a.bin',
  });
  const second = store.createTask({
    id: 'same-request',
    sessionId: 'session-b',
    direction: 'download',
    fileName: 'b.bin',
    localPath: 'C:\\b.bin',
    remotePath: '/b.bin',
  });

  store.applyProgress({
    id: 'same-request',
    sessionId: 'session-a',
    direction: 'upload',
    current: 50,
    total: 100,
    percent: 50,
    status: 'uploading',
  });

  assert.equal(first.current, 50);
  assert.equal(first.status, 'uploading');
  assert.equal(second.current, 0);
  assert.equal(second.status, 'waiting');
});

test('removing one session task preserves the other session task with the same request id', async () => {
  const { useSftpTransfersStore } = await loadTransferStore();
  setActivePinia(createPinia());
  const store = useSftpTransfersStore();

  store.createTask({ id: 'request-1', sessionId: 'session-a', direction: 'upload', fileName: 'a' });
  store.createTask({ id: 'request-1', sessionId: 'session-b', direction: 'upload', fileName: 'b' });

  store.removeTask('session-a', 'request-1');

  assert.equal(store.findTask('session-a', 'request-1'), undefined);
  assert.equal(store.findTask('session-b', 'request-1')?.fileName, 'b');
});

test('SFTP panel navigation does not disconnect the previous session', () => {
  const source = read('../components/sftp/SftpFileManager.vue');
  const sessionWatcher = source.slice(
    source.indexOf('watch(() => props.sessionId'),
    source.indexOf('watch(() => props.visible'),
  );
  const unmount = source.slice(source.indexOf('onUnmounted(() =>'));

  assert.doesNotMatch(sessionWatcher, /disconnectSftpSession\(prevSessionId\)/);
  assert.doesNotMatch(unmount, /disconnectSftpSession\(sessionId\)/);
});

test('SFTP progress is owned by the application store instead of the panel component', () => {
  const app = read('../App.vue');
  const manager = read('../components/sftp/SftpFileManager.vue');
  const dock = read('../components/app-shell/TransferDock.vue');
  const backend = read('../../src-tauri/src/sftp/mod.rs');

  assert.match(app, /useSftpTransfersStore/);
  assert.match(app, /listenEvent\('sftp-progress',[\s\S]*applyProgress/);
  assert.match(manager, /useSftpTransfersStore/);
  assert.doesNotMatch(manager, /listenEvent\('sftp-progress'/);
  assert.match(dock, /useSftpTransfersStore/);
  assert.doesNotMatch(dock, /sftp-transfer-status/);
  assert.match(backend, /#\[serde\(rename = "sessionId"\)\]\s*session_id: String/);
});

test('session actor registers cancellation before spawning a transfer task', () => {
  const actor = read('../../src-tauri/src/session/actor.rs');
  for (const message of ['SessionMessage::StartSftpDownload', 'SessionMessage::StartSftpUpload']) {
    const start = actor.indexOf(message);
    const spawn = actor.indexOf('tokio::spawn(async move', start);
    const beforeSpawn = actor.slice(start, spawn);
    assert.match(beforeSpawn, /register_cancel_token\(&req_id\)/, `${message} must register before spawn`);
  }
});

test('waiting transfers cancel locally while active transfers request backend cancellation', async () => {
  const { useSftpTransfersStore } = await loadTransferStore();
  setActivePinia(createPinia());
  const store = useSftpTransfersStore();

  const waiting = store.createTask({ id: 'waiting', sessionId: 'session-a', direction: 'upload', fileName: 'a' });
  const active = store.createTask({ id: 'active', sessionId: 'session-a', direction: 'download', fileName: 'b' });
  active.status = 'uploading';

  assert.equal(store.requestCancel('session-a', 'waiting'), 'local');
  assert.equal(waiting.status, 'cancelled');
  assert.equal(store.requestCancel('session-a', 'active'), 'remote');
  assert.equal(active.status, 'cancelling');
  store.applyProgress({
    sessionId: 'session-a',
    id: 'active',
    direction: 'download',
    status: 'uploading',
    current: 10,
    total: 100,
    percent: 10,
  });
  assert.equal(active.status, 'cancelling');
});

test('session actor retains transfer tasks and drains them during disconnect', () => {
  const state = read('../../src-tauri/src/session/state.rs');
  const actor = read('../../src-tauri/src/session/actor.rs');

  assert.match(state, /transfer_tasks:\s*HashMap<String,\s*tokio::task::JoinHandle<\(\)>>/);
  assert.match(actor, /runtime_state\.transfer_tasks\.insert\(transfer_req_id, task\)/);
  assert.match(actor, /drain_transfer_tasks\(&mut runtime_state\)\.await/);
});
