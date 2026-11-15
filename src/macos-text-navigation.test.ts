// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { bindMacTextNavigation } from './macos-text-navigation'
let dispose: (() => void) | undefined
function field(value: string, position = value.length) {
  const input = document.createElement('input'); input.value = value
  document.body.append(input); input.focus(); input.setSelectionRange(position, position)
  dispose = bindMacTextNavigation(input)
  return input
}
function press(input: HTMLInputElement, key: string, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options })
  input.dispatchEvent(event); return event
}
afterEach(() => { dispose?.(); document.body.replaceChildren() })
describe('Mac single-line navigation', () => {
  it('moves inside a link and clamps at its edges without modifying or emitting text', () => {
    const input = field('https://example.test/?q={Query}')
    let changes = 0; input.addEventListener('input', () => changes++)
    const original = input.value
    expect(press(input, 'ArrowRight').defaultPrevented).toBe(true)
    expect(input.selectionStart).toBe(original.length)
    press(input, 'ArrowLeft'); expect(input.selectionStart).toBe(original.length - 1)
    press(input, 'ArrowUp'); expect(input.selectionStart).toBe(0)
    press(input, 'ArrowLeft'); expect(input.selectionStart).toBe(0)
    press(input, 'ArrowDown'); expect(input.selectionStart).toBe(original.length)
    expect(input.value).toBe(original); expect(changes).toBe(0)
  })
  it('moves by grapheme and extends or collapses directional selections', () => {
    const input = field('A🐱e\u0301中')
    press(input, 'ArrowLeft', { shiftKey: true })
    expect([input.selectionStart, input.selectionEnd, input.selectionDirection]).toEqual([5, 6, 'backward'])
    press(input, 'ArrowLeft', { shiftKey: true })
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 6])
    press(input, 'ArrowRight', { shiftKey: true })
    expect([input.selectionStart, input.selectionEnd]).toEqual([5, 6])
    press(input, 'ArrowLeft'); expect([input.selectionStart, input.selectionEnd]).toEqual([5, 5])
    press(input, 'ArrowLeft'); expect(input.selectionStart).toBe(3)
    press(input, 'ArrowLeft'); expect(input.selectionStart).toBe(1)
  })
  it('supports Command edges and Option words with selection', () => {
    const input = field('hello world')
    press(input, 'ArrowLeft', { altKey: true }); expect(input.selectionStart).toBe(6)
    press(input, 'ArrowLeft', { metaKey: true }); expect(input.selectionStart).toBe(0)
    press(input, 'ArrowRight', { altKey: true, shiftKey: true }); expect(input.selectionEnd).toBe(5)
    press(input, 'ArrowRight', { metaKey: true, shiftKey: true }); expect(input.selectionEnd).toBe(11)
  })
  it('leaves IME composition, control bindings and ordinary typing with the browser', () => {
    const input = field('中文')
    for (const options of [{ isComposing: true }, { keyCode: 229 }, { ctrlKey: true }])
      expect(press(input, 'ArrowLeft', options).defaultPrevented).toBe(false)
    input.dispatchEvent(new CompositionEvent('compositionstart'))
    expect(press(input, 'ArrowLeft').defaultPrevented).toBe(false)
    input.dispatchEvent(new CompositionEvent('compositionend'))
    expect(press(input, 'ArrowLeft').defaultPrevented).toBe(false)
    input.dispatchEvent(new KeyboardEvent('keyup'))
    expect(press(input, 'a').defaultPrevented).toBe(false)
    expect(press(input, 'ArrowLeft').defaultPrevented).toBe(true)
  })
  it('blocks only navigation fallback characters and detaches cleanly', () => {
    const input = field('link')
    press(input, '\uf702'); expect(input.selectionStart).toBe(3)
    const text = (data: string, inputType = 'insertText') => {
      const e = new InputEvent('beforeinput', { data, inputType, cancelable: true }); input.dispatchEvent(e); return e.defaultPrevented
    }
    expect(text('\uf702')).toBe(true)
    expect(text('中')).toBe(false)
    expect(text('\uf702', 'insertFromPaste')).toBe(false)
    input.dispatchEvent(new KeyboardEvent('keyup'))
    expect(text('\uf702')).toBe(false)
    dispose?.(); expect(press(input, 'ArrowLeft').defaultPrevented).toBe(false)
  })
})
