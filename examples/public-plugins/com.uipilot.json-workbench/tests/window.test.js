import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { JSDOM } from 'jsdom'

const html = await readFile(new URL('../package/dist/window.html', import.meta.url), 'utf8')
const source = await readFile(new URL('../package/dist/window.js', import.meta.url), 'utf8')
const loaded = (text = '', revision = 0) => ({ ok: true, text, revision, hasSaved: revision > 0 })
const tick = () => new Promise(setImmediate)
function deferred() {
  let resolve, reject
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
function page(t) {
  const dom = new JSDOM(html, { runScripts: 'outside-only' })
  t.after(() => dom.window.close())
  const { window } = dom
  const document = window.document
  const calls = []
  let update
  let currentRequest
  Object.defineProperty(window, 'uipilotPluginWindow', { value: {
    onUpdate(handler) { update = handler; return () => {} },
    get request() { return currentRequest },
    get storage() { throw new Error('content must not access storage') },
  } })
  window.TextEncoder = TextEncoder
  window.eval(source)
  const ui = Object.fromEntries(['input', 'output', 'format', 'save', 'reload', 'status', 'draft-state', 'discard', 'discard-confirm', 'discard-cancel'].map(id => [id, document.getElementById(id)]))
  function session(data = loaded(), requestId = 'open-1') {
    currentRequest = input => { const pending = deferred(); calls.push({ input, pending, requestId }); return pending.promise }
    update({ requestId, data, theme: 'light', input: '', platform: 'windows', instanceNumber: 1, invokedAt: '' })
  }
  function edit(text) { ui.input.value = text; ui.input.dispatchEvent(new window.Event('input', { bubbles: true })) }
  return { ui, calls, session, edit, window }
}

test('onUpdate captures each session without requesting during ACK and late old results cannot replace it', async t => {
  const p = page(t)
  p.session(loaded('old', 1))
  assert.equal(p.calls.length, 0)
  assert.equal(p.ui.input.value, 'old')
  p.ui.format.click()
  assert.equal(p.calls.length, 1)
  p.session(loaded('new', 2), 'open-2')
  p.calls[0].pending.resolve({ ok: true, formatted: 'stale' })
  await tick()
  assert.equal(p.ui.input.value, 'new')
  assert.equal(p.ui.output.value, '')
  p.ui.format.click()
  assert.equal(p.calls[1].requestId, 'open-2')
})

test('repeated buttons do not send another request and format cannot apply to newer edits', async t => {
  const p = page(t)
  p.session()
  p.edit('{"a":1}')
  p.ui.format.click()
  p.ui.format.click()
  p.ui.save.click()
  assert.equal(p.calls.length, 1)
  p.edit('{"a":2}')
  p.calls[0].pending.resolve({ ok: true, formatted: '{\n  "a": 1\n}' })
  await tick()
  assert.equal(p.ui.input.value, '{"a":2}')
  assert.equal(p.ui.output.value, '')
  assert.equal(p.ui.format.disabled, false)
})

test('save acknowledgement advances the persisted revision while retaining later unsaved edits', async t => {
  const p = page(t)
  p.session(loaded('old', 2))
  p.edit('draft')
  p.ui.save.click()
  assert.equal(p.calls.length, 1)
  assert.equal(p.calls[0].input.payload.expectedRevision, 2)
  p.edit('later')
  p.calls[0].pending.resolve({ ok: true, revision: 3, text: 'draft' })
  await tick()
  assert.equal(p.ui.input.value, 'later')
  assert.match(p.ui['draft-state'].textContent, /未保存/)
  p.ui.save.click()
  assert.equal(p.calls[1].input.payload.expectedRevision, 3)
  assert.equal(p.calls[1].input.payload.text, 'later')
})

test('uncertain save retains draft, prevents retry and requires inline confirmation before reload', async t => {
  const p = page(t)
  p.session()
  p.edit('{draft')
  p.ui.save.click()
  assert.equal(p.calls.length, 1)
  p.calls[0].pending.reject(Object.assign(new Error('hidden details'), { name: 'WindowRequestTimeoutError' }))
  await tick()
  assert.equal(p.ui.input.value, '{draft')
  assert.equal(p.ui.save.disabled, true)
  assert.doesNotMatch(p.ui.status.textContent, /hidden details/)
  p.ui.reload.click()
  assert.equal(p.ui.discard.hidden, false)
  assert.equal(p.calls.length, 1)
  p.ui['discard-cancel'].click()
  assert.equal(p.ui.discard.hidden, true)
  assert.equal(p.ui.input.value, '{draft')
  p.ui.reload.click()
  p.ui['discard-confirm'].click()
  assert.equal(p.calls.length, 2)
  assert.equal(p.calls[1].input.action, 'load')
  assert.equal(p.calls[1].input.payload, null)
  p.calls[1].pending.resolve(loaded('{draft', 1))
  await tick()
  assert.equal(p.ui.input.value, '{draft')
  assert.equal(p.ui.save.disabled, false)
  assert.match(p.ui['draft-state'].textContent, /已保存/)
})

test('reload cannot replace typing that occurred after confirmation', async t => {
  const p = page(t)
  p.session(loaded('saved', 1))
  p.ui.reload.click()
  assert.equal(p.calls.length, 1)
  p.edit('new typing')
  p.calls[0].pending.resolve(loaded('other', 2))
  await tick()
  assert.equal(p.ui.input.value, 'new typing')
  assert.equal(p.ui.save.disabled, true)
  assert.match(p.ui.status.textContent, /保留/)
})

test('incompatible stored data blocks saving but permits editing and formatting', async t => {
  const p = page(t)
  p.session({ ok: false, code: 'invalid-stored-data' })
  assert.equal(p.ui.save.disabled, true)
  assert.equal(p.ui.input.disabled, false)
  p.edit('{}')
  p.ui.format.click()
  assert.equal(p.calls.length, 1)
  assert.equal(p.calls[0].input.action, 'format')
  p.calls[0].pending.resolve({ ok: true, formatted: '{}' })
  await tick()
  assert.equal(p.ui.output.value, '{}')
  assert.equal(p.ui.save.disabled, true)
})
