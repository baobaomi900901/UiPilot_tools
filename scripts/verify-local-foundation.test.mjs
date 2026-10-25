import { spawnSync } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, symlinkSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { discover, buildStages, runStages, main, repositoryRoot } from './verify-local-foundation.mjs';

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'verify foundation '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const name of ['z plugin', 'a plugin']) {
    const base = path.join(root, 'examples/public-plugins', name);
    mkdirSync(path.join(base, 'package'), { recursive: true });
    mkdirSync(path.join(base, 'tests'));
    writeFileSync(path.join(base, 'package/plugin.json'), '{}');
    writeFileSync(path.join(base, 'tests/runtime.test.js'), '');
    writeFileSync(path.join(base, 'tests/sdk-contract.ts'), '');
  }
  return root;
}

test('discovery sorts all collections and preserves spaced paths as argv', t => {
  const root = fixture(t);
  const found = discover(root);
  assert.equal(found.packages.length, 2);
  for (const items of Object.values(found)) assert.ok(items[0].includes('a plugin'));
  const stages = buildStages(root, found, { npmCli: 'C:/npm cli/npm-cli.js' });
  assert.deepEqual(stages.map(stage => stage.name), [
    'coordinator tests', 'frontend tests', 'frontend build', 'Rust library tests',
    'manifest schema check', 'CLI verify', 'public plugin tests', 'SDK contracts',
    'reference package validation',
  ]);
  assert.deepEqual(stages[6].args.slice(1), found.tests);
  assert.deepEqual(stages[7].args.slice(-found.contracts.length), found.contracts);
  assert.equal(stages[3].args.at(-1), '--offline');
  assert.equal(stages[4].args.at(-1), '--check');
  assert.equal(stages[1].command, process.execPath);
  assert.equal(stages[1].args[0], 'C:/npm cli/npm-cli.js');
  assert.deepEqual(stages[8].commands.map(item => item.args[2]), found.packages);
});

test('missing root and empty collections fail closed', t => {
  const root = fixture(t);
  rmSync(path.join(root, 'examples'), { recursive: true });
  assert.throws(() => discover(root), /discover/i);
  mkdirSync(path.join(root, 'examples/public-plugins'), { recursive: true });
  assert.throws(() => discover(root), /empty/i);
});

for (const collection of ['package', 'tests', 'contracts']) {
  test(`empty ${collection} fails closed`, t => {
    const root = fixture(t);
    for (const name of ['a plugin', 'z plugin']) {
      const base = path.join(root, 'examples/public-plugins', name);
      if (collection === 'contracts') {
        rmSync(path.join(base, 'tests/sdk-contract.ts'));
      } else {
        rmSync(path.join(base, collection), { recursive: true });
      }
    }
    assert.throws(() => discover(root), /empty/i);
  });
}

test('links fail closed', t => {
  const root = fixture(t);
  symlinkSync(root, path.join(root, 'examples/public-plugins/link'), 'junction');
  assert.throws(() => discover(root), /link|ordinary/i);
});

test('first failure stops later stages and spawn errors identify stage', async () => {
  const stages = [
    { name: 'schema', command: 'cargo', args: ['--check'], cwd: repositoryRoot },
    { name: 'later', commands: [] },
  ];
  let calls = 0;
  await assert.rejects(runStages(stages, () => {
    calls++;
    return { status: 7 };
  }, () => {}), /schema.*7/);
  assert.equal(calls, 1);
  await assert.rejects(runStages(stages, () => ({ error: new Error('ENOENT') }), () => {}), /schema.*ENOENT/);
});

test('--list runs no process and resolves root independently of cwd', async t => {
  const root = fixture(t);
  let calls = 0;
  const output = [];
  const result = await main(['--list'], {
    root,
    spawn: () => { calls++; },
    log: message => output.push(message),
  });
  assert.equal(result, 0);
  assert.equal(calls, 0);
  assert.match(output.join('\n'), /2 tests/);
  assert.ok(path.isAbsolute(repositoryRoot));
});

test('schema check failure never runs the generator or later stages', async t => {
  const root = fixture(t);
  const schema = path.join(root, 'docs/plugin-sdk/uipilot-plugin-v1.schema.json');
  mkdirSync(path.dirname(schema), { recursive: true });
  writeFileSync(schema, 'original');
  const calls = [];
  const result = await main([], {
    root,
    spawn: (command, args, options) => {
      calls.push({ command, args, options });
      const isSchemaGenerator = args.includes('generate_public_plugin_schema');
      if (isSchemaGenerator && !args.includes('--check')) {
        writeFileSync(schema, 'generated');
        return { status: 0 };
      }
      return { status: isSchemaGenerator ? 1 : 0 };
    },
    log: () => {},
  });
  assert.equal(result, 1);
  assert.equal(readFileSync(schema, 'utf8'), 'original');
  assert.equal(calls.length, 5);
  assert.ok(calls[4].args.includes('generate_public_plugin_schema'));
  assert.ok(calls[4].args.includes('--check'));
  assert.ok(calls.every(call => !call.args.includes('plugin-cli/dist/cli.mjs')));
});

test('actual entry --list works from another cwd without running checks', t => {
  const root = fixture(t);
  const result = spawnSync(process.execPath, [
    path.join(repositoryRoot, 'scripts/verify-local-foundation.mjs'), '--list',
  ], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /9\. reference package validation/);
  assert.doesNotMatch(result.stdout, /TAP version|Compiling|vitest run/);
});

