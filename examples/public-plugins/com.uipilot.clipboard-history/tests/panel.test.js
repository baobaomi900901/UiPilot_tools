import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { JSDOM } from 'jsdom'

const panelHtmlUrl = new URL('../package/dist/panel.html', import.meta.url)
const panelScriptUrl = new URL('../package/dist/panel.js', import.meta.url)
const panelCssUrl = new URL('../package/dist/panel.css', import.meta.url)
let panelModuleSequence = 0

const snapshot = Object.freeze({
  revision: '10',
  entries: Object.freeze([
    Object.freeze({
      id: 'text-1',
      kind: 'text',
      capturedAt: '2026-08-30T06:00:00.000Z',
      textPreview: '项目发布说明\n第二行预览',
    }),
    Object.freeze({
      id: 'image-1',
      kind: 'image',
      capturedAt: '2026-08-30T05:59:00.000Z',
      previewDataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      width: 1280,
      height: 720,
    }),
    Object.freeze({
      id: 'files-1',
      kind: 'files',
      capturedAt: '2026-08-30T05:58:00.000Z',
      firstFileName: '交付清单.pdf',
      fileCount: 3,
      available: true,
    }),
    Object.freeze({
      id: 'files-missing',
      kind: 'files',
      capturedAt: '2026-08-30T05:57:00.000Z',
      firstFileName: '已移动的文件.zip',
      fileCount: 1,
      available: false,
    }),
  ]),
})

function deferred() {
  let resolve
  let reject
  const promise = new Promise((nextResolve, nextReject) => {
    resolve = nextResolve
    reject = nextReject
  })
  return { promise, resolve, reject }
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

async function loadPanel({
  initialList = Promise.resolve(snapshot),
  paste = async () => ({ outcome: 'admitted' }),
  scrollMetrics,
} = {}) {
  const html = await readFile(panelHtmlUrl, 'utf8')
  const dom = new JSDOM(html, { url: 'https://panel.uipilot.invalid/' })
  dom.window.requestAnimationFrame = (callback) => {
    callback(0)
    return 1
  }
  if (scrollMetrics) {
    const historyList = dom.window.document.querySelector('#history-list')
    const scrollbar = dom.window.document.querySelector('#history-list-scrollbar')
    assert.ok(historyList)
    assert.ok(scrollbar)
    Object.defineProperties(historyList, {
      clientHeight: { configurable: true, value: scrollMetrics.clientHeight },
      scrollHeight: { configurable: true, value: scrollMetrics.scrollHeight },
    })
    Object.defineProperty(scrollbar, 'clientHeight', {
      configurable: true,
      value: scrollMetrics.trackHeight,
    })
  }
  const calls = {
    order: [],
    paste: [],
    remove: [],
    clear: 0,
    focusHostInput: 0,
  }
  let hostKeyHandler = null
  let updateHandler = null
  let changedHandler = null

  const api = {
    onHostKey(handler) {
      calls.order.push('onHostKey')
      hostKeyHandler = handler
      return () => {}
    },
    onUpdate(handler) {
      calls.order.push('onUpdate')
      updateHandler = handler
      return () => {}
    },
    async focusHostInput() {
      calls.focusHostInput += 1
    },
    async requestHide() {},
    storage: {
      get: async () => null,
      set: async () => {},
      remove: async () => {},
    },
    clipboardHistory: {
      list() {
        calls.order.push('list')
        return initialList
      },
      onChanged(handler) {
        calls.order.push('onChanged')
        changedHandler = handler
        return () => {}
      },
      async paste(input) {
        calls.paste.push(input)
        return paste(input)
      },
      async remove(input) {
        calls.remove.push(input)
      },
      async clear() {
        calls.clear += 1
      },
    },
  }
  Object.defineProperty(dom.window, 'uipilotPluginPanel', { value: api })

  globalThis.window = dom.window
  globalThis.document = dom.window.document
  panelModuleSequence += 1
  const moduleUrl = new URL(panelScriptUrl)
  moduleUrl.searchParams.set('test', String(panelModuleSequence))
  await import(moduleUrl.href)
  await flush()

  return {
    calls,
    document: dom.window.document,
    emitSnapshot: async (next) => changedHandler?.(next),
    hostKey: async (event) => hostKeyHandler?.(event),
    update: async (value) => updateHandler?.(value),
    async click(selector) {
      const target = dom.window.document.querySelector(selector)
      assert.ok(target, `missing click target: ${selector}`)
      target.dispatchEvent(new dom.window.MouseEvent('click', { bubbles: true, cancelable: true }))
      await flush()
    },
    flush,
    cleanup() {
      dom.window.close()
      delete globalThis.window
      delete globalThis.document
    },
  }
}

test('registers bridges before list completion and renders every summary kind', async (t) => {
  const pendingList = deferred()
  const panel = await loadPanel({ initialList: pendingList.promise })
  t.after(panel.cleanup)

  assert.deepEqual(panel.calls.order, ['onHostKey', 'onUpdate', 'onChanged', 'list'])
  assert.equal(panel.document.querySelectorAll('[role="tab"]').length, 4)
  assert.equal(panel.document.querySelectorAll('[role="option"]').length, 0)

  pendingList.resolve(snapshot)
  await panel.flush()

  assert.equal(panel.document.querySelectorAll('[role="option"]').length, 4)
  assert.match(panel.document.body.textContent, /项目发布说明/)
  assert.match(panel.document.body.textContent, /1280 × 720/)
  assert.match(panel.document.body.textContent, /交付清单\.pdf/)
  assert.match(panel.document.body.textContent, /3 个文件/)
  assert.match(panel.document.body.textContent, /文件不存在/)
  assert.equal(panel.document.querySelector('[data-entry-id="text-1"]')?.getAttribute('aria-selected'), 'true')
})

function hostKey(key, routeSequence, { shiftKey = false } = {}) {
  return Object.freeze({
    key,
    routeSequence,
    sessionEpoch: '1',
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey,
  })
}

function selectedEntryId(panel) {
  return panel.document.querySelector('[role="option"][aria-selected="true"]')?.dataset.entryId ?? null
}

function activeFilter(panel) {
  return panel.document.querySelector('[role="tab"][aria-selected="true"]')?.dataset.filter ?? null
}

test('keeps the scrollable history list out of the sequential focus order', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  assert.equal(panel.document.querySelector('#history-list')?.getAttribute('tabindex'), '-1')
})

