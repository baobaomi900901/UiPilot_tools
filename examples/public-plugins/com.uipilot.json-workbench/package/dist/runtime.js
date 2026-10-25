const STORAGE_KEY = 'workbench.draft'
const encoder = new TextEncoder()
const bytes = (text) => encoder.encode(text).length

class WorkbenchError extends Error {
  constructor(code) { super(code); this.code = code }
}
function reject(code) { throw new WorkbenchError(code) }

function exact(value, keys, code = 'invalid-payload') {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || Object.keys(value).length !== keys.length
    || keys.some((key) => !Object.hasOwn(value, key))) reject(code)
}
function textInput(text) {
  if (typeof text !== 'string') reject('invalid-payload')
  if (bytes(text) > 8192) reject('text-limit')
  return text
}

async function load(api) {
  let raw
  try { raw = await api.storage.get(STORAGE_KEY) } catch { reject('storage-unavailable') }
  if (raw === null) return { ok: true, revision: 0, text: '', hasSaved: false }
  exact(raw, ['schemaVersion', 'revision', 'text'], 'invalid-stored-data')
  if (raw.schemaVersion !== 1 || !Number.isSafeInteger(raw.revision) || raw.revision < 1
    || typeof raw.text !== 'string' || bytes(raw.text) > 8192) reject('invalid-stored-data')
  return { ok: true, revision: raw.revision, text: raw.text, hasSaved: true }
}

function format(text) {
  let value
  try { value = JSON.parse(textInput(text)) } catch (error) {
    if (error instanceof WorkbenchError) throw error
    reject('invalid-json')
  }
  // Walk parsed values before stringify: JSON.parse accepts 1e400 as Infinity.
  const pending = [{ value, depth: 0 }]
  let count = 0
  while (pending.length) {
    const item = pending.pop()
    if (++count > 2048) reject('value-limit')
    if (item.depth > 16) reject('depth-limit')
    if (typeof item.value === 'number' && !Number.isFinite(item.value)) reject('non-finite-number')
    if (item.value !== null && typeof item.value === 'object') {
      for (const child of Object.values(item.value)) pending.push({ value: child, depth: item.depth + 1 })
    }
  }
  const formatted = JSON.stringify(value, null, 2)
  if (bytes(formatted) > 32768) reject('formatted-limit')
  return { ok: true, formatted }
}

function response(requestId, data) {
  const result = { requestId, data }
  if (bytes(JSON.stringify(result)) > 64 * 1024) return { requestId, data: { ok: false, code: 'response-limit' } }
  return result
}
function failure(error) {
  return { ok: false, code: error instanceof WorkbenchError ? error.code : 'action-failed' }
}

export async function onCommand(invocation, api) {
  try { return response(invocation.requestId, await load(api)) }
  catch (error) { return response(invocation.requestId, failure(error)) }
}

export async function onWindowAction(invocation, api) {
  try {
    const input = invocation.payload
    if (invocation.action === 'load') {
      if (input !== null) reject('invalid-payload')
      return response(invocation.requestId, await load(api))
    }
    if (invocation.action === 'format') {
      exact(input, ['text'])
      return response(invocation.requestId, format(input.text))
    }
    if (invocation.action === 'save') {
      exact(input, ['text', 'expectedRevision'])
      const text = textInput(input.text)
      if (!Number.isSafeInteger(input.expectedRevision) || input.expectedRevision < 0) reject('invalid-payload')
      const current = await load(api)
      if (current.revision !== input.expectedRevision) reject('revision-conflict')
      if (current.revision === Number.MAX_SAFE_INTEGER) reject('revision-exhausted')
      const revision = current.revision + 1
      // One full-record write. A rejection may follow a commit: never retry or reset.
      try { await api.storage.set(STORAGE_KEY, { schemaVersion: 1, revision, text }) }
      catch { reject('save-uncertain') }
      return response(invocation.requestId, { ok: true, revision, text })
    }
    reject('unknown-action')
  } catch (error) { return response(invocation.requestId, failure(error)) }
}
