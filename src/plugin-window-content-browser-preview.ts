import { isWindowPinShortcut, WINDOW_PIN_SHORTCUT_EVENT } from './window-pin-shortcut'

// The real plugin document and runtime run against an in-memory preview store.
const base = '/examples/public-plugins/com.uipilot.json-workbench/package/dist/'
const scheme = new URLSearchParams(window.location.search).get('theme') === 'light' ? 'light' : 'dark'
document.documentElement.dataset.colorScheme = scheme
const source = await fetch(`${base}window.html`).then(response => response.text())
const documentSource = new DOMParser().parseFromString(source, 'text/html')
documentSource.querySelectorAll('script').forEach(script => script.remove())
document.body.replaceChildren(...documentSource.body.childNodes)
const stylesheet = document.createElement('link')
stylesheet.rel = 'stylesheet'
stylesheet.href = `${base}window.css`
document.head.append(stylesheet)
let stored = { schemaVersion: 1, revision: 1, text: '{"name":"UiPilot","enabled":true,"commands":["搜索","格式化"]}' }
const storage = { get: async () => structuredClone(stored), set: async (_key: string, value: typeof stored) => { stored = structuredClone(value) } }
const runtime = await import(/* @vite-ignore */ `${base}runtime.js`)
type PreviewRequest = { action: string; payload: unknown }
const bridge = {
  onUpdate(handler: (update: unknown) => void) {
    void runtime.onCommand({ requestId: 'preview-window' }, { storage }).then((result: { data: unknown }) => {
      handler({ requestId: 'preview-window', theme: scheme, data: result.data })
    })
    return () => {}
  },
  async request(input: PreviewRequest) {
    return (await runtime.onWindowAction({ ...input, requestId: 'preview-action' }, { storage })).data
  },
  files: { openText: async () => ({ name: 'example.json', text: '{"imported":true}' }) },
}
Object.assign(window, { uipilotPluginWindow: bridge })
window.addEventListener('keydown', event => {
  if (!isWindowPinShortcut(event)) return
  event.preventDefault()
  event.stopPropagation()
  if (!event.repeat) window.parent.dispatchEvent(new Event(WINDOW_PIN_SHORTCUT_EVENT))
}, true)
await import(/* @vite-ignore */ `${base}window.js`)
