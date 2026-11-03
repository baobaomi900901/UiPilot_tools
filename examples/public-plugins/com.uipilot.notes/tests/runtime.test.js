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
  const statuses = []
  let actionsProvider = null
  let panelReady = Promise.resolve()
  const store = new Map([['notes.entries', structuredClone(notes)]])

  dom.window.requestAnimationFrame = (callback) => {
    callback(0)
    return 1
  }
  dom.window.cancelAnimationFrame = () => {}
  dom.window.document.execCommand = () => true

  dom.window.uipilotPluginPanel = {
    onActions(provider) { actionsProvider = provider; return () => { actionsProvider = null } },
    async setStatus(value) { statuses.push(structuredClone(value)) },
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
      return undefined
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
  globalThis.HTMLButtonElement = dom.window.HTMLButtonElement
  globalThis.Element = dom.window.Element
  globalThis.Node = dom.window.Node

  panelModuleSequence += 1
  await import(`${panelScriptPath.href}?seq=${panelModuleSequence}`)
  await panelReady
  await new Promise((resolve) => setImmediate(resolve))

  return {
    actions(source = 'content') { return actionsProvider?.({ source }) },
    statuses,
    window: dom.window,
    document: dom.window.document,
    hostKeyRegisterCount: () => hostKeyRegisterCount,
    hostKey(event) {
      return hostKeyHandler?.(event)
    },
    focusHostInputCalls: () => focusHostInputCalls,
    requestHideCalls: () => requestHideCalls,
    storedNotes() {
      return structuredClone(store.get('notes.entries') ?? [])
    },
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
      delete globalThis.HTMLButtonElement
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
  assert.equal(manifest.version, '1.2.10')
  assert.equal(manifest.minimumHostVersion, '0.16.0')
  assert.equal(manifest.command.defaultName, 'notes')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.equal(manifest.command.inputRequired, false)
  assert.equal(manifest.command.inputPlaceholder, '搜索目录')
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

test('browser preview reuses panel assets and provides a local Host bridge', async () => {
  const [html, source] = await Promise.all([
    readFile(new URL('../preview.html', import.meta.url), 'utf8'),
    readFile(new URL('../preview.js', import.meta.url), 'utf8'),
  ])

  assert.match(html, /<script type="module" src="\.\/preview\.js"><\/script>/)
  for (const required of [
    "fetch('./package/dist/panel.html')",
    "import('./package/dist/panel.js')",
    "Object.defineProperty(window, 'uipilotPluginPanel'",
    "'notes.entries'",
    '--uipilot-color-${name}',
    'onHostKey',
    'onUpdate',
    'focusHostInput',
    'requestHide',
    'storage',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
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

test('sortNotes places most recently pinned notes first and preserves ordinary order', async () => {
  const { sortNotes } = await loadModule(logicUrl)
  const sorted = sortNotes([
    { id: 'ordinary-new', createdAt: '2026-08-27T10:00:00.000Z' },
    { id: 'pinned-old', createdAt: '2026-08-27T08:00:00.000Z', pinnedAt: '2026-08-27T11:00:00.000Z' },
    { id: 'ordinary-old', createdAt: '2026-08-27T09:00:00.000Z' },
    { id: 'pinned-new', createdAt: '2026-08-27T07:00:00.000Z', pinnedAt: '2026-08-27T12:00:00.000Z' },
  ])

  assert.deepEqual(sorted.map((note) => note.id), [
    'pinned-new',
    'pinned-old',
    'ordinary-new',
    'ordinary-old',
  ])
})

test('sidebar toolbar renders a directory title and icon-only new action', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  const toolbar = panel.document.querySelector('.toolbar')
  assert.equal(toolbar.textContent.includes('主输入框搜索'), false)
  assert.equal(toolbar.querySelector('.toolbar-hint'), null)
  assert.equal(toolbar.querySelector('.toolbar-title')?.textContent, '目录')

  const newButton = toolbar.querySelector('#new-btn')
  assert.equal(newButton.getAttribute('aria-label'), '新建笔记')
  assert.equal(newButton.textContent.trim(), '')
  assert.ok(newButton.querySelector('svg[aria-hidden="true"]'))

  const css = await readFile(new URL('../package/dist/panel.css', import.meta.url), 'utf8')
  const style = panel.document.createElement('style')
  style.textContent = css
  panel.document.head.append(style)
  const newButtonStyle = panel.window.getComputedStyle(newButton)
  const newButtonIconStyle = panel.window.getComputedStyle(newButton.querySelector('svg'))

  assert.match(css, /\.toolbar\s*\{[\s\S]*justify-content:\s*space-between;/)
  assert.match(css, /\.btn\.new-btn\s*\{[^}]*color:\s*var\(--note-canvas\);/s)
  assert.equal(newButtonStyle.padding, '0px')
  assert.equal(newButtonStyle.lineHeight, '0')
  assert.equal(newButtonIconStyle.display, 'block')
  assert.match(css, /\.note-list\s*\{[^}]*padding:\s*6px 14px 6px 6px;/)
  assert.match(css, /\.note-item\s*\{[^}]*padding:\s*0;/)
})

test('panel shell delegates the outer separator to Host and keeps directory selection styling', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  const emptyIcon = panel.document.querySelector('.empty-icon')
  assert.equal(emptyIcon.getAttribute('aria-hidden'), 'true')
  assert.ok(emptyIcon.querySelector('svg'))
  assert.equal(emptyIcon.textContent.trim(), '')

  const css = await readFile(new URL('../package/dist/panel.css', import.meta.url), 'utf8')
  assert.doesNotMatch(css.match(/\.app\s*\{[^}]*\}/s)?.[0] ?? '', /border-top:/)
  assert.match(css, /:root\s*\{[^}]*--note-selection-surface:\s*var\(--note-surface\);/s)
  assert.match(css, /@supports[^}]*--note-selection-surface:\s*color-mix\(in srgb, var\(--uipilot-color-text[^)]*\) 13%, var\(--note-canvas\)\);/s)
  assert.match(css, /\.note-card\.is-active\s*\{[^}]*background:\s*var\(--note-selection-surface\);[^}]*box-shadow:\s*inset 0 0 0 1px var\(--note-selection-border\);/s)
  assert.match(css, /\.empty-icon svg\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;/s)
})

test('panel content uses only the panel bridge APIs', async () => {
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginPanel.onUpdate',
    'uipilotPluginPanel.onHostKey',
    'uipilotPluginPanel.storage',
    'uipilotPluginPanel.focusHostInput()',
    'uipilotPluginPanel.requestHide()',
    'hidePanelAfterListCopy',
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

test('new-note dialog is concise and places cancel before save', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  panel.hostKey({
    key: 'n',
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    sessionEpoch: '1',
    routeSequence: '1',
  })
  await panel.flush()

  const dialog = panel.document.querySelector('#new-dialog')
  const input = panel.document.querySelector('#new-title-input')
  assert.equal(panel.document.activeElement, input)
  assert.equal(dialog.textContent.includes('为这条笔记起一个目录名'), false)
  assert.equal(dialog.querySelector('label[for="new-title-input"]'), null)
  assert.equal(input.getAttribute('aria-label'), '笔记名称')

  const actions = [...dialog.querySelectorAll('.dialog-actions button')]
  assert.deepEqual(actions.map((button) => button.id), ['new-cancel-btn', 'new-save-btn'])
  assert.equal(actions[1].disabled, false)
  assert.equal(actions[1].getAttribute('aria-disabled'), 'true')

  input.value = 'Meeting notes'
  input.dispatchEvent(new panel.window.Event('input', { bubbles: true }))
  assert.equal(actions[1].getAttribute('aria-disabled'), 'false')
})

test('note item menu renames, pins newest-first, and requires delete confirmation', async (t) => {
  const panel = await loadPanel({
    notes: [
      { id: '1', title: 'First', content: 'one', createdAt: '2026-08-27T10:00:00.000Z' },
      { id: '2', title: 'Second', content: 'two', createdAt: '2026-08-27T09:00:00.000Z' },
      { id: '3', title: 'Third', content: 'three', createdAt: '2026-08-27T08:00:00.000Z' },
    ],
  })
  t.after(panel.cleanup)
  await panel.flush()

  assert.equal(panel.document.querySelector('.note-delete'), null)
  const moreFor = (noteId) => panel.document.querySelector(`[data-note-id="${noteId}"].note-more`)
  assert.equal(moreFor('2')?.tabIndex, -1)
  assert.ok(moreFor('2')?.querySelector('svg[aria-hidden="true"]'))

  panel.document.querySelector('#note-list').focus()
  const focusedMore = moreFor('2')
  focusedMore.focus()
  focusedMore.click()
  const menu = panel.document.querySelector('#note-actions-menu')
  assert.equal(menu.hidden, false)
  assert.deepEqual(
    [...menu.querySelectorAll('[role="menuitem"]')].map((item) => item.textContent.trim()),
    ['重命名', '置顶', '删除'],
  )

  menu.querySelector('[data-note-action="rename"]').click()
  assert.equal(panel.dialogOpen('#rename-dialog'), true)
  const renameInput = panel.document.querySelector('#rename-title-input')
  assert.equal(renameInput.value, 'Second')
  renameInput.value = 'Renamed'
  renameInput.dispatchEvent(new panel.window.Event('input', { bubbles: true }))
  panel.document.querySelector('#rename-form').dispatchEvent(new panel.window.Event('submit', {
    bubbles: true,
    cancelable: true,
  }))
  await panel.flush()
  assert.equal(panel.storedNotes().find((note) => note.id === '2')?.title, 'Renamed')

  moreFor('2').click()
  menu.querySelector('[data-note-action="pin"]').click()
  await panel.flush()
  assert.equal(panel.storedNotes()[0]?.id, '2')
  assert.equal(typeof panel.storedNotes()[0]?.pinnedAt, 'string')

  moreFor('1').click()
  menu.querySelector('[data-note-action="pin"]').click()
  await panel.flush()
  assert.deepEqual(panel.storedNotes().slice(0, 2).map((note) => note.id), ['1', '2'])

  moreFor('2').click()
  menu.querySelector('[data-note-action="delete"]').click()
  assert.equal(panel.dialogOpen('#delete-dialog'), true)
  assert.equal(panel.storedNotes().some((note) => note.id === '2'), true)
  panel.document.querySelector('#delete-form').dispatchEvent(new panel.window.Event('submit', {
    bubbles: true,
    cancelable: true,
  }))
  await panel.flush()
  assert.equal(panel.storedNotes().some((note) => note.id === '2'), false)
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

test('list Enter copies content then requests hide once', async (t) => {
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
  panel.document.querySelector('#note-list').focus()
  assert.equal(panel.document.activeElement?.id, 'note-list')

  const enter = new panel.window.KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
  })
  panel.document.dispatchEvent(enter)
  await panel.flush()
  assert.equal(panel.requestHideCalls(), 1)
})

test('clicking a list item keeps list focus so Enter copies and hides', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  const selectButton = panel.document.querySelector('[data-note-id].note-select')
  assert.ok(selectButton)
  selectButton.dispatchEvent(new panel.window.MouseEvent('click', { bubbles: true }))
  await panel.flush()
  assert.equal(panel.document.activeElement?.id, 'note-list')

  const enter = new panel.window.KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
  })
  panel.document.dispatchEvent(enter)
  await panel.flush()
  assert.equal(panel.requestHideCalls(), 1)
})

