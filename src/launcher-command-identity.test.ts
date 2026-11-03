// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { createLauncherCore, safeLauncherActivation } from './launcher-core'
import { parseU64Decimal, type BuiltinFeatureView, type LauncherClient, type LauncherResultActivation, type SearchResponse } from './protocol'

describe('duplicate launcher command identity', () => {
  it('accepts only the supported stable builtin activation', () => {
    const activation = { kind: 'builtinActivation', feature: 'webSearch', initialArgument: '' }
    expect(safeLauncherActivation(activation)).toEqual(activation)
    expect(safeLauncherActivation({ ...activation, feature: 'find' })).toBeUndefined()
    expect(safeLauncherActivation({ ...activation, initialArgument: '\n' })).toBeUndefined()
  })
  it.each(['windowActivation', 'panelActivation'] as const)('requires a stable command id for %s', (kind) => {
    const activation = { kind, pluginId: 'com.example.multi', commandId: 'second', initialArgument: 'hello', favorite: true,
      ...(kind === 'windowActivation' ? { commandLabel: 'shared' } : {}) }
    expect(safeLauncherActivation(activation)).toEqual(activation)
    const { commandId: _, ...missing } = activation
    expect(safeLauncherActivation(missing)).toBeUndefined()
    expect(safeLauncherActivation({ ...activation, commandId: '../other' })).toBeUndefined()
  })

  it.each(['mainResultActivation', 'windowActivation', 'panelActivation'] as const)('preserves selected command and arguments for %s', async (kind) => {
    const activation = { kind, pluginId: 'com.example.multi', commandId: 'second', initialArgument: 'hello', favorite: false,
      ...(kind === 'panelActivation' ? {} : { commandLabel: 'shared' }) } as Extract<LauncherResultActivation, { kind: 'mainResultActivation' | 'windowActivation' | 'panelActivation' }>
    const searchApps = vi.fn<LauncherClient['searchApps']>(async () => null)
    const listen = async () => () => {}
    const openPluginPanel = vi.fn<LauncherClient['openPluginPanel']>(async () => ({ sessionEpoch: parseU64Decimal('1')!, pluginId: 'com.example.multi', commandLabel: 'shared', hostKeys: [] }))
    const partial: Partial<LauncherClient> = {
      searchApps, openPluginPanel, listenShown: listen, listenHidden: listen,
      listenPluginPanelError: listen, listenPluginPanelReset: listen, listenScreenshotError: listen,
      listenPluginPanelFocusHostInput: listen, listenMessageStateChanged: listen,
      getMessageSummary: async () => ({ revision: '0', unreadCount: 0 }),
      loadSettings: async () => ({ hotkey: 'Alt+Space', autostart: false, theme: 'system', webSearchEngine: 'bing', filePreviewEnabled: true, screenshotHotkey: null, screenshotHotkeyStatus: 'unset', screenshotSaveDirectory: null, screenshotSaveDirectoryPath: null }),
      submitPluginPanel: async () => ({ sessionEpoch: parseU64Decimal('1')!, pluginId: 'com.example.multi', commandLabel: 'shared', hostKeys: [] }),
      closePluginPanel: async () => {},
    }
    const core = createLauncherCore(partial as LauncherClient)
    try {
      await core.start()
      core.shown({ invocationId: 'identity-test', target: 'launcher', notice: null })
      searchApps.mockResolvedValueOnce({ requestId: 'shared-options', items: [
        { resultId: 'first', title: 'First', activation: { ...activation, commandId: 'first' } },
        { resultId: 'second', title: 'Second', activation },
      ], replaceLocalResults: true } satisfies SearchResponse)
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/shared hello', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
      expect(openPluginPanel).not.toHaveBeenCalled()
      core.activateResult(1)
      if (kind === 'panelActivation') {
        await vi.waitFor(() => expect(openPluginPanel).toHaveBeenCalledWith({ pluginId: 'com.example.multi', commandId: 'second', argument: 'hello' }))
      } else {
        await vi.waitFor(() => expect(searchApps).toHaveBeenCalledWith(expect.objectContaining({
          query: '/shared hello', completionOrigin: { pluginId: 'com.example.multi', commandId: 'second', phase: kind === 'windowActivation' ? 'commit' : 'preview' },
        })))
      }
    } finally { core.destroy() }
  })
})

