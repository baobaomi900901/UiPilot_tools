import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { expect, it, vi } from 'vitest'

function bridge(images = true, response: () => Promise<unknown> = async () => undefined) {
  const listeners = new Map<string, (event: { detail?: unknown }) => void>()
  const invoke = vi.fn(async (command: string, _args?: unknown) => command === 'plugin_panel_clear_image' ? response() : { ok: true })
  const window: any = { __TAURI_INTERNALS__: { invoke }, addEventListener: (name: string, callback: (event: { detail?: unknown }) => void) => listeners.set(name, callback) }
  const read = (path: string) => readFileSync(path, 'utf8')
  const template = read('src-tauri/src/plugin_panel.rs').match(/PUBLIC_PANEL_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)![1]!
  let source = template
    .replace('__CONTEXT_ACTIONS_UI__', read('src/context-actions-ui.js').replace('export function', 'function') + '\n' + read('src/panel-context-actions.js').replaceAll('export function', 'function'))
    .replace('__PANEL_EFFECTS__', read('src-tauri/src/panel_effects.js'))
    .replace('__CLIPBOARD_ENABLED__', 'false').replace('__SPEECH_ENABLED__', 'false')
    .replace('__PANEL_JSON__', read('src-tauri/src/public_plugins/panel_action_json.js'))
    .replace('__RUNTIME_ACTIONS__', 'true').replace('__IMAGES_ENABLED__', String(images))
    .replaceAll('__SESSION_EPOCH__', '7').replace('__HOST_KEYS__', '[]')
  if (source.includes('__PANEL_IMAGE_BRIDGE__')) source = source.replace('__PANEL_IMAGE_BRIDGE__', read('src-tauri/src/panel_image_bridge.js'))
  vm.runInNewContext(source, { window, document: { documentElement: { style: { setProperty() {} } }, addEventListener() {}, querySelector() { return null } }, TextEncoder, Object, Error, TypeError, Promise, Reflect, Set, WeakSet, setTimeout, clearInterval, setInterval, queueMicrotask })
  const api = window.uipilotPluginPanel
  const update = async (requestId = 'a') => {
    await window.__UIPILOT_PLUGIN_PANEL_UPDATE__({ sessionEpoch: '7', requestId, theme: 'light' })
    window.__UIPILOT_PLUGIN_PANEL_READY__({ sessionEpoch: '7', requestId })
    await Promise.resolve()
    await Promise.resolve()
  }
  return { api, window, invoke, update, send: (detail: unknown) => listeners.get('uipilot-plugin-panel-image-changed')?.({ detail }), hide: () => listeners.get('pagehide')?.({}) }
}
const image = (id = 'image-a') => ({ id, mimeType: 'image/png', width: 1, height: 1, byteLength: 70, previewDataUrl: 'data:image/png;base64,aGVsbG8=' })
const event = (revision: string, value: ReturnType<typeof image> | null = image()) => ({ sessionEpoch: '7', revision, image: value })

it('exposes image APIs only for capable panels and delivers the latest frozen snapshot after Ready', async () => {
  expect(bridge(false).api.onImageChanged).toBeUndefined()
  expect(bridge(false).api.clearImage).toBeUndefined()
  const p = bridge(), order: string[] = [], events: any[] = []
  p.api.onImageChanged((value: unknown) => { order.push('image'); events.push(value) })
  p.api.onReady(() => order.push('ready'))
  p.api.onUpdate(() => undefined)
  p.send(event('1'))
  const latest = event('2', image('image-b'))
  p.send(latest)
  latest.image!.id = 'mutated'
  expect(events).toEqual([])
  await p.update()
  expect(order).toEqual(['ready', 'image'])
  expect(events).toEqual([event('2', image('image-b'))])
  expect(Object.isFrozen(events[0])).toBe(true)
  expect(Object.isFrozen(events[0].image)).toBe(true)
  p.send(event('2')); p.send(event('1')); p.send({ ...event('9'), sessionEpoch: '6' })
  await p.update('theme-update')
  expect(events).toHaveLength(1)
  p.send(event('3', null))
  await Promise.resolve()
  expect(events[events.length - 1]).toEqual(event('3', null))
})

