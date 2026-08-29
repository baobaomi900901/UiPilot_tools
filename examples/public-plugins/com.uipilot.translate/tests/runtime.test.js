import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
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

async function loadRuntime() {
  const source = await requiredFile(runtimeUrl)
  return {
    source,
    runtime: await import(
      `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
    ),
  }
}

function invocation(input, requestId = 'translate-request-1') {
  return Object.freeze({
    apiVersion: 1,
    requestId,
    input,
    context: Object.freeze({
      platform: 'windows',
      theme: 'dark',
      invokedAt: '2026-08-28T12:00:00+08:00',
    }),
  })
}
function networkApi(request) {
  return Object.freeze({
    network: Object.freeze({ request }),
  })
}

function successResponse(translation, language = 'en2zh-CHS') {
  return Object.freeze({
    status: 200,
    headers: Object.freeze({ 'content-type': Object.freeze(['application/json']) }),
    body: JSON.stringify({
      errorCode: '0',
      query: 'fixture',
      translation: [translation],
      l: language,
    }),
  })
}

function testTranslator(runtime) {
  return runtime.createTranslator({
    appId: 'test-app-id',
    appSecret: 'test-app-secret',
    createSalt: () => 'test-salt',
    currentTimeSeconds: () => '1787889600',
  })
}

test('manifest declares the translate main-result network contract', async () => {
  const manifest = JSON.parse(await requiredFile(new URL('../package/plugin.json', import.meta.url)))
  assert.equal(manifest.pluginId, 'com.uipilot.translate')
  assert.equal(manifest.command.defaultName, 'translate')
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'mainResult')
  assert.equal(manifest.command.inputRequired, true)
  assert.equal(manifest.minimumHostVersion, '0.3.2')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['clipboard.write', 'network.https'])
  assert.deepEqual(manifest.network, { httpsHosts: ['openapi.youdao.com'] })
  assert.equal('window' in manifest, false)
  assert.equal('panel' in manifest, false)
})

test('strict package root contains only installable plugin files', async () => {
  const rootFiles = (await requiredDirectory(packageRoot)).sort()
  const distFiles = (await requiredDirectory(new URL('dist/', packageRoot))).sort()
  assert.deepEqual(rootFiles, ['dist', 'plugin.json'])
  assert.deepEqual(distFiles, ['runtime.js'])
})

test('runtime uses only the Host network facade', async () => {
  const { source, runtime } = await loadRuntime()
  const forbidden = ['fetch(', 'XMLHttpRequest', 'WebSocket', '__TAURI_INTERNALS__', 'node:http', 'node:https']
  assert.equal(forbidden.some((token) => source.includes(token)), false)
  assert.equal(typeof runtime.onCommand, 'function')
  assert.equal(typeof runtime.createTranslator, 'function')
})

test('v3 signing truncates long input and hashes the documented field order', async () => {
  const { runtime } = await loadRuntime()
  const query = '123456789012345678901'
  const truncated = '1234567890212345678901'
  assert.equal(runtime.truncateQuery(query), truncated)

  const signingInput = `test-app-id${truncated}test-salt1787889600test-app-secret`
  const expected = createHash('sha256').update(signingInput, 'utf8').digest('hex')
  assert.equal(
    await runtime.signV3({
      appId: 'test-app-id',
      appSecret: 'test-app-secret',
      query,
      salt: 'test-salt',
      curtime: '1787889600',
    }),
    expected,
  )
})

test('English input requests Chinese and returns a copyable main result', async () => {
  const { runtime } = await loadRuntime()
  const translate = testTranslator(runtime)
  let capturedRequest = null
  const response = await translate(
    invocation('Hello world'),
    networkApi(async (request) => {
      capturedRequest = request
      return successResponse('你好，世界')
    }),
  )

  assert.equal(capturedRequest.url, 'https://openapi.youdao.com/api')
  assert.equal(capturedRequest.method, 'POST')
  assert.equal(capturedRequest.body.type, 'form')
  assert.deepEqual(
    {
      q: capturedRequest.body.value.q,
      from: capturedRequest.body.value.from,
      to: capturedRequest.body.value.to,
      appKey: capturedRequest.body.value.appKey,
      salt: capturedRequest.body.value.salt,
      signType: capturedRequest.body.value.signType,
      curtime: capturedRequest.body.value.curtime,
    },
    {
      q: 'Hello world',
      from: 'en',
      to: 'zh-CHS',
      appKey: 'test-app-id',
      salt: 'test-salt',
      signType: 'v3',
      curtime: '1787889600',
    },
  )
  assert.equal(capturedRequest.body.value.sign.length, 64)
  assert.deepEqual(response, {
    requestId: 'translate-request-1',
    results: [
      {
        id: 'translate-result',
        title: '你好，世界',
        subtitle: '英译中 · 按 Enter 复制',
        defaultAction: { type: 'copyText', text: '你好，世界' },
      },
    ],
  })
})

test('Chinese input requests English and preserves request ownership', async () => {
  const { runtime } = await loadRuntime()
  const translate = testTranslator(runtime)
  let form = null
  const response = await translate(
    invocation('你好，世界', 'translate-request-zh'),
    networkApi(async (request) => {
      form = request.body.value
      return successResponse('Hello, world', 'zh-CHS2en')
    }),
  )

  assert.equal(form.from, 'zh-CHS')
  assert.equal(form.to, 'en')
  assert.equal(response.requestId, 'translate-request-zh')
  assert.equal(response.results[0].title, 'Hello, world')
  assert.deepEqual(response.results[0].defaultAction, {
    type: 'copyText',
    text: 'Hello, world',
  })
})

test('HTTP error status becomes a readable main result', async () => {
  const { runtime } = await loadRuntime()
  const response = await testTranslator(runtime)(
    invocation('Hello'),
    networkApi(async () => ({ status: 503, headers: {}, body: 'unavailable' })),
  )

  assert.equal(response.requestId, 'translate-request-1')
  assert.equal(response.results[0].title, '翻译服务暂时不可用')
  assert.equal(response.results[0].subtitle, 'HTTP 503，请稍后重试')
  assert.equal('defaultAction' in response.results[0], false)
})

test('provider error becomes a readable main result without provider payload', async () => {
  const { runtime } = await loadRuntime()
  const response = await testTranslator(runtime)(
    invocation('Hello'),
    networkApi(async () => ({
      status: 200,
      headers: {},
      body: JSON.stringify({ errorCode: '202', message: 'sensitive-provider-detail' }),
    })),
  )

  assert.equal(response.results[0].title, '翻译服务认证失败')
  assert.equal(response.results[0].subtitle, '请检查临时测试凭据')
  assert.equal(JSON.stringify(response).includes('sensitive-provider-detail'), false)
})

const hostErrorResults = new Map([
  ['InvalidNetworkRequestError', ['翻译请求无效', '插件请求配置需要修正']],
  ['PermissionDeniedError', ['网络权限未授权', '请在插件设置中开启网络访问']],
  ['NetworkTargetDeniedError', ['翻译服务地址被拒绝', '请检查插件声明的 HTTPS 域名']],
  ['NetworkTimeoutError', ['翻译请求超时', '请稍后重试']],
  ['NetworkFailureError', ['网络连接失败', '请检查网络后重试']],
  ['NetworkResponseTooLargeError', ['翻译响应过大', '服务返回内容超过 Host 限制']],
  ['NetworkResponseInvalidError', ['翻译响应无效', '服务返回了无法处理的内容']],
  ['NetworkLimitExceededError', ['翻译请求过于频繁', '请稍后重试']],
  ['ExpiredRequestError', ['本次翻译已取消', '输入已变化或插件状态已更新']],
])

for (const [name, [title, subtitle]] of hostErrorResults) {
  test(`${name} becomes a stable main result`, async () => {
    const { runtime } = await loadRuntime()
    const error = Object.assign(new Error('redacted fixture'), { name })
    const response = await testTranslator(runtime)(
      invocation('Hello', `request-${name}`),
      networkApi(async () => {
        throw error
      }),
    )

    assert.equal(response.requestId, `request-${name}`)
    assert.equal(response.results[0].title, title)
    assert.equal(response.results[0].subtitle, subtitle)
    assert.equal(JSON.stringify(response).includes('redacted fixture'), false)
  })
}
