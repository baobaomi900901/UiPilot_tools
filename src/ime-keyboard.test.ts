// @vitest-environment jsdom
import { expect, it, vi } from 'vitest'
import { bindCompositionKeys, isCompositionKey } from './ime-keyboard'
it('blocks WK confirmation after compositionend and keyCode 229, then admits a new Enter', () => {
  vi.useFakeTimers()
  const input = document.createElement('input')
  const dispose = bindCompositionKeys(input)
  input.dispatchEvent(new CompositionEvent('compositionstart'))
  input.dispatchEvent(new CompositionEvent('compositionend'))
  const press = (keyCode: number) => { const e = new KeyboardEvent('keydown', { key: 'Enter', keyCode }); input.dispatchEvent(e); return e }
  expect(isCompositionKey(press(13))).toBe(true)
  input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Enter' }))
  expect(isCompositionKey(press(13))).toBe(false)
  vi.runAllTimers()
  expect(isCompositionKey(press(229))).toBe(true)
  expect(isCompositionKey(press(13))).toBe(false)
  dispose(); vi.useRealTimers()
})
it('does not leave a mouse-confirmed composition swallowing the next Enter', () => {
  vi.useFakeTimers()
  const input = document.createElement('input'); const dispose = bindCompositionKeys(input)
  input.dispatchEvent(new CompositionEvent('compositionstart')); input.dispatchEvent(new CompositionEvent('compositionend'))
  vi.runAllTimers()
  const e = new KeyboardEvent('keydown', { key: 'Enter' }); input.dispatchEvent(e)
  expect(isCompositionKey(e)).toBe(false)
  dispose(); vi.useRealTimers()
})
