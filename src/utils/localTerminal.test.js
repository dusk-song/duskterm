import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('session list keeps local shell launchers above the session tree', () => {
  const source = read('../components/session/SessionList.vue');
  const search = source.indexOf('<div class="search-bar">');
  const launcher = source.indexOf('class="local-shell-launcher"');
  const tree = source.indexOf('class="session-tree-viewport"');

  assert.ok(search >= 0);
  assert.ok(launcher > search);
  assert.ok(tree > launcher);
  assert.match(source, /grid-template-columns:\s*repeat\(var\(--local-shell-button-count\)/);
  assert.match(source, /white-space:\s*nowrap/);
});

test('session directories toggle from the full row and expose a trailing reorder handle', () => {
  const source = read('../components/session/SessionList.vue');
  assert.match(source, /@click="onTreeRowClick\(item\)"/);
  assert.match(source, /class="tree-drag-handle"/);
  assert.match(source, /<GripVertical \/>/);
  assert.match(source, /@drop\.stop\.prevent/);
  assert.match(source, /sourceBlock[\s\S]*next\.splice\(targetIdx,\s*0,\s*\.\.\.sourceBlock\)/);
});

test('windows launcher is restricted to PowerShell and CMD', () => {
  const source = read('../components/session/SessionList.vue');
  assert.match(source, /\['powershell', 'cmd'\]\.includes\(profile\.id\)/);
  assert.doesNotMatch(source, /pwsh|wsl|git-bash/i);
});

test('local terminal uses the shared terminal connection path', () => {
  const store = read('../stores/ssh.js');
  const backend = read('../../src-tauri/src/session/actor.rs');

  assert.match(store, /protocol:\s*'local'/);
  assert.match(store, /openLocalTerminal/);
  assert.match(backend, /is_local_protocol\(&config\)/);
  assert.match(backend, /connect_local_terminal_runtime/);
});

test('terminal decoder preserves utf-8 sequences split across output chunks', () => {
  const terminal = read('../components/terminal/Terminal.vue');
  assert.match(terminal, /textDecoder\.decode\(rawBytes,\s*\{\s*stream:\s*true\s*\}\)/);
});

test('application exit drains all managed terminal runtimes', () => {
  const backend = read('../../src-tauri/src/lib.rs');
  const supervisor = read('../../src-tauri/src/session/supervisor.rs');

  assert.match(backend, /disconnect_all\(/);
  assert.match(supervisor, /pub async fn disconnect_all/);
});

test('local terminal owns its process tree and resolves the Unix account shell', () => {
  const backend = read('../../src-tauri/src/local_terminal.rs');
  assert.match(backend, /JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE/);
  assert.match(backend, /libc::kill\(-self\.process_group/);
  assert.match(backend, /\.or_else\(account_login_shell\)/);
});

test('terminal output is merged before xterm rendering', () => {
  const terminal = read('../components/terminal/Terminal.vue');
  const enqueue = terminal.slice(
    terminal.indexOf('const enqueueTerminalOutput'),
    terminal.indexOf('function focusSearchInputSoon'),
  );
  assert.match(enqueue, /pendingOutputChunks\.push\(chunk\)/);
  assert.match(enqueue, /requestAnimationFrame\(flushTerminalOutput\)/);
  assert.doesNotMatch(enqueue, /term\?\.write\(chunk\)/);
});