it('invalidates queued notifications and retained clear calls on session replacement or pagehide', async () => {
  const p = bridge(), received = vi.fn()
  p.api.onImageChanged(received); p.api.onUpdate(() => undefined)
  await p.update()
  const clear = p.api.clearImage
  p.send(event('1'))
  p.window.__UIPILOT_PLUGIN_PANEL_PREPARE__({ sessionEpoch: '8' })
  await Promise.resolve()
  expect(received).not.toHaveBeenCalled()
  await expect(clear()).rejects.toHaveProperty('name', 'ExpiredPanelSession')
  const currentClear = p.api.clearImage
  const request = p.api.request
  p.hide()
  await expect(currentClear()).rejects.toHaveProperty('name', 'ExpiredPanelSession')
  await expect(request({ action: 'recognize', payload: null, imageId: 'old' })).rejects.toHaveProperty('name', 'ExpiredPanelSession')
  expect(p.invoke.mock.calls.filter(([command]) => command === 'plugin_panel_clear_image')).toEqual([])
})

it('requires early registration, supports unsubscribe and contains handler failures', async () => {
  const p = bridge(), received = vi.fn(() => { throw new Error('private') })
  const unsubscribe = p.api.onImageChanged(received)
  p.api.onUpdate(() => undefined)
  expect(() => p.api.onImageChanged(() => undefined)).toThrow()
  await p.update()
  p.send(event('1')); await Promise.resolve()
  expect(received).toHaveBeenCalledTimes(1)
  p.send(event('2')); unsubscribe(); await Promise.resolve()
  expect(received).toHaveBeenCalledTimes(1)
})

it('clears through a session-bound IPC and rejects stale completion', async () => {
  let release!: () => void
  const p = bridge(true, () => new Promise<void>(resolve => { release = resolve }))
  const pending = p.api.clearImage()
  expect(p.invoke).toHaveBeenCalledWith('plugin_panel_clear_image', { sessionEpoch: '7' })
  p.window.__UIPILOT_PLUGIN_PANEL_PREPARE__({ sessionEpoch: '8' }); release()
  await expect(pending).rejects.toHaveProperty('name', 'ExpiredPanelSession')
})

it('accepts explicit imageId only for capable panels and snapshots without invoking getters', async () => {
  const p = bridge()
  await expect(p.api.request({ action: 'recognize', payload: { providerId: 'baidu' }, imageId: 'panel-image' })).resolves.toEqual({ ok: true })
  expect(p.invoke).toHaveBeenCalledWith('plugin_panel_request', { sessionEpoch: '7', request: { action: 'recognize', payload: { providerId: 'baidu' }, imageId: 'panel-image' } })
  await expect(bridge(false).api.request({ action: 'recognize', payload: null, imageId: 'a' })).rejects.toHaveProperty('name', 'InvalidPanelRequestError')
  const getter = vi.fn(() => 'a')
  const accessor = Object.defineProperty({ action: 'recognize', payload: null }, 'imageId', { enumerable: true, get: getter })
  for (const input of [accessor, ...[null, '', 3, 'x'.repeat(513), '\0', '字'.repeat(200)].map(imageId => ({ action: 'recognize', payload: null, imageId }))]) {
    await expect(p.api.request(input)).rejects.toHaveProperty('name', 'InvalidPanelRequestError')
  }
  expect(getter).not.toHaveBeenCalled()
  expect(p.invoke).toHaveBeenCalledTimes(1)
})

it('drops malformed or oversized host events without consuming the next valid revision', async () => {
  const p = bridge(), received = vi.fn()
  p.api.onImageChanged(received); p.api.onUpdate(() => undefined); await p.update()
  for (const invalid of [event('01'), event('18446744073709551616'), { ...event('1'), extra: true }, event('1', { ...image(), previewDataUrl: 'data:image/png;base64,' + 'A'.repeat(60000) }), event('1', { ...image(), mimeType: 'image/jpeg' })]) p.send(invalid)
  await Promise.resolve()
  expect(received).not.toHaveBeenCalled()
  p.send(event('1')); await Promise.resolve()
  expect(received).toHaveBeenCalledTimes(1)
})
