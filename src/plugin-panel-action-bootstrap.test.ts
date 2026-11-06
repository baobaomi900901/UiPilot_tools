import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'

const jsonSource = readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8')
const template = (path: string, name: string) => {
  const source = readFileSync(path, 'utf8')
  const match = source.match(new RegExp(`${name}: &str = r#"([\\s\\S]*?)"#;`, 'u'))
  if (!match) throw new Error('missing bootstrap')
  return match[1]!
}
const env = { TextEncoder, Object, Error, TypeError, Promise, Reflect, Set, WeakSet, setTimeout, clearInterval, setInterval, queueMicrotask }
const panel = (actions = true, result: () => Promise<unknown> = async () => ({ ok: true })) => {
  const invoke = vi.fn(async (command: string) => command === 'plugin_panel_request' ? result() : undefined)
  const window: any = { __TAURI_INTERNALS__: { invoke } }
  const source = template('src-tauri/src/plugin_panel.rs', 'PUBLIC_PANEL_BOOTSTRAP_TEMPLATE')
    .replace('__CONTEXT_ACTIONS_UI__', readFileSync('src/context-actions-ui.js', 'utf8').replace('export function', 'function') + '\n' + readFileSync('src/panel-context-actions.js', 'utf8').replace('export function', 'function').replace('export function', 'function'))
    .replace('__PANEL_EFFECTS__', readFileSync('src-tauri/src/panel_effects.js', 'utf8'))
    .replace('__PANEL_IMAGE_BRIDGE__', readFileSync('src-tauri/src/panel_image_bridge.js', 'utf8')).replace('__IMAGES_ENABLED__', 'false')
    .replace('__CLIPBOARD_ENABLED__', 'false').replace('__SPEECH_ENABLED__', 'false')
    .replace('__PANEL_JSON__', jsonSource).replace('__RUNTIME_ACTIONS__', String(actions))
    .replaceAll('__SESSION_EPOCH__', '7').replace('__HOST_KEYS__', '[]')
  vm.runInNewContext(source, { ...env, window, document: { documentElement: { style: { setProperty() {} } }, addEventListener() {}, querySelector() { return null } } })
  return { window, invoke, api: window.uipilotPluginPanel }
}
async function runtime(module: any, optedIn = true, windowActions = false) {
  let callback: any
  const invoke = vi.fn(async () => undefined)
  const window: any = { testModule: module, __TAURI_INTERNALS__: { invoke, transformCallback(next: any) { callback = next; return 1 } } }
  const document = { title: '', documentElement: { dataset: { runtimeEntry: '/runtime.js' } } }
  const source = template('src-tauri/src/public_plugins/runtime.rs', 'PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE')
    .replace('__RESULT_ACTIONS__', 'false').replace('__WINDOW_ACTIONS__', String(windowActions)).replace('__CONTEXT_ACTIONS_UI__', readFileSync('src/context-actions-ui.js', 'utf8').replace('export function', 'function') + '\n' + readFileSync('src/panel-context-actions.js', 'utf8').replace('export function', 'function').replace('export function', 'function'))
    .replace('__PANEL_JSON__', jsonSource).replace('__PANEL_ACTIONS__', String(optedIn)).replace('__NETWORK_HTTPS__', 'false')
    .replace('const module = entry ? await import(entry) : null;', 'const module = window.testModule;')
  vm.runInNewContext(source, { ...env, window, document })
  await vi.waitFor(() => expect(document.title).not.toBe(''))
  return { callback, document, invoke }
}
const context = { pluginId: 'com.example.panel', pluginGeneration: 1, requestId: 'public-request-0000000000000001' }
const invocation = { apiVersion: 1, requestId: context.requestId, action: 'preview', payload: { body: 'Hi', values: [1, 2] }, context: { platform: 'windows', theme: 'dark', invokedAt: '2026-10-24T00:00:00Z' } }

