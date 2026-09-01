import {
  STORAGE_KEY,
  createId,
  filterNotes,
  normalizeNotes,
  sortNotes,
} from './notes-logic.js'

const ITEM_HEIGHT = 48
const LIST_OVERSCAN = 4

const newBtn = document.querySelector('#new-btn')
const noteListShell = document.querySelector('.note-list-shell')
const noteList = document.querySelector('#note-list')
const noteListScrollbar = document.querySelector('#note-list-scrollbar')
const noteListScrollbarThumb = document.querySelector('#note-list-scrollbar-thumb')
const listEmpty = document.querySelector('#list-empty')
const noteListVirtual = document.querySelector('#note-list-virtual')
const noteListSpacer = document.querySelector('#note-list-spacer')
const noteListViewport = document.querySelector('#note-list-viewport')
const emptyState = document.querySelector('#empty-state')
const editorPanel = document.querySelector('#editor-panel')
const editorTitle = document.querySelector('#editor-title')
const editorTime = document.querySelector('#editor-time')
const editorSaveState = document.querySelector('#editor-save-state')
const editorSurface = document.querySelector('.editor-surface')
const editorContent = document.querySelector('#editor-content')
const editorScrollbar = document.querySelector('#editor-scrollbar')
const editorScrollbarThumb = document.querySelector('#editor-scrollbar-thumb')
const copyBtn = document.querySelector('#editor-copy-btn')
const editorStatus = document.querySelector('#editor-status')
const noteActionsMenu = document.querySelector('#note-actions-menu')

const newDialog = document.querySelector('#new-dialog')
const newForm = document.querySelector('#new-form')
const newTitleInput = document.querySelector('#new-title-input')
const newSaveBtn = document.querySelector('#new-save-btn')
const newCancelBtn = document.querySelector('#new-cancel-btn')

const renameDialog = document.querySelector('#rename-dialog')
const renameForm = document.querySelector('#rename-form')
const renameTitleInput = document.querySelector('#rename-title-input')
const renameSaveBtn = document.querySelector('#rename-save-btn')
const renameCancelBtn = document.querySelector('#rename-cancel-btn')

const deleteDialog = document.querySelector('#delete-dialog')
const deleteForm = document.querySelector('#delete-form')
const deleteMessage = document.querySelector('#delete-message')
const deleteCancelBtn = document.querySelector('#delete-cancel-btn')

const unsavedDialog = document.querySelector('#unsaved-dialog')
const unsavedForm = document.querySelector('#unsaved-form')
const unsavedDiscardBtn = document.querySelector('#unsaved-discard-btn')
const unsavedCancelBtn = document.querySelector('#unsaved-cancel-btn')

let sessionToken = 0
let storage = null
let notes = []
let selectedId = null
let savedContent = ''
let searchQuery = ''
let pendingSelectId = null
let pendingDeleteId = null
let pendingRenameId = null
let menuNoteId = null
let menuOrigin = null
let unsavedResolver = null
let listScrollFrame = 0
let listHasFocus = false
let escapeHideInFlight = false

function isSessionError(error) {
  return (
    error?.code === 'ExpiredWindowSessionError' ||
    error?.message === 'ExpiredWindowSessionError'
  )
}

function getSelectedNote() {
  return notes.find((note) => note.id === selectedId) ?? null
}

function isDirty() {
  const note = getSelectedNote()
  if (!note) {
    return false
  }
  return editorContent.value !== savedContent
}

function formatNoteTime(createdAt) {
  const date = new Date(createdAt)
  if (Number.isNaN(date.getTime())) {
    return ''
  }
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(date)
}

function updateEditorMeta(note = getSelectedNote()) {
  editorTitle.textContent = note?.title ?? ''
  editorTime.textContent = note ? formatNoteTime(note.createdAt) : ''
  editorTime.dateTime = note?.createdAt ?? ''
  const dirty = Boolean(note) && isDirty()
  editorSaveState.classList.toggle('is-dirty', dirty)
  editorSaveState.querySelector('span').textContent = dirty ? '未保存' : '已保存'
}

