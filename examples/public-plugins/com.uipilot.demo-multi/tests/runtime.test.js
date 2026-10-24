import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('./fixtures/multi-package/', import.meta.url)
const runtimeUrl = new URL('dist/runtime.js', packageRoot)
const manifest = async () => JSON.parse(await readFile(new URL('plugin.json', packageRoot), 'utf8'))
const loadRuntime = async (transform = source => source) => {
  const source = transform(await readFile(runtimeUrl, 'utf8'))
  return import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'))
}
const invocation = (commandId, requestId = 'request-1') => Object.freeze({
  apiVersion: 1, requestId, ...(commandId === undefined ? {} : { commandId }), input: 'I am  Jack',
  context: Object.freeze({ platform: 'windows', theme: 'dark', invokedAt: '2026-10-24T12:00:00+08:00' }),
})
function mockHost(prefix = '[demo] ') {
  const storage = new Map()
  const settings = Object.freeze({ prefix })
  const api = Object.freeze({
    storage: Object.freeze({
      async get(key) { return storage.has(key) ? structuredClone(storage.get(key)) : null },
      async set(key, value) { storage.set(key, structuredClone(value)) },
      async remove(key) { storage.delete(key) },
    }),
    settings: Object.freeze({
      async get(key) { return Object.hasOwn(settings, key) ? settings[key] : null },
      async isSecretConfigured() { return false },
    }),
  })
  return { api, storage, settings }
}
function expected(requestId, text, count) {
  return { requestId, results: [{ id: 'text-copy', title: text,
    subtitle: 'Shared invocation ' + count, defaultAction: { type: 'copyText', text } }] }
}

test('fixture declares two mainResult IDs and one shared offline runtime, settings and permission', async () => {
  const value = await manifest()
  assert.equal(value.pluginId, 'com.uipilot.demo-multi')
  assert.equal(value.minimumHostVersion, '0.5.0')
  assert.equal(value.schemaVersion, 1)
  assert.equal(value.apiVersion, 1)
  assert.equal(Object.hasOwn(value, 'command'), false)
  assert.deepEqual(value.commands, [
    { id: 'upper', defaultName: 'demo-upper', summary: 'Convert text to uppercase', activationMode: 'live', outputMode: 'mainResult', inputRequired: true, inputPlaceholder: 'Enter text' },
    { id: 'lower', defaultName: 'demo-lower', summary: 'Convert text to lowercase', activationMode: 'submit', outputMode: 'mainResult', inputRequired: true, inputPlaceholder: 'Enter text' },
  ])
  assert.deepEqual(value.runtime, { entry: 'dist/runtime.js' })
  assert.deepEqual(value.settings, [{ type: 'text', key: 'prefix', label: 'Result prefix', default: '[demo] ' }])
  assert.deepEqual(value.permissions, ['clipboard.write'])
  for (const key of ['window', 'panel', 'network']) assert.equal(Object.hasOwn(value, key), false)
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'plugin.json'])
  assert.deepEqual(await readdir(new URL('dist/', packageRoot)), ['runtime.js'])
})

test('one runtime dispatches both IDs and shares package settings and storage', async () => {
  const runtime = await loadRuntime()
  assert.deepEqual(Object.keys(runtime), ['onCommand'])
  const host = mockHost()
  const upper = invocation('upper', 'upper-request')
  assert.deepEqual(await runtime.onCommand(upper, host.api), expected('upper-request', '[demo] I AM  JACK', 1))
  assert.deepEqual(await runtime.onCommand(invocation('lower', 'lower-request'), host.api), expected('lower-request', '[demo] i am  jack', 2))
  assert.equal(host.storage.get('invocation-count'), 2)
  assert.equal(host.storage.get('last-command'), 'lower')
  assert.equal(host.settings.prefix, '[demo] ')
  assert.equal(upper.input, 'I am  Jack')
})

test('missing, unknown and invalid IDs fail explicitly before touching package data', async () => {
  const runtime = await loadRuntime()
  const host = mockHost()
  for (const id of [undefined, null, '', 'default', 'constructor', 'toString', 'UPPER', 1]) {
    await assert.rejects(runtime.onCommand(invocation(id), host.api), /Unsupported command ID/)
  }
  assert.equal(host.storage.size, 0)
  assert.deepEqual(Object.keys(invocation(undefined)).sort(), ['apiVersion', 'context', 'input', 'requestId'])
  assert.deepEqual(Object.keys(invocation('upper')).sort(), ['apiVersion', 'commandId', 'context', 'input', 'requestId'])
})

test('dispatch remains ID-based across changed versions, startup names and command order', async () => {
  const value = await manifest()
  const runtime = await loadRuntime()
  const variants = [value, { ...value, version: '2.0.0', commands: [...value.commands].reverse().map(command => ({ ...command, defaultName: command.id === 'upper' ? 'new-upper' : 'new-lower' })) }]
  for (const variant of variants) {
    const host = mockHost('')
    for (const command of variant.commands) {
      const request = invocation(command.id, variant.version + '-' + command.id)
      const text = command.id === 'upper' ? 'I AM  JACK' : 'i am  jack'
      assert.deepEqual(await runtime.onCommand(request, host.api), expected(request.requestId, text, host.storage.get('invocation-count')))
    }
  }
})

test('legal constructor command variant uses explicit dispatch without prototype lookup', async () => {
  const runtime = await loadRuntime(source => {
    assert.equal(source.split("case 'upper':").length, 2)
    return source.replace("case 'upper':", "case 'constructor':")
  })
  assert.deepEqual(await runtime.onCommand(invocation('constructor'), mockHost('').api), expected('request-1', 'I AM  JACK', 1))
  await assert.rejects(runtime.onCommand(invocation('toString'), mockHost().api), /Unsupported command ID/)
})
