import test from 'node:test'
import assert from 'node:assert/strict'
import * as runtime from '../package/dist/runtime.js'

const payload = { providerId: 'youdao', text: '  hello\n ', sourceLanguage: 'auto', targetLanguage: 'zh-CHS' }
const success = { translations: ['  你好\n ', '第二行'], sourceLanguage: 'en', targetLanguage: 'zh-CHS', unchanged: false }
const invoke = (input = payload, api = {}, action = 'translate') => runtime.onPanelAction({ requestId: 'panel-request', action, payload: input }, api)
const service = translate => ({ services: { youdao: { translate } } })

for (const text of ['', '  hello\n ']) test(`initialization preserves ${JSON.stringify(text)} without translating`, async () => {
  let key
  const api = { storage: { get: async value => { key = value; return { sourceLanguage: 'en', targetLanguage: 'en', providerId: 'unknown', text: 'private' } } }, ...service(() => assert.fail('initialization must not translate')) }
  assert.deepEqual(await runtime.onCommand({ requestId: 'init', input: text }, api), {
    requestId: 'init', data: { text, preferences: { sourceLanguage: 'en', targetLanguage: 'en', providerId: 'youdao' } },
  })
  assert.equal(key, 'translate.preferences.v1')
})

test('unavailable or malformed stored preferences fall back safely', async () => {
  for (const get of [async () => null, async () => ({ targetLanguage: 'auto' }), async () => { throw Error('PRIVATE') }]) {
    const result = await runtime.onCommand({ requestId: 'init', input: '' }, { storage: { get } })
    assert.deepEqual(result.data.preferences, { sourceLanguage: 'auto', targetLanguage: 'zh-CHS', providerId: 'youdao' })
  }
})

for (const sourceLanguage of ['auto', 'en', 'zh-CHS']) test(`dispatches ${sourceLanguage} without changing text`, async () => {
  const calls = []
  const result = await invoke({ ...payload, sourceLanguage }, service(async input => { calls.push(input); return success }))
  assert.deepEqual(calls, [{ text: payload.text, from: sourceLanguage, to: 'zh-CHS' }])
  assert.deepEqual(result, { requestId: 'panel-request', data: { ok: true, text: '  你好\n \n第二行', detectedSourceLanguage: 'en', targetLanguage: 'zh-CHS' } })
})

test('unknown detection remains null', async () => {
  assert.equal((await invoke(payload, service(async () => ({ ...success, sourceLanguage: null })))).data.detectedSourceLanguage, null)
})

const invalidPayloads = [null, [], {}, { ...payload, providerId: 'constructor' }, { ...payload, providerId: '__proto__' }, { ...payload, providerId: 'other' }, { ...payload, sourceLanguage: 'fr' }, { ...payload, targetLanguage: 'auto' }, { ...payload, text: '' }, { ...payload, text: '\0' }, { ...payload, text: '中'.repeat(2731) }, { ...payload, url: 'private' }]
for (const [index, input] of invalidPayloads.entries()) test(`invalid input ${index} never invokes API`, async () => {
  const result = await invoke(input, service(() => assert.fail('invalid payload called service')))
  assert.equal(result.data.ok, false)
  assert.equal(result.data.code, 'INVALID_REQUEST')
})

test('accessors and symbols are rejected without reading payload fields', async () => {
  const accessor = { ...payload }
  Object.defineProperty(accessor, 'text', { enumerable: true, get() { assert.fail('getter evaluated') } })
  for (const input of [accessor, { ...payload, [Symbol('extra')]: true }]) assert.equal((await invoke(input)).data.code, 'INVALID_REQUEST')
})

test('unknown action is rejected before dispatch', async () => {
  assert.equal((await invoke(payload, service(() => assert.fail()), 'constructor')).data.code, 'INVALID_REQUEST')
})

