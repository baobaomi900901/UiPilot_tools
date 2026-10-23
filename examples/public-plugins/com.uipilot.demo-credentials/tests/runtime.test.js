import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
async function runtime() {
  const source = await readFile(new URL('dist/runtime.js', packageRoot), 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}
const invocation = (input = '') => ({ requestId: 'synthetic-request', input, context: { platform: 'windows', theme: 'dark', invokedAt: '2026-10-23T08:00:00+08:00' } })
const response = (body, status = 200) => ({ status, headers: {}, body })

test('manifest binds only the Windows 0.4.0 bearer reference to a declared secret', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json', packageRoot), 'utf8'))
  assert.equal(manifest.pluginId, 'com.uipilot.demo-credentials')
  assert.equal(manifest.minimumHostVersion, '0.4.0')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.equal(manifest.command.outputMode, 'mainResult')
  assert.deepEqual(manifest.permissions, ['network.https'])
  assert.deepEqual(manifest.network, { httpsHosts: ['api.example.com'], credentials: [{ id: 'service-token', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' }] })
  assert.deepEqual(manifest.settings, [{ type: 'secret', key: 'api-token', label: 'Synthetic API token' }])
})

test('offline provider receives only the exact credential reference and the runtime shows a bounded response', async () => {
  const { onCommand } = await runtime()
  const calls = []
  const result = await onCommand(invocation(), { network: { async request(request) { calls.push(request); return response('x'.repeat(9000), 503) } } })
  assert.deepEqual(calls, [{ url: 'https://api.example.com/v1/demo', method: 'GET', credential: 'service-token' }])
  assert.equal(result.requestId, 'synthetic-request')
  assert.equal(result.results[0].title, 'HTTP 503')
  assert.equal(result.results[0].detail.length, 8204)
  assert.match(result.results[0].detail, /\n\[truncated\]$/)
})

test('offline fake echo demonstrates the explicit service response trust boundary', async () => {
  const { onCommand } = await runtime()
  // Synthetic provider fixture only. No token is embedded in the installable package.
  const syntheticEcho = 'Bearer synthetic-demo-token'
  const result = await onCommand(invocation(), { network: { async request() { return response(syntheticEcho) } } })
  assert.equal(result.results[0].detail, syntheticEcho)
})

test('stable credential failures expose no provider error text or accidental error names', async () => {
  const { onCommand } = await runtime()
  for (const name of ['CredentialUnavailableError', 'PermissionDeniedError', 'InvalidNetworkRequestError', 'NetworkTargetDeniedError', 'ExpiredRequestError', 'NetworkTimeoutError', 'unknown']) {
    const result = await onCommand(invocation(), { network: { async request() { throw Object.assign(new Error('synthetic-private-error'), { name }) } } })
    assert.equal(result.results[0].title, name === 'unknown' ? 'NetworkFailureError' : name)
    assert.doesNotMatch(JSON.stringify(result), /synthetic-private-error/)
  }
})

test('status checks use only isSecretConfigured and never invoke the provider', async () => {
  const { onCommand } = await runtime()
  for (const configured of [true, false]) {
    const keys = []
    const result = await onCommand(invocation('status'), { settings: { async isSecretConfigured(key) { keys.push(key); return configured } }, network: { request() { assert.fail('status must remain offline') } } })
    assert.deepEqual(keys, ['api-token'])
    assert.equal(result.results[0].title, configured ? 'Credential configured' : 'Credential not configured')
  }
})

test('missing network capability and unknown commands remain local', async () => {
  const { onCommand } = await runtime()
  assert.equal((await onCommand(invocation(), {})).results[0].title, 'Network API unavailable')
  assert.equal((await onCommand(invocation('unknown'), {})).results[0].title, 'Usage')
})

test('installable fixture contains no embedded token or direct networking', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'plugin.json'])
  const source = await readFile(new URL('dist/runtime.js', packageRoot), 'utf8')
  assert.doesNotMatch(source, /synthetic-demo-token|Bearer |fetch\s*\(|XMLHttpRequest|getSecret/)
})

test('multibyte provider output fits the Host result detail byte budget', async () => {
  const { onCommand } = await runtime()
  const result = await onCommand(invocation(), { network: { async request() { return response('界'.repeat(9000)) } } })
  assert.ok(Buffer.byteLength(result.results[0].detail, 'utf8') <= 16 * 1024)
  assert.match(result.results[0].detail, /\n\[truncated\]$/)
})
