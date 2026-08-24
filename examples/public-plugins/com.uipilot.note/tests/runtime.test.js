import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)

async function loadRuntime() {
  const source = await readFile(runtimeUrl, 'utf8')
  assert.doesNotMatch(source, /OUTPUT_MODE|mainResult/)
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}

const invocation = Object.freeze({
  apiVersion: 1,
  requestId: 'note-request-1',
  input: '',
  context: Object.freeze({
    platform: 'windows',
    theme: 'dark',
    invokedAt: '2026-08-24T12:00:00+08:00',
  }),
})

test('manifest declares the note window contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.note')
  assert.equal(manifest.name, 'Note')
  assert.equal(manifest.command.defaultName, 'note')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'window')
  assert.equal(manifest.command.inputRequired, false)
  assert.equal(manifest.version, '1.1.5')
  assert.deepEqual(manifest.supportedPlatforms, ['windows', 'macos'])
  assert.deepEqual(manifest.permissions, ['ui.window'])
  assert.deepEqual(manifest.window, { entry: 'dist/window.html' })
})

test('strict package root contains only the note assets', async () => {
  const rootFiles = (await readdir(packageRoot)).sort()
  const distFiles = (await readdir(new URL('dist/', packageRoot))).sort()
  assert.deepEqual(rootFiles, ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual(distFiles, ['runtime.js', 'window.css', 'window.html', 'window.js'])
})

test('runtime returns an empty window payload and preserves request ownership', async () => {
  const runtime = await loadRuntime()
  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'note-request-1',
    data: {},
  })
})

test('content page uses the host window bridge and local storage', async () => {
  const source = await readFile(new URL('../package/dist/window.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginWindow.onUpdate',
    'uipilotPluginWindow.storage',
    'notes.entries',
    'note-list',
    'note-list-viewport',
    'note-card',
    'list-empty',
    'new-dialog',
    'delete-dialog',
    'ArrowUp',
    'ArrowDown',
    'ArrowLeft',
    'ArrowRight',
    'isFocusInList',
    'tryCopyFromList',
    'copyEditorContent',
    'closePluginWindow',
    'uipilotPluginWindow.close',
    'dataset.theme',
  ]) {
    assert.match(source, new RegExp(required.replace('.', '\\.')))
  }
  for (const forbidden of ['invoke(', 'fetch(', 'WebSocket', 'alwaysOnTop']) {
    assert.doesNotMatch(source, new RegExp(forbidden.replace('(', '\\(')))
  }
})
