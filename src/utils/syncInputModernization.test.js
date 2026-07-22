import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const historyModule = await import('./terminalCommandHistory.js').catch(() => ({}));
const overviewModule = await import('./sessionOverview.js').catch(() => ({}));
const syncChannelModule = await import('./syncInputChannels.js').catch(() => ({}));

test('terminal line replacement clears the existing line for every synchronized target', () => {
  assert.equal(typeof historyModule.buildTerminalLineReplacementPayload, 'function');
  assert.equal(historyModule.buildTerminalLineReplacementPayload('cd /srv/app'), '\u0015cd /srv/app');
});

test('history matches only expose commands ordered by count then recency', () => {
  assert.equal(typeof historyModule.findCommandHistoryMatches, 'function');
  const matches = historyModule.findCommandHistoryMatches([
    { cmd: 'cd /data/old', count: 4 },
    { cmd: 'cd /data/nginx', count: 7 },
    { cmd: 'cd /data/current', count: 7 },
  ], 'cd /data');

  assert.deepEqual(matches.map((entry) => entry.cmd), [
    'cd /data/current',
    'cd /data/nginx',
    'cd /data/old',
  ]);
});

test('history recording increments, moves to most recent, and preserves completed tab paths', () => {
  assert.equal(typeof historyModule.recordCommandHistoryEntry, 'function');
  assert.equal(typeof historyModule.extractCommandFromTerminalLine, 'function');

  const command = historyModule.extractCommandFromTerminalLine(
    '[prod] ➜  cd /data/nginx-1.30.3/conf/',
    'cd /datangconf',
  );
  assert.equal(command, 'cd /data/nginx-1.30.3/conf/');

  const history = historyModule.recordCommandHistoryEntry([
    { cmd: command, count: 1 },
    { cmd: 'systemctl status nginx', count: 2 },
  ], command);
  assert.deepEqual(history.at(-1), { cmd: command, count: 2 });
});

test('session overview collapses channel members into one selectable card', () => {
  assert.equal(typeof overviewModule.buildSessionOverviewItems, 'function');
  const sessions = [
    { id: 'a', status: 'connected', name: 'A' },
    { id: 'b', status: 'connected', name: 'B' },
    { id: 'c', status: 'disconnected', name: 'C' },
  ];
  const channels = [{
    id: 'channel-1',
    name: '生产频道',
    sourceMode: 'primary',
    primarySessionId: 'b',
    sendMode: 'realtime',
    sessionIds: ['a', 'b'],
  }];

  const items = overviewModule.buildSessionOverviewItems(sessions, channels, 'c');
  assert.equal(items.length, 2);
  assert.equal(items[0].type, 'channel');
  assert.deepEqual(items[0].sessions.map((session) => session.id), ['a', 'b']);
  assert.equal(items[0].selectSessionId, 'b');
  assert.equal(items[1].selectSessionId, 'c');
});

test('sync input resolves root and split shell channel IPC targets correctly', () => {
  assert.equal(typeof syncChannelModule.buildSyncInputWriteRequest, 'function');
  assert.deepEqual(
    syncChannelModule.buildSyncInputWriteRequest({ id: 'root' }, 'ls'),
    { command: 'write_ssh', args: { sessionId: 'root', data: 'ls' } },
  );
  assert.deepEqual(
    syncChannelModule.buildSyncInputWriteRequest({
      id: 'child',
      isSplitChild: true,
      workspaceSessionId: 'root',
    }, '\u0015ls'),
    {
      command: 'write_ssh_shell_channel',
      args: { rootSessionId: 'root', channelId: 'child', data: '\u0015ls' },
    },
  );
});

test('automatic merged view is removed while sync routing uses modern russh writes', () => {
  const app = read('../App.vue');
  const modal = read('../components/terminal/SyncInputModal.vue');
  const preferences = read('./preferences.js');
  const settings = read('../components/settings/SettingsModal.vue');
  const inputRouter = read('../composables/useInputRouter.js');
  const terminal = read('../components/terminal/Terminal.vue');
  const ssh = read('../../src-tauri/src/ssh/mod.rs');

  assert.doesNotMatch(app, /SyncMerged|syncMerged|syncInputAutoMerge|SYNC_INPUT_AUTO_MERGE/);
  assert.doesNotMatch(modal, /autoMerge|auto-merge|syncInputAutoMerge|自动合并视图/);
  assert.doesNotMatch(preferences, /syncMerged/);
  assert.doesNotMatch(settings, /syncMerged|合并视图/);
  assert.match(inputRouter, /buildSyncInputWriteRequest/);
  assert.match(terminal, /buildTerminalLineReplacementPayload/);
  assert.match(ssh, /channel\.data_bytes\(data\)\.await/);
});

test('history suggestions render as a single command-only line', () => {
  const terminal = read('../components/terminal/Terminal.vue');
  assert.match(terminal, /findCommandHistoryMatches/);
  assert.doesNotMatch(terminal, /quick-hint-hist-tag|`×\$\{/);
  assert.match(terminal, /item\._source === 'history'[\s\S]*item\.command/);
});
