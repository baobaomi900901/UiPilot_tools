import assert from 'node:assert/strict'
import { readFile, mkdtemp, readdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { execFileSync } from 'node:child_process'
import test from 'node:test'
const root = new URL('../', import.meta.url)
const load = async (url) => import('data:text/javascript;base64,' + Buffer.from(await readFile(url, 'utf8')).toString('base64'))
test('offline settings demo reads all current settings on every command without side effects', async () => {
  const { onCommand } = await load(new URL('package/runtime.js', root))
  const manifest = JSON.parse(await readFile(new URL('package/plugin.json', root), 'utf8'))
  assert.deepEqual(manifest.permissions, []); assert.equal(manifest.command.defaultName, 'settings-demo'); assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'mainResult'); assert.equal(manifest.minimumHostVersion, '0.2.0')
  assert.deepEqual(manifest.settings.map(s => s.type), ['text', 'number', 'boolean', 'select'])
  const values = Object.fromEntries(manifest.settings.map(s => [s.key, s.default]))
  const reads = []
  const api = new Proxy({ settings: { async get(key) { reads.push(key); return values[key] } } }, { get(target, key) { assert.equal(key, 'settings'); return target[key] } })
  const first = await onCommand({ requestId: 'one' }, api)
  assert.equal(first.requestId, 'one'); assert.equal(first.results[0].title, '你好 × 2'); assert.match(first.results[0].subtitle, /uppercase=false · style=plain · build 1.0.0/)
  Object.assign(values, { greeting: '人工验收', count: 3, uppercase: true, style: 'boxed' })
  const second = await onCommand({ requestId: 'two' }, api)
  assert.equal(second.results[0].title, '【人工验收 × 3】'); assert.match(second.results[0].subtitle, /uppercase=true · style=boxed · build 1.0.0/)
  assert.deepEqual(reads, [...manifest.settings.map(s => s.key), ...manifest.settings.map(s => s.key)])
  assert.deepEqual(Object.keys(second.results[0]).sort(), ['id', 'subtitle', 'title'])
  values.greeting = '<b>Text</b>'; assert.equal((await onCommand({ requestId: 'three' }, api)).results[0].title, '【<B>TEXT</B> × 3】')
})
test('compatible upgrade changes only the manifest version and visible build mark', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'uipilot-settings-fixture-'))
  execFileSync(process.execPath, [new URL('prepare-upgrade.mjs', root).pathname.replace(/^\/(\w:)/, '$1'), directory])
  const original = JSON.parse(await readFile(new URL('package/plugin.json', root), 'utf8'))
  const upgraded = JSON.parse(await readFile(join(directory, 'plugin.json'), 'utf8'))
  assert.equal(upgraded.version, '1.0.1'); assert.deepEqual({ ...upgraded, version: original.version }, original)
  const runtime = await load(new URL('file:///' + join(directory, 'runtime.js').replaceAll('\\', '/')))
  const values = { greeting: 'Keep', count: 4, uppercase: false, style: 'plain' }
  const result = await runtime.onCommand({ requestId: 'upgrade' }, { settings: { async get(key) { return values[key] } } })
  assert.equal(result.results[0].title, 'Keep × 4'); assert.match(result.results[0].subtitle, /build 1.0.1/)
  assert.deepEqual((await readdir(directory)).sort(), ['plugin.json', 'runtime.js'])
})
