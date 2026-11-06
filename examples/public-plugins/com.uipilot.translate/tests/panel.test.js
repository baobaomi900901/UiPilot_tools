import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { JSDOM } from 'jsdom'

const dist = new URL('../package/dist/', import.meta.url)
const flush = () => new Promise(resolve => setTimeout(resolve, 15))
function deferred() { let resolve; const promise = new Promise(done => { resolve = done }); return { promise, resolve } }
async function setup(t, options = {}) {
  const html = await readFile(new URL('panel.html', dist), 'utf8').catch(() => '')
  assert.ok(html.includes('id="source-text"'), 'editable source Panel is implemented')
  const { mountPanel } = await import(new URL('panel.js', dist))
  const dom = new JSDOM(html)
  const document = dom.window.document
  const order = [], calls = [], saves = [], statuses = []
  let update, ready, actions, hostKey, sourceProvider, receiver, inUpdate = false, current = 'A'
  const host = {
    share: { onSource(fn) { sourceProvider = fn; return () => {} }, onReceive(fn) { receiver = fn; return () => {} } },
    onReady(fn) { order.push('ready'); ready = fn; return () => {} },
    onUpdate(fn) { order.push('update'); update = fn; return () => {} },
    onActions(fn) { actions = fn; return () => {} },
    onHostKey(fn) { hostKey = fn; return () => {} },
    get request() { const owner = current; return value => { assert.equal(inUpdate, false); calls.push({ owner, ...value }); return options.request?.(value) ?? Promise.resolve({ ok: true, text: '<img src=x onerror=alert(1)>\n你好', detectedSourceLanguage: 'en', targetLanguage: value.payload.targetLanguage }) } },
    get storage() { const owner = current; return { set: (key, value) => { saves.push({ owner, key, value }); return options.save?.(owner, value) ?? Promise.resolve() } } },
    get setStatus() { const owner = current; return value => { statuses.push({ owner, ...value }); return Promise.resolve() } },
  }
  Object.assign(host, options.effects ?? {})
  const app = mountPanel(document, host, options.adapter ? { createCapabilities: options.adapter } : {})
  t.after(() => { app.dispose(); dom.window.close() })
  const node = id => document.getElementById(id)
  const api = { receive: event => receiver(event), shareSource: () => sourceProvider?.(), app, dom, document, node, order, calls, saves, statuses, actions: () => actions({ source: 'content' }),
    update(owner = 'A', text = 'hello', preferences, requestId = owner) { current = owner; inUpdate = true; try { const result = update({ sessionEpoch: owner, requestId, input: text, theme: owner === 'B' ? 'light' : 'dark', data: { text, preferences } }); assert.equal(result, undefined) } finally { inUpdate = false } },
    ready(owner = current) { ready({ sessionEpoch: owner, requestId: owner }) },
    key(id, key, props = {}) { const event = new dom.window.KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...props }); node(id).dispatchEvent(event); return event },
    input(id, value) { node(id).value = value; node(id).dispatchEvent(new dom.window.Event('input', { bubbles: true })) },
    hostKey(event) { hostKey(event) },
  }
  return api
}

test('Host readiness precedes update registration, replaces input/theme without stealing focus, and uses current request facade', async t => {
  const a = await setup(t)
  assert.deepEqual(a.order, ['ready', 'update'])
  a.update(); assert.equal(a.document.activeElement, a.document.body)
  await flush(); assert.equal(a.calls.length, 0)
  a.ready(); await flush(); assert.equal(a.calls[0].owner, 'A')
  assert.equal(a.node('translation').textContent, '<img src=x onerror=alert(1)>\n你好')
  assert.equal(a.node('translation').querySelector('img'), null)
  a.update('B', 'new'); assert.equal(a.node('source-text').value, 'new')
  assert.equal(a.node('translation').textContent, '')
  assert.equal(a.document.documentElement.dataset.theme, 'light')
  a.ready('A'); await flush(); assert.equal(a.calls.length, 1)
  a.ready('B'); await flush(); assert.equal(a.calls[1].owner, 'B')
})

test('source and detection use same searchable selector; keyboard selection restores focus; target excludes auto', async t => {
  const a = await setup(t); a.update(); a.ready(); await flush()
  a.node('source-language').click()
  assert.equal(a.node('choice-dialog').open, true)
  assert.equal(a.actions(), null)
  assert.equal(a.node('choice-options').children.length, 113)
  a.input('choice-search', '英语')
  a.key('choice-search', 'ArrowDown'); a.key('choice-search', 'Enter')
  assert.equal(a.node('choice-dialog').open, false)
  assert.equal(a.document.activeElement, a.node('source-language'))
  assert.equal(a.node('source-language').textContent.includes('英语'), true)
  a.node('target-language').click()
  assert.equal(a.node('choice-options').textContent.includes('自动检测'), false)
  const event = a.key('choice-search', 'Escape')
  assert.equal(event.defaultPrevented, true)
  assert.equal(a.node('choice-dialog').open, false)
  assert.equal(a.document.activeElement, a.node('target-language'))
  a.update('B'); a.ready(); await flush()
  a.node('detected-language').click()
  assert.equal(a.node('choice-title').textContent, '源语言')
  a.key('choice-search', 'Escape')
  assert.equal(a.document.activeElement, a.node('detected-language'))
})

