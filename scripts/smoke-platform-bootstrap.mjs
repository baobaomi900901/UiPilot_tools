// Run from an isolated test account without an existing UiPilot instance.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repo = fileURLToPath(new URL('../', import.meta.url));
const marker = '[UiPilot] main frontend ready';

export function waitForBootstrap(binary, args = [], { timeout = 60_000, log = console.log } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let ready = false;
    let timer;
    function startTimeout(stage) {
      clearTimeout(timer);
      timer = setTimeout(() => {
        child.kill();
        reject(new Error('Timeout waiting for ' + stage));
      }, timeout);
    }
    startTimeout('main frontend ready');
    for (const stream of [child.stdout, child.stderr]) {
      createInterface({ input: stream }).on('line', line => {
        log(line);
        if (line === marker && !ready) {
          ready = true;
          log('Ready (PID ' + child.pid + '). Choose Quit in this instance’s tray menu.');
          startTimeout('tray Quit');
        }
      });
    }
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('close', (code, signal) => {
      clearTimeout(timer);
      if (!ready || code !== 0) {
        reject(new Error('Bootstrap exited (ready=' + ready + ', status=' + (code ?? signal) + ')'));
      } else resolve();
    });
  });
}

async function main() {
  const [flag, platform, ...extra] = process.argv.slice(2);
  if (flag !== '--platform' || extra.length || !['macos', 'windows'].includes(platform)) {
    throw new Error('Usage: node scripts/smoke-platform-bootstrap.mjs --platform macos|windows');
  }
  if (process.platform !== (platform === 'macos' ? 'darwin' : 'win32')) {
    throw new Error('Run on the matching native OS');
  }
  const target = process.env.CARGO_TARGET_DIR ?? path.join(repo, 'src-tauri', 'target');
  const binary = platform === 'macos'
    ? path.join(target, 'aarch64-apple-darwin/release/bundle/macos/UiPilot_tools.app/Contents/MacOS/UiPilot_tools')
    : path.join(target, 'release/UiPilot_tools.exe');
  console.log('Launching ' + binary);
  await waitForBootstrap(binary);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { await main(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