function filteredNotes() {
  return filterNotes(notes, searchQuery)
}

function setStatus(message, tone = '') {
  editorStatus.textContent = message
  if (message && tone) {
    editorStatus.dataset.tone = tone
  } else {
    delete editorStatus.dataset.tone
  }
}

function clearStatusLater(message, delayMs = 2000) {
  setStatus(message, 'success')
  window.setTimeout(() => {
    if (editorStatus.textContent === message) {
      setStatus('')
    }
  }, delayMs)
}

function getVisibleIndex(noteId = selectedId) {
  if (!noteId) {
    return -1
  }
  return filteredNotes().findIndex((note) => note.id === noteId)
}

function scrollToVisibleIndex(index) {
  if (index < 0) {
    return
  }
  const top = index * ITEM_HEIGHT
  const bottom = top + ITEM_HEIGHT
  if (top < noteList.scrollTop) {
    noteList.scrollTop = top
  } else if (bottom > noteList.scrollTop + noteList.clientHeight) {
    noteList.scrollTop = bottom - noteList.clientHeight
  }
}

function updateListAria() {
  const activeIndex = getVisibleIndex()
  if (activeIndex < 0) {
    noteList.removeAttribute('aria-activedescendant')
    return
  }
  noteList.setAttribute('aria-activedescendant', `note-option-${filteredNotes()[activeIndex].id}`)
}

function createNoteListItem(note) {
  const item = document.createElement('li')
  item.className = 'note-item'
  item.id = `note-option-${note.id}`
  item.setAttribute('role', 'option')
  item.setAttribute('aria-selected', String(note.id === selectedId))

  const card = document.createElement('div')
  card.className = 'note-card'
  if (note.id === selectedId) {
    card.classList.add('is-active')
  }

  const selectBtn = document.createElement('button')
  selectBtn.type = 'button'
  selectBtn.className = 'note-select'
  selectBtn.tabIndex = -1
  selectBtn.dataset.noteId = note.id

  const title = document.createElement('span')
  title.className = 'note-title'
  title.textContent = note.title

  const preview = document.createElement('small')
  preview.className = 'note-preview'
  preview.textContent = note.content.trim().split(/\r?\n/, 1)[0] || '空白笔记'

  selectBtn.append(title, preview)

  const moreBtn = document.createElement('button')
  moreBtn.type = 'button'
  moreBtn.className = 'btn note-more'
  moreBtn.tabIndex = -1
  moreBtn.title = '更多'
  moreBtn.setAttribute('aria-label', `更多 ${note.title}`)
  moreBtn.setAttribute('aria-haspopup', 'menu')
  moreBtn.setAttribute('aria-expanded', 'false')
  moreBtn.dataset.noteId = note.id

  const moreIcon = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  moreIcon.setAttribute('aria-hidden', 'true')
  moreIcon.setAttribute('viewBox', '0 0 24 24')
  moreIcon.setAttribute('fill', 'currentColor')
  for (const cx of [5, 12, 19]) {
    const dot = document.createElementNS('http://www.w3.org/2000/svg', 'circle')
    dot.setAttribute('cx', String(cx))
    dot.setAttribute('cy', '12')
    dot.setAttribute('r', '1.7')
    moreIcon.append(dot)
  }
  moreBtn.append(moreIcon)

  card.append(selectBtn, moreBtn)
  item.append(card)
  return item
}

