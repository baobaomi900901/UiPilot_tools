import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
const runtime = () => import('../package/dist/runtime.js')
const invocation = { requestId: 'r1', action: 'recognize', payload: { providerId: 'baidu' }, image: { id: 'runtime-image' } }
const service = fn => ({ services: { baidu: { ocr: fn } } })
test('new identity declares minimal image Panel contract', async () => {
 const m = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url)))
 assert.equal(m.pluginId, 'com.uipilot.ocr'); assert.equal(m.name, 'ocr'); assert.equal(m.version, '1.1.0')
 assert.equal(m.minimumHostVersion, '0.22.0'); assert.equal(m.command.defaultName, 'ocr')
 assert.equal(m.command.outputMode, 'panel'); assert.equal(m.command.inputRequired, false); assert.equal(m.command.inputPlaceholder, '粘贴图片')
 assert.deepEqual(m.command.inputCapabilities, ['image.ocr'])
 assert.deepEqual(m.share, { version: 1, source: true })
 assert.deepEqual(m.permissions.sort(), ['clipboard.write', 'credentials.baiduOcr', 'network.https', 'ui.panel'])
 assert.deepEqual(m.panel, { entry: 'dist/panel.html', runtimeActions: true, hostKeys: ['Enter', 'Tab', 'Shift+Tab'], hostKeyFocus: 'content' })
})
test('onCommand only initializes; service authority belongs to onPanelAction', async () => {
 const r = await runtime(); let calls = 0
 for (const image of [undefined, { id: 'command-image' }]) assert.deepEqual(await r.onCommand({ requestId: 'init', image }, service(() => calls++)), { requestId: 'init', data: {} })
 assert.equal(calls, 0)
 const text = '  # 标题\n<b>literal</b>\n😀  '; const inputs = []
 assert.deepEqual(await r.onPanelAction({ ...invocation, payload: { providerId: 'baidu', imageId: 'FORGED' } }, service(async input => { inputs.push(input); return { text, lines: text.split('\n') } })), { requestId: 'r1', data: { ok: true, text, lines: text.split('\n') } })
 assert.deepEqual(inputs, [{ imageId: 'runtime-image' }])
})
for (const [name, message] of Object.entries({ CredentialNotConfiguredError: '请先配置百度 OCR', CredentialDisabledError: '请在设置中启用百度 OCR', CredentialUnavailableError: '百度 OCR 凭据不可用，请重新配置', ServicePermissionDeniedError: '请在设置中授权百度 OCR', PermissionDeniedError: '请在设置中授权网络访问', NetworkTargetDeniedError: '百度 OCR 网络目标未获授权', ProviderAuthenticationError: '百度 OCR 认证失败', ProviderQuotaError: '百度 OCR 额度不足或请求受限', NetworkTimeoutError: '百度 OCR 请求超时', NetworkFailureError: '无法连接百度 OCR', NetworkLimitExceededError: '请求过于频繁，请稍后重试', NetworkResponseTooLargeError: '识别结果过长，请分区域识别', NetworkResponseInvalidError: '百度 OCR 返回了无效结果', InvalidServiceRequestError: '图片不可用，请重新粘贴', UnknownError: '百度 OCR 识别失败' })) test(`safe ${name}, no retry`, async () => {
 const r = await runtime(); let calls = 0
 const result = await r.onPanelAction(invocation, service(async () => { calls++; throw { name, message: 'SECRET' } }))
 assert.equal(calls, 1); assert.equal(result.data.ok, false); assert.equal(result.data.message, message); assert.doesNotMatch(JSON.stringify(result), /SECRET/)
})
test('missing authority, provider and action never call service; empty/invalid/oversize never succeeds', async () => {
 const r = await runtime(); let calls = 0
 for (const change of [{ image: undefined }, { action: 'bad' }, { payload: { providerId: 'other' } }]) assert.equal((await r.onPanelAction({ ...invocation, ...change }, service(() => calls++))).data.ok, false)
 assert.equal(calls, 0); assert.equal((await r.onPanelAction(invocation, {})).data.ok, false)
 for (const text of ['', ' \n ', 'bad\0text', 7, '汉'.repeat(6000), '\t'.repeat(16384)]) assert.equal((await r.onPanelAction(invocation, service(async () => ({ text, lines: [] })))).data.ok, false)
})
test('expired authority propagates', async () => {
 const r = await runtime(); const error = Object.assign(new Error('expired'), { name: 'ExpiredRequestError' })
 await assert.rejects(r.onPanelAction(invocation, service(async () => { throw error })), error)
})
