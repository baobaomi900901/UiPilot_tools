import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
async function loadRuntime() {
  let source
  try { source = await readFile(runtimeUrl, 'utf8') }
  catch { assert.fail('OCR runtime must exist') }
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`)
}
const image = Object.freeze({ id: 'image-test-1', mimeType: 'image/png', width: 279, height: 282, byteLength: 12000 })
function invocation(overrides = {}) {
  return Object.freeze({
    apiVersion: 1, requestId: 'ocr-request-1', commandId: 'default', input: '', image,
    context: Object.freeze({ platform: 'windows', theme: 'dark', invokedAt: '2026-11-05T14:16:57Z' }),
    ...overrides,
  })
}
function apiWith(ocr) { return Object.freeze({ services: Object.freeze({ baidu: Object.freeze({ ocr }) }) }) }

test('manifest declares a submit image OCR command using only the required grants', async () => {
  let manifest
  try { manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8')) }
  catch { assert.fail('OCR manifest must exist') }
  assert.equal(manifest.pluginId, 'com.uipilot.baidu-ocr')
  assert.equal(manifest.name, '百度 OCR')
  assert.equal(manifest.minimumHostVersion, '0.20.0')
  assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.command.inputCapabilities, ['image.ocr'])
  assert.equal(manifest.command.activationMode, 'submit')
  assert.equal(manifest.command.outputMode, 'mainResult')
  assert.deepEqual([...manifest.permissions].sort(), ['clipboard.write', 'credentials.baiduOcr', 'network.https'])
  assert.deepEqual(manifest.network.httpsHosts, ['aip.baidubce.com'])
  assert.deepEqual(manifest.settings, [])
})

test('recognition calls the bound image exactly once and preserves full text and line breaks', async () => {
  const runtime = await loadRuntime()
  const text = 'D2R_loader.dll\nd3d12.dll\ndxdiag.txt\ndxilconv7.dll\nnvngx_dlss.dll\nwinhttp.dll'
  const calls = []
  const result = await runtime.onCommand(invocation(), apiWith(async (input) => {
    calls.push(input)
    return Object.freeze({ text, lines: Object.freeze(text.split('\n')) })
  }))
  assert.deepEqual(calls, [{ imageId: image.id }])
  assert.equal(result.requestId, 'ocr-request-1')
  assert.equal(result.results.length, 1)
  assert.equal(result.results[0].detail, text)
  assert.deepEqual(result.results[0].defaultAction, { type: 'copyText', text })
})

test('recognition text remains literal including markdown, html, unicode, and spaces', async () => {
  const { onCommand } = await loadRuntime()
  const text = '  # 标题\n<b>literal</b>\n```\n中 文 😀  '
  const result = await onCommand(invocation(), apiWith(async () => ({ text, lines: text.split('\n') })))
  assert.equal(result.results[0].detail, text)
  assert.equal(result.results[0].defaultAction.text, text)
})

test('empty recognition is informational and does not replace the clipboard', async () => {
  const { onCommand } = await loadRuntime()
  const result = await onCommand(invocation(), apiWith(async () => ({ text: '', lines: [] })))
  assert.equal(result.results[0].title, '未识别到文字')
  assert.equal(result.results[0].defaultAction, undefined)
})

test('a missing image never calls the service', async () => {
  const { onCommand } = await loadRuntime()
  let called = false
  const result = await onCommand(invocation({ image: undefined }), apiWith(async () => { called = true }))
  assert.equal(called, false)
  assert.equal(result.results[0].title, '请粘贴一张图片')
  assert.equal(result.results[0].defaultAction, undefined)
})

test('service facade absence requests an updated host without exposing an exception', async () => {
  const { onCommand } = await loadRuntime()
  const result = await onCommand(invocation(), Object.freeze({}))
  assert.match(result.results[0].title, /宿主.*升级|升级.*宿主/)
  assert.equal(result.results[0].defaultAction, undefined)
})

for (const [name, title] of [
  ['CredentialNotConfiguredError', '请先配置百度 OCR'],
  ['CredentialDisabledError', '请在设置中启用百度 OCR'],
  ['ServicePermissionDeniedError', '请在设置中授权百度 OCR'],
  ['PermissionDeniedError', '请在设置中授权网络访问'],
  ['ProviderAuthenticationError', '百度 OCR 认证失败'],
  ['ProviderQuotaError', '百度 OCR 额度不足或请求受限'],
  ['NetworkTimeoutError', '百度 OCR 请求超时'],
  ['NetworkFailureError', '无法连接百度 OCR'],
]) {
  test(`handles ${name} without copy action or automatic retry`, async () => {
    const { onCommand } = await loadRuntime()
    let calls = 0
    const result = await onCommand(invocation(), apiWith(async () => {
      calls++
      throw Object.assign(new Error('sensitive provider payload'), { name })
    }))
    assert.equal(calls, 1)
    assert.equal(result.results[0].title, title)
    assert.equal(result.results[0].defaultAction, undefined)
    assert.doesNotMatch(JSON.stringify(result), /sensitive provider payload/)
  })
}

test('expired invocation propagates cancellation instead of publishing a stale error', async () => {
  const { onCommand } = await loadRuntime()
  const failure = Object.assign(new Error('expired'), { name: 'ExpiredRequestError' })
  await assert.rejects(onCommand(invocation(), apiWith(async () => { throw failure })), failure)
})

test('unknown service errors show fixed text without secrets', async () => {
  const { onCommand } = await loadRuntime()
  const result = await onCommand(invocation(), apiWith(async () => { throw new Error('token=private') }))
  assert.equal(result.results[0].title, '百度 OCR 识别失败')
  assert.doesNotMatch(JSON.stringify(result), /private/)
  assert.equal(result.results[0].defaultAction, undefined)
})

test('long text is never silently truncated into a copy action', async () => {
  const { onCommand } = await loadRuntime()
  const text = '汉'.repeat(6000)
  const result = await onCommand(invocation(), apiWith(async () => ({ text, lines: [text] })))
  assert.equal(result.results[0].title, '识别结果过长，请分区域识别')
  assert.equal(result.results[0].defaultAction, undefined)
})

test('escaped response overflow and malformed output cannot become a partial result', async () => {
  const { onCommand } = await loadRuntime()
  for (const text of ['\t'.repeat(16384), 'bad\0text', 7]) {
    const result = await onCommand(invocation(), apiWith(async () => ({ text, lines: [] })))
    assert.equal(result.results[0].defaultAction, undefined)
  }
})

test('unknown command identity never invokes OCR', async () => {
  const { onCommand } = await loadRuntime()
  let called = false
  const result = await onCommand(invocation({ commandId: 'unknown' }), apiWith(async () => { called = true }))
  assert.equal(called, false)
  assert.equal(result.results[0].defaultAction, undefined)
})