function renderVirtualList() {
  const visibleNotes = filteredNotes()

  if (visibleNotes.length === 0) {
    listEmpty.hidden = false
    noteListVirtual.hidden = true
    listEmpty.textContent = searchQuery.trim() ? '没有匹配的笔记' : '暂无笔记，按 Ctrl+N 新建'
    noteListViewport.replaceChildren()
    noteListSpacer.style.height = '0px'
    noteListViewport.style.transform = 'translateY(0px)'
    updateListAria()
    return
  }

  listEmpty.hidden = true
  noteListVirtual.hidden = false
  noteListSpacer.style.height = `${visibleNotes.length * ITEM_HEIGHT}px`

  const scrollTop = noteList.scrollTop
  const viewportHeight = noteList.clientHeight || 0
  const startIndex = Math.max(0, Math.floor(scrollTop / ITEM_HEIGHT) - LIST_OVERSCAN)
  const endIndex = Math.min(
    visibleNotes.length,
    Math.ceil((scrollTop + viewportHeight) / ITEM_HEIGHT) + LIST_OVERSCAN,
  )

  noteListViewport.style.transform = `translateY(${startIndex * ITEM_HEIGHT}px)`
  noteListViewport.replaceChildren()
  for (const note of visibleNotes.slice(startIndex, endIndex)) {
    noteListViewport.append(createNoteListItem(note))
  }
  updateListAria()
}

function scheduleVirtualListRender() {
  if (listScrollFrame) {
    return
  }
  listScrollFrame = window.requestAnimationFrame(() => {
    listScrollFrame = 0
    renderVirtualList()
  })
}

