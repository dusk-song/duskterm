import test from 'node:test';
import assert from 'node:assert/strict';

import { createSessionBooleanState, resolveFocusedSessionId } from './sftpPanelState.js';

test('resolveFocusedSessionId prefers the focused split leaf', () => {
  assert.equal(resolveFocusedSessionId('panel-a', { 'panel-a': 'session-b' }), 'session-b');
});

test('resolveFocusedSessionId falls back to the active panel session', () => {
  assert.equal(resolveFocusedSessionId('session-a', {}), 'session-a');
  assert.equal(resolveFocusedSessionId(null, {}), '');
});

test('session boolean state is isolated by session id', () => {
  const state = createSessionBooleanState(false);

  state.set('session-a', true);
  assert.equal(state.get('session-a'), true);
  assert.equal(state.get('session-b'), false);

  assert.equal(state.toggle('session-b'), true);
  assert.equal(state.get('session-a'), true);
  assert.equal(state.get('session-b'), true);

  state.set('session-a', false);
  assert.equal(state.get('session-a'), false);
  assert.equal(state.get('session-b'), true);
});
