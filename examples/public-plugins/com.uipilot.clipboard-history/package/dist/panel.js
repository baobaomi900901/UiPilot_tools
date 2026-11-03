import {
  compareRevisions,
  cycleFilter,
  filterEntries,
  moveSelection,
  reconcileSelection,
  shouldApplySnapshot,
} from './clipboard-history-logic.js'

const panel = window.uipilotPluginPanel
const tabs = [...document.querySelectorAll('[data-filter]')]
const historyCount = document.querySelector('#history-count')
const historyListShell = document.querySelector('.history-list-shell')
const historyList = document.querySelector('#history-list')
const historyListScrollbar = document.querySelector('#history-list-scrollbar')
const historyListScrollbarThumb = document.querySelector('#history-list-scrollbar-thumb')
const emptyState = document.querySelector('#empty-state')
const clearHistory = document.querySelector('#clear-history')

const COPY_ERROR_MESSAGES = Object.freeze({
  PermissionDenied: '剪贴板历史所需权限不可用，请在「设置 → 权限」中检查历史读取和历史复制权限。',
  ExpiredPanelSession: '面板会话已失效',
  RecordNotFound: '这条记录已不存在',
  RecordUnavailable: '记录内容不可用',
  ClipboardWriteFailed: '无法写入系统剪贴板',
})

let revision = null
let entries = []
let activeFilter = 'all'
let selectedId = null
let sessionLeaving = false
let sessionGeneration = 0
let selectionGeneration = 0
let sessionEpoch = null
let mutation = null
let copyOperation = null
let query = ''
let requestGeneration = 0
let observedRevision = null
let historyFacade = panel.clipboardHistory
let unsubscribeHistory = null

const EMPTY_TITLES = Object.freeze({
  all: '暂无剪贴板记录',
  image: '暂无图片记录',
  files: '暂无文件记录',
  text: '暂无文字记录',
})

function createVirtualScrollbar({ scrollable, surface, track, thumb }) {
  let updateScheduled = false
  let drag = null

  function update() {
    const scrollRange = scrollable.scrollHeight - scrollable.clientHeight
    const trackHeight = track.clientHeight
    if (scrollRange <= 1 || trackHeight <= 0) {
      surface.classList.remove('is-scrollable')
      thumb.style.height = ''
      thumb.style.transform = ''
      return
    }

    const thumbHeight = Math.max(24, trackHeight * scrollable.clientHeight / scrollable.scrollHeight)
    const thumbRange = Math.max(0, trackHeight - thumbHeight)
    const thumbTop = thumbRange * scrollable.scrollTop / scrollRange
    thumb.style.height = `${thumbHeight}px`
    thumb.style.transform = `translateY(${thumbTop}px)`
    surface.classList.add('is-scrollable')
  }

  function schedule() {
    if (updateScheduled) return
    updateScheduled = true
    window.requestAnimationFrame(() => {
      updateScheduled = false
      update()
    })
  }

  scrollable.addEventListener('scroll', schedule, { passive: true })
  track.addEventListener('wheel', (event) => {
    event.preventDefault()
    scrollable.scrollTop += event.deltaY
  }, { passive: false })
  track.addEventListener('pointerdown', (event) => {
    if (event.target === thumb) return
    event.preventDefault()
    const trackRect = track.getBoundingClientRect()
    const thumbRect = thumb.getBoundingClientRect()
    const thumbRange = Math.max(0, trackRect.height - thumbRect.height)
    const scrollRange = Math.max(0, scrollable.scrollHeight - scrollable.clientHeight)
    if (thumbRange === 0 || scrollRange === 0) return
    const thumbTop = Math.min(
      thumbRange,
      Math.max(0, event.clientY - trackRect.top - thumbRect.height / 2),
    )
    scrollable.scrollTop = scrollRange * thumbTop / thumbRange
  })
  thumb.addEventListener('pointerdown', (event) => {
    event.preventDefault()
    drag = {
      pointerId: event.pointerId,
      startY: event.clientY,
      startScrollTop: scrollable.scrollTop,
    }
    thumb.classList.add('is-dragging')
    thumb.setPointerCapture?.(event.pointerId)
  })
  thumb.addEventListener('pointermove', (event) => {
    if (!drag || drag.pointerId !== event.pointerId) return
    const thumbRange = track.clientHeight - thumb.clientHeight
    const scrollRange = scrollable.scrollHeight - scrollable.clientHeight
    if (thumbRange <= 0 || scrollRange <= 0) return
    scrollable.scrollTop = drag.startScrollTop
      + (event.clientY - drag.startY) * scrollRange / thumbRange
  })

  function finishDrag(event) {
    if (!drag || drag.pointerId !== event.pointerId) return
    drag = null
    thumb.classList.remove('is-dragging')
    if (event.type !== 'lostpointercapture' && thumb.hasPointerCapture?.(event.pointerId)) {
      thumb.releasePointerCapture(event.pointerId)
    }
  }

  thumb.addEventListener('pointerup', finishDrag)
  thumb.addEventListener('pointercancel', finishDrag)
  thumb.addEventListener('lostpointercapture', finishDrag)
  return Object.freeze({ schedule })
}

