import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'
import { JSDOM } from 'jsdom'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
const logicUrl = new URL('../package/dist/notes-logic.js', import.meta.url)
const panelHtmlUrl = new URL('../package/dist/panel.html', import.meta.url)
const panelScriptPath = new URL('../package/dist/panel.js', import.meta.url)
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
let panelModuleSequence = 0

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

async function loadModule(url) {
  const source = await readFile(url, 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}

function installDialogPolyfill(window) {
  const proto = window.HTMLDialogElement?.prototype
  if (!proto) {
    return
  }
  if (typeof proto.showModal !== 'function') {
    proto.showModal = function showModal() {
      this.open = true
      this.setAttribute('open', '')
    }
  }
  if (typeof proto.close !== 'function') {
    proto.close = function close() {
      this.open = false
      this.removeAttribute('open')
    }
  }
}

async function loadPanel({ notes = sampleNotes } = {}) {
  const html = await readFile(panelHtmlUrl, 'utf8')
  const dom = new JSDOM(html, {
    url: 'https://notes.uipilot.invalid/',
    pretendToBeVisual: true,
  })
  installDialogPolyfill(dom.window)

  let hostKeyHandler = null
  let hostKeyRegisterCount = 0
  let focusHostInputCalls = 0
  let requestHideCalls = 0
  let panelReady = Promise.resolve()
  const store = new Map([['notes.entries', structuredClone(notes)]])

  dom.window.requestAnimationFrame = (callback) => {
    callback(0)
    return 1
  }
  dom.window.cancelAnimationFrame = () => {}

  dom.window.uipilotPluginPanel = {
    onHostKey(handler) {
      hostKeyRegisterCount += 1
      hostKeyHandler = handler
      return () => {
        hostKeyHandler = null
      }
    },
    onUpdate(handler) {
      panelReady = Promise.resolve().then(() =>
        handler({
          requestId: 'notes-request-1',
          input: '',
          platform: 'windows',
          theme: 'dark',
          invokedAt: '2026-08-26T12:00:00Z',
          sessionEpoch: '1',
          data: {},
        }),
      )
      return () => {}
    },
    focusHostInput: async () => {
      focusHostInputCalls += 1
    },
    requestHide: async () => {
      requestHideCalls += 1
    },
    storage: {
      async get(key) {
        return store.has(key) ? structuredClone(store.get(key)) : null
      },
      async set(key, value) {
        store.set(key, structuredClone(value))
      },
      async remove(key) {
        store.delete(key)
      },
    },
  }

  globalThis.window = dom.window
  globalThis.document = dom.window.document
  globalThis.HTMLElement = dom.window.HTMLElement
  globalThis.Element = dom.window.Element
  globalThis.Node = dom.window.Node

  panelModuleSequence += 1
  await import(`${panelScriptPath.href}?seq=${panelModuleSequence}`)
  await panelReady
  await new Promise((resolve) => setImmediate(resolve))

  return {
    window: dom.window,
    document: dom.window.document,
    hostKeyRegisterCount: () => hostKeyRegisterCount,
    hostKey(event) {
      return hostKeyHandler?.(event)
    },
    focusHostInputCalls: () => focusHostInputCalls,
    requestHideCalls: () => requestHideCalls,
    selectedId() {
      const active = dom.window.document.querySelector('.note-card.is-active')
      return active?.querySelector('[data-note-id]')?.dataset.noteId ?? null
    },
    editor() {
      return dom.window.document.querySelector('#editor-content')
    },
    dialogOpen(id) {
      return Boolean(dom.window.document.querySelector(id)?.open)
    },
    async flush() {
      await new Promise((resolve) => setImmediate(resolve))
      await new Promise((resolve) => setImmediate(resolve))
    },
    cleanup() {
      dom.window.close()
      delete globalThis.window
      delete globalThis.document
      delete globalThis.HTMLElement
      delete globalThis.Element
      delete globalThis.Node
    },
  }
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

test('manifest declares the fixed panel notes contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.notes')
  assert.equal(manifest.version, '1.1.0')
  assert.equal(manifest.minimumHostVersion, '0.3.1')
  assert.equal(manifest.command.defaultName, 'notes')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['ui.panel'])
  assert.deepEqual(manifest.panel, {
    entry: 'dist/panel.html',
    hostKeys: ['ArrowDown', 'ArrowUp', 'Primary+N'],
  })
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
    'uipilotPluginPanel.onHostKey',
    'uipilotPluginPanel.storage',
    'uipilotPluginPanel.focusHostInput()',
    'uipilotPluginPanel.requestHide()',
    'note-list-viewport',
    'note-card',
    'ArrowUp',
    'ArrowDown',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
  assert.equal([...source.matchAll(/uipilotPluginPanel\.onHostKey/g)].length, 1)
  assert.doesNotMatch(source, /handleEscapeKeydown[\s\S]*stopPropagation/)
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

test('registers exactly one onHostKey handler and routes host keys', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  assert.equal(panel.hostKeyRegisterCount(), 1)
  await panel.flush()

  const settled = panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '1',
  })
  assert.equal(settled, undefined)
  await panel.flush()
  assert.equal(panel.selectedId(), '1')

  panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '2',
  })
  await panel.flush()
  assert.equal(panel.selectedId(), '2')

  panel.hostKey({
    key: 'ArrowUp',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '3',
  })
  await panel.flush()
  assert.equal(panel.selectedId(), '1')

  panel.hostKey({
    key: 'n',
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '4',
  })
  await panel.flush()
  assert.equal(panel.dialogOpen('#new-dialog'), true)

  panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '5',
  })
  await panel.flush()
  assert.equal(panel.selectedId(), '1')
  assert.equal(panel.dialogOpen('#new-dialog'), true)
})