test('single provider opens real selector and capability controls explain unavailable status', async t => {
  const a = await setup(t); a.update()
  a.node('provider').click()
  assert.equal(a.node('choice-options').children.length, 1)
  assert.equal(a.node('choice-options').textContent.includes('有道翻译'), true)
  a.key('choice-search', 'Escape')
  for (const id of ['source-copy', 'source-speak', 'translation-copy', 'translation-speak']) {
    assert.equal(a.node(id).disabled, true)
    assert.ok(a.node(id).title.length > 0)
    assert.ok(a.node(id).getAttribute('aria-label'))
  }
  assert.equal(a.node('swap').disabled, true)
  assert.ok(a.node('swap').title)
})

test('plain Enter and IME do not submit; Ctrl+Enter submits; stale action callbacks do nothing', async t => {
  const a = await setup(t); a.update(); a.ready(); await flush()
  const stale = a.actions().context.find(action => action.id === 'translate')
  assert.equal(a.key('source-text', 'Enter').defaultPrevented, false)
  a.node('source-text').dispatchEvent(new a.dom.window.CompositionEvent('compositionstart'))
  a.input('source-text', '组词')
  a.key('source-text', 'Enter', { ctrlKey: true, isComposing: true }); await flush()
  assert.equal(a.calls.length, 1)
  a.node('source-text').dispatchEvent(new a.dom.window.CompositionEvent('compositionend'))
  a.key('source-text', 'Enter', { ctrlKey: true }); await flush()
  assert.equal(a.calls.length, 2)
  stale.run(); await flush(); assert.equal(a.calls.length, 2)
  a.input('source-text', 'edited')
  assert.equal(a.node('translation').textContent, '')
  assert.equal(a.node('swap').disabled, true)
})

test('preferences writes serialize/coalesce and preserve captured ownership without storing text', async t => {
  const first = deferred()
  const a = await setup(t, { save: owner => owner === 'A' ? first.promise : Promise.resolve() })
  const choose = (id, query) => { a.node(id).click(); a.input('choice-search', query); a.key('choice-search', 'Enter') }
  a.update(); a.ready(); choose('source-language', '英语'); await flush()
  assert.equal(a.saves.length, 1)
  choose('target-language', '英语'); choose('target-language', '简体中文')
  a.update('B'); a.ready(); choose('target-language', '英语'); await flush()
  assert.equal(a.saves.length, 1)
  first.resolve(); await flush()
  assert.equal(a.saves.length, 2)
  assert.equal(a.saves[1].owner, 'B')
  assert.deepEqual(Object.keys(a.saves[1].value).sort(), ['providerId', 'sourceLanguage', 'targetLanguage'])
})

test('unavailable readiness stays explicit and disposing removes Host callbacks/timers', async t => {
  const a = await setup(t)
  a.update(); a.ready(); await flush()
  a.app.dispose()
  a.input('source-text', 'after dispose'); await flush()
  assert.equal(a.calls.length, 1)
})

test('same-session resubmit preserves unsaved local preferences while new session uses Runtime preferences', async t => {
  const a = await setup(t)
  a.update(); a.ready()
  a.node('source-language').click(); a.input('choice-search', '英语'); a.key('choice-search', 'Enter')
  a.update('A', 'replacement', { sourceLanguage: 'auto', targetLanguage: 'zh-CHS', providerId: 'youdao' })
  assert.ok(a.node('source-language').textContent.includes('英语'))
  a.update('B', '', { sourceLanguage: 'auto', targetLanguage: 'en', providerId: 'youdao' })
  assert.ok(a.node('source-language').textContent.includes('自动检测'))
  assert.ok(a.node('target-language').textContent.includes('英语'))
})

test('uncertain save failure is reported once without automatic retry or text persistence', async t => {
  const a = await setup(t, { save: () => Promise.reject(new Error('private native diagnostic')) })
  a.update(); a.ready()
  a.node('source-language').click(); a.input('choice-search', '英语'); a.key('choice-search', 'Enter')
  await flush(); await flush()
  assert.equal(a.saves.length, 1)
  assert.equal(a.statuses.length, 1)
  assert.equal(a.statuses[0].message.includes('private'), false)
  assert.equal(a.saves[0].value.text, undefined)
})

