import { readFileSync, readdirSync, lstatSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function discover(root) {
  const found = { tests: [], contracts: [], packages: [] };

  function walk(dir) {
    const stat = lstatSync(dir);
    if (stat.isSymbolicLink() || !stat.isDirectory()) {
      throw new Error(`Not an ordinary directory (or link): ${dir}`);
    }
    for (const name of readdirSync(dir).sort()) {
      const full = path.join(dir, name);
      const info = lstatSync(full);
      if (info.isSymbolicLink() || (!info.isFile() && !info.isDirectory())) {
        throw new Error(`Not an ordinary entry (or link): ${full}`);
      }
      if (info.isDirectory()) {
        if (name === 'package') found.packages.push(full);
        walk(full);
      } else {
        if (/\.test\.(?:js|mjs|cjs)$/.test(name)) found.tests.push(full);
        if (name === 'sdk-contract.ts') found.contracts.push(full);
      }
    }
  }

  try {
    walk(path.join(root, 'examples/public-plugins'));
  } catch (error) {
    throw new Error(`Discovery failed: ${error.message}`);
  }
  for (const [kind, items] of Object.entries(found)) {
    items.sort();
    if (!items.length) throw new Error(`Discovery found empty ${kind} collection`);
  }
  for (const pkg of found.packages) {
    if (!existsSync(path.join(pkg, 'plugin.json'))) {
      throw new Error(`Empty or invalid package: ${pkg}`);
    }
  }
  return found;
}

export function resolveNpmCli(env = process.env) {
  const candidates = [env.npm_execpath];
  const searchDirectories = [
    path.dirname(process.execPath),
    ...(env.PATH ?? env.Path ?? '').split(path.delimiter),
  ];
  for (const dir of searchDirectories) {
    candidates.push(
      path.join(dir, 'node_modules/npm/bin/npm-cli.js'),
      path.join(dir, 'npm/node_modules/npm/bin/npm-cli.js'),
    );
  }
  const cli = candidates.find(candidate =>
    candidate && /npm-cli\.js$/i.test(candidate) && existsSync(candidate));
  if (!cli) {
    throw new Error('Cannot locate npm-cli.js; run through npm or add Node/npm to PATH');
  }
  return cli;
}

export function buildStages(root, found, { npmCli = resolveNpmCli() } = {}) {
  const command = (name, executable, args, cwd = root) => ({ name, command: executable, args, cwd });
  const node = (name, args, cwd) => command(name, process.execPath, args, cwd);
  const npm = (name, args) => node(name, [npmCli, ...args]);

  return [
    node('coordinator tests', ['--test', path.join(root, 'scripts/verify-local-foundation.test.mjs')]),
    npm('frontend tests', ['test']),
    npm('frontend build', ['run', 'build']),
    command('Rust library tests', 'cargo', ['test', '--lib', '--offline'], path.join(root, 'src-tauri')),
    command('manifest schema check', 'cargo', [
      'run', '--offline', '--bin', 'generate_public_plugin_schema', '--', '--check',
    ], path.join(root, 'src-tauri')),
    npm('CLI verify', ['run', 'verify', '--workspace', '@uipilot/plugin-cli']),
    node('public plugin tests', ['--test', ...found.tests]),
    node('SDK contracts', [
      path.join(root, 'node_modules/typescript/bin/tsc'),
      '--ignoreConfig', '--noEmit', '--strict', ...found.contracts,
    ]),
    {
      name: 'reference package validation',
      commands: found.packages.flatMap(pkg => {
        const { supportedPlatforms } = JSON.parse(readFileSync(path.join(pkg, 'plugin.json'), 'utf8'));
        if (!Array.isArray(supportedPlatforms) || supportedPlatforms.length === 0 ||
            !supportedPlatforms.every(platform => ['windows','macos'].includes(platform))) {
          throw new Error('Invalid supportedPlatforms in ' + pkg);
        }
        return ['windows','macos'].map(platform => {
          const item = node(pkg + ' (' + platform + ')', [
            path.join(root, 'packages/plugin-cli/dist/cli.mjs'), 'validate', pkg, '--platform', platform,
          ]);
          if (!supportedPlatforms.includes(platform)) {
            item.args.push('--json');
            item.expectIncompatible = platform;
          }
          return item;
        });
      }),
    },
  ];
}

export async function runStages(stages, spawn = spawnSync, log = console.log) {
  for (const stage of stages) {
    log(`\n[verify] ${stage.name}`);
    for (const item of stage.commands ?? [stage]) {
      let result;
      try {
        result = spawn(item.command, item.args, { cwd: item.cwd, stdio: item.expectIncompatible ? ['ignore','pipe','inherit'] : 'inherit', encoding: 'utf8', shell: false });
      } catch (error) {
        throw new Error(`${stage.name}: spawn failed: ${error.message}`);
      }
      if (result.error) {
        throw new Error(`${stage.name}: spawn failed: ${result.error.message}`);
      }
      if (item.expectIncompatible) {
        const report = result.status === 1 ? JSON.parse(result.stdout) : null;
        if (report?.valid !== false || report.target?.platform !== item.expectIncompatible ||
            !report.issues?.some(issue => issue.code === 'PLATFORM_INCOMPATIBLE')) {
          throw new Error(stage.name + ': expected PLATFORM_INCOMPATIBLE for ' + item.name);
        }
        continue;
      }
      if (result.status !== 0) {
        throw new Error(`${stage.name}: ${item.name} exited ${result.status ?? result.signal ?? 'unknown'}`);
      }
    }
  }
}

export async function main(args = process.argv.slice(2), {
  root = repositoryRoot, spawn = spawnSync, log = console.log,
} = {}) {
  try {
    if (args.some(arg => arg !== '--list')) {
      throw new Error('Usage: verify-local-foundation.mjs [--list]');
    }
    const found = discover(root);
    const stages = buildStages(root, found);
    log(`[verify] discovered ${found.tests.length} tests, ${found.contracts.length} contracts, ${found.packages.length} packages`);
    if (args.includes('--list')) {
      stages.forEach((stage, index) => {
        log(`${index + 1}. ${stage.name}${stage.commands ? ` (${stage.commands.length})` : ''}`);
      });
    } else {
      await runStages(stages, spawn, log);
    }
    return 0;
  } catch (error) {
    log(`[verify] FAILED: ${error.message}`);
    return 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main();
}
