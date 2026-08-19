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
  requestId: 'demo-win-request-1',
  input: 'str',
  context: Object.freeze({
    platform: 'windows',
    theme: 'dark',
    invokedAt: '2026-08-13T23:59:58+08:00',
  }),
})

test('manifest declares the fixed demo-win window contract', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.demo-win')
  assert.equal(manifest.name, 'Public Plugin Demo Window')
  assert.equal(manifest.command.defaultName, 'demo-win')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'window')
  assert.equal(manifest.version, '1.0.3')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['ui.window', 'notifications.publish'])
  assert.deepEqual(manifest.window, { entry: 'dist/window.html' })
})

test('strict package root contains only the demo-win assets', async () => {
  const rootFiles = (await readdir(packageRoot)).sort()
  const distFiles = (await readdir(new URL('dist/', packageRoot))).sort()
  assert.deepEqual(rootFiles, ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual(distFiles, ['runtime.js', 'window.css', 'window.html', 'window.js'])
})

test('window mode echoes ownership and returns the local date text', async () => {
  const runtime = await loadRuntime()
  const published = []
  const api = Object.freeze({
    notifications: Object.freeze({
      async publish(input) {
        published.push(input)
      },
    }),
  })
  assert.deepEqual(await runtime.onCommand(invocation, api), {
    requestId: 'demo-win-request-1',
    data: { returnText: 'str 2026-08-13' },
  })
  assert.deepEqual(published, [{ content: 'str 2026-08-13' }])
})

test('notification rejection prevents a window response', async () => {
  const runtime = await loadRuntime()
  const failure = new Error('notification unavailable')
  const api = Object.freeze({
    notifications: Object.freeze({
      async publish() {
        throw failure
      },
    }),
  })

  await assert.rejects(runtime.onCommand(invocation, api), failure)
})

test('content page uses the one-way update bridge for all five acceptance fields', async () => {
  const source = await readFile(new URL('../package/dist/window.js', import.meta.url), 'utf8')
  for (const required of [
    'uipilotPluginWindow.onUpdate',
    'update.input',
    'update.platform',
    'update.theme',
    'update.instanceNumber',
    'update.data.returnText',
  ]) {
    assert.match(source, new RegExp(required.replace('.', '\\.')))
  }
  for (const forbidden of ['invoke(', 'fetch(', 'WebSocket', 'alwaysOnTop']) {
    assert.doesNotMatch(source, new RegExp(forbidden.replace('(', '\\(')))
  }
})