test('editor removes text actions and exposes a bottom-right copy icon', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  assert.equal(panel.document.querySelector('#copy-btn'), null)
  assert.equal(panel.document.querySelector('#save-btn'), null)
  const copyButton = panel.document.querySelector('#editor-copy-btn')
  assert.ok(copyButton)
  assert.equal(copyButton.getAttribute('aria-label'), '复制正文')
  assert.ok(copyButton.querySelector('svg[aria-hidden="true"]'))
  const scrollbar = panel.document.querySelector('#editor-scrollbar')
  assert.ok(scrollbar)
  assert.equal(scrollbar.getAttribute('aria-hidden'), 'true')
  assert.ok(scrollbar.querySelector('#editor-scrollbar-thumb'))
  const listScrollbar = panel.document.querySelector('#note-list-scrollbar')
  assert.ok(listScrollbar)
  assert.equal(listScrollbar.getAttribute('aria-hidden'), 'true')
  assert.ok(listScrollbar.querySelector('#note-list-scrollbar-thumb'))
  const status = panel.document.querySelector('#editor-status')
  const editorHeader = panel.document.querySelector('.editor-header')
  const editorTitle = panel.document.querySelector('#editor-title')
  const editorSaveState = panel.document.querySelector('#editor-save-state')
  assert.ok(editorHeader)
  assert.ok(editorTitle)
  assert.equal(editorSaveState, null)
  assert.equal(status, null)
  assert.equal(panel.document.querySelector('.editor-actions'), null)

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
  assert.equal(editorTitle.textContent, 'Project Plan')
  assert.equal(panel.document.querySelector('#editor-save-state'), null)
  panel.editor().value = 'changed'
  panel.editor().dispatchEvent(new panel.window.Event('input', { bubbles: true }))
  assert.equal(panel.document.querySelector('#editor-save-state'), null)
  copyButton.click()
  await panel.flush()
  assert.deepEqual(panel.statuses.at(-1), { message: '已复制', tone: 'success' })
  assert.equal(panel.requestHideCalls(), 0)

  panel.document.execCommand = () => false
  copyButton.click()
  await panel.flush()
  assert.deepEqual(panel.statuses.at(-1), { message: '复制失败', tone: 'error' })

  const css = await readFile(new URL('../package/dist/panel.css', import.meta.url), 'utf8')
  assert.match(css, /\.editor-copy-button\s*\{[\s\S]*position:\s*absolute;[\s\S]*right:[^;]+;[\s\S]*bottom:[^;]+;/)
  assert.match(css, /\.editor-panel\s*\{[^}]*padding:\s*0;[^}]*gap:\s*0;/)
  assert.match(css, /\.editor-header\s*\{[^}]*border-bottom:\s*1px solid var\(--uipilot-color-border\);/s)
  assert.match(css, /\.editor-surface\s*\{[^}]*padding:\s*0;[^}]*border:\s*0;[^}]*border-radius:\s*0;/)
  assert.match(css, /\.editor-content\s*\{[^}]*scrollbar-width:\s*none;/)
  assert.match(css, /\.editor-content::-webkit-scrollbar\s*\{[^}]*display:\s*none;[^}]*width:\s*0;/)
  assert.match(css, /\.editor-scrollbar\s*\{[^}]*position:\s*absolute;/)
  assert.match(css, /\.editor-scrollbar-thumb\s*\{[^}]*position:\s*absolute;/)
  assert.match(css, /\.note-list\s*\{[^}]*scrollbar-width:\s*none;/)
  assert.match(css, /\.note-list::-webkit-scrollbar\s*\{[^}]*display:\s*none;[^}]*width:\s*0;/)
  assert.match(css, /\.note-list-scrollbar\s*\{[^}]*position:\s*absolute;/)
  assert.match(css, /\.note-list-scrollbar-thumb\s*\{[^}]*position:\s*absolute;/)

  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  for (const required of [
    'createVirtualScrollbar',
    "scrollable.addEventListener('scroll'",
    "track.addEventListener('pointerdown'",
    "thumb.addEventListener('pointerdown'",
    'noteListVirtualScrollbar.schedule',
    'editorVirtualScrollbar.schedule',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
})

