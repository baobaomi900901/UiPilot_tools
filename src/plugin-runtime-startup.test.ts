import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import { setImmediate } from 'node:timers/promises'
import { expect, it, vi } from 'vitest'

function startRuntime(readyState: string, root: { dataset: { runtimeEntry?: string } } | null) {
  const document = {
    readyState, documentElement: root, title: '',
    addEventListener: vi.fn((_event: string, _callback: () => void, _options: unknown) => {}),
  }
  const loadModule = vi.fn(async (_entry: string) => ({ onCommand() {}, onPanelAction() {} }))
  const window = { __TAURI_INTERNALS__: { invoke: vi.fn() }, loadModule }
  const template = readFileSync('src-tauri/src/public_plugins/runtime.rs', 'utf8')
    .match(/PUBLIC_RUNTIME_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)![1]
  const source = template
    .replace('__PANEL_JSON__', readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8'))
    .replace('__PANEL_ACTIONS__', 'true').replace('__WINDOW_ACTIONS__', 'false')
    .replace('__RESULT_ACTIONS__', 'false').replace('__NETWORK_HTTPS__', 'true')
    // Only replace module loading; execute the real entry lookup, handler checks and Ready signal.
    .replace('await import(entry)', 'await window.loadModule(entry)')
  vm.runInNewContext(source, { window, document, TextEncoder, setTimeout })
  return { document, window, loadModule }
}

it.each([null, { dataset: {} }])('waits for parsed Runtime HTML when injected at document creation: %j', async root => {
  const runtime = startRuntime('loading', root)
  await setImmediate()
  expect(runtime.document.title).toBe('')
  expect(runtime.loadModule).not.toHaveBeenCalled()
  expect(runtime.window).not.toHaveProperty('__TAURI_INTERNALS__')
  expect(runtime.document.addEventListener).toHaveBeenCalledWith('DOMContentLoaded', expect.any(Function), { once: true })
  runtime.document.documentElement = { dataset: { runtimeEntry: '/dist/runtime.js' } }
  runtime.document.readyState = 'interactive'
  runtime.document.addEventListener.mock.calls[0]![1]()
  await setImmediate()
  expect(runtime.loadModule).toHaveBeenCalledExactlyOnceWith('/dist/runtime.js')
  expect(runtime.document.title).toBe('uipilot-public-plugin-ready')
})

it.each(['interactive', 'complete'])('starts immediately when Runtime HTML is already %s', async state => {
  const runtime = startRuntime(state, { dataset: { runtimeEntry: '/dist/runtime.js' } })
  await setImmediate()
  expect(runtime.document.addEventListener).not.toHaveBeenCalled()
  expect(runtime.loadModule).toHaveBeenCalledExactlyOnceWith('/dist/runtime.js')
  expect(runtime.document.title).toBe('uipilot-public-plugin-ready')
})

it('still reports a missing entry after the document is parsed', async () => {
  const runtime = startRuntime('complete', { dataset: {} })
  await setImmediate()
  expect(runtime.loadModule).not.toHaveBeenCalled()
  expect(runtime.document.title).toBe('uipilot-public-plugin-failed')
})
