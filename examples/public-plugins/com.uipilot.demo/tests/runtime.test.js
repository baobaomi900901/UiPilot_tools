import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)

async function loadRuntime(outputMode) {
  const source = await readFile(runtimeUrl, 'utf8')
  assert.match(source, /const OUTPUT_MODE = 'window'/)
  const configured = source.replace(
    "const OUTPUT_MODE = 'window'",
    `const OUTPUT_MODE = '${outputMode}'`,
  )
  if (outputMode !== 'window') assert.notEqual(configured, source)
  return import(`data:text/javascript;base64,${Buffer.from(configured).toString('base64')}`)
}

const invocation = Object.freeze({
  apiVersion: 1,
  requestId: 'demo-request-1',
  input: 'str',
  context: Object.freeze({
    platform: 'windows',
    theme: 'dark',
    invokedAt: '2026-08-13T23:59:58+08:00',
  }),
})

test('strict package root contains only the public demo assets', async () => {
  const rootFiles = (await readdir(packageRoot)).sort()
  const distFiles = (await readdir(new URL('dist/', packageRoot))).sort()
  assert.deepEqual(rootFiles, ['dist', 'plugin.json'])
  assert.deepEqual(distFiles, ['runtime.js', 'window.css', 'window.html', 'window.js'])
})

test('window mode echoes ownership and returns the local date text', async () => {
  const runtime = await loadRuntime('window')
  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'demo-request-1',
    data: { returnText: 'str 2026-08-13' },
  })
})

test('static mainResult variant returns one copyText default action', async () => {
  const runtime = await loadRuntime('mainResult')
  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'demo-request-1',
    results: [
      {
        id: 'demo-copy',
        title: 'str 2026-08-13',
        subtitle: 'Demo result',
        defaultAction: { type: 'copyText', text: 'str 2026-08-13' },
      },
    ],
  })
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
