import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'
import { JSDOM } from 'jsdom'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
const panelHtmlUrl = new URL('../package/dist/panel.html', import.meta.url)
const panelScriptUrl = new URL('../package/dist/panel.js', import.meta.url)
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
let panelModuleSequence = 0

async function loadRuntime() {
  const source = await readFile(runtimeUrl, 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}

async function loadPanel({ focused = false } = {}) {
  const [html, source] = await Promise.all([
    readFile(panelHtmlUrl, 'utf8'),
    readFile(panelScriptUrl, 'utf8'),
  ])
  const dom = new JSDOM(html, { url: 'https://panel.uipilot.invalid/' })
  let contentFocused = focused
  let hostKeyHandler
  let focusHostInputCalls = 0
  let requestHideCalls = 0

  Object.defineProperty(dom.window.document, 'hasFocus', {
    configurable: true,
    value: () => contentFocused,
  })
  dom.window.uipilotPluginPanel = {
    onHostKey(handler) {
      hostKeyHandler = handler
      return () => {}
    },
    onUpdate() {
      return () => {}
    },
    focusHostInput: async () => { focusHostInputCalls += 1 },
    requestHide: async () => { requestHideCalls += 1 },
    storage: {
      get: async () => undefined,
      set: async () => {},
      remove: async () => {},
    },
  }

  globalThis.window = dom.window
  globalThis.document = dom.window.document
  panelModuleSequence += 1
  await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}#${panelModuleSequence}`)

  return {
    window: dom.window,
    text: (selector) => dom.window.document.querySelector(selector)?.textContent,
    history: () => [...dom.window.document.querySelectorAll('#key-history li')].map((item) => item.textContent),
    hostKey: (event) => hostKeyHandler?.(event),
    setFocused(next) { contentFocused = next },
    focusHostInputCalls: () => focusHostInputCalls,
    requestHideCalls: () => requestHideCalls,
    cleanup() {
      dom.window.close()
      delete globalThis.window
      delete globalThis.document
    },
  }
}

const invocation = Object.freeze({
  apiVersion: 1,
  requestId: 'demo-panel-request-1',
  input: 'hello',
  context: Object.freeze({
    platform: 'windows',
    theme: 'dark',
    invokedAt: '2026-08-24T12:00:00Z',
  }),
})

test('manifest declares the fixed panel contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.demo-panel')
  assert.equal(manifest.version, '1.0.2')
  assert.equal(manifest.minimumHostVersion, '0.3.1')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['ui.panel'])
  assert.deepEqual(manifest.panel, {
    entry: 'dist/panel.html',
    hostKeys: ['ArrowDown', 'ArrowUp', 'Primary+N'],
  })
  assert.equal('window' in manifest, false)
})

test('strict package root contains only panel assets', async () => {
  assert.deepEqual(
    (await readdir(packageRoot)).filter((entry) => entry !== 'icon.png').sort(),
    ['dist', 'plugin.json'],
  )
  assert.deepEqual(
    (await readdir(new URL('dist/', packageRoot))).sort(),
    ['panel.css', 'panel.html', 'panel.js', 'runtime.js'],
  )
})

test('runtime returns a panel response for every submit', async () => {
  const runtime = await loadRuntime()
  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'demo-panel-request-1',
    data: { echo: 'hello', requestId: 'demo-panel-request-1' },
  })
})

test('panel content uses only the panel bridge and isolated storage', async () => {
  const source = await readFile(new URL('../package/dist/panel.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginPanel.onUpdate',
    'uipilotPluginPanel.storage.get',
    'uipilotPluginPanel.storage.set',
    'uipilotPluginPanel.focusHostInput()',
    'uipilotPluginPanel.onHostKey',
    'uipilotPluginPanel.requestHide()',
    "event.ctrlKey || event.metaKey",
    "event.key.toLowerCase() === 'f'",
    "event.key.toLowerCase() === 'h'",
    'event.preventDefault()',
    'update.input',
    'update.theme',
  ]) {
    assert.match(source, new RegExp(escapeRegex(required)))
  }
  for (const forbidden of ['invoke(', 'fetch(', 'WebSocket', 'uipilotPluginWindow', 'timer', 'notifications']) {
    assert.doesNotMatch(source, new RegExp(forbidden.replace('(', '\\(')))
  }
})

test('panel diagnostics reflect initial focus and focus transitions', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  assert.equal(panel.text('#focus-state'), 'Not focused')
  panel.setFocused(true)
  panel.window.dispatchEvent(new panel.window.Event('focus'))
  assert.equal(panel.text('#focus-state'), 'Focused')
  panel.setFocused(false)
  panel.window.dispatchEvent(new panel.window.Event('blur'))
  assert.equal(panel.text('#focus-state'), 'Not focused')
})

test('panel diagnostics show Host-routed keys and route sequence', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  panel.hostKey({ key: 'ArrowDown', routeSequence: '42' })

  assert.equal(panel.text('#latest-key'), 'ArrowDown')
  assert.equal(panel.text('#key-source'), 'Host route')
  assert.equal(panel.text('#key-count'), '1')
  assert.equal(panel.text('#route-sequence'), '42')
  assert.deepEqual(panel.history(), ['ArrowDown | Host route | route 42'])
})

test('panel diagnostics show content keys newest-first and retain five events', async (t) => {
  const panel = await loadPanel({ focused: true })
  t.after(panel.cleanup)
  const content = panel.window.document.querySelector('main')
  content.addEventListener('keydown', (event) => event.stopPropagation())

  for (const key of ['a', 'b', 'c', 'd', 'e', 'f']) {
    content.dispatchEvent(new panel.window.KeyboardEvent('keydown', {
      bubbles: true,
      key,
      shiftKey: key === 'f',
    }))
  }

  assert.equal(panel.text('#latest-key'), 'Shift+F')
  assert.equal(panel.text('#key-source'), 'Panel content')
  assert.equal(panel.text('#key-count'), '6')
  assert.equal(panel.text('#route-sequence'), 'None')
  assert.deepEqual(panel.history(), [
    'Shift+F | Panel content',
    'E | Panel content',
    'D | Panel content',
    'C | Panel content',
    'B | Panel content',
  ])
})

test('panel diagnostics record Ctrl shortcuts before preserving bridge commands', async (t) => {
  const panel = await loadPanel({ focused: true })
  t.after(panel.cleanup)

  panel.window.dispatchEvent(new panel.window.KeyboardEvent('keydown', { key: 'f', ctrlKey: true, cancelable: true }))
  assert.equal(panel.text('#latest-key'), 'Ctrl+F')
  assert.equal(panel.focusHostInputCalls(), 1)

  panel.window.dispatchEvent(new panel.window.KeyboardEvent('keydown', { key: 'h', ctrlKey: true, cancelable: true }))
  assert.equal(panel.text('#latest-key'), 'Ctrl+H')
  assert.equal(panel.requestHideCalls(), 1)
})
