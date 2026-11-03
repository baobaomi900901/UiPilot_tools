import { createContextActionsMenu } from '../../../src/context-actions-ui.js'
import { installPanelContextActions } from '../../../src/panel-context-actions.js'

const STORAGE_PREFIX = 'uipilot.notes.preview.'
const STORAGE_KEY = 'notes.entries'

const sampleNotes = [
  {
    id: 'preview-project',
    title: '项目备忘',
    content: '整理 Panel 页面交互\n确认键盘焦点和状态提示',
    createdAt: '2026-08-27T09:30:00.000Z',
  },
  {
    id: 'preview-code',
    title: '代码片段',
    content: 'npm.cmd test -- src/launcher.test.tsx',
    createdAt: '2026-08-27T09:20:00.000Z',
  },
  {
    id: 'preview-links',
    title: '常用链接',
    content: 'http://127.0.0.1:14321/',
    createdAt: '2026-08-27T09:10:00.000Z',
  },
]

function clone(value) {
  return value === undefined ? undefined : structuredClone(value)
}

function storageName(key) {
  return `${STORAGE_PREFIX}${key}`
}

const storage = Object.freeze({
  async get(key) {
    const stored = localStorage.getItem(storageName(key))
    return stored === null ? null : clone(JSON.parse(stored))
  },
  async set(key, value) {
    localStorage.setItem(storageName(key), JSON.stringify(value))
  },
  async remove(key) {
    localStorage.removeItem(storageName(key))
  },
})

function installTheme(theme) {
  const root = document.documentElement
  const tokens = theme === 'dark'
    ? ['#07080a', '#0d0d0d', '#f4f4f6', '#9c9c9d', '#242728', '#ffffff', '#ff6161']
    : ['#f7f7f8', '#ffffff', '#171719', '#6f6f74', '#d9d9dc', '#18191a', '#c62828']
  const names = ['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger']
  names.forEach((name, index) => root.style.setProperty(`--uipilot-color-${name}`, tokens[index]))
  root.style.setProperty('--uipilot-font-family', 'Inter, Microsoft YaHei UI, system-ui, sans-serif')
  root.dataset.theme = theme
}

function installBridge(theme) {
  let hostKeyHandler = null
  const actions = installPanelContextActions({ document, window, getEpoch: () => '1', createMenu: createContextActionsMenu,
    invoke: async (_command, payload) => window.parent.postMessage({ type: 'notes-preview-actions-submit', ...payload }, window.location.origin),
  })
  document.addEventListener('keydown', event => actions.handleKey(event), true)
  window.addEventListener('message', event => {
    if (event.origin !== window.location.origin || event.source !== window.parent) return
    if (event.data?.type === 'notes-preview-actions-open') window.__UIPILOT_PLUGIN_PANEL_ACTIONS__(event.data)
  })
  document.querySelector('#preview-operations')?.addEventListener('click', () => window.__UIPILOT_PLUGIN_PANEL_ACTIONS__({ sessionEpoch: '1' }))
  const update = Object.freeze({
    requestId: 'notes-preview-request',
    input: '',
    platform: 'windows',
    theme,
    invokedAt: new Date().toISOString(),
    sessionEpoch: '1',
    data: Object.freeze({}),
  })
  const api = Object.freeze({
    onActions: actions.register,
    async setStatus(status) {
      if (window.parent !== window) {
        window.parent.postMessage({ type: 'notes-preview-status', status }, window.location.origin)
      } else {
        const footer = document.querySelector('#preview-status')
        footer.textContent = status.message
        footer.dataset.tone = status.tone || 'info'
      }
    },
    onHostKey(handler) {
      hostKeyHandler = handler
      return () => {
        if (hostKeyHandler === handler) hostKeyHandler = null
      }
    },
    onUpdate(handler) {
      queueMicrotask(() => void Promise.resolve(handler(update)).catch(console.error))
      return () => undefined
    },
    async focusHostInput() { window.parent.postMessage({ type: 'notes-preview-focus-input' }, window.location.origin) },
    async requestHide() {},
    storage,
  })
  Object.defineProperty(window, 'uipilotPluginPanel', { value: api, configurable: false })
}

async function loadPanel() {
  const theme = new URLSearchParams(window.location.search).get('theme') === 'light' ? 'light' : 'dark'
  if (localStorage.getItem(storageName(STORAGE_KEY)) === null) {
    localStorage.setItem(storageName(STORAGE_KEY), JSON.stringify(sampleNotes))
  }

  const response = await fetch('./package/dist/panel.html')
  if (!response.ok) throw new Error(`Unable to load panel HTML (${response.status})`)
  const source = new DOMParser().parseFromString(await response.text(), 'text/html')
  source.querySelectorAll('script').forEach((script) => script.remove())

  const stylesheet = document.createElement('link')
  stylesheet.rel = 'stylesheet'
  stylesheet.href = './package/dist/panel.css'
  document.head.append(stylesheet)
  document.title = `${source.title} Preview`
  document.body.replaceChildren(...Array.from(source.body.childNodes, (node) => document.importNode(node, true)))
  if (window.parent === window) {
    const footer = document.createElement('footer')
    footer.id = 'preview-footer'
    const status = document.createElement('span')
    status.id = 'preview-status'
    status.setAttribute('role', 'status')
    status.setAttribute('aria-live', 'polite')
    const operations = document.createElement('button')
    operations.id = 'preview-operations'
    operations.textContent = '操作 Ctrl+K'
    operations.addEventListener('pointerdown', event => event.preventDefault())
    footer.append(status, operations)
    document.body.append(footer)
    const style = document.createElement('style')
    style.textContent = `body { display: flex; flex-direction: column; }
      .app { flex: 1; min-height: 0; height: auto; }
      #preview-footer { display: flex; align-items: center; gap: 12px; flex: none; min-height: 36px; padding: 6px 10px; border-top: 1px solid var(--uipilot-color-border); font-size: 12px; color: var(--uipilot-color-text-muted); }
      #preview-status { flex: 1 1 0; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
      #preview-operations { flex: none; white-space: nowrap; text-align: right; }
      #preview-status[data-tone=success] { color: #22a06b; }
      #preview-status[data-tone=error] { color: var(--uipilot-color-danger); }`
    document.head.append(style)
  }

  installTheme(theme)
  installBridge(theme)
  await import('./package/dist/panel.js')
}

void loadPanel().catch((error) => {
  console.error(error)
  document.body.textContent = 'Notes preview failed to load.'
})
