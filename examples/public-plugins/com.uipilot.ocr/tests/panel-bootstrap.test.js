import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import { JSDOM } from 'jsdom'
import * as plugin from '../package/dist/runtime.js'
import { mountPanel } from '../package/dist/panel.js'
const repo = new URL('../../../../', import.meta.url), root = new URL('../package/', import.meta.url)
const read = path => readFile(new URL(path, repo), 'utf8')
const flush = async () => { for (let i = 0; i < 5; i++) await new Promise(resolve => setImmediate(resolve)) }
const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail }); return { promise, resolve, reject } }
async function setup(t) {
 const manifest = JSON.parse(await readFile(new URL('plugin.json', root), 'utf8'))
 const dom = new JSDOM(await readFile(new URL(manifest.panel.entry, root), 'utf8'))
 const extract = (source, name) => { const match = source.match(new RegExp(`${name}: &str = r#"([\\s\\S]*?)"#;`, 'u')); assert.ok(match); return match[1] }
 const json = await read('src-tauri/src/public_plugins/panel_action_json.js')
 const contextActions = (await read('src/context-actions-ui.js') + '\n' + await read('src/panel-context-actions.js')).replaceAll('export function', 'function')
 const env = { Object, TextEncoder, Error, TypeError, Promise, Reflect, Set, WeakSet, queueMicrotask, setTimeout, setInterval, clearInterval }
 const ipc = [], services = [], copies = [], completions = new Map(); let serial = 0, imageRevision = 0
 const runtimeDocument = { title: '', documentElement: { dataset: { runtimeEntry: '/runtime.js' } } }
 const runtimeWindow = { testModule: plugin, __TAURI_INTERNALS__: {
  async invoke(command, args) {
   ipc.push({ side: 'runtime', command, args })
   if (command === 'plugin_baidu_ocr') { const job = deferred(); services.push({ ...job, args }); return job.promise }
   if (command === 'complete_plugin_command') { const completion = args.completion; const job = completions.get(completion.context.requestId); assert.ok(job); completion.failed ? job.reject(Error('Runtime failed')) : job.resolve(completion.response); completions.delete(completion.context.requestId) }
  },
 } }
 const runtimeSource = extract(await read('src-tauri/src/public_plugins/runtime.rs'), 'PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE')
  .replace('__PANEL_JSON__', json).replace('__RESULT_ACTIONS__', 'false').replace('__WINDOW_ACTIONS__', 'false').replace('__PANEL_ACTIONS__', 'true').replace('__NETWORK_HTTPS__', 'true')
  .replace('const module = entry ? await import(entry) : null;', 'const module = window.testModule;')
 vm.runInNewContext(runtimeSource, { ...env, window: runtimeWindow, document: runtimeDocument })
 await flush(); assert.equal(runtimeDocument.title, 'uipilot-public-plugin-ready')
 function dispatch(kind, data) {
  const requestId = `public-request-${String(++serial).padStart(16, '0')}`, job = deferred()
  completions.set(requestId, job)
  const context = { pluginId: manifest.pluginId, pluginGeneration: 1, requestId }
  const invocation = { apiVersion: 1, requestId, context: { platform: 'windows', theme: 'light', invokedAt: '2026-11-06T00:00:00Z' }, ...data }
  void runtimeWindow.__UIPILOT_PUBLIC_PLUGIN_DISPATCH__({ context, kind, invocation })
  return job.promise
 }
 const panelSource = extract(await read('src-tauri/src/plugin_panel.rs'), 'PUBLIC_PANEL_BOOTSTRAP_TEMPLATE')
  .replace('__CONTEXT_ACTIONS_UI__', contextActions).replace('__PANEL_JSON__', json)
  .replace('__PANEL_EFFECTS__', await read('src-tauri/src/panel_effects.js')).replace('__PANEL_IMAGE_BRIDGE__', await read('src-tauri/src/panel_image_bridge.js'))
  .replace('__IMAGES_ENABLED__', 'true').replace('__CLIPBOARD_ENABLED__', 'true').replace('__SPEECH_ENABLED__', 'false')
  .replace('__RUNTIME_ACTIONS__', 'true').replaceAll('__SESSION_EPOCH__', '7').replace('__HOST_KEYS__', JSON.stringify(manifest.panel.hostKeys))
 const image = (id, revision = ++imageRevision) => dom.window.dispatchEvent(new dom.window.CustomEvent('uipilot-plugin-panel-image-changed', { detail: { sessionEpoch: '7', revision: String(revision), image: id ? { id, mimeType: 'image/png', width: 100, height: 80, byteLength: 120, previewDataUrl: 'data:image/png;base64,AAAA' } : null } }))
 dom.window.__TAURI_INTERNALS__ = { async invoke(command, args) {
  ipc.push({ side: 'panel', command, args })
  if (command === 'plugin_panel_request') {
   assert.equal(args.sessionEpoch, '7'); assert.ok(args.request.imageId)
   const runtimeImage = { id: `runtime:${args.request.imageId}`, mimeType: 'image/png', width: 100, height: 80, byteLength: 120 }
   return (await dispatch('panelAction', { ...args.request, image: runtimeImage })).data
  }
  if (command === 'plugin_panel_clipboard_write_text') copies.push(args.input.text)
  if (command === 'plugin_panel_clear_image') image(null)
  if (command === 'plugin_panel_request_hide_admit') return { outcome: 'noop' }
 } }
 vm.runInNewContext(panelSource, { ...env, window: dom.window, document: dom.window.document })
 const app = mountPanel(dom.window.document, dom.window.uipilotPluginPanel); await flush()
 t.after(() => { app.dispose(); dom.window.close() })
 const install = async (theme = 'light') => {
  const result = await dispatch('command', { commandId: 'default', input: '' })
  const update = { sessionEpoch: '7', requestId: result.requestId, input: '', theme, data: result.data }
  dom.window.__UIPILOT_PLUGIN_PANEL_PREPARE__({ sessionEpoch: '7' })
  await dom.window.__UIPILOT_PLUGIN_PANEL_UPDATE__(update)
  return update
 }
 const ready = update => dom.window.__UIPILOT_PLUGIN_PANEL_READY__(update)
 const node = id => dom.window.document.getElementById(id)
 return { dom, app, node, install, ready, image, services, ipc, copies }
}
test('actual Panel and Runtime bootstraps: ACK/Ready, request-bound image, editable copy, latest pending and clear', async t => {
 const a = await setup(t)
 const initial = await a.install(); await flush(); assert.equal(a.services.length, 0)
 a.image('A'); await flush(); assert.equal(a.services.length, 0, 'initial image queues until Ready')
 a.ready({ ...initial, requestId: 'stale' }); await flush(); assert.equal(a.services.length, 0)
 a.ready(initial); a.ready(initial); await flush(); assert.equal(a.services.length, 1)
 assert.equal(a.services[0].args.input.input.imageId, 'runtime:A', 'Baidu receives Runtime authority, not Panel image ID')
 assert.ok(Object.isFrozen(a.services[0].args.input.context))
 a.image('B'); a.image('C'); await flush(); assert.equal(a.services.length, 1, 'physical Runtime slot remains occupied')
 a.services[0].resolve({ text: 'STALE', lines: ['STALE'] }); await flush(); assert.equal(a.node('ocr-text').value, '')
 assert.equal(a.services.length, 2); assert.equal(a.services[1].args.input.input.imageId, 'runtime:C')
 a.services[1].resolve({ text: 'CURRENT\n全文', lines: ['CURRENT', '全文'] }); await flush(); assert.equal(a.node('ocr-text').value, 'CURRENT\n全文')
 a.node('ocr-text').value = 'Edited\n全文😀'; a.node('ocr-text').dispatchEvent(new a.dom.window.Event('input'))
 const themed = await a.install('dark'); a.ready(themed); await flush(); assert.equal(a.services.length, 2); assert.equal(a.node('ocr-text').value, 'Edited\n全文😀')
 await a.dom.window.__UIPILOT_PLUGIN_PANEL_HOST_KEY__({ key: 'Enter', sessionEpoch: '7', routeSequence: '1', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false })
 await flush(); assert.deepEqual(a.copies, ['Edited\n全文😀'])
 assert.ok(a.ipc.some(call => call.command === 'plugin_panel_host_key_ack'))
 assert.ok(a.ipc.some(call => call.command === 'plugin_panel_focus_host_input'))
 assert.ok(a.ipc.some(call => call.command === 'plugin_panel_set_status' && call.args.status.message === '已复制'))
 a.node('clear-image').click(); assert.equal(a.node('copy').disabled, true); await flush(); assert.equal(a.node('ocr-text').value, ''); assert.equal(a.services.length, 2)
 assert.equal(a.node('image').hasAttribute('src'), false)
})
test('actual bootstrap empty entry zero calls, safe provider failure, explicit retry, pagehide ignores late completion', async t => {
 const a = await setup(t); const update = await a.install(); a.ready(update); await flush(); assert.equal(a.services.length, 0)
 a.image('A'); await flush(); a.services[0].reject({ code: 'providerQuota' }); await flush()
 assert.equal(a.node('copy').disabled, true); assert.equal(a.node('ocr-status').textContent, '百度 OCR 额度不足或请求受限'); assert.equal(a.services.length, 1)
 a.node('retry').click(); await flush(); assert.equal(a.services.length, 2)
 a.dom.window.dispatchEvent(new a.dom.window.Event('pagehide')); a.services[1].resolve({ text: 'LATE', lines: ['LATE'] }); await flush()
 assert.equal(a.node('ocr-text').value, ''); assert.equal(a.copies.length, 0)
 await assert.rejects(a.dom.window.__UIPILOT_PLUGIN_PANEL_UPDATE__(update), /content not ready/)
})
