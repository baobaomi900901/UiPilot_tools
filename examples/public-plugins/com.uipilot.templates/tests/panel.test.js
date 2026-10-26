import assert from 'node:assert/strict'
import { readFile, mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { JSDOM } from 'jsdom'

const root = new URL('../../../../', import.meta.url)
const dist = new URL('../package/dist/', import.meta.url)
let sequence = 0
const summaries = [{ id: 'template-1', title: 'Alpha' }, { id: 'template-2', title: 'Beta' }]
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))
function deferred() { let resolve; const promise = new Promise((done) => { resolve = done }); return { promise, resolve } }
async function setup(t, request = async ({ payload }) => ({ ok: true, template: { ...summaries.find((i) => i.id === payload.id), revision: 1, body: 'saved body', values: [] }, variables: [] })) {
  const dom = new JSDOM(await readFile(new URL('panel.html', dist), 'utf8'))
  const document = dom.window.document, calls = []
  let hostKey, update, inHostKey = false
  const dialog = document.querySelector('dialog'), closeEvents = []
  dialog.showModal = () => { dialog.open = true }
  dialog.close = (value) => { dialog.returnValue = value; dialog.open = false; closeEvents.push(() => dialog.dispatchEvent(new dom.window.Event('close'))) }
  dom.window.uipilotPluginPanel = {
    onHostKey(handler) { hostKey = handler },
    onUpdate(handler) { update = handler },
    async request(value) { assert.equal(inHostKey, false, 'request must be outside Host key ACK'); calls.push(value); return request(value) },
    async focusHostInput() {},
  }
  globalThis.window = dom.window; globalThis.document = document
  await import(new URL('panel.js?test=' + ++sequence, dist).href)
  t.after(() => { dom.window.close(); delete globalThis.window; delete globalThis.document })
  const api = {
    document, dialog, calls,
    node: (id) => document.getElementById(id),
    update: (overrides = {}) => update({ sessionEpoch: '1', requestId: 'r1', input: '', theme: 'dark', data: { ok: true, templates: summaries }, ...overrides }),
    host: (key) => { inHostKey = true; try { return hostKey({ key, sessionEpoch: '1' }) } finally { inHostKey = false } },
    click: (id) => document.getElementById(id).click(),
    input: (id, value) => { const node = document.getElementById(id); node.value = value; node.dispatchEvent(new dom.window.Event('input', { bubbles: true })) },
    close: (value) => dialog.close(value),
    dispatchClose: () => closeEvents.shift()?.(),
    flush,
  }
  api.update()
  return api
}
test('manifest declares arrows with content focus and keeps Host Enter as submit', async () => {
  const manifest = JSON.parse(await readFile(new URL('../package/plugin.json', import.meta.url), 'utf8'))
  assert.equal(manifest.version, '1.1.0')
  assert.equal(manifest.minimumHostVersion, '0.6.0')
  assert.deepEqual(manifest.permissions, ['ui.panel'])
  assert.deepEqual(manifest.panel.hostKeys, ['ArrowDown', 'ArrowUp', 'Primary+N', 'Tab', 'Shift+Tab'])
  assert.ok(!manifest.panel.hostKeyFocus || manifest.panel.hostKeyFocus === 'content')
  assert.equal(manifest.command.inputPlaceholder, '搜索模板名称')
})
test('real module import: arrows focus without loading; click loads once and preserves row on updates', async (t) => {
  const app = await setup(t)
  assert.equal(app.node('editor').hidden, true)
  app.host('ArrowDown')
  const row = app.document.activeElement
  assert.equal(row.textContent, 'Alpha')
  assert.equal(app.calls.length, 0)
  app.update({ theme: 'light' })
  assert.equal(app.document.activeElement, row)
  row.click(); row.click(); await flush()
  assert.equal(app.calls.length, 1)
  assert.equal(app.node('title').value, 'Alpha')
  assert.equal(row.getAttribute('aria-current'), 'true')
})
test('dirty switch cancel preserves draft, preview and loaded identity; filtering does not discard', async (t) => {
  const app = await setup(t)
  app.node('list').children[0].click(); await flush()
  app.input('body', 'unsaved body'); app.node('output').value = 'existing preview'
  app.node('list').children[1].click()
  assert.equal(app.dialog.open, true)
  const focus = app.document.activeElement
  app.host('ArrowDown'); app.host('n'); app.host('Tab')
  assert.equal(app.document.activeElement, focus)
  app.close('cancel'); app.dispatchClose()
  assert.equal(app.node('body').value, 'unsaved body')
  assert.equal(app.node('output').value, 'existing preview')
  assert.equal(app.node('list').children[0].getAttribute('aria-current'), 'true')
  assert.equal(app.calls.length, 1)
  app.update({ input: 'Beta' })
  assert.equal(app.node('body').value, 'unsaved body')
  assert.equal(app.node('list').querySelector('[aria-current="true"]'), null)
})
test('busy save retains inputs, prevents duplicate calls and marks later typing dirty', async (t) => {
  const pending = deferred()
  const app = await setup(t, () => pending.promise)
  app.click('new'); app.input('title', 'Draft'); app.input('body', 'first')
  const body = app.node('body'); body.focus()
  app.click('save'); app.click('save')
  app.input('body', 'later')
  assert.equal(app.node('body'), body)
  assert.equal(app.node('editor').hidden, false)
  assert.equal(app.node('editor').getAttribute('aria-busy'), 'true')
  assert.equal(app.calls.length, 1)
  pending.resolve({ ok: true, id: 'template-3', revision: 1, templates: [...summaries, { id: 'template-3', title: 'Draft' }], variables: [], values: [] })
  await flush()
  assert.equal(body.value, 'later')
  assert.match(app.node('draft-state').textContent, /未保存/)
  assert.equal(app.node('editor').getAttribute('aria-busy'), 'false')
})
test('new session rejects late request reply', async (t) => {
  const pending = deferred()
  const app = await setup(t, () => pending.promise)
  app.click('new'); app.input('title', 'Old'); app.click('save')
  app.update({ sessionEpoch: '2', requestId: 'r2' })
  app.click('new'); app.input('title', 'New')
  pending.resolve({ ok: true, id: 'old', revision: 1, templates: [], variables: [], values: [] })
  await flush()
  assert.equal(app.node('title').value, 'New')
  assert.equal(app.node('list').querySelectorAll('button').length, 2)
  assert.match(app.node('draft-state').textContent, /未保存/)
})
test('queued old dialog close cannot consume a new session choice', async (t) => {
  const app = await setup(t)
  app.click('new'); app.input('title', 'Old')
  app.node('list').children[0].click()
  app.update({ sessionEpoch: '2', requestId: 'r2' })
  app.click('new'); app.input('title', 'New')
  app.node('list').children[1].click()
  assert.equal(app.dialog.open, true, 'new choice can open immediately after session replacement')
  app.close('discard')
  app.dispatchClose()
  assert.equal(app.calls.length, 0, 'old queued close cannot execute new action')
  app.dispatchClose(); await flush()
  assert.equal(app.calls.length, 1)
  assert.equal(app.calls[0].payload.id, 'template-2')
})
test('uncertain new save needs a new Host submission before deliberate new draft', async (t) => {
  const app = await setup(t, async () => ({ ok: false, code: 'save-uncertain' }))
  app.click('new'); app.input('title', 'Uncertain'); app.click('save'); await flush()
  assert.equal(app.node('save').disabled, true)
  app.click('reload')
  app.update({ theme: 'light' }); app.click('new')
  assert.equal(app.dialog.open, false)
  app.update({ requestId: 'r2' }); app.click('new')
  assert.equal(app.dialog.open, true)
  app.close('cancel'); app.dispatchClose()
  assert.equal(app.node('title').value, 'Uncertain')
})
test('vendored files equal source and read-only sync check passes', async () => {
  for (const file of ['ui.js', 'ui.css']) {
    assert.equal(await readFile(new URL('ui/' + file, dist), 'utf8'), await readFile(new URL('packages/plugin-ui/src/' + file, root), 'utf8'))
  }
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('packages/plugin-ui/scripts/sync-templates.mjs', root)), '--check'], { encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
})

