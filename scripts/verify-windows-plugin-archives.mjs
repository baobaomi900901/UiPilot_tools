import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Transfer verification.json and its sibling archives/ together. For the native
// check, use an x64 MSVC developer shell with the Tauri build inputs staged as in
// the Windows desktop-port workflow. --check verifies the archive bytes on any OS.
const [reportPath, option, ...extra] = process.argv.slice(2);
if (!reportPath || (option && option !== '--check') || extra.length) {
  throw new Error('Usage: node scripts/verify-windows-plugin-archives.mjs <verification.json> [--check]');
}
const checkOnly = option === '--check';
if (!checkOnly && (process.platform !== 'win32' || process.arch !== 'x64')) {
  throw new Error('The native check requires x64 Windows. Use --check to verify the archive bytes here.');
}

const report = JSON.parse(readFileSync(reportPath, 'utf8'));
const archives = join(dirname(resolve(reportPath)), 'archives');
const expectedFiles = [
  'demo-settings.uipilot-plugin',
  'demo-settings-upgrade.uipilot-plugin',
  'demo-return.uipilot-plugin',
  'demo-multi.uipilot-plugin',
];
if (!Array.isArray(report.packages) || report.packages.length !== expectedFiles.length) {
  throw new Error('Expected four fixture archives in the macOS verification report.');
}

function checkArchiveHashes() {
  for (const file of expectedFiles) {
    const [entry, duplicate] = report.packages.filter(pkg => pkg?.file === file);
    if (!entry || duplicate || typeof entry.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(entry.sha256)) {
      throw new Error(`Missing or invalid hash entry for ${file}`);
    }
    const digest = createHash('sha256').update(readFileSync(join(archives, file))).digest('hex');
    if (digest !== entry.sha256) throw new Error(`SHA-256 mismatch for ${file}`);
  }
}

checkArchiveHashes();
console.log('All four archives match the macOS report SHA-256 hashes.');

if (!checkOnly) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const test = 'public_plugins::activation::tests::archive_management_matches_macos_fixtures';
  const result = spawnSync('cargo', [
    'test', '--locked', '--target', 'x86_64-pc-windows-msvc',
    '--manifest-path', 'src-tauri/Cargo.toml', '--lib', test, '--', '--ignored', '--exact',
  ], {
    cwd: root,
    env: { ...process.env, UIPILOT_ARCHIVE_TEST_INPUT: archives },
    encoding: 'utf8',
  });
  process.stdout.write(result.stdout ?? '');
  process.stderr.write(result.stderr ?? '');
  if (result.status !== 0 || !result.stdout?.includes('test result: ok. 1 passed;')) {
    throw new Error('Expected one successful Windows archive management test.', { cause: result.error });
  }
  checkArchiveHashes();
  console.log('Windows archive management passed. Runtime and installed-app acceptance are separate.');
}
