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
  copy = async () => ({ outcome: 'copied' }),
  list,
  scrollMetrics,
  remove = async () => {},
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
    status: [],
    order: [],
    copy: [],
    remove: [],
    list: [],
    clear: 0,
    focusHostInput: 0,
  }
  let actionProvider = null
  let hostKeyHandler = null
  let updateHandler = null
  let changedHandler = null

  const api = {
    onActions(provider) { actionProvider = provider; return () => {} },
    async setStatus(value) { calls.status.push(value) },
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
      list(input) {
        calls.list.push(input)
        if (list) return list(input)
        calls.order.push('list')
        return initialList
      },
      onChanged(handler) {
        calls.order.push('onChanged')
        changedHandler = handler
        return () => {}
      },
      async copy(input) {
        calls.copy.push(input)
        return copy(input)
      },
      async remove(input) {
        calls.remove.push(input)
        return remove(input)
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
    api,
    historyListener: () => changedHandler,
    actions: (source = 'content') => actionProvider?.({ source }),
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
  localKey(panel, 'ArrowDown')
  assert.equal(selectedEntryId(panel), 'image-1')
  localKey(panel, 'ArrowDown')
  localKey(panel, 'ArrowDown')
  localKey(panel, 'ArrowDown')
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

test('rejects stale snapshots and preserves a selected entry across newer snapshots', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('ArrowDown', '13'))
  localKey(panel, 'ArrowDown')
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
  assert.equal(panel.calls.focusHostInput, 1)

  await panel.click('[data-entry-id="files-1"] .entry-remove')
  assert.deepEqual(panel.calls.remove, [{ id: 'files-1' }])
  assert.equal(panel.calls.focusHostInput, 2)

  await panel.click('#clear-history')
  assert.equal(panel.calls.clear, 1)
  assert.equal(panel.calls.focusHostInput, 3)
})

test('keeps Enter inert when the active filter is empty', async (t) => {
  const panel = await loadPanel({
    initialList: Promise.resolve(Object.freeze({ revision: '1', entries: Object.freeze([]) })),
  })
  t.after(panel.cleanup)

  await panel.hostKey(hostKey('Enter', '15'))
  assert.deepEqual(panel.calls.copy, [])
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
  assert.equal(panel.calls.status.at(-1)?.message, '剪贴板历史读取权限不可用，请在「设置 → 权限」中检查。')
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
    '--uipilot-color-text',
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

test('matches Raycast panel density with a surface-based active tab', async () => {
  const css = await readFile(panelCssUrl, 'utf8')
  const activeTabRule = css.match(/\.filter-tab\[aria-selected='true'\]\s*\{([^}]*)\}/s)?.[1]

  assert.match(css, /grid-template-columns:\s*112px minmax\(0, 1fr\)/)
  assert.match(css, /\.filter-tab\s*\{[^}]*height:\s*36px;/s)
  assert.match(css, /\.history-item\s*\{[^}]*min-height:\s*55px;/s)
  assert.ok(activeTabRule)
  assert.match(activeTabRule, /color:\s*var\(--uipilot-color-text/)
  assert.match(activeTabRule, /background:\s*var\(--panel-card\)/)
  assert.doesNotMatch(activeTabRule, /border-(?:left|right)|box-shadow|outline/)
})

test('registers category and deletion actions and guards changed selection', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const actions = panel.actions()
  assert.ok(actions)
  const deletion = actions.context.find((action) => action.id === 'delete-entry')
  assert.equal(deletion.shortcut, 'Delete')
  assert.equal(deletion.disabledReason, undefined)
  await actions.global.find((action) => action.shortcut === 'Tab').run()
  assert.equal(activeFilter(panel), 'image')
  await deletion.run()
  assert.deepEqual(panel.calls.remove, [])
  await panel.actions().context.find((action) => action.id === 'delete-entry').run()
  assert.deepEqual(panel.calls.remove, [{ id: 'image-1' }])
  assert.equal(panel.document.querySelector('#status'), null)
  assert.equal(panel.calls.status.at(-1)?.message, '已删除')
})

test('Delete shares deletion with the action menu and blocks duplicate mutations', async (t) => {
  const pending = deferred()
  const panel = await loadPanel({ remove: () => pending.promise })
  t.after(panel.cleanup)
  const first = panel.hostKey(hostKey('Delete', '20'))
  await panel.hostKey(hostKey('Delete', '21'))
  assert.deepEqual(panel.calls.remove, [{ id: 'text-1' }])
  assert.ok(panel.actions().context[0].disabledReason)
  pending.reject(new Error('sensitive detail'))
  await first
  assert.equal(selectedEntryId(panel), 'text-1')
  assert.equal(panel.calls.status.at(-1)?.message, '无法删除这条记录')
})

test('empty list disables deletion and stale session callbacks stay inert', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const old = panel.actions().context[0]
  await panel.update({ sessionEpoch: '2', theme: 'dark' })
  await old.run()
  assert.deepEqual(panel.calls.remove, [])
  await panel.emitSnapshot({ revision: '20', entries: [] })
  assert.ok(panel.actions().context[0].disabledReason)
  await panel.hostKey(hostKey('Delete', '22'))
  assert.deepEqual(panel.calls.remove, [])
})

