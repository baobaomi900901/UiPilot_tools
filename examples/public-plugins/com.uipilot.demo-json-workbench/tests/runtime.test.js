import assert from 'node:assert/strict'
import test from 'node:test'
import { onCommand, onWindowAction } from '../package/dist/runtime.js'

const key = 'workbench.draft'
const context = { platform: 'windows', theme: 'dark', invokedAt: '2026-10-25T12:00:00+08:00' }
function store(initial = null, mode = '') {
  let saved = structuredClone(initial)
  let writes = 0
  let reads = 0
  const api = { storage: {
    async get(requested) {
      assert.equal(requested, key)
      reads += 1
      if (mode === 'read-failure') throw new Error('synthetic read failure')
      return structuredClone(saved)
    },
    async set(requested, value) {
      assert.equal(requested, key)
      writes += 1
      if (mode === 'write-failure') throw new Error('synthetic write failure')
      saved = structuredClone(value)
      if (mode === 'lost-ack') throw new Error('synthetic lost acknowledgement')
    },
  } }
  return { api, get saved() { return saved }, get writes() { return writes }, get reads() { return reads } }
}
async function action(state, name, payload) {
  const reply = await onWindowAction({ apiVersion: 1, requestId: 'action-1', action: name, payload, context }, state.api)
  assert.deepEqual(Object.keys(reply).sort(), ['data', 'requestId'])
  assert.equal(reply.requestId, 'action-1')
  return reply.data
}
const record = (text = '', revision = 1) => ({ schemaVersion: 1, revision, text })

test('onCommand only loads the saved draft and preserves request ownership', async () => {
  const state = store(record(' {\n "name": "甲"\n}\n', 4))
  const reply = await onCommand({ apiVersion: 1, requestId: 'open-1', input: 'ignored', context }, state.api)
  assert.equal(reply.requestId, 'open-1')
  assert.deepEqual(reply.data, { ok: true, revision: 4, text: ' {\n "name": "甲"\n}\n', hasSaved: true })
  assert.equal(state.reads, 1)
  assert.equal(state.writes, 0)
})

test('missing data loads revision zero without creating or resetting storage', async () => {
  const state = store()
  assert.deepEqual(await action(state, 'load', null), { ok: true, revision: 0, text: '', hasSaved: false })
  assert.equal(state.writes, 0)
  assert.equal(state.saved, null)
})

test('format handles JSON primitives and nested data without storage access', async () => {
  const state = store()
  for (const text of ['{"a":[1,true,null,"甲"]}', '1', 'false', 'null', '"hello"', '{"__proto__":1,"constructor":2}']) {
    assert.deepEqual(await action(state, 'format', { text }), { ok: true, formatted: JSON.stringify(JSON.parse(text), null, 2) })
  }
  assert.equal(state.reads + state.writes, 0)
})

test('format rejects malformed syntax and nonfinite numbers instead of converting to null', async () => {
  const state = store()
  for (const text of ['', '{', 'NaN', '{"a":}', '[1,]']) assert.equal((await action(state, 'format', { text })).code, 'invalid-json')
  for (const text of ['1e400', '-1e400', '{"a":[1e400]}']) assert.equal((await action(state, 'format', { text })).code, 'non-finite-number')
  assert.equal(state.writes, 0)
})

test('format enforces depth and total value limits at the boundary', async () => {
  const state = store()
  const nested = (depth) => '['.repeat(depth) + '0' + ']'.repeat(depth)
  assert.equal((await action(state, 'format', { text: nested(16) })).ok, true)
  assert.equal((await action(state, 'format', { text: nested(17) })).code, 'depth-limit')
  assert.equal((await action(state, 'format', { text: JSON.stringify(Array(2047).fill(0)) })).ok, true)
  assert.equal((await action(state, 'format', { text: JSON.stringify(Array(2048).fill(0)) })).code, 'value-limit')
})

test('text limit counts UTF-8 bytes and includes boundary whitespace', async () => {
  const state = store()
  assert.equal((await action(state, 'save', { text: '😀'.repeat(2048), expectedRevision: 0 })).ok, true)
  assert.equal(Buffer.byteLength(state.saved.text), 8192)
  assert.equal((await action(state, 'save', { text: '汉'.repeat(2731), expectedRevision: 1 })).code, 'text-limit')
  assert.equal((await action(state, 'format', { text: ' '.repeat(8192) + '0' })).code, 'text-limit')
  assert.equal((await action(state, 'format', { text: ' '.repeat(8191) + '0' })).ok, true)
  assert.equal(state.writes, 1)
})