test('choosing manual language from detection restores visible source control and modal traps Tab', async t => {
  const a = await setup(t); a.update(); a.ready(); await flush()
  a.node('detected-language').click()
  assert.equal(a.document.activeElement, a.node('choice-search'))
  a.key('choice-search', 'Tab')
  assert.equal(a.document.activeElement, a.node('choice-close'))
  a.key('choice-close', 'Tab', { shiftKey: true })
  assert.equal(a.document.activeElement, a.node('choice-search'))
  a.input('choice-search', '英语'); a.key('choice-search', 'Enter')
  assert.equal(a.document.activeElement, a.node('source-language'))
})

test('refresh keys are suppressed and search shortcut stays within the open selector', async t => {
  const a = await setup(t); a.update()
  for (const [key, props] of [['r', { ctrlKey: true }], ['R', { ctrlKey: true, shiftKey: true }], ['F5', {}]]) assert.equal(a.key('source-text', key, props).defaultPrevented, true)
  a.node('source-language').click(); a.node('choice-close').focus()
  assert.equal(a.key('choice-close', 'f', { ctrlKey: true }).defaultPrevented, true)
  assert.equal(a.document.activeElement, a.node('choice-search'))
})

test('injected effects receive captured facades, invalidation and disposal; stale menus cannot run them', async t => {
  const sessions = [], runs = [], invalidations = [], disposals = []
  const a = await setup(t, { adapter: context => {
    sessions.push(context)
    return { availability: () => null, run: (...args) => runs.push(args), invalidate: () => invalidations.push(context), dispose: () => disposals.push(context) }
  } })
  a.update(); a.ready(); await flush()
  assert.equal(a.node('source-copy').disabled, false)
  a.node('source-copy').click()
  assert.equal(runs[0][0], 'copy')
  const stale = a.actions().context.find(item => item.id === 'source-copy')
  a.input('source-text', 'edited')
  assert.equal(invalidations.length, 1)
  stale.run(); assert.equal(runs.length, 1)
  a.update('B'); assert.equal(disposals.length, 1)
  assert.equal(sessions[0].isCurrent(), false)
  sessions[0].notify('stale failure')
  assert.equal(a.statuses.length, 0)
  assert.notEqual(sessions[0].facades.request, sessions[1].facades.request)
})

 test('real effect adapter uses captured facades, accessible stop labels and shared Host actions', async t => {
  const copies=[], plays=[], stops=[]; let handler
  const effects={clipboard:{async writeText(input){copies.push(input)}},speech:{async voices(){return [{language:'en'},{language:'zh-CHS'}]},onState(fn){handler=fn;return()=>{}},async play(input){plays.push(input);return {playbackId:String(plays.length),status:'playing'}},async stop(input){stops.push(input);return {playbackId:input.playbackId,status:'stopped'}}}}
  const a=await setup(t,{effects});a.update();assert.equal(plays.length,0);a.ready();await flush()
  a.node('source-copy').focus();a.node('source-copy').click();await flush();assert.deepEqual(copies,[{text:'hello'}]);assert.equal(a.document.activeElement,a.node('source-copy'));assert.equal(a.statuses.at(-1).message,'已复制')
  a.node('source-speak').click();await flush();assert.equal(a.node('source-speak').getAttribute('aria-label'),'停止朗读原文');assert.equal(a.node('source-speak').getAttribute('aria-pressed'),'true')
  assert.equal(a.actions().context.find(x=>x.id==='source-speak').title,'停止朗读原文')
  a.actions().context.find(x=>x.id==='translation-speak').run();await flush();assert.equal(plays.length,2);assert.deepEqual(stops,[{playbackId:'1'}])
  handler({playbackId:'1',status:'completed'});assert.equal(a.node('translation-speak').getAttribute('aria-label'),'停止朗读译文')
  a.input('source-text','changed');await flush();assert.deepEqual(stops,[{playbackId:'1'},{playbackId:'2'}]);assert.equal(a.node('translation-copy').disabled,true)
})