function createVirtualScrollbar({ scrollable, surface, track, thumb, hidden = () => false }) {
  let updateScheduled = false
  let drag = null

  function update() {
    const scrollRange = scrollable.scrollHeight - scrollable.clientHeight
    const trackHeight = track.clientHeight
    if (hidden() || scrollRange <= 1 || trackHeight <= 0) {
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
    if (updateScheduled) {
      return
    }
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
    if (event.target === thumb) {
      return
    }
    event.preventDefault()
    const trackRect = track.getBoundingClientRect()
    const thumbRect = thumb.getBoundingClientRect()
    const thumbRange = Math.max(0, trackRect.height - thumbRect.height)
    const scrollRange = Math.max(0, scrollable.scrollHeight - scrollable.clientHeight)
    if (thumbRange === 0 || scrollRange === 0) {
      return
    }
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
    if (!drag || drag.pointerId !== event.pointerId) {
      return
    }
    const thumbRange = track.clientHeight - thumb.clientHeight
    const scrollRange = scrollable.scrollHeight - scrollable.clientHeight
    if (thumbRange <= 0 || scrollRange <= 0) {
      return
    }
    scrollable.scrollTop = drag.startScrollTop
      + (event.clientY - drag.startY) * scrollRange / thumbRange
  })

  function finishDrag(event) {
    if (!drag || drag.pointerId !== event.pointerId) {
      return
    }
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

const noteListVirtualScrollbar = createVirtualScrollbar({
  scrollable: noteList,
  surface: noteListShell,
  track: noteListScrollbar,
  thumb: noteListScrollbarThumb,
})
const editorVirtualScrollbar = createVirtualScrollbar({
  scrollable: editorContent,
  surface: editorSurface,
  track: editorScrollbar,
  thumb: editorScrollbarThumb,
  hidden: () => editorPanel.hidden,
})

function renderList() {
  renderVirtualList()
  noteListVirtualScrollbar.schedule()
}

function renderEditor() {
  const note = getSelectedNote()
  if (!note) {
    emptyState.hidden = false
    editorPanel.hidden = true
    editorContent.value = ''
    savedContent = ''
    updateEditorMeta(null)
    setStatus('')
    editorVirtualScrollbar.schedule()
    return
  }

  emptyState.hidden = true
  editorPanel.hidden = false
  editorContent.value = note.content
  savedContent = note.content
  updateEditorMeta(note)
  setStatus('')
  editorVirtualScrollbar.schedule()
}

function render() {
  renderList()
  renderEditor()
}

async function persistNotes(token) {
  if (!storage) {
    return false
  }
  try {
    await storage.set(STORAGE_KEY, notes)
    return token === sessionToken
  } catch (error) {
    if (isSessionError(error)) {
      return false
    }
    setStatus('保存失败', 'error')
    return false
  }
}

async function loadNotes(token) {
  if (!storage) {
    notes = []
    selectedId = null
    render()
    return
  }
  try {
    const stored = await storage.get(STORAGE_KEY)
    if (token !== sessionToken) {
      return
    }
    notes = normalizeNotes(stored)
    if (selectedId && !notes.some((note) => note.id === selectedId)) {
      selectedId = null
    }
    render()
  } catch (error) {
    if (isSessionError(error)) {
      return
    }
    notes = []
    selectedId = null
    render()
    setStatus('读取笔记失败', 'error')
  }
}

function closeNoteActionsMenu({ restoreFocus = false } = {}) {
  const origin = menuOrigin
  noteActionsMenu.hidden = true
  noteActionsMenu.style.left = ''
  noteActionsMenu.style.top = ''
  origin?.setAttribute('aria-expanded', 'false')
  menuNoteId = null
  menuOrigin = null
  if (restoreFocus && origin?.isConnected) {
    origin.focus()
  }
}

function openNoteActionsMenu(origin, noteId) {
  if (!noteActionsMenu.hidden && menuNoteId === noteId) {
    closeNoteActionsMenu({ restoreFocus: true })
    return
  }
  closeNoteActionsMenu()
  const note = notes.find((item) => item.id === noteId)
  if (!note) {
    return
  }

  menuNoteId = noteId
  menuOrigin = origin
  origin.setAttribute('aria-expanded', 'true')
  noteActionsMenu.hidden = false
  const originRect = origin.getBoundingClientRect()
  const menuWidth = noteActionsMenu.offsetWidth || 120
  const menuHeight = noteActionsMenu.offsetHeight || 112
  const left = Math.min(
    Math.max(8, originRect.right - menuWidth),
    Math.max(8, window.innerWidth - menuWidth - 8),
  )
  const below = originRect.bottom + 4
  const top = below + menuHeight <= window.innerHeight - 8
    ? below
    : Math.max(8, originRect.top - menuHeight - 4)
  noteActionsMenu.style.left = `${left}px`
  noteActionsMenu.style.top = `${top}px`
  noteActionsMenu.querySelector('[role="menuitem"]')?.focus()
}

function openRenameDialog(noteId) {
  const note = notes.find((item) => item.id === noteId)
  if (!note) {
    return
  }
  pendingRenameId = noteId
  renameTitleInput.value = note.title
  renameSaveBtn.setAttribute('aria-disabled', 'false')
  renameDialog.showModal()
  renameTitleInput.focus()
  renameTitleInput.select()
}

function closeRenameDialog() {
  pendingRenameId = null
  renameDialog.close()
  renameTitleInput.value = ''
  renameSaveBtn.setAttribute('aria-disabled', 'true')
}

function openNewDialog() {
  closeNoteActionsMenu()
  newTitleInput.value = ''
  newSaveBtn.setAttribute('aria-disabled', 'true')
  newDialog.showModal()
  newTitleInput.focus()
}

function closeNewDialog() {
  newDialog.close()
  newTitleInput.value = ''
  newSaveBtn.setAttribute('aria-disabled', 'true')
}

async function createNote(title) {
  const trimmedTitle = title.trim()
  if (!trimmedTitle) {
    return
  }
  const token = sessionToken
  const note = {
    id: createId(),
    title: trimmedTitle,
    content: '',
    createdAt: new Date().toISOString(),
  }
  notes = sortNotes([note, ...notes])
  selectedId = note.id
  savedContent = ''
  noteList.scrollTop = 0
  render()
  const persisted = await persistNotes(token)
  if (!persisted) {
    return
  }
  editorContent.focus()
  clearStatusLater('已创建')
}

async function renameNote(noteId, title) {
  const trimmedTitle = title.trim()
  if (!trimmedTitle || !notes.some((note) => note.id === noteId)) {
    return false
  }
  const token = sessionToken
  notes = notes.map((note) => note.id === noteId ? { ...note, title: trimmedTitle } : note)
  renderList()
  const persisted = await persistNotes(token)
  if (!persisted) {
    return false
  }
  clearStatusLater('已重命名')
  focusNoteList()
  return true
}

function nextPinnedAt() {
  const latestPinnedAt = notes.reduce((latest, note) => {
    const timestamp = typeof note.pinnedAt === 'string' ? Date.parse(note.pinnedAt) : Number.NaN
    return Number.isFinite(timestamp) ? Math.max(latest, timestamp) : latest
  }, 0)
  return new Date(Math.max(Date.now(), latestPinnedAt + 1)).toISOString()
}

async function pinNote(noteId) {
  if (!notes.some((note) => note.id === noteId)) {
    return false
  }
  const token = sessionToken
  const pinnedAt = nextPinnedAt()
  notes = sortNotes(notes.map((note) => note.id === noteId ? { ...note, pinnedAt } : note))
  noteList.scrollTop = 0
  renderList()
  const persisted = await persistNotes(token)
  if (!persisted) {
    return false
  }
  clearStatusLater('已置顶')
  focusNoteList()
  return true
}

function requestUnsavedDecision() {
  return new Promise((resolve) => {
    unsavedResolver = resolve
    unsavedDialog.showModal()
  })
}

async function selectNote(noteId, { focusEditor = true } = {}) {
  if (noteId === selectedId) {
    return
  }
  if (isDirty()) {
    pendingSelectId = noteId
    const decision = await requestUnsavedDecision()
    pendingSelectId = null
    if (decision === 'cancel') {
      return
    }
    if (decision === 'save') {
      const saved = await saveCurrentNote()
      if (!saved) {
        return
      }
    }
  }
  selectedId = noteId
  render()
  scrollToVisibleIndex(getVisibleIndex(noteId))
  if (focusEditor) {
    editorContent.focus()
  }
}

async function saveCurrentNote() {
  const note = getSelectedNote()
  if (!note) {
    return true
  }
  const token = sessionToken
  const nextContent = editorContent.value
  notes = notes.map((item) =>
    item.id === note.id ? { ...item, content: nextContent } : item,
  )
  savedContent = nextContent
  render()
  const persisted = await persistNotes(token)
  if (!persisted) {
    return false
  }
  clearStatusLater('已保存')
  focusNoteList()
  return true
}

function openDeleteDialog(noteId) {
  closeNoteActionsMenu()
  const note = notes.find((item) => item.id === noteId)
  if (!note) {
    return
  }
  pendingDeleteId = noteId
  deleteMessage.textContent = `确定删除「${note.title}」吗？此操作不可恢复。`
  deleteDialog.showModal()
}

async function deleteNote(noteId) {
  const token = sessionToken
  const deletingSelected = selectedId === noteId
  notes = notes.filter((item) => item.id !== noteId)
  if (deletingSelected) {
    selectedId = null
    savedContent = ''
  }
  render()
  const persisted = await persistNotes(token)
  if (!persisted) {
    return
  }
  clearStatusLater('已删除')
  focusNoteList()
}

function hidePanelAfterListCopy() {
  void window.uipilotPluginPanel.requestHide().catch(() => {
    setStatus('复制成功，但无法隐藏窗口', 'error')
  })
}

function copyEditorContent({ onSuccess, preferSync = false } = {}) {
  const note = getSelectedNote()
  if (!note) {
    return false
  }
  const text = editorContent.value

  function finishCopy(success) {
    if (success) {
      clearStatusLater('已复制')
      onSuccess?.()
    } else {
      setStatus('复制失败', 'error')
    }
    return success
  }

  function copyWithExecCommand() {
    const helper = document.createElement('textarea')
    helper.value = text
    helper.setAttribute('readonly', 'true')
    helper.style.position = 'fixed'
    helper.style.left = '-9999px'
    document.body.append(helper)
    helper.select()
    const copied = document.execCommand('copy')
    helper.remove()
    return copied
  }

  try {
    if (preferSync) {
      return finishCopy(copyWithExecCommand())
    }
    if (window.navigator.clipboard?.writeText) {
      void window.navigator.clipboard.writeText(text).then(
        () => finishCopy(true),
        () => finishCopy(copyWithExecCommand()),
      )
      return true
    }
    return finishCopy(copyWithExecCommand())
  } catch {
    return finishCopy(false)
  }
}

function isDialogOpen() {
  return !noteActionsMenu.hidden || newDialog.open || renameDialog.open || deleteDialog.open || unsavedDialog.open
}

function isFocusInList(target) {
  if (!(target instanceof HTMLElement)) {
    return false
  }
  if (target === noteList) {
    return true
  }
  return noteList.contains(target)
}

function isListFocused() {
  const active = document.activeElement
  if (!(active instanceof HTMLElement)) {
    return false
  }
  return active === noteList || noteList.contains(active)
}

function isListInteractionContext() {
  if (isDialogOpen() || !getSelectedNote()) {
    return false
  }
  if (isListFocused() || listHasFocus) {
    return true
  }
  const active = document.activeElement
  if (!(active instanceof HTMLElement)) {
    return false
  }
  if (isFocusInEditor(active)) {
    return false
  }
  return noteList.contains(active)
}

function isEnterKey(event) {
  return event.key === 'Enter' || event.key === 'NumpadEnter'
}

function tryCopyFromList(event) {
  if (!isEnterKey(event)) {
    return false
  }
  if (!isListInteractionContext()) {
    return false
  }
  event.preventDefault()
  event.stopPropagation()
  focusNoteList()
  if (copyEditorContent({ preferSync: true })) {
    hidePanelAfterListCopy()
  }
  return true
}

function isFocusInEditor(target) {
  return target === editorContent
}

function focusNoteList() {
  listHasFocus = true
  noteList.focus()
}

function shouldHandleVerticalArrowKeys(target) {
  if (isDialogOpen()) {
    return false
  }
  if (!(target instanceof HTMLElement)) {
    return true
  }
  return !isFocusInEditor(target)
}

async function moveListSelection(delta, { focusList = true } = {}) {
  const visibleNotes = filteredNotes()
  if (visibleNotes.length === 0) {
    return
  }

  const currentIndex = getVisibleIndex()
  let nextIndex = currentIndex
  if (currentIndex === -1) {
    nextIndex = delta > 0 ? 0 : visibleNotes.length - 1
  } else {
    nextIndex = Math.max(0, Math.min(visibleNotes.length - 1, currentIndex + delta))
  }

  const nextNote = visibleNotes[nextIndex]
  if (!nextNote) {
    return
  }

  listHasFocus = true
  if (nextNote.id !== selectedId) {
    await selectNote(nextNote.id, { focusEditor: false })
  } else {
    renderList()
    scrollToVisibleIndex(nextIndex)
  }
  if (focusList) {
    focusNoteList()
  }
}

async function handleNavigationKeys(event) {
  const target = event.target instanceof HTMLElement ? event.target : null
  if (!target || isDialogOpen()) {
    return
  }

  if (event.key === 'ArrowRight') {
    if (!isFocusInList(target)) {
      return
    }
    if (!getSelectedNote() || editorPanel.hidden) {
      return
    }
    event.preventDefault()
    editorContent.focus()
    return
  }

  if (event.key === 'ArrowLeft') {
    if (isFocusInEditor(target)) {
      return
    }
    event.preventDefault()
    focusNoteList()
    return
  }

  if (isEnterKey(event)) {
    return
  }

  if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') {
    return
  }
  if (!shouldHandleVerticalArrowKeys(target)) {
    return
  }

  const visibleNotes = filteredNotes()
  if (visibleNotes.length === 0) {
    return
  }

  event.preventDefault()
  await moveListSelection(event.key === 'ArrowDown' ? 1 : -1, { focusList: true })
}

function handleShortcut(event) {
  const modifier = event.ctrlKey || event.metaKey
  if (!modifier) {
    return
  }

  const key = event.key.toLowerCase()
  if (key === 's') {
    if (isDialogOpen() || !isFocusInEditor(event.target)) {
      return
    }
    event.preventDefault()
    void saveCurrentNote()
    return
  }
  if (key === 'n') {
    if (isDialogOpen()) {
      return
    }
    event.preventDefault()
    openNewDialog()
  }
}

function closeOrdinaryDialogsOnEscape() {
  if (!noteActionsMenu.hidden) {
    closeNoteActionsMenu({ restoreFocus: true })
    return true
  }
  if (newDialog.open) {
    closeNewDialog()
    return true
  }
  if (renameDialog.open) {
    closeRenameDialog()
    return true
  }
  if (deleteDialog.open) {
    pendingDeleteId = null
    deleteDialog.close()
    return true
  }
  if (unsavedDialog.open) {
    unsavedDialog.close()
    unsavedResolver?.('cancel')
    unsavedResolver = null
    pendingSelectId = null
    escapeHideInFlight = false
    return true
  }
  return false
}

async function finishEscapeUnsaved(decision) {
  if (decision === 'cancel') {
    escapeHideInFlight = false
    return
  }
  if (decision === 'save') {
    const saved = await saveCurrentNote()
    if (!saved) {
      escapeHideInFlight = false
      return
    }
  }
  escapeHideInFlight = false
  void window.uipilotPluginPanel.requestHide().catch(() => {
    setStatus('无法隐藏窗口', 'error')
  })
}

function handleEscapeKeydown(event) {
  if (event.key !== 'Escape' || event.isComposing) {
    return
  }

  if (closeOrdinaryDialogsOnEscape()) {
    event.preventDefault()
    return
  }

  if (isDirty()) {
    event.preventDefault()
    if (escapeHideInFlight) {
      return
    }
    escapeHideInFlight = true
    void requestUnsavedDecision().then((decision) => finishEscapeUnsaved(decision))
    return
  }
}

function handleHostKey(event) {
  if (isDialogOpen() || escapeHideInFlight) {
    return
  }

  if (event.key === 'n') {
    openNewDialog()
    return
  }

  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    // Fire-and-forget: never await dialogs inside the Host ack window.
    void moveListSelection(event.key === 'ArrowDown' ? 1 : -1, { focusList: true })
  }
}