test('late deletion cannot publish feedback or steal focus from a newer session', async (t) => {
  const pending = deferred()
  const panel = await loadPanel({ remove: () => pending.promise })
  t.after(panel.cleanup)
  const task = panel.hostKey(hostKey('Delete', '23'))
  await panel.update({ sessionEpoch: '2', theme: 'light' })
  const statuses = panel.calls.status.length
  pending.reject(new Error('old failure'))
  await task
  assert.equal(panel.calls.status.length, statuses)
  assert.equal(panel.calls.focusHostInput, 0)
})

test('content Delete protects editors, composition, repeat and consumed menu events', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const win = panel.document.defaultView
  const input = panel.document.createElement('input')
  panel.document.body.append(input)
  input.dispatchEvent(new win.KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }))
  for (const flags of [{ repeat: true }, { isComposing: true }, { ctrlKey: true }, { shiftKey: true }, { metaKey: true }, { altKey: true }]) {
    panel.document.body.dispatchEvent(new win.KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true, ...flags }))
  }
  const consumed = new win.KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true })
  consumed.preventDefault()
  panel.document.body.dispatchEvent(consumed)
  await panel.flush()
  assert.deepEqual(panel.calls.remove, [])
  panel.document.body.dispatchEvent(new win.KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }))
  await panel.flush()
  assert.deepEqual(panel.calls.remove, [{ id: 'text-1' }])
})

test('a stale action stays inert after selecting away and back to the same record', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const old = panel.actions().context[0]
  await panel.hostKey(hostKey('ArrowDown', '30'))
  await panel.hostKey(hostKey('ArrowUp', '31'))
  await old.run()
  assert.deepEqual(panel.calls.remove, [])
})

test('delete completion does not steal focus if the user opens actions while it is pending', async (t) => {
  const pending = deferred()
  const panel = await loadPanel({ remove: () => pending.promise })
  t.after(panel.cleanup)
  const task = panel.hostKey(hostKey('Delete', '40'))
  const menu = panel.document.createElement('button')
  panel.document.body.append(menu)
  menu.focus()
  pending.resolve()
  await task
  assert.equal(panel.calls.focusHostInput, 0)
  assert.equal(panel.document.activeElement, menu)
})

test('stale category actions cannot change filter or request Host focus', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const action = panel.actions('content').global[0]
  await panel.update({ sessionEpoch: '2', theme: 'dark' })
  await action.run()
  assert.equal(activeFilter(panel), 'all')
  assert.equal(panel.calls.focusHostInput, 0)
})