test('real adapter editing preserves current value and selection, clears output and stops speech through IME', async t => {
  const stops = []
  const effects = {
    speech: {
      async voices() { return [{ language: 'en' }, { language: 'zh-CHS' }] },
      onState() { return () => {} },
      async play() { return { playbackId: 'editing', status: 'playing' } },
      async stop(input) { stops.push(input); return { playbackId: input.playbackId, status: 'stopped' } },
    },
  }
  const a = await setup(t, { effects })
  a.update(); a.ready(); await flush()
  a.node('source-speak').click(); await flush()
  const source = a.node('source-text')
  source.focus()
  source.dispatchEvent(new a.dom.window.CompositionEvent('compositionstart', { bubbles: true }))
  source.value = 'hello again after你好'
  source.setSelectionRange(6, 11, 'backward')
  source.dispatchEvent(new a.dom.window.InputEvent('input', { bubbles: true, isComposing: true }))
  assert.equal(source.value, 'hello again after你好')
  assert.equal(source.selectionStart, 6)
  assert.equal(source.selectionEnd, 11)
  assert.equal(source.selectionDirection, 'backward')
  assert.equal(a.node('translation').textContent, '')
  assert.equal(a.node('translation-speak').disabled, true)
  await flush()
  assert.deepEqual(stops, [{ playbackId: 'editing' }])
  a.key('source-text', 'Enter', { ctrlKey: true, isComposing: true })
  await flush()
  assert.equal(a.calls.length, 1, 'composition does not submit')
  source.dispatchEvent(new a.dom.window.CompositionEvent('compositionend', { bubbles: true }))
  a.key('source-text', 'Enter', { ctrlKey: true })
  await flush()
  assert.equal(a.calls.length, 2)
  assert.equal(a.calls[1].payload.text, 'hello again after你好')
  assert.equal(source.value, 'hello again after你好')
  assert.equal(source.selectionStart, 6)
  assert.equal(source.selectionEnd, 11)
  assert.equal(a.document.activeElement, source)
})


test('multilingual selector exposes manual-detection guidance and both text regions support RTL', async t => {
  const a = await setup(t); a.update(); a.ready(); await flush()
  assert.equal(a.node('source-text').getAttribute('dir'), 'auto')
  assert.equal(a.node('translation').getAttribute('dir'), 'auto')
  a.node('source-language').click()
  a.input('choice-search', 'Cantonese')
  assert.match(a.node('choice-options').textContent, /粤语.*手动/)
  a.key('choice-search', 'Enter')
  assert.match(a.node('source-language').textContent, /粤语/)
  a.node('target-language').click()
  a.input('choice-search', 'Japanese')
  a.key('choice-search', 'Enter')
  assert.match(a.node('target-language').textContent, /日语/)
})


test('keyboard navigation keeps the active language visible in a long list', async t => {
  const a=await setup(t); a.update(); a.ready(); await flush()
  const scrolled=[]
  a.dom.window.HTMLElement.prototype.scrollIntoView=function(options){scrolled.push({text:this.textContent,options})}
  a.node('source-language').click()
  a.key('choice-search','ArrowUp')
  assert.match(scrolled.at(-1)?.text ?? '', /祖鲁/)
  assert.equal(scrolled.at(-1).options.block,'nearest')
})

test('share accepts exact text once after preferences/Ready, translates once and exposes output source', async t => {
 const a = await setup(t); a.update('A', '', { sourceLanguage: 'auto', targetLanguage: 'en', providerId: 'youdao' }); a.ready(); await flush()
 const event = { version: 1, transferId: 's1', receiverId: 'translate', sessionEpoch: 'A', payload: { type: 'text/plain', text: ' edited\n原文  ' } }
 assert.deepEqual(a.receive(event), { accepted: true }); assert.deepEqual(a.receive(event), { accepted: false })
 await flush(); assert.equal(a.calls.length, 1); assert.equal(a.calls[0].payload.text, event.payload.text); assert.equal(a.calls[0].payload.targetLanguage, 'en')
 assert.equal(a.node('source-text').value, event.payload.text)
 assert.equal(a.shareSource().text, a.node('translation').textContent)
 a.update('A', ''); a.ready(); await flush(); assert.equal(a.calls.length, 1); assert.equal(a.node('source-text').value, event.payload.text)
})
test('shared translation business failure keeps received original and target UI without retry', async t => {
 const a = await setup(t, { request: async () => ({ ok: false, code: 'SERVICE_ERROR', message: '服务暂不可用' }) }); a.update('A', ''); a.ready()
 assert.deepEqual(a.receive({ version: 1, transferId: 's1', receiverId: 'translate', sessionEpoch: 'A', payload: { type: 'text/plain', text: '  keep\nthis ' } }), { accepted: true })
 await flush(); assert.equal(a.calls.length, 1); assert.equal(a.node('source-text').value, '  keep\nthis '); assert.equal(a.node('translation-status').textContent, '服务暂不可用'); assert.equal(a.shareSource(), null)
})

test('new empty submission after sharing clears source while same-request theme update preserves it', async t => {
 const a = await setup(t); a.update('A', ''); a.ready()
 a.receive({ version: 1, transferId: 's1', receiverId: 'translate', sessionEpoch: 'A', payload: { type: 'text/plain', text: 'shared' } }); await flush()
 a.update('A', ''); assert.equal(a.node('source-text').value, 'shared')
 a.update('A', '', undefined, 'new-empty-submit'); assert.equal(a.node('source-text').value, ''); assert.equal(a.node('translation').textContent, '')
})
