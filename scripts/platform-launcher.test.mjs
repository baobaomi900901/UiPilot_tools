import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { getLaunchCommand } from './platform-launcher.mjs';

const read = file => JSON.parse(readFileSync(new URL('../' + file, import.meta.url)));
const config = name => read('src-tauri/tauri.' + name + 'json');
const target = 'aarch64-apple-darwin';
const settings = { platform: 'darwin', arch: 'arm64', env: {}, rustHost: target };
const run = (args, overrides) => getLaunchCommand('tauri', args, { ...settings, ...overrides });

test('npm dispatch retains Windows PowerShell development wrappers and literal arguments', () => {
  for (const [entry, script] of [['tauri', 'tauri'], ['dev', 'dev-with-everything']]) {
    assert.match(read('package.json').scripts[entry], /platform-launcher\.mjs/);
    const command = getLaunchCommand(entry, ['dev', '--config', 'a b.json'], { platform: 'win32' });
    assert.equal(command.file, 'powershell.exe');
    assert.deepEqual(command.args, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
      'scripts/' + script + '.ps1', 'dev', '--config', 'a b.json']);
  }
});

test('Windows packaging enters the managed Provider session with literal Tauri and Cargo arguments', () => {
  for (const verb of ['build', 'bundle']) {
    const args = [verb, '--config', 'path with spaces.json', '--', '--locked'];
    const command = getLaunchCommand('tauri', args, { platform: 'win32', env: process.env });
    assert.equal(command.file, 'python');
    assert.deepEqual(command.args, ['scripts/provider_build.py', 'build-session', '--', ...args]);
    assert.equal(command.env, process.env);
  }
});

test('native Mac development and builds use the project CLI and fixed deployment target', () => {
  const dev = getLaunchCommand('dev', [], settings);
  assert.match(dev.args[0], /vite\/bin\/vite\.js$/);
  const build = run(['build', '--bundles', 'app,dmg', '--', '--locked']);
  assert.equal(build.file, process.execPath);
  assert.deepEqual(build.args.slice(1), ['build', '--target', target, '--bundles', 'app,dmg', '--', '--locked']);
  assert.equal(build.env.MACOSX_DEPLOYMENT_TARGET, '15.7.4');
  assert.equal(run(['build', '-t', target]).args.filter(arg => arg === target).length, 1);
  assert.equal(run(['bundle']).args.at(-1), target);
});

test('rejects translated Node/Rust, stray Cargo targets and non-native target forms', () => {
  assert.throws(() => run(['build'], { arch: 'x64' }), /arm64/);
  assert.throws(() => run(['build'], { rustHost: 'x86_64-apple-darwin' }), /aarch64/);
  assert.throws(() => run(['build'], { env: { CARGO_BUILD_TARGET: 'x86_64-apple-darwin' } }), /target/i);
  assert.throws(() => run(['--verbose', 'build', '-t', 'universal-apple-darwin']), /target/i);
  for (const extra of [['--target', 'universal-apple-darwin'], ['--target=x86_64-apple-darwin'],
    ['-tx86_64-apple-darwin'], ['--', '--target', 'x86_64-apple-darwin']]) {
    assert.throws(() => run(['build', ...extra]), /target/i);
  }
});

test('CLI propagates launch errors to npm', () => {
  const result = spawnSync(process.execPath, ['scripts/platform-launcher.mjs', 'unknown']);
  assert.equal(result.status, 1);
  assert.match(result.stderr.toString(), /unknown/i);
});

test('Mac child receives literal arguments and its exit status reaches the caller',
  { skip: process.platform !== 'darwin' || process.arch !== 'arm64' }, t => {
    const dir = mkdtempSync(path.join(tmpdir(), 'uipilot-launcher-'));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    const bin = path.join(dir, 'node_modules/vite/bin');
    mkdirSync(bin, { recursive: true });
    writeFileSync(path.join(bin, 'vite.js'), 'console.log(JSON.stringify(process.argv.slice(2)));process.exitCode=23');
    const launcher = fileURLToPath(new URL('./platform-launcher.mjs', import.meta.url));
    const args = ['two words', 'x & y'];
    const result = spawnSync(process.execPath, [launcher, 'dev', ...args], { cwd: dir, encoding: 'utf8' });
    assert.equal(result.status, 23);
    assert.deepEqual(JSON.parse(result.stdout), args);
  });

test('Mac config isolates resources, startup windows, hooks, and icons', () => {
  const base = config('conf.');
  const mac = config('macos.conf.');
  assert.equal(base.build.beforeBuildCommand, 'npm run build');
  assert.equal(base.build.beforeBundleCommand, undefined);
  assert.deepEqual((mac.app?.windows ?? base.app.windows).map(w => w.label), ['main']);
  assert.deepEqual(mac.bundle.targets, ['app', 'dmg']);
  assert.equal(mac.bundle.macOS.minimumSystemVersion, '15.7.4');
  assert.deepEqual(mac.bundle.icon, ['icons/icon.icns']);
  assert.deepEqual(mac.bundle.resources ?? base.bundle.resources, ['resources/sounds/message-notification.wav']);
});

test('Windows and profile overrides retain resources and staging order', () => {
  const win = config('windows.conf.');
  const vm = config('vm-instrumented.conf.');
  const base = config('conf.');
  const winResources = win.bundle.resources;
  for (const resource of [...base.bundle.resources, 'resources/everything/Everything.exe',
    'resources/provider/UIAutoma.UIAutomation.Provider.exe', 'resources/provider/asset-manifest.json']) {
    assert.ok(winResources.includes(resource));
  }
  assert.deepEqual((win.app?.windows ?? base.app.windows).map(w => w.label), ['main', 'find']);
  assert.match(win.build.beforeBuildCommand, /provider_build.py prepare && npm run build && npm run build:managed-everything$/);
  assert.match(win.build.beforeBundleCommand, /provider_build.py validate && npm run stage:managed-everything:production$/);
  assert.match(vm.build.beforeBundleCommand, /stage:managed-everything:vm$/);
  assert.equal(win.bundle.windows.nsis.installerHooks, 'windows/nsis-hooks.nsh');
});
