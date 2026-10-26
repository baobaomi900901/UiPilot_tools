import assert from 'node:assert/strict'
import test from 'node:test'
import { existsSync, readFileSync } from 'node:fs'

const runtimeUrl = new URL('../package/dist/runtime.js', import.meta.url)
const context = { platform: 'windows', theme: 'dark', invokedAt: '2026-10-26T00:00:00+08:00' }
const record = (text = '', revision = 1) => ({ schemaVersion: 1, revision, text })
function store(initial = null, mode = '') {
  let saved = structuredClone(initial), writes = 0, reads = 0
  return {
    api: { storage: {
      async get(key) { assert.equal(key, 'snippet.latest'); reads++; if (mode === 'read-failure') throw Error('private'); return structuredClone(saved) },
      async set(key, value) { assert.equal(key, 'snippet.latest'); writes++; if (mode === 'write-failure') throw Error('private'); saved = structuredClone(value); if (mode === 'lost-ack') throw Error('private') },
    } },
    get saved() { return saved }, get writes() { return writes }, get reads() { return reads },
  }
}
async function runtime() { assert.ok(existsSync(runtimeUrl), 'standalone snippet Runtime must exist'); return import(runtimeUrl.href) }
async function command(state, commandId = 'transform', input = 'Hello 片段') {
  const reply = await (await runtime()).onCommand({ apiVersion: 1, requestId: 'command', commandId, input, context }, state.api)
  assert.equal(reply.requestId, 'command'); assert.ok(Buffer.byteLength(JSON.stringify(reply)) <= 65536)
  return reply
}
async function action(state, actionId = 'save', payload = { text: 'HELLO', expectedRevision: 0 }, commandId = 'transform', resultId = 'text') {
  const reply = await (await runtime()).onAction({ apiVersion: 1, requestId: 'action', commandId, resultId, actionId, payload, input: 'original', context }, state.api)
  assert.equal(reply.requestId, 'action'); assert.ok(Buffer.byteLength(JSON.stringify(reply)) <= 65536)
  return reply
}
const info = reply => reply.results[0].title
const allActions = reply => reply.results.flatMap(row => row.actions)

