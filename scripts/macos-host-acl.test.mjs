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
    'allow-search-apps', 'allow-execute-result', 'allow-get-host-capabilities', 'allow-hide-launcher', 'allow-load-settings', 'allow-set-theme-preference',
    'core:app:allow-version', 'core:event:allow-listen', 'core:event:allow-unlisten',
  ].toSorted());
  assert.ok(read('capabilities/main.json').permissions.includes('allow-get-host-capabilities'));
});

import { waitForBootstrap } from './smoke-platform-bootstrap.mjs';
test('smoke requires both the trusted frontend marker and a clean exit', async () => {
  const run = code => waitForBootstrap(process.execPath, ['-e', code], { log() {} });
  await run("console.error('[UiPilot] main frontend ready')");
  await assert.rejects(run('process.exit(0)'), /ready=false/);
  await assert.rejects(run("console.error('[UiPilot] main frontend ready'); process.exitCode = 2"), /status=2/);
});