describe('builtin hotkey entry', () => {
  const web: BuiltinFeatureView = { feature: 'webSearch', name: '网页搜索', command: '/shared', defaultCommand: '/web-search', favorite: false, hotkey: 'Ctrl+W', hotkeyStatus: 'active', directory: null, description: '搜索', examples: ['/shared'], version: '1.0.0', enabled: true }
  function fixture(listBuiltinFeatures: LauncherClient['listBuiltinFeatures'] = async () => [web]) {
    const searchApps = vi.fn<LauncherClient['searchApps']>(async () => null)
    const listQuicklinks = vi.fn(async () => ({ items: [] }))
    const core = createLauncherCore({ searchApps, listBuiltinFeatures, listQuicklinks,
      getMessageSummary: async () => ({ revision: '0', unreadCount: 0 }) } as unknown as LauncherClient)
    return { core, searchApps, listQuicklinks }
  }
  it('keeps web search stable while typing arguments and releases it when the command changes', async () => {
    const { core, searchApps } = fixture()
    try {
      core.shown({ invocationId: 'builtin-entry', target: 'launcher', notice: null, builtinEntry: 'webSearch' })
      await vi.waitFor(() => expect(core.getSnapshot().query).toBe('/shared '))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/shared hello', inputType: 'insertText' })
      await vi.waitFor(() => expect(searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '/shared hello', stableBuiltinTarget: 'webSearch' })))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/different', inputType: 'insertText' })
      await vi.waitFor(() => expect(searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '/different' })))
      expect(searchApps.mock.lastCall?.[0]).not.toHaveProperty('stableBuiltinTarget')
    } finally { core.destroy() }
  })
  it('does not overwrite user input after a delayed builtin lookup', async () => {
    let resolve!: (features: readonly BuiltinFeatureView[]) => void
    const { core } = fixture(() => new Promise((accept) => { resolve = accept }))
    try {
      core.shown({ invocationId: 'builtin-entry', target: 'launcher', notice: null, builtinEntry: 'webSearch' })
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'my query', inputType: 'insertText' })
      resolve([web])
      await Promise.resolve()
      expect(core.getSnapshot().query).toBe('my query')
    } finally { core.destroy() }
  })
  it.each(['webSearch', 'quicklinks'] as const)('does not replace an IME draft after delayed %s lookup', async (feature) => {
    let resolve!: (features: readonly BuiltinFeatureView[]) => void
    const { core, listQuicklinks } = fixture(() => new Promise((accept) => { resolve = accept }))
    try {
      core.shown({ invocationId: 'builtin-entry', target: 'launcher', notice: null, builtinEntry: feature })
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'compositionStart', control })
      core.text({ kind: 'compositionInput', control, value: '正在输入', inputType: 'insertCompositionText' })
      resolve([{ ...web, feature }])
      await Promise.resolve()
      expect(core.getSnapshot().queryControlValue).toBe('正在输入')
      expect(listQuicklinks).not.toHaveBeenCalled()
    } finally { core.destroy() }
  })
  it('opens quicklinks by identity even when its alias is shared', async () => {
    const { core, listQuicklinks } = fixture(async () => [{ ...web, feature: 'quicklinks' }])
    try {
      core.shown({ invocationId: 'builtin-entry', target: 'launcher', notice: null, builtinEntry: 'quicklinks' })
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks).toBeDefined())
      expect(listQuicklinks).toHaveBeenCalledOnce()
    } finally { core.destroy() }
  })
})
