function minutesAgo(minutes) {
  return new Date(Date.now() - minutes * 60_000).toISOString()
}

function createPreviewImage() {
  const canvas = document.createElement('canvas')
  canvas.width = 640
  canvas.height = 360
  const context = canvas.getContext('2d')
  context.fillStyle = '#f4f7f6'
  context.fillRect(0, 0, canvas.width, canvas.height)
  context.fillStyle = '#173f35'
  context.fillRect(42, 38, 556, 48)
  context.fillStyle = '#ffffff'
  context.font = '600 24px Segoe UI, sans-serif'
  context.fillText('Release overview', 66, 70)
  const colors = ['#2f7d68', '#d7a43b', '#d85c4a', '#4d6f91']
  const heights = [116, 184, 142, 218]
  heights.forEach((height, index) => {
    context.fillStyle = colors[index]
    context.fillRect(74 + index * 126, 304 - height, 76, height)
  })
  return canvas.toDataURL('image/png')
}

function clone(value) {
  return structuredClone(value)
}

function installTheme(theme) {
  const root = document.documentElement
  const tokens = theme === 'light'
    ? ['#ffffff', '#f7f7f7', '#171717', '#595959', '#d9d9d9', '#17745f', '#c62828']
    : ['#1c1f1e', '#252927', '#f2f4f3', '#aeb8b4', '#414845', '#63c5a8', '#ff7875']
  const names = ['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger']
  names.forEach((name, index) => root.style.setProperty(`--uipilot-color-${name}`, tokens[index]))
  root.style.setProperty('--uipilot-font-family', 'Segoe UI, system-ui, sans-serif')
  root.dataset.theme = theme
}

function installBridge(theme) {
  let hostKeyHandler = null
  let changedHandler = null
  let routeSequence = 0
  let revision = 1
  let entries = [
    {
      id: 'preview-text-long',
      kind: 'text',
      capturedAt: minutesAgo(1),
      textPreview: '会议结论：周五之前完成验收清单，并确认 Windows 下文本、图片和多文件粘贴都只执行一次。',
    },
    {
      id: 'preview-image',
      kind: 'image',
      capturedAt: minutesAgo(4),
      previewDataUrl: createPreviewImage(),
      width: 1280,
      height: 720,
    },
    {
      id: 'preview-files',
      kind: 'files',
      capturedAt: minutesAgo(8),
      firstFileName: '产品发布资料.pdf',
      fileCount: 3,
      available: true,
    },
    {
      id: 'preview-text-code',
      kind: 'text',
      capturedAt: minutesAgo(12),
      textPreview: 'node --test --experimental-test-isolation=none examples/public-plugins/com.uipilot.clipboard-history/tests/*.test.js',
    },
    {
      id: 'preview-files-missing',
      kind: 'files',
      capturedAt: minutesAgo(18),
      firstFileName: '已移动的归档.zip',
      fileCount: 1,
      available: false,
    },
  ]

  function snapshot() {
    return Object.freeze({
      revision: String(revision),
      entries: Object.freeze(clone(entries).map(Object.freeze)),
    })
  }

  function publish() {
    revision += 1
    if (changedHandler) queueMicrotask(() => void changedHandler(snapshot()))
  }

  const clipboardHistory = Object.freeze({
    async list() {
      return snapshot()
    },
    onChanged(handler) {
      changedHandler = handler
      return () => {
        if (changedHandler === handler) changedHandler = null
      }
    },
    async paste({ id }) {
      if (!entries.some((entry) => entry.id === id)) {
        const error = new Error('redacted')
        error.name = 'RecordNotFound'
        throw error
      }
      return Object.freeze({ outcome: 'admitted' })
    },
    async remove({ id }) {
      entries = entries.filter((entry) => entry.id !== id)
      publish()
    },
    async clear() {
      entries = []
      publish()
    },
  })

  const api = Object.freeze({
    onHostKey(handler) {
      hostKeyHandler = handler
      return () => {
        if (hostKeyHandler === handler) hostKeyHandler = null
      }
    },
    onUpdate(handler) {
      queueMicrotask(() => void handler(Object.freeze({
        requestId: 'clipboard-preview-request',
        input: '',
        platform: 'windows',
        theme,
        invokedAt: new Date().toISOString(),
        sessionEpoch: '1',
        data: Object.freeze({}),
      })))
      return () => {}
    },
    async focusHostInput() {},
    async requestHide() {},
    storage: Object.freeze({
      async get() { return null },
      async set() {},
      async remove() {},
    }),
    clipboardHistory,
  })
  Object.defineProperty(window, 'uipilotPluginPanel', { value: api })

  window.addEventListener('keydown', (event) => {
    if (!hostKeyHandler || event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return
    let routed = null
    if (event.key === 'Tab') routed = { key: 'Tab', shiftKey: event.shiftKey }
    else if (!event.shiftKey && event.key === 'ArrowDown') routed = { key: 'ArrowDown', shiftKey: false }
    else if (!event.shiftKey && event.key === 'ArrowUp') routed = { key: 'ArrowUp', shiftKey: false }
    else if (!event.shiftKey && event.key === 'Enter') routed = { key: 'Enter', shiftKey: false }
    if (!routed) return

    event.preventDefault()
    routeSequence += 1
    void hostKeyHandler(Object.freeze({
      ...routed,
      routeSequence: String(routeSequence),
      sessionEpoch: '1',
      ctrlKey: false,
      metaKey: false,
      altKey: false,
    }))
  }, true)
}

async function loadPanel() {
  const theme = new URLSearchParams(window.location.search).get('theme') === 'light' ? 'light' : 'dark'
  const response = await fetch('./package/dist/panel.html')
  if (!response.ok) throw new Error(`Unable to load panel HTML (${response.status})`)
  const source = new DOMParser().parseFromString(await response.text(), 'text/html')
  source.querySelectorAll('script').forEach((script) => script.remove())

  const stylesheet = document.createElement('link')
  stylesheet.rel = 'stylesheet'
  stylesheet.href = './package/dist/panel.css'
  document.head.append(stylesheet)
  document.body.replaceChildren(...Array.from(source.body.childNodes, (node) => document.importNode(node, true)))

  installTheme(theme)
  installBridge(theme)
  await import('./package/dist/panel.js')
}

void loadPanel().catch(() => {
  document.body.textContent = 'Clipboard history preview failed to load.'
})
