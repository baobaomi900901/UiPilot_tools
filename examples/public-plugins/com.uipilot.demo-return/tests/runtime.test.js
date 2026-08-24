import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)

async function requiredFile(url) {
  try {
    return await readFile(url, 'utf8')
  } catch {
    assert.fail(`missing required file: ${url.pathname}`)
  }
}

async function requiredDirectory(url) {
  try {
    return await readdir(url)
  } catch {
    assert.fail(`missing required directory: ${url.pathname}`)
  }
}

test('manifest declares the fixed demo-return main-result contract', async () => {
  const manifest = JSON.parse(await requiredFile(new URL('../package/plugin.json', import.meta.url)))
  assert.equal(manifest.pluginId, 'com.uipilot.demo-return')
  assert.equal(manifest.name, 'Public Plugin Demo Return')
  assert.equal(manifest.command.defaultName, 'demo-return')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'mainResult')
  assert.deepEqual(manifest.permissions, ['clipboard.write'])
  assert.equal('window' in manifest, false)
})

test('strict package root contains only the demo-return assets', async () => {
  const rootFiles = (await requiredDirectory(packageRoot)).sort()
  const distFiles = (await requiredDirectory(new URL('dist/', packageRoot))).sort()
  assert.deepEqual(rootFiles, ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual(distFiles, ['runtime.js'])
})

test('runtime preserves input spaces and returns one copy result', async () => {
  const source = await requiredFile(runtimeUrl)
  assert.doesNotMatch(source, /OUTPUT_MODE|ui\.window/)
  const runtime = await import(
    `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
  )
  const invocation = Object.freeze({
    apiVersion: 1,
    requestId: 'demo-return-request-1',
    input: 'I am  Jack',
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-17T23:59:58+08:00',
    }),
  })

  assert.deepEqual(await runtime.onCommand(invocation, Object.freeze({})), {
    requestId: 'demo-return-request-1',
    results: [
      {
        id: 'demo-return-copy',
        title: 'I am  Jack 2026-08-17',
        subtitle: 'Demo return result',
        defaultAction: { type: 'copyText', text: 'I am  Jack 2026-08-17' },
      },
    ],
  })
})
