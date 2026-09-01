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
    : ['#f7f7f8', '#ffffff', '#171719', '#6f6f74', '#d9d9dc', '#18191a', '#dc4343']
  const names = ['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger']
  names.forEach((name, index) => root.style.setProperty(`--uipilot-color-${name}`, tokens[index]))
  root.style.setProperty('--uipilot-font-family', 'Inter, Microsoft YaHei UI, system-ui, sans-serif')
  root.dataset.theme = theme
}

function installBridge(theme) {
  let hostKeyHandler = null
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
    async focusHostInput() {},
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

  installTheme(theme)
  installBridge(theme)
  await import('./package/dist/panel.js')
}

void loadPanel().catch((error) => {
  console.error(error)
  document.body.textContent = 'Notes preview failed to load.'
})
