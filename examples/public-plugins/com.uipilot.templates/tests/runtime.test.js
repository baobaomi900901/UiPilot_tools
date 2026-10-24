import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { onCommand, onPanelAction } from '../package/dist/runtime.js'

const key = 'templates.catalog-v1'
const context = { platform: 'windows', theme: 'dark', invokedAt: '2026-10-25T12:00:00+08:00' }
function store(initial = null) {
  let saved = structuredClone(initial)
  let writes = 0
  let failing = false
  const api = { storage: {
    async get(requested) { assert.equal(requested, key); return structuredClone(saved) },
    async set(requested, value) { assert.equal(requested, key); writes += 1; if (failing) throw new Error('synthetic disk failure'); saved = structuredClone(value) },
  } }
  return { api, get saved() { return saved }, get writes() { return writes }, fail() { failing = true } }
}
async function action(state, name, payload) {
  const reply = await onPanelAction({ apiVersion: 1, requestId: 'request-1', action: name, payload: structuredClone(payload), context }, state.api)
  assert.equal(reply.requestId, 'request-1')
  return reply.data
}
const create = (overrides = {}) => ({ id: null, expectedRevision: null, title: '问候', body: 'Hello {{name}}', values: [], ...overrides })
const savedItem = (id, body = '{{name}}', revision = 1) => ({ id: `template-${id}`, revision, title: `模板 ${id}`, body, values: [] })

test('literal substitution discovers first-use variables, permits prototype names and never recursively expands', async () => {
  const state = store()
  const data = await action(state, 'preview', { body: '{{ name }} / {{name}}\n{{constructor}} {{prototype}}', values: [{ name: 'name', value: '<b>$& {{other}}</b>' }, { name: 'constructor', value: '甲' }, { name: 'prototype', value: '乙' }, { name: 'unused', value: 'discard' }] })
  assert.equal(data.ok, true)
  assert.deepEqual(data.variables, ['name', 'constructor', 'prototype'])
  assert.deepEqual(data.values.map((item) => item.name), data.variables)
  assert.equal(data.text, '<b>$& {{other}}</b> / <b>$& {{other}}</b>\n甲 乙')
  assert.equal(state.writes, 0)
})

test('missing sample differs from an explicit empty string; missing samples may be saved', async () => {
  const state = store()
  const absent = await action(state, 'preview', { body: 'A{{name}}B', values: [] })
  assert.equal(absent.code, 'missing-variable')
  assert.deepEqual(absent.missing, ['name'])
  const empty = await action(state, 'preview', { body: 'A{{name}}B', values: [{ name: 'name', value: '' }] })
  assert.equal(empty.ok, true)
  assert.equal(empty.text, 'AB')
  assert.equal((await action(state, 'save', create())).ok, true)
  assert.deepEqual(state.saved.templates[0].values, [])
})

test('malformed braces and unsupported expressions reject without writes', async () => {
  const state = store()
  for (const body of ['{{', '{name}', '{{name}', '{{}}', '{{a.b}}', '{{1name}}', '{{name}}}', '{{a b}}', '{{' + 'a'.repeat(33) + '}}']) {
    assert.equal((await action(state, 'preview', { body, values: [] })).code, 'invalid-syntax', body)
    assert.equal((await action(state, 'save', create({ body }))).code, 'invalid-syntax', body)
  }
  assert.equal(state.writes, 0)
})

test('strict payload/sample shapes and duplicates reject, including unused malformed entries', async () => {
  const state = store()
  assert.equal((await action(state, 'preview', { body: '', values: [], extra: 1 })).code, 'invalid-payload')
  for (const values of [{ name: 'a' }, [{ name: 'a', value: 'x' }, { name: 'a', value: '' }], [{ name: 'bad-name', value: '' }], [{ name: 'unused', value: 1 }], [{ name: 'a', value: '', extra: 1 }]]) {
    assert.equal((await action(state, 'preview', { body: '{{a}}', values })).ok, false)
  }
  assert.equal((await action(state, 'load', { id: 'template-1', extra: null })).code, 'invalid-payload')
  assert.equal((await action(state, 'other', null)).code, 'unknown-action')
  assert.equal(state.writes, 0)
})

test('save/load/restart use one catalog write and stable revision, while onCommand returns metadata only', async () => {
  const state = store()
  const first = await action(state, 'save', create({ values: [{ name: 'name', value: 'Ada' }, { name: 'unused', value: 'drop' }] }))
  assert.equal(first.id, 'template-1')
  assert.equal(first.revision, 1)
  assert.equal(state.writes, 1)
  const reopened = store(state.saved)
  const loaded = await action(reopened, 'load', { id: first.id })
  assert.equal(loaded.template.body, 'Hello {{name}}')
  assert.deepEqual(loaded.template.values, [{ name: 'name', value: 'Ada' }])
  const updated = await action(reopened, 'save', create({ id: first.id, expectedRevision: 1, body: 'Welcome {{name}}' }))
  assert.equal(updated.id, first.id)
  assert.equal(updated.revision, 2)
  assert.equal(reopened.saved.templates.length, 1)
  assert.equal(reopened.writes, 1)
  const list = await onCommand({ requestId: 'list', input: '问', context }, reopened.api)
  assert.deepEqual(list.data.templates, [{ id: 'template-1', revision: 2, title: '问候' }])
  assert.equal(Object.hasOwn(list.data.templates[0], 'body'), false)
})