test('missing service capability has a fixed failure', async () => {
  assert.deepEqual((await invoke()).data, { ok: false, code: 'SERVICE_UNAVAILABLE', message: '翻译服务不可用，需要 UiPilot 0.18.0 或更高版本' })
})

const errorMessages = {
  ServicePermissionDeniedError: '有道翻译未授权，请在设置 → 权限中开启有道翻译服务',
  PermissionDeniedError: '网络访问已关闭，请在设置 → 权限中开启网络访问',
  CredentialNotConfiguredError: '有道翻译未配置，请在设置 → 密钥配置中配置有道翻译',
  CredentialDisabledError: '有道翻译配置已关闭，请在设置 → 密钥配置中启用有道翻译',
  CredentialUnavailableError: '有道翻译密钥不可用，请在设置 → 密钥配置中检查有道翻译配置',
  ProviderAuthenticationError: '有道翻译认证失败，请在设置 → 密钥配置中检查有道翻译配置',
  ProviderQuotaError: '有道翻译额度不足或调用受限，请检查有道翻译服务额度',
  NetworkTimeoutError: '翻译请求超时，请稍后重试',
}
for (const [name, message] of Object.entries(errorMessages)) test(`safe failure ${name}`, async () => {
  const result = await invoke(payload, service(async () => { throw { name, message: 'PRIVATE' } }))
  assert.equal(result.data.ok, false)
  assert.equal(result.data.message, message)
  assert.match(result.data.code, /^[A-Z_]{1,64}$/)
  assert.ok(!JSON.stringify(result).includes('PRIVATE'))
})
for (const name of ['constructor', '__proto__', 'toString', 'UnknownError']) test(`unrecognized error ${name}`, async () => {
  assert.deepEqual((await invoke(payload, service(async () => { throw { name, message: 'PRIVATE' } }))).data, { ok: false, code: 'TRANSLATION_FAILED', message: '翻译失败，请稍后重试' })
})

for (const response of [null, {}, { ...success, translations: [] }, { ...success, translations: [42] }, { ...success, translations: [''] }, { ...success, sourceLanguage: 'fr' }, { ...success, sourceLanguage: undefined }, { ...success, targetLanguage: 'en' }, { ...success, unchanged: undefined }]) test(`malformed service response ${JSON.stringify(response)}`, async () => {
  assert.deepEqual((await invoke(payload, service(async () => response))).data, { ok: false, code: 'INVALID_RESPONSE', message: '翻译服务响应异常，请稍后重试' })
})

test('reverse direction and maximum text size use the Host service', async () => {
  const text = 'x'.repeat(8192)
  let seen
  const response = await invoke({ ...payload, text, sourceLanguage: 'zh-CHS', targetLanguage: 'en' }, service(async input => {
    seen = input
    return { translations: ['hello'], sourceLanguage: 'zh-CHS', targetLanguage: 'en', unchanged: false }
  }))
  assert.deepEqual(seen, { text, from: 'zh-CHS', to: 'en' })
  assert.equal(response.data.text, 'hello')
})

test('same-language calls still go through Host admission', async () => {
  let calls = 0
  const response = await invoke({ ...payload, sourceLanguage: 'en', targetLanguage: 'en' }, service(async input => {
    calls++
    return { translations: [input.text], sourceLanguage: 'en', targetLanguage: 'en', unchanged: true }
  }))
  assert.equal(calls, 1)
  assert.equal(response.data.text, payload.text)
})

test('sparse translation arrays are rejected', async () => {
  const response = await invoke(payload, service(async () => ({ ...success, translations: new Array(1) })))
  assert.equal(response.data.code, 'INVALID_RESPONSE')
})

test('whitespace-only explicit action never invokes the service', async () => {
  const result = await invoke({ ...payload, text: ' \t\r\n ' }, service(() => assert.fail('blank text called service')))
  assert.equal(result.data.ok, false)
  assert.equal(result.data.code, 'INVALID_REQUEST')
})
