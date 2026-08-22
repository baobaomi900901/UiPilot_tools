import { execFileSync } from 'node:child_process'
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { builtinModules } from 'node:module'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

import { build } from 'esbuild'

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = join(packageRoot, '..', '..')
const dist = join(packageRoot, 'dist')
const entry = join(packageRoot, 'src', 'bin.ts')
const output = join(dist, 'cli.mjs')
const builtinBridge = `
import assert from 'node:assert';
import buffer from 'node:buffer';
import stream from 'node:stream';
import util from 'node:util';
import zlib from 'node:zlib';
const __uipilotBuiltins = Object.freeze({ assert, buffer, stream, util, zlib });
const require = (name) => {
  if (Object.prototype.hasOwnProperty.call(__uipilotBuiltins, name)) return __uipilotBuiltins[name];
  throw new Error('Unsupported bundled module: ' + String(name));
};`

for (const script of [
  'sync-plugin-schema.mjs',
  'generate-schema-validator.mjs',
  'generate-unicode-data.mjs',
]) {
  execFileSync(process.execPath, [join(packageRoot, 'scripts', script), '--check'], {
    cwd: repositoryRoot,
    stdio: 'inherit',
  })
}

await rm(dist, { recursive: true, force: true })
await mkdir(dist, { recursive: true })
const result = await build({
  entryPoints: [entry],
  outfile: output,
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: ['node20'],
  packages: 'bundle',
  sourcemap: false,
  minify: false,
  metafile: true,
  legalComments: 'none',
  banner: { js: builtinBridge },
})

await copyFile(
  join(packageRoot, 'schema', 'uipilot-plugin-v1.schema.json'),
  join(dist, 'uipilot-plugin-v1.schema.json'),
)
await writeFile(join(dist, 'metafile.json'), `${JSON.stringify(result.metafile, null, 2)}\n`, 'utf8')

const allowedBuiltins = new Set([
  'assert',
  'buffer',
  'child_process',
  'crypto',
  'events',
  'fs',
  'fs/promises',
  'module',
  'os',
  'path',
  'stream',
  'string_decoder',
  'url',
  'util',
  'zlib',
])
const builtinNames = new Set(builtinModules.map((name) => name.replace(/^node:/u, '')))
for (const [inputPath, metadata] of Object.entries(result.metafile.inputs)) {
  const normalized = inputPath.replaceAll('\\', '/')
  if (normalized.includes('/node_modules/ajv/')) throw new Error('Ajv entered the runtime bundle')
  for (const imported of metadata.imports) {
    const name = imported.path.replace(/^node:/u, '')
    if (imported.kind === 'dynamic-import') {
      throw new Error(`dynamic import entered the runtime bundle: ${normalized}`)
    }
    if (imported.external && !builtinNames.has(name)) {
      throw new Error(`external runtime dependency entered the bundle: ${imported.path}`)
    }
    if (builtinNames.has(name) && !allowedBuiltins.has(name)) {
      throw new Error(`disallowed Node builtin in bundle: ${name}`)
    }
    if (name === 'child_process' && !normalized.endsWith('src/windows-attributes.ts')) {
      throw new Error(`child_process imported outside Windows adapter: ${normalized}`)
    }
  }
}

const bundled = await readFile(output, 'utf8')
if (!bundled.startsWith('#!/usr/bin/env node\n') || bundled.indexOf('#!/usr/bin/env node', 2) !== -1) {
  throw new Error('bundle must contain exactly one leading Node shebang')
}
for (const pattern of [
  /\bFunction\s*\(/u,
  /\beval\s*\(/u,
  /\bfetch\s*\(/u,
  /\bWebSocket\b/u,
  /\bEventSource\b/u,
  /\bprocess\.binding\s*\(/u,
  /\bcreateRequire\s*\(/u,
]) {
  if (pattern.test(bundled)) throw new Error(`disallowed runtime capability: ${pattern}`)
}

const outputMetadata = result.metafile.outputs[relative(packageRoot, output).replaceAll('\\', '/')]
if (!outputMetadata) throw new Error('bundle output is missing from metafile')
