// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it, vi } from 'vitest'

const source = readFileSync('src-tauri/src/webview_reload_guard.js', 'utf8')
const cleanups: (() => void)[] = []
function install() {
  new Function('window', source)({
    addEventListener(type: string, handler: EventListener, capture: boolean) {
      window.addEventListener(type, handler, capture)
      cleanups.push(() => window.removeEventListener(type, handler, capture))
    },
  })
}
afterEach(() => {
  cleanups.splice(0).forEach(cleanup => cleanup())
  document.body.replaceChildren()
})

describe('document-start reload shortcut guard', () => {
  it.each([
    { key: 'r', ctrlKey: true }, { key: 'R', ctrlKey: true, shiftKey: true },
    { key: 'F5' }, { key: 'F5', ctrlKey: true }, { key: 'F5', shiftKey: true },
    { key: 'F5', ctrlKey: true, shiftKey: true }, { key: 'BrowserRefresh' },
    { key: 'r', ctrlKey: true, repeat: true },
  ])('blocks refresh from input, editor, navigation or background: %j', init => {
    install()
    document.body.innerHTML = '<input><textarea></textarea><button role="tab">插件</button>'
    for (const target of [document.body, ...document.body.children]) {
      const pageHandler = vi.fn()
      target.addEventListener('keydown', pageHandler, { once: true })
      cleanups.push(() => target.removeEventListener('keydown', pageHandler))
      const event = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true })
      target.dispatchEvent(event)
      expect(event.defaultPrevented).toBe(true)
      expect(pageHandler).not.toHaveBeenCalled()
    }
  })

  it.each([
    { key: 'r' }, { key: 'R', shiftKey: true },
    ...['t', 'c', 'v', 'x', 'a', 's', 'f', 'k'].map(key => ({ key, ctrlKey: true })),
    { key: 'F12' }, { key: 'Tab' }, { key: 'ArrowDown' },
    { key: 'r', ctrlKey: true, altKey: true }, { key: 'r', metaKey: true },
  ])('preserves unrelated input and application shortcuts: %j', init => {
    install()
    const pageHandler = vi.fn()
    document.body.addEventListener('keydown', pageHandler, { once: true })
    const event = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true })
    document.body.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(pageHandler).toHaveBeenCalledOnce()
  })

  it('leaves business refresh clicks intact and installs on every native WebView', () => {
    install()
    const refresh = document.createElement('button')
    document.body.append(refresh)
    const load = vi.fn()
    refresh.addEventListener('click', load)
    refresh.click()
    expect(load).toHaveBeenCalledOnce()
    const native = readFileSync('src-tauri/src/webview_reload_guard.rs', 'utf8')
    expect(native).toContain('.js_init_script_on_all_frames(include_str!("webview_reload_guard.js"))')
    expect(native).toContain('.on_webview_ready(')
    expect(readFileSync('src-tauri/src/lib.rs', 'utf8')).toContain('.plugin(webview_reload_guard::init())')
    expect(native).not.toContain('SetAreBrowserAcceleratorKeysEnabled')
  })
})
