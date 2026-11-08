import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import crypto from 'node:crypto'
import { snapshotGraph, bundleSnapshot } from './bundle.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.resolve(here, '../..')
const output = process.argv[2]
if (!output || !path.isAbsolute(output)) throw new Error('expected build output directory')
const cases = [
  ['demo-return', 'examples/public-plugins/com.uipilot.demo-return/package', 'dist/runtime.js'],
  ['translate', 'examples/public-plugins/com.uipilot.translate/package', 'dist/runtime.js'],
  ['relative', 'spikes/macos-jsc-esm-probe/fixtures/relative', 'runtime.js'],
  ...['strict', 'private-channel', 'intrinsics', 'wrong-request', 'pending', 'infinite', 'oversized', 'throw']
    .map(name => [name, 'spikes/macos-jsc-esm-probe/fixtures', `${name}.js`]),
]
const runner = await fs.readFile(path.join(here, 'runner.js'))
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex')
function array(name, data) {
  const bytes = [...data]
  const rows = []
  for (let i = 0; i < bytes.length; i += 24) rows.push(bytes.slice(i, i + 24).join(','))
  return `static const unsigned char ${name}[] = {\n${rows.join(',\n')}\n};\n`
}
const evidence = {
  node: process.version,
  packageLockSha256: sha256(await fs.readFile(path.join(repo, 'package-lock.json'))),
  runnerSha256: sha256(runner),
  cases: [],
}
for (const [name, relativeRoot, entry] of cases) {
  const result = await bundleSnapshot(await snapshotGraph(path.join(repo, relativeRoot), entry))
  const dir = path.join(output, name)
  await fs.mkdir(dir)
  await fs.writeFile(path.join(dir, 'bundle.js'), result.code)
  await fs.writeFile(path.join(dir, 'fixture.h'), array('bundle_source', Buffer.from(result.code)) + array('runner_source', runner) + `static const BOOL execute_command = ${name === 'translate' ? 'NO' : 'YES'};\n`)
  evidence.cases.push({ name, relativeRoot, entry, ...result, code: undefined })
}
await fs.writeFile(path.join(output, 'modules.json'), `${JSON.stringify(evidence, null, 2)}\n`)
console.log(`Snapshotted and bundled ${cases.length} fixed module graphs; no package code executed in Node.`)
