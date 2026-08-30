import {
  cycleFilter,
  filterEntries,
  moveSelection,
  reconcileSelection,
  shouldApplySnapshot,
} from './clipboard-history-logic.js'

const panel = window.uipilotPluginPanel
const tabs = [...document.querySelectorAll('[data-filter]')]
const historyCount = document.querySelector('#history-count')
const historyList = document.querySelector('#history-list')
const emptyState = document.querySelector('#empty-state')
const status = document.querySelector('#status')
const clearHistory = document.querySelector('#clear-history')

const PASTE_ERROR_MESSAGES = Object.freeze({
  PermissionDenied: '没有剪贴板粘贴权限',
  ExpiredPanelSession: '面板会话已失效',
  RecordNotFound: '这条记录已不存在',
  RecordUnavailable: '记录内容不可用',
  PasteTargetUnavailable: '原窗口不可用于粘贴',
  ClipboardWriteFailed: '无法写入系统剪贴板',
})

let revision = null
let entries = []
let activeFilter = 'all'
let selectedId = null
let sessionLeaving = false

const EMPTY_TITLES = Object.freeze({
  all: '暂无剪贴板记录',
  image: '暂无图片记录',
  files: '暂无文件记录',
  text: '暂无文字记录',
})

function setStatus(message, tone = '') {
  status.textContent = message
  if (tone) status.dataset.tone = tone
  else delete status.dataset.tone
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
    selectedId = entry.id
    setStatus('')
    render()
    void panel.focusHostInput()
  })

  const remove = document.createElement('button')
  remove.className = 'entry-remove icon-button'
  remove.type = 'button'
  remove.tabIndex = -1
  remove.title = '删除这条记录'
  remove.setAttribute('aria-label', `删除${entryTitle(entry)}`)
  remove.innerHTML = '<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"></path><path d="M8 6V4h8v2"></path><path d="M19 6l-1 14H6L5 6"></path></svg>'
  remove.addEventListener('click', async () => {
    if (sessionLeaving) return
    setStatus('')
    try {
      await panel.clipboardHistory.remove({ id: entry.id })
    } catch {
      setStatus('无法删除这条记录', 'error')
    } finally {
      await panel.focusHostInput()
    }
  })

  item.append(select, remove)
  return item
}

function render() {
  const visibleEntries = filterEntries(entries, activeFilter)
  selectedId = reconcileSelection(visibleEntries, selectedId)
  for (const tab of tabs) {
    tab.setAttribute('aria-selected', String(tab.dataset.filter === activeFilter))
    tab.querySelector('.filter-count').textContent = String(filterEntries(entries, tab.dataset.filter).length)
  }

  historyList.replaceChildren(...visibleEntries.map(createEntry))
  historyCount.textContent = `${visibleEntries.length} 条记录`
  clearHistory.disabled = entries.length === 0
  document.querySelector('#empty-title').textContent = EMPTY_TITLES[activeFilter]
  emptyState.hidden = visibleEntries.length !== 0
  historyList.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' })
}

function setFilter(nextFilter) {
  activeFilter = nextFilter
  selectedId = null
  setStatus('')
  render()
}

function moveVisibleSelection(direction) {
  const visibleEntries = filterEntries(entries, activeFilter)
  selectedId = moveSelection(visibleEntries, selectedId, direction)
  setStatus('')
  render()
}

function selectedEntry() {
  return entries.find((entry) => entry.id === selectedId) ?? null
}

async function pasteSelection(routeSequence) {
  const entry = selectedEntry()
  if (!entry) return
  if (entry.kind === 'files' && !entry.available) {
    setStatus('文件已不存在', 'error')
    return
  }

  try {
    const result = await panel.clipboardHistory.paste({ id: entry.id, routeSequence })
    if (result.outcome === 'admitted') sessionLeaving = true
  } catch (error) {
    setStatus(PASTE_ERROR_MESSAGES[error?.name] ?? '粘贴失败，请重试', 'error')
  }
}

function applySnapshot(snapshot) {
  if (sessionLeaving) return
  if (!shouldApplySnapshot(revision, snapshot.revision)) return
  revision = snapshot.revision
  entries = [...snapshot.entries]
  render()
}

panel.onHostKey(async (event) => {
  if (sessionLeaving) return
  if (event.key === 'Tab') {
    setFilter(cycleFilter(activeFilter, event.shiftKey ? -1 : 1))
    return
  }
  if (event.key === 'ArrowUp') {
    moveVisibleSelection(-1)
    return
  }
  if (event.key === 'ArrowDown') {
    moveVisibleSelection(1)
    return
  }
  if (event.key === 'Enter') {
    await pasteSelection(event.routeSequence)
  }
})
panel.onUpdate((update) => {
  document.documentElement.dataset.theme = update.theme
})
panel.clipboardHistory.onChanged(applySnapshot)
void panel.clipboardHistory.list().then(applySnapshot).catch(() => {
  if (sessionLeaving) return
  historyCount.textContent = '读取失败'
  setStatus('无法读取剪贴板历史', 'error')
  emptyState.hidden = false
})

for (const tab of tabs) {
  tab.addEventListener('click', () => {
    if (sessionLeaving) return
    setFilter(tab.dataset.filter)
    void panel.focusHostInput()
  })
}

clearHistory.addEventListener('click', async () => {
  if (sessionLeaving) return
  setStatus('')
  try {
    await panel.clipboardHistory.clear()
  } catch {
    setStatus('无法清空剪贴板历史', 'error')
  } finally {
    await panel.focusHostInput()
  }
})