test('Ctrl+S in the editor saves and restores the selected list item context', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()

  const selectButton = panel.document.querySelector('[data-note-id="2"].note-select')
  assert.ok(selectButton)
  selectButton.dispatchEvent(new panel.window.MouseEvent('click', { bubbles: true }))
  await panel.flush()

  const noteList = panel.document.querySelector('#note-list')
  const right = new panel.window.KeyboardEvent('keydown', {
    key: 'ArrowRight',
    bubbles: true,
    cancelable: true,
  })
  noteList.dispatchEvent(right)
  assert.equal(panel.document.activeElement?.id, 'editor-content')

  panel.editor().value = 'saved from shortcut'
  const save = new panel.window.KeyboardEvent('keydown', {
    key: 's',
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  })
  panel.editor().dispatchEvent(save)
  assert.equal(save.defaultPrevented, true)
  await panel.flush()

  assert.equal(panel.document.activeElement?.id, 'note-list')
  assert.equal(noteList.getAttribute('aria-activedescendant'), 'note-option-2')
  assert.deepEqual(panel.statuses.at(-1), { message: '已保存', tone: 'success' })
  panel.window.uipilotPluginPanel.storage.set = async () => { throw new Error('storage failed') }
  panel.editor().value = 'failed edit'
  panel.editor().dispatchEvent(new panel.window.KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true, cancelable: true }))
  await panel.flush()
  assert.deepEqual(panel.statuses.at(-1), { message: '保存失败', tone: 'error' })
})

