import { readFileSync, readdirSync } from 'node:fs'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'
const json = readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8')
const template = readFileSync('src-tauri/src/public_plugins/runtime.rs', 'utf8').match(/PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)![1]!
async function runtime(module: any, enabled = true) {
  const invoke = vi.fn(async (_command: string, _args?: any): Promise<void> => undefined)
  const window: any = { testModule: module, __TAURI_INTERNALS__: { invoke } }
  const document = { title: '', documentElement: { dataset: { runtimeEntry: '/runtime.js' } } }
  const source = template.replace('__RESULT_ACTIONS__', String(enabled)).replace('__WINDOW_ACTIONS__', 'false').replace('__PANEL_ACTIONS__', 'false').replace('__NETWORK_HTTPS__', 'false').replace('__PANEL_JSON__', json).replace('const module = entry ? await import(entry) : null;', 'const module = window.testModule;')
  vm.runInNewContext(source, { window, document, TextEncoder, Object, Reflect, Error, TypeError, Promise, WeakSet, Set, setTimeout })
  await vi.waitFor(() => expect(document.title).not.toBe(''))
  return { callback: (message: { payload: unknown }) => window.__UIPILOT_PUBLIC_PLUGIN_DISPATCH__(message.payload), document, invoke }
}
const context = { pluginId: 'com.example.test', pluginGeneration: 1, requestId: 'public-request-0000000000000001' }
const invocation = { apiVersion: 1, requestId: context.requestId, commandId: 'constructor', resultId: 'constructor', actionId: 'save', payload: { value: 1 }, input: 'saved input', context: { platform: 'windows', theme: 'dark', invokedAt: '2026-10-25T00:00:00Z' } }
describe('result action executable Runtime bootstrap', () => {
  it('requires onAction only for declared capability', async () => {
    expect((await runtime({ onCommand() {} })).document.title).toBe('uipilot-public-plugin-failed')
    expect((await runtime({ onCommand() {} }, false)).document.title).toBe('uipilot-public-plugin-ready')
    expect((await runtime({ onAction() {} })).document.title).toBe('uipilot-public-plugin-failed')
  })
  it('dispatches saved business identity, freezes fresh facades, expires API before completion and never repeats lost completion', async () => {
    let oldApi: any, firstInvocation: any
    const onAction = vi.fn(async (input, api) => { firstInvocation = input; oldApi = api; await api.storage.set('draft', input.payload); return { requestId: input.requestId, results: [] } })
    const b = await runtime({ onCommand() {}, onAction })
    await b.callback({ payload: { kind: 'resultAction', context, invocation } })
    expect(onAction).toHaveBeenCalledTimes(1)
    expect(Object.isFrozen(firstInvocation.payload)).toBe(true)
    expect(Object.isFrozen(oldApi.storage)).toBe(true)
    expect(firstInvocation.commandId).toBe('constructor')
    expect(b.invoke.mock.calls.filter(([cmd]) => cmd === 'plugin_api_call')).toHaveLength(1)
    expect(() => oldApi.storage.get('draft')).toThrow('ExpiredRequestError')
    const completed = b.invoke.mock.calls.filter(([cmd]) => cmd === 'complete_plugin_command')
    expect(completed).toHaveLength(1)
    expect(completed[0]![1]).toMatchObject({ completion: { response: { requestId: context.requestId, results: [] } } })
    b.invoke.mockImplementation(async (command: string) => { if (command === 'complete_plugin_command') throw new Error('transport') })
    const previous = oldApi
    await b.callback({ payload: { kind: 'resultAction', context: { ...context, requestId: 'public-request-0000000000000002' }, invocation: { ...invocation, requestId: 'public-request-0000000000000002' } } })
    expect(oldApi).not.toBe(previous)
    expect(b.invoke.mock.calls.filter(([cmd]) => cmd === 'complete_plugin_command')).toHaveLength(2)
  })
  it('classifies thrown handlers separately from malformed response snapshots without invoking accessors', async () => {
    let getters = 0
    const getter = Object.defineProperty({}, 'results', { enumerable: true, get() { getters++; return [] } })
    Object.defineProperty(getter, 'requestId', { value: context.requestId, enumerable: true })
    const cycle: any = {}; cycle.loop = cycle
    const dangerous = JSON.parse('{"constructor":1}')
    for (const results of [getter, { requestId: context.requestId, results: [cycle] }, { requestId: context.requestId, results: [dangerous] }, { requestId: context.requestId, results: Array(4096).fill(null) }, { requestId: context.requestId, results: [, 1] }]) {
      const b = await runtime({ onCommand() {}, async onAction() { return results } })
      await b.callback({ payload: { kind: 'resultAction', context, invocation } })
      expect(b.invoke.mock.calls.find(([cmd]) => cmd === 'complete_plugin_command')![1]).toEqual({ completion: { context, response: null } })
    }
    expect(getters).toBe(0)
    const b = await runtime({ onCommand() {}, async onAction() { throw new Error('private exception') } })
    await b.callback({ payload: { kind: 'resultAction', context, invocation } })
    expect(b.invoke.mock.calls.find(([cmd]) => cmd === 'complete_plugin_command')![1]).toEqual({ completion: { context, failed: true } })
  })
})
it('registers actual result action commands only for main and guards caller before state access', () => {
  const source = readFileSync('src-tauri/src/commands.rs', 'utf8')
  for (const command of ['execute_result_action', 'retire_result_actions']) {
    expect(readFileSync('src-tauri/build.rs', 'utf8')).toContain(`"${command}"`)
    expect(readFileSync('src-tauri/src/lib.rs', 'utf8')).toContain(`commands::${command}`)
    expect(readFileSync(`src-tauri/permissions/autogenerated/${command}.toml`, 'utf8')).toContain(`commands.allow = ["${command}"]`)
    const grants = readdirSync('src-tauri/capabilities').filter(file => file.endsWith('.json')).map(file => JSON.parse(readFileSync(`src-tauri/capabilities/${file}`, 'utf8'))).filter(c => c.permissions.includes(`allow-${command.replaceAll('_', '-')}`))
    expect(grants).toHaveLength(1); expect(grants[0].webviews).toEqual(['main'])
    const body = source.split(`fn ${command}(`)[1]!.split('\n}')[0]!
    expect(body.indexOf('require_main_label(window.label())?;')).toBeLessThan(body.indexOf('.state::<'))
    expect(body).toContain('serde_json::from_value(request)')
  }
})
