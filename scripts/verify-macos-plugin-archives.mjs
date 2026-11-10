import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin' || process.arch !== 'arm64') {
  throw new Error('Run with native arm64 Node on macOS.');
}

const root = fileURLToPath(new URL('../', import.meta.url));
const output = mkdtempSync(join(realpathSync(tmpdir()), 'uipilot-plugin-archives.'));
const archives = join(output, 'archives');
const examples = join(root, 'examples', 'public-plugins');
const cli = join(root, 'packages', 'plugin-cli', 'dist', 'cli.mjs');
mkdirSync(archives);
console.log('Plugin archive verification output:', output);

function run(program, args, log, env = process.env) {
  const result = spawnSync(program, args, { cwd: root, env, encoding: 'utf8', timeout: 600_000 });
  writeFileSync(join(output, log), result.stdout ?? '');
  const diagnostics = [result.stderr, result.error?.message].filter(Boolean).join('\n');
  if (diagnostics) writeFileSync(join(output, `${log}.stderr`), diagnostics);
  if (result.status !== 0) {
    throw new Error(`${program} failed. See ${join(output, log)} and its stderr log if present.`, { cause: result.error });
  }
  return result.stdout;
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

const sourceCommit = run('git', ['rev-parse', 'HEAD'], 'source-commit.txt').trim();
const sourceTreeDirty = Boolean(run('git', ['status', '--porcelain'], 'source-status.txt').trim());
run('npm', ['run', 'build', '--workspace', '@uipilot/plugin-cli'], 'cli-build.log');
const upgrade = join(output, 'upgrade-source');
run(process.execPath, [join(examples, 'com.uipilot.demo-settings', 'prepare-upgrade.mjs'), upgrade], 'upgrade-source.log');
const cases = [
  ['demo-settings', join(examples, 'com.uipilot.demo-settings', 'package')],
  ['demo-settings-upgrade', upgrade],
  ['demo-return', join(examples, 'com.uipilot.demo-return', 'package')],
  ['demo-multi', join(examples, 'com.uipilot.demo-multi', 'package')],
];
const packages = cases.map(([name, source]) => {
  const file = `${name}.uipilot-plugin`;
  const path = join(archives, file);
  const packed = JSON.parse(run(process.execPath, [
    cli, 'pack', source, '--output', path, '--platform', 'windows', '--json',
  ], `${name}-pack.json`));
  const hash = sha256(path);
  if (!packed.created || packed.output.sha256 !== hash) {
    throw new Error(`Packed archive hash mismatch for ${file}`);
  }
  const validations = ['windows', 'macos'].map(platform => JSON.parse(run(process.execPath, [
    cli, 'validate', path, '--platform', platform, '--json',
  ], `${name}-${platform}.json`)));
  const { pluginId, version } = packed.validation.plugin;
  if (validations.some((validation, i) =>
    !validation.valid || validation.target.platform !== ['windows', 'macos'][i]
      || validation.plugin?.pluginId !== pluginId || validation.plugin?.version !== version
  )) {
    throw new Error(`Platform validation disagreed for ${file}`);
  }
  return { file, pluginId, version, sha256: hash, cliValidatedPlatforms: ['windows', 'macos'] };
});
writeFileSync(join(archives, 'SHA256SUMS'), packages.map(pkg => `${pkg.sha256}  ${pkg.file}\n`).join(''));

const probe = join(output, 'probe');
run(join(root, 'spikes', 'macos-runtime-bridge-probe', 'build.sh'), [probe], 'probe-build.log');
const helper = join(probe, 'helper');
const helperHash = sha256(helper);
const env = {
  ...process.env,
  UIPILOT_LAUNCHER_TEST_HELPER: helper,
  UIPILOT_ARCHIVE_TEST_INPUT: archives,
  UIPILOT_ARCHIVE_TEST_SETTINGS_ROOT: join(output, 'settings-fixture'),
};
const prefix = 'public_plugins::launcher_runtime_macos::tests::process::archive::';
const cargo = ['test', '--offline', '--locked', '--manifest-path', 'src-tauri/Cargo.toml', '--lib',
  '--features', 'macos-find-preview,macos-login-item-preview'];
const seed = run('cargo', [...cargo, `${prefix}seed_settings`, '--', '--exact', '--ignored', '--nocapture'], 'archive-seed.log', env);
const results = run('cargo', [...cargo, prefix, '--', '--ignored', '--skip', 'seed_settings', '--nocapture'], 'archive-tests.log', env);
if (!seed.includes('test result: ok. 1 passed;') || !results.includes('test result: ok. 3 passed;')) {
  throw new Error('Expected one seed test and three archive verification tests.');
}
if (sha256(helper) !== helperHash || packages.some(pkg => sha256(join(archives, pkg.file)) !== pkg.sha256)) {
  throw new Error('Archive or helper bytes changed during verification.');
}
writeFileSync(join(output, 'verification.json'), JSON.stringify({
  sourceCommit,
  sourceTreeDirty,
  packages,
  macosDiagnosticHost: { archiveTestsPassed: 4, helperSha256: helperHash },
  productionHostsVerified: [],
}, null, 2) + '\n');
console.log('Archive tests passed. Report:', join(output, 'verification.json'));