test('cycles filters and clamps list selection with Host-routed keys', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  assert.equal(activeFilter(panel), 'all')
  assert.equal(selectedEntryId(panel), 'text-1')

  await panel.hostKey(hostKey('Tab', '1'))
  assert.equal(activeFilter(panel), 'image')
  assert.equal(selectedEntryId(panel), 'image-1')

  await panel.hostKey(hostKey('Tab', '2', { shiftKey: true }))
  assert.equal(activeFilter(panel), 'all')
  assert.equal(selectedEntryId(panel), 'text-1')

  await panel.hostKey(hostKey('ArrowUp', '3'))
  assert.equal(selectedEntryId(panel), 'text-1')
  await panel.hostKey(hostKey('ArrowDown', '4'))
  assert.equal(selectedEntryId(panel), 'image-1')
  await panel.hostKey(hostKey('ArrowDown', '5'))
  await panel.hostKey(hostKey('ArrowDown', '6'))
  await panel.hostKey(hostKey('ArrowDown', '7'))
  assert.equal(selectedEntryId(panel), 'files-missing')
})

test('keeps Host input focus stable while consecutive Tab remains routable', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('Tab', '17'))
  assert.equal(activeFilter(panel), 'image')
  assert.equal(panel.calls.focusHostInput, 0)

  await panel.hostKey(hostKey('Tab', '18'))
  assert.equal(activeFilter(panel), 'files')
  assert.equal(panel.calls.focusHostInput, 0)
})

test('keeps Host input focus stable while routed Arrow keys navigate the list', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('ArrowDown', '17'))
  assert.equal(selectedEntryId(panel), 'image-1')
  assert.equal(panel.calls.focusHostInput, 0)

  await panel.hostKey(hostKey('ArrowUp', '18'))
  assert.equal(selectedEntryId(panel), 'text-1')
  assert.equal(panel.calls.focusHostInput, 0)
})

