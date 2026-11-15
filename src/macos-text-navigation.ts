/// <reference lib="es2022.intl" />
import { bindCompositionKeys, isCompositionKey } from './ime-keyboard'

const arrows: Record<string, string> = {
  '\uf700': 'ArrowUp', '\uf701': 'ArrowDown', '\uf702': 'ArrowLeft', '\uf703': 'ArrowRight',
}

// Only used by Mac Quicklinks single-line fields. Own navigation without writing
// the value: WK/AppKit must not reinterpret a navigation key as printable text.
export function bindMacTextNavigation(input: HTMLInputElement): () => void {
  const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' })
  const words = new Intl.Segmenter(undefined, { granularity: 'word' })
  const unbindComposition = bindCompositionKeys(input)
  let navigating = false
  const clear = () => { navigating = false }
  const keydown = (event: KeyboardEvent) => {
    navigating = false
    const key = arrows[event.key] ?? event.key
    if (event.defaultPrevented || isCompositionKey(event) || event.ctrlKey ||
        !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(key) ||
        input.disabled || input.selectionStart === null || input.selectionEnd === null) return
    event.preventDefault()
    event.stopPropagation()
    navigating = true
    const value = input.value
    const start = input.selectionStart, end = input.selectionEnd
    const backward = input.selectionDirection === 'backward'
    const focus = backward ? start : end
    const anchor = backward ? end : start
    const left = key === 'ArrowLeft' || key === 'ArrowUp'
    const edge = event.metaKey || key === 'ArrowUp' || key === 'ArrowDown'
    let next: number
    if (edge) next = left ? 0 : value.length
    else if (event.altKey) {
      const segments = [...words.segment(value)].filter(part => part.isWordLike)
      const part = left ? segments.filter(part => part.index < focus).pop()
        : segments.find(part => part.index + part.segment.length > focus)
      next = part ? part.index + (left ? 0 : part.segment.length) : left ? 0 : value.length
    } else if (!event.shiftKey && start !== end) next = left ? start : end
    else {
      const boundaries = [0, ...[...graphemes.segment(value)].map(part => part.index + part.segment.length)]
      next = left ? boundaries.filter(index => index < focus).pop() ?? 0
        : boundaries.find(index => index > focus) ?? value.length
    }
    if (event.shiftKey) input.setSelectionRange(Math.min(anchor, next), Math.max(anchor, next), next < anchor ? 'backward' : 'forward')
    else input.setSelectionRange(next, next, 'none')
  }
  const beforeinput = (event: InputEvent) => {
    // A late native fallback may arrive after keydown. Never filter paste,
    // composition or ordinary text; only this navigation's function character.
    if (navigating && !event.isComposing && event.inputType === 'insertText' &&
        event.data !== null && /^[\uf700-\uf703]$/.test(event.data)) event.preventDefault()
  }
  input.addEventListener('keydown', keydown)
  input.addEventListener('beforeinput', beforeinput)
  input.addEventListener('keyup', clear)
  input.addEventListener('blur', clear)
  input.addEventListener('compositionstart', clear)
  return () => {
    unbindComposition()
    input.removeEventListener('keydown', keydown)
    input.removeEventListener('beforeinput', beforeinput)
    input.removeEventListener('keyup', clear)
    input.removeEventListener('blur', clear)
    input.removeEventListener('compositionstart', clear)
  }
}
