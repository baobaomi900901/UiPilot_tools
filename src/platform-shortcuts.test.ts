// @vitest-environment jsdom
import { expect, it } from 'vitest'
import { isPrimaryShortcut, primaryShortcutLabel } from './platform-shortcuts'
import { isWindowPinShortcut } from './window-pin-shortcut'

it.each(['windows', 'macos'])('uses only the platform primary modifier on %s and rejects composing keys', platform => {
  const options = platform === 'macos' ? { metaKey: true } : { ctrlKey: true }
  const opposite = platform === 'macos' ? { ctrlKey: true } : { metaKey: true }
  const key = (extra: KeyboardEventInit = {}) => new KeyboardEvent('keydown', { key: 'k', ...options, ...extra })
  expect(isPrimaryShortcut(key(), 'K', platform)).toBe(true)
  expect(isPrimaryShortcut(new KeyboardEvent('keydown', { key: 'k', ...opposite }), 'k', platform)).toBe(false)
  for (const extra of [{ shiftKey: true }, { altKey: true }, { ctrlKey: true, metaKey: true }, { isComposing: true }, { keyCode: 229 }]) {
    expect(isPrimaryShortcut(key(extra), 'k', platform)).toBe(false)
  }
  expect(primaryShortcutLabel(platform, 'K')).toBe(platform === 'macos' ? '⌘+K' : 'Ctrl+K')
  expect(isWindowPinShortcut(key({ key: 't' }), platform)).toBe(true)
})