test('manifest declares only the two Windows submit commands and clipboard-write capability', async () => {
  await runtime()
  const manifest = JSON.parse(readFileSync(new URL('../package/plugin.json', import.meta.url)))
  assert.equal(manifest.pluginId, 'com.uipilot.text-snippet'); assert.equal(manifest.version, '1.1.0')
  assert.equal(manifest.minimumHostVersion, '0.12.0'); assert.deepEqual(manifest.supportedPlatforms, ['windows'])
  assert.deepEqual(manifest.permissions, ['clipboard.write', 'url.open']); assert.equal(manifest.runtime.resultActions, true)
  assert.deepEqual(manifest.commands.map(row => [row.id, row.defaultName, row.activationMode, row.outputMode, row.inputRequired]), [
    ['transform', 'text-snippet', 'submit', 'mainResult', true], ['latest', 'latest-snippet', 'submit', 'mainResult', false],
  ])
})
test('transform preserves text and returns original/uppercase copy defaults and revision-bound save choices without writing', async () => {
  const state = store(record('old', 4)); const reply = await command(state)
  assert.deepEqual(reply.results.map(row => row.title), ['原文', '大写'])
  assert.deepEqual(reply.results.map(row => row.actions[0].text), ['Hello 片段', 'HELLO 片段'])
  for (const row of reply.results) { assert.equal(row.defaultActionId, 'copy'); assert.deepEqual(row.actions[1], { id: 'save', title: '保存为最新片段', type: 'runtime', payload: { text: row.actions[0].text, expectedRevision: 4 } }) }
  assert.equal(state.writes, 0); assert.equal(state.reads, 1)
})
test('missing and saved empty strings remain distinct; latest and read never write', async () => {
  const state = store(); const missing = await command(state, 'latest', '')
  assert.equal(info(missing), '尚未保存片段'); const read = missing.results[1]
  assert.equal(read.defaultActionId, undefined); assert.deepEqual(read.actions, [{ id: 'read', title: '重新读取', type: 'runtime', payload: null }])
  assert.equal(info(await action(state, 'read', null, 'latest', 'read')), '尚未保存片段')
  const empty = store(record('')); const reply = await command(empty, 'latest', '')
  assert.equal(reply.results[0].actions[0].text, ''); assert.equal(reply.results[0].defaultActionId, 'copy')
  assert.equal(state.writes + empty.writes, 0)
})
test('save awaits one whole-record write and fresh read works for identical business IDs in both commands', async () => {
  const state = store(); const saved = await action(state)
  assert.equal(info(saved), '已保存'); assert.deepEqual(state.saved, record('HELLO')); assert.equal(state.writes, 1)
  for (const commandId of ['transform', 'latest']) {
    const reply = await action(state, 'read', null, commandId, 'text')
    assert.equal(reply.results[0].id, 'text'); assert.equal(reply.results[0].actions[0].text, 'HELLO')
  }
  assert.equal(state.writes, 1)
})
test('stale revisions and increment exhaustion never overwrite and offer explicit fresh read', async () => {
  const state = store(record('old', 3)); const reply = await action(state)
  assert.equal(info(reply), '片段已更改，请重新读取'); assert.equal(allActions(reply).some(row => row.id === 'read'), true)
  assert.deepEqual(state.saved, record('old', 3)); assert.equal(state.writes, 0)
  const full = store(record('old', Number.MAX_SAFE_INTEGER))
  assert.equal(info(await action(full, 'save', { text: 'new', expectedRevision: Number.MAX_SAFE_INTEGER })), '片段版本已达上限')
  assert.equal(full.writes, 0)
})
test('bad payloads, unsupported action identities, and missing/unknown command IDs cannot save', async () => {
  const state = store()
  for (const payload of [null, [], {}, { text: '', expectedRevision: -1 }, { text: '', expectedRevision: 0.5 }, { text: '', expectedRevision: Number.MAX_SAFE_INTEGER + 1 }, { text: '', expectedRevision: '0' }, { text: '', expectedRevision: 0, extra: true }]) assert.equal(info(await action(state, 'save', payload)), '操作参数无效')
  assert.equal(info(await action(state, 'read', {})), '操作参数无效')
  assert.equal(info(await action(state, 'save', { text: '', expectedRevision: 0 }, 'latest')), '操作参数无效')
  assert.equal(info(await action(state, 'save', { text: '', expectedRevision: 0 }, 'transform', 'wrong')), '操作参数无效')
  assert.equal(info(await action(state, 'unknown', null)), '操作参数无效')
  for (const commandId of [undefined, '', 'constructor', 'unknown']) {
    const mod = await runtime()
    await assert.rejects(mod.onCommand({ requestId: 'x', commandId, input: '', context }, state.api), /Unsupported command ID/)
    await assert.rejects(mod.onAction({ requestId: 'x', commandId, actionId: 'save', payload: {}, context }, state.api), /Unsupported command ID/)
  }
  assert.equal(state.writes, 0)
})
test('unknown schema, invalid/excessive records, and unsafe revisions block every write without reset', async () => {
  for (const raw of [{}, false, [], record('', 0), record('', -1), record('', 1.1), record('', Number.MAX_SAFE_INTEGER + 1), { ...record(), schemaVersion: 2 }, { ...record(), extra: true }, { ...record(), text: null }, record('汉'.repeat(2731)), record('\0')]) {
    const state = store(raw)
    for (const commandId of ['transform', 'latest']) assert.equal(info(await command(state, commandId)), '已保存片段无法读取，请核对数据')
    assert.equal(info(await action(state)), '已保存片段无法读取，请核对数据')
    assert.equal(state.writes, 0); assert.deepEqual(state.saved, raw)
  }
})
test('UTF-8 boundaries, uppercase expansion, NUL and compact-JSON overflow produce bounded information without actions/writes', async () => {
  const state = store()
  assert.equal((await command(state, 'transform', '😀'.repeat(2048))).results.length, 2)
  for (const text of ['汉'.repeat(2731), 'ŉ'.repeat(4096), '\0', '\u0001'.repeat(8192)]) {
    const reply = await command(state, 'transform', text); assert.equal(allActions(reply).length, 0)
  }
  assert.equal(state.writes, 0)
  for (const text of ['汉'.repeat(2731), '\0', '\u0001'.repeat(8192)]) {
    const fresh = store(); assert.equal(allActions(await action(fresh, 'save', { text, expectedRevision: 0 })).length, 0); assert.equal(fresh.writes, 0)
  }
})
test('save rejects and lost acknowledgements stay uncertain with exactly one write and no automatic reread/retry', async () => {
  for (const mode of ['write-failure', 'lost-ack']) {
    const state = store(null, mode); const reply = await action(state)
    assert.equal(info(reply), '保存结果未确认，请重新读取核对'); assert.equal(state.writes, 1); assert.equal(state.reads, 1)
    assert.equal(JSON.stringify(reply).includes('private'), false)
    const loaded = await action(state, 'read', null, 'latest', 'read')
    assert.equal(mode === 'lost-ack' ? loaded.results[0].actions[0].text : info(loaded), mode === 'lost-ack' ? 'HELLO' : '尚未保存片段')
  }
  const failed = store(null, 'read-failure'); assert.equal(info(await action(failed)), '读取失败，请重新读取'); assert.equal(failed.writes, 0)
})

test('URL actions appear only on exact valid original/latest text and preserve copy defaults without effects', async () => {
  for (const text of ['https://example.com/Case?q=%20#x', 'HTTPS://例子.测试/路径', 'http://localhost:80/%00']) {
    const state = store(record(text)); const reply = await command(state, 'transform', text)
    assert.deepEqual(reply.results[0].actions.at(-1), { id: 'open', title: '在浏览器打开', type: 'openUrl', url: text })
    assert.equal(reply.results[1].actions.some(a => a.type === 'openUrl'), false)
    const latest = await command(state, 'latest', '')
    assert.equal(latest.results[0].actions.at(-1).url, text)
    assert.equal(latest.results[0].defaultActionId, 'copy'); assert.equal(reply.results[0].defaultActionId, 'copy')
    assert.equal(state.writes, 0)
  }
  for (const text of ['ordinary text', 'https://@example.com', 'https:///example.com', 'https:example.com', 'javascript:alert(1)', 'file:///tmp/a', 'https://u:p@example.com', 'https://example.com/ a', 'https://example.com/\\x', 'https://example.com/\u0080', 'https://example.com/' + '汉'.repeat(500), 'https://example.com/' + 'x'.repeat(4096)]) {
    const state = store(record(text))
    for (const reply of [await command(state, 'transform', text), await command(state, 'latest', '')]) assert.equal(allActions(reply).some(a => a.type === 'openUrl'), false, text)
    assert.equal(state.writes, 0)
  }
})
