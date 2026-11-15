// These components own presentation and DOM events only; consumers own business state.
export function createList(container, { onActivate }) {
  const document = container.ownerDocument
  const nodes = new Map()
  const empty = document.createElement('p')
  empty.className = 'uipilot-ui-list-empty'
  let ids = [], focusedId = null, disabled = false, disposed = false
  let disabledFocusId = null
  container.classList.add('uipilot-ui-list')
  function rowFor(target) {
    const row = target?.closest?.('button')
    return row && nodes.get(row.dataset.itemId) === row ? row : null
  }
  function focus(id) { disabledFocusId = null; focusedId = id; nodes.get(id)?.focus() }
  function move(delta) {
    if (disposed || disabled || !ids.length || !delta) return
    const index = ids.indexOf(rowFor(document.activeElement)?.dataset.itemId ?? focusedId)
    const next = index < 0 ? (delta > 0 ? 0 : ids.length - 1) : Math.max(0, Math.min(ids.length - 1, index + delta))
    focus(ids[next])
  }
  function click(event) {
    const row = rowFor(event.target)
    if (!disposed && !disabled && row && !row.disabled) onActivate(row.dataset.itemId)
  }
  function keydown(event) {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || disabled || !rowFor(event.target)) return
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      move(event.key === 'ArrowDown' ? 1 : -1)
    }
    // Enter/Space use the native button click, so activation has a single path.
  }
  function focusin(event) { const row = rowFor(event.target); if (row) focusedId = row.dataset.itemId }
  container.addEventListener('click', click)
  container.addEventListener('keydown', keydown)
  container.addEventListener('focusin', focusin)
  return {
    move,
    update({ items, selectedId = null, disabled: nextDisabled = false, emptyText = '' }) {
      if (disposed) return
      const active = rowFor(document.activeElement)
      const documentFocused = document.hasFocus()
      const atDocumentRoot = document.activeElement === document.body || document.activeElement === document.documentElement
      // Native disabling can fix focus up to BODY. Remember only the row that
      // owned focus; never reclaim it from an editor, another list or the Host.
      if (!documentFocused || (!active && !atDocumentRoot)) disabledFocusId = null
      if (!disabled && nextDisabled && active && documentFocused) disabledFocusId = active.dataset.itemId
      const previousIndex = ids.indexOf(active?.dataset.itemId ?? focusedId)
      const nextIds = items.map((item) => item.id)
      const keep = new Set(nextIds)
      if (!keep.has(disabledFocusId)) disabledFocusId = null
      disabled = nextDisabled
      for (const [id, row] of nodes) {
        if (!keep.has(id)) { row.remove(); nodes.delete(id) }
      }
      empty.remove()
      items.forEach((item, index) => {
        let row = nodes.get(item.id)
        if (!row) {
          row = document.createElement('button')
          row.type = 'button'
          row.className = 'uipilot-ui-list-item'
          row.dataset.itemId = item.id
          nodes.set(item.id, row)
        }
        row.textContent = item.title
        row.title = item.title
        row.disabled = disabled
        row.setAttribute('aria-current', String(item.id === selectedId))
        if (container.children[index] !== row) container.insertBefore(row, container.children[index] ?? null)
      })
      ids = nextIds
      if (focusedId !== null && !keep.has(focusedId)) focusedId = ids[Math.min(Math.max(previousIndex, 0), ids.length - 1)] ?? null
      if (!disabled) {
        if (active && documentFocused && focusedId && document.activeElement !== nodes.get(focusedId)) focus(focusedId)
        else if (disabledFocusId && atDocumentRoot && documentFocused) focus(disabledFocusId)
        disabledFocusId = null
      }
      if (!ids.length) { empty.textContent = emptyText; container.append(empty) }
    },
    dispose() {
      disposed = true
      disabledFocusId = null
      container.removeEventListener('click', click)
      container.removeEventListener('keydown', keydown)
      container.removeEventListener('focusin', focusin)
    },
  }
}
export function createDetail({ empty, content, label }) {
  let disposed = false
  return {
    update({ hasContent, busy = false, text = '' }) {
      if (disposed) return
      empty.hidden = hasContent
      content.hidden = !hasContent
      content.setAttribute('aria-busy', String(busy))
      label.textContent = text
    },
    dispose() { disposed = true },
  }
}
export function createActions(entries) {
  let disposed = false
  const listeners = Object.entries(entries).map(([id, { button, onInvoke }]) => {
    const click = () => { if (!disposed && !button.disabled) onInvoke() }
    button.addEventListener('click', click)
    return { id, button, click }
  })
  return {
    update(state) {
      if (disposed) return
      for (const { id, button } of listeners) {
        if (state[id]) button.disabled = Boolean(state[id].disabled)
      }
    },
    dispose() {
      disposed = true
      for (const { button, click } of listeners) button.removeEventListener('click', click)
    },
  }
}
export { createForm } from './form.js'
export { createActionPanel } from './action-panel.js'
