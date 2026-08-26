import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
const escapeRegex = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

async function loadRuntime() {
  const source = await readFile(runtimeUrl, 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
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
  assert.equal(manifest.version, '1.0.1')
  assert.equal(manifest.minimumHostVersion, '0.3.0')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'panel')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['ui.panel'])
  assert.deepEqual(manifest.panel, { entry: 'dist/panel.html' })
  assert.equal('window' in manifest, false)
})

test('strict package root contains only panel assets', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'plugin.json'])
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
    "event.ctrlKey || event.metaKey",
    "event.key.toLowerCase() === 'f'",
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