test('context actions follow input, list and editor focus and preserve note ownership', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()
  const input = panel.actions('hostInput')
  assert.equal(input.global.some(action => action.shortcut === 'Ctrl+F'), false)
  assert.deepEqual(input.context.map(action => action.title), ['进入目录'])
  assert.ok(input.global.some(action => action.shortcut === 'Ctrl+N'))
  await input.context[0].run()
  const selectedId = panel.selectedId()
  const list = panel.actions()
  assert.equal(list.global.filter(action => action.shortcut === 'Ctrl+F').length, 1)
  assert.ok(list.context.some(action => action.title === '复制并隐藏'))
  await list.context.find(action => action.title === '编辑正文').run()
  assert.equal(panel.document.activeElement, panel.editor())
  const editor = panel.actions()
  assert.ok(editor.context.some(action => action.shortcut === 'Ctrl+S'))
  panel.editor().setSelectionRange(1, 3)
  await editor.context.find(action => action.title === '复制正文').run()
  assert.equal(panel.document.activeElement, panel.editor())
  assert.deepEqual([panel.editor().selectionStart, panel.editor().selectionEnd], [1, 3])
  panel.editor().value = 'saved from actions'
  await editor.context.find(action => action.shortcut === 'Ctrl+S').run()
  await panel.flush()
  assert.equal(panel.storedNotes().find(note => note.id === selectedId).content, 'saved from actions')
  assert.equal(panel.document.activeElement.id, 'note-list')
  const otherId = selectedId === '1' ? '2' : '1'
  panel.document.querySelector(`[data-note-id="${otherId}"].note-select`).click()
  await panel.flush()
  await list.context.find(action => action.title === '删除笔记').run()
  assert.equal(panel.dialogOpen('#delete-dialog'), false)
  await panel.actions().global.find(action => action.shortcut === 'Ctrl+N').run()
  assert.equal(panel.dialogOpen('#new-dialog'), true)
  assert.equal(panel.actions(), null)
})


