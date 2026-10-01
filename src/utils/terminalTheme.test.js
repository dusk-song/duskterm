import assert from 'node:assert/strict';
import test from 'node:test';
import {
  TERMINAL_SESSION_PRESENTATIONS,
  resolveTerminalSessionPresentation
} from './terminalPresentation.js';

test('resolveTerminalSessionPresentation accepts the traditional tab presentation', () => {
  assert.equal(
    resolveTerminalSessionPresentation(TERMINAL_SESSION_PRESENTATIONS.CLASSIC_TABS),
    TERMINAL_SESSION_PRESENTATIONS.CLASSIC_TABS
  );
});

test('resolveTerminalSessionPresentation falls back to the current presentation', () => {
  assert.equal(resolveTerminalSessionPresentation(), TERMINAL_SESSION_PRESENTATIONS.CURRENT);
  assert.equal(resolveTerminalSessionPresentation('unknown'), TERMINAL_SESSION_PRESENTATIONS.CURRENT);
});