test('host key handler settles before unsaved dialog completes', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '1',
  })
  await panel.flush()
  panel.editor().value = 'dirty draft'
  assert.equal(panel.editor().value, 'dirty draft')

  const started = Date.now()
  const result = panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '2',
  })
  assert.equal(result, undefined)
  assert.ok(Date.now() - started < 50)
  await panel.flush()
  assert.equal(panel.dialogOpen('#unsaved-dialog'), true)

  panel.hostKey({
    key: 'n',
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '3',
  })
  await panel.flush()
  assert.equal(panel.dialogOpen('#new-dialog'), false)
  assert.equal(panel.dialogOpen('#unsaved-dialog'), true)
})

test('Escape arbitration covers dialogs, unsaved flows, and clean hide handoff', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  panel.document.querySelector('#new-btn').click()
  await panel.flush()
  assert.equal(panel.dialogOpen('#new-dialog'), true)
  const cancelNew = new panel.window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true,
  })
  panel.document.dispatchEvent(cancelNew)
  assert.equal(cancelNew.defaultPrevented, true)
  assert.equal(panel.dialogOpen('#new-dialog'), false)
  assert.equal(panel.requestHideCalls(), 0)

  panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '1',
  })
  await panel.flush()
  panel.editor().value = 'unsaved body'

  const dirtyEscape = new panel.window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true,
  })
  let preventedBeforeAwait = dirtyEscape.defaultPrevented
  panel.document.dispatchEvent(dirtyEscape)
  preventedBeforeAwait = dirtyEscape.defaultPrevented
  assert.equal(preventedBeforeAwait, true)
  await panel.flush()
  assert.equal(panel.dialogOpen('#unsaved-dialog'), true)
  assert.equal(panel.requestHideCalls(), 0)

  panel.document.querySelector('#unsaved-cancel-btn').click()
  await panel.flush()
  assert.equal(panel.dialogOpen('#unsaved-dialog'), false)
  assert.equal(panel.requestHideCalls(), 0)
  assert.ok(panel.editor().value.includes('unsaved'))

  const dirtyEscapeAgain = new panel.window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true,
  })
  panel.document.dispatchEvent(dirtyEscapeAgain)
  assert.equal(dirtyEscapeAgain.defaultPrevented, true)
  await panel.flush()
  panel.document.querySelector('#unsaved-discard-btn').click()
  await panel.flush()
  assert.equal(panel.requestHideCalls(), 1)

  const cleanPanel = await loadPanel()
  t.after(cleanPanel.cleanup)
  await cleanPanel.flush()
  const cleanEscape = new cleanPanel.window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true,
  })
  cleanPanel.document.dispatchEvent(cleanEscape)
  assert.equal(cleanEscape.defaultPrevented, false)
  assert.equal(cleanPanel.requestHideCalls(), 0)
})

test('Escape save path calls requestHide once after sync preventDefault', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  panel.hostKey({
    key: 'ArrowDown',
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '1',
  })
  await panel.flush()
  panel.editor().value = 'persist me'

  const escape = new panel.window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true,
  })
  panel.document.dispatchEvent(escape)
  assert.equal(escape.defaultPrevented, true)
  await panel.flush()
  panel.document.querySelector('#unsaved-form').requestSubmit()
  await panel.flush()
  assert.equal(panel.requestHideCalls(), 1)
})