test('sync check returns nonzero on drift without repairing the copy', async (t) => {
  const fixture = await mkdtemp(join(tmpdir(), 'uipilot-ui-sync-'))
  t.after(async () => {
    assert.ok(resolve(fixture).startsWith(resolve(tmpdir()) + sep + 'uipilot-ui-sync-'))
    await rm(fixture, { recursive: true, force: true })
  })
  const script = join(fixture, 'packages/plugin-ui/scripts/sync-templates.mjs')
  const source = join(fixture, 'packages/plugin-ui/src')
  const target = join(fixture, 'examples/public-plugins/com.uipilot.templates/package/dist/ui')
  await mkdir(join(fixture, 'packages/plugin-ui/scripts'), { recursive: true })
  await mkdir(source, { recursive: true }); await mkdir(target, { recursive: true })
  await writeFile(script, await readFile(new URL('packages/plugin-ui/scripts/sync-templates.mjs', root)))
  for (const file of ['ui.js', 'ui.css']) {
    await writeFile(join(source, file), 'source')
    await writeFile(join(target, file), 'drift')
  }
  const result = spawnSync(process.execPath, [script, '--check'], { encoding: 'utf8' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /UI copy differs.*ui.js/)
  assert.equal(await readFile(join(target, 'ui.js'), 'utf8'), 'drift')
  assert.equal(await readFile(join(target, 'ui.css'), 'utf8'), 'drift')
})

test('replacement session abandons old disabled-row focus ownership', async (t) => {
  const pending = deferred()
  const app = await setup(t, () => pending.promise)
  app.document.hasFocus = () => true
  app.host('ArrowDown')
  app.document.activeElement.click()
  app.document.body.tabIndex = -1
  app.document.body.focus()
  app.update({ sessionEpoch: '2', requestId: 'r2' })
  assert.equal(app.document.activeElement, app.document.body)
  assert.equal(app.node('editor').hidden, true)
})
