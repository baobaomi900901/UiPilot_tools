// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { MacHotkeyStatusNotice, parseMacHotkeyStatus } from './macos-hotkey-status'
it('validates runtime status and does not treat persisted intent as active registration', () => {
  expect(parseMacHotkeyStatus({ status: 'fallback', configuredHotkey: 'DoubleMeta', effectiveHotkey: 'Alt+Space' })?.status).toBe('fallback')
  for (const row of [null, {}, { status: 'active', configuredHotkey: 'DoubleMeta', effectiveHotkey: null }, { status: 'unavailable', configuredHotkey: 'DoubleMeta', effectiveHotkey: 'Alt+Space' }]) expect(parseMacHotkeyStatus(row)).toBeNull()
})
it('shows fallback, refreshes on focus, and rejects a late earlier read', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const host = document.createElement('div'); const root = createRoot(host)
  let old!: (value: unknown) => void
  const read = vi.fn().mockResolvedValueOnce({ status: 'fallback', configuredHotkey: 'DoubleMeta', effectiveHotkey: 'Alt+Space' })
    .mockImplementationOnce(() => new Promise(resolve => { old = resolve }))
    .mockResolvedValueOnce({ status: 'active', configuredHotkey: 'Alt+Space', effectiveHotkey: 'Alt+Space' })
  try {
    await act(async () => root.render(<MacHotkeyStatusNotice read={read} revision={0} />))
    expect(host.textContent).toContain('双击 ⌘ 不可用，暂用')
    expect(host.textContent).toContain('Option')
    await act(async () => window.dispatchEvent(new Event('focus')))
    await act(async () => root.render(<MacHotkeyStatusNotice read={read} revision={1} />))
    expect(host.textContent).toContain('当前生效')
    await act(async () => old({ status: 'unavailable', configuredHotkey: 'DoubleMeta', effectiveHotkey: null }))
    expect(host.textContent).toContain('当前生效')
  } finally { await act(async () => root.unmount()) }
})
