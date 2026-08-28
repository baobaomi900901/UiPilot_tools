import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)

async function requiredJson(relativePath) {
  const source = await readFile(new URL(relativePath, packageRoot), 'utf8')
  return JSON.parse(source)
}

async function loadRuntime() {
  const source = await readFile(new URL('dist/runtime.js', packageRoot), 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}

function invocation(input, requestId = 'demo-http-request-1') {
  return Object.freeze({
    apiVersion: 1,
    requestId,
    input,
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-28T08:00:00+08:00',
    }),
  })
}

function networkApi(calls, implementation = async () => ({
  status: 200,
  headers: Object.freeze({ 'content-type': Object.freeze(['application/json']) }),
  body: '{"ok":true}',
})) {
  return Object.freeze({
    network: Object.freeze({
      async request(input) {
        calls.push(input)
        return implementation(input)
      },
    }),
  })
}

test('manifest declares the local Host HTTPS acceptance contract', async () => {
  const manifest = await requiredJson('plugin.json')

  assert.equal(manifest.pluginId, 'com.uipilot.demo-http')
  assert.equal(manifest.minimumHostVersion, '0.3.2')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.equal(manifest.command.defaultName, 'demo-http')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'mainResult')
  assert.equal(manifest.command.inputRequired, false)
  assert.deepEqual(manifest.permissions, ['network.https'])
  assert.deepEqual(manifest.network, { httpsHosts: ['httpbin.org'] })
})

test('runtime maps every acceptance command to one exact Host request', async () => {
  const runtime = await loadRuntime()
  const cases = [
    ['', { url: 'https://httpbin.org/get', method: 'GET' }],
    ['get hello world', { url: 'https://httpbin.org/get?text=hello%20world', method: 'GET' }],
    ['post Hello  world', {
      url: 'https://httpbin.org/anything',
      method: 'POST',
      body: { type: 'json', value: { text: 'Hello  world' } },
    }],
    ['text Hello', {
      url: 'https://httpbin.org/anything',
      method: 'POST',
      body: { type: 'text', value: 'Hello' },
    }],
    ['form Hello', {
      url: 'https://httpbin.org/anything',
      method: 'POST',
      body: { type: 'form', value: { text: 'Hello' } },
    }],
    ['status 503', { url: 'https://httpbin.org/status/503', method: 'GET' }],
    ['timeout', { url: 'https://httpbin.org/delay/15', method: 'GET' }],
    ['denied', { url: 'https://example.com/', method: 'GET' }],
    ['protected', {
      url: 'https://httpbin.org/anything',
      method: 'GET',
      headers: { Host: 'example.com' },
    }],
  ]

  for (const [input, expected] of cases) {
    const calls = []
    await runtime.onCommand(invocation(input), networkApi(calls))
    assert.deepEqual(calls, [expected], input || '(empty input)')
  }
})

test('runtime returns bounded success details for every HTTP status', async () => {
  const runtime = await loadRuntime()
  const calls = []
  const api = networkApi(calls, async () => ({
    status: 503,
    headers: Object.freeze({}),
    body: 'x'.repeat(9_000),
  }))

  const response = await runtime.onCommand(invocation('status 503'), api)

  assert.equal(response.requestId, 'demo-http-request-1')
  assert.equal(response.results.length, 1)
  assert.equal(response.results[0].title, 'GET 503')
  assert.equal(response.results[0].subtitle, 'Host HTTPS request completed')
  assert.equal(response.results[0].detail.length, 8_204)
  assert.match(response.results[0].detail, /\n\[truncated\]$/)
})

test('runtime exposes only the stable Host Error.name', async () => {
  const runtime = await loadRuntime()
  const calls = []
  const failure = new Error('must not be displayed')
  failure.name = 'NetworkTimeoutError'

  const response = await runtime.onCommand(
    invocation('timeout'),
    networkApi(calls, async () => { throw failure }),
  )

  assert.deepEqual(response, {
    requestId: 'demo-http-request-1',
    results: [{
      id: 'demo-http-error',
      title: 'NetworkTimeoutError',
      subtitle: 'Host HTTPS request rejected',
      detail: 'Command: timeout',
    }],
  })
  assert.doesNotMatch(JSON.stringify(response), /must not be displayed/)
})

test('runtime reports usage without invoking Host for an unknown command', async () => {
  const runtime = await loadRuntime()
  const calls = []

  const response = await runtime.onCommand(invocation('unknown'), networkApi(calls))

  assert.deepEqual(calls, [])
  assert.equal(response.results[0].title, 'Usage')
  assert.match(response.results[0].detail, /get \| post TEXT/)
})

test('strict package root contains only the manifest and Runtime', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'icon.png', 'plugin.json'])
  assert.deepEqual(
    (await readdir(new URL('dist/', packageRoot))).sort(),
    ['runtime.js'],
  )
})
