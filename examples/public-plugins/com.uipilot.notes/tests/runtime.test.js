import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
const logicUrl = new URL('../package/dist/notes-logic.js', import.meta.url)
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

async function loadModule(url) {
  const source = await readFile(url, 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}

const invocation = Object.freeze({
  apiVersion: 1,
  requestId: 'notes-request-1',
  input: 'hello',
  context: Object.freeze({
    platform: 'windows',
    theme: 'dark',
    invokedAt: '2026-08-25T12:00:00Z',
  }),
})

const sampleNotes = Object.freeze([
  Object.freeze({
    id: '1',
    title: 'Project Plan',
    content: 'Ship the panel mode',
    createdAt: '2026-08-25T10:00:00.000Z',
  }),
  Object.freeze({
    id: '2',
    title: 'Grocery',
    content: 'Milk and eggs',
    createdAt: '2026-08-25T09:00:00.000Z',
  }),
])

test('manifest declares the fixed panel notes contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.notes')
  assert.equal(manifest.version, '1.0.0')
  assert.equal(manifest.minimumHostVersion, '0.3.0')
  assert.equal(manifest.command.defaultName, 'notes')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['ui.panel'])
  assert.deepEqual(manifest.panel, { entry: 'dist/panel.html' })
  assert.equal('window' in manifest, false)
})

test('strict package root contains only notes panel assets', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual(
    (await readdir(new URL('dist/', packageRoot))).sort(),
    ['notes-logic.js', 'panel.css', 'panel.html', 'panel.js', 'runtime.js'],
  )
})

test('runtime returns a panel response and preserves requestId', async () => {
  const runtime = await loadModule(runtimeUrl)
  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'notes-request-1',
    data: {},
  })
})

test('onUpdate.input filters titles case-insensitively', async () => {
  const { filterNotes } = await loadModule(logicUrl)
  const matched = filterNotes(sampleNotes, 'project')
  assert.deepEqual(
    matched.map((note) => note.id),
    ['1'],
  )
})

test('onUpdate.input filters content case-insensitively', async () => {
  const { filterNotes } = await loadModule(logicUrl)
  const matched = filterNotes(sampleNotes, 'MILK')
  assert.deepEqual(
    matched.map((note) => note.id),
    ['2'],
  )
})

test('empty query shows all notes', async () => {
  const { filterNotes } = await loadModule(logicUrl)
  assert.equal(filterNotes(sampleNotes, '').length, 2)
  assert.equal(filterNotes(sampleNotes, '   ').length, 2)
})

test('normalizeNotes drops invalid records safely', async () => {
  const { normalizeNotes } = await loadModule(logicUrl)
  const normalized = normalizeNotes([
    null,
    { id: 'ok', title: 'Keep', content: 'body', createdAt: '2026-08-25T08:00:00.000Z' },
    { title: '   ' },
    { title: 'Legacy', content: 12, updatedAt: '2026-08-24T08:00:00.000Z' },
  ])
  assert.equal(normalized.length, 2)
  assert.equal(normalized[0].title, 'Keep')
  assert.equal(normalized[1].title, 'Legacy')
  assert.equal(normalized[1].content, '')
  assert.equal(normalized[1].createdAt, '2026-08-24T08:00:00.000Z')
})

test('storage helpers persist through get set and remove contracts', async () => {
  const panel = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  const logic = await readFile(new URL('../package/dist/notes-logic.js', import.meta.url), 'utf8')
  assert.match(logic, /notes\.entries/)
  assert.match(panel, /STORAGE_KEY/)
  assert.match(panel, /uipilotPluginPanel\.storage/)
  assert.match(panel, /await storage\.set\(STORAGE_KEY, notes\)/)
  assert.match(panel, /await storage\.get\(STORAGE_KEY\)/)

  const store = new Map()
  const storage = {
    async get(key) {
      return store.has(key) ? structuredClone(store.get(key)) : null
    },
    async set(key, value) {
      store.set(key, structuredClone(value))
    },
    async remove(key) {
      store.delete(key)
    },
  }
  const { STORAGE_KEY, normalizeNotes } = await loadModule(logicUrl)
  const entries = normalizeNotes([
    { id: 'a', title: 'Alpha', content: 'one', createdAt: '2026-08-25T08:00:00.000Z' },
  ])
  await storage.set(STORAGE_KEY, entries)
  assert.deepEqual(await storage.get(STORAGE_KEY), entries)
  await storage.remove(STORAGE_KEY)
  assert.equal(await storage.get(STORAGE_KEY), null)
})

test('Ctrl+F calls focusHostInput without arguments', async () => {
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginPanel.focusHostInput()',
    "event.ctrlKey || event.metaKey",
    "event.key.toLowerCase() === 'f'",
    'event.preventDefault()',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
  assert.doesNotMatch(source, /focusHostInput\([^)]+\)/)
})

test('panel has no in-panel search box and uses host input for filtering', async () => {
  const html = await readFile(new URL('../package/dist/panel.html', import.meta.url), 'utf8')
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  assert.doesNotMatch(html, /search-input|type="search"/)
  assert.doesNotMatch(source, /#search-input|searchInput/)
  assert.match(source, /searchQuery = typeof update\.input === 'string' \? update\.input : ''/)
  assert.match(source, /update\.theme/)
})

test('panel content uses only the panel bridge APIs', async () => {
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginPanel.onUpdate',
    'uipilotPluginPanel.storage',
    'uipilotPluginPanel.focusHostInput()',
    'note-list-viewport',
    'note-card',
    'ArrowUp',
    'ArrowDown',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
  for (const forbidden of [
    'invoke(',
    'fetch(',
    'WebSocket',
    'uipilotPluginWindow',
    'timer',
    'notifications',
  ]) {
    assert.doesNotMatch(source, new RegExp(forbidden.replace('(', '\\(')))
  }
})