test('revision conflict never overwrites; failed storage preserves the previous catalog', async () => {
  const state = store()
  await action(state, 'save', create())
  const before = structuredClone(state.saved)
  assert.equal((await action(state, 'save', create({ id: 'template-1', expectedRevision: 2, body: 'stale' }))).code, 'conflict')
  assert.deepEqual(state.saved, before)
  assert.equal(state.writes, 1)
  state.fail()
  assert.equal((await action(state, 'save', create({ id: 'template-1', expectedRevision: 1, body: 'new' }))).code, 'save-uncertain')
  assert.deepEqual(state.saved, before)
})

test('all saved items are validated, corruption is visible and never silently reset', async () => {
  for (const raw of [{ schemaVersion: 2, nextId: 1, templates: [] }, { schemaVersion: 1, nextId: 3, templates: [savedItem(1), { ...savedItem(2), revision: 0 }] }, { schemaVersion: 1, nextId: 2, templates: [savedItem(1), savedItem(1)] }, { schemaVersion: 1, nextId: 1, templates: [savedItem(1)] }]) {
    const state = store(raw)
    assert.equal((await action(state, 'save', create())).code, 'catalog-invalid')
    assert.equal((await onCommand({ requestId: 'list', input: '', context }, state.api)).data.code, 'catalog-invalid')
    assert.deepEqual(state.saved, raw)
    assert.equal(state.writes, 0)
  }
})

test('a storage rejection after commit remains uncertain rather than claiming nothing was saved', async () => {
  let persisted = null
  const state = { api: { storage: {
    async get() { return persisted },
    async set(_key, value) { persisted = structuredClone(value); throw new Error('synthetic lost acknowledgement') },
  } } }
  assert.equal((await action(state, 'save', create())).code, 'save-uncertain')
  assert.equal(persisted.templates.length, 1)
  const loaded = await action(state, 'load', { id: 'template-1' })
  assert.equal(loaded.ok, true)
  assert.equal(loaded.template.revision, 1)
})

test('limits count Unicode characters and UTF-8 bytes; count/revision exhaustion is closed', async () => {
  const state = store()
  assert.equal((await action(state, 'save', create({ title: '😀'.repeat(80), body: '' }))).ok, true)
  for (const input of [create({ title: '😀'.repeat(81) }), create({ title: '  ' }), create({ body: '汉'.repeat(2731) }), create({ body: Array.from({ length: 17 }, (_, index) => `{{a${index}}}`).join('') }), create({ values: [{ name: 'name', value: '😀'.repeat(257) }] })]) assert.equal((await action(state, 'save', input)).ok, false)
  const full = store({ schemaVersion: 1, nextId: 21, templates: Array.from({ length: 20 }, (_, index) => savedItem(index + 1)) })
  assert.equal((await action(full, 'save', create())).code, 'template-limit')
  const exhausted = store({ schemaVersion: 1, nextId: Number.MAX_SAFE_INTEGER, templates: [savedItem(1, '', Number.MAX_SAFE_INTEGER)] })
  assert.equal((await action(exhausted, 'save', create())).code, 'counter-exhausted')
  assert.equal((await action(exhausted, 'save', create({ id: 'template-1', expectedRevision: Number.MAX_SAFE_INTEGER }))).code, 'counter-exhausted')
  assert.equal(full.writes + exhausted.writes, 0)
})

test('aggregate escaped catalog capacity rejects without altering draft or prior catalog', async () => {
  const raw = { schemaVersion: 1, nextId: 11, templates: Array.from({ length: 10 }, (_, index) => savedItem(index + 1, '\0'.repeat(8192))) }
  const state = store(raw)
  const draft = create({ body: '\0'.repeat(8192) })
  const original = structuredClone(draft)
  assert.equal((await action(state, 'save', draft)).code, 'catalog-capacity')
  assert.deepEqual(draft, original)
  assert.deepEqual(state.saved, raw)
  assert.equal(state.writes, 0)
})

test('escaped preview is bounded by the full serialized reply as well as plain UTF-8 output', async () => {
  const state = store()
  const payload = { body: '{{a}}'.repeat(100), values: [{ name: 'a', value: '\0'.repeat(256) }] }
  const data = await action(state, 'preview', payload)
  assert.equal(data.code, 'response-limit')
  assert.ok(Buffer.byteLength(JSON.stringify({ requestId: 'request-1', data })) < 65536)
  const tooLong = await action(state, 'preview', { body: '{{a}}'.repeat(130), values: [{ name: 'a', value: 'a'.repeat(256) }] })
  assert.equal(tooLong.code, 'preview-limit')
})

test('installable panel has no direct storage/business/network bridge bypass', async () => {
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  assert.match(source, /panel\.request\(\{ action, payload \}\)/)
  assert.doesNotMatch(source, /\.storage\.|fetch\(|invoke\(|WebSocket|innerHTML/)
})
