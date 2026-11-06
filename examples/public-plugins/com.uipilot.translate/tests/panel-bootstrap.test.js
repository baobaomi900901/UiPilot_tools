import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import { JSDOM } from 'jsdom'
import { mountPanel } from '../package/dist/panel.js'
import { onCommand, onPanelAction } from '../package/dist/runtime.js'

const root = new URL('../package/', import.meta.url)
const repo = new URL('../../../../', import.meta.url)
const read = path => readFile(new URL(path, repo), 'utf8')
const flush = () => new Promise(resolve => setTimeout(resolve, 20))

// Execute the shipped Rust bootstrap, not a reduced fake Panel API. Only IPC is fake.
test('shipped manifest and actual bootstrap mount, await settled readiness, and dispose on pagehide', async t => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json', root), 'utf8'))
  assert.deepEqual(manifest.panel.hostKeys ?? [], [], 'translation uses document keys and normal Host submit')
  const dom = new JSDOM(await readFile(new URL(manifest.panel.entry, root), 'utf8'))
  t.after(() => dom.window.close())
  const rust = await read('src-tauri/src/plugin_panel.rs')
  const template = rust.match(/PUBLIC_PANEL_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)?.[1]
  assert.ok(template)
  const source = template
    .replace('__CONTEXT_ACTIONS_UI__', (await read('src/context-actions-ui.js') + '\n' + await read('src/panel-context-actions.js')).replaceAll('export function', 'function'))
    .replace('__PANEL_EFFECTS__', await read('src-tauri/src/panel_effects.js'))
    .replace('__PANEL_IMAGE_BRIDGE__', await read('src-tauri/src/panel_image_bridge.js')).replace('__IMAGES_ENABLED__', 'false')
    .replace('__PANEL_JSON__', await read('src-tauri/src/public_plugins/panel_action_json.js'))
    .replace('__CLIPBOARD_ENABLED__', 'true').replace('__SPEECH_ENABLED__', 'true')
    .replace('__PANEL_ACTIONS__', 'false').replace('__RUNTIME_ACTIONS__', String(manifest.panel.runtimeActions))
    .replaceAll('__SESSION_EPOCH__', '7').replace('__HOST_KEYS__', JSON.stringify(manifest.panel.hostKeys ?? []))
  const calls = []
  let finishTranslation
  const runtime = { storage: { async get() { return null } }, services: { youdao: { translate: () => new Promise(resolve => { finishTranslation = resolve }) } } }
  dom.window.__TAURI_INTERNALS__ = { async invoke(command, args) {
    calls.push({ command, args: JSON.parse(JSON.stringify(args)) })
    if (command === 'plugin_panel_request') return (await onPanelAction({ requestId: 'action-1', ...args.request }, runtime)).data
    if (command === 'plugin_panel_speech_voices') return []
  } }
  // Imported UI modules and bridge share browser Object intrinsics in production. Preserve that here.
  vm.runInNewContext(source, { window: dom.window, document: dom.window.document, Object, TextEncoder, queueMicrotask, setTimeout, setInterval, clearInterval })
  assert.equal(typeof dom.window.uipilotPluginPanel.onHostKey, 'function', 'real public API is present even without declared keys')
  const app = mountPanel(dom.window.document, dom.window.uipilotPluginPanel)
  t.after(() => app.dispose())
  await flush()
  assert.deepEqual(calls.filter(call => call.command === 'plugin_panel_content_ready').map(call => call.args), [{ sessionEpoch: '7', hostKeyReceiverRegistered: false, hostKeyRegistrationViolation: false }])
  const result = await onCommand({ requestId: 'command-1', input: 'hello' }, runtime)
  const update = { sessionEpoch: '7', requestId: result.requestId, input: 'hello', theme: 'light', data: result.data }
  dom.window.__UIPILOT_PLUGIN_PANEL_PREPARE__({ sessionEpoch: '7' })
  await dom.window.__UIPILOT_PLUGIN_PANEL_UPDATE__(update)
  await flush()
  assert.equal(calls.filter(call => call.command === 'plugin_panel_request').length, 0, 'ACK alone never starts Runtime work')
  assert.ok(calls.some(call => call.command === 'plugin_panel_content_ack'))
  dom.window.__UIPILOT_PLUGIN_PANEL_READY__({ sessionEpoch: '7', requestId: 'stale' })
  await flush()
  assert.equal(finishTranslation, undefined)
  dom.window.__UIPILOT_PLUGIN_PANEL_READY__(update)
  await flush()
  assert.equal(typeof finishTranslation, 'function', 'settled readiness starts actual Runtime action')
  assert.equal(calls.filter(call => call.command === 'plugin_panel_request').length, 1)
  dom.window.dispatchEvent(new dom.window.Event('pagehide'))
  finishTranslation({ translations: ['late translation'], sourceLanguage: 'en', targetLanguage: 'zh-CHS', unchanged: false })
  await flush()
  assert.equal(dom.window.document.getElementById('translation').textContent, '', 'disposed UI ignores late Runtime completion')
  await assert.rejects(dom.window.__UIPILOT_PLUGIN_PANEL_UPDATE__(update), /content not ready/, 'pagehide removes the plugin update receiver')
  dom.window.document.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }))
  await flush()
  assert.equal(calls.filter(call => call.command === 'plugin_panel_request').length, 1, 'disposed document handlers cannot submit')
})
