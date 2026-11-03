import { createContextActionsMenu } from '../../../src/context-actions-ui.js'
import { installPanelContextActions } from '../../../src/panel-context-actions.js'

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
    ? ['#f7f7f8', '#ffffff', '#171719', '#6f6f74', '#d9d9dc', '#18191a', '#c62828']
    : ['#07080a', '#0d0d0d', '#f4f4f6', '#9c9c9d', '#242728', '#ffffff', '#ff6161']
  const names = ['background', 'surface', 'text', 'text-muted', 'border', 'accent', 'danger']
  names.forEach((name, index) => root.style.setProperty(`--uipilot-color-${name}`, tokens[index]))
  root.style.setProperty('--uipilot-font-family', 'Inter, Microsoft YaHei UI, system-ui, sans-serif')
  root.dataset.theme = theme
}

function installBridge(theme) {
  const actions = installPanelContextActions({ document, window, getEpoch: () => '1', createMenu: createContextActionsMenu,
    invoke: async (command, payload) => {
      if (window.parent === window && command === 'plugin_panel_actions_submit') {
        submitPreviewQuery()
      } else window.parent.postMessage({ type: 'notes-preview-actions-submit', ...payload }, window.location.origin)
    },
  })
  window.addEventListener('message', event => {
    if (event.origin !== window.location.origin || event.source !== window.parent) return
    if (event.data?.type === 'notes-preview-actions-open') window.__UIPILOT_PLUGIN_PANEL_ACTIONS__(event.data)
  })
  function openPreviewActions() {
    const source = document.activeElement?.id === 'preview-query' ? 'hostInput' : 'content'
    window.__UIPILOT_PLUGIN_PANEL_ACTIONS__({ sessionEpoch: '1', source, ticket: source === 'hostInput' ? 'preview-submit' : '' })
  }
  document.querySelector('#preview-operations')?.addEventListener('click', openPreviewActions)
  let updateHandler = null
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
    async list({ query = '' } = {}) {
      const value = query.trim().toLocaleLowerCase()
      const full = snapshot()
      return { ...full, entries: full.entries.filter(entry => !value || (entry.textPreview ?? entry.firstFileName ?? '').toLocaleLowerCase().includes(value)) }
    },
    onChanged(handler) {
      changedHandler = handler
      return () => {
        if (changedHandler === handler) changedHandler = null
      }
    },
    async copy({ id }) {
      if (!entries.some((entry) => entry.id === id)) {
        const error = new Error('redacted')
        error.name = 'RecordNotFound'
        throw error
      }
      return Object.freeze({ outcome: 'copied' })
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
    onActions: actions.register,
    async setStatus(status) {
      if (window.parent !== window) window.parent.postMessage({ type: 'notes-preview-status', status }, window.location.origin)
      else {
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
      updateHandler = handler
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
    async focusHostInput() {
      if (window.parent === window) document.querySelector('#preview-query')?.focus()
      else window.parent.postMessage({ type: 'notes-preview-focus-input' }, window.location.origin)
    },
    async requestHide() {},
    storage: Object.freeze({
      async get() { return null },
      async set() {},
      async remove() {},
    }),
    clipboardHistory,
  })
  Object.defineProperty(window, 'uipilotPluginPanel', { value: api })
  const input = document.querySelector('#preview-query')
  function submitPreviewQuery() {
    if (input) updateHandler?.({ input: input.value, theme, sessionEpoch: '1' })
  }
  input?.addEventListener('keydown', event => {
    if (event.isComposing || event.repeat) return
    if (event.key === 'Enter') {
      event.preventDefault()
      submitPreviewQuery()
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      hostKeyHandler?.({ key: 'ArrowDown', sessionEpoch: '1' })
    }
  })

  window.addEventListener('keydown', (event) => {
    if (event.target === input && event.key.toLowerCase() === 'k' && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && !event.isComposing && !event.repeat) {
      event.preventDefault()
      event.stopImmediatePropagation()
      openPreviewActions()
      return
    }
    if (actions.handleKey(event)) return
    if (event.defaultPrevented || event.target.closest?.('input, textarea, [contenteditable]')) return
    if (event.target.closest?.('.entry-select')) return
    if (!hostKeyHandler || event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return
    let routed = null
    if (event.key === 'Tab') routed = { key: 'Tab', shiftKey: event.shiftKey }
    else if (!event.shiftKey && event.key === 'ArrowDown') routed = { key: 'ArrowDown', shiftKey: false }
    else if (!event.shiftKey && event.key === 'ArrowUp') routed = { key: 'ArrowUp', shiftKey: false }
    else if (!event.shiftKey && !event.repeat && event.key === 'Delete') routed = { key: 'Delete', shiftKey: false }
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

  if (window.parent === window) {
    const input = document.createElement('input')
    input.id = 'preview-query'
    input.placeholder = '搜索文字或文件名'
    input.setAttribute('aria-label', '搜索剪贴板历史')
    document.body.prepend(input)
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
      .clipboard-app { flex: 1; min-height: 0; height: auto; }
      #preview-query { flex: none; height: 44px; margin: 8px; padding: 0 12px; border: 1px solid var(--uipilot-color-border); border-radius: 8px; background: var(--uipilot-color-surface); color: var(--uipilot-color-text); }
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

void loadPanel().catch(() => {
  document.body.textContent = 'Clipboard history preview failed to load.'
})
