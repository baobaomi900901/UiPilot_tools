import { readFileSync, readdirSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'
const json = readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8')
const template = (file: string, name: string) => readFileSync(file, 'utf8').match(new RegExp(`${name}: &str = r#"([\\s\\S]*?)"#;`, 'u'))![1]!
const windowBridge = (enabled = true, result: () => Promise<any> = async () => ({ ok: true })) => {
  const invoke = vi.fn(async (command: string) => command === 'plugin_window_request' ? result() : undefined)
  const window: any = { __TAURI_INTERNALS__: { invoke } }
  const document = { addEventListener: vi.fn(), documentElement: { style: { setProperty() {} }, dataset: {} } }
  const source = template('src-tauri/src/plugin_window.rs', 'PUBLIC_CONTENT_BOOTSTRAP').replace('__PANEL_JSON__', json).replace('__WINDOW_ACTIONS__', String(enabled))
  vm.runInNewContext(source, { window, document, TextEncoder, Object, Reflect, Error, TypeError, Promise, WeakSet, setTimeout })
  const prepare = (session = '7', document = '1') => window.__UIPILOT_PLUGIN_TIMER_PREPARE__({ sessionGeneration: session, documentGeneration: document })
  return { window, invoke, api: window.uipilotPluginWindow, prepare, document }
}
describe('window action executable bootstrap', () => {
  it('routes Ctrl+T from plugin content through its current session and consumes held-key repeats', async () => {
    const b = windowBridge()
    const keydown = b.document.addEventListener.mock.calls.find(([type]) => type === 'keydown')![1]
    const press = (extra = {}) => {
      const event = { key: 't', ctrlKey: true, preventDefault: vi.fn(), stopImmediatePropagation: vi.fn(), ...extra }
      keydown(event)
      return event
    }
    press()
    expect(b.invoke).not.toHaveBeenCalled()
    b.prepare('7')
    for (const extra of [{ repeat: true }, { shiftKey: true }, { altKey: true }, { metaKey: true }, { isComposing: true }, { ctrlKey: false }]) press(extra)
    expect(b.invoke).not.toHaveBeenCalled()
    const event = press()
    press()
    expect(event.preventDefault).toHaveBeenCalledOnce()
    expect(event.stopImmediatePropagation).toHaveBeenCalledOnce()
    expect(b.invoke).toHaveBeenCalledExactlyOnceWith('plugin_window_content_toggle_pin', { sessionGeneration: '7' })
    await new Promise(resolve => setTimeout(resolve, 0))
    b.prepare('8')
    press({ key: 'T' })
    expect(b.invoke).toHaveBeenLastCalledWith('plugin_window_content_toggle_pin', { sessionGeneration: '8' })
    expect(b.api.togglePinned).toBeUndefined()
  })
  it('omits legacy property, installs a new bound capability before update and expires old timer/storage/request', async () => {
    expect(Object.prototype.hasOwnProperty.call(windowBridge(false).api, 'request')).toBe(false)
    const b = windowBridge()
    expect(b.api.request).toBeUndefined()
    b.prepare()
    const old = b.api.request, timer = b.api.timer, storage = b.api.storage
    expect(typeof old).toBe('function')
    let captured: any
    b.api.onUpdate(async () => { captured = b.api.request; await expect(captured({ action: 'load', payload: null })).rejects.toHaveProperty('name', 'WindowRequestBusyError') })
    await b.window.__UIPILOT_PLUGIN_WINDOW_UPDATE__({ requestId: 'r', theme: 'dark' })
    expect(captured).toBe(old)
    b.prepare('8')
    expect(b.api.request).not.toBe(old)
    await expect(old({ action: 'load', payload: null })).rejects.toHaveProperty('name', 'ExpiredWindowSessionError')
    await expect(timer.getState()).rejects.toThrow('ExpiredWindowSessionError')
    await expect(storage.get('draft')).rejects.toThrow('ExpiredWindowSessionError')
    await b.api.storage.get('draft')
    expect(b.invoke).toHaveBeenCalledWith('plugin_window_storage_get', { sessionGeneration: '8', key: 'draft' })
    await b.api.request({ action: 'load', payload: null })
    expect(b.invoke).toHaveBeenCalledWith('plugin_window_request', { sessionGeneration: '8', documentGeneration: '1', request: { action: 'load', payload: null } })
  })
  it('snapshots without getters, enforces bounds, one pending call, frozen data and fixed error mapping', async () => {
    let release!: (v: any) => void
    const b = windowBridge(true, () => new Promise(resolve => { release = resolve }))
    b.prepare()
    let getters = 0
    const access = Object.defineProperty({}, 'x', { enumerable: true, get() { getters++; return 1 } })
    const cycle: any = {}; cycle.self = cycle
    let nested: any = null; for (let i = 0; i < 33; i++) nested = [nested]
    for (const payload of [access, cycle, undefined, NaN, Infinity, BigInt(1), Symbol('x'), () => 1, [, 1], { constructor: 1 }, { toJSON() { getters++; return 1 } }, 'x'.repeat(65536), Array(4096).fill(0), nested]) {
      await expect(b.api.request({ action: 'save', payload })).rejects.toHaveProperty('name', 'InvalidWindowRequestError')
    }
    for (const input of [{ action: 'save' }, { action: 'constructor', payload: null }, { action: 'save', payload: null, extra: 1 }]) await expect(b.api.request(input)).rejects.toHaveProperty('name', 'InvalidWindowRequestError')
    expect(getters).toBe(0)
    const input = { action: 'save', payload: { value: 1 } }
    const pending = b.api.request(input); input.payload.value = 2
    await expect(b.api.request(input)).rejects.toHaveProperty('name', 'WindowRequestBusyError')
    release({ ok: true, values: [1, 2] }); const data = await pending
    expect(Object.isFrozen(data.values)).toBe(true)
    expect(data.values.map((n: number) => n * 2)).toEqual([2, 4])
    expect(b.invoke).toHaveBeenCalledWith('plugin_window_request', { sessionGeneration: '7', documentGeneration: '1', request: { action: 'save', payload: { value: 1 } } })
    for (const name of ['ExpiredWindowSessionError', 'WindowRequestCancelledError', 'WindowRequestTimeoutError', 'InvalidWindowResponseError', 'WindowRequestFailedError', 'WindowRequestUnavailableError']) {
      const e = windowBridge(true, async () => { throw name }); e.prepare()
      await expect(e.api.request({ action: 'load', payload: null })).rejects.toHaveProperty('name', name)
    }
    const e = windowBridge(true, async () => { throw new Error('private native detail') }); e.prepare()
    await expect(e.api.request({ action: 'load', payload: null })).rejects.toHaveProperty('name', 'WindowRequestUnavailableError')
  })
})

it('registers the bridge command and grants only the exact isolated Window content surface', () => {
  const command = 'plugin_window_request'
  expect(readFileSync('src-tauri/build.rs', 'utf8')).toContain(`"${command}"`)
  expect(readFileSync('src-tauri/src/lib.rs', 'utf8')).toContain(`commands::${command}`)
  expect(readFileSync(`src-tauri/permissions/autogenerated/${command}.toml`, 'utf8')).toContain(`commands.allow = ["${command}"]`)
  const grants = readdirSync('src-tauri/capabilities').filter(file => file.endsWith('.json')).map(file => JSON.parse(readFileSync(`src-tauri/capabilities/${file}`, 'utf8'))).filter(capability => capability.permissions.includes('allow-plugin-window-request'))
  expect(grants).toHaveLength(1)
  expect(grants[0].webviews).toEqual(['plugin-content-*'])
  expect(grants[0].identifier).toBe('plugin-window-content')
})