test('shows a Notes-style virtual scrollbar and mirrors list scrolling', async (t) => {
  const panel = await loadPanel({
    scrollMetrics: { clientHeight: 100, scrollHeight: 400, trackHeight: 100 },
  })
  t.after(panel.cleanup)

  const shell = panel.document.querySelector('.history-list-shell')
  const list = panel.document.querySelector('#history-list')
  const scrollbar = panel.document.querySelector('#history-list-scrollbar')
  const thumb = panel.document.querySelector('#history-list-scrollbar-thumb')
  assert.ok(shell)
  assert.ok(list)
  assert.ok(scrollbar)
  assert.ok(thumb)
  assert.equal(shell.classList.contains('is-scrollable'), true)
  assert.equal(thumb.style.height, '25px')
  assert.equal(thumb.style.transform, 'translateY(0px)')

  list.scrollTop = 150
  list.dispatchEvent(new panel.document.defaultView.Event('scroll'))
  assert.equal(thumb.style.transform, 'translateY(37.5px)')

  const wheel = new panel.document.defaultView.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: 25,
  })
  scrollbar.dispatchEvent(wheel)
  assert.equal(wheel.defaultPrevented, true)
  assert.equal(list.scrollTop, 175)
})

test('pastes the selected entry once and ignores later snapshots after admission', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('Enter', '8'))

  assert.deepEqual(panel.calls.paste, [{ id: 'text-1', routeSequence: '8' }])
  await panel.emitSnapshot(Object.freeze({
    revision: '11',
    entries: Object.freeze([
      Object.freeze({
        id: 'new-text',
        kind: 'text',
        capturedAt: '2026-08-30T06:01:00.000Z',
        textPreview: '不应在接纳后渲染',
      }),
    ]),
  }))
  assert.doesNotMatch(panel.document.body.textContent, /不应在接纳后渲染/)
})

test('keeps the panel visible and does not paste an unavailable file entry', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('Tab', '9'))
  await panel.hostKey(hostKey('Tab', '10'))
  assert.equal(activeFilter(panel), 'files')
  assert.equal(selectedEntryId(panel), 'files-1')
  await panel.hostKey(hostKey('ArrowDown', '11'))
  assert.equal(selectedEntryId(panel), 'files-missing')
  await panel.hostKey(hostKey('Enter', '12'))

  assert.deepEqual(panel.calls.paste, [])
  assert.match(panel.document.querySelector('#status')?.textContent ?? '', /文件已不存在/)
})

test('rejects stale snapshots and preserves a selected entry across newer snapshots', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('ArrowDown', '13'))
  assert.equal(selectedEntryId(panel), 'image-1')

  await panel.emitSnapshot(Object.freeze({
    revision: '9',
    entries: Object.freeze([
      Object.freeze({
        id: 'stale',
        kind: 'text',
        capturedAt: '2026-08-30T06:02:00.000Z',
        textPreview: '旧快照',
      }),
    ]),
  }))
  assert.equal(selectedEntryId(panel), 'image-1')
  assert.doesNotMatch(panel.document.body.textContent, /旧快照/)

  await panel.emitSnapshot(Object.freeze({
    revision: '11',
    entries: Object.freeze([
      Object.freeze({
        id: 'new-text',
        kind: 'text',
        capturedAt: '2026-08-30T06:03:00.000Z',
        textPreview: '新快照',
      }),
      snapshot.entries[1],
    ]),
  }))
  assert.equal(selectedEntryId(panel), 'image-1')

  await panel.emitSnapshot(Object.freeze({
    revision: '12',
    entries: Object.freeze([
      Object.freeze({
        id: 'newest-text',
        kind: 'text',
        capturedAt: '2026-08-30T06:04:00.000Z',
        textPreview: '最新快照',
      }),
    ]),
  }))
  assert.equal(selectedEntryId(panel), 'newest-text')
})

test('supports pointer selection, one-entry removal, and clear-all through the Host bridge', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.click('[data-filter="files"]')
  assert.equal(activeFilter(panel), 'files')
  assert.equal(selectedEntryId(panel), 'files-1')
  assert.equal(panel.calls.focusHostInput, 1)

  await panel.click('[data-entry-id="files-1"] .entry-select')
  assert.equal(selectedEntryId(panel), 'files-1')
  assert.equal(panel.calls.focusHostInput, 2)

  await panel.click('[data-entry-id="files-1"] .entry-remove')
  assert.deepEqual(panel.calls.remove, [{ id: 'files-1' }])
  assert.equal(panel.calls.focusHostInput, 3)

  await panel.click('#clear-history')
  assert.equal(panel.calls.clear, 1)
  assert.equal(panel.calls.focusHostInput, 4)
})

