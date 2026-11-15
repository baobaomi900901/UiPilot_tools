const STORAGE_KEY = 'templates.catalog-v1'
const NAME = /^[A-Za-z][A-Za-z0-9_]{0,31}$/
const ID = /^template-([1-9][0-9]*)$/
const encoder = new TextEncoder()
const bytes = (value) => encoder.encode(value).length
const chars = (value) => [...value].length

class ValidationError extends Error {
  constructor(code) { super(code); this.code = code }
}

function reject(code) { throw new ValidationError(code) }

function exact(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || Object.keys(value).length !== keys.length
    || keys.some((key) => !Object.hasOwn(value, key))) reject('invalid-payload')
}

function parse(body) {
  if (typeof body !== 'string' || bytes(body) > 8192) reject('body-limit')
  const variables = []
  const parts = []
  let literal = ''
  for (let index = 0; index < body.length;) {
    const char = body[index]
    if (char !== '{' && char !== '}') {
      literal += char
      index += 1
      continue
    }
    if (literal) { parts.push({ text: literal }); literal = '' }
    const match = /^\{\{\s*([A-Za-z][A-Za-z0-9_]{0,31})\s*\}\}/.exec(body.slice(index))
    if (!match) reject('invalid-syntax')
    const name = match[1]
    if (!variables.includes(name)) variables.push(name)
    if (variables.length > 16) reject('variable-limit')
    parts.push({ name })
    index += match[0].length
  }
  if (literal) parts.push({ text: literal })
  return { variables, parts }
}

function samples(input, variables) {
  if (!Array.isArray(input)) reject('invalid-values')
  const entries = new Map()
  for (const entry of input) {
    exact(entry, ['name', 'value'])
    if (typeof entry.name !== 'string' || !NAME.test(entry.name)
      || typeof entry.value !== 'string' || chars(entry.value) > 256
      || entries.has(entry.name)) reject('invalid-values')
    entries.set(entry.name, entry.value)
  }
  return variables.filter((name) => entries.has(name)).map((name) => ({ name, value: entries.get(name) }))
}

function content(title, body, values) {
  if (typeof title !== 'string' || !title.trim() || chars(title) > 80) reject('invalid-title')
  const parsed = parse(body)
  return { title, body, values: samples(values, parsed.variables), variables: parsed.variables }
}

function validateCatalog(raw) {
  if (raw === null) return { schemaVersion: 1, nextId: 1, templates: [] }
  try {
    exact(raw, ['schemaVersion', 'nextId', 'templates'])
    if (raw.schemaVersion !== 1 || !Number.isSafeInteger(raw.nextId) || raw.nextId < 1
      || !Array.isArray(raw.templates) || raw.templates.length > 20) reject('catalog-invalid')
    const ids = new Set()
    const templates = raw.templates.map((item) => {
      exact(item, ['id', 'revision', 'title', 'body', 'values'])
      if (typeof item.id !== 'string' || !ID.test(item.id) || ids.has(item.id)
        || !Number.isSafeInteger(Number(item.id.slice(9))) || Number(item.id.slice(9)) >= raw.nextId
        || !Number.isSafeInteger(item.revision) || item.revision < 1) reject('catalog-invalid')
      ids.add(item.id)
      const normalized = content(item.title, item.body, item.values)
      if (normalized.values.length !== item.values.length || normalized.values.some((entry, index) => entry.name !== item.values[index].name || entry.value !== item.values[index].value)) reject('catalog-invalid')
      return { id: item.id, revision: item.revision, title: item.title, body: item.body, values: normalized.values }
    })
    const catalog = { schemaVersion: 1, nextId: raw.nextId, templates }
    if (bytes(JSON.stringify(catalog)) > 512 * 1024) reject('catalog-invalid')
    return catalog
  } catch { reject('catalog-invalid') }
}

async function readCatalog(api) {
  let raw
  try { raw = await api.storage.get(STORAGE_KEY) } catch { reject('storage-unavailable') }
  return validateCatalog(raw)
}