const historyListVirtualScrollbar = createVirtualScrollbar({
  scrollable: historyList,
  surface: historyListShell,
  track: historyListScrollbar,
  thumb: historyListScrollbarThumb,
})

function setStatus(message, tone = 'info', publish = panel.setStatus) {
  void publish({ message, tone }).catch(() => {})
}

function focusHostInput() {
  void panel.focusHostInput().catch(() => {})
}

async function mutateHistory(id = null) {
  if (sessionLeaving || mutation || copyOperation || (id !== null && !entries.some(entry => entry.id === id))) return
  const operation = { generation: sessionGeneration }
  const publish = panel.setStatus
  const history = panel.clipboardHistory
  mutation = operation
  setStatus('')
  render()
  try {
    if (id === null) await history.clear()
    else await history.remove({ id })
    if (sessionGeneration === operation.generation && !sessionLeaving) setStatus(id === null ? '已清空' : '已删除', 'success', publish)
  } catch {
    if (sessionGeneration === operation.generation && !sessionLeaving) setStatus(id === null ? '无法清空剪贴板历史' : '无法删除这条记录', 'error', publish)
  } finally {
    if (mutation === operation) mutation = null
    if (sessionGeneration === operation.generation && !sessionLeaving) {
      render()
    }
  }
}

function relativeTime(capturedAt) {
  const captured = Date.parse(capturedAt)
  if (!Number.isFinite(captured)) return ''
  const absoluteSeconds = Math.abs(Math.round((captured - Date.now()) / 1000))
  if (absoluteSeconds < 60) return '刚刚'
  if (absoluteSeconds < 3600) return `${Math.round(absoluteSeconds / 60)} 分钟前`
  if (absoluteSeconds < 86400) return `${Math.round(absoluteSeconds / 3600)} 小时前`
  return `${Math.round(absoluteSeconds / 86400)} 天前`
}

function entryTitle(entry) {
  if (entry.kind === 'text') return entry.textPreview
  if (entry.kind === 'image') return '图片'
  return entry.firstFileName
}

function entryMeta(entry) {
  if (entry.kind === 'image') return `${entry.width} × ${entry.height}`
  if (entry.kind === 'files') {
    return entry.available
      ? `${entry.fileCount} 个文件`
      : '文件不存在'
  }
  return '文字'
}

