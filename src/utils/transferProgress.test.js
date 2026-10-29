import assert from 'node:assert/strict';
import test from 'node:test';
import { advanceTransferProgress, transferProgressTarget } from './transferProgress.js';

test('unconfirmed byte completion and finalizing never display 100%', () => {
  for (const status of ['uploading', 'transferring', 'finalizing', 'cancelling', 'paused']) {
    assert.equal(transferProgressTarget({ status, loaded: 100, total: 100, progress: 100 }), 99);
  }
});

test('confirmed success reaches 100% including zero-byte transfers', () => {
  assert.equal(transferProgressTarget({ status: 'success', loaded: 0, total: 0, progress: 0 }), 100);
});

test('byte totals give a fractional target while unknown size uses reported progress', () => {
  assert.equal(transferProgressTarget({ status: 'transferring', loaded: 1, total: 8, progress: 12 }), 12.5);
  assert.equal(transferProgressTarget({ status: 'transferring', progress: 45 }), 45);
  assert.equal(transferProgressTarget({ status: 'transferring', progress: NaN }), 0);
  assert.equal(transferProgressTarget({ status: 'transferring', progress: -10 }), 0);
});

test('large first updates are speed limited and every frame stays below the true target', () => {
  let value = 0;
  for (let frame = 0; frame < 150; frame++) {
    const next = advanceTransferProgress(value, 90, 16);
    assert.ok(next >= value && next <= 90);
    assert.ok(next - value <= 2);
    value = next;
  }
  assert.equal(value, 90);
});

test('idle samples stop at the last known target without inventing progress', () => {
  let value = 0;
  for (let frame = 0; frame < 200; frame++) value = advanceTransferProgress(value, 30, 16);
  assert.equal(value, 30);
  assert.equal(advanceTransferProgress(value, 15, 16), 30);
  assert.equal(advanceTransferProgress(value, 30, 16), 30);
});

test('success converges exactly and never overshoots', () => {
  let value = 0;
  for (let frame = 0; frame < 150; frame++) {
    value = advanceTransferProgress(value, 100, 16);
    assert.ok(value <= 100);
  }
  assert.equal(value, 100);
});

test('a long frame does not jump directly to completion', () => {
  assert.ok(advanceTransferProgress(0, 100, 60000) <= 8);
  assert.equal(advanceTransferProgress(30, 90, 0), 30);
  assert.equal(advanceTransferProgress(30, 90, NaN), 30);
});
