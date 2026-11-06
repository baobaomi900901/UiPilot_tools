import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { JSDOM } from 'jsdom'
const flush = () => new Promise(resolve => setImmediate(resolve))
async function setup(t, overrides = {}) {
 const { mountPanel } = await import('../package/dist/panel.js')
 const dom = new JSDOM(await readFile(new URL('../package/dist/panel.html', import.meta.url), 'utf8'))
 let update, ready, changed, key, actions; let epoch = '1'; const calls = [], copies = [], statuses = [], focus = []; const order = []
 const host = {
  onUpdate(fn) { order.push('update'); update = fn; return () => { update = null } },
  onReady(fn) { order.push('ready'); ready = fn; return () => { ready = null } },
  onImageChanged(fn) { order.push('image'); changed = fn; return () => { changed = null } },
  onHostKey(fn) { order.push('key'); key = fn; return () => { key = null } },
  onActions(fn) { actions = fn; return () => {} },
  get request() { const owner = epoch; return async input => { calls.push({ owner, input }); return overrides.request ? overrides.request(input) : { ok: true, text: 'Original\n全文', lines: [] } } },
  get clipboard() { const owner = epoch; return { async writeText(input) { copies.push({ owner, ...input }); return overrides.copy?.(input) } } },
  get clearImage() { const owner = epoch; return async () => { if (overrides.clear) return overrides.clear(); changed({ sessionEpoch: owner, revision: '99', image: null }) } },
  get focusHostInput() { const owner = epoch; return async () => { focus.push(owner) } },
  get setStatus() { const owner = epoch; return async status => { statuses.push({ owner, ...status }) } },
 }
 const app = mountPanel(dom.window.document, host); const node = id => dom.window.document.getElementById(id)
 t.after(() => { app.dispose(); dom.window.close() })
 const install = (requestId = 'r1', sessionEpoch = '1', theme = 'light') => { epoch = sessionEpoch; update({ requestId, sessionEpoch, theme, input: '', data: {} }) }
 const signal = (requestId = 'r1', sessionEpoch = '1') => ready({ requestId, sessionEpoch })
 const image = (id = 'A', revision = '1', sessionEpoch = '1') => changed({ sessionEpoch, revision, image: id ? { id, width: 100, height: 80, previewDataUrl: 'data:image/png;base64,AAAA' } : null })
 const edit = text => { node('ocr-text').value = text; node('ocr-text').dispatchEvent(new dom.window.Event('input', { bubbles: true })) }
 const localKey = (target, key, extra = {}) => { const event = new dom.window.KeyboardEvent('keydown', { key, cancelable: true, bubbles: true, ...extra }); target.dispatchEvent(event); return event }
 return { app, dom, node, calls, copies, statuses, focus, order, install, signal, image, edit, localKey, hostKey: extra => key({ key: 'Enter', sessionEpoch: '1', routeSequence: '1', ...extra }), actions: () => actions({ source: 'content' }) }
}
test('receivers register before update; no autofocus; readiness and image initialize once', async t => {
 const a = await setup(t); assert.deepEqual(a.order, ['image', 'ready', 'key', 'update']); a.install(); a.image(); await flush()
 assert.equal(a.calls.length, 0); assert.equal(a.node('copy').disabled, true); assert.equal(a.dom.window.document.activeElement.tagName, 'BODY')
 a.signal(); await flush(); assert.equal(a.calls.length, 1); assert.equal(a.node('ocr-text').value, 'Original\n全文')
 assert.equal(a.node('image').getAttribute('src'), 'data:image/png;base64,AAAA'); assert.equal(a.node('copy').disabled, false)
 a.edit('Edited\n<b>literal</b>😀'); a.node('copy').focus(); a.node('copy').click(); await flush()
 assert.deepEqual(a.copies, [{ owner: '1', text: 'Edited\n<b>literal</b>😀' }]); assert.equal(a.dom.window.document.activeElement, a.node('copy')); assert.equal(a.statuses.at(-1).message, '已复制')
 a.install('theme', '1', 'dark'); a.signal('theme'); await flush(); assert.equal(a.dom.window.document.documentElement.dataset.theme, 'dark'); assert.equal(a.calls.length, 1); assert.equal(a.node('ocr-text').value, 'Edited\n<b>literal</b>😀')
})
test('host Enter ACK is immediate, copy uses current edited value, native focus returns even when empty', async t => {
 let finish; const a = await setup(t, { copy: () => new Promise(resolve => { finish = resolve }) }); a.install(); a.signal()
 assert.equal(a.hostKey({}), undefined); await flush(); assert.equal(a.copies.length, 0); assert.equal(a.focus.length, 1)
 a.image(); await flush(); a.edit('CURRENT'); assert.equal(a.hostKey({}), undefined); assert.equal(a.focus.length, 2)
 await flush(); assert.equal(a.copies[0].text, 'CURRENT'); assert.equal(a.hostKey({}), undefined); await flush(); assert.equal(a.copies.length, 1)
 finish(); await flush(); a.hostKey({ sessionEpoch: 'old' }); assert.equal(a.focus.length, 3, 'second Enter restored input during pending copy; stale event ignored')
})
test('local textarea newline, IME and repeats remain isolated; deterministic Tab spans main input and controls', async t => {
 const a = await setup(t); a.install(); a.signal(); a.image(); await flush(); const output = a.node('ocr-text')
 assert.equal(a.localKey(output, 'Enter').defaultPrevented, false); assert.equal(a.localKey(output, 'Enter', { isComposing: true }).defaultPrevented, false)
 assert.equal(a.localKey(a.node('copy'), 'Enter', { repeat: true }).defaultPrevented, true); assert.equal(a.copies.length, 0)
 a.hostKey({ key: 'Tab' }); assert.equal(a.dom.window.document.activeElement, a.node('clear-image'))
 a.localKey(a.node('clear-image'), 'Tab'); assert.equal(a.dom.window.document.activeElement, output)
 a.hostKey({ key: 'Tab', shiftKey: true }); assert.equal(a.dom.window.document.activeElement, a.node('provider'))
 a.localKey(a.node('provider'), 'Tab'); assert.equal(a.focus.length, 1)
 a.node('provider').click(); assert.equal(a.node('source-dialog').hasAttribute('open'), true)
 a.localKey(a.node('provider-option'), 'Escape'); assert.equal(a.node('source-dialog').hasAttribute('open'), false); assert.equal(a.dom.window.document.activeElement, a.node('provider'))
 a.node('provider').click(); a.node('provider-option').click(); await flush(); assert.equal(a.calls.length, 1); assert.equal(a.dom.window.document.activeElement, a.node('provider'))
})
test('clear pending disables old copy immediately; stale copy feedback cannot restore state', async t => {
 let finishCopy, finishClear
 const a = await setup(t, { copy: () => new Promise(resolve => { finishCopy = resolve }), clear: () => new Promise(resolve => { finishClear = resolve }) })
 a.install(); a.signal(); a.image(); await flush(); a.node('copy').click(); await flush(); a.node('clear-image').click()
 assert.equal(a.node('copy').disabled, true); assert.equal(a.node('ocr-text').value, ''); a.hostKey({}); await flush(); assert.equal(a.copies.length, 1)
 finishCopy(); await flush(); assert.equal(a.statuses.some(s => s.message === '已复制'), false)
 finishClear(); await flush(); assert.equal(a.calls.length, 1)
})
test('full long text copied unchanged, oversize edit rejected; errors and no text never copy', async t => {
 const a = await setup(t); a.install(); a.signal(); a.image(); await flush(); const text = 'a'.repeat(16000) + '\n最后'
 a.edit(text); a.node('copy').click(); await flush(); assert.equal(a.copies[0].text, text)
 a.edit('汉'.repeat(6000)); assert.equal(a.node('copy').disabled, true); a.hostKey({}); await flush(); assert.equal(a.copies.length, 1)
 a.image(null, '2'); await flush(); assert.equal(a.node('copy').disabled, true)
})
test('failure retries explicitly; copy error preserves text; new session hides old text and stale actions', async t => {
 let calls = 0; const a = await setup(t, { request: async () => ++calls === 1 ? { ok: false, code: 'quota', message: '额度不足' } : { ok: true, text: 'retry text', lines: [] }, copy: async () => { throw Error('SECRET') } })
 a.install(); a.signal(); a.image(); await flush(); assert.equal(a.node('copy').disabled, true); assert.equal(a.node('retry').hidden, false); assert.equal(a.calls.length, 1)
 a.node('retry').click(); await flush(); const oldAction = a.actions().context.find(action => action.id === 'copy')
 a.node('copy').click(); await flush(); assert.equal(a.node('ocr-text').value, 'retry text'); assert.equal(a.statuses.at(-1).message, '复制失败，请重试')
 a.install('new', '2'); a.signal('new', '2'); oldAction.run(); await flush(); assert.equal(a.node('ocr-text').value, ''); assert.equal(a.copies.length, 1)
 a.app.dispose(); a.node('copy').click(); await flush(); assert.equal(a.copies.length, 1)
})
test('new image arriving before deferred clear IPC revokes the old clear intent', async t => {
 let clearCalls = 0; const a = await setup(t, { clear: async () => { clearCalls++ } })
 a.install(); a.signal(); a.image(); await flush(); a.node('clear-image').click(); a.image('NEW', '2'); await flush()
 assert.equal(clearCalls, 0); assert.equal(a.node('image').getAttribute('src'), 'data:image/png;base64,AAAA'); assert.equal(a.calls.length, 2)
})
