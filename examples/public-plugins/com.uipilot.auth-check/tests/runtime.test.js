import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { readFile, readdir } from 'node:fs/promises'
import test from 'node:test'

const packageRoot = new URL('../package/', import.meta.url)
async function runtime() {
  const entry = new URL('dist/runtime.js', packageRoot)
  assert.ok(existsSync(entry), 'auth-check runtime must implement the fixed acceptance contract')
  const source = await readFile(entry, 'utf8')
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}
const invocation = (input = '') => ({ apiVersion: 1, requestId: 'offline-request', input, context: { platform: 'windows', theme: 'dark', invokedAt: '2026-10-27T08:00:00+08:00' } })
const expectedResult = (title, detail) => ({ requestId: 'offline-request', results: [{ id: 'auth-check-result', title, detail }] })
const guidance = '请检查 Host 测试凭据和网络授权，重新运行 request；勿填真实密钥。'
const expectedRequest = { url: 'https://httpbun.com/bearer/uipilot-acceptance-token', method: 'GET', credential: 'acceptance-token' }

function statusOnlyApi(configured, keys) {
  return new Proxy({ settings: new Proxy({ async isSecretConfigured(key) { keys.push(key); return configured } }, {
    get(target, key) { assert.equal(key, 'isSecretConfigured'); return target[key] },
  }) }, { get(target, key) { assert.equal(key, 'settings'); return target[key] } })
}

test('empty and normalized status input only check the declared secret locally', async () => {
  const { onCommand } = await runtime()
  for (const input of ['', '  ', 'status', ' StAtUs ']) {
    for (const configured of [false, true]) {
      const keys = []
      const result = await onCommand(invocation(input), statusOnlyApi(configured, keys))
      assert.deepEqual(keys, ['test-token'])
      assert.deepEqual(result, expectedResult(configured ? '测试凭据已配置' : '测试凭据未配置', '仅检查配置状态；请在 Host 设置中保存公开测试值，勿填真实密钥。'))
    }
  }
})

test('unknown input stays local and provides fixed usage', async () => {
  const { onCommand } = await runtime()
  const api = new Proxy({}, { get() { assert.fail('unknown input must use no API') } })
  for (const input of ['help', 'request now', 'https://untrusted.invalid', 'uipilot-wrong-token']) {
    assert.deepEqual(await onCommand(invocation(input), api), expectedResult('请输入 status 或 request', 'status 检查本地配置；request 发起一次认证测试。'))
  }
})

test('only explicit normalized request makes exactly one fixed Host call', async () => {
  const { onCommand } = await runtime()
  for (const input of ['request', ' ReQuEsT ']) {
    const calls = []
    const result = await onCommand(invocation(input), { network: { async request(request) { calls.push(request); return { status: 200 } } } })
    assert.deepEqual(calls, [expectedRequest])
    assert.deepEqual(result, expectedResult('认证成功（HTTP 200）', '仅依据 HTTP 状态；未读取服务响应内容。'))
  }
})

test('HTTP outcomes never access response body or headers and only 200 succeeds', async () => {
  const { onCommand } = await runtime()
  for (const status of [200, 401, 204, 302, 429, 503]) {
    const accessed = []
    let calls = 0
    const response = { status,
      get body() { accessed.push('body'); throw new Error('private-response-body') },
      get headers() { accessed.push('headers'); throw new Error('private-response-headers') },
    }
    const result = await onCommand(invocation('request'), { network: { async request() { calls++; return response } } })
    const title = status === 200 ? '认证成功（HTTP 200）' : status === 401 ? '认证被拒绝（HTTP 401）' : `收到其他 HTTP 状态（HTTP ${status}）`
    assert.deepEqual(result, expectedResult(title, '仅依据 HTTP 状态；未读取服务响应内容。'))
    assert.deepEqual(accessed, [])
    assert.equal(calls, 1)
  }
})