test('import is inert', () => {
  const url = new URL('./verify-local-foundation.mjs', import.meta.url).href;
  const result = spawnSync(process.execPath, [
    '--input-type=module', '-e', `await import(${JSON.stringify(url)})`,
  ], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
});

test('empty package directory is rejected', t => {
  const root = fixture(t);
  rmSync(path.join(root, 'examples/public-plugins/a plugin/package/plugin.json'));
  assert.throws(() => discover(root), /invalid package/);
});

test('thrown spawn error is attributed and stops execution', async () => {
  await assert.rejects(runStages([
    { name: 'launch', command: 'missing', args: [] },
  ], () => { throw new Error('ENOENT'); }, () => {}), /launch.*ENOENT/);
});

test('entry reports spawn failure as nonzero without running later stages', async t => {
  const root = fixture(t);
  const output = [];
  let calls = 0;
  const result = await main([], {
    root,
    spawn: (_command, _args, options) => {
      calls++;
      assert.equal(options.shell, false);
      assert.equal(options.cwd, root);
      return { error: new Error('ENOENT') };
    },
    log: message => output.push(message),
  });
  assert.equal(result, 1);
  assert.equal(calls, 1);
  assert.match(output.join('\n'), /FAILED: coordinator tests: spawn failed: ENOENT/);
});

test('a file in place of the discovery root fails closed', t => {
  const root = fixture(t);
  const directory = path.join(root, 'examples/public-plugins');
  rmSync(directory, { recursive: true });
  writeFileSync(directory, 'not a directory');
  assert.throws(() => discover(root), /ordinary directory/);
});

test('unreleased multi-package fixture adds tests and SDK contracts without release validation', t => {
  const root = fixture(t);
  const base = path.join(root, 'examples/public-plugins/multi plugin/tests');
  const pkg = path.join(base, 'fixtures/multi-package');
  mkdirSync(path.join(pkg, 'dist'), { recursive: true });
  writeFileSync(path.join(pkg, 'plugin.json'), '{}');
  writeFileSync(path.join(pkg, 'dist/runtime.js'), 'export async function onCommand() {}');
  writeFileSync(path.join(base, 'runtime.test.js'), '');
  writeFileSync(path.join(base, 'sdk-contract.ts'), '');
  const found = discover(root);
  assert.equal(found.tests.length, 3);
  assert.equal(found.contracts.length, 3);
  assert.equal(found.packages.length, 2);
  assert.ok(found.tests.includes(path.join(base, 'runtime.test.js')));
  assert.ok(found.contracts.includes(path.join(base, 'sdk-contract.ts')));
  const stages = buildStages(root, found, { npmCli: 'C:/npm cli/npm-cli.js' });
  assert.deepEqual(stages[7].args.slice(-found.contracts.length), found.contracts);
  assert.deepEqual(stages[8].commands.map(item => item.args[2]), found.packages);
  assert.ok(stages[8].commands.every(item => !item.args.includes(pkg)));
});

test('release repository inventory includes the complete multi-command package', () => {
  const found = discover(repositoryRoot);
  assert.equal(found.tests.length, 18);
  assert.equal(found.contracts.length, 12);
  assert.equal(found.packages.length, 15);
  assert.ok(found.tests.some(file => file.includes('com.uipilot.demo-multi')));
  assert.ok(found.contracts.some(file => file.includes('com.uipilot.demo-multi')));
  assert.ok(found.packages.some(file => file.includes('com.uipilot.demo-multi')));
  assert.ok(found.tests.some(file => file.includes('com.uipilot.demo-settings')));
  assert.ok(found.packages.some(file => file.includes('com.uipilot.demo-settings')));
  assert.ok(found.tests.some(file => file.includes('com.uipilot.json-workbench')));
  assert.ok(found.contracts.some(file => file.includes('com.uipilot.json-workbench')));
  assert.ok(found.packages.some(file => file.includes('com.uipilot.json-workbench')));
});


test('Host release versions agree while the CLI package version stays independent', () => {
  const read = name => readFileSync(path.join(repositoryRoot, name), 'utf8').replaceAll('\r\n', '\n');
  const root = JSON.parse(read('package.json'));
  const lock = JSON.parse(read('package-lock.json'));
  assert.equal(root.version, '0.9.0');
  assert.equal(lock.version, root.version);
  assert.equal(lock.packages[''].version, root.version);
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version, root.version);
  assert.match(read('src-tauri/Cargo.toml'), /\[package\][\s\S]*?\nversion = "0\.9\.0"/);
  assert.match(read('src-tauri/Cargo.lock'), /name = "uipilot"\nversion = "0\.9\.0"/);
  const runtimeVersion = read('src-tauri/src/public_plugins.rs').match(/impl PublicPluginHost \{[\s\S]*?version: \[(\d+), (\d+), (\d+)\]/);
  assert.ok(runtimeVersion, 'real runtime Host version is missing');
  assert.equal(runtimeVersion.slice(1).join('.'), root.version);
  assert.equal(JSON.parse(read('packages/plugin-cli/package.json')).version, '0.1.0');
  assert.equal(lock.packages['packages/plugin-cli'].version, '0.1.0');
});
