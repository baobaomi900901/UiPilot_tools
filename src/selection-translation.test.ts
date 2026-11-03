// @vitest-environment jsdom
import { expect, it, vi } from 'vitest'
import { createLauncherCore } from './launcher-core'
import { parseLauncherShown, type LauncherClient } from './protocol'
const entry = { pluginId: 'com.uipilot.translate', commandId: 'default', hotkey: 'Alt+A', bindingRevision: '1' }
const resolved = { pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'renamed', outputMode: 'mainResult', activationMode: 'submit', inputRequired: true, inputPlaceholder: null }
function fixture(delayed = false) {
  let resolve!: (value: unknown) => void
  const searchApps = vi.fn<LauncherClient['searchApps']>(async () => null)
  const core = createLauncherCore({ searchApps, resolvePublicPluginCommandEntry: delayed ? () => new Promise((accept) => { resolve = accept }) : async () => resolved,
    getMessageSummary: async () => ({ revision: '0', unreadCount: 0 }) } as unknown as LauncherClient)
  const show = (selectionTranslation: unknown) => core.shown({ invocationId: 'selection', target: 'launcher', notice: null, commandEntry: entry, selectionTranslation })
  return { core, show, searchApps, resolve: () => resolve(resolved) }
}
it('submits selected Unicode and multiline text once with stable identity', async () => {
  const f = fixture()
  try {
    f.show({ id: 'one', text: ' 你好\nworld ' })
    await vi.waitFor(() => expect(f.searchApps).toHaveBeenCalledWith(expect.objectContaining({ query: '/renamed  你好\nworld ', completionOrigin: expect.objectContaining({ pluginId: entry.pluginId, phase: 'commit' }) })))
    const count = f.searchApps.mock.calls.length
    f.show({ id: 'one', text: 'again' })
    await Promise.resolve()
    expect(f.searchApps).toHaveBeenCalledTimes(count)
  } finally { f.core.destroy() }
})
it('does not submit capture errors or whitespace', async () => {
  const f = fixture()
  try {
    f.show({ id: 'error', error: '未选择文字' })
    expect(f.core.getSnapshot().status).toBe('未选择文字')
    f.show({ id: 'empty', text: '  ' })
    await Promise.resolve()
    expect(f.searchApps).not.toHaveBeenCalled()
  } finally { f.core.destroy() }
})
it('keeps capture failures visible in the translation input after deferred searches settle', async () => {
  vi.useFakeTimers()
  const f = fixture()
  try {
    f.searchApps.mockResolvedValue({ requestId: 'default-search', items: [], replaceLocalResults: true })
    f.show({ id: 'unsupported', error: '无法读取当前应用的选中文字，请手动输入' })
    await vi.advanceTimersByTimeAsync(500)
    expect(f.core.getSnapshot().status).toBe('无法读取当前应用的选中文字，请手动输入')
    expect(f.core.getSnapshot().mainResultCommand).toEqual(expect.objectContaining({ pluginId: entry.pluginId, commandId: entry.commandId }))
    expect(f.searchApps).not.toHaveBeenCalled()
  } finally { f.core.destroy(); vi.useRealTimers() }
})
it('preserves a capture failure without a resolvable command entry', async () => {
  vi.useFakeTimers()
  const f = fixture()
  try {
    f.searchApps.mockResolvedValue({ requestId: 'default-search', items: [], replaceLocalResults: true })
    f.core.shown({ invocationId: 'failure', target: 'launcher', notice: null, selectionTranslation: { id: 'no-entry', error: '读取选中文字超时，请重试' } })
    await vi.advanceTimersByTimeAsync(500)
    expect(f.core.getSnapshot().status).toBe('读取选中文字超时，请重试')
    expect(f.searchApps).not.toHaveBeenCalled()
  } finally { f.core.destroy(); vi.useRealTimers() }
})
it('accepts manual input after a capture failure', async () => {
  vi.useFakeTimers()
  const f = fixture()
  try {
    f.show({ id: 'error', error: '未选择文字' })
    await vi.advanceTimersByTimeAsync(500)
    const control = f.core.getSnapshot().mainResultCommand!.suffixControl
    f.core.text({ kind: 'ordinaryInput', control, value: 'manual text', inputType: 'insertText' })
    await vi.advanceTimersByTimeAsync(500)
    expect(f.core.getSnapshot().status).toBe('')
    expect(f.core.getSnapshot().mainResultCommand?.suffix).toBe('manual text')
    expect(f.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '/renamed manual text', stableCommandTarget: entry }))
  } finally { f.core.destroy(); vi.useRealTimers() }
})
it('does not replace new edits with a delayed failure entry', async () => {
  const f = fixture(true)
  try {
    f.show({ id: 'error', error: '未选择文字' })
    f.core.text({ kind: 'ordinaryInput', control: f.core.getSnapshot().queryControl, value: 'new query', inputType: 'insertText' })
    f.resolve(); await Promise.resolve()
    expect(f.core.getSnapshot().query).toBe('new query')
    expect(f.core.getSnapshot().mainResultCommand).toBeUndefined()
    expect(f.core.getSnapshot().status).not.toBe('未选择文字')
  } finally { f.core.destroy() }
})
it('does not overwrite edits while resolving', async () => {
  const f = fixture(true)
  try {
    f.show({ id: 'one', text: 'selection' })
    f.core.text({ kind: 'ordinaryInput', control: f.core.getSnapshot().queryControl, value: 'my query', inputType: 'insertText' })
    f.resolve(); await Promise.resolve()
    expect(f.core.getSnapshot().query).toBe('my query')
  } finally { f.core.destroy() }
})
it('rejects ambiguous capture outcomes', () => {
  expect(parseLauncherShown({ invocationId: '1', target: 'launcher', notice: null, commandEntry: entry, selectionTranslation: { id: '1', text: 'text', error: 'error' } })).toBeNull()
})
it.each(['hide', 'new show', 'composition'] as const)('retires a delayed selection resolve on %s', async (action) => {
  const f = fixture(true)
  try {
    f.show({ id: 'old', text: 'old selection' })
    if (action === 'hide') await f.core.requestHide()
    if (action === 'new show') f.core.shown({ invocationId: 'new', target: 'launcher', notice: null })
    if (action === 'composition') f.core.text({ kind: 'compositionStart', control: f.core.getSnapshot().queryControl })
    f.resolve(); await Promise.resolve()
    expect(f.core.getSnapshot().mainResultCommand).toBeUndefined()
    expect(f.searchApps).not.toHaveBeenCalled()
  } finally { f.core.destroy() }
})
it.each(['x'.repeat(8193), 'hello\0world'])('does not submit invalid captured text', async (text) => {
  const f = fixture()
  try { f.show({ id: 'invalid', text }); await Promise.resolve(); expect(f.searchApps).not.toHaveBeenCalled() }
  finally { f.core.destroy() }
})