function createEntry(entry) {
  const item = document.createElement('div')
  item.className = 'history-item'
  item.dataset.entryId = entry.id
  item.dataset.unavailable = String(entry.kind === 'files' && !entry.available)
  item.setAttribute('role', 'option')
  item.setAttribute('aria-selected', String(entry.id === selectedId))

  const select = document.createElement('button')
  select.className = 'entry-select'
  select.type = 'button'
  select.tabIndex = -1
  select.setAttribute('aria-label', `选择${entryTitle(entry)}`)

  if (entry.kind === 'image') {
    const preview = document.createElement('img')
    preview.className = 'entry-image'
    preview.src = entry.previewDataUrl
    preview.alt = ''
    select.append(preview)
  } else {
    const icon = document.createElement('span')
    icon.className = `entry-icon entry-icon-${entry.kind}`
    icon.setAttribute('aria-hidden', 'true')
    select.append(icon)
  }

  const copy = document.createElement('div')
  copy.className = 'entry-copy'
  const title = document.createElement('p')
  title.className = 'entry-title'
  title.textContent = entryTitle(entry)
  const meta = document.createElement('p')
  meta.className = 'entry-meta'
  meta.textContent = entryMeta(entry)
  const time = document.createElement('time')
  time.className = 'entry-time'
  time.dateTime = entry.capturedAt
  time.textContent = relativeTime(entry.capturedAt)
  time.title = new Date(entry.capturedAt).toLocaleString('zh-CN')
  const details = document.createElement('div')
  details.className = 'entry-details'
  details.append(meta, time)
  copy.append(title, details)
  select.append(copy)
  select.addEventListener('click', () => {
    if (sessionLeaving) return
    selectionGeneration += 1
    selectedId = entry.id
    setStatus('')
    render()
    focusSelectedEntry()
  })

  const remove = document.createElement('button')
  remove.className = 'entry-remove icon-button'
  remove.type = 'button'
  remove.tabIndex = -1
  remove.title = '删除这条记录'
  remove.setAttribute('aria-label', `删除${entryTitle(entry)}`)
  remove.innerHTML = '<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"></path><path d="M8 6V4h8v2"></path><path d="M19 6l-1 14H6L5 6"></path></svg>'
  remove.disabled = mutation !== null
  remove.addEventListener('click', () => {
    void mutateHistory(entry.id)
    focusHostInput()
  })

  item.append(select, remove)
  return item
}

function focusSelectedEntry() {
  const target = historyList.querySelector('[aria-selected="true"] .entry-select')
  if (target) target.focus({ preventScroll: true })
  else focusHostInput()
}

function render(preserveFocus = true) {
  const restoreRecordFocus = preserveFocus && !!document.activeElement?.closest?.('.entry-select')
  const visibleEntries = filterEntries(entries, activeFilter)
  const nextSelectedId = reconcileSelection(visibleEntries, selectedId)
  if (nextSelectedId !== selectedId) selectionGeneration += 1
  selectedId = nextSelectedId
  for (const tab of tabs) {
    tab.setAttribute('aria-selected', String(tab.dataset.filter === activeFilter))
    tab.querySelector('.filter-count').textContent = String(filterEntries(entries, tab.dataset.filter).length)
  }

  historyList.replaceChildren(...visibleEntries.map(createEntry))
  historyCount.textContent = `${visibleEntries.length} 条记录`
  clearHistory.disabled = entries.length === 0 || mutation !== null
  document.querySelector('#empty-title').textContent = EMPTY_TITLES[activeFilter]
  emptyState.hidden = visibleEntries.length !== 0
  revealSelectedEntry()
  historyListVirtualScrollbar.schedule()
  if (restoreRecordFocus) focusSelectedEntry()
}

// scrollIntoView also scrolls overflow-hidden ancestors and can hide the header.
function revealSelectedEntry() {
  const selected = historyList.querySelector('[aria-selected="true"]')
  if (!selected) return
  const viewport = historyList.getBoundingClientRect()
  const item = selected.getBoundingClientRect()
  if (viewport.height <= 0) return
  if (item.top < viewport.top || item.height > viewport.height) {
    historyList.scrollTop += item.top - viewport.top
  } else if (item.bottom > viewport.bottom) {
    historyList.scrollTop += item.bottom - viewport.bottom
  }
}

function setFilter(nextFilter) {
  if (sessionLeaving) return
  selectionGeneration += 1
  activeFilter = nextFilter
  selectedId = null
  setStatus('')
  render()
}