test('every SDK network error uses only its fixed name and static guidance without retry', async () => {
  const { onCommand } = await runtime()
  for (const name of ['InvalidNetworkRequestError', 'PermissionDeniedError', 'NetworkTargetDeniedError', 'CredentialUnavailableError', 'NetworkTimeoutError', 'NetworkFailureError', 'NetworkResponseTooLargeError', 'NetworkResponseInvalidError', 'NetworkLimitExceededError', 'ExpiredRequestError']) {
    let calls = 0
    const error = { name, get message() { assert.fail('must not read error message') }, get stack() { assert.fail('must not read error stack') } }
    const result = await onCommand(invocation('request'), { network: { async request() { calls++; throw error } } })
    assert.deepEqual(result, expectedResult(name, name === 'PermissionDeniedError'
      ? '网络访问已关闭，可在「设置 → 权限」中开启后重新运行 request。'
      : guidance))
    assert.equal(calls, 1)
  }
})

test('unknown exceptions cannot leak names, messages, or stack contents', async () => {
  const { onCommand } = await runtime()
  for (const error of [new Error('private-message'), { name: 'private-name', message: 'private-message', stack: 'private-stack' }, { name: '__proto__' }, null, 'private-exception']) {
    let calls = 0
    const result = await onCommand(invocation('request'), { network: { async request() { calls++; throw error } } })
    assert.deepEqual(result, expectedResult('NetworkFailureError', guidance))
    assert.equal(calls, 1)
  }
})

test('local status failures use a fixed result and never contact the provider', async () => {
  const { onCommand } = await runtime()
  for (const input of ['', 'status']) {
    const result = await onCommand(invocation(input), { settings: { async isSecretConfigured() { throw new Error('private-status-error') } }, network: { request() { assert.fail('status failure must remain local') } } })
    assert.deepEqual(result, expectedResult('测试凭据状态读取失败', '请重新打开 Host 插件设置并检查配置；勿填真实密钥。'))
  }
})

test('missing network capability is a static local failure', async () => {
  const { onCommand } = await runtime()
  assert.deepEqual(await onCommand(invocation('request'), {}), expectedResult('网络能力不可用', '请检查 Host 版本是否支持网络能力，以及插件是否申报网络访问权限。'))
})

test('manifest declares only the fixed Windows bearer binding and public test secret', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json', packageRoot), 'utf8'))
  assert.equal(manifest.schemaVersion, 1)
  assert.equal(manifest.apiVersion, 1)
  assert.equal(manifest.pluginId, 'com.uipilot.auth-check')
  assert.equal(manifest.version, '1.0.0')
  assert.equal(manifest.name, '认证链路验收')
  assert.match(manifest.description, /勿填真实密钥/)
  assert.equal(manifest.minimumHostVersion, '0.4.0')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.command, { defaultName: 'auth-check', summary: '使用公开测试值验收认证链路', activationMode: 'submit', outputMode: 'mainResult', inputRequired: false, inputPlaceholder: 'status 或 request' })
  assert.deepEqual(manifest.runtime, { entry: 'dist/runtime.js' })
  assert.deepEqual(manifest.permissions, ['network.https'])
  assert.deepEqual(manifest.network, { httpsHosts: ['httpbun.com'], credentials: [{ id: 'acceptance-token', settingKey: 'test-token', host: 'httpbun.com', type: 'bearer' }] })
  assert.deepEqual(manifest.settings, [{ type: 'secret', key: 'test-token', label: '公开测试凭据（勿填真实密钥）' }])
  for (const key of ['commands', 'panel', 'window']) assert.equal(Object.hasOwn(manifest, key), false)
})

test('installable package has only manifest/runtime and no direct networking or secret reader', async () => {
  assert.deepEqual((await readdir(packageRoot)).sort(), ['dist', 'plugin.json'])
  assert.deepEqual(await readdir(new URL('dist/', packageRoot)), ['runtime.js'])
  const source = await readFile(new URL('dist/runtime.js', packageRoot), 'utf8')
  assert.doesNotMatch(source, /authorization|Bearer |fetch\s*\(|XMLHttpRequest|getSecret|console\.|\.body\b|\.headers\b|defaultAction|setTimeout|setInterval/i)
})