test('maps fixed paste errors to redacted Chinese status messages', async () => {
  const cases = [
    ['PermissionDenied', '没有剪贴板粘贴权限'],
    ['ExpiredPanelSession', '面板会话已失效'],
    ['RecordNotFound', '这条记录已不存在'],
    ['RecordUnavailable', '记录内容不可用'],
    ['PasteTargetUnavailable', '原窗口不可用于粘贴'],
    ['ClipboardWriteFailed', '无法写入系统剪贴板'],
  ]

  for (const [name, message] of cases) {
    const panel = await loadPanel({
      paste: async () => {
        const error = new Error('redacted')
        error.name = name
        throw error
      },
    })
    try {
      await panel.hostKey(hostKey('Enter', '14'))
      assert.equal(panel.document.querySelector('#status')?.textContent, message)
    } finally {
      panel.cleanup()
    }
  }
})

test('keeps Enter inert when the active filter is empty', async (t) => {
  const panel = await loadPanel({
    initialList: Promise.resolve(Object.freeze({ revision: '1', entries: Object.freeze([]) })),
  })
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('Enter', '15'))
  assert.deepEqual(panel.calls.paste, [])
})

test('renders tab counts, semantic capture times, and a category-specific empty state', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  assert.equal(panel.document.querySelector('[data-filter="all"] .filter-count')?.textContent, '4')
  assert.equal(panel.document.querySelector('[data-filter="image"] .filter-count')?.textContent, '1')
  assert.equal(panel.document.querySelector('[data-filter="files"] .filter-count')?.textContent, '2')
  assert.equal(panel.document.querySelector('[data-filter="text"] .filter-count')?.textContent, '1')
  assert.equal(panel.document.querySelectorAll('.entry-time').length, 4)
  assert.equal(panel.document.querySelector('[data-entry-id="text-1"] .entry-time')?.getAttribute('datetime'), '2026-08-30T06:00:00.000Z')

  await panel.emitSnapshot(Object.freeze({
    revision: '11',
    entries: Object.freeze([snapshot.entries[0]]),
  }))
  await panel.hostKey(hostKey('Tab', '16'))
  assert.equal(panel.document.querySelector('#empty-title')?.textContent, '暂无图片记录')
})

test('applies Host theme updates and reports an initial history load failure', async (t) => {
  const pendingList = deferred()
  const panel = await loadPanel({ initialList: pendingList.promise })
  t.after(panel.cleanup)

  await panel.update(Object.freeze({ theme: 'light' }))
  assert.equal(panel.document.documentElement.dataset.theme, 'light')

  const error = new Error('redacted')
  error.name = 'PermissionDenied'
  pendingList.reject(error)
  await panel.flush()
  assert.equal(panel.document.querySelector('#status')?.textContent, '无法读取剪贴板历史')
})

test('uses a stable responsive layout and contains no forbidden capability bypasses', async () => {
  const [css, source] = await Promise.all([
    readFile(panelCssUrl, 'utf8'),
    readFile(panelScriptUrl, 'utf8'),
  ])

  for (const required of [
    'grid-template-columns: 112px minmax(0, 1fr)',
    '-webkit-line-clamp: 2',
    'letter-spacing: 0',
    '@media (max-width: 520px)',
    '--uipilot-color-background',
    '--uipilot-color-accent',
  ]) {
    assert.match(css, new RegExp(required.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
  }

  for (const forbidden of [
    'navigator.clipboard',
    'invoke(',
    'fetch(',
    'WebSocket',
    'localStorage',
    'uipilotPluginWindow',
  ]) {
    assert.doesNotMatch(source, new RegExp(forbidden.replace('(', '\\(')))
  }
})

test('matches Host settings density without drawing an active tab edge', async () => {
  const css = await readFile(panelCssUrl, 'utf8')
  const activeTabRule = css.match(/\.filter-tab\[aria-selected='true'\]\s*\{([^}]*)\}/s)?.[1]

  assert.match(css, /grid-template-columns:\s*112px minmax\(0, 1fr\)/)
  assert.match(css, /\.filter-tab\s*\{[^}]*height:\s*40px;/s)
  assert.match(css, /\.history-item\s*\{[^}]*min-height:\s*54px;/s)
  assert.ok(activeTabRule)
  assert.match(activeTabRule, /color:\s*var\(--uipilot-color-accent/)
  assert.match(activeTabRule, /background:\s*transparent/)
  assert.doesNotMatch(activeTabRule, /border-(?:left|right)|box-shadow|outline/)
})