window.uipilotPluginPanel.onHostKey(handleHostKey)

window.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
    event.preventDefault()
    void window.uipilotPluginPanel.focusHostInput()
  }
})

document.addEventListener('keydown', handleEscapeKeydown)
document.addEventListener('keydown', handleShortcut)
document.addEventListener(
  'keydown',
  (event) => {
    if (tryCopyFromList(event)) {
      return
    }
    void handleNavigationKeys(event)
  },
  true,
)

noteList.addEventListener('scroll', () => {
  scheduleVirtualListRender()
  closeNoteActionsMenu()
}, { passive: true })

noteList.addEventListener('focus', () => {
  listHasFocus = true
  renderList()
})

noteList.addEventListener('keydown', (event) => {
  tryCopyFromList(event)
}, true)

noteList.addEventListener('blur', (event) => {
  if (event.relatedTarget instanceof Node && noteList.contains(event.relatedTarget)) {
    return
  }
  listHasFocus = false
})

window.addEventListener('resize', scheduleVirtualListRender)
window.addEventListener('resize', noteListVirtualScrollbar.schedule)
window.addEventListener('resize', editorVirtualScrollbar.schedule)
window.addEventListener('resize', closeNoteActionsMenu)

editorContent.addEventListener('input', () => {
  editorVirtualScrollbar.schedule()
  updateEditorMeta()
})

