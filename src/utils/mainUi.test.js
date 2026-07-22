import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { normalizeBackgroundSettings } from './background.js';

describe('normalizeMainUiSettings background', () => {
  it('fills defaults and clamps invalid background values', () => {
    const result = normalizeBackgroundSettings({ fit: 'bad', blur: 99, opacity: -1, originalPath: 'legacy' });
    assert.equal(result.enabled, false);
    assert.equal(result.fit, 'cover');
    assert.equal(result.blur, 40);
    assert.equal(result.opacity, 0);
    assert.equal(result.darkOverlay, 0.35);
    assert.equal(result.lightOverlay, 0.18);
    assert.equal('originalPath' in result, false);
  });
});

describe('background asset protocol', () => {
  it('allows the absolute application data directories used on Linux', () => {
    const config = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
    const scope = config.app.security.assetProtocol.scope;

    assert.equal(scope.requireLiteralLeadingDot, false);
    assert.ok(scope.allow.includes('$LOCALDATA/duskterm/backgrounds/**/*'));
    assert.ok(scope.allow.includes('$DATA/duskterm/backgrounds/**/*'));
  });
});