test('selection reveal scrolls only the list, leaving the header and outer page fixed', async (t) => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const win = panel.document.defaultView
  const list = panel.document.querySelector('#history-list')
  let ancestorScrollCalls = 0
  win.HTMLElement.prototype.scrollIntoView = () => { ancestorScrollCalls += 1 }
  list.getBoundingClientRect = () => ({ top: 100, bottom: 220, height: 120 })
  const originalRect = win.HTMLElement.prototype.getBoundingClientRect
  win.HTMLElement.prototype.getBoundingClientRect = function () {
    if (this.classList.contains('history-item')) {
      const index = [...list.children].indexOf(this)
      const top = 100 + index * 60 - list.scrollTop
      return { top, bottom: top + 60, height: 60 }
    }
    return originalRect.call(this)
  }
  await panel.hostKey(hostKey('ArrowDown', '60'))
  localKey(panel, 'ArrowDown')
  localKey(panel, 'ArrowDown')
  assert.equal(list.scrollTop, 60)
  await panel.hostKey(hostKey('ArrowUp', '62'))
  await panel.hostKey(hostKey('ArrowUp', '63'))
  assert.equal(list.scrollTop, 0)
  assert.equal(ancestorScrollCalls, 0)
  assert.equal(panel.document.body.scrollTop, 0)
})

function localKey(panel, key, flags = {}) {
  const target = panel.document.activeElement
  target.dispatchEvent(new panel.document.defaultView.KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...flags }))
}

test('Host Enter submits query; ArrowDown enters selected record and local Enter copies without leaving', async t => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.hostKey(hostKey('Enter', '100'))
  assert.deepEqual(panel.calls.copy, [])
  await panel.hostKey(hostKey('ArrowDown', '101'))
  assert.equal(panel.document.activeElement.closest('[data-entry-id]').dataset.entryId, 'text-1')
  localKey(panel, 'ArrowDown')
  assert.equal(selectedEntryId(panel), 'image-1')
  localKey(panel, 'Enter')
  await panel.flush()
  assert.deepEqual(panel.calls.copy, [{ id: 'image-1' }])
  assert.equal(panel.calls.status.at(-1).message, '已复制到剪贴板')
  assert.equal(panel.document.activeElement.closest('[data-entry-id]').dataset.entryId, 'image-1')
})

test('queries accept equal revisions, discard old requests and requery pushed history', async t => {
  const pending = []
  const panel = await loadPanel({ list(input) { const item = deferred(); pending.push({ ...item, input }); return item.promise } })
  t.after(panel.cleanup)
  await panel.update({ input: 'old', sessionEpoch: '1', theme: 'dark' })
  await panel.update({ input: 'new', sessionEpoch: '1', theme: 'dark' })
  assert.equal(pending.at(-1).input.query, 'new')
  pending.at(-1).resolve({ revision: '10', entries: [snapshot.entries[1]] })
  await panel.flush()
  pending.at(-2).resolve(snapshot)
  await panel.flush()
  assert.equal(selectedEntryId(panel), 'image-1')
  await panel.emitSnapshot({ revision: '11', entries: snapshot.entries })
  assert.equal(selectedEntryId(panel), 'image-1')
  assert.equal(pending.at(-1).input.query, 'new')
  pending.at(-1).resolve({ revision: '11', entries: [] })
  await panel.flush()
  assert.equal(selectedEntryId(panel), null)
})

test('actions follow origin and stale query actions cannot copy', async t => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const input = panel.actions('hostInput')
  assert.ok(input.context.some(a => a.id === 'enter-list'))
  assert.ok(!input.context.some(a => a.id === 'copy-entry' || a.id === 'delete-entry'))
  const old = panel.actions('content').context.find(a => a.id === 'copy-entry')
  await panel.update({ input: 'query', sessionEpoch: '1', theme: 'dark' })
  await old.run()
  assert.deepEqual(panel.calls.copy, [])
})

