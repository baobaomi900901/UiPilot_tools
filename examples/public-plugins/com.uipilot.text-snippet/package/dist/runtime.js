const STORAGE_KEY = 'snippet.latest'
const encoder = new TextEncoder()
const bytes = text => encoder.encode(text).length
const messages = {
  'invalid-payload': '操作参数无效',
  'text-limit': '文本超过可处理上限',
  'unsupported-text': '文本包含不支持的字符',
  'response-limit': '文本超过结果容量，请缩短后重试',
  'invalid-stored-data': '已保存片段无法读取，请核对数据',
  'storage-unavailable': '读取失败，请重新读取',
  'revision-conflict': '片段已更改，请重新读取',
  'revision-exhausted': '片段版本已达上限',
  'save-uncertain': '保存结果未确认，请重新读取核对',
}
class SnippetError extends Error {
  constructor(code) { super(code); this.code = code }
}
const reject = code => { throw new SnippetError(code) }
function exact(value, keys, code = 'invalid-payload') {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))
    || Object.keys(value).length !== keys.length
    || keys.some(key => !Object.hasOwn(value, key))) reject(code)
}
function textInput(text) {
  if (typeof text !== 'string') reject('invalid-payload')
  if (bytes(text) > 8192) reject('text-limit')
  if (text.includes('\0')) reject('unsupported-text')
  return text
}
function commandIdentity(commandId) {
  if (commandId !== 'transform' && commandId !== 'latest') throw new Error('Unsupported command ID')
}
async function load(api) {
  let raw
  try { raw = await api.storage.get(STORAGE_KEY) } catch { reject('storage-unavailable') }
  if (raw === null) return { revision: 0, text: '', hasSaved: false }
  exact(raw, ['schemaVersion', 'revision', 'text'], 'invalid-stored-data')
  if (raw.schemaVersion !== 1 || !Number.isSafeInteger(raw.revision) || raw.revision < 1
    || typeof raw.text !== 'string' || bytes(raw.text) > 8192 || raw.text.includes('\0')) reject('invalid-stored-data')
  return { revision: raw.revision, text: raw.text, hasSaved: true }
}
const information = title => ({ id: 'status', title, actions: [] })
const readAction = () => ({ id: 'read', title: '重新读取', type: 'runtime', payload: null })
const readRow = () => ({ id: 'read', title: '重新读取最新片段', actions: [readAction()] })
const copyAction = text => ({ id: 'copy', title: '复制', type: 'copyText', text })
// Eligibility only; the Host independently validates and owns the browser effect.
function urlActions(text) {
  if (bytes(text) > 4096 || /[\s\x00-\x1f\x7f-\x9f\\]/u.test(text)) return []
  const prefix = /^https?:\/\//iu.exec(text)
  if (!prefix) return []
  const authority = text.slice(prefix[0].length).split(/[/?#]/u)[0]
  if (!authority || authority.includes('@')) return []
  try {
    const url = new URL(text)
    if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password || bytes(url.href) > 4096) return []
    return [{ id: 'open', title: '在浏览器打开', type: 'openUrl', url: text }]
  } catch { return [] }
}
const pasteActions = text => text.length ? [{ id: 'paste', title: '粘贴到原应用', type: 'pasteText', text }] : []
function latestRows(current) {
  if (!current.hasSaved) return [information('尚未保存片段'), readRow()]
  return [{ id: 'text', title: '最新片段', detail: current.text, actions: [copyAction(current.text), readAction(), ...urlActions(current.text), ...pasteActions(current.text)], defaultActionId: 'copy' }]
}
function response(requestId, results) {
  const reply = { requestId, results }
  if (bytes(JSON.stringify(reply)) > 65536) reject('response-limit')
  return reply
}
function failure(requestId, error) {
  const code = error instanceof SnippetError ? error.code : 'action-failed'
  const rows = [information(messages[code] ?? '操作未完成，请重新读取核对')]
  if (!['text-limit', 'unsupported-text', 'response-limit'].includes(code)) rows.push(readRow())
  return { requestId, results: rows }
}
export async function onCommand(invocation, api) {
  commandIdentity(invocation.commandId)
  try {
    if (invocation.commandId === 'latest') return response(invocation.requestId, latestRows(await load(api)))
    const text = textInput(invocation.input)
    const upper = textInput(text.toUpperCase())
    const current = await load(api)
    return response(invocation.requestId, [[text, 'text', '原文'], [upper, 'uppercase', '大写']].map(([value, id, title]) => ({
      id, title, detail: value, defaultActionId: 'copy', actions: [copyAction(value),
        { id: 'save', title: '保存为最新片段', type: 'runtime', payload: { text: value, expectedRevision: current.revision } }, ...(id === 'text' ? urlActions(value) : []), ...pasteActions(value)],
    })))
  } catch (error) { return failure(invocation.requestId, error) }
}
export async function onAction(invocation, api) {
  commandIdentity(invocation.commandId)
  try {
    if (invocation.actionId === 'read' && ['text', 'read'].includes(invocation.resultId)) {
      if (invocation.payload !== null) reject('invalid-payload')
      return response(invocation.requestId, latestRows(await load(api)))
    }
    if (invocation.commandId !== 'transform' || invocation.actionId !== 'save'
      || !['text', 'uppercase'].includes(invocation.resultId)) reject('invalid-payload')
    exact(invocation.payload, ['text', 'expectedRevision'])
    const text = textInput(invocation.payload.text)
    const expected = invocation.payload.expectedRevision
    if (!Number.isSafeInteger(expected) || expected < 0) reject('invalid-payload')
    const current = await load(api)
    if (current.revision !== expected) reject('revision-conflict')
    if (current.revision === Number.MAX_SAFE_INTEGER) reject('revision-exhausted')
    const revision = current.revision + 1
    // Prepare the complete bounded reply before the sole write. A rejection may follow a commit.
    const reply = response(invocation.requestId, [information('已保存'), ...latestRows({ text, revision, hasSaved: true })])
    try { await api.storage.set(STORAGE_KEY, { schemaVersion: 1, revision, text }) }
    catch { reject('save-uncertain') }
    return reply
  } catch (error) { return failure(invocation.requestId, error) }
}
