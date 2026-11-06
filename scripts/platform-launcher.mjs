import { spawn, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { constants } from 'node:os';
import { pathToFileURL } from 'node:url';

const macTarget = 'aarch64-apple-darwin';
const macConfig = new URL('../src-tauri/tauri.macos.conf.json', import.meta.url);

function nativeCommandIndex(args) {
  const index = args.findIndex(arg => !arg.startsWith('-'));
  return ['dev', 'build', 'bundle'].includes(args[index]) ? index : -1;
}

function macTauriArguments(args, env, rustHost) {
  const commandIndex = nativeCommandIndex(args);
  if (commandIndex < 0) return args;
  if (rustHost !== macTarget) throw new Error('Install a native Rust toolchain with host ' + macTarget);
  if (env.CARGO_BUILD_TARGET && env.CARGO_BUILD_TARGET !== macTarget) {
    throw new Error('CARGO_BUILD_TARGET must match ' + macTarget);
  }
  let hasCliTarget = false;
  let cargoArguments = false;
  for (let index = commandIndex + 1; index < args.length; index++) {
    const arg = args[index];
    if (arg === '--') cargoArguments = true;
    let target;
    if (arg === '--target' || arg === '-t') target = args[++index];
    else if (arg.startsWith('--target=')) target = arg.slice('--target='.length);
    else if (arg.startsWith('-t') && arg.length > 2) target = arg.slice(2).replace(/^=/, '');
    else continue;

    if (target !== macTarget) throw new Error('The only supported macOS target is ' + macTarget);
    if (!cargoArguments) hasCliTarget = true;
  }
  return hasCliTarget ? args : [...args.slice(0, commandIndex + 1), '--target', macTarget, ...args.slice(commandIndex + 1)];
}

export function getLaunchCommand(entry, args, { platform, arch, env, rustHost }) {
  if (entry !== 'tauri' && entry !== 'dev') throw new Error('Unknown launcher entry: ' + entry);
  if (platform === 'win32') {
    const script = entry === 'dev' ? 'dev-with-everything' : 'tauri';
    return { file: 'powershell.exe', args: ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
      'scripts/' + script + '.ps1', ...args], env };
  }
  if (platform !== 'darwin' || arch !== 'arm64') {
    throw new Error('UiPilot requires Windows or native arm64 macOS.');
  }
  const config = JSON.parse(readFileSync(macConfig, 'utf8'));
  const macEnv = { ...env, MACOSX_DEPLOYMENT_TARGET: config.bundle.macOS.minimumSystemVersion };
  if (entry === 'dev') {
    return { file: process.execPath, args: ['node_modules/vite/bin/vite.js', ...args], env: macEnv };
  }

  return { file: process.execPath,
    args: ['node_modules/@tauri-apps/cli/tauri.js', ...macTauriArguments(args, macEnv, rustHost)], env: macEnv };
}

async function main() {
  const [entry, ...args] = process.argv.slice(2);
  let rustHost;
  if (process.platform === 'darwin' && entry === 'tauri' && nativeCommandIndex(args) >= 0) {
    const rust = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
    rustHost = rust.stdout?.match(/^host: (.+)$/m)?.[1].trim();
  }
  const command = getLaunchCommand(entry, args, { platform: process.platform, arch: process.arch,
    env: process.env, rustHost });
  const child = spawn(command.file, command.args, { stdio: 'inherit', env: command.env });
  const signals = ['SIGINT', 'SIGTERM'];
  const forward = signals.map(signal => () => child.kill(signal));
  signals.forEach((signal, index) => process.on(signal, forward[index]));
  try {
    return await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve(code ?? 128 + constants.signals[signal]));
    });
  } finally {
    signals.forEach((signal, index) => process.off(signal, forward[index]));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { process.exitCode = await main(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