test('formatted output is bounded before returning a response', async () => {
  const state = store()
  const text = '['.repeat(15) + JSON.stringify(Array(1200).fill(0)) + ']'.repeat(15)
  const reply = await action(state, 'format', { text })
  assert.deepEqual(reply, { ok: false, code: 'formatted-limit' })
})

test('payloads have exact fields, correct types and safe expected revisions', async () => {
  const state = store()
  for (const payload of [{}, { text: '', extra: null }, { text: 1 }, [], null]) assert.equal((await action(state, 'format', payload)).code, 'invalid-payload')
  for (const payload of [{}, { text: '', expectedRevision: -1 }, { text: '', expectedRevision: 0.5 }, { text: '', expectedRevision: Number.MAX_SAFE_INTEGER + 1 }, { text: '', expectedRevision: '0' }, { text: '', expectedRevision: 0, extra: null }]) assert.equal((await action(state, 'save', payload)).code, 'invalid-payload')
  for (const payload of [{}, [], false]) assert.equal((await action(state, 'load', payload)).code, 'invalid-payload')
  assert.deepEqual(await action(state, 'other', null), { ok: false, code: 'unknown-action' })
  assert.equal(state.writes, 0)
})

test('save preserves empty, incomplete and whitespace drafts with one complete record write', async () => {
  const state = store()
  for (const [expectedRevision, text] of ['', '  {"incomplete":\n', '\n  \n'].entries()) {
    assert.deepEqual(await action(state, 'save', { text, expectedRevision }), { ok: true, revision: expectedRevision + 1, text })
    assert.deepEqual(state.saved, record(text, expectedRevision + 1))
  }
  assert.equal(state.writes, 3)
  const reopened = store(state.saved)
  assert.deepEqual(await action(reopened, 'load', null), { ok: true, revision: 3, text: '\n  \n', hasSaved: true })
})

test('stale revision never overwrites and revision exhaustion fails closed', async () => {
  const state = store(record('existing', 3))
  assert.equal((await action(state, 'save', { text: 'stale', expectedRevision: 2 })).code, 'revision-conflict')
  assert.deepEqual(state.saved, record('existing', 3))
  const full = store(record('existing', Number.MAX_SAFE_INTEGER))
  assert.equal((await action(full, 'save', { text: 'new', expectedRevision: Number.MAX_SAFE_INTEGER })).code, 'revision-exhausted')
  assert.equal(state.writes + full.writes, 0)
})

test('invalid or newer stored records are visible and never reset or overwritten', async () => {
  for (const raw of [{}, record('', 0), record('', -1), record('', 1.2), record('', Number.MAX_SAFE_INTEGER + 1), { ...record(), schemaVersion: 2 }, { ...record(), extra: true }, { ...record(), text: null }, record('汉'.repeat(2731)), false, []]) {
    const state = store(raw)
    assert.equal((await action(state, 'load', null)).code, 'invalid-stored-data')
    assert.equal((await action(state, 'save', { text: 'new', expectedRevision: 0 })).code, 'invalid-stored-data')
    assert.equal((await onCommand({ requestId: 'open', input: '', context }, state.api)).data.code, 'invalid-stored-data')
    assert.deepEqual(state.saved, raw)
    assert.equal(state.writes, 0)
  }
})

test('read failure returns a fixed code without writing or exposing exception details', async () => {
  const state = store(null, 'read-failure')
  assert.deepEqual(await action(state, 'load', null), { ok: false, code: 'storage-unavailable' })
  assert.deepEqual(await action(state, 'save', { text: 'new', expectedRevision: 0 }), { ok: false, code: 'storage-unavailable' })
  assert.equal(state.writes, 0)
})

test('save failure never retries and lost acknowledgement stays uncertain until explicit load', async () => {
  for (const mode of ['write-failure', 'lost-ack']) {
    const state = store(null, mode)
    assert.deepEqual(await action(state, 'save', { text: '{draft', expectedRevision: 0 }), { ok: false, code: 'save-uncertain' })
    assert.equal(state.writes, 1)
    assert.equal(state.reads, 1)
    const loaded = await action(state, 'load', null)
    assert.equal(loaded.revision, mode === 'lost-ack' ? 1 : 0)
    assert.equal(loaded.text, mode === 'lost-ack' ? '{draft' : '')
  }
})