newBtn.addEventListener('click', () => {
  if (!isDialogOpen()) {
    openNewDialog()
  }
})

newTitleInput.addEventListener('input', () => {
  newSaveBtn.setAttribute('aria-disabled', String(newTitleInput.value.trim().length === 0))
})

newCancelBtn.addEventListener('click', () => {
  closeNewDialog()
})

newForm.addEventListener('submit', async (event) => {
  event.preventDefault()
  const title = newTitleInput.value
  if (!title.trim()) {
    return
  }
  closeNewDialog()
  await createNote(title)
})

renameTitleInput.addEventListener('input', () => {
  renameSaveBtn.setAttribute('aria-disabled', String(renameTitleInput.value.trim().length === 0))
})

renameCancelBtn.addEventListener('click', () => {
  closeRenameDialog()
  focusNoteList()
})

renameForm.addEventListener('submit', async (event) => {
  event.preventDefault()
  const noteId = pendingRenameId
  const title = renameTitleInput.value
  if (!noteId || !title.trim()) {
    return
  }
  closeRenameDialog()
  await renameNote(noteId, title)
})

noteListViewport.addEventListener('click', async (event) => {
  const target = event.target
  if (!(target instanceof Element)) {
    return
  }
  const moreButton = target.closest('[data-note-id].note-more')
  if (moreButton instanceof HTMLButtonElement) {
    openNoteActionsMenu(moreButton, moreButton.dataset.noteId)
    return
  }
  const selectButton = target.closest('[data-note-id].note-select')
  if (selectButton instanceof HTMLButtonElement) {
    listHasFocus = true
    await selectNote(selectButton.dataset.noteId, { focusEditor: false })
    focusNoteList()
  }
})

