import {
  STORAGE_KEY,
  createId,
  filterNotes,
  normalizeNotes,
  sortNotes,
} from './notes-logic.js'

const ITEM_HEIGHT = 52
const LIST_OVERSCAN = 4

const newBtn = document.querySelector('#new-btn')
const noteList = document.querySelector('#note-list')
const listEmpty = document.querySelector('#list-empty')
const noteListVirtual = document.querySelector('#note-list-virtual')
const noteListSpacer = document.querySelector('#note-list-spacer')
const noteListViewport = document.querySelector('#note-list-viewport')
const emptyState = document.querySelector('#empty-state')
const editorPanel = document.querySelector('#editor-panel')
const editorContent = document.querySelector('#editor-content')
const copyBtn = document.querySelector('#copy-btn')
const saveBtn = document.querySelector('#save-btn')
const editorStatus = document.querySelector('#editor-status')

const newDialog = document.querySelector('#new-dialog')
const newForm = document.querySelector('#new-form')
const newTitleInput = document.querySelector('#new-title-input')
const newSaveBtn = document.querySelector('#new-save-btn')
const newCancelBtn = document.querySelector('#new-cancel-btn')

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

function filteredNotes() {
  return filterNotes(notes, searchQuery)
}

function setStatus(message) {
  editorStatus.textContent = message
}

function clearStatusLater(message, delayMs = 2000) {
  setStatus(message)
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

  selectBtn.append(title)

  const deleteBtn = document.createElement('button')
  deleteBtn.type = 'button'
  deleteBtn.className = 'btn note-delete'
  deleteBtn.tabIndex = -1
  deleteBtn.title = '删除'
  deleteBtn.setAttribute('aria-label', `删除 ${note.title}`)
  deleteBtn.textContent = '×'
  deleteBtn.dataset.noteId = note.id

  card.append(selectBtn, deleteBtn)
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

function renderList() {
  renderVirtualList()
}

function renderEditor() {
  const note = getSelectedNote()
  if (!note) {
    emptyState.hidden = false
    editorPanel.hidden = true
    editorContent.value = ''
    savedContent = ''
    setStatus('')
    return
  }

  emptyState.hidden = true
  editorPanel.hidden = false
  editorContent.value = note.content
  savedContent = note.content
  setStatus('')
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
    setStatus('保存失败')
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
    setStatus('读取笔记失败')
  }
}

function openNewDialog() {
  newTitleInput.value = ''
  newSaveBtn.disabled = true
  newDialog.showModal()
  newTitleInput.focus()
}

function closeNewDialog() {
  newDialog.close()
  newTitleInput.value = ''
  newSaveBtn.disabled = true
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
  return true
}

function openDeleteDialog(noteId) {
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
}

function hidePanelAfterListCopy() {
  void window.uipilotPluginPanel.requestHide().catch(() => {
    setStatus('复制成功，但无法隐藏窗口')
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
      setStatus('复制失败')
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
  return newDialog.open || deleteDialog.open || unsavedDialog.open
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

function isEnterKey(event) {
  return event.key === 'Enter' || event.key === 'NumpadEnter'
}

function tryCopyFromList(event) {
  if (!isEnterKey(event)) {
    return false
  }
  if (!isListFocused() && !listHasFocus) {
    return false
  }
  if (!getSelectedNote()) {
    return false
  }
  event.preventDefault()
  event.stopPropagation()
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
  if (key === 'n') {
    if (isDialogOpen()) {
      return
    }
    event.preventDefault()
    openNewDialog()
  }
}

function closeOrdinaryDialogsOnEscape() {
  if (newDialog.open) {
    closeNewDialog()
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
    setStatus('无法隐藏窗口')
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

noteList.addEventListener('scroll', scheduleVirtualListRender, { passive: true })

noteList.addEventListener('focus', () => {
  listHasFocus = true
  renderList()
})

noteList.addEventListener('blur', () => {
  listHasFocus = false
  renderList()
})

window.addEventListener('resize', scheduleVirtualListRender)

newBtn.addEventListener('click', () => {
  if (!isDialogOpen()) {
    openNewDialog()
  }
})

newTitleInput.addEventListener('input', () => {
  newSaveBtn.disabled = newTitleInput.value.trim().length === 0
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

noteListViewport.addEventListener('click', async (event) => {
  const target = event.target
  if (!(target instanceof Element)) {
    return
  }
  const deleteButton = target.closest('[data-note-id].note-delete')
  if (deleteButton instanceof HTMLButtonElement) {
    openDeleteDialog(deleteButton.dataset.noteId)
    return
  }
  const selectButton = target.closest('[data-note-id].note-select')
  if (selectButton instanceof HTMLButtonElement) {
    listHasFocus = true
    await selectNote(selectButton.dataset.noteId, { focusEditor: false })
    focusNoteList()
  }
})

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

saveBtn.addEventListener('click', async () => {
  await saveCurrentNote()
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