function moveVisibleSelection(direction) {
  selectionGeneration += 1
  const visibleEntries = filterEntries(entries, activeFilter)
  selectedId = moveSelection(visibleEntries, selectedId, direction)
  setStatus('')
  render()
}

function selectedEntry() {
  return entries.find((entry) => entry.id === selectedId) ?? null
}

async function copySelection() {
  if (mutation || copyOperation || sessionLeaving) return
  const generation = sessionGeneration
  const selection = selectionGeneration
  const publish = panel.setStatus
  const entry = selectedEntry()
  if (!entry) return
  if (entry.kind === 'files' && !entry.available) {
    setStatus('文件已不存在', 'error')
    return
  }
  const operation = {}
  copyOperation = operation
  try {
    await panel.clipboardHistory.copy({ id: entry.id })
    if (generation === sessionGeneration && selection === selectionGeneration) setStatus('已复制到剪贴板', 'success', publish)
  } catch (error) {
    if (generation === sessionGeneration && selection === selectionGeneration) setStatus(COPY_ERROR_MESSAGES[error?.name] ?? '复制失败，请重试', 'error', publish)
  } finally {
    if (copyOperation === operation) copyOperation = null
  }
}

function applySnapshot(snapshot) {
  if (sessionLeaving) return
  if (!shouldApplySnapshot(revision, snapshot.revision)) return
  revision = snapshot.revision
  entries = [...snapshot.entries]
  render()
}

panel.onHostKey(async event => {
  if (sessionLeaving || (sessionEpoch !== null && event.sessionEpoch !== sessionEpoch)) return
  if (event.key === 'Tab') {
    setFilter(cycleFilter(activeFilter, event.shiftKey ? -1 : 1))
    // Content routing focuses the WebView before delivery. Give it a real target.
    if (selectedId !== null) focusSelectedEntry()
    else focusHostInput()
  } else if (event.key === 'ArrowDown') {
    focusSelectedEntry()
  } else if (event.key === 'ArrowUp') {
    moveVisibleSelection(-1)
    focusSelectedEntry()
  } else if (event.key === 'Delete' && !event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey && selectedId !== null) {
    await mutateHistory(selectedId)
  }
})
panel.onActions(({ source }) => {
  const generation = sessionGeneration
  const selection = selectionGeneration
  const request = requestGeneration
  const id = selectedId
  const current = () => generation === sessionGeneration && selection === selectionGeneration && request === requestGeneration && !sessionLeaving
  const guarded = run => () => { if (current()) return run() }
  const unavailable = mutation || copyOperation ? '正在处理，请稍候' : id === null ? '没有选中的记录' : undefined
  const changeCategory = direction => {
    setFilter(cycleFilter(activeFilter, direction))
    if (source === 'content') focusSelectedEntry()
    else focusHostInput()
  }
  return {
    context: source === 'hostInput' ? [{
      id: 'enter-list', title: '进入记录列表', shortcut: '↓', icon: 'arrow',
      disabledReason: id === null ? '没有可用记录' : undefined,
      run: guarded(focusSelectedEntry),
    }] : [
      { id: 'copy-entry', title: '复制到剪贴板', shortcut: 'Enter', icon: 'copy', disabledReason: unavailable, run: guarded(copySelection) },
      { id: 'previous-entry', title: '上一条', shortcut: '↑', icon: 'arrow', disabledReason: unavailable, run: guarded(() => { moveVisibleSelection(-1); focusSelectedEntry() }) },
      { id: 'next-entry', title: '下一条', shortcut: '↓', icon: 'arrow', disabledReason: unavailable, run: guarded(() => { moveVisibleSelection(1); focusSelectedEntry() }) },
      { id: 'delete-entry', title: '删除当前记录', shortcut: 'Delete', icon: 'trash', disabledReason: unavailable,
        run: guarded(() => id !== null ? mutateHistory(id) : undefined) },
    ],
    global: [
      { id: 'next-category', title: '下一分类', shortcut: 'Tab', icon: 'arrow-right-to-line', run: guarded(() => changeCategory(1)) },
      { id: 'previous-category', title: '上一分类', shortcut: 'Shift+Tab', icon: 'arrow-left-to-line', run: guarded(() => changeCategory(-1)) },
    ],
  }
})
panel.onUpdate(update => {
  document.documentElement.dataset.theme = update.theme
  sessionGeneration += 1
  if (sessionEpoch !== update.sessionEpoch) { revision = null; observedRevision = null }
  sessionEpoch = update.sessionEpoch
  query = typeof update.input === 'string' ? update.input : ''
  entries = []
  selectedId = null
  selectionGeneration += 1
  render(false)
  mutation = null
  copyOperation = null
  sessionLeaving = false
  setStatus('')
  bindHistory()
  refreshHistory()
})
function bindHistory() {
  unsubscribeHistory?.()
  historyFacade = panel.clipboardHistory
  const generation = sessionGeneration
  unsubscribeHistory = historyFacade.onChanged(snapshot => {
    if (generation !== sessionGeneration) return
    if (observedRevision !== null && compareRevisions(snapshot.revision, observedRevision) < 0) return
    observedRevision = snapshot.revision
    requestGeneration += 1
    if (query.trim()) refreshHistory()
    else applySnapshot(snapshot)
  })
}
bindHistory()
window.addEventListener('resize', historyListVirtualScrollbar.schedule)
function refreshHistory() {
  const generation = sessionGeneration
  const request = ++requestGeneration
  const publish = panel.setStatus
  void historyFacade.list({ query }).then(snapshot => {
    if (generation !== sessionGeneration || request !== requestGeneration || sessionLeaving) return
    if (observedRevision !== null && compareRevisions(snapshot.revision, observedRevision) < 0) return
    revision = snapshot.revision
    observedRevision = snapshot.revision
    entries = [...snapshot.entries]
    render()
  }).catch(error => {
    if (sessionLeaving || generation !== sessionGeneration || request !== requestGeneration) return
    historyCount.textContent = '读取失败'
    setStatus(error?.name === 'PermissionDenied'
      ? '剪贴板历史读取权限不可用，请在「设置 → 权限」中检查。'
      : ['InvalidClipboardHistoryRequest', 'InvalidQuery'].includes(error?.name) ? '查询不能超过 512 个字符' : '无法读取剪贴板历史', 'error', publish)
    emptyState.hidden = false
  })
}
refreshHistory()

