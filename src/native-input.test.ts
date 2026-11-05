import { describe, expect, it, vi } from 'vitest'
import { bindNativeImagePaste } from './native-input'

describe('trusted launcher image paste', () => {
  function setup() {
    const listeners = new Map<string, EventListener>()
    const input = { addEventListener: vi.fn((type, callback) => listeners.set(type, callback)), removeEventListener: vi.fn() } as unknown as HTMLInputElement
    const receive = vi.fn()
    const unbind = bindNativeImagePaste(input, receive)
    const fire = (options: Record<string, unknown>) => {
      const event = { isTrusted: true, target: input, preventDefault: vi.fn(), clipboardData: { items: [], getData: () => '' }, ...options }
      listeners.get('paste')!(event as unknown as Event)
      return event
    }
    return { input, receive, unbind, fire }
  }
  it('accepts an event bitmap and prevents inserting its filename', () => {
    const { receive, fire } = setup()
    const file = new File(['png'], 'clipboard.png', { type: 'image/png' })
    const event = fire({ clipboardData: { items: [{ kind: 'file', type: 'image/png', getAsFile: () => file }], getData: () => '' } })
    expect(receive).toHaveBeenCalledExactlyOnceWith(file)
    expect(event.preventDefault).toHaveBeenCalledOnce()
  })
  it('ignores synthetic paste, text paste and non-image files', () => {
    const { receive, fire } = setup()
    fire({ isTrusted: false })
    const text = fire({ clipboardData: { items: [], getData: () => 'ordinary text' } })
    const file = fire({ clipboardData: { items: [{ kind: 'file', type: 'text/plain' }], getData: () => '' } })
    expect(receive).not.toHaveBeenCalled()
    expect(text.preventDefault).not.toHaveBeenCalled()
    expect(file.preventDefault).not.toHaveBeenCalled()
  })
  it('requests the version-bound native fallback only for an explicit empty paste', () => {
    const { receive, fire } = setup()
    fire({})
    expect(receive).toHaveBeenCalledExactlyOnceWith()
  })
})