test('copy blocks repeat, composition and concurrent writes; late completion cannot overwrite status', async t => {
  const pending = deferred()
  const panel = await loadPanel({ copy: () => pending.promise })
  t.after(panel.cleanup)
  await panel.hostKey(hostKey('ArrowDown', '1'))
  localKey(panel, 'Enter', { repeat: true })
  localKey(panel, 'Enter', { isComposing: true })
  assert.equal(panel.calls.copy.length, 0)
  localKey(panel, 'Enter')
  localKey(panel, 'Enter')
  assert.equal(panel.calls.copy.length, 1)
  await panel.update({ input: '', sessionEpoch: '2', theme: 'light' })
  const count = panel.calls.status.length
  pending.resolve({ outcome: 'copied' })
  await panel.flush()
  assert.equal(panel.calls.status.length, count)
})


test('copy errors remain redacted and unavailable file records never write', async t => {
  const panel = await loadPanel({ copy: async () => { const error = new Error('private path'); error.name = 'ClipboardWriteFailed'; throw error } })
  t.after(panel.cleanup)
  await panel.hostKey(hostKey('ArrowDown', '1'))
  localKey(panel, 'Enter')
  await panel.flush()
  assert.equal(panel.calls.status.at(-1).message, '无法写入系统剪贴板')
  await panel.click('[data-filter="files"]')
  await panel.hostKey(hostKey('ArrowDown', '2'))
  localKey(panel, 'ArrowDown')
  localKey(panel, 'Enter')
  await panel.flush()
  assert.equal(panel.calls.copy.length, 1)
  assert.equal(panel.calls.status.at(-1).message, '文件已不存在')
})

test('category menu actions restore useful focus and empty navigation returns to input', async t => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.actions('content').global[0].run()
  assert.equal(panel.document.activeElement.closest('[data-entry-id]').dataset.entryId, 'image-1')
  const before = panel.calls.focusHostInput
  await panel.actions('hostInput').global[0].run()
  assert.equal(panel.calls.focusHostInput, before + 1)
  await panel.emitSnapshot({ revision: '20', entries: [] })
  const empty = panel.calls.focusHostInput
  await panel.hostKey(hostKey('ArrowDown', '3'))
  assert.equal(panel.calls.focusHostInput, empty + 1)
})

test('content shortcuts respect menus and CtrlF accepts bootstrap prevention', async t => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  await panel.hostKey(hostKey('ArrowDown', '1'))
  const win = panel.document.defaultView
  const enter = new win.KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
  enter.preventDefault()
  panel.document.activeElement.dispatchEvent(enter)
  assert.deepEqual(panel.calls.copy, [])
  const event = new win.KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true, cancelable: true })
  event.preventDefault()
  panel.document.activeElement.dispatchEvent(event)
  assert.equal(panel.calls.focusHostInput, 1)
})


test('actual shared actions menu accepts both contexts and owns shortcuts while open', async t => {
  const { createContextActionsMenu } = await import('../../../../src/context-actions-ui.js')
  const { installPanelContextActions } = await import('../../../../src/panel-context-actions.js')
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const win = panel.document.defaultView
  const adapter = installPanelContextActions({
    document: panel.document, window: win, getEpoch: () => '1',
    invoke: async () => {}, createMenu: createContextActionsMenu,
  })
  adapter.register(({ source }) => panel.actions(source))
  win.__UIPILOT_PLUGIN_PANEL_ACTIONS__({ sessionEpoch: '1', source: 'hostInput', ticket: 'ticket' })
  let menu = panel.document.querySelector('.uipilot-context-actions')
  assert.ok(menu)
  assert.match(menu.textContent, /搜索/)
  assert.match(menu.textContent, /进入记录列表/)
  assert.doesNotMatch(menu.textContent, /复制到剪贴板/)
  adapter.reset()
  await panel.hostKey(hostKey('ArrowDown', '1'))
  win.__UIPILOT_PLUGIN_PANEL_ACTIONS__({ sessionEpoch: '1', source: 'content' })
  menu = panel.document.querySelector('.uipilot-context-actions')
  assert.ok(menu)
  assert.match(menu.textContent, /复制到剪贴板/)
  assert.match(menu.textContent, /返回主输入框/)
  const before = panel.calls.focusHostInput
  localKey(panel, 'f', { ctrlKey: true })
  assert.equal(panel.calls.focusHostInput, before)
  assert.ok(panel.document.querySelector('.uipilot-context-actions'))
  localKey(panel, 'ArrowDown')
  assert.equal(selectedEntryId(panel), 'text-1')
  assert.deepEqual(panel.calls.copy, [])
  adapter.reset()
})