test('an older success timer cannot clear a newer identical message or an error', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.flush()
  panel.document.querySelector('[data-note-id="1"].note-select').click()
  await panel.flush()
  const timers = []
  panel.window.setTimeout = (callback) => { timers.push(callback); return timers.length }
  const copyButton = panel.document.querySelector('#editor-copy-btn')
  copyButton.click()
  await panel.flush()
  copyButton.click()
  await panel.flush()
  assert.equal(timers.length, 2)
  timers[0]()
  assert.deepEqual(panel.statuses.at(-1), { message: '已复制', tone: 'success' })
  panel.document.execCommand = () => false
  copyButton.click()
  await panel.flush()
  timers[1]()
  assert.deepEqual(panel.statuses.at(-1), { message: '复制失败', tone: 'error' })
})

test('dialog close buttons follow cancellation without mutating notes or discarding the editor draft', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const node = (id) => panel.document.querySelector('#' + id)
  const close = async (id) => {
    const button = node(id + '-close-btn')
    assert.ok(button)
    assert.equal(button.type, 'button')
    assert.equal(button.getAttribute('aria-label'), '关闭')
    button.click()
    await panel.flush()
    assert.equal(node(id + '-dialog').open, false)
  }
  node('new-btn').click()
  node('new-title-input').value = 'Cancelled new note'
  await close('new')
  assert.equal(node('new-title-input').value, '')
  const openAction = (action) => {
    panel.document.querySelector('[data-note-id="1"].note-more').click()
    panel.document.querySelector('[data-note-action="' + action + '"]').click()
  }
  openAction('rename')
  node('rename-title-input').value = 'Cancelled rename'
  assert.deepEqual([...node('rename-dialog').querySelectorAll('.dialog-actions button')].map((button) => button.id), ['rename-cancel-btn', 'rename-save-btn'])
  await close('rename')
  assert.equal(node('rename-title-input').value, '')
  openAction('delete')
  await close('delete')
  panel.document.querySelector('[data-note-id="1"].note-select').click()
  await panel.flush()
  panel.editor().value = 'Keep this unsaved draft'
  panel.editor().dispatchEvent(new panel.window.Event('input', { bubbles: true }))
  panel.document.querySelector('[data-note-id="2"].note-select').click()
  await panel.flush()
  assert.equal(node('unsaved-dialog').open, true)
  await close('unsaved')
  assert.equal(panel.selectedId(), '1')
  assert.equal(panel.editor().value, 'Keep this unsaved draft')
  assert.deepEqual(panel.storedNotes(), sampleNotes)
  panel.editor().value = sampleNotes[0].content
  panel.document.querySelector('[data-note-id="2"].note-select').click()
  await panel.flush()
  assert.equal(panel.selectedId(), '2')
})
