import assert from 'node:assert/strict'
import test from 'node:test'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { onCommand } from '../package/dist/runtime.js'
import { createFixtures } from '../scripts/create-fixtures.mjs'

const invoke = (input, commandId = 'recovery') => Object.freeze({ apiVersion: 1, requestId: 'synthetic-request', commandId, input,
  context: Object.freeze({ platform: 'windows', theme: 'dark', invokedAt: '2026-10-25T12:00:00+08:00' }) })
function host(initial = null, failure = '') {
  let stored = structuredClone(initial)
  const writes = []
  const api = Object.freeze({ storage: Object.freeze({
    async get() { if (failure === 'read') throw new Error('RAW_PRIVATE_ERROR'); return structuredClone(stored) },
    async set(key, value) {
      writes.push({ key, value: structuredClone(value) })
      if (failure !== 'before') stored = structuredClone(value)
      if (failure === 'before' || failure === 'after') throw new Error('RAW_PRIVATE_ERROR')
    },
    async remove() { assert.fail('removal is forbidden') },
  }), settings: Object.freeze({ async get() { return 'RECOVERY_SETTING_SENTINEL' } }) })
  return { api, writes, read: () => structuredClone(stored) }
}
const text = value => JSON.stringify(value)

test('missing/status/help never writes; explicit increment writes one complete snapshot and persists across reads', async () => {
  const h = host()
  for (const input of ['', 'status', 'unknown']) {
    const r = await onCommand(invoke(input), h.api)
    assert.equal(r.requestId, 'synthetic-request')
    assert.equal(Object.hasOwn(r.results[0], 'defaultAction'), false)
  }
  assert.deepEqual(h.writes, [])
  assert.equal(h.read(), null)
  assert.match(text(await onCommand(invoke('inc'), h.api)), /saved/)
  assert.deepEqual(h.writes, [{ key: 'recovery-snapshot', value: { schemaVersion: 1, count: 1 } }])
  assert.match(text(await onCommand(invoke('status'), h.api)), /计数 1/)
  assert.equal(h.writes.length, 1)
})

test('all malformed, unknown, newer and out-of-bounds snapshots remain byte-for-byte unchanged', async () => {
  for (const initial of [false, 0, '', [], {}, { schemaVersion: 2, count: 5 }, { schemaVersion: 99, count: 5 },
    { schemaVersion: 1, count: -1 }, { schemaVersion: 1, count: 1.5 }, { schemaVersion: 1, count: 1_000_001 },
    { schemaVersion: 1, count: 1, extra: 'sentinel' }, { count: 1 }]) {
    const h = host(initial), before = JSON.stringify(h.read())
    for (const input of ['status', 'inc']) assert.match(text(await onCommand(invoke(input), h.api)), /write-blocked/)
    assert.equal(JSON.stringify(h.read()), before)
    assert.equal(h.writes.length, 0)
  }
  const full = host({ schemaVersion: 1, count: 1_000_000 })
  assert.match(text(await onCommand(invoke('inc'), full.api)), /limit/)
  assert.equal(full.writes.length, 0)
})

test('both failed save and committed save with lost acknowledgement report uncertainty without retry', async () => {
  for (const failure of ['before', 'after']) {
    const h = host({ schemaVersion: 1, count: 4 }, failure)
    const r = await onCommand(invoke('inc'), h.api)
    assert.match(text(r), /save-uncertain/)
    assert.doesNotMatch(text(r), /RAW_PRIVATE_ERROR|已回滚|未保存/)
    assert.equal(h.writes.length, 1)
    assert.equal(h.read().count, failure === 'after' ? 5 : 4)
    assert.match(text(await onCommand(invoke('status'), h.api)), new RegExp(`计数 ${h.read().count}`))
    assert.equal(h.writes.length, 1)
  }
})

test('read failure and explicit Runtime throw never write; unsupported command identity fails before access', async () => {
  const h = host(null, 'read')
  assert.match(text(await onCommand(invoke('inc'), h.api)), /read-unavailable/)
  await assert.rejects(onCommand(invoke('throw'), h.api), /RECOVERY_ERROR_SENTINEL/)
  for (const id of [undefined, 'other', 'constructor']) {
    await assert.rejects(onCommand({ ...invoke('inc'), commandId: id }, h.api), /Unsupported command ID/)
  }
  assert.equal(h.writes.length, 0)
})

test('generated A/B, broken C, throwing T, D upgrade and disposable schema2 preserve stable identity and data', async t => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'uipilot-recovery-'))
  t.after(() => rm(temporary, { recursive: true, force: true }))
  const fixtures = await createFixtures(path.join(temporary, 'fixtures'))
  await assert.rejects(createFixtures(path.join(temporary, 'fixtures')), /EEXIST/)
  const modules = new Map()
  for (const fixture of fixtures) {
    const manifest = JSON.parse(await readFile(path.join(fixture.packageRoot, 'plugin.json'), 'utf8'))
    assert.equal(manifest.pluginId, 'com.uipilot.demo-recovery')
    assert.equal(manifest.commands[0].id, 'recovery')
    assert.equal(manifest.version, fixture.id === 'D-upgrade' ? '1.0.1' : '1.0.0')
    assert.deepEqual(manifest.permissions, [])
    const runtime = await import(pathToFileURL(path.join(fixture.packageRoot, 'dist/runtime.js')).href)
    modules.set(fixture.id, runtime)
  }
  assert.equal(modules.get('C-readiness-broken').onCommand, undefined)
  const h = host({ schemaVersion: 1, count: 7 })
  for (const id of ['A', 'B', 'D-upgrade']) {
    assert.match(text(await modules.get(id).onCommand(invoke('status'), h.api)), new RegExp(`构建 ${id}`))
    assert.equal(h.read().count, 7)
  }
  await assert.rejects(modules.get('T-controlled-throw').onCommand(invoke('throw'), h.api), /RECOVERY_ERROR_SENTINEL/)
  const schema2 = modules.get('V2-disposable')
  await schema2.onCommand(invoke('status'), h.api)
  assert.equal(h.writes.length, 0)
  await schema2.onCommand(invoke('seed-v2'), h.api)
  const snapshot = JSON.stringify(h.read())
  assert.deepEqual(h.read(), { schemaVersion: 2, count: 7, sentinel: 'RECOVERY_PRIVATE_SENTINEL_V2' })
  assert.match(text(await onCommand(invoke('inc'), h.api)), /write-blocked/)
  assert.match(text(await schema2.onCommand(invoke('inspect-v2'), h.api)), /RECOVERY_PRIVATE_SENTINEL_V2/)
  assert.equal(JSON.stringify(h.read()), snapshot)
  assert.equal(h.writes.length, 1)
  const malformed = host({ schemaVersion: 3, count: 7 })
  assert.match(text(await schema2.onCommand(invoke('seed-v2'), malformed.api)), /write-blocked/)
  assert.equal(malformed.writes.length, 0)
})

