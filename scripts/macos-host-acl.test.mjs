import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = path => JSON.parse(readFileSync(new URL('../src-tauri/' + path, import.meta.url)));
test('Mac production enables only a local main capability and its core commands', () => {
  const config = read('tauri.macos.conf.json');
  assert.deepEqual(config.app.security.capabilities, ['macos-main']);
  const acl = read('capabilities/macos-main.json');
  assert.deepEqual(acl.platforms, ['macOS']);
  assert.deepEqual(acl.webviews, ['main']);
  assert.equal(acl.remote, undefined);
  assert.deepEqual(acl.permissions.toSorted(), [
    'allow-get-host-capabilities', 'allow-hide-launcher', 'allow-set-settings-pinned', 'allow-load-settings', 'allow-set-theme-preference',
    'allow-search-apps', 'allow-execute-result', 'allow-set-web-search-engine', 'allow-save-hotkey', 'allow-set-hotkey-recording',
    'allow-get-macos-login-item', 'allow-set-macos-login-item', 'allow-open-macos-login-item-settings',
    'allow-list-macos-plugins', 'allow-prepare-macos-plugin-install', 'allow-commit-macos-plugin-install',
    'allow-cancel-macos-plugin-install', 'allow-set-macos-plugin-enabled', 'allow-set-macos-plugin-permission',
    'allow-save-macos-plugin-command', 'allow-set-plugin-favorite',
    'allow-save-macos-plugin-settings', 'allow-remove-macos-plugin',
    'allow-get-launcher-hotkey-status', 'core:window:allow-start-dragging',
    'core:app:allow-version', 'core:event:allow-listen', 'core:event:allow-unlisten',
  ].toSorted());
  assert.ok(read('capabilities/main.json').permissions.includes('allow-get-host-capabilities'));
});

test('preview grants main and Find separate local IPC routes', () => {
  const [main, find] = read('macos-find-preview-acl.json');
  // The preview grants these commands at runtime; build-time command pruning cannot see that ACL.
  for (const file of ['tauri.conf.json', 'tauri.macos.conf.json', 'tauri.macos-internal.conf.json']) {
    assert.notEqual(read(file).build?.removeUnusedCommands, true);
  }
  for (const acl of [main, find]) {
    assert.deepEqual(acl.platforms, ['macOS']);
    assert.equal(acl.remote, undefined);
    assert.equal(acl.windows, undefined);
  }
  assert.deepEqual(main.webviews, ['main']);
  assert.deepEqual(main.permissions.toSorted(), ['allow-open-find-window', 'allow-set-builtin-feature-favorite']);
  assert.deepEqual(find.webviews, ['find']);
  assert.deepEqual(find.permissions.toSorted(), [
    'allow-cancel-find-query', 'allow-commit-find-ready', 'allow-execute-result', 'allow-get-find-ready-status',
    'allow-hide-find-window', 'allow-load-find-thumbnail', 'allow-open-find-document', 'allow-prepare-find-initialization', 'allow-search-files',
    'allow-set-find-pinned', 'allow-set-find-preview-preference',
    'core:event:allow-listen', 'core:event:allow-unlisten',
  ].toSorted());
  for (const acl of [main, find]) {
    assert.ok(!read('tauri.macos.conf.json').app.security.capabilities.includes(acl.identifier));
  }
});

test('only the interactive public preview Find webview may request Documents access', () => {
  const acl = read('macos-find-documents-acl.json');
  assert.deepEqual(acl.platforms, ['macOS']);
  assert.deepEqual(acl.webviews, ['find']);
  assert.equal(acl.remote, undefined);
  assert.equal(acl.windows, undefined);
  assert.deepEqual(acl.permissions.toSorted(), [
    'allow-get-find-documents-access', 'allow-grant-find-documents-access', 'allow-revoke-find-documents-access',
  ]);
  assert.ok(!read('tauri.macos.conf.json').app.security.capabilities.includes(acl.identifier));
  for (const window of read('macos-find-preview-acl.json')) {
    assert.ok(!window.permissions.some(permission => acl.permissions.includes(permission)));
  }
});

import { waitForBootstrap } from './smoke-platform-bootstrap.mjs';
test('smoke requires both the trusted frontend marker and a clean exit', async () => {
  const run = code => waitForBootstrap(process.execPath, ['-e', code], { log() {} });
  await run("console.error('[UiPilot] main frontend ready')");
  await assert.rejects(run('process.exit(0)'), /ready=false/);
  await assert.rejects(run("console.error('[UiPilot] main frontend ready'); process.exitCode = 2"), /status=2/);
});
