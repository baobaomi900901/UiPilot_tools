// WK may finish composition before dispatching its confirmation keydown.
const active = new WeakSet<EventTarget>()
const ended = new WeakSet<EventTarget>()
export function isCompositionKey(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229 || !!event.target &&
    (active.has(event.target) || ended.has(event.target))
}
export function bindCompositionKeys(input: HTMLInputElement): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined
  const clear = () => { clearTimeout(timer); ended.delete(input) }
  const start = () => { clear(); active.add(input) }
  const end = () => {
    active.delete(input); ended.add(input)
    clearTimeout(timer); timer = setTimeout(clear, 0)
  }
  const release = () => clear()
  const blur = () => { clear(); active.delete(input) }
  const dispose = () => {
    blur(); input.removeEventListener('compositionstart', start)
    input.removeEventListener('compositionend', end); input.removeEventListener('keyup', release)
    input.removeEventListener('blur', blur)
  }
  try {
    input.addEventListener('compositionstart', start)
    input.addEventListener('compositionend', end)
    input.addEventListener('keyup', release)
    input.addEventListener('blur', blur)
  } catch (error) { dispose(); throw error }
  return dispose
}
