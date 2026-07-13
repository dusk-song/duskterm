import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('global background UI surfaces', () => {
  it('keeps titlebar drag affordance, terminal transparency, and session search shape wired', () => {
    const app = read('../App.vue');
    const titlebar = read('../components/app-shell/CustomTitlebar.vue');
    const monitorDock = read('../components/app-shell/MonitorDock.vue');
    const transferDock = read('../components/app-shell/TransferDock.vue');
    const settingsModal = read('../components/settings/SettingsModal.vue');
    const settingsMainUiPane = read('../components/settings/SettingsMainUiPane.vue');
    const terminal = read('../components/terminal/Terminal.vue');
    const terminalManager = read('../components/terminal/TerminalPanelManager.vue');
    const sftpManager = read('../components/sftp/SftpFileManager.vue');
    const sessionList = read('../components/session/SessionList.vue');

    assert.match(titlebar, /class="titlebar-drag-layer"[^>]*data-tauri-drag-region/);
    assert.match(titlebar, /\.dusk-titlebar\s*\{[^}]*align-items:\s*center[^}]*height:\s*62px/s);
    assert.match(titlebar, /\.titlebar-left,\s*\.titlebar-center,\s*\.titlebar-right\s*\{[^}]*transform:\s*translateY\(-4px\)/s);
    assert.match(settingsMainUiPane, /emit\('background-preview-change'\)/);
    assert.match(settingsMainUiPane, /mainUiSettings\.background\?\.blur[\s\S]*mainUiSettings\.background\?\.opacity[\s\S]*mainUiSettings\.background\?\.darkOverlay[\s\S]*mainUiSettings\.background\?\.lightOverlay/);
    assert.match(settingsModal, /new CustomEvent\('main-ui-settings-changed',\s*\{[\s\S]*detail:\s*\{[\s\S]*preview:\s*true[\s\S]*settings:/);
    assert.match(app, /class="app-shell has-floating-surfaces"/);
    assert.match(app, /\.app-shell\.has-floating-surfaces\s*\{[^}]*--terminal-surface-bg/s);
    assert.match(app, /\.app-shell\.has-floating-surfaces \.workspace-grid\s*\{[^}]*padding:\s*0 7px 7px/s);
    assert.match(app, /\.recent-sessions-tree-container\s*\{[^}]*border-radius:\s*var\(--niri-radius-lg/s);
    assert.match(monitorDock, /\.monitor-dock\s*\{[^}]*font:\s*700 11px/s);
    assert.match(transferDock, /<Teleport to="body">/);
    assert.match(transferDock, /\.transfer-popup\s*\{[^}]*position:\s*fixed[^}]*z-index:\s*99999/s);
    assert.match(terminal, /closest\('\.has-floating-surfaces'\)/);
    assert.match(terminal, /--terminal-surface-bg/);
    assert.match(terminalManager, /\.split-leaf\s*\{[^}]*background:\s*transparent/s);
    assert.match(app, /class="main-panel-body"[^>]*has-sftp-panel/);
    assert.match(app, /\.main-panel-body\.has-sftp-panel\s*\{[^}]*background:\s*var\(--terminal-surface-bg/s);
    assert.match(app, /\.main-panel-body\.has-sftp-panel \.terminal-panel-manager\s*\{[^}]*border-bottom-left-radius:\s*0/s);
    assert.match(app, /\.main-panel-body\.has-sftp-panel \.sftp-bottom-panel\s*\{[^}]*background:\s*transparent/s);
    assert.match(sftpManager, /\.file-manager\s*\{[^}]*background:\s*transparent/s);
    assert.match(sftpManager, /\.fm-table-body\s*\{[^}]*background:\s*transparent/s);
    assert.match(sessionList, /class="session-search-input"/);
    assert.match(sessionList, /\.search-bar\s*\{[^}]*background:\s*transparent/s);
  });
});