describe('panel action executable bridges', () => {
  it('delivers the host request-scoped image as a frozen Panel action descriptor', async () => {
    const descriptor = { id: 'request-image', mimeType: 'image/png', width: 10, height: 20, byteLength: 80 }
    const handler = vi.fn(async (next: any) => {
      expect(next.image).toEqual(descriptor)
      expect(Object.isFrozen(next.image)).toBe(true)
      expect(() => { next.image.id = 'other' }).toThrow()
      return { requestId: next.requestId, data: { ok: true } }
    })
    const r = await runtime({ onCommand: async () => ({}), onPanelAction: handler })
    await r.callback({ payload: { context, kind: 'panelAction', invocation: { ...structuredClone(invocation), image: { ...descriptor } } } })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(r.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, response: { requestId: context.requestId, data: { ok: true } } } })
  })

  it('can start one Runtime request from a settled-ready receiver without another ACK', async () => {
    const p = panel()
    let response: Promise<unknown> | undefined
    p.api.onReady(() => { response = p.api.request({ action: 'translate', payload: { text: 'hello' } }) })
    p.api.onUpdate(() => undefined)
    await p.window.__UIPILOT_PLUGIN_PANEL_UPDATE__({ sessionEpoch: '7', requestId: 'a', theme: 'light' })
    expect(p.invoke.mock.calls.filter(([command]) => command === 'plugin_panel_request')).toHaveLength(0)
    p.window.__UIPILOT_PLUGIN_PANEL_READY__({ sessionEpoch: '7', requestId: 'a' })
    p.window.__UIPILOT_PLUGIN_PANEL_READY__({ sessionEpoch: '7', requestId: 'a' })
    await Promise.resolve()
    await expect(response).resolves.toEqual({ ok: true })
    expect(p.invoke.mock.calls.filter(([command]) => command === 'plugin_panel_request')).toHaveLength(1)
    expect(p.invoke.mock.calls.filter(([command]) => command === 'plugin_panel_content_ack')).toHaveLength(1)
  })

  it('exposes only opted-in request, snapshots before IPC, freezes response and expires retained references', async () => {
    expect(panel(false).api.request).toBeUndefined()
    let release!: (value: unknown) => void
    const p = panel(true, () => new Promise(resolve => { release = resolve }))
    const input = { action: 'preview', payload: { value: 1 } }
    const retained = p.api.request
    const pending = retained(input)
    input.payload.value = 2
    expect(p.invoke.mock.calls.find(([command]) => command === 'plugin_panel_request')).toEqual(['plugin_panel_request', { sessionEpoch: '7', request: { action: 'preview', payload: { value: 1 } } }])
    await expect(p.api.request(input)).rejects.toHaveProperty('name', 'PanelRequestBusyError')
    release({ ok: true, values: [1, 2] })
    const resolved = await pending
    expect(Object.isFrozen(resolved)).toBe(true)
    expect(resolved.values.map((v: number) => v * 2)).toEqual([2, 4])
    p.window.__UIPILOT_PLUGIN_PANEL_PREPARE__({ sessionEpoch: '8' })
    await expect(retained(input)).rejects.toHaveProperty('name', 'ExpiredPanelSession')
  })

  it('rejects non-JSON/accessor/shape/size/depth/count input without executing getters or IPC', async () => {
    const p = panel()
    let getters = 0
    const accessor = Object.defineProperty({}, 'x', { enumerable: true, get() { getters++; return 1 } })
    const cycle: any = {}; cycle.self = cycle
    let nested: any = null; for (let i = 0; i < 33; i++) nested = [nested]
    for (const payload of [accessor, cycle, undefined, NaN, Infinity, BigInt(1), () => 1, Symbol('x'), [, 1], { constructor: 1 }, { x: undefined }, { toJSON() { getters++; return 1 } }, 'x'.repeat(65536), Array(4096).fill(0), nested]) {
      await expect(p.api.request({ action: 'save', payload })).rejects.toHaveProperty('name', 'InvalidPanelRequestError')
    }
    for (const input of [{ action: 'save' }, { action: 'constructor', payload: null }, { action: 'save', payload: null, extra: 1 }]) {
      await expect(p.api.request(input)).rejects.toHaveProperty('name', 'InvalidPanelRequestError')
    }
    expect(getters).toBe(0)
    expect(p.invoke).not.toHaveBeenCalled()
  })

  it('rejects missing handler at readiness only for opt-in and sanitizes handler exceptions', async () => {
    const missing = await runtime({ onCommand: async () => ({}) })
    expect(missing.document.title).toBe('uipilot-public-plugin-failed')
    const legacy = await runtime({ onCommand: async () => ({}) }, false)
    expect(legacy.document.title).toBe('uipilot-public-plugin-ready')
    const failed = await runtime({ onCommand: async () => ({}), onPanelAction: async () => { throw new Error('sensitive details') } })
    await failed.callback({ payload: { context, kind: 'panelAction', invocation } })
    expect(failed.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, failed: true } })
  })

  it('uses fresh frozen invocation/API, bounded reply snapshot and shared physical busy slot', async () => {
    let release!: () => void
    const calls: any[] = []
    const handler = vi.fn(async (next: any, api: any) => {
      expect(next.payload.values.map((v: number) => v * 2)).toEqual([2, 4])
      calls.push([next, api])
      await new Promise<void>(resolve => { release = resolve })
      return { requestId: next.requestId, data: { ok: true } }
    })
    const r = await runtime({ onCommand: vi.fn(async () => ({})), onPanelAction: handler })
    const first = r.callback({ payload: { context, kind: 'panelAction', invocation: structuredClone(invocation) } })
    await r.callback({ payload: { context, kind: 'panelAction', invocation: structuredClone(invocation) } })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(Object.isFrozen(calls[0][0].payload)).toBe(true)
    expect(Object.isFrozen(calls[0][1].storage)).toBe(true)
    release(); await first
    expect(() => calls[0][1].storage.get('late')).toThrow('ExpiredRequestError')
    expect(r.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, response: { requestId: context.requestId, data: { ok: true } } } })
    const invalid = await runtime({ onCommand: async () => ({}), onPanelAction: async () => ({ requestId: context.requestId, data: { constructor: 1 } }) })
    await invalid.callback({ payload: { context, kind: 'panelAction', invocation: structuredClone(invocation) } })
    expect(invalid.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, response: null } })
  })
})


