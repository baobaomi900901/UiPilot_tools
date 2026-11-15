export function createActionPanel({ dialog, trigger, list, closeButton, cancelButton, actions }) {
  const document = dialog.ownerDocument
  let disposed = false, previousFocus = null
  const entries = actions.map((action) => {
    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'uipilot-ui-action-panel-item'
    button.dataset.actionId = action.id
    button.textContent = action.label
    list.append(button)
    return { ...action, button }
  })
  const available = () => entries.filter(({ button }) => !button.disabled && !button.hidden)
  const otherModal = () => [...document.querySelectorAll('dialog[open]')].some((node) => node !== dialog)
  const validFocus = (node) => node?.isConnected && typeof node.focus === 'function' && !node.disabled && !node.closest('[hidden], dialog:not([open])')
  function open() {
    if (disposed || dialog.open || trigger.disabled || !validFocus(trigger) || otherModal()) return
    previousFocus = document.activeElement
    dialog.showModal()
    ;(available()[0]?.button ?? cancelButton).focus()
  }
  function close({ restoreFocus = true } = {}) {
    if (disposed || !dialog.open) return
    const target = previousFocus
    previousFocus = null
    const documentFocused = document.hasFocus()
    dialog.close()
    // Never dispatch business or restore focus from the queued native close event.
    if (restoreFocus && documentFocused && !otherModal()) {
      if (validFocus(target)) target.focus()
      else if (validFocus(trigger)) trigger.focus()
    }
  }
  function click(event) {
    if (disposed || !dialog.open) return
    const entry = entries.find(({ button }) => button === event.target.closest?.('button'))
    if (!entry || entry.button.disabled || entry.button.hidden) return
    close({ restoreFocus: false })
    entry.onInvoke()
  }
  function cancel(event) { event.preventDefault(); close() }
  function keydown(event) {
    if (disposed || !dialog.open || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    const enabled = available()
    const index = enabled.findIndex(({ button }) => button === document.activeElement)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? enabled.length - 1
      : index < 0 ? (event.key === 'ArrowDown' ? 0 : enabled.length - 1)
      : Math.max(0, Math.min(enabled.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
    event.preventDefault()
    ;(enabled[next]?.button ?? cancelButton).focus()
  }
  trigger.addEventListener('click', open)
  list.addEventListener('click', click)
  closeButton.addEventListener('click', cancel)
  cancelButton.addEventListener('click', cancel)
  dialog.addEventListener('cancel', cancel)
  dialog.addEventListener('keydown', keydown)
  return {
    open, close,
    update(state) {
      if (disposed) return
      const focused = entries.find(({ button }) => button === document.activeElement)
      for (const { id, button } of entries) {
        const value = state instanceof Map ? state.get(id) : Object.hasOwn(state, id) ? state[id] : undefined
        if (value) { button.disabled = Boolean(value.disabled); button.hidden = Boolean(value.hidden) }
      }
      if (dialog.open && focused && (focused.button.disabled || focused.button.hidden) && document.hasFocus()) {
        ;(available()[0]?.button ?? cancelButton).focus()
      }
    },
    dispose() {
      if (disposed) return
      close({ restoreFocus: false })
      disposed = true
      previousFocus = null
      trigger.removeEventListener('click', open)
      list.removeEventListener('click', click)
      closeButton.removeEventListener('click', cancel)
      cancelButton.removeEventListener('click', cancel)
      dialog.removeEventListener('cancel', cancel)
      dialog.removeEventListener('keydown', keydown)
    },
  }
}