test('standalone preview input CtrlK supplies Host search and submits its local query', async () => {
  const { createContextActionsMenu } = await import('../../../../src/context-actions-ui.js')
  const { installPanelContextActions } = await import('../../../../src/panel-context-actions.js')
  const source = await readFile(new URL('../preview.js', import.meta.url), 'utf8')
  const bridge = source.slice(source.indexOf('function installBridge('), source.indexOf('async function loadPanel('))
  const dom = new JSDOM('<input id="preview-query"><button id="preview-operations">操作</button>', { url: 'https://panel.uipilot.invalid/' })
  try {
    const win = dom.window
    win.requestAnimationFrame = callback => { callback(); return 1 }
    const install = new Function('window', 'document', 'installPanelContextActions', 'createContextActionsMenu', 'minutesAgo', 'clone', 'createPreviewImage', `${bridge}; return installBridge`)(
      win, win.document, installPanelContextActions, createContextActionsMenu,
      () => new Date().toISOString(), structuredClone, () => 'data:image/png;base64,iVBORw0KGgo=',
    )
    install('dark')
    let update
    win.uipilotPluginPanel.onUpdate(value => { update = value })
    win.uipilotPluginPanel.onActions(({ source }) => ({ context: [{ id: 'origin', title: source, run() {} }] }))
    await Promise.resolve()
    const input = win.document.querySelector('#preview-query')
    input.value = '产品'
    input.focus()
    input.dispatchEvent(new win.KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }))
    const menu = win.document.querySelector('.uipilot-context-actions')
    assert.ok(menu)
    assert.match(menu.textContent, /hostInput/)
    assert.match(menu.textContent, /搜索/)
    win.document.activeElement.dispatchEvent(new win.KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await Promise.resolve()
    await Promise.resolve()
    assert.equal(update.input, '产品')
    assert.equal(win.document.activeElement, input)
    assert.equal(win.document.querySelector('.uipilot-context-actions'), null)
  } finally { dom.window.close() }
})


test('rebinds history polling facade on a new session and ignores old callbacks', async t => {
  const panel = await loadPanel()
  t.after(panel.cleanup)
  const oldCallback = panel.historyListener()
  let listener
  let unsubscribed = false
  const requested = []
  panel.api.clipboardHistory = {
    ...panel.api.clipboardHistory,
    list: async ({ query }) => { requested.push(query); return { revision: '10', entries: [snapshot.entries[1]] } },
    onChanged(handler) { listener = handler; return () => { unsubscribed = true } },
  }
  await panel.update({ input: 'new query', sessionEpoch: '2', theme: 'dark' })
  await panel.flush()
  assert.equal(selectedEntryId(panel), 'image-1')
  oldCallback({ revision: '90', entries: snapshot.entries })
  await panel.flush()
  assert.equal(selectedEntryId(panel), 'image-1')
  assert.equal(requested.length, 1)
  listener({ revision: '11', entries: snapshot.entries })
  await panel.flush()
  assert.deepEqual(requested, ['new query', 'new query'])
  await panel.update({ input: 'third', sessionEpoch: '3', theme: 'light' })
  assert.equal(unsubscribed, true)
})
