import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { JSDOM } from 'jsdom'
import { onCommand, onPanelAction } from '../package/dist/runtime.js'
import { mountPanel } from '../package/dist/panel.js'
const root = new URL('../package/', import.meta.url)
const manifest = JSON.parse(await readFile(new URL('plugin.json', root), 'utf8'))
const flush = () => new Promise(resolve => setTimeout(resolve, 20))
function deferred() { let resolve; const promise = new Promise(done => { resolve = done }); return { promise, resolve } }

// This adapter implements only the Host boundary: all business/model/effect logic is real plugin code.
async function setup(t, translate) {
  assert.equal(manifest.command.outputMode, 'panel', 'Host must route the command into the Panel')
  const dom = new JSDOM(await readFile(new URL(manifest.panel.entry, root), 'utf8'))
  let update, ready, owner, inCallback = false, serial = 0
  const requests = [], services = [], copies = [], plays = [], stops = [], statuses = [], handlers = []
  const runtimeApi = { storage: { async get() { return null } }, services: { youdao: { async translate(input) {
    services.push(input)
    return translate?.(input) ?? { translations: [`译文:${input.text}`], sourceLanguage: 'en', targetLanguage: input.to, unchanged: false }
  } } } }
  const host = {
    onUpdate(fn) { update = fn; return () => { update = null } },
    onReady(fn) { ready = fn; return () => { ready = null } },
    onActions() { return () => {} },
    get request() { const session = owner; return async request => {
      assert.equal(inCallback, false, 'requests must follow callback return and readiness')
      const requestId = `action-${++serial}`
      requests.push({ session, ...request })
      const result = await onPanelAction({ requestId, ...request }, runtimeApi)
      assert.equal(result.requestId, requestId)
      return result.data
    } },
    storage: { async set() {} },
    get setStatus() { const session = owner; return async value => { statuses.push({ session, ...value }) } },
    get clipboard() { const session = owner; return { async writeText(input) { copies.push({ session, ...input }) } } },
    get speech() { const session = owner; return {
      async voices() { return [{ language: 'en' }, { language: 'zh-CHS' }] },
      onState(fn) { handlers.push(fn); return () => {} },
      async play(input) { const playbackId = `speech-${plays.length}`; plays.push({ session, ...input, playbackId }); return { playbackId, status: 'playing' } },
      async stop(input) { stops.push({ session, ...input }); return { ...input, status: 'stopped' } },
    } },
  }
  const app = mountPanel(dom.window.document, host)
  t.after(() => { app.dispose(); dom.window.close() })
  const node = id => dom.window.document.getElementById(id)
  return { dom, node, requests, services, copies, plays, stops, statuses, handlers, app,
    async command(text, session = 'A', signalReady = true) {
      owner = session
      const requestId = `command-${++serial}`
      const result = await onCommand({ requestId, input: text }, runtimeApi)
      assert.equal(result.requestId, requestId)
      inCallback = true
      try { assert.equal(update({ sessionEpoch: session, requestId, input: text, theme: 'light', data: result.data }), undefined) }
      finally { inCallback = false }
      if (signalReady) { inCallback = true; try { ready({ sessionEpoch: session, requestId }) } finally { inCallback = false } }
    },
  }
}

test('actual command, readiness, Runtime provider and DOM complete translation with real effect adapter', async t => {
  const a = await setup(t)
  await a.command('hello\nworld', 'A', false)
  await flush(); assert.equal(a.requests.length, 0)
  await a.command('hello\nworld'); await flush()
  assert.deepEqual(a.services, [{ text: 'hello\nworld', from: 'auto', to: 'zh-CHS' }])
  assert.equal(a.node('translation').textContent, '译文:hello\nworld')
  assert.match(a.node('detected-language').textContent, /英语/)
  a.node('translation-copy').focus(); a.node('translation-copy').click(); await flush()
  assert.deepEqual(a.copies, [{ session: 'A', text: '译文:hello\nworld' }])
  assert.equal(a.dom.window.document.activeElement, a.node('translation-copy'))
  assert.equal(a.statuses.at(-1).message, '已复制')
  a.node('source-speak').click(); await flush()
  a.node('translation-speak').click(); await flush()
  assert.deepEqual(a.plays.map(({ text, language }) => ({ text, language })), [{ text: 'hello\nworld', language: 'en' }, { text: '译文:hello\nworld', language: 'zh-CHS' }])
  assert.equal(a.stops.length, 1)
})

for (const name of ['ServicePermissionDeniedError', 'PermissionDeniedError']) test(`${name} traverses Runtime and renders safe failure without success`, async t => {
  const a = await setup(t, () => { throw { name, message: 'private provider diagnostic' } })
  await a.command('hello'); await flush()
  assert.equal(a.node('translation').textContent, '')
  assert.equal(a.node('translation-copy').disabled, true)
  assert.equal(a.node('translation-status').dataset.error, 'true')
  assert.match(a.node('translation-status').textContent, /权限|授权/)
  assert.equal(a.node('translation-status').textContent.includes('private'), false)
  await flush(); assert.equal(a.requests.length, 1, 'no automatic permission retry')
})

test('replacement selection invalidates old response/audio and empty failure cannot reuse old text', async t => {
  const pending = deferred()
  const a = await setup(t, input => input.text === 'old' ? pending.promise : undefined)
  await a.command('first'); await flush()
  a.node('translation-speak').click(); await flush()
  const oldSpeech = a.plays[0].playbackId
  await a.command('old'); await flush()
  await a.command('new selection', 'B'); await flush()
  assert.equal(a.requests.length, 2, 'single flight waits for old request')
  assert.equal(a.node('translation').textContent, '')
  assert.ok(a.stops.some(item => item.playbackId === oldSpeech))
  pending.resolve({ translations: ['STALE'], sourceLanguage: 'en', targetLanguage: 'zh-CHS', unchanged: false })
  await flush(); await flush()
  assert.equal(a.node('translation').textContent, '译文:new selection')
  a.handlers[0]({ playbackId: oldSpeech, status: 'error', errorCode: 'SpeechVoiceUnavailable' })
  assert.equal(a.statuses.length, 0)
  await a.command('', 'C'); await flush()
  assert.equal(a.node('source-text').value, '')
  assert.equal(a.node('translation').textContent, '')
  assert.equal(a.requests.length, 3, 'failed selection represented by empty input does not request stale text')
  assert.equal(a.node('translation-copy').disabled, true)
  a.app.dispose(); await flush()
  assert.equal(a.node('translation').textContent, '')
})