it('never executes inherited array toJSON/getters while snapshotting and serializing', () => {
  const result = vm.runInNewContext(`${jsonSource}
    let calls = 0;
    Object.defineProperty(Array.prototype, 'toJSON', { configurable: true, get() { calls++; return () => { calls++; return 'hijacked'; }; } });
    const request = panelJson.snapshot({ action: 'save', payload: { values: [1, 2] } }, ['action', 'payload']);
    const serialized = JSON.stringify(request);
    ({ calls, serialized });
  `, { TextEncoder })
  expect(result.calls).toBe(0)
  expect(result.serialized).toBe('{"action":"save","payload":{"values":[1,2]}}')
})


it('requires onWindowAction at opt-in readiness and dispatches fresh frozen Window invocations on the shared slot', async () => {
  const missing = await runtime({ onCommand: async () => ({}) }, false, true)
  expect(missing.document.title).toBe('uipilot-public-plugin-failed')
  const commands = vi.fn(async () => ({}))
  let retainedApi: any
  const windowHandler = vi.fn(async (next: any, api: any) => {
    expect(Object.isFrozen(next.payload)).toBe(true)
    retainedApi = api
    return { requestId: next.requestId, data: { values: [1, 2] } }
  })
  const r = await runtime({ onCommand: commands, onWindowAction: windowHandler }, false, true)
  expect(r.document.title).toBe('uipilot-public-plugin-ready')
  await r.callback({ payload: { context, kind: 'windowAction', invocation: structuredClone(invocation) } })
  expect(commands).not.toHaveBeenCalled()
  expect(windowHandler).toHaveBeenCalledTimes(1)
  expect(() => retainedApi.storage.get('late')).toThrow('ExpiredRequestError')
  expect(r.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, response: { requestId: context.requestId, data: { values: [1, 2] } } } })
  const failed = await runtime({ onCommand: commands, onWindowAction: async () => { throw new Error('secret stack') } }, false, true)
  await failed.callback({ payload: { context, kind: 'windowAction', invocation: structuredClone(invocation) } })
  expect(failed.invoke).toHaveBeenCalledWith('complete_plugin_command', { completion: { context, failed: true } })
})