noteActionsMenu.addEventListener('click', (event) => {
  const target = event.target
  if (!(target instanceof Element)) {
    return
  }
  const actionButton = target.closest('[data-note-action]')
  if (!(actionButton instanceof HTMLButtonElement) || !menuNoteId) {
    return
  }
  const noteId = menuNoteId
  const action = actionButton.dataset.noteAction
  closeNoteActionsMenu()
  if (action === 'rename') {
    openRenameDialog(noteId)
  } else if (action === 'pin') {
    void pinNote(noteId)
  } else if (action === 'delete') {
    openDeleteDialog(noteId)
  }
})

noteActionsMenu.addEventListener('keydown', (event) => {
  if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') {
    return
  }
  event.preventDefault()
  const items = [...noteActionsMenu.querySelectorAll('[role="menuitem"]')]
  const currentIndex = items.indexOf(document.activeElement)
  const delta = event.key === 'ArrowDown' ? 1 : -1
  const nextIndex = (currentIndex + delta + items.length) % items.length
  items[nextIndex]?.focus()
})

document.addEventListener('pointerdown', (event) => {
  if (noteActionsMenu.hidden || !(event.target instanceof Node)) {
    return
  }
  if (noteActionsMenu.contains(event.target)) {
    return
  }
  if (event.target instanceof Element && event.target.closest('.note-more')) {
    return
  }
  closeNoteActionsMenu()
}, true)

deleteCancelBtn.addEventListener('click', () => {
  pendingDeleteId = null
  deleteDialog.close()
})

deleteForm.addEventListener('submit', async (event) => {
  event.preventDefault()
  const noteId = pendingDeleteId
  pendingDeleteId = null
  deleteDialog.close()
  if (noteId) {
    await deleteNote(noteId)
  }
})

unsavedDiscardBtn.addEventListener('click', () => {
  unsavedDialog.close()
  unsavedResolver?.('discard')
  unsavedResolver = null
})

unsavedCancelBtn.addEventListener('click', () => {
  unsavedDialog.close()
  unsavedResolver?.('cancel')
  unsavedResolver = null
})

unsavedForm.addEventListener('submit', (event) => {
  event.preventDefault()
  unsavedDialog.close()
  unsavedResolver?.('save')
  unsavedResolver = null
})

copyBtn.addEventListener('click', () => {
  copyEditorContent()
})

window.uipilotPluginPanel.onUpdate(async (update) => {
  document.documentElement.dataset.theme = update.theme
  sessionToken += 1
  const token = sessionToken
  storage = window.uipilotPluginPanel.storage
  searchQuery = typeof update.input === 'string' ? update.input : ''
  await loadNotes(token)
})