function metadata(catalog) {
  return catalog.templates.map(({ id, revision, title }) => ({ id, revision, title }))
}

function result(requestId, data) {
  const response = { requestId, data }
  if (bytes(JSON.stringify(response)) > 64 * 1024) return { requestId, data: { ok: false, code: 'response-limit' } }
  return response
}

export async function onCommand(invocation, api) {
  let data
  try {
    const catalog = await readCatalog(api)
    const filter = invocation.input.trim().toLocaleLowerCase()
    data = { ok: true, filter: invocation.input, templates: metadata(catalog).filter((item) => item.title.toLocaleLowerCase().includes(filter)) }
  } catch (error) {
    data = { ok: false, code: error instanceof ValidationError ? error.code : 'storage-unavailable' }
  }
  return result(invocation.requestId, data)
}

export async function onPanelAction(invocation, api) {
  try {
    const input = invocation.payload
    if (invocation.action === 'preview') {
      exact(input, ['body', 'values'])
      const { variables, parts } = parse(input.body)
      const values = samples(input.values, variables)
      const supplied = new Map(values.map(({ name, value }) => [name, value]))
      const missing = variables.filter((name) => !supplied.has(name))
      const text = parts.map((part) => Object.hasOwn(part, 'text') ? part.text : supplied.get(part.name) ?? '').join('')
      if (bytes(text) > 32 * 1024) reject('preview-limit')
      return result(invocation.requestId, { ok: missing.length === 0, code: missing.length ? 'missing-variable' : 'previewed', variables, missing, values, text })
    }
    if (invocation.action === 'load') {
      exact(input, ['id'])
      if (typeof input.id !== 'string' || !ID.test(input.id)) reject('invalid-payload')
      const catalog = await readCatalog(api)
      const template = catalog.templates.find((item) => item.id === input.id)
      if (!template) reject('not-found')
      return result(invocation.requestId, { ok: true, template, variables: parse(template.body).variables })
    }
    if (invocation.action === 'save') {
      exact(input, ['id', 'expectedRevision', 'title', 'body', 'values'])
      const normalized = content(input.title, input.body, input.values)
      const catalog = await readCatalog(api)
      let template
      if (input.id === null && input.expectedRevision === null) {
        if (catalog.templates.length >= 20) reject('template-limit')
        if (catalog.nextId >= Number.MAX_SAFE_INTEGER) reject('counter-exhausted')
        template = { id: `template-${catalog.nextId}`, revision: 1, title: normalized.title, body: normalized.body, values: normalized.values }
        catalog.nextId += 1
        catalog.templates.push(template)
      } else {
        if (typeof input.id !== 'string' || !ID.test(input.id)
          || !Number.isSafeInteger(input.expectedRevision) || input.expectedRevision < 1) reject('invalid-payload')
        const index = catalog.templates.findIndex((item) => item.id === input.id)
        if (index < 0 || catalog.templates[index].revision !== input.expectedRevision) reject('conflict')
        if (input.expectedRevision >= Number.MAX_SAFE_INTEGER) reject('counter-exhausted')
        template = { id: input.id, revision: input.expectedRevision + 1, title: normalized.title, body: normalized.body, values: normalized.values }
        catalog.templates[index] = template
      }
      if (bytes(JSON.stringify(catalog)) > 512 * 1024) reject('catalog-capacity')
      // One complete snapshot write; no cache or second metadata write can get ahead of storage.
      try { await api.storage.set(STORAGE_KEY, catalog) } catch { reject('save-uncertain') }
      return result(invocation.requestId, { ok: true, id: template.id, revision: template.revision, values: template.values, variables: normalized.variables, templates: metadata(catalog) })
    }
    reject('unknown-action')
  } catch (error) {
    return result(invocation.requestId, { ok: false, code: error instanceof ValidationError ? error.code : 'action-failed' })
  }
}