// Content focus can occur through accessibility or the shared action menu.
// Do not intercept editing, composition, menu-owned, or repeated Delete events.
document.addEventListener('keydown', event => {
  if (!event.isComposing && !event.repeat && !event.altKey && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); focusHostInput(); return }
  if (event.defaultPrevented || event.isComposing || event.altKey || sessionLeaving) return
  if (event.target instanceof window.Element && event.target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')) return
  if (event.ctrlKey || event.metaKey) return
  if (event.key === 'Tab') {
    event.preventDefault()
    setFilter(cycleFilter(activeFilter, event.shiftKey ? -1 : 1))
    focusSelectedEntry()
  } else if (!event.shiftKey && (event.key === 'ArrowDown' || event.key === 'ArrowUp') && event.target.closest?.('.entry-select')) {
    event.preventDefault()
    moveVisibleSelection(event.key === 'ArrowDown' ? 1 : -1)
    focusSelectedEntry()
  } else if (event.key === 'Enter' && !event.shiftKey && event.target.closest?.('.entry-select')) {
    event.preventDefault()
    if (!event.repeat) void copySelection()
  } else if (event.key === 'Delete' && !event.shiftKey && !event.repeat) {
    event.preventDefault()
    if (selectedId !== null) void mutateHistory(selectedId)
  }
})

for (const tab of tabs) {
  tab.addEventListener('click', () => {
    if (sessionLeaving) return
    setFilter(tab.dataset.filter)
    focusHostInput()
  })
}

clearHistory.addEventListener('click', () => {
  void mutateHistory()
  focusHostInput()
})
