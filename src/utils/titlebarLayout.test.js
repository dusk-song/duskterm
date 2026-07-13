import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { resolveTitlebarVisibility } from './titlebarLayout.js';

describe('resolveTitlebarVisibility', () => {
  it('hides monitor, transfer, then menus from right to left as width shrinks', () => {
    assert.deepEqual(resolveTitlebarVisibility(1400), {
      monitor: true,
      transfer: true,
      menus: ['file', 'edit', 'view', 'connection', 'help'],
    });
    assert.equal(resolveTitlebarVisibility(1200).monitor, false);
    assert.equal(resolveTitlebarVisibility(1200).transfer, true);
    assert.equal(resolveTitlebarVisibility(1000).transfer, false);
    assert.deepEqual(resolveTitlebarVisibility(900).menus, ['file', 'edit', 'view', 'connection']);
    assert.deepEqual(resolveTitlebarVisibility(820).menus, ['file', 'edit', 'view']);
    assert.deepEqual(resolveTitlebarVisibility(740).menus, ['file', 'edit']);
    assert.deepEqual(resolveTitlebarVisibility(660).menus, ['file']);
    assert.deepEqual(resolveTitlebarVisibility(580).menus, []);
  });
});
