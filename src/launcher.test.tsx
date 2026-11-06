// @vitest-environment jsdom

import { existsSync, readFileSync } from 'node:fs'

import { describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { theme } from 'antd'

import { createLauncherCore, safeLauncherActivation } from './launcher-core'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import launcherCoreSource from './launcher-core.ts?raw'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import mainSource from './main.ts?raw'
import { bindNativeTextInput } from './native-input'
import * as nativeInput from './native-input'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import nativeInputSource from './native-input.ts?raw'
import { LauncherView } from './launcher-view'
import { HotkeyRecorderInput } from './hotkey-recorder-input'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import launcherViewSource from './launcher-view.tsx?raw'
import {
  compareDecimalRevision,
  parseFileSearchResponse,
  parseRefreshAtUtc,
  parseLauncherShown,
  parsePluginInventorySnapshot,
  parsePluginPanelCommandResult,
  parsePluginPanelErrorEvent,
  parsePluginPanelFocusHostInputEvent,
  parsePluginMutationOutcome,
  parseU64Decimal,
  parsePublicPluginInventory,
  type ClassifiedTextRecord,
  type ControlKey,
  type ExecuteOutcome,
  type ResultActionOutcome,
  type FileResultItem,
  type FileSearchResponse,
  type LauncherClient,
  type FindClient,
  type LauncherShown,
  type PluginInventorySnapshot,
  type PluginInventoryView,
  type PluginPanelCommandResult,
  type SearchResponse,
  type SettingsView,
} from './protocol'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import protocolSource from './protocol.ts?raw'

const stylesSource = readFileSync('src/styles.css', 'utf8')
const uiThemeSource = readFileSync('src/ui-theme.css', 'utf8')
const publicPluginPanelSource = readFileSync('src/public-plugin-panel.tsx', 'utf8')

it('provides a browser-only launcher preview outside the production entry', () => {
  expect(existsSync('dev/main-preview.html')).toBe(true)
  const previewHtml = readFileSync('dev/main-preview.html', 'utf8')
  const previewSource = readFileSync('src/launcher-browser-preview.tsx', 'utf8')
  expect(previewHtml).toContain('/src/launcher-browser-preview.tsx')
  expect(previewSource).toContain('LauncherView')
  expect(previewSource).toContain("get('mode') === 'settings'")
  expect(previewSource).toContain("get('mode') === 'panel'")
  expect(previewSource).toContain("get('command')")
  expect(previewSource).toContain('panelPreviewCommand')
  expect(previewSource).toContain("kind: 'panelActivation'")
  expect(previewSource).toContain('previewPublicPlugins')
  expect(previewSource).toContain('com.uipilot.notes')
  expect(previewSource).toContain('com.uipilot.translate')
  expect(previewSource).toContain('core.activateResult')
  expect(previewSource).toContain('/examples/public-plugins/${contentPreview.path}/preview.html?theme=${previewTheme}')
  expect(previewSource).toContain("querySelector<HTMLElement>('.panel-host-region')")
  expect(previewHtml).toContain('/src/browser-preview.css')
  expect(previewSource).toContain("frame.className = 'browser-preview-frame browser-preview-panel-frame'")
  expect(previewSource).toContain('region.replaceChildren(frame)')
  expect(previewSource).not.toContain('frame.style.')
  expect(previewSource).not.toContain('@tauri-apps')
  expect(readFileSync('index.html', 'utf8')).not.toContain('launcher-browser-preview')
})

it('keeps managed Everything startup and unavailable messages fixed in the launcher compatibility path', () => {
  expect(protocolSource).toContain("| 'fileSearchStarting'")
  expect(launcherCoreSource).toContain("fileSearchStarting: '文件搜索正在准备'")
  expect(launcherCoreSource).toContain("searchUnavailable: '文件搜索服务不可用'")
  expect(launcherCoreSource).not.toContain("fileSearchStarting: 'private runtime diagnostic'")
})

describe('screenshot launcher entry', () => {
  const item = { resultId: 'screenshot', title: '/screenshot', iconKind: 'screenshot',
    activation: { kind: 'openScreenshot' }, hasDefaultAction: true,
    favorite: { target: { kind: 'builtin', feature: 'screenshot' }, favorite: false } }
  async function setup() {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValue({ requestId: 'capture-results', items: [item], replaceLocalResults: true } as SearchResponse)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('capture-invocation'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/screenshot', inputType: 'insertText' })
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalled())
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    return { ...fake, core }
  }
  it('searches without capture and enters region once with current query authority', async () => {
    const { client, core } = await setup()
    expect(core.getSnapshot().results).toHaveLength(1)
    expect(client.startScreenshot).not.toHaveBeenCalled()
    core.keyDown('Enter', false)
    core.keyDown('Enter', false)
    expect(client.startScreenshot).toHaveBeenCalledExactlyOnceWith({ mode: 'region', invocationId: 'capture-invocation', querySequence: core.getSnapshot().querySequence })
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(core.getSnapshot().query).toBe('/screenshot')
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.destroy()
  })
  it('offers window action and ignores actions after the query changes', async () => {
    const { client, core } = await setup()
    core.executeResultAction(0, 'screenshot-window')
    expect(client.startScreenshot).toHaveBeenCalledWith(expect.objectContaining({ mode: 'window' }))
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'new query', inputType: 'insertText' })
    core.executeResultAction(0, 'screenshot-window')
    expect(client.startScreenshot).toHaveBeenCalledOnce()
    core.destroy()
  })
  it('rejects a screenshot favorite attached to another activation', async () => {
    const { client, core } = await setup()
    vi.mocked(client.searchApps).mockResolvedValue({ requestId: 'forged', items: [{ ...item, activation: { kind: 'openQuicklinks' } }], replaceLocalResults: true } as SearchResponse)
    vi.mocked(client.searchApps).mockClear()
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'changed', inputType: 'insertText' })
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalled())
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results).toHaveLength(0)
    core.destroy()
  })
  it('retires pending screenshot execution when query ownership changes', async () => {
    const { client, core } = await setup()
    const pending = deferred<void>()
    vi.mocked(client.startScreenshot).mockReturnValueOnce(pending.promise)
    core.keyDown('Enter', false)
    expect(core.getSnapshot().executePending).toBe(true)
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'replacement', inputType: 'insertText' })
    expect(core.getSnapshot().executePending).toBe(false)
    pending.reject(new Error('late old failure'))
    await Promise.resolve()
    expect(core.getSnapshot().status).not.toContain('截图未能启动')
    core.destroy()
  })
  it('shows only capture errors owned by the current invocation and query', async () => {
    const { core, emitScreenshotError } = await setup()
    const querySequence = core.getSnapshot().querySequence
    emitScreenshotError({ code: 'resourceLimit', invocationId: 'older', querySequence })
    expect(core.getSnapshot().status).toBe('')
    emitScreenshotError({ code: 'resourceLimit', invocationId: 'capture-invocation', querySequence })
    expect(core.getSnapshot().status).toContain('内存限制')
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'newer', inputType: 'insertText' })
    emitScreenshotError({ code: 'resourceLimit', invocationId: 'capture-invocation', querySequence })
    expect(core.getSnapshot().status).not.toContain('内存限制')
    core.destroy()
  })
})

describe('retired validation settings contract', () => {
  it('contains no research, rescan, export, or validation-clear surface', () => {
    const productionSources = [protocolSource, launcherCoreSource, launcherViewSource, mainSource]
    for (const forbidden of [
      'researchId',
      'rescanApps',
      'exportValidation',
      'exportValidationData',
      'clearValidationData',
      'clearConfirmation',
      'validationFailed',
      "'rescan'",
      "'export'",
      "'clear'",
    ]) {
      expect(productionSources.every((source) => !source.includes(forbidden)), forbidden).toBe(true)
    }
  })
})

function installedPlugin(version = '1.0.0', description = '# Math', id = 'internal.math'): PluginInventoryView {
  return {
    key: `plugin:${id}`,
    id,
    displayName: id,
    installed: { state: 'valid', activeVersion: version, versions: [version], trigger: `/${id}` },
    development: { state: 'absent' },
    description: description
      ? { state: 'available', source: 'installed', markdown: description }
      : { state: 'unavailable' },
  }
}

function developmentPlugin(version = '1.0.0'): PluginInventoryView {
  return {
    key: 'plugin:internal-math',
    id: 'internal.math',
    displayName: 'internal.math',
    installed: { state: 'absent' },
    development: { state: 'valid', version, trigger: '/math' },
    description: { state: 'available', source: 'development', markdown: '# Math' },
  }
}

function pluginInventory(
  items: PluginInventoryView[] = [],
  revision = '1',
): PluginInventorySnapshot {
  return { revision, items }
}

describe('plugin protocol', () => {
  const plugin = installedPlugin()

  it('accepts only exact revisioned dense inventory snapshots', () => {
    expect(parsePluginInventorySnapshot(pluginInventory([plugin]))).toEqual(pluginInventory([plugin]))
    expect(parsePluginInventorySnapshot(pluginInventory())).toEqual(pluginInventory())
    expect(parsePluginInventorySnapshot({ ...pluginInventory(), revision: 1 })).toBeNull()
    expect(parsePluginInventorySnapshot({ ...pluginInventory(), revision: '01' })).toBeNull()
    expect(parsePluginInventorySnapshot(pluginInventory([plugin, { ...plugin }]))).toBeNull()
    expect(parsePluginInventorySnapshot(pluginInventory([{ ...plugin, extra: true } as never]))).toBeNull()
    const sparse = new Array(1)
    expect(parsePluginInventorySnapshot({ revision: '1', items: sparse })).toBeNull()
    expect(parsePluginInventorySnapshot(Object.assign(Object.create({}), pluginInventory()))).toBeNull()
  })

  it('strictly parses public inventory settings and rejects output mode or secret value leaks', () => {
    const item = {
      pluginId: 'com.example.demo', name: 'Demo', description: null, version: '1.0.0',
      source: 'localPackage', installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'demo', effectiveName: 'demo', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
      fault: null, generation: 1,
      iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.demo/1/icon.png',
      network: null,
      permissions: [{ permission: 'clipboard.write', supported: true, granted: true }],
      settings: [
        { definition: { type: 'text', key: 'prefix', label: 'Prefix', default: 'Hi' }, value: 'Hello' },
        { definition: { type: 'number', key: 'limit', label: 'Limit', min: 1, max: 20, step: 1 }, value: 3 },
        { definition: { type: 'boolean', key: 'loud', label: 'Loud' }, value: true },
        { definition: { type: 'select', key: 'style', label: 'Style', options: [{ value: 'short', label: 'Short' }] }, value: 'short' },
        { definition: { type: 'secret', key: 'token', label: 'Token' }, secretConfigured: false },
      ],
    }
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [item] })).toEqual({ revision: '1', hotkeyRevision: '0', items: [item] })
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [{ ...item, iconUrl: null }] })).not.toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [{ ...item, iconUrl: 'https://example.com/icon.png' }] })).toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [{ ...item, outputMode: 'mainResult' }] })).toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [{ ...item, settings: [{ definition: { type: 'secret', key: 'token', label: 'Token' }, secretConfigured: true, value: 'leak' }] }] })).toBeNull()
  })
  it('parses mutation revisions and compares the full u64 range without Number', () => {
    expect(parsePluginMutationOutcome({ revision: '18446744073709551615' })).toEqual({
      revision: '18446744073709551615',
    })
    expect(parsePluginMutationOutcome({ revision: '18446744073709551616' })).toBeNull()
    expect(compareDecimalRevision('9007199254740991', '9007199254740992')).toBe(-1)
    expect(compareDecimalRevision('18446744073709551614', '18446744073709551615')).toBe(-1)
  })

  it('strictly parses panel command identities and epoch-bound error events', () => {
    const identity = {
      sessionEpoch: '18446744073709551615',
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      inputPlaceholder: '搜索目录',
      hostKeys: ['Enter', 'Shift+Tab', 'Tab', 'Primary+N', 'ArrowUp', 'ArrowDown'],
    }
    expect(parsePluginPanelCommandResult(identity)).toEqual({
      ...identity,
      hostKeys: ['ArrowDown', 'ArrowUp', 'Primary+N', 'Tab', 'Shift+Tab', 'Enter'],
    })
    expect(parsePluginPanelErrorEvent({ sessionEpoch: '9' })).toEqual({ sessionEpoch: '9' })
    for (const invalid of [
      { ...identity, sessionEpoch: '0' },
      { ...identity, sessionEpoch: '01' },
      { ...identity, pluginId: 'Invalid Plugin' },
      { ...identity, commandLabel: '/demo-panel' },
      { ...identity, hostKeys: 'ArrowDown' },
      { ...identity, hostKeys: ['Space'] },
      { ...identity, hostKeys: ['ArrowDown', 'ArrowDown'] },
      { ...identity, extra: true },
    ]) expect(parsePluginPanelCommandResult(invalid)).toBeNull()
    expect(parsePluginPanelErrorEvent({ sessionEpoch: '9', extra: true })).toBeNull()
    expect(parsePluginPanelFocusHostInputEvent({ sessionEpoch: '9', focusRequestId: '10' })).toEqual({
      sessionEpoch: '9',
      focusRequestId: '10',
    })
    for (const invalid of [
      { sessionEpoch: '0', focusRequestId: '10' },
      { sessionEpoch: '9', focusRequestId: '0' },
      { sessionEpoch: '9', focusRequestId: '01' },
      { sessionEpoch: '9', focusRequestId: '10', extra: true },
    ]) expect(parsePluginPanelFocusHostInputEvent(invalid)).toBeNull()
  })
})

const configCapture = vi.hoisted(() => ({ values: [] as unknown[] }))
const tauriCapture = vi.hoisted(() => ({ capabilities: vi.fn(), invoke: vi.fn(), listen: vi.fn() }))
const windowCapture = vi.hoisted(() => ({ label: 'main' }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: (command: string, ...args: unknown[]) =>
  command === 'get_host_capabilities' ? tauriCapture.capabilities(...args) : tauriCapture.invoke(command, ...args) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: tauriCapture.listen }))

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: windowCapture.label }) }))
vi.mock('antd', async () => {
  const actual = await vi.importActual<typeof import('antd')>('antd')
  const React = await import('react')
  return {
    ...actual,
    ConfigProvider: (props: React.ComponentProps<typeof actual.ConfigProvider>) => {
      configCapture.values.push(props.theme)
      return React.createElement(actual.ConfigProvider, props)
    },
  }
});

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
Object.defineProperty(globalThis, 'ResizeObserver', {
  configurable: true,
  value: class {
    observe() {}
    unobserve() {}
    disconnect() {}
  },
})

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}

const emptySettings: SettingsView = {
  screenshotHotkey: null,
  screenshotHotkeyStatus: 'unset',
  screenshotSaveDirectory: null,
  screenshotSaveDirectoryPath: 'C:\\Users\\demo\\OneDrive\\Desktop',
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
  theme: 'system',
  webSearchEngine: 'bing',
}

const settingsFixture: SettingsView = {
  screenshotHotkey: null,
  screenshotHotkeyStatus: 'unset',
  screenshotSaveDirectory: null,
  screenshotSaveDirectoryPath: 'C:\\Users\\demo\\OneDrive\\Desktop',
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
  theme: 'system',
  webSearchEngine: 'bing',
}

type TestLauncherClient = LauncherClient & {
  searchFiles: FindClient['searchFiles']
  setFilePreviewPreference(input: { preference: { enabled: boolean } }): Promise<void>
}

function fakeClient() {
  let shownHandler: ((payload: unknown) => void) | undefined
  let hiddenHandler: (() => void) | undefined
  let builtinFeaturesChangedHandler: (() => void) | undefined
  let messageStateHandler: ((payload: unknown) => void) | undefined
  let panelStatusHandler: ((payload: unknown) => void) | undefined
  let panelErrorHandler: ((payload: unknown) => void) | undefined
  let panelResetHandler: ((payload: unknown) => void) | undefined
  let panelFocusHandler: ((payload: unknown) => void) | undefined
  let screenshotErrorHandler: ((payload: unknown) => void) | undefined
  const unlisten = vi.fn()
  const client = {
    listenShown: vi.fn(async (handler) => {
      shownHandler = handler
      return unlisten
    }),
    listenHidden: vi.fn(async (handler) => {
      hiddenHandler = handler
      return vi.fn()
    }),
    listenBuiltinFeaturesChanged: vi.fn(async (handler) => {
      builtinFeaturesChangedHandler = handler
      return vi.fn()
    }),
    listenMessageStateChanged: vi.fn(async (handler) => {
      messageStateHandler = handler
      return vi.fn()
    }),
    listenPluginPanelStatus: vi.fn(async (handler) => { panelStatusHandler = handler; return vi.fn() }),
    listenPluginPanelError: vi.fn(async (handler) => {
      panelErrorHandler = handler
      return vi.fn()
    }),
    listenPluginPanelReset: vi.fn(async (handler) => {
      panelResetHandler = handler
      return vi.fn()
    }),
    listenPluginPanelFocusHostInput: vi.fn(async (handler) => {
      panelFocusHandler = handler
      return vi.fn()
    }),
    getMessageSummary: vi.fn(async () => ({ revision: '0', unreadCount: 0 })),
    openMessageCenter: vi.fn(async () => ({ revision: '0', unreadCount: 0, messages: [] })),
    readMessageCenter: vi.fn(async () => ({ revision: '0', unreadCount: 0, messages: [] })),
    clearMessages: vi.fn(async () => ({ revision: '0', unreadCount: 0, messages: [] })),
    searchApps: vi.fn(async () => null),
    listQuicklinks: vi.fn(async () => ({ items: [] })),
    saveQuicklink: vi.fn(async (input: { input: { id?: string; name: string; command: string; template: string; iconToken?: string | null } }) => ({
      id: input.input.id ?? 'quicklink-new',
      name: input.input.name,
      command: input.input.command,
      template: input.input.template,
      iconDataUrl: undefined,
      createdAt: '2026-08-31T00:00:00Z',
      updatedAt: '2026-08-31T00:00:00Z',
    })),
    deleteQuicklink: vi.fn(async () => undefined),
    chooseQuicklinkIcon: vi.fn(async () => null),
    commitPluginWindowTransfer: vi.fn(async () => {}),
    openFind: vi.fn(async () => ({ status: 'forwarded' as const })),
    startScreenshot: vi.fn(async () => undefined),
    listenScreenshotError: vi.fn(async (handler) => { screenshotErrorHandler = handler; return vi.fn() }),
    searchFiles: vi.fn(async () => null),
    setFilePreviewPreference: vi.fn(async () => undefined),
    setThemePreference: vi.fn(async () => undefined),
    setWebSearchEngine: vi.fn(async () => undefined),
    executeResult: vi.fn(async () => ({ status: 'launchRequested' }) satisfies ExecuteOutcome),
    executeResultAction: vi.fn(async () => ({ kind: 'copied' }) satisfies ResultActionOutcome),
    retireResultActions: vi.fn(async () => undefined),
    openPluginPanel: vi.fn(async () => ({
      sessionEpoch: '1',
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: [],
    })),
    submitPluginPanel: vi.fn(async (input: { sessionEpoch: string }) => ({
      sessionEpoch: input.sessionEpoch,
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: [],
    })),
    enqueuePluginPanelHostKey: vi.fn(async () => ({ outcome: 'enqueued', routeSequence: '1' })),
    setPluginPanelBounds: vi.fn(async () => undefined),
    closePluginPanel: vi.fn(async () => undefined),
    acknowledgePluginPanelFocusHostInput: vi.fn(async () => undefined),
    resolvePublicPluginCommandEntry: vi.fn(async () => ({ pluginId: 'com.example.hotkey', commandId: 'default', commandLabel: 'hotkey', outputMode: 'mainResult', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })),
    setPublicPluginCommandHotkey: vi.fn(async () => undefined),
    listBuiltinFeatures: vi.fn(async () => []),
    setBuiltinFeatureName: vi.fn(async () => undefined),
    setBuiltinFeatureHotkey: vi.fn(async () => undefined),
    setBuiltinFeatureEnabled: vi.fn(async () => []),
    listPublicPlugins: vi.fn(async () => ({ revision: '0', hotkeyRevision: '0', items: [] })),
    selectPublicPluginArchive: vi.fn(async () => null),
    selectPublicPluginDirectory: vi.fn(async () => null),
    preparePublicPlugin: vi.fn(async () => { throw new Error('not prepared') }),
    commitPublicPlugin: vi.fn(async () => undefined),
    cancelPublicPlugin: vi.fn(async () => undefined),
    getPublicPluginDiagnostics: vi.fn(async ({ pluginId }) => ({ schemaVersion: 1, hostVersion: '0.7.0', platform: 'windows', pluginId, state: null, lastOperation: null })),
    copyPublicPluginDiagnostics: vi.fn(async () => undefined),
    setPublicPluginEnabled: vi.fn(async () => undefined),
    setPublicPluginNetworkAccess: vi.fn(async () => undefined),
    setPublicPluginPermissionAccess: vi.fn(async () => undefined),
    setPublicPluginFavorite: vi.fn(async () => undefined),
    setBuiltinFeatureFavorite: vi.fn(async () => undefined),
    setPublicPluginEffectiveName: vi.fn(async () => undefined),
    savePublicPluginSettings: vi.fn(async () => undefined),
    uninstallPublicPlugin: vi.fn(async () => undefined),    listPlugins: vi.fn(async () => pluginInventory()),
    installPlugin: vi.fn(async () => ({ revision: '2' })),
    reloadPlugin: vi.fn(async () => ({ revision: '2' })),
    deletePlugin: vi.fn(async () => ({ revision: '2' })),
    loadSettings: vi.fn(async () => emptySettings),
    saveSettings: vi.fn(async () => undefined),
    saveHotkey: vi.fn(async (input: { hotkey: { hotkey: string } }) => ({ hotkey: input.hotkey.hotkey })),
    setScreenshotHotkey: vi.fn(async () => {}),
    selectScreenshotSaveDirectory: vi.fn(async () => null),
    setScreenshotSaveDirectory: vi.fn(async () => ({ directory: null, path: null })),
    setHotkeyRecording: vi.fn(async () => {}),
    hideLauncher: vi.fn(async () => undefined),
  } as unknown as TestLauncherClient
  return {
    client,
    emit(payload: unknown) {
      if (!shownHandler) throw new Error('shown listener is not installed')
      shownHandler(payload)
    },
    emitHidden() {
      if (!hiddenHandler) throw new Error('hidden listener is not installed')
      hiddenHandler()
    },
    emitBuiltinFeaturesChanged() {
      if (!builtinFeaturesChangedHandler) throw new Error('builtin feature listener is not installed')
      builtinFeaturesChangedHandler()
    },
    emitScreenshotError(payload: unknown) {
      screenshotErrorHandler?.(payload)
    },
    emitMessageState(payload: unknown) {
      if (!messageStateHandler) throw new Error('message state listener is not installed')
      messageStateHandler(payload)
    },
    emitPanelStatus(payload: unknown) { panelStatusHandler?.(payload) },
    emitPanelError(payload: unknown) {
      if (!panelErrorHandler) throw new Error('panel error listener is not installed')
      panelErrorHandler(payload)
    },
    emitPanelReset(payload: unknown) {
      if (!panelResetHandler) throw new Error('panel reset listener is not installed')
      panelResetHandler(payload)
    },
    emitPanelFocus(payload: unknown) {
      if (!panelFocusHandler) throw new Error('panel focus listener is not installed')
      panelFocusHandler(payload)
    },
    unlisten,
  }
}

function u64(value: string) {
  return parseU64Decimal(value)!
}

describe('main result actions', () => {
  const command = { pluginId: 'com.uipilot.actions', commandId: 'transform', commandLabel: 'actions', argument: 'hello' }
  function response(requestId = 'req-actions', defaultAction = true): SearchResponse {
    return { requestId, mainResultCommand: command, items: [0, 1].map((index) => ({
      resultId: `item-${requestId}-${index}`, title: `Result ${index}`, activation: executeActivation,
      hasDefaultAction: defaultAction,
      actions: [{ actionId: `action-${requestId}-${index}`, title: '复制' }, { actionId: `save-${requestId}-${index}`, title: '保存' }],
      ...(defaultAction ? { defaultActionId: `action-${requestId}-${index}` } : {}),
    })) }
  }
  async function setup(defaultAction = true) {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (input) => input.submit ? response('req-actions', defaultAction) : null)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('action-owner'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/actions hello', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    return { ...fake, core }
  }

  it('keeps the operations entry stable and moves action shortcuts into the menu', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client } = await setup()
    const mounted = await mountLauncherView(core)
    try {
      const footer = mounted.host.querySelector('.launcher-status-region')!
      const trigger = footer.querySelector<HTMLButtonElement>('[aria-label="操作 Ctrl+K"]')!
      expect(footer.textContent).not.toContain('Enter')
      await act(async () => trigger.click())
      const menu = document.querySelector('.uipilot-context-actions')!
      expect(menu.textContent).toContain('复制Enter')
      expect(menu.textContent).not.toMatch(/点击执行|↑↓选择/)
      await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
      vi.mocked(client.executeResultAction).mockResolvedValueOnce({ kind: 'copied' })
      await act(async () => core.keyDown('Enter', false))
      expect(footer.querySelector('[role="status"]')?.textContent).toBe('已复制')
      expect(trigger.disabled).toBe(false)
      await act(async () => trigger.click())
      expect(document.querySelector('.uipilot-context-actions')?.textContent).toContain('重新运行')
      expect(document.querySelector('.uipilot-context-action[aria-disabled="true"]')).not.toBeNull()
    } finally { await mounted.unmount(); core.destroy() }
  })

  it('invalidates menu callbacks when results refresh with reused action identifiers', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client } = await setup()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('.launcher-operations-trigger')!.click())
      const old = document.querySelector<HTMLButtonElement>('.uipilot-context-action')!
      vi.mocked(client.executeResultAction).mockResolvedValueOnce({ kind: 'resultsUpdated', response: { ...response(), requestId: 'refreshed-request' } })
      await act(async () => core.executeResultAction(0, 'save-req-actions-0'))
      expect(document.querySelector('.uipilot-context-actions')).toBeNull()
      await act(async () => old.click())
      expect(client.executeResultAction).toHaveBeenCalledOnce()
    } finally { await mounted.unmount(); core.destroy() }
  })

  it.each([true, false])('separates legacy main-result status and executable hints: %s', async (executable) => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (input) => input.submit ? {
      requestId: 'legacy-footer', mainResultCommand: command,
      items: [{ resultId: 'legacy', activation: executeActivation, title: executable ? 'Method' : '翻译失败', subtitle: executable ? '中译英' : '请稍后重试', hasDefaultAction: executable }],
    } : null)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('legacy-footer'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/actions hello', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    const mounted = await mountLauncherView(core)
    try {
      const footer = mounted.host.querySelector('.launcher-status-region')!
      expect(footer.querySelector('[role="status"]')?.textContent).toBe('1 个结果')
      expect(footer.querySelector('[aria-label="操作 Ctrl+K"]')).not.toBeNull()
      expect(footer.textContent).not.toContain('Enter')
    } finally { await mounted.unmount(); core.destroy() }
  })

  it('opens the menu for a no-default row and executes only the explicitly chosen action once', async () => {
    const { core, client } = await setup(false)
    core.keyDown('Enter', false)
    expect(core.getSnapshot().resultActionMenu).toEqual({ resultKey: core.getSnapshot().results[0]!.key })
    expect(client.executeResult).not.toHaveBeenCalled()
    expect(client.executeResultAction).not.toHaveBeenCalled()
    const pending = deferred<ResultActionOutcome>()
    vi.mocked(client.executeResultAction).mockReturnValue(pending.promise)
    core.executeResultAction(0, 'save-req-actions-0')
    core.executeResultAction(1, 'save-req-actions-1')
    expect(client.executeResultAction).toHaveBeenCalledExactlyOnceWith({ requestId: 'req-actions', resultId: 'item-req-actions-0', actionId: 'save-req-actions-0' })
    pending.resolve({ kind: 'copied' })
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe('已复制'))
    expect(core.getSnapshot().resultActionsDisabled).toBe(true)
    expect(core.getSnapshot().resultActionRerun).toBe(true)
    core.keyDown('Enter', false)
    expect(client.executeResultAction).toHaveBeenCalledOnce()
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.rerunResultCommand()
    expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ submit: true, query: '/actions hello' }))
    expect(client.executeResultAction).toHaveBeenCalledOnce()
    core.destroy()
  })

  it.each(['staleRequest', 'unknownResult', 'urlOpenFailed', 'actionFailed', 'actionInvalidResponse', 'actionTimedOut', 'actionUnavailable', 'clipboardWriteFailed', 'actionBusy'] as const)('handles %s with the reported source consumption policy', async (code) => {
    const { core, client } = await setup()
    const consumed = !['staleRequest', 'unknownResult', 'actionBusy'].includes(code)
    vi.mocked(client.executeResultAction).mockResolvedValue({ kind: 'failed', code, sourceConsumed: consumed })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(core.getSnapshot().resultActionsDisabled).toBe(code !== 'actionBusy')
    expect(core.getSnapshot().resultActionRerun).toBe(code !== 'actionBusy')
    if (code === 'staleRequest' || code === 'unknownResult') expect(core.getSnapshot().status).toBe('结果已过期，请重新运行')
    if (consumed) expect(core.getSnapshot().status).toBe('操作未完成，请重新读取核对后再运行')
    if (code === 'actionBusy') {
      core.keyDown('Enter', false)
      expect(client.executeResultAction).toHaveBeenCalledTimes(2)
    }
    core.destroy()
  })

  it('consumes the owned source after a URL request without hiding or executing siblings', async () => {
    const { core, client } = await setup()
    vi.mocked(client.executeResultAction).mockResolvedValue({ kind: 'urlOpened' })
    core.executeResultAction(0, 'save-req-actions-0')
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe('已请求打开链接'))
    expect(core.getSnapshot().resultActionMenu).toBeUndefined()
    expect(core.getSnapshot().resultActionsDisabled).toBe(true)
    expect(core.getSnapshot().resultActionRerun).toBe(true)
    core.executeResultAction(1, 'save-req-actions-1')
    expect(client.executeResultAction).toHaveBeenCalledOnce()
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.destroy()
  })

  it.each([
    ['pasteTargetUnavailable', false, '无法返回原应用，请从目标应用重新唤起后重试。'],
    ['pasteInputBusy', false, '请松开按键后重新执行。'],
    ['pasteFailed', true, '粘贴未确认，请检查目标应用后重新运行命令。'],
  ] as const)('shows %s with explicit consumption', async (code, sourceConsumed, status) => {
    const { core, client } = await setup()
    vi.mocked(client.executeResultAction).mockResolvedValue({ kind: 'failed', code, sourceConsumed })
    core.executeResultAction(0, 'save-req-actions-0')
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe(status))
    expect(core.getSnapshot().resultActionsDisabled).toBe(sourceConsumed)
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.destroy()
  })
  it('reports one accepted paste request and ignores its completion after the normal hidden event', async () => {
    const first = await setup()
    vi.mocked(first.client.executeResultAction).mockResolvedValue({ kind: 'pasteSent' })
    first.core.executeResultAction(0, 'save-req-actions-0')
    await vi.waitFor(() => expect(first.core.getSnapshot().status).toBe('已请求粘贴'))
    expect(first.core.getSnapshot().resultActionsDisabled).toBe(true)
    first.core.destroy()
    const second = await setup()
    const pending = deferred<ResultActionOutcome>()
    vi.mocked(second.client.executeResultAction).mockReturnValue(pending.promise)
    second.core.executeResultAction(0, 'save-req-actions-0')
    second.emitHidden()
    const hidden = second.core.getSnapshot()
    pending.resolve({ kind: 'pasteSent' })
    await Promise.resolve(); await Promise.resolve()
    expect(second.core.getSnapshot().results).toEqual(hidden.results)
    expect(second.core.getSnapshot().status).toBe(hidden.status)
    second.core.destroy()
  })

  it('installs fresh handles after a highlight change without autoexecution or suffix replacement', async () => {
    const { core, client } = await setup()
    const pending = deferred<ResultActionOutcome>()
    vi.mocked(client.executeResultAction).mockReturnValueOnce(pending.promise)
    const suffix = core.getSnapshot().mainResultCommand!.suffixControl
    core.keyDown('Enter', false)
    core.selectResult(1)
    pending.resolve({ kind: 'resultsUpdated', response: response('req-fresh') })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.title).toBe('Result 0'))
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(core.getSnapshot().selectedIndex).toBe(0)
    expect(core.getSnapshot().mainResultCommand!.suffixControl).toBe(suffix)
    expect(client.executeResultAction).toHaveBeenCalledOnce()
    core.keyDown('Enter', false)
    expect(client.executeResultAction).toHaveBeenLastCalledWith({ requestId: 'req-fresh', resultId: 'item-req-fresh-0', actionId: 'action-req-fresh-0' })
    expect(client.executeResult).not.toHaveBeenCalled()
    core.destroy()
  })

  it.each(['success', 'url-opened', 'paste-sent', 'failure', 'reject'] as const)('retires before debounce and ignores old %s while a new action is pending', async (kind) => {
    const { core, client } = await setup()
    const old = deferred<ResultActionOutcome>()
    const next = deferred<ResultActionOutcome>()
    vi.mocked(client.executeResultAction).mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise)
    core.keyDown('Enter', false)
    vi.mocked(client.searchApps).mockImplementation(async (input) => input.submit
      ? { ...response('req-next'), mainResultCommand: { ...command, argument: 'next' } } : null)
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'next', inputType: 'insertText' })
    expect(client.retireResultActions).toHaveBeenCalledWith({ requestId: 'req-actions' })
    expect(core.getSnapshot().executePending).toBe(false)
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    core.keyDown('Enter', false)
    expect(core.getSnapshot().executePending).toBe(true)
    expect(core.getSnapshot().status).toBe('正在执行动作…')
    if (kind === 'success') old.resolve({ kind: 'resultsUpdated', response: response('req-too-late') })
    else if (kind === 'url-opened') old.resolve({ kind: 'urlOpened' })
    else if (kind === 'paste-sent') old.resolve({ kind: 'pasteSent' })
    else if (kind === 'failure') old.resolve({ kind: 'failed', code: 'actionTimedOut', sourceConsumed: true })
    else old.reject(new Error('private failure'))
    await Promise.resolve(); await Promise.resolve()
    expect(core.getSnapshot().executePending).toBe(true)
    expect(core.getSnapshot().status).toBe('正在执行动作…')
    expect(core.getSnapshot().mainResultCommand?.suffix).toBe('next')
    next.resolve({ kind: 'copied' })
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe('已复制'))
    core.destroy()
  })

  it('retires on settings departure and suppresses a late completion', async () => {
    const { core, client } = await setup()
    const pending = deferred<ResultActionOutcome>()
    vi.mocked(client.executeResultAction).mockReturnValueOnce(pending.promise)
    core.keyDown('Enter', false)
    core.navigate('settings')
    expect(client.retireResultActions).toHaveBeenCalledWith({ requestId: 'req-actions' })
    pending.resolve({ kind: 'resultsUpdated', response: response('req-too-late') })
    await Promise.resolve(); await Promise.resolve()
    expect(core.getSnapshot().view).toBe('settings')
    expect(core.getSnapshot().results).toHaveLength(0)
    core.destroy()
  })

  it.each(['footer', 'context', 'keyboard', 'no-default'] as const)('routes the %s entry to the same action menu without a row default', async (entry) => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client } = await setup(entry !== 'no-default')
    const mounted = await mountLauncherView(core)
    try {
      const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
      await act(async () => input.focus())
      const actions = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="操作 Ctrl+K"]')
      expect(actions).not.toBeNull()
      expect(mounted.host.querySelector('.launcher-operation-hint')).toBeNull()
      if (entry === 'footer') await act(async () => actions!.click())
      else if (entry === 'context') await act(async () => mounted.host.querySelectorAll<HTMLElement>('[role="option"]')[1]!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true })))
      else await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', {
        key: entry === 'keyboard' ? 'k' : 'Enter', ctrlKey: entry === 'keyboard', bubbles: true, cancelable: true,
      })))
      await vi.waitFor(() => expect(document.querySelector('[role="menu"], .uipilot-context-actions')).not.toBeNull())
      expect(client.executeResult).not.toHaveBeenCalled()
      expect(client.executeResultAction).not.toHaveBeenCalled()
      const save = [...document.querySelectorAll<HTMLElement>('[role="menuitem"], .uipilot-context-action')].find((element) => element.textContent === '保存')!
      await act(async () => save.click())
      const selected = entry === 'context' ? 1 : 0
      await vi.waitFor(() => expect(client.executeResultAction).toHaveBeenCalledExactlyOnceWith({ requestId: 'req-actions', resultId: `item-req-actions-${selected}`, actionId: `save-req-actions-${selected}` }))
      expect(client.executeResult).not.toHaveBeenCalled()
      expect(mounted.host.querySelector<HTMLButtonElement>('button[aria-label="操作 Ctrl+K"]')!.disabled).toBe(false)
      expect(core.getSnapshot().resultActionRerun).toBe(true)
      if (entry === 'footer') await vi.waitFor(() => expect(document.activeElement).toBe(input))
    } finally { core.destroy(); await mounted.unmount() }
  })

  it('keeps suffix caret and routes Tab through actions and Settings; Escape closes the menu first', async () => {
    installMatchMedia(true)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client } = await setup()
    const mounted = await mountLauncherView(core)
    try {
      const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
      const actions = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="操作 Ctrl+K"]')!
      const settings = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="打开设置"]')!
      expect(settings).not.toBeNull()
      await act(async () => { input.focus(); input.setSelectionRange(2, 2) })
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })))
      expect(document.activeElement).toBe(actions)
      await act(async () => actions.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })))
      expect(document.activeElement).toBe(settings)
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, isComposing: true, bubbles: true, cancelable: true })))
      expect(core.getSnapshot().resultActionMenu).toBeUndefined()
      await act(async () => { input.focus(); input.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true })) })
      await vi.waitFor(() => expect(document.querySelector('.uipilot-context-actions')).not.toBeNull())
      await act(async () => document.querySelector<HTMLElement>('.uipilot-context-actions')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
      expect(core.getSnapshot().resultActionMenu).toBeUndefined()
      expect(client.hideLauncher).not.toHaveBeenCalled()
      await vi.waitFor(() => expect(document.activeElement).toBe(input))
      expect(input.selectionStart).toBe(2)
    } finally { core.destroy(); await mounted.unmount() }
  })

  it('routes menu Home/End/arrows/Enter and Tab without dispatching the row default', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client } = await setup()
    const mounted = await mountLauncherView(core)
    try {
      const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
      const key = async (element: Element, value: string, keyCode: number) => act(async () => element.dispatchEvent(new KeyboardEvent('keydown', { key: value, keyCode, which: keyCode, bubbles: true, cancelable: true })))
      await act(async () => { input.focus(); input.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true })) })
      await vi.waitFor(() => expect(document.querySelector('.uipilot-context-actions')).not.toBeNull())
      const menu = document.querySelector<HTMLElement>('.uipilot-context-actions')!
      await key(menu, 'End', 35)
      await vi.waitFor(() => expect(document.activeElement?.textContent).toBe('隐藏窗口Esc'))
      await key(document.activeElement!, 'Home', 36)
      await vi.waitFor(() => expect(document.activeElement?.textContent).toBe('复制Enter'))
      await key(document.activeElement!, 'ArrowDown', 40)
      await vi.waitFor(() => expect(document.activeElement?.textContent).toBe('保存'))
      await key(document.activeElement!, 'ArrowUp', 38)
      await vi.waitFor(() => expect(document.activeElement?.textContent).toBe('复制Enter'))
      await key(document.activeElement!, 'Tab', 9)
      expect(document.activeElement?.textContent).toBe('保存')
      await key(document.activeElement!, 'Enter', 13)
      await vi.waitFor(() => expect(client.executeResultAction).toHaveBeenCalledExactlyOnceWith({ requestId: 'req-actions', resultId: 'item-req-actions-0', actionId: 'save-req-actions-0' }))
      expect(client.executeResult).not.toHaveBeenCalled()
    } finally { core.destroy(); await mounted.unmount() }
  })

  it('handles an empty replacement atomically and ignores late retirement settlement', async () => {
    const { core, client } = await setup()
    const retirement = deferred<void>()
    vi.mocked(client.retireResultActions).mockReturnValueOnce(retirement.promise)
    core.rerunResultCommand() // Only an explicit consumed-source control may resubmit.
    expect(client.retireResultActions).not.toHaveBeenCalled()
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().resultActionRerun).toBe(true))
    vi.mocked(client.searchApps).mockResolvedValue(response('req-new'))
    core.rerunResultCommand()
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    retirement.reject(new Error('private retirement failure'))
    await Promise.resolve(); await Promise.resolve()
    expect(core.getSnapshot().status).toBe('')
    const suffix = core.getSnapshot().mainResultCommand!.suffixControl
    vi.mocked(client.executeResultAction).mockResolvedValueOnce({ kind: 'resultsUpdated', response: { ...response('req-empty'), items: [] } })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(0))
    expect(core.getSnapshot().selectedIndex).toBe(-1)
    expect(core.getSnapshot().executePending).toBe(false)
    expect(core.getSnapshot().mainResultCommand!.suffixControl).toBe(suffix)
    core.destroy()
  })

  it('accepts action results from a live command and never falls back for malformed action metadata', async () => {
    const fake = fakeClient()
    let malformed = false
    vi.mocked(fake.client.searchApps).mockImplementation(async () => ({
      ...response(), items: malformed ? [{ ...response().items[0]!, defaultActionId: 'missing' }] : response().items,
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('live-actions'))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    expect(core.getSnapshot().mainResultCommand).toMatchObject({ commandId: 'transform', suffix: 'hello' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().resultActionRerun).toBe(true))
    expect(fake.client.executeResultAction).toHaveBeenCalledOnce()
    malformed = true
    core.rerunResultCommand()
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(0))
    core.keyDown('Enter', false)
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.executeResultAction).toHaveBeenCalledOnce()
    core.destroy()
  })

  it('retires a composing or hidden source before its late copy feedback', async () => {
    for (const departure of ['composition', 'hidden'] as const) {
      const { core, client, emitHidden } = await setup()
      const pending = deferred<ResultActionOutcome>()
      vi.mocked(client.executeResultAction).mockReturnValueOnce(pending.promise)
      core.keyDown('Enter', false)
      if (departure === 'composition') core.text({ kind: 'compositionStart', control: core.getSnapshot().mainResultCommand!.suffixControl })
      else emitHidden()
      expect(client.retireResultActions).toHaveBeenCalledWith({ requestId: 'req-actions' })
      pending.resolve({ kind: 'copied' })
      await Promise.resolve(); await Promise.resolve()
      expect(core.getSnapshot().status).not.toBe('已复制')
      expect(core.getSnapshot().executePending).toBe(false)
      core.destroy()
    }
  })

  it('retires an initially empty main-result source on edit even without row metadata', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValue({ ...response('req-empty-initial'), items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('empty-action-source'))
    await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand).toBeDefined())
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'edited', inputType: 'insertText' })
    expect(fake.client.retireResultActions).toHaveBeenCalledWith({ requestId: 'req-empty-initial' })
    core.destroy()
  })
})

function panelItem(initialArgument: string, pluginId = 'com.uipilot.demo-panel') {
  return {
    resultId: `panel-${pluginId}`,
    title: '/demo-panel',
    subtitle: '打开面板',
    activation: {
      kind: 'panelActivation' as const, commandId: 'default',
      pluginId,
      initialArgument,
      favorite: false,
    },
    favorite: {
      target: { kind: 'publicPlugin' as const, pluginId, commandId: 'default' },
      favorite: false,
    },
    hasDefaultAction: false,
  }
}

const executeActivation = { kind: 'executeResult' } as const

function findLauncherItem(query: string) {
  return {
    resultId: `find-${query || 'empty'}`,
    title: '/find',
    subtitle: query ? `搜索文件：${query}` : '搜索文件',
    iconKind: 'find' as const,
    activation: { kind: 'openFind' as const, query },
    favorite: {
      target: { kind: 'builtin' as const, feature: 'find' as const },
      favorite: false,
    },
    hasDefaultAction: false,
  }
}

function quicklinkView(overrides: Partial<{
  id: string
  name: string
  command: string
  template: string
  iconDataUrl?: string
  createdAt: string
  updatedAt: string
}> = {}) {
  return {
    id: overrides.id ?? 'quicklink-jd',
    name: overrides.name ?? '京东搜索',
    command: overrides.command ?? 'jd',
    template: overrides.template ?? 'https://search.jd.com/Search?keyword={Query}',
    ...(overrides.iconDataUrl === undefined ? {} : { iconDataUrl: overrides.iconDataUrl }),
    createdAt: overrides.createdAt ?? '2026-08-31T00:00:00Z',
    updatedAt: overrides.updatedAt ?? '2026-08-31T00:00:00Z',
  }
}

function fileItem(
  fullPath = String.raw`C:\Private\UiPilot.txt`,
  resultId = 'file-result-1',
  modifiedUtc = '2026-07-22T00:00:00.000Z',
): FileResultItem {
  const segments = fullPath.split('\\')
  return {
    resultId,
    name: segments[segments.length - 1]!,
    kind: 'file',
    sizeBytes: '42',
    modifiedUtc,
    fullPath,
  }
}

function folderItem(fullPath = String.raw`C:\Private\Reports`, resultId = 'folder-result-1'): FileResultItem {
  return {
    ...fileItem(fullPath, resultId),
    kind: 'folder',
    sizeBytes: null,
  }
}

function fileResponse(
  revision: string,
  items: FileResultItem[] = [fileItem()],
  status: FileSearchResponse['status'] = 'ready',
): FileSearchResponse {
  return {
    requestId: `file-request-${revision}`,
    indexRevision: revision,
    total: String(items.length),
    status,
    items,
  }
}

function shown(invocationId: string, target: LauncherShown['target'] = 'launcher', notice: LauncherShown['notice'] = null) {
  return { invocationId, target, notice }
}

function messageCenterSnapshot(revision: string, unreadCount: number, contents: string[] = []) {
  return {
    revision,
    unreadCount,
    messages: contents.map((content, index) => ({
      id: String(index + 1),
      pluginId: 'com.uipilot.demo-win',
      pluginNameSnapshot: 'Demo Window',
      pluginIconUrl: null,
      createdAt: '2026-08-19T01:02:03.000Z',
      content,
      readAt: null,
    })),
  }
}

function installMatchMedia(initial: boolean) {
  let matches = initial
  let listener: ((event: MediaQueryListEvent) => void) | undefined
  const add = vi.fn((_type: 'change', next: (event: MediaQueryListEvent) => void) => {
    listener = next
  })
  const remove = vi.fn((_type: 'change', removed: (event: MediaQueryListEvent) => void) => {
    if (listener === removed) listener = undefined
  })
  const media = '(prefers-color-scheme: dark)'
  const primary = {
    get matches() {
      return matches
    },
    media,
    addEventListener: add,
    removeEventListener: remove,
  } as unknown as MediaQueryList
  let calls = 0
  const matchMedia = vi.fn((query: string) => {
    calls += 1
    if (calls === 1) return primary
    return {
      matches: initial,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    } as unknown as MediaQueryList
  })
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: matchMedia })
  return {
    add,
    remove,
    matchMedia,
    emit(next: boolean) {
      matches = next
      listener?.({ matches, media } as MediaQueryListEvent)
    },
  }
}

async function mountLauncherView(core: ReturnType<typeof createLauncherCore>, setSettingsPinned?: (active: boolean, pinned: boolean) => Promise<void>) {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  const onReady = vi.fn()
  await act(async () => root.render(<LauncherView core={core} onReady={onReady} setSettingsPinned={setSettingsPinned} />))
  return {
    host,
    onReady,
    async unmount() {
      await act(async () => root.unmount())
      host.remove()
    },
  }
}

function settingsTab(host: HTMLElement, label: '通用' | '消息' | '插件' | '权限' | '密钥配置'): HTMLElement {
  const tab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find(
    (candidate) => label === '消息'
      ? candidate.querySelector('.settings-message-tab-badge') !== null
      : candidate.textContent?.trim().endsWith(label),
  )
  if (!tab) throw new Error(`settings tab missing: ${label}`)
  return tab
}

async function activateSettingsTab(host: HTMLElement, label: '通用' | '消息' | '插件' | '权限' | '密钥配置'): Promise<HTMLElement> {
  const tab = settingsTab(host, label)
  await act(async () => {
    tab.focus()
    tab.click()
  })
  await vi.waitFor(() => expect(tab.getAttribute('aria-selected')).toBe('true'))
  return tab
}

async function startedCore(settings: SettingsView = emptySettings) {
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settings)
  const core = createLauncherCore(fake.client)
  await core.start()
  return { core, ...fake }
}

async function startedSettingsCore() {
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
  const core = createLauncherCore(fake.client)
  await core.start()
  fake.emit(shown('settings-r3', 'settings'))
  await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))
  return { core, ...fake }
}

async function startedFileView(items: FileResultItem[] = [fileItem()]) {
  const fake = fakeClient()
  vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1', items))
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('file-panel')))
  const control = core.getSnapshot().queryControl
  await act(async () =>
    core.text({ kind: 'ordinaryInput', control, value: '/find quarterly', inputType: 'insertText' }),
  )
  await act(async () => core.keyDown('Enter', false))
  await vi.waitFor(() => expect(core.getSnapshot().file?.results.length).toBe(items.length))
  return { core, mounted, ...fake }
}

type R3TextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'compositionBoundary'; control: ControlKey }

function r3(record: R3TextRecord): ClassifiedTextRecord {
  return record
}

describe('protocol and cached store', () => {
  it('strictly parses only the frozen launcher shown shape', () => {
    for (const target of ['launcher', 'settings', 'messages'] as const) {
      for (const notice of [null, 'settingsFailed'] as const) {
        const value = shown('invocation', target, notice)
        expect(parseLauncherShown(value)).toEqual(value)
      }
    }

    for (const value of [
      null,
      [],
      {},
      { ...shown('x'), extra: true },
      { ...shown('x'), notice: 'validationFailed' },
      { invocationId: 'x', target: 'launcher' },
      { invocationId: 7, target: 'launcher', notice: null },
      { invocationId: 'x', target: 'other', notice: null },
      { invocationId: 'x', target: 'launcher', notice: undefined },
      Object.create(shown('inherited')),
      Object.assign(Object.create({ inherited: true }), shown('own-fields')),
    ]) {
      expect(parseLauncherShown(value)).toBeNull()
    }
  })

  it('keeps stable store functions and publishes one immutable snapshot per mutation', async () => {
    const { core, emit } = await startedCore()
    const initial = core.getSnapshot()
    expect(core.getSnapshot()).toBe(initial)
    expect(core.getSnapshot).toBe(core.getSnapshot)
    expect(core.subscribe).toBe(core.subscribe)

    const listener = vi.fn()
    const unsubscribe = core.subscribe(listener)
    emit({ ...shown('bad'), extra: true })
    expect(core.getSnapshot()).toBe(initial)
    expect(listener).not.toHaveBeenCalled()

    emit(shown('one'))
    const next = core.getSnapshot()
    expect(next).not.toBe(initial)
    expect(Object.isFrozen(next)).toBe(true)
    expect(Object.isFrozen(next.results)).toBe(true)
    expect(listener).toHaveBeenCalledTimes(1)

    core.retireControl(999)
    unsubscribe()
    unsubscribe()
    emit(shown('two'))
    expect(listener).toHaveBeenCalledTimes(1)
  })
})

describe('startup ownership', () => {
  it('installs the listener before loading settings and accepts shown while load is pending', async () => {
    const fake = fakeClient()
    const load = deferred<SettingsView>()
    const order: string[] = []
    vi.mocked(fake.client.listenShown).mockImplementationOnce(async (handler) => {
      order.push('listen')
      const unlisten = vi.fn()
      ;(fake as unknown as { emit: (payload: unknown) => void }).emit = handler
      return unlisten
    })
    vi.mocked(fake.client.loadSettings).mockImplementationOnce(() => {
      order.push('load')
      return load.promise
    })
    const core = createLauncherCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(order).toEqual(['listen', 'load']))
    fake.emit(shown('during-load', 'settings'))
    expect(core.getSnapshot().view).toBe('settings')
    load.resolve(emptySettings)
    await start
  })

  it('queues one current settings load while startup hydration owns the operation', async () => {
    const fake = fakeClient()
    const initial = deferred<SettingsView>()
    const retry = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(initial.promise).mockReturnValueOnce(retry.promise)
    const core = createLauncherCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
    fake.emit(shown('startup-settings', 'settings'))

    const blockedReload = core.reloadSettings()
    await Promise.resolve()
    expect(fake.client.loadSettings).toHaveBeenCalledOnce()
    await blockedReload

    initial.reject({ code: 'settingsFailed', message: 'private' })
    await start
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().status).toBe('')
    retry.resolve({ ...settingsFixture, autostart: true })
    await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(true))
    expect(core.getSnapshot().status).toBe('')
  })

  it('does not load after listener failure and exposes only fixed local text', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.listenShown).mockRejectedValueOnce(new Error('secret listener failure'))
    const core = createLauncherCore(fake.client)
    const listener = vi.fn()
    core.subscribe(listener)
    await core.start()
    expect(fake.client.loadSettings).not.toHaveBeenCalled()
    expect(core.getSnapshot().status).toBe('操作不可用，请重试。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('secret')
    expect(listener).toHaveBeenCalledTimes(1)
  })

  it('unlistens a late registration after destroy and never loads', async () => {
    const fake = fakeClient()
    const registration = deferred<() => void>()
    vi.mocked(fake.client.listenPluginPanelFocusHostInput).mockReturnValueOnce(registration.promise)
    const lateUnlisten = vi.fn()
    const core = createLauncherCore(fake.client)
    const start = core.start()
    core.destroy()
    core.destroy()
    registration.resolve(lateUnlisten)
    await start
    expect(lateUnlisten).toHaveBeenCalledTimes(1)
    expect(fake.client.loadSettings).not.toHaveBeenCalled()
  })

  it('keeps launcher search usable after settings load fails', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private' })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('launcher'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    expect(fake.client.searchApps).toHaveBeenCalledWith({ query: 'calc', invocationId: 'launcher', querySequence: 2 })
  })
})

describe('shown and search ownership', () => {
  it('requests and selects the backend empty capability snapshot on native show', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: `empty-${request.invocationId}-${request.querySequence}`,
      items: [
        {
          resultId: 'find-completion',
          title: '/find',
          activation: { kind: 'completion', completionText: '/find ' },
          hasDefaultAction: false,
        },
        {
          resultId: 'web-completion',
          title: '/web-search',
          activation: { kind: 'completion', completionText: '/web-search ' },
          hasDefaultAction: false,
        },
      ],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()

    fake.emit(shown('native-empty'))

    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith({
      query: '', invocationId: 'native-empty', querySequence: 1,
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results.map((item) => item.title)).toEqual([
      '/find', '/web-search',
    ]))
    expect(core.getSnapshot().selectedIndex).toBe(0)
  })

  it('queries empty classification for clear and whitespace edits with increasing ownership', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: `request-${request.querySequence}`,
      items: [{
        resultId: `row-${request.querySequence}`,
        title: request.query.trim() === '' ? '/find' : `Result ${request.query}`,
        activation: request.query.trim() === ''
          ? { kind: 'completion', completionText: '/find ' }
          : executeActivation,
      }],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('empty-edits'))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledTimes(1))
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'alpha', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.title).toBe('Result alpha'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '', inputType: 'deleteContentBackward' })
    expect(core.getSnapshot().results).toEqual([])
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.title).toBe('/find'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '   ', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(vi.mocked(fake.client.searchApps).mock.calls.map(([request]) => ({
      query: request.query, sequence: request.querySequence,
    }))).toEqual([
      { query: '', sequence: 1 },
      { query: 'alpha', sequence: 2 },
      { query: '', sequence: 3 },
      { query: '   ', sequence: 4 },
    ])
  })

  it('keeps local navigation on one invocation and resets only a native re-show', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValue({ requestId: 'navigation', items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('first-native'))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledTimes(1))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'alpha', inputType: 'insertText' })
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledTimes(2))

    core.navigate('settings')
    core.navigate('launcher')
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledTimes(3))
    expect(fake.client.searchApps).toHaveBeenLastCalledWith({
      query: 'alpha', invocationId: 'first-native', querySequence: 3,
    })

    fake.emit(shown('second-native'))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledTimes(4))
    expect(fake.client.searchApps).toHaveBeenLastCalledWith({
      query: '', invocationId: 'second-native', querySequence: 1,
    })
  })

  it('executes a fast submitted web-search response exactly once', async () => {
    const fake = fakeClient()
    vi.useFakeTimers()
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => {
        if (request.query === '') return { requestId: 'empty', items: [] }
        return {
          requestId: 'submitted-web',
          autoExecuteResultId: 'web-result',
          items: [{
            resultId: 'web-result',
            title: 'Bing 搜索',
            activation: executeActivation,
            hasDefaultAction: true,
          }],
        }
      })
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('fast-web'))
      await vi.runAllTicks()
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/web-search UiPilot', inputType: 'insertText',
      })

      core.keyDown('Enter', false)
      await vi.runAllTicks()
      await Promise.resolve()

      expect(fake.client.searchApps).toHaveBeenCalledWith({
        query: '/web-search UiPilot', invocationId: 'fast-web', querySequence: 3, submit: true,
      })
      expect(fake.client.executeResult).toHaveBeenCalledOnce()
      expect(fake.client.executeResult).toHaveBeenCalledWith({
        requestId: 'submitted-web', resultId: 'web-result',
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('auto-executes the submitted result selected by the backend response', async () => {
    const fake = fakeClient()
    vi.useFakeTimers()
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => {
        if (request.query === '') return { requestId: 'empty', items: [] }
        return {
          requestId: 'submitted-quicklink',
          autoExecuteResultId: 'quicklink-result',
          items: [
            {
              resultId: 'quicklink-result',
              title: '/jd',
              activation: executeActivation,
              hasDefaultAction: true,
            },
          ],
        }
      })
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('fast-quicklink'))
      await vi.runAllTicks()
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/jd 手机', inputType: 'insertText',
      })

      core.keyDown('Enter', false)
      await vi.runAllTicks()
      await Promise.resolve()

      expect(fake.client.searchApps).toHaveBeenCalledWith({
        query: '/jd 手机', invocationId: 'fast-quicklink', querySequence: 3, submit: true,
      })
      expect(fake.client.executeResult).toHaveBeenCalledOnce()
      expect(fake.client.executeResult).toHaveBeenCalledWith({
        requestId: 'submitted-quicklink', resultId: 'quicklink-result',
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('opens the built-in quicklinks panel from a submitted /quicklinks result', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView()],
    })

    emit(shown('quicklinks-open'))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '/quicklinks', inputType: 'insertText',
    })
    core.keyDown('Enter', false)

    await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))
    expect(client.listQuicklinks).toHaveBeenCalledOnce()
    expect(core.getSnapshot().quicklinks?.items.map((item) => item.command)).toEqual(['jd'])
    expect(core.getSnapshot().quicklinks?.selectedId).toBeUndefined()
    expect(core.getSnapshot().quicklinks?.draftActive).toBe(false)
    expect(core.getSnapshot().query).toBe('')
    expect(core.getSnapshot().results).toHaveLength(0)
  })

  it('completes a focused quicklink command back into the launcher input', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView()],
    })

    emit(shown('quicklinks-complete'))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '/quicklinks', inputType: 'insertText',
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

    core.completeQuicklink('quicklink-jd')

    expect(core.getSnapshot().quicklinks).toBeUndefined()
    expect(core.getSnapshot().queryControlValue).toBe('/jd ')
  })

  it('filters quicklinks by typing in the quicklinks panel input without app searching', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [
        quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' }),
        quicklinkView({ id: 'quicklink-github', name: 'GitHub 搜索', command: 'gh' }),
      ],
    })

    emit(shown('quicklinks-filter'))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '/quicklinks', inputType: 'insertText',
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))
    vi.mocked(client.searchApps).mockClear()

    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '京', inputType: 'insertText',
    })

    expect(core.getSnapshot().queryControlValue).toBe('京')
    expect(core.getSnapshot().quicklinks?.items.map((item) => item.command)).toEqual(['jd'])
    expect(client.searchApps).not.toHaveBeenCalled()
  })

  it('focuses the quicklinks panel input and renders empty placeholders on both sides', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({ items: [] })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-empty')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')
      expect(input).not.toBeNull()
      expect(input?.placeholder).toBe('搜索快速链接')
      expect(document.activeElement).toBe(input)
      expect(mounted.host.textContent).toContain('暂无快速链接')
      expect(mounted.host.textContent).toContain('请选择或新增快速链接')
      expect(mounted.host.textContent).not.toContain('目录名称')
    } finally {
      await mounted.unmount()
    }
  })

  it('closes the quicklinks tag only for non-composing Backspace at filter caret zero', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView()],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-backspace')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!
      expect(input).not.toBeNull()
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'jd', inputType: 'insertText',
      }))
      input.setSelectionRange(2, 2)
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true })))
      expect(core.getSnapshot().quicklinks).toBeDefined()

      input.setSelectionRange(0, 0)
      await act(async () => input.dispatchEvent(new KeyboardEvent(
        'keydown', { key: 'Backspace', bubbles: true, isComposing: true },
      )))
      expect(core.getSnapshot().quicklinks).toBeDefined()

      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '', inputType: 'deleteContentBackward',
      }))
      input.setSelectionRange(0, 0)
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true })))
      expect(core.getSnapshot().quicklinks).toBeUndefined()
      expect(core.getSnapshot().queryControlValue).toBe('')
    } finally {
      await mounted.unmount()
    }
  })

  it('moves focus from the quicklinks panel input to the selected directory item and back with Ctrl+F', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [
        quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' }),
        quicklinkView({ id: 'quicklink-github', name: 'GitHub 搜索', command: 'gh' }),
      ],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-arrow-selection')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))
      expect(core.getSnapshot().quicklinks?.selectedId).toBeUndefined()

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!
      const arrowDown = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
      await act(async () => input.dispatchEvent(arrowDown))

      expect(arrowDown.defaultPrevented).toBe(true)
      expect(core.getSnapshot().quicklinks?.selectedId).toBe('quicklink-jd')
      expect(document.activeElement).toBe(mounted.host.querySelector('.quicklink-item.is-selected'))
      expect(document.activeElement?.textContent).toContain('/jd')

      const focusSearch = new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true, cancelable: true })
      await act(async () => document.activeElement?.dispatchEvent(focusSearch))

      expect(focusSearch.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(input)
    } finally {
      await mounted.unmount()
    }
  })

  it('moves directory selection with vertical arrows and completes the focused quicklink with Enter', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [
        quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' }),
        quicklinkView({ id: 'quicklink-github', name: 'GitHub 搜索', command: 'gh' }),
      ],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-directory-keyboard')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })))
      const firstItem = mounted.host.querySelector<HTMLButtonElement>('.quicklink-item.is-selected')!
      expect(document.activeElement).toBe(firstItem)

      const arrowDown = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
      await act(async () => firstItem.dispatchEvent(arrowDown))

      expect(arrowDown.defaultPrevented).toBe(true)
      expect(core.getSnapshot().quicklinks?.selectedId).toBe('quicklink-github')
      expect(document.activeElement).toBe(mounted.host.querySelector('.quicklink-item.is-selected'))
      expect(mounted.host.querySelector('.quicklink-item.is-selected code')?.textContent).toBe('/gh')
      expect(mounted.host.querySelector<HTMLInputElement>('.quicklinks-editor [aria-label="启动键"]')?.value).toBe('gh')

      const arrowUp = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })
      await act(async () => document.activeElement?.dispatchEvent(arrowUp))

      expect(arrowUp.defaultPrevented).toBe(true)
      expect(core.getSnapshot().quicklinks?.selectedId).toBe('quicklink-jd')
      expect(document.activeElement).toBe(mounted.host.querySelector('.quicklink-item.is-selected'))
      expect(mounted.host.querySelector('.quicklink-item.is-selected code')?.textContent).toBe('/jd')
      expect(mounted.host.querySelector<HTMLInputElement>('.quicklinks-editor [aria-label="启动键"]')?.value).toBe('jd')

      const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
      await act(async () => document.activeElement?.dispatchEvent(enter))

      expect(enter.defaultPrevented).toBe(true)
      expect(core.getSnapshot().quicklinks).toBeUndefined()
      expect(core.getSnapshot().queryControlValue).toBe('/jd ')
    } finally {
      await mounted.unmount()
    }
  })

  it('moves from a directory item to the form with ArrowRight and restores item focus after Ctrl+S saves', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [
        quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' }),
        quicklinkView({ id: 'quicklink-github', name: 'GitHub 搜索', command: 'gh' }),
      ],
    })
    const save = deferred<Awaited<ReturnType<typeof client.saveQuicklink>>>()
    vi.mocked(client.saveQuicklink).mockReturnValueOnce(save.promise)
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-editor-keyboard')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!
      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })))
      const item = mounted.host.querySelector<HTMLButtonElement>('.quicklink-item.is-selected')!
      expect(document.activeElement).toBe(item)

      const arrowRight = new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true })
      await act(async () => item.dispatchEvent(arrowRight))

      expect(arrowRight.defaultPrevented).toBe(true)
      const nameInput = mounted.host.querySelector<HTMLInputElement>('.quicklinks-editor [aria-label="目录名称"]')!
      expect(document.activeElement).toBe(nameInput)

      await act(async () => core.setQuicklinkDraft('name', '京东商城'))
      const ctrlS = new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true, cancelable: true })
      await act(async () => nameInput.dispatchEvent(ctrlS))

      expect(ctrlS.defaultPrevented).toBe(true)
      expect(client.saveQuicklink).toHaveBeenCalledWith({
        input: {
          id: 'quicklink-jd',
          name: '京东商城',
          command: 'jd',
          template: 'https://search.jd.com/Search?keyword={Query}',
          iconToken: null,
        },
      })
      await act(async () => {
        save.resolve(quicklinkView({ id: 'quicklink-jd', name: '京东商城', command: 'jd' }))
        await save.promise
      })
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.operation).toBeUndefined())
      await vi.waitFor(() => expect(document.activeElement).toBe(mounted.host.querySelector('.quicklink-item.is-selected')))
      await vi.waitFor(() => expect(mounted.host.querySelector('.quicklinks-editor-status')?.textContent).toBe('已保存'))
      expect(document.activeElement?.textContent).toContain('/jd')
    } finally {
      await mounted.unmount()
    }
  })

  it('keeps quicklinks chrome outside Tab order and traps Tab inside the right form', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' })],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-tab-scope')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!
      const addButton = mounted.host.querySelector<HTMLButtonElement>('[aria-label="新增快速链接"]')!
      const item = mounted.host.querySelector<HTMLButtonElement>('.quicklink-item')!
      expect(input.tabIndex).toBe(-1)
      expect(addButton.tabIndex).toBe(-1)
      expect(item.tabIndex).toBe(-1)

      const inputTab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
      await act(async () => input.dispatchEvent(inputTab))
      expect(inputTab.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(input)

      await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })))
      expect(document.activeElement).toBe(item)
      const itemTab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
      await act(async () => item.dispatchEvent(itemTab))
      expect(itemTab.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(item)

      await act(async () => item.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true })))
      const formControls = Array.from(
        mounted.host.querySelectorAll<HTMLElement>('.quicklinks-editor input:not(:disabled), .quicklinks-editor button:not(:disabled)'),
      )
      expect(formControls.length).toBeGreaterThan(1)
      const firstControl = formControls[0]!
      const lastControl = formControls[formControls.length - 1]!
      expect(document.activeElement).toBe(firstControl)

      await act(async () => lastControl.focus())
      const wrapForward = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
      await act(async () => lastControl.dispatchEvent(wrapForward))
      expect(wrapForward.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(firstControl)

      const wrapBackward = new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true })
      await act(async () => firstControl.dispatchEvent(wrapBackward))
      expect(wrapBackward.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(lastControl)
    } finally {
      await mounted.unmount()
    }
  })

  it('renders quicklinks with notes-like scroll shells and compact horizontal form rows', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView({ id: 'quicklink-jd', name: '京东搜索', command: 'jd' })],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-notes-like-layout')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      expect(mounted.host.querySelector('.quicklinks-list-scroll')).not.toBeNull()
      expect(mounted.host.querySelector('.quicklinks-editor-pane.is-empty')).not.toBeNull()
      expect(mounted.host.textContent).toContain('请选择或新增快速链接')

      await act(async () => core.selectQuicklink('quicklink-jd'))

      expect(mounted.host.querySelector('.quicklinks-editor-scroll')).not.toBeNull()
      expect(mounted.host.querySelector('.quicklinks-editor-pane')).not.toBeNull()
      expect(mounted.host.querySelector('.quicklinks-editor-header')?.textContent).toContain('京东搜索')
      expect(mounted.host.querySelector('.quicklinks-editor-heading code')?.textContent).toBe('/jd')
      expect(mounted.host.querySelectorAll('.quicklinks-editor .quicklinks-form-row')).toHaveLength(4)
      expect(mounted.host.querySelector('.quicklinks-editor .quicklinks-form-row-icon')).not.toBeNull()
      expect(stylesSource).toContain('.quicklinks-editor .quicklinks-form-row .ant-form-item-row')
      expect(stylesSource).toContain('grid-template-columns: 76px minmax(0, 1fr);')
      expect(stylesSource).toContain('.quicklinks-list-scroll')
      expect(stylesSource).toContain('.quicklinks-editor-scroll')
      expect(stylesSource).toContain('padding: 6px 14px 6px 6px;')
      expect(stylesSource).toContain('grid-template-rows: 48px minmax(0, 1fr);')
      expect(stylesSource).toContain('box-shadow: inset 0 0 0 1px')
    } finally {
      await mounted.unmount()
    }
  })

  it('opens the quicklink creation dialog from Ctrl+N without replacing the selected editor', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({
      items: [quicklinkView()],
    })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-ctrl-new')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))
      await act(async () => core.selectQuicklink('quicklink-jd'))
      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="搜索快速链接目录"]')!

      const ctrlN = new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true })
      await act(async () => input.dispatchEvent(ctrlN))

      expect(ctrlN.defaultPrevented).toBe(true)
      await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')?.textContent).toContain('新建快速链接'))
      expect(mounted.host.querySelector<HTMLInputElement>('[aria-label="目录名称"]')?.value).toBe('京东搜索')
    } finally {
      await mounted.unmount()
    }
  })

  it('opens the quicklink creation dialog from the add button while the empty editor stays empty', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({ items: [] })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-add-new')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))

      await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="新增快速链接"]')?.click())

      await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')?.textContent).toContain('新建快速链接'))
      const editor = mounted.host.querySelector<HTMLElement>('.quicklinks-editor-empty')!
      expect(editor.textContent).toContain('请选择或新增快速链接')
      expect(editor.textContent).not.toContain('目录名称')
    } finally {
      await mounted.unmount()
    }
  })

  it('saves a new quicklink from the creation dialog and selects it', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/quicklinks') return { requestId: 'empty', items: [] }
      return {
        requestId: 'quicklinks-open',
        items: [{
          resultId: 'quicklinks-result',
          title: '/quicklinks',
          activation: { kind: 'openQuicklinks' },
          hasDefaultAction: false,
        }],
      }
    })
    vi.mocked(client.listQuicklinks).mockResolvedValueOnce({ items: [] })
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('quicklinks-save-new')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().quicklinks?.status).toBe('ready'))
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="新增快速链接"]')?.click())
      await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')?.textContent).toContain('新建快速链接'))

      const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
      const setInputValue = async (selector: string, value: string) => {
        const input = dialog.querySelector<HTMLInputElement>(selector)!
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
        await act(async () => {
          setter.call(input, value)
          input.dispatchEvent(new Event('input', { bubbles: true }))
          input.dispatchEvent(new Event('change', { bubbles: true }))
        })
      }
      await setInputValue('[aria-label="目录名称"]', '淘宝搜索')
      await setInputValue('[aria-label="启动键"]', 'tb')
      await setInputValue('[aria-label="链接"]', 'https://s.taobao.com/search?q={Query}')
      await act(async () => {
        ;[...document.querySelectorAll<HTMLButtonElement>('button')]
          .find((button) => button.textContent?.trim() === '保存')
          ?.click()
      })

      await vi.waitFor(() => expect(client.saveQuicklink).toHaveBeenCalledWith({
        input: {
          name: '淘宝搜索',
          command: 'tb',
          template: 'https://s.taobao.com/search?q={Query}',
          iconToken: null,
        },
      }))
      await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
      expect(core.getSnapshot().quicklinks?.selectedId).toBe('quicklink-new')
      expect(core.getSnapshot().quicklinks?.draft.name).toBe('淘宝搜索')
    } finally {
      await mounted.unmount()
    }
  })

  it('accepts only the closed launcher activation union and completion grammar', () => {
    const boundary = `/d ${'a'.repeat(65_533)}`
    const valid = [
      { kind: 'completion', completionText: '/demo-win ' },
      { kind: 'completion', completionText: '/demo-win da  value' },
      { kind: 'completion', completionText: boundary },
      {
        kind: 'pluginCompletion',
        completionText: '/demo-win value',
        pluginId: 'com.uipilot.demo-win',
        favorite: true,
      },
      {
        kind: 'windowActivation', commandId: 'default',
        pluginId: 'com.uipilot.pomodoro',
        commandLabel: 'pomodoro',
        initialArgument: 'focus',
        favorite: true,
      },
      {
        kind: 'mainResultActivation',
        commandId: 'default',
        pluginId: 'com.uipilot.demo-return',
        commandLabel: 'demo-return',
        initialArgument: 'hello',
        favorite: false,
      },
      {
        kind: 'panelActivation', commandId: 'default',
        pluginId: 'com.uipilot.demo-panel',
        initialArgument: 'hello',
        favorite: true,
      },
      { kind: 'openFind', query: ' da  value ' },
      { kind: 'openQuicklinks' },
      { kind: 'executeResult' },
    ]
    for (const activation of valid) expect(safeLauncherActivation(activation)).toEqual(activation)

    const invalid = [
      undefined,
      { kind: 'unknown' },
      { kind: 'executeResult', query: 'borrowed' },
      { kind: 'openFind' },
      { kind: 'openFind', query: 'query', completionText: '/demo-win ' },
      { kind: 'openQuicklinks', query: 'borrowed' },
      { kind: 'completion' },
      { kind: 'completion', completionText: '/demo-win' },
      { kind: 'completion', completionText: '/demo-win  da' },
      { kind: 'completion', completionText: '/demo-win da ' },
      { kind: 'completion', completionText: `/demo-win ${'a'.repeat(65_527)}` },
      { kind: 'completion', completionText: '/demo-win da\0value' },
      { kind: 'completion', completionText: '/demo-win da\rvalue' },
      { kind: 'completion', completionText: '/demo-win da\nvalue' },
      { kind: 'completion', completionText: '/demo-win da\u2028value' },
      { kind: 'completion', completionText: '/demo-win da\u2029value' },
      { kind: 'completion', completionText: '/demo-win da\u0085value' },
      { kind: 'pluginCompletion', completionText: '/demo-win value', pluginId: 'Invalid Plugin', favorite: true },
      { kind: 'pluginCompletion', completionText: '/demo-win value', pluginId: 'com.uipilot.demo-win' },
      { kind: 'pluginCompletion', completionText: '/demo-win value', pluginId: 'com.uipilot.demo-win', favorite: 1 },
      { kind: 'pluginCompletion', completionText: '/demo win ', pluginId: 'com.uipilot.demo-win', favorite: false },
      { kind: 'pluginCompletion', completionText: '/demo-win value', pluginId: 'com.uipilot.demo-win', favorite: true, extra: false },
      { kind: 'windowActivation', commandId: 'default', pluginId: 'Invalid Plugin', commandLabel: 'pomodoro', initialArgument: '', favorite: false },
      { kind: 'windowActivation', commandId: 'default', pluginId: 'com.uipilot.pomodoro', commandLabel: 'Pomodoro', initialArgument: '', favorite: false },
      { kind: 'windowActivation', commandId: 'default', pluginId: 'com.uipilot.pomodoro', commandLabel: 'pomodoro', initialArgument: ' bad', favorite: false },
      { kind: 'windowActivation', commandId: 'default', pluginId: 'com.uipilot.pomodoro', commandLabel: 'pomodoro', initialArgument: '', favorite: false, extra: true },
      { kind: 'mainResultActivation', pluginId: 'Invalid Plugin', commandLabel: 'demo-return', initialArgument: '', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'Demo-return', initialArgument: '', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'demo-return', initialArgument: ' bad', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'demo-return', initialArgument: '', favorite: false, extra: true },
      { kind: 'panelActivation', commandId: 'default', pluginId: 'Invalid Plugin', initialArgument: '', favorite: true },
      { kind: 'panelActivation', commandId: 'default', pluginId: 'com.uipilot.demo-panel', initialArgument: 'bad\narg', favorite: false },
      { kind: 'panelActivation', commandId: 'default', pluginId: 'com.uipilot.demo-panel', initialArgument: ' hello', favorite: false },
    ]
    for (const activation of invalid) expect(safeLauncherActivation(activation)).toBeUndefined()
  })

  it('accepts only the strict informational quick evaluation activation', () => {
    expect(safeLauncherActivation({ kind: 'informational' })).toEqual({ kind: 'informational' })
    expect(safeLauncherActivation({ kind: 'informational', query: 'borrowed' })).toBeUndefined()
    expect(safeLauncherActivation({ kind: 'Informational' })).toBeUndefined()
  })

  it('drops one malformed launcher activation while retaining valid sibling rows', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'activation-filtering',
      items: [
        { resultId: 'execute', title: 'Execute', activation: { kind: 'executeResult' } },
        {
          resultId: 'malformed',
          title: 'Malformed',
          activation: { kind: 'completion', completionText: '/demo win ' },
        },
        { resultId: 'find', title: 'Find', activation: { kind: 'openFind', query: 'probe' } },
        {
          resultId: 'completion',
          title: 'Complete',
          activation: { kind: 'completion', completionText: '/demo-win probe' },
        },
      ],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('activation-filtering'))

    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'probe',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Execute', 'Find', 'Complete'])
  })

  it('routes a backend openFind activation through the dedicated find transaction', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'backend-find-activation',
      items: [{
        resultId: 'non-executable-find',
        title: '/find',
        activation: { kind: 'openFind', query: 'windows' },
        hasDefaultAction: false,
      }],
      replaceLocalResults: true,
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('backend-find-activation'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'windows',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    core.keyDown('Enter', false)

    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
      query: 'windows',
      invocationId: 'backend-find-activation',
      querySequence: 2,
    }))
    expect(fake.client.executeResult).not.toHaveBeenCalled()
  })

  it('opens the catalog find row directly with an empty query', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'backend-empty-find-activation',
      items: [findLauncherItem('')],
      replaceLocalResults: true,
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('empty-find-activation'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    const querySequence = core.getSnapshot().querySequence
    core.keyDown('Enter', false)

    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
      query: '',
      invocationId: 'empty-find-activation',
      querySequence,
    }))
    expect(fake.client.executeResult).not.toHaveBeenCalled()
  })

  it('submits a direct window activation on the first list activation', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'window-catalog',
      items: [{
        resultId: 'pomodoro-window',
        title: '/pomodoro',
        activation: {
          kind: 'windowActivation', commandId: 'default',
          pluginId: 'com.uipilot.pomodoro',
          commandLabel: 'pomodoro',
          initialArgument: 'focus',
          favorite: true,
        },
        hasDefaultAction: false,
      }],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'window-dispatch',
      items: [],
      windowTransferToken: 'window-token',
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('window-direct'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'focus',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    core.activateResult(0)

    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenLastCalledWith({
      query: '/pomodoro focus',
      invocationId: 'window-direct',
      querySequence: 3,
      submit: true,
      completionOrigin: { phase: 'commit', pluginId: 'com.uipilot.pomodoro', commandId: 'default' },
    }))
    expect(core.getSnapshot().query).toBe('/pomodoro focus')
    expect(fake.client.searchApps).toHaveBeenCalledTimes(2)
  })

  it('refreshes unread messages whenever native shown reopens the main window', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    vi.mocked(fake.client.getMessageSummary).mockResolvedValueOnce({ revision: '1', unreadCount: 1 })

    fake.emit(shown('message-summary-recovery'))

    await vi.waitFor(() => expect(core.getSnapshot().messageCenter).toMatchObject({
      status: 'ready', unreadCount: 1, summaryRevision: '1',
    }))
    expect(fake.client.getMessageSummary).toHaveBeenCalledTimes(2)
  })

  it('drops malformed plugin completion metadata without an execution fallback', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'malformed-plugin-completion',
      items: [{
        resultId: 'malformed',
        title: '/demo win',
        subtitle: 'invalid completion',
        activation: { kind: 'completion', completionText: '/demo win ' },
        hasDefaultAction: false,
      }],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('malformed-plugin-completion'))

    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/demo',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledOnce())
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results).toEqual([])
    core.keyDown('Enter', false)
    expect(fake.client.executeResult).not.toHaveBeenCalled()
  })

  it('navigates between launcher and settings without hiding and refreshes the preserved launcher query', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.searchApps).mockResolvedValue({
      requestId: 'local-navigation-search',
      items: [findLauncherItem('calc'), { resultId: 'calculator', title: 'Calculator', activation: executeActivation }],
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('local-navigation'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'calc',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    vi.mocked(fake.client.searchApps).mockClear()
    vi.mocked(fake.client.loadSettings).mockClear()

    const navigationCore = core as typeof core & {
      navigate(target: 'launcher' | 'settings'): void
    }
    expect(navigationCore.navigate).toBeTypeOf('function')
    const launcherEpoch = core.getSnapshot().viewEpoch
    navigationCore.navigate('settings')

    expect(core.getSnapshot()).toMatchObject({
      view: 'settings',
      viewEpoch: launcherEpoch + 1,
      invocationId: 'local-navigation',
      query: 'calc',
      queryControlValue: 'calc',
      results: [],
      selectedIndex: -1,
    })
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))
    expect(fake.client.loadSettings).toHaveBeenCalledOnce()

    core.keyDown('Escape', false)

    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      viewEpoch: launcherEpoch + 2,
      invocationId: 'local-navigation',
      query: 'calc',
      queryControlValue: 'calc',
      querySequence: 3,
      searchPending: false,
    })
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith({
      query: 'calc',
      invocationId: 'local-navigation',
      querySequence: 3,
    }))
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
  })

  it('routes the messages target to its settings tab and marks read once per entry', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()

    fake.emit(shown('notification-click', 'messages'))
    expect(core.getSnapshot()).toMatchObject({
      view: 'settings',
      settingsTab: 'messages',
      messageCenter: { status: 'ready', unreadCount: 0 },
    })
    await vi.waitFor(() => expect(fake.client.openMessageCenter).toHaveBeenCalledOnce())

    core.navigate('messages')
    expect(fake.client.openMessageCenter).toHaveBeenCalledOnce()
    fake.emitMessageState({ status: 'ready', revision: '1', unreadCount: 1 })
    await vi.waitFor(() => expect(fake.client.readMessageCenter).toHaveBeenCalledOnce())
    expect(fake.client.openMessageCenter).toHaveBeenCalledOnce()
  })

  it('uses the exact native shown reset and empty-catalog search rules', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('first'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    vi.mocked(client.searchApps).mockClear()

    emit(shown('second', 'launcher', 'settingsFailed'))
    expect(core.getSnapshot()).toMatchObject({
      invocationId: 'second',
      query: '',
      queryControlValue: '',
      querySequence: 1,
      results: [],
      selectedIndex: -1,
      shownNotice: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
    })
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledWith({
      query: '', invocationId: 'second', querySequence: 1,
    }))

    vi.mocked(client.searchApps).mockClear()
    emit(shown('settings', 'settings'))
    expect(client.searchApps).not.toHaveBeenCalled()
  })

  it('keeps the default launcher list visible while refreshing after native show', async () => {
    const { core, client, emit } = await startedCore()
    const first = deferred<SearchResponse | null>()
    const second = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)

    emit(shown('first-default'))
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledWith({
      query: '', invocationId: 'first-default', querySequence: 1,
    }))
    first.resolve({
      requestId: 'first-default-request',
      items: [{ resultId: 'old-default', title: 'Old default', activation: executeActivation }],
    })
    await first.promise
    await vi.waitFor(() => expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Old default']))

    emit(shown('second-default'))
    expect(core.getSnapshot()).toMatchObject({
      invocationId: 'second-default',
      query: '',
      queryControlValue: '',
      querySequence: 1,
      searchPending: false,
    })
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Old default'])
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledWith({
      query: '', invocationId: 'second-default', querySequence: 1,
    }))
    expect(core.getSnapshot().searchPending).toBe(false)
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Old default'])

    second.resolve({
      requestId: 'second-default-request',
      items: [{ resultId: 'new-default', title: 'New default', activation: executeActivation }],
    })
    await second.promise
    await vi.waitFor(() => expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['New default']))
  })

  it('clears on empty, commits current results, wraps selection, and ignores stale completions', async () => {
    const { core, client, emit } = await startedCore()
    const first = deferred<SearchResponse | null>()
    const second = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    emit(shown('search'))

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'a', inputType: 'insertText' })
    const beforeSecond = core.getSnapshot()
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'ab', inputType: 'insertText' })
    expect(core.getSnapshot()).toMatchObject({
      query: 'ab',
      querySequence: 3,
      results: [],
      selectedIndex: -1,
      searchPending: true,
      status: '',
    })
    first.resolve({ requestId: 'old-request', items: [{ resultId: 'old', title: 'old', activation: executeActivation }] })
    await first.promise
    await Promise.resolve()
    expect(core.getSnapshot()).not.toBe(beforeSecond)
    expect(core.getSnapshot().results).toEqual([])

    second.resolve({
      requestId: 'request',
      items: [
        findLauncherItem('ab'),
        { resultId: 'one', title: 'One', activation: executeActivation },
        { resultId: 'two', title: 'Two', subtitle: 'Second', activation: executeActivation },
      ],
    })
    await second.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['/find', 'One', 'Two'])
    expect(core.getSnapshot().selectedIndex).toBe(0)
    core.keyDown('ArrowUp', false)
    expect(core.getSnapshot().selectedIndex).toBe(2)
    core.keyDown('ArrowDown', false)
    expect(core.getSnapshot().selectedIndex).toBe(0)

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '', inputType: 'deleteContentBackward' })
    expect(core.getSnapshot()).toMatchObject({ query: '', querySequence: 4, results: [], selectedIndex: -1, searchPending: true, status: '' })
  })

  it('replaces ordinary backend capabilities with an exclusive built-in result', async () => {
    const { core, client, emit } = await startedCore()
    const response = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(response.promise)
    emit(shown('math'))

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '1+1', inputType: 'insertText' })
    expect(core.getSnapshot().results).toEqual([])

    response.resolve({
      requestId: 'math-request',
      items: [{ resultId: 'math-result', title: '2', subtitle: '复制结果', hasDefaultAction: true, activation: executeActivation }],
      replaceLocalResults: true,
    })
    await response.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['2'])
    expect(core.getSnapshot().selectedIndex).toBe(0)
  })

  it('carries the exact currency icon kind and falls back for injected unknown values', async () => {
    const { core, client, emit } = await startedCore()
    const response = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(response.promise)
    emit(shown('result-icon-kinds'))

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'alpha', inputType: 'insertText' })
    expect(core.getSnapshot().results).toEqual([])

    response.resolve({
      requestId: 'icon-kinds',
      items: [
        findLauncherItem('alpha'),
        { resultId: 'quicklinks', title: '/quicklinks', iconKind: 'quicklinks', activation: { kind: 'openQuicklinks' } },
        { resultId: 'calculator', title: '2', iconKind: 'calculator', activation: executeActivation },
        { resultId: 'currency', title: '713.245 CNY', iconKind: 'currency', activation: executeActivation },
        { resultId: 'date', title: '2026-09-10', iconKind: 'date', activation: executeActivation },
        { resultId: 'unit', title: '39.3701 in', iconKind: 'unit', activation: executeActivation },
        { resultId: 'web', title: 'Bing 搜索', iconKind: 'webSearch', activation: executeActivation },
        { resultId: 'app', title: 'App', icon: 'data:image/png;base64,AA==', activation: executeActivation },
        {
          resultId: 'plugin',
          title: '/demo-win',
          activation: executeActivation,
          pluginIconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
        },
        { resultId: 'forged-plugin', title: 'Forged', pluginIconUrl: 'https://example.com/icon.png', activation: executeActivation },
        { resultId: 'unknown', title: 'Unknown', iconKind: 'unknown', activation: executeActivation },
      ],
    } as unknown as SearchResponse)
    await response.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results.map(({ title, iconKind, icon, pluginIconUrl }) => ({ title, iconKind, icon, pluginIconUrl }))).toEqual([
      { title: '/find', iconKind: 'find', icon: undefined, pluginIconUrl: undefined },
      { title: '/quicklinks', iconKind: 'quicklinks', icon: undefined, pluginIconUrl: undefined },
      { title: '2', iconKind: 'calculator', icon: undefined, pluginIconUrl: undefined },
      { title: '713.245 CNY', iconKind: 'currency', icon: undefined, pluginIconUrl: undefined },
      { title: '2026-09-10', iconKind: 'date', icon: undefined, pluginIconUrl: undefined },
      { title: '39.3701 in', iconKind: 'unit', icon: undefined, pluginIconUrl: undefined },
      { title: 'Bing 搜索', iconKind: 'webSearch', icon: undefined, pluginIconUrl: undefined },
      { title: 'App', iconKind: undefined, icon: 'data:image/png;base64,AA==', pluginIconUrl: undefined },
      {
        title: '/demo-win', iconKind: undefined, icon: undefined,
        pluginIconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
      },
      { title: 'Forged', iconKind: undefined, icon: undefined, pluginIconUrl: undefined },
      { title: 'Unknown', iconKind: undefined, icon: undefined, pluginIconUrl: undefined },
    ])
  })

  it('keeps unit conversion query and result ownership under rapid edits', async () => {
    const { core, client, emit } = await startedCore()
    const staleUnit = deferred<SearchResponse | null>()
    const currentOrdinary = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps)
      .mockReturnValueOnce(staleUnit.promise)
      .mockReturnValueOnce(currentOrdinary.promise)
    emit(shown('rapid-unit-edit'))

    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '1000 mm to in', inputType: 'insertText',
    })
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'notes', inputType: 'insertText',
    })
    currentOrdinary.resolve({
      requestId: 'current-ordinary-request',
      items: [{ resultId: 'current-ordinary-result', title: 'Notes', activation: executeActivation }],
    })
    await currentOrdinary.promise
    await vi.waitFor(() => expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Notes']))

    staleUnit.resolve({
      requestId: 'stale-unit-request',
      items: [{
        resultId: 'stale-unit-result',
        title: '39.3701 in',
        iconKind: 'unit',
        hasDefaultAction: true,
        activation: executeActivation,
      }],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    await staleUnit.promise
    await Promise.resolve()

    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['Notes'])
    core.keyDown('Enter', false)
    expect(client.executeResult).toHaveBeenCalledWith({
      requestId: 'current-ordinary-request',
      resultId: 'current-ordinary-result',
    })
    expect(client.executeResult).not.toHaveBeenCalledWith({
      requestId: 'stale-unit-request',
      resultId: 'stale-unit-result',
    })
  })

  it('retains only well-formed informational quick evaluation rows and non-empty execute ids', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'informational-filtering',
      items: [
        {
          resultId: '',
          title: '日期表达式无效',
          activation: { kind: 'informational' },
          hasDefaultAction: false,
        },
        {
          resultId: '',
          title: 'Missing flag',
          activation: { kind: 'informational' },
        },
        {
          resultId: '',
          title: 'Executable information',
          activation: { kind: 'informational' },
          hasDefaultAction: true,
        },
        {
          resultId: '',
          title: 'Malformed information',
          activation: { kind: 'informational', query: 'borrowed' },
          hasDefaultAction: false,
        },
        { resultId: '', title: 'Empty execute id', activation: executeActivation },
        { resultId: 'opaque-copy-id', title: '2026-09-10', activation: executeActivation },
      ],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    emit(shown('informational-filtering'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'today + 1 fortnight',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results.map((item) => item.title)).toEqual([
      '日期表达式无效',
      '2026-09-10',
    ])
    expect(core.getSnapshot().selectedIndex).toBe(0)
  })
  it('releases a current null without inventing status and leaves stale null zero-effect', async () => {
    const { core, client, emit } = await startedCore()
    const stale = deferred<SearchResponse | null>()
    const current = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(stale.promise).mockReturnValueOnce(current.promise)
    emit(shown('nulls'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'a', inputType: 'insertText' })
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'b', inputType: 'insertText' })
    const pending = core.getSnapshot()
    stale.resolve(null)
    await stale.promise
    await Promise.resolve()
    expect(core.getSnapshot()).toBe(pending)
    current.resolve(null)
    await current.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().status).toBe('')
  })

  it('debounces slash live edits, submits on first Enter, and keeps actionless results inert', async () => {
    const { core, client, emit } = await startedCore()
    vi.useFakeTimers()
    try {
      const live = deferred<SearchResponse | null>()
      const submit = deferred<SearchResponse | null>()
      vi.mocked(client.searchApps).mockReturnValueOnce(live.promise).mockReturnValueOnce(submit.promise)
      emit(shown('public-plugin'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/demo   I am  Jack  ', inputType: 'insertText' })

      expect(client.searchApps).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(149)
      expect(client.searchApps).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(1)
      expect(client.searchApps).toHaveBeenCalledWith({
        query: '/demo   I am  Jack  ',
        invocationId: 'public-plugin',
        querySequence: 2,
        submit: false,
      })
      live.resolve(null)
      await vi.runAllTicks()
      await Promise.resolve()

      core.keyDown('Enter', false)
      expect(client.searchApps).toHaveBeenLastCalledWith({
        query: '/demo   I am  Jack  ',
        invocationId: 'public-plugin',
        querySequence: 3,
        submit: true,
      })
      submit.resolve({
        requestId: 'public-request',
        items: [{ resultId: 'answer', title: 'Answer', detail: '<b>plain</b>', hasDefaultAction: false, activation: executeActivation }],
      })
      await submit.promise
      await Promise.resolve()
      expect(core.getSnapshot().results).toMatchObject([{ title: 'Answer', detail: '<b>plain</b>', hasDefaultAction: false }])
      core.keyDown('Enter', false)
      expect(client.executeResult).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('opens a panel activation in one Enter, owns the epoch before first submit, and preserves the initial argument', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: `panel-catalog-${request.querySequence}`,
      items: request.query === 'hello' ? [panelItem('hello')] : [],
    } as unknown as SearchResponse))
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('7'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: [],
    })
    vi.mocked(client.submitPluginPanel).mockImplementationOnce(async (input) => {
      expect(core.getSnapshot().panel).toMatchObject({
        pluginId: 'com.uipilot.demo-panel',
        sessionEpoch: '7',
        suffix: 'hello',
      })
      return {
        sessionEpoch: input.sessionEpoch,
        pluginId: 'com.uipilot.demo-panel',
        commandLabel: 'demo-panel',
        hostKeys: [],
      }
    })

    emit(shown('panel-list-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'hello',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    core.keyDown('Enter', false)

    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('7'))
    expect(client.openPluginPanel).toHaveBeenCalledWith({
      commandId: 'default', pluginId: 'com.uipilot.demo-panel',
      argument: 'hello',
    })
    expect(client.submitPluginPanel).toHaveBeenCalledWith({
      sessionEpoch: '7',
      argument: 'hello',
      uiIntentEpoch: 1,
    })
    expect(core.getSnapshot()).toMatchObject({ results: [], selectedIndex: -1 })
    expect(vi.mocked(client.openPluginPanel).mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(client.submitPluginPanel).mock.invocationCallOrder[0]!)
  })

  it.each(['success', 'failure'] as const)(
    'releases panel open busy state when its query owner becomes stale after %s',
    async (outcome) => {
      const { core, client, emit } = await startedCore()
      const opened = deferred<PluginPanelCommandResult>()
      vi.mocked(client.searchApps).mockImplementation(async (request) => ({
        requestId: `panel-stale-${request.querySequence}`,
        items: request.query === 'hello' ? [panelItem('hello')] : [],
      } as unknown as SearchResponse))
      vi.mocked(client.openPluginPanel).mockReturnValueOnce(opened.promise)

      emit(shown(`panel-stale-${outcome}`))
      await new Promise((resolve) => setTimeout(resolve, 0))
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'hello', inputType: 'insertText',
      })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
      expect(core.getSnapshot().executePending).toBe(true)

      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'changed', inputType: 'insertText',
      })
      if (outcome === 'success') {
        opened.resolve({
          sessionEpoch: u64('8'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
        })
      } else {
        opened.reject({ code: 'windowFailed' })
      }

      await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
      expect(core.getSnapshot()).toMatchObject({ query: 'changed', status: '未找到应用' })
      if (outcome === 'success') {
        expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '8' })
      } else {
        expect(client.closePluginPanel).not.toHaveBeenCalled()
      }
    },
  )

  it('submits a slash panel activation on the first Enter and keeps only the latest frontend owner', async () => {
    const { core, client, emit } = await startedCore()
    const submitA = deferred<PluginPanelCommandResult>()
    const submitB = deferred<PluginPanelCommandResult>()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-slash-result',
      items: [panelItem('seed')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('9'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: [],
    })
    vi.mocked(client.submitPluginPanel)
      .mockResolvedValueOnce({
        sessionEpoch: u64('9'),
        pluginId: 'com.uipilot.demo-panel',
        commandLabel: 'demo-panel',
        hostKeys: [],
      })
      .mockReturnValueOnce(submitA.promise)
      .mockReturnValueOnce(submitB.promise)

    emit(shown('panel-slash-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/demo-panel seed',
      inputType: 'insertText',
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('9'))

    const suffixControl = core.getSnapshot().panel!.suffixControl
    core.text({ kind: 'ordinaryInput', control: suffixControl, value: 'A', inputType: 'insertText' })
    core.keyDown('Enter', false)
    core.text({ kind: 'ordinaryInput', control: suffixControl, value: 'B', inputType: 'insertText' })
    core.keyDown('Enter', false)
    expect(client.submitPluginPanel).toHaveBeenNthCalledWith(2, {
      sessionEpoch: '9', argument: 'A', uiIntentEpoch: 2,
    })
    expect(client.submitPluginPanel).toHaveBeenNthCalledWith(3, {
      sessionEpoch: '9', argument: 'B', uiIntentEpoch: 3,
    })

    submitA.reject({ code: 'windowFailed' })
    await Promise.resolve()
    expect(core.getSnapshot().panel).toMatchObject({ sessionEpoch: '9', suffix: 'B' })
    expect(core.getSnapshot().status).toBe('')
    submitB.resolve({
      sessionEpoch: u64('9'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
    })
    await submitB.promise
    await vi.waitFor(() => expect(core.getSnapshot().panel?.submitPending).toBe(false))
  })

  it('discards stale panel errors and clears only the matching session epoch', async () => {
    const { core, client, emit, emitPanelError } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-error-result',
      items: [panelItem('')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('12'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: [],
    })
    emit(shown('panel-error-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '/demo-panel', inputType: 'insertText',
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('12'))

    emitPanelError({ sessionEpoch: '11' })
    expect(core.getSnapshot().panel?.sessionEpoch).toBe('12')
    emitPanelError({ sessionEpoch: '12' })
    expect(core.getSnapshot().panel).toBeUndefined()
    expect(core.getSnapshot().status).toBe('操作不可用，请重试。')
  })

  it('ignores stale panel-bound failures and closes only the matching current session', async () => {
    const { core, client, emit, emitPanelReset } = await startedCore()
    const staleBounds = deferred<void>()
    const currentBounds = deferred<void>()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-bounds-owner-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel)
      .mockResolvedValueOnce({
        sessionEpoch: u64('41'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
      })
      .mockResolvedValueOnce({
        sessionEpoch: u64('42'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
      })
    vi.mocked(client.setPluginPanelBounds)
      .mockReturnValueOnce(staleBounds.promise)
      .mockReturnValueOnce(currentBounds.promise)

    const openFromQuery = async (invocationId: string) => {
      emit(shown(invocationId))
      await new Promise((resolve) => setTimeout(resolve, 0))
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'hello', inputType: 'insertText',
      })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
    }
    const bounds = { x: 12, y: 64, width: 696, height: 320 }

    await openFromQuery('panel-bounds-owner-first')
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('41'))
    core.setPanelBounds({ sessionEpoch: u64('41'), bounds })
    emitPanelReset({ sessionEpoch: '41' })
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeUndefined())

    await openFromQuery('panel-bounds-owner-second')
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('42'))
    staleBounds.reject(new Error('stale session'))
    await Promise.resolve()
    expect(core.getSnapshot().panel?.sessionEpoch).toBe('42')
    expect(client.closePluginPanel).not.toHaveBeenCalled()

    core.setPanelBounds({ sessionEpoch: u64('41'), bounds })
    expect(client.setPluginPanelBounds).toHaveBeenCalledTimes(1)
    core.setPanelBounds({ sessionEpoch: u64('42'), bounds })
    currentBounds.reject({ message: 'Command set_plugin_panel_bounds not found' })
    await vi.waitFor(() => expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '42' }))
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeUndefined())
    expect(core.getSnapshot().shownNotice).toBe('Panel 布局同步失败（PANEL_BOUNDS_COMMAND_NOT_FOUND）。')
  })

  it('discards a panel after hide and starts the next shown invocation as a fresh launcher', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-hide-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('21'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
    })
    emit(shown('panel-hide-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'hello', inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('21'))

    await core.requestHide()
    expect(client.hideLauncher).toHaveBeenCalledOnce()
    expect(core.getSnapshot().panel).toBeUndefined()

    emit(shown('panel-hide-next'))
    expect(core.getSnapshot()).toMatchObject({
      invocationId: 'panel-hide-next', query: '', results: [],
    })
    expect(core.getSnapshot().panel).toBeUndefined()
  })

  it('keeps the panel footer and rejects stale or out-of-order status updates', async () => {
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const { core, client, emit, emitPanelStatus, emitPanelReset } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValue({ requestId: 'panel-status', items: [panelItem('')] } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({ sessionEpoch: u64('22'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [] })
    emit(shown('panel-status-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/demo-panel', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('22'))
    installMatchMedia(false)
    const mounted = await mountLauncherView(core)
    const send = (sessionEpoch: string, sequence: string, message: string) => act(() => emitPanelStatus({ sessionEpoch, sequence, status: { message, tone: 'success' } }))
    const footer = mounted.host.querySelector('.launcher-status-region')!
    const operations = footer.querySelector('.launcher-operations-trigger')
    expect(operations?.textContent).toContain('操作 Ctrl+K')
    expect(footer.querySelector('[role="status"]')?.textContent).toBe('')
    await send('22', '2', '保存成功')
    expect(footer.querySelector('[role="status"]')?.textContent).toBe('保存成功')
    expect(footer.querySelector('.launcher-operations-trigger')).toBe(operations)
    expect(operations?.textContent).toContain('操作 Ctrl+K')
    expect(mounted.host.querySelector('.launcher-status-region .status-region')?.classList.contains('is-action-success')).toBe(true)
    await send('21', '99', '旧面板')
    await send('22', '1', '乱序')
    expect(core.getSnapshot().panel?.feedback?.status.message).toBe('保存成功')
    await send('22', '3', '')
    expect(footer.querySelector('[role="status"]')?.textContent).toBe('')
    expect(mounted.host.querySelector('.launcher-status-region')?.textContent).toContain('Ctrl+K')
    await act(() => emitPanelStatus({ sessionEpoch: '22', sequence: '4', status: { message: '保存失败'.repeat(100), tone: 'error' } }))
    expect(footer.querySelector('[role="status"]')?.textContent).toBe('保存失败'.repeat(100))
    expect(operations?.textContent).toBe('操作 Ctrl+K')
    expect(footer.querySelector('.status-region')?.classList.contains('is-action-error')).toBe(true)
    await act(() => emitPanelReset({ sessionEpoch: '22' }))
    await send('22', '5', '已退出')
    expect(mounted.host.querySelector('.launcher-status-region')?.textContent).not.toContain('已退出')
    expect(stylesSource).not.toMatch(/is-panel-active[^{}]*>[^{}]*launcher-status-region[^{}]*\{[^}]*display:\s*none/)
    await mounted.unmount()
    core.destroy()
  })

  it('accepts only the current single-use operations ticket with an unchanged panel input', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    let receive: (payload: unknown) => void = () => {}
    client.listenPluginPanelActionsSubmit = vi.fn(async handler => { receive = handler; return vi.fn() })
    client.openPluginPanelActions = vi.fn(async () => {})
    vi.mocked(client.searchApps).mockResolvedValue({ requestId: 'menu-panel', items: [panelItem('')] } as unknown as SearchResponse)
    emit(shown('menu-panel'))
    await new Promise(resolve => setTimeout(resolve, 0))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/demo-panel', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeDefined())
    const mounted = await mountLauncherView(core)
    vi.mocked(client.submitPluginPanel).mockClear()
    try {
      const trigger = mounted.host.querySelector<HTMLButtonElement>('[aria-label="操作 Ctrl+K"]')!
      await act(async () => trigger.click())
      const first = vi.mocked(client.openPluginPanelActions).mock.lastCall![0]
      expect(first.source).toBe('hostInput')
      await act(async () => { receive({ ...first, ticket: 'wrong' }); receive({ ...first, sessionEpoch: '999' }) })
      expect(client.submitPluginPanel).not.toHaveBeenCalled()
      await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().panel!.suffixControl, value: 'updated', inputType: 'insertText' }))
      await act(async () => receive(first))
      expect(client.submitPluginPanel).not.toHaveBeenCalled()
      await act(async () => trigger.click())
      const second = vi.mocked(client.openPluginPanelActions).mock.lastCall![0]
      await act(async () => { receive(second); receive(second) })
      expect(client.submitPluginPanel).toHaveBeenCalledOnce()
    } finally { await mounted.unmount(); core.destroy() }
  })

  it('silently resets only the matching panel session after a host-driven plugin mutation', async () => {
    const { core, client, emit, emitPanelReset } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-reset-result', items: [panelItem('')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('22'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
    })
    emit(shown('panel-reset-entry'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: '/demo-panel', inputType: 'insertText',
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('22'))

    emitPanelReset({ sessionEpoch: '21' })
    expect(core.getSnapshot().panel?.sessionEpoch).toBe('22')
    emitPanelReset({ sessionEpoch: '22' })
    expect(core.getSnapshot().panel).toBeUndefined()
    expect(core.getSnapshot().status).toBe('')
  })

  it('closes the panel tag only for non-composing Backspace at suffix caret zero', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-dom-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('1'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      inputPlaceholder: '搜索目录',
      hostKeys: [],
    } as unknown as PluginPanelCommandResult)
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('panel-dom-entry')))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'hello', inputType: 'insertText',
      }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeDefined())
    const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!
    expect(input).not.toBeNull()
    expect([input.selectionStart, input.selectionEnd]).toEqual([5, 5])
    expect(input.placeholder).toBe('搜索目录')

    input.setSelectionRange(2, 2)
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true })))
    expect(client.closePluginPanel).not.toHaveBeenCalled()
    expect(core.getSnapshot().panel).toBeDefined()

    input.setSelectionRange(0, 2)
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true })))
    expect(client.closePluginPanel).not.toHaveBeenCalled()

    input.setSelectionRange(0, 0)
    await act(async () => input.dispatchEvent(new KeyboardEvent(
      'keydown', { key: 'Backspace', bubbles: true, isComposing: true },
    )))
    expect(client.closePluginPanel).not.toHaveBeenCalled()

    input.setSelectionRange(0, 0)
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true })))
    await vi.waitFor(() => expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '1' }))
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeUndefined())
    await mounted.unmount()
  })

  it('serializes declared Host keys and consumes queue-full client sequences', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-host-key-result',
      items: [panelItem('')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('31'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: ['ArrowDown', 'Primary+N'],
    })
    vi.mocked(client.submitPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('31'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys: ['ArrowDown', 'Primary+N'],
    })
    const first = deferred<{ outcome: 'droppedQueueFull' }>()
    vi.mocked(client.enqueuePluginPanelHostKey)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce({ outcome: 'enqueued', routeSequence: u64('1') })
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('panel-host-key-entry')))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await act(async () => core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'hello', inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('31'))
    const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!

    const down = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
    const primaryN = new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true })
    input.dispatchEvent(down)
    input.dispatchEvent(primaryN)
    expect(down.defaultPrevented).toBe(true)
    expect(primaryN.defaultPrevented).toBe(true)
    await vi.waitFor(() => expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(1))
    expect(client.enqueuePluginPanelHostKey).toHaveBeenNthCalledWith(1, {
      sessionEpoch: '31', clientSequence: '1', declaration: 'ArrowDown', key: 'ArrowDown',
      ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
    })

    first.resolve({ outcome: 'droppedQueueFull' })
    await vi.waitFor(() => expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(2))
    expect(client.enqueuePluginPanelHostKey).toHaveBeenNthCalledWith(2, {
      sessionEpoch: '31', clientSequence: '2', declaration: 'Primary+N', key: 'n',
      ctrlKey: true, metaKey: false, shiftKey: false, altKey: false,
    })

    const undeclaredDelete = new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true })
    input.dispatchEvent(undeclaredDelete)
    expect(undeclaredDelete.defaultPrevented).toBe(false)
    const undeclared = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })
    const shifted = new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true, cancelable: true })
    const composing = new KeyboardEvent('keydown', { key: 'ArrowDown', isComposing: true, bubbles: true, cancelable: true })
    input.dispatchEvent(undeclared)
    input.dispatchEvent(shifted)
    input.dispatchEvent(composing)
    expect(undeclared.defaultPrevented).toBe(false)
    expect(shifted.defaultPrevented).toBe(false)
    expect(composing.defaultPrevented).toBe(false)
    expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(2)
    await mounted.unmount()
  })

  it('routes Delete only for a declared empty Panel input and ignores IME modifiers and repeat', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    vi.mocked(client.searchApps).mockResolvedValue({ requestId: 'delete-key', items: [panelItem('')] } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('32'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: ['Delete'],
    })
    vi.mocked(client.submitPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('32'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: ['Delete'],
    })
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('delete-key')))
    await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'hello', inputType: 'insertText' }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('32'))
    const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!
    for (const extra of [{ isComposing: true }, { repeat: true }, { ctrlKey: true }, { altKey: true }, { shiftKey: true }, { metaKey: true }]) {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true, ...extra }))
    }
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(client.enqueuePluginPanelHostKey).not.toHaveBeenCalled()
    const del = new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true })
    input.dispatchEvent(del)
    expect(del.defaultPrevented).toBe(true)
    await vi.waitFor(() => expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(1))
    expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledWith(expect.objectContaining({ declaration: 'Delete', key: 'Delete' }))
    await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().panel!.suffixControl, value: 'text', inputType: 'insertText' }))
    const edit = new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true })
    input.dispatchEvent(edit)
    expect(edit.defaultPrevented).toBe(false)
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(1)
    await mounted.unmount()
  })

  it('routes declared Tab Shift+Tab and Enter Host keys without focus traversal or panel submit', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const hostKeys = ['Tab', 'Shift+Tab', 'Enter'] as const
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-extended-host-key-result',
      items: [panelItem('')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('32'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys,
    })
    vi.mocked(client.submitPluginPanel).mockResolvedValueOnce({
      sessionEpoch: u64('32'),
      pluginId: 'com.uipilot.demo-panel',
      commandLabel: 'demo-panel',
      hostKeys,
    })
    vi.mocked(client.enqueuePluginPanelHostKey).mockResolvedValue({ outcome: 'enqueued', routeSequence: u64('1') })
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('panel-extended-host-key-entry')))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await act(async () => core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'hello', inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('32'))
    await vi.waitFor(() => expect(client.submitPluginPanel).toHaveBeenCalledTimes(1))
    vi.mocked(client.submitPluginPanel).mockClear()
    const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!
    input.focus()

    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    const shiftTab = new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true })
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    const ctrlTab = new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, bubbles: true, cancelable: true })
    const altTab = new KeyboardEvent('keydown', { key: 'Tab', altKey: true, bubbles: true, cancelable: true })
    const composingEnter = new KeyboardEvent(
      'keydown',
      { key: 'Enter', isComposing: true, bubbles: true, cancelable: true },
    )

    input.dispatchEvent(ctrlTab)
    input.dispatchEvent(altTab)
    input.dispatchEvent(composingEnter)
    input.dispatchEvent(tab)
    input.dispatchEvent(shiftTab)
    input.dispatchEvent(enter)

    expect(ctrlTab.defaultPrevented).toBe(false)
    expect(altTab.defaultPrevented).toBe(false)
    expect(composingEnter.defaultPrevented).toBe(false)
    expect(tab.defaultPrevented).toBe(true)
    expect(shiftTab.defaultPrevented).toBe(true)
    expect(enter.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(input)
    await vi.waitFor(() => expect(client.enqueuePluginPanelHostKey).toHaveBeenCalledTimes(3))
    expect(client.enqueuePluginPanelHostKey).toHaveBeenNthCalledWith(1, {
      sessionEpoch: '32', clientSequence: '1', declaration: 'Tab', key: 'Tab',
      ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
    })
    expect(client.enqueuePluginPanelHostKey).toHaveBeenNthCalledWith(2, {
      sessionEpoch: '32', clientSequence: '2', declaration: 'Shift+Tab', key: 'Tab',
      ctrlKey: false, metaKey: false, shiftKey: true, altKey: false,
    })
    expect(client.enqueuePluginPanelHostKey).toHaveBeenNthCalledWith(3, {
      sessionEpoch: '32', clientSequence: '3', declaration: 'Enter', key: 'Enter',
      ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
    })
    expect(client.submitPluginPanel).not.toHaveBeenCalled()
    await mounted.unmount()
  })

  it('prevents native browser find across the main surface without stopping key propagation', async () => {
    const { core, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('browser-find-guard')))
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const bubbled = vi.fn()
    input.addEventListener('keydown', bubbled)
    const browserFind = new KeyboardEvent('keydown', {
      key: 'f', ctrlKey: true, bubbles: true, cancelable: true,
    })
    const shiftedFind = new KeyboardEvent('keydown', {
      key: 'f', ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true,
    })

    input.dispatchEvent(browserFind)
    input.dispatchEvent(shiftedFind)

    expect(browserFind.defaultPrevented).toBe(true)
    expect(shiftedFind.defaultPrevented).toBe(false)
    expect(bubbled).toHaveBeenCalledTimes(2)
    await mounted.unmount()
  })

  it('focuses only the current tagged panel input and acknowledges the exact ordered request', async () => {
    const { core, client, emit, emitPanelFocus } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-focus-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('panel-focus-entry')))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await act(async () => core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'hello', inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('1'))

    const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!
    const outside = document.createElement('button')
    document.body.append(outside)
    input.setSelectionRange(2, 2)
    outside.focus()
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '10' }))
    await vi.waitFor(() => expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledWith({
      sessionEpoch: '1', focusRequestId: '10', focused: true,
    }))
    expect(document.activeElement).toBe(input)
    expect(input.value).toBe('hello')
    expect([input.selectionStart, input.selectionEnd]).toEqual([2, 2])
    expect(core.getSnapshot().panel).toMatchObject({ sessionEpoch: '1', suffix: 'hello' })

    outside.focus()
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '10' }))
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '9' }))
    expect(document.activeElement).toBe(outside)
    expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(1)

    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '11' }))
    await vi.waitFor(() => expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(2))
    expect(document.activeElement).toBe(input)
    input.setSelectionRange(3, 3)
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '20' }))
    await vi.waitFor(() => expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(3))
    expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenLastCalledWith({
      sessionEpoch: '1', focusRequestId: '20', focused: true,
    })
    expect(document.activeElement).toBe(input)
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 3])

    outside.focus()
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '19' }))
    expect(document.activeElement).toBe(outside)
    expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(3)
    await act(async () => emitPanelFocus({ sessionEpoch: '2', focusRequestId: '21' }))
    expect(document.activeElement).toBe(outside)
    expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(3)

    const focus = vi.spyOn(input, 'focus').mockImplementationOnce(() => undefined)
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '21' }))
    await vi.waitFor(() => expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenLastCalledWith({
      sessionEpoch: '1', focusRequestId: '21', focused: false,
    }))
    expect(document.activeElement).toBe(outside)
    focus.mockRestore()

    const closed = deferred<void>()
    vi.mocked(client.closePluginPanel).mockReturnValueOnce(closed.promise)
    let closing!: Promise<void>
    await act(async () => {
      closing = core.closePanel()
      await Promise.resolve()
    })
    expect(core.getSnapshot().panel?.closePending).toBe(true)
    await act(async () => emitPanelFocus({ sessionEpoch: '1', focusRequestId: '22' }))
    expect(document.activeElement).toBe(outside)
    expect(client.acknowledgePluginPanelFocusHostInput).toHaveBeenCalledTimes(4)
    closed.resolve()
    await act(async () => closing)
    outside.remove()
    await mounted.unmount()
  })

  it('buffers only the newest focus event until the matching panel identity is installed', async () => {
    const { core, client, emit, emitPanelFocus } = await startedCore()
    const opened = deferred<PluginPanelCommandResult>()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-early-focus-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel).mockReturnValueOnce(opened.promise)

    emit(shown('panel-early-focus'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'hello', inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(client.openPluginPanel).toHaveBeenCalledOnce())
    emitPanelFocus({ sessionEpoch: '1', focusRequestId: '8' })
    emitPanelFocus({ sessionEpoch: '1', focusRequestId: '9' })
    expect(core.getSnapshot().panel).toBeUndefined()

    opened.resolve({
      sessionEpoch: u64('1'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
    })
    await vi.waitFor(() => expect(core.getSnapshot().panel?.focusRequestId).toBe('9'))
    expect(client.acknowledgePluginPanelFocusHostInput).not.toHaveBeenCalled()
    core.destroy()
  })

  it('rejects old focus events and settlements after a real panel epoch replacement', async () => {
    const { core, client, emit, emitPanelFocus, emitPanelReset } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-focus-replacement-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    vi.mocked(client.openPluginPanel)
      .mockResolvedValueOnce({
        sessionEpoch: u64('1'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
      })
      .mockResolvedValueOnce({
        sessionEpoch: u64('2'), pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
      })

    const openFromQuery = async (invocationId: string) => {
      emit(shown(invocationId))
      await new Promise((resolve) => setTimeout(resolve, 0))
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'hello', inputType: 'insertText',
      })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
    }

    await openFromQuery('panel-focus-first')
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('1'))
    emitPanelFocus({ sessionEpoch: '1', focusRequestId: '5' })
    expect(core.getSnapshot().panel?.focusRequestId).toBe('5')
    emitPanelReset({ sessionEpoch: '1' })
    await vi.waitFor(() => expect(core.getSnapshot().panel).toBeUndefined())

    await openFromQuery('panel-focus-second')
    await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('2'))
    emitPanelFocus({ sessionEpoch: '1', focusRequestId: '99' })
    core.settlePanelHostInputFocus({ sessionEpoch: u64('1'), focusRequestId: u64('99'), focused: true })
    expect(core.getSnapshot().panel?.focusRequestId).toBeUndefined()
    expect(client.acknowledgePluginPanelFocusHostInput).not.toHaveBeenCalled()

    emitPanelFocus({ sessionEpoch: '2', focusRequestId: '1' })
    expect(core.getSnapshot().panel?.focusRequestId).toBe('1')
    core.destroy()
  })

  it('removes native input listeners when the panel bound callback fails', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-bind-failure-result',
      items: [panelItem('hello')],
    } as unknown as SearchResponse)
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('panel-bind-failure')))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await act(async () => core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'hello', inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    const originalSetSelectionRange = HTMLInputElement.prototype.setSelectionRange
    const remove = vi.spyOn(HTMLInputElement.prototype, 'removeEventListener')
    const selection = vi.spyOn(HTMLInputElement.prototype, 'setSelectionRange').mockImplementation(function (
      this: HTMLInputElement,
      start: number | null,
      end: number | null,
      direction?: 'forward' | 'backward' | 'none',
    ) {
      if (this.getAttribute('aria-label') === 'demo-panel argument') throw new Error('private panel bind failure')
      return originalSetSelectionRange.call(this, start, end, direction)
    })
    try {
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('1'))
      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')!
      await vi.waitFor(() => {
        const removedEvents = remove.mock.calls.flatMap((args, index) =>
          remove.mock.instances[index] === input ? [args[0]] : [],
        )
        expect(removedEvents).toEqual(expect.arrayContaining(['compositionstart', 'input', 'compositionend']))
      })
    } finally {
      selection.mockRestore()
      remove.mockRestore()
      await mounted.unmount()
    }
  })

  it('renders the panel tag inside one input shell and closes to a fresh launcher', async () => {
    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const style = document.createElement('style')
    style.textContent = stylesSource
    document.head.append(style)
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-shell-result',
      items: [panelItem('')],
    } as unknown as SearchResponse)
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('panel-shell-entry')))
      await new Promise((resolve) => setTimeout(resolve, 0))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/demo-panel', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().panel).toBeDefined())

      const tag = mounted.host.querySelector<HTMLElement>('[aria-label="command demo-panel"]')
      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-panel argument"]')
      const surface = mounted.host.querySelector<HTMLElement>('.launcher-surface')
      const statusRegion = mounted.host.querySelector<HTMLElement>('.launcher-status-region')
      const shell = tag?.parentElement ?? null
      const inputRegion = shell?.parentElement ?? null
      expect(surface?.classList.contains('is-panel-active')).toBe(true)
      expect(inputRegion?.classList.contains('panel-input-region')).toBe(true)
      expect(inputRegion?.parentElement?.classList.contains('panel-launcher')).toBe(true)
      expect(shell).toContain(tag)
      expect(shell).toContain(input)
      const declaredProperty = (element: Element, property: string) => {
        let value = ''
        for (const rule of Array.from(style.sheet?.cssRules ?? [])) {
          if (!(rule instanceof CSSStyleRule)) continue
          try {
            if (element.matches(rule.selectorText)) value = rule.style.getPropertyValue(property) || value
          } catch {
            // Pseudo-element selectors are not matchable against an Element.
          }
        }
        return value.trim()
      }
      expect(declaredProperty(shell!, 'border')).toContain('1px solid')
      expect(declaredProperty(statusRegion!, 'display')).not.toBe('none')
      expect(declaredProperty(surface!, 'grid-template-rows')).toBe('minmax(52px, 1fr) minmax(0, auto)')
      expect(declaredProperty(tag!, 'background')).toContain('color-mix')
      expect(declaredProperty(tag!, 'border')).toContain('1px solid')
      expect(declaredProperty(input!, 'border')).toMatch(/^0(?:px)?$/)
      expect(declaredProperty(input!, 'background')).toBe('transparent')

      const close = mounted.host.querySelector<HTMLButtonElement>('[aria-label="退出 demo-panel 面板"]')!
      expect(close.tabIndex).toBe(-1)
      await act(async () => close.click())
      await vi.waitFor(() => expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '1' }))
      await vi.waitFor(() => expect(core.getSnapshot().panel).toBeUndefined())
      expect(core.getSnapshot()).toMatchObject({ view: 'launcher', query: '' })
    } finally {
      style.remove()
      await mounted.unmount()
    }
  })

  it.each([1, 1.5, 1.8])('syncs physical panel bounds at pixel ratio %s and cleans up observers', async (pixelRatio) => {
    const originalRatio = window.devicePixelRatio
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: pixelRatio })
    const physical = (value: { x: number; y: number; width: number; height: number }) => Object.fromEntries(Object.entries(value).map(([key, number]) => [key, number * pixelRatio]))
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const resizeObservers: Array<{
      callback: ResizeObserverCallback
      observed: Element[]
      disconnect: ReturnType<typeof vi.fn>
    }> = []
    const OriginalResizeObserver = globalThis.ResizeObserver
    class TestResizeObserver {
      readonly callback: ResizeObserverCallback
      readonly observed: Element[] = []
      readonly disconnect = vi.fn()

      constructor(callback: ResizeObserverCallback) {
        this.callback = callback
        resizeObservers.push(this)
      }

      observe(target: Element) {
        this.observed.push(target)
      }

      unobserve() {}
    }
    Object.defineProperty(globalThis, 'ResizeObserver', { configurable: true, value: TestResizeObserver })

    let nextFrameId = 1
    const frames = new Map<number, FrameRequestCallback>()
    const requestFrame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      const id = nextFrameId
      nextFrameId += 1
      frames.set(id, callback)
      return id
    })
    const cancelFrame = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((id) => {
      frames.delete(id)
    })
    const flushFrames = async () => {
      const pending = [...frames.values()]
      frames.clear()
      await act(async () => pending.forEach((callback) => callback(performance.now())))
    }

    let bounds = { x: 12, y: 64, width: 696, height: 320 }
    const originalRect = HTMLElement.prototype.getBoundingClientRect
    const rect = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (!this.classList.contains('panel-host-region')) return originalRect.call(this)
      return {
        ...bounds,
        left: bounds.x,
        top: bounds.y,
        right: bounds.x + bounds.width,
        bottom: bounds.y + bounds.height,
        toJSON: () => ({ ...bounds }),
      } as DOMRect
    })

    const { core, client, emit } = await startedCore()
    installMatchMedia(false)
    vi.mocked(client.searchApps).mockResolvedValue({
      requestId: 'panel-bounds-result',
      items: [panelItem('')],
    } as unknown as SearchResponse)
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => emit(shown('panel-bounds-entry')))
      await new Promise((resolve) => setTimeout(resolve, 0))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/demo-panel', inputType: 'insertText',
      }))
      await act(async () => core.keyDown('Enter', false))
      await vi.waitFor(() => expect(core.getSnapshot().panel?.sessionEpoch).toBe('1'))

      const region = mounted.host.querySelector<HTMLElement>('.panel-host-region')!
      const observer = resizeObservers.find((candidate) => candidate.observed.includes(region))
      expect(observer).toBeDefined()
      await flushFrames()
      expect(client.setPluginPanelBounds).toHaveBeenCalledWith({
        sessionEpoch: '1',
        bounds: physical({ x: 12, y: 64, width: 696, height: 320 }),
      })

      const framesBeforeResize = requestFrame.mock.calls.length
      observer!.callback([], observer as unknown as ResizeObserver)
      observer!.callback([], observer as unknown as ResizeObserver)
      window.dispatchEvent(new Event('resize'))
      expect(requestFrame).toHaveBeenCalledTimes(framesBeforeResize + 1)
      await flushFrames()
      expect(client.setPluginPanelBounds).toHaveBeenCalledTimes(1)

      bounds = { x: 8.5, y: 62.25, width: 703.5, height: 329.75 }
      observer!.callback([], observer as unknown as ResizeObserver)
      window.dispatchEvent(new Event('resize'))
      await flushFrames()
      expect(client.setPluginPanelBounds).toHaveBeenLastCalledWith({
        sessionEpoch: '1',
        bounds: physical(bounds),
      })
      expect(client.setPluginPanelBounds).toHaveBeenCalledTimes(2)

      await mounted.unmount()
      observer!.callback([], observer as unknown as ResizeObserver)
      window.dispatchEvent(new Event('resize'))
      await flushFrames()
      expect(observer!.disconnect).toHaveBeenCalledOnce()
      expect(client.setPluginPanelBounds).toHaveBeenCalledTimes(2)
    } finally {
      if (mounted.host.isConnected) await mounted.unmount()
      Object.defineProperty(globalThis, 'ResizeObserver', { configurable: true, value: OriginalResizeObserver })
      requestFrame.mockRestore()
      cancelFrame.mockRestore()
      rect.mockRestore()
      Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: originalRatio })
    }
  })

  it('arms a host plugin completion, commits once, and lets a returned action execute', async () => {
    const { core, client, emit } = await startedCore()
    vi.useFakeTimers()
    try {
      const preview = deferred<SearchResponse | null>()
      const commit = deferred<SearchResponse | null>()
      vi.mocked(client.searchApps).mockImplementation((request) => {
        if (request.query === 'abc') {
          return Promise.resolve({
            requestId: 'catalog',
            items: [{
              resultId: 'demo-win-completion',
              title: '/demo-win',
              activation: {
                kind: 'pluginCompletion',
                completionText: '/demo-win abc',
                pluginId: 'com.uipilot.demo-win',
                favorite: true,
              },
              hasDefaultAction: false,
            }],
          } as unknown as SearchResponse)
        }
        return request.completionOrigin?.phase === 'preview' ? preview.promise : commit.promise
      })
      emit(shown('plugin-origin'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(client.searchApps).mockClear()

      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'abc', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
      expect(core.getSnapshot().query).toBe('/demo-win abc')
      await vi.advanceTimersByTimeAsync(150)
      expect(client.searchApps).toHaveBeenLastCalledWith({
        query: '/demo-win abc',
        invocationId: 'plugin-origin',
        querySequence: 3,
        submit: false,
        completionOrigin: { phase: 'preview', pluginId: 'com.uipilot.demo-win' },
      })

      core.keyDown('Enter', false)
      core.keyDown('Enter', false)
      expect(client.searchApps).toHaveBeenLastCalledWith({
        query: '/demo-win abc',
        invocationId: 'plugin-origin',
        querySequence: 4,
        submit: true,
        completionOrigin: { phase: 'commit', pluginId: 'com.uipilot.demo-win' },
      })
      expect(client.searchApps).toHaveBeenCalledTimes(3)

      preview.reject({ code: 'searchUnavailable' })
      await Promise.resolve()
      commit.resolve({
        requestId: 'plugin-result',
        items: [{ resultId: 'copy', title: 'abc result', activation: executeActivation }],
      })
      await commit.promise
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      expect(core.getSnapshot().status).toBe('')

      core.keyDown('Enter', false)
      expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'plugin-result', resultId: 'copy' })
      expect(client.searchApps).toHaveBeenCalledTimes(3)
    } finally {
      vi.useRealTimers()
    }
  })

  it.each(['keyboard', 'pointer'] as const)(
    'enters a main-result command tag on first %s completion activation',
    async (activationMethod) => {
      const { core, client, emit } = await startedCore()
      vi.mocked(client.searchApps).mockImplementation(async (request) => {
        if (request.query === '/demo-return' && request.completionOrigin?.phase !== 'preview') {
          return {
            requestId: 'demo-return-suggestion',
            items: [{
              resultId: 'demo-return-activation',
              title: '/demo-return',
              activation: {
                kind: 'mainResultActivation',
                commandId: 'default',
                pluginId: 'com.uipilot.demo-return',
                commandLabel: 'demo-return',
                initialArgument: '',
                favorite: false,
              },
              hasDefaultAction: false,
            }],
          } as unknown as SearchResponse
        }
        return {
          requestId: 'demo-return-hint',
          items: [],
          commandHint: '请输入信息回车',
        } as unknown as SearchResponse
      })
      emit(shown(`main-result-${activationMethod}`))
      await new Promise((resolve) => setTimeout(resolve, 0))
      core.text({
        kind: 'ordinaryInput',
        control: core.getSnapshot().queryControl,
        value: '/demo-return',
        inputType: 'insertText',
      })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

      if (activationMethod === 'keyboard') core.keyDown('Enter', false)
      else core.activateResult(0)

      expect(core.getSnapshot().mainResultCommand).toMatchObject({
        pluginId: 'com.uipilot.demo-return',
        commandLabel: 'demo-return',
        suffix: '',
      })
      expect(core.getSnapshot().query).toBe('/demo-return')
      expect(client.searchApps).not.toHaveBeenCalledWith(expect.objectContaining({ submit: true }))
      await vi.waitFor(() => expect(core.getSnapshot().commandHint).toBe('请输入信息回车'))
      expect(client.searchApps).toHaveBeenCalledWith(expect.objectContaining({
        query: '/demo-return',
        submit: false,
        completionOrigin: {
          phase: 'preview',
          pluginId: 'com.uipilot.demo-return', commandId: 'default',
        },
      }))
    },
  )

  it('replaces a committing completion with an edited armed owner and rejects late A failure', async () => {
    const { core, client, emit } = await startedCore()
    vi.useFakeTimers()
    try {
      const commitA = deferred<SearchResponse | null>()
      vi.mocked(client.searchApps).mockImplementation((request) => {
        if (request.query === 'seed') {
          return Promise.resolve({
            requestId: 'catalog',
            items: [{
              resultId: 'demo-completion',
              title: '/demo-win',
              activation: {
                kind: 'pluginCompletion', completionText: '/demo-win seed',
                pluginId: 'com.uipilot.demo-win', favorite: true,
              },
            }],
          } as unknown as SearchResponse)
        }
        if (request.completionOrigin?.phase === 'commit' && request.query.endsWith('seed')) return commitA.promise
        return Promise.resolve({ requestId: `preview-${request.querySequence}`, items: [], commandHint: '请输入信息回车' })
      })
      emit(shown('plugin-edit-owner'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(client.searchApps).mockClear()
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'seed', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
      await vi.advanceTimersByTimeAsync(150)
      await Promise.resolve()
      core.keyDown('Enter', false)

      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/demo-win B', inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(150)
      expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({
        query: '/demo-win B', submit: false,
        completionOrigin: { phase: 'preview', pluginId: 'com.uipilot.demo-win' },
      }))
      commitA.reject({ code: 'windowFailed' })
      await Promise.resolve()
      expect(core.getSnapshot()).toMatchObject({ query: '/demo-win B', status: '', commandHint: '请输入信息回车' })

      core.keyDown('Enter', false)
      expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({
        query: '/demo-win B', submit: true,
        completionOrigin: { phase: 'commit', pluginId: 'com.uipilot.demo-win' },
      }))
    } finally {
      vi.useRealTimers()
    }
  })

  it('consumes an ambiguous plugin commit and keeps a third Enter inert', async () => {
    const { core, client, emit } = await startedCore()
    vi.useFakeTimers()
    try {
      vi.mocked(client.searchApps).mockImplementation((request) => {
        if (request.query === 'seed') return Promise.resolve({
          requestId: 'catalog',
          items: [{
            resultId: 'demo-completion', title: '/demo-win',
            activation: {
              kind: 'pluginCompletion', completionText: '/demo-win seed',
              pluginId: 'com.uipilot.demo-win', favorite: true,
            },
          }],
        } as unknown as SearchResponse)
        if (request.completionOrigin?.phase === 'commit') return Promise.reject({ code: 'windowFailed' })
        return Promise.resolve({ requestId: 'preview', items: [] })
      })
      emit(shown('plugin-consumed'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(client.searchApps).mockClear()
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'seed', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
      await vi.advanceTimersByTimeAsync(150)
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(core.getSnapshot().status).toBe('窗口操作失败。'))
      const calls = vi.mocked(client.searchApps).mock.calls.length
      core.keyDown('Enter', false)
      expect(client.searchApps).toHaveBeenCalledTimes(calls)
    } finally {
      vi.useRealTimers()
    }
  })

  it('makes plugin commit sequence exhaustion absorbing until a new native shown invocation', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client, 3)
    await core.start()
    vi.useFakeTimers()
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === ''
        ? null
        : request.query === 'seed' ? ({
            requestId: 'catalog',
            items: [{
              resultId: 'demo-completion', title: '/demo-win',
              activation: {
                kind: 'pluginCompletion', completionText: '/demo-win seed',
                pluginId: 'com.uipilot.demo-win', favorite: true,
              },
            }],
          } as unknown as SearchResponse) : ({ requestId: 'preview', items: [] }))
      fake.emit(shown('plugin-exhausted'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(fake.client.searchApps).mockClear()
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'seed', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.keyDown('Enter', false)
      await vi.advanceTimersByTimeAsync(150)
      core.keyDown('Enter', false)
      expect(core.getSnapshot().status).toBe('查询次数已达上限，请重新打开主界面。')
      const calls = vi.mocked(fake.client.searchApps).mock.calls.length

      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/demo-win edited', inputType: 'insertText',
      })
      core.keyDown('Enter', false)
      await vi.advanceTimersByTimeAsync(500)
      expect(fake.client.searchApps).toHaveBeenCalledTimes(calls)
      expect(core.getSnapshot().status).toBe('查询次数已达上限，请重新打开主界面。')

      fake.emit(shown('plugin-reopened'))
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot()).toMatchObject({ query: '', querySequence: 1, status: '' })
      expect(fake.client.searchApps).toHaveBeenCalledTimes(calls + 1)
    } finally {
      vi.useRealTimers()
    }
  })

  it('refreshes the captured query after a current favorite mutation succeeds', async () => {
    const { core, client, emit } = await startedCore()
    const mutation = deferred<void>()
    let favorite = false
    vi.mocked(client.setPublicPluginFavorite).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: `favorite-${request.querySequence}`,
      items: request.query === 'abc' ? [{
        resultId: 'demo-completion',
        title: '/demo-win',
        activation: {
          kind: 'pluginCompletion', completionText: '/demo-win abc',
          pluginId: 'com.uipilot.demo-win', favorite,
        },
        favorite: {
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win', commandId: 'default' },
          favorite,
        },
      }] : [],
    } as unknown as SearchResponse))
    emit(shown('favorite-current'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    vi.mocked(client.searchApps).mockClear()
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'abc', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    core.openPluginContextMenu(0)
    core.setPluginFavorite(0, true)
    core.closePluginContextMenu()
    expect(client.setPublicPluginFavorite).toHaveBeenCalledWith({ commandId: 'default',
      pluginId: 'com.uipilot.demo-win', favorite: true,
    })
    expect(core.getSnapshot().favoriteMutationPending).toBe(true)
    favorite = true
    mutation.resolve()
    await mutation.promise
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledTimes(2))
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.favorite?.favorite).toBe(true))
    expect(core.getSnapshot()).toMatchObject({ query: 'abc', status: '', favoriteMutationPending: false })
    expect(client.executeResult).not.toHaveBeenCalled()
    expect(client.hideLauncher).not.toHaveBeenCalled()
  })

  it('persists a builtin favorite without optimistic state and refreshes the current query', async () => {
    const { core, client, emit } = await startedCore()
    const mutation = deferred<void>()
    let favorite = false
    vi.mocked(client.setBuiltinFeatureFavorite).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: `builtin-favorite-${request.querySequence}`,
      items: request.query === '' ? [{
        ...findLauncherItem(''),
        favorite: {
          target: { kind: 'builtin', feature: 'find' },
          favorite,
        },
      }] : [],
    } as unknown as SearchResponse))
    emit(shown('builtin-favorite-current'))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    core.openPluginContextMenu(0)
    core.setPluginFavorite(0, true)
    core.closePluginContextMenu()
    expect(client.setBuiltinFeatureFavorite).toHaveBeenCalledWith({ feature: 'find', favorite: true })
    expect(core.getSnapshot().results[0]?.favorite?.favorite).toBe(false)
    expect(core.getSnapshot().favoriteMutationPending).toBe(true)

    favorite = true
    mutation.resolve()
    await mutation.promise
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.favorite?.favorite).toBe(true))
    expect(core.getSnapshot()).toMatchObject({ query: '', status: '', favoriteMutationPending: false })
  })

  it('removes a nonmatching plugin from the captured plain query after cancelling favorite', async () => {
    const { core, client, emit } = await startedCore()
    let favorite = true
    vi.mocked(client.setPublicPluginFavorite).mockImplementationOnce(async () => { favorite = false })
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: `favorite-cancel-${request.querySequence}`,
      items: request.query === 'unrelated' && favorite ? [{
        resultId: 'demo-completion', title: '/demo-win',
        activation: {
          kind: 'pluginCompletion', completionText: '/demo-win unrelated',
          pluginId: 'com.uipilot.demo-win', favorite: true,
        },
        favorite: {
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win', commandId: 'default' },
          favorite: true,
        },
      }] : [],
    } as unknown as SearchResponse))
    emit(shown('favorite-cancel-current'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'unrelated', inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    core.openPluginContextMenu(0)
    core.setPluginFavorite(0, false)
    core.closePluginContextMenu()
    await vi.waitFor(() => expect(core.getSnapshot().favoriteMutationPending).toBe(false))
    await vi.waitFor(() => expect(core.getSnapshot().results).toEqual([]))
    expect(core.getSnapshot().query).toBe('unrelated')
  })

  it('keeps a current favorite failure local and publishes only the fixed error', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.setPublicPluginFavorite).mockRejectedValueOnce({
      code: 'pluginListFailed', message: 'private backend detail',
    })
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: `favorite-failure-${request.querySequence}`,
      items: request.query === 'abc' ? [{
        resultId: 'demo-completion', title: '/demo-win',
        activation: {
          kind: 'pluginCompletion', completionText: '/demo-win abc',
          pluginId: 'com.uipilot.demo-win', favorite: false,
        },
        favorite: {
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win', commandId: 'default' },
          favorite: false,
        },
      }] : [],
    } as unknown as SearchResponse))
    emit(shown('favorite-current-failure'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'abc', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    const resultKey = core.getSnapshot().results[0]?.key
    core.openPluginContextMenu(0)
    core.setPluginFavorite(0, true)
    core.closePluginContextMenu()
    await vi.waitFor(() => expect(core.getSnapshot().favoriteMutationPending).toBe(false))
    expect(core.getSnapshot()).toMatchObject({ query: 'abc', status: '操作不可用，请重试。' })
    expect(core.getSnapshot().results[0]?.key).toBe(resultKey)
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend detail')
  })

  it('keeps late favorite success and failure inert after interaction ownership changes', async () => {
    for (const invalidation of ['keyboard', 'edit', 'pointer', 'menu', 'view', 'hideReopen'] as const) {
      const { core, client, emit } = await startedCore()
      const mutation = deferred<void>()
      vi.mocked(client.setPublicPluginFavorite).mockReturnValueOnce(mutation.promise)
      vi.mocked(client.searchApps).mockImplementation(async (request) => ({
        requestId: `${invalidation}-${request.querySequence}`,
        items: request.query === 'abc' ? [
          {
            resultId: 'first', title: '/demo-a',
            activation: {
              kind: 'pluginCompletion', completionText: '/demo-a abc',
              pluginId: 'com.uipilot.demo-a', favorite: false,
            },
            favorite: {
              target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-a', commandId: 'default' },
              favorite: false,
            },
          },
          {
            resultId: 'second', title: '/demo-b',
            activation: {
              kind: 'pluginCompletion', completionText: '/demo-b abc',
              pluginId: 'com.uipilot.demo-b', favorite: false,
            },
            favorite: {
              target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-b', commandId: 'default' },
              favorite: false,
            },
          },
        ] : [],
      } as unknown as SearchResponse))
      emit(shown(`favorite-stale-${invalidation}`))
      await new Promise((resolve) => setTimeout(resolve, 0))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'abc', inputType: 'insertText' })
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
      core.openPluginContextMenu(0)
      core.setPluginFavorite(0, true)
      core.closePluginContextMenu()

      if (invalidation === 'keyboard') core.keyDown('ArrowDown', false)
      if (invalidation === 'edit') core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'changed', inputType: 'insertText',
      })
      if (invalidation === 'pointer') core.activateResult(1)
      if (invalidation === 'menu') core.openPluginContextMenu(1)
      if (invalidation === 'view') core.navigate('settings')
      if (invalidation === 'hideReopen') {
        await core.requestHide()
        emit(shown(`favorite-reopened-${invalidation}`))
        await new Promise((resolve) => setTimeout(resolve, 0))
      }
      if (invalidation === 'edit') {
        await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
      }
      const searches = vi.mocked(client.searchApps).mock.calls.length
      const statusBeforeSettlement = core.getSnapshot().status
      if (invalidation === 'keyboard' || invalidation === 'edit' || invalidation === 'view') mutation.resolve()
      else mutation.reject({ code: 'pluginListFailed' })
      await mutation.promise.catch(() => undefined)
      await Promise.resolve()

      expect(client.searchApps).toHaveBeenCalledTimes(searches)
      expect(core.getSnapshot().status).toBe(statusBeforeSettlement)
      expect(core.getSnapshot().favoriteMutationPending).toBe(false)
      core.destroy()
    }
  })
  it('commits a prepared plugin window only for the still-current query owner', async () => {
    const { core, client, emit } = await startedCore()
    vi.useFakeTimers()
    try {
      const response = deferred<SearchResponse | null>()
      const commit = deferred<void>()
      vi.mocked(client.searchApps).mockReturnValueOnce(response.promise)
      vi.mocked(client.commitPluginWindowTransfer).mockReturnValueOnce(commit.promise)
      emit(shown('window-owner'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/demo A', inputType: 'insertText' })
      core.keyDown('Enter', false)
      response.resolve({ requestId: 'window-request-a', items: [], windowTransferToken: 'window-transfer-a' })
      await response.promise
      await Promise.resolve()
      expect(client.commitPluginWindowTransfer).toHaveBeenCalledWith({ transferToken: 'window-transfer-a' })

      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/demo B', inputType: 'insertText' })
      const edited = core.getSnapshot()
      commit.resolve()
      await commit.promise
      await Promise.resolve()
      expect(core.getSnapshot()).toBe(edited)
      expect(core.getSnapshot().query).toBe('/demo B')
    } finally {
      vi.useRealTimers()
    }
  })
  it('keeps only strict bounded quicklink-sized PNG data icons', async () => {
    const { core, client, emit } = await startedCore()
    const backendMaxPngBytes = 256 * 1024
    const backendMaxBase64Payload = Math.ceil(backendMaxPngBytes / 3) * 4
    const valid = `data:image/png;base64,${'A'.repeat(backendMaxBase64Payload)}`
    const invalid = [
      'data:image/png;base64,',
      'data:image/svg+xml;base64,AAAA',
      'file:///C:/private/icon.png',
      'https://example.invalid/icon.png',
      'data:image/png;base64,AAA',
      'data:image/png;base64,AA=A',
      'data:image/png;base64,AAAA===',
      'data:image/png;base64,AA_A',
      'data:image/png;base64,AA%2F',
      'data:image/png;base64,AAAA\n',
      `data:image/png;base64,${'A'.repeat(backendMaxBase64Payload + 4)}`,
    ]
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'icons',
      items: [
        findLauncherItem('icon'),
        { resultId: 'valid', title: 'Valid', icon: valid, activation: executeActivation },
        ...invalid.map((icon, index) => ({ resultId: `bad-${index}`, title: `Bad ${index}`, icon, activation: executeActivation })),
      ],
    })
    emit(shown('icons'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icon', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results[0]?.icon).toBeUndefined()
    expect(core.getSnapshot().results[1]?.icon).toBe(valid)
    expect(core.getSnapshot().results.slice(2).every((item) => item.icon === undefined)).toBe(true)
  })
})

describe('minute refresh ownership', () => {
  const currentTime = '2026-09-03T14:34:30Z'
  const firstBoundary = '2026-09-03T14:35:00Z'

  function timeZoneResponse(
    requestId: string,
    title: string,
    refreshAtUtc?: unknown,
  ): SearchResponse {
    return {
      requestId,
      ...(refreshAtUtc === undefined ? {} : { refreshAtUtc }),
      items: [{
        resultId: `${requestId}-result`,
        title,
        subtitle: '2026-09-03 · New York · UTC−04:00 (EDT)',
        iconKind: 'timeZone',
        activation: executeActivation,
        hasDefaultAction: true,
      }],
    } as unknown as SearchResponse
  }

  async function scheduledTimeCore(maximumQuerySequence = Number.MAX_SAFE_INTEGER) {
    const fake = fakeClient()
    const quiet = deferred<SearchResponse | null>()
    let timeSearches = 0
    vi.mocked(fake.client.searchApps).mockImplementation((request) => {
      if (request.query !== 'NewYork time') {
        return Promise.resolve(timeZoneResponse('ordinary-latest', request.query || 'empty'))
      }
      timeSearches += 1
      return timeSearches === 1
        ? Promise.resolve(timeZoneResponse('time-initial', '10:34', firstBoundary))
        : quiet.promise
    })
    const core = createLauncherCore(fake.client, maximumQuerySequence)
    await core.start()
    fake.emit(shown('minute-owner'))
    await vi.advanceTimersByTimeAsync(0)
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'NewYork time',
      inputType: 'insertText',
    })
    await vi.advanceTimersByTimeAsync(0)
    vi.mocked(fake.client.searchApps).mockClear()
    return { core, fake, quiet }
  }

  it('sanitizes only real strict UTC RFC3339 Z minute refresh values', () => {
    expect(parseRefreshAtUtc('2026-09-03T14:35:00Z')).toBe('2026-09-03T14:35:00Z')
    expect(parseRefreshAtUtc('2026-09-03T14:35:00.125Z')).toBe('2026-09-03T14:35:00.125Z')
    for (const invalid of [
      '2026-02-30T14:35:00Z',
      '2026-09-03T14:35:00+00:00',
      '2026-09-03t14:35:00Z',
      '2026-09-03T14:35:00z',
      '2026-9-03T14:35:00Z',
      '2026-09-03T24:00:00Z',
      1_788_442_500_000,
      null,
    ]) expect(parseRefreshAtUtc(invalid)).toBeNull()
  })

  it('accepts a strict future UTC refresh timestamp and fires exactly at the time zone minute boundary', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-09-03T14:34:30Z'))
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === 'NewYork time'
        ? {
            requestId: `time-${request.querySequence}`,
            refreshAtUtc: request.querySequence === 2
              ? '2026-09-03T14:35:00Z'
              : '2026-09-03T14:36:00Z',
            items: [{
              resultId: `time-result-${request.querySequence}`,
              title: request.querySequence === 2 ? '10:34' : '10:35',
              subtitle: '2026-09-03 · New York · UTC−04:00 (EDT)',
              iconKind: 'timeZone',
              activation: executeActivation,
              hasDefaultAction: true,
            }],
          } as unknown as SearchResponse
        : { requestId: 'empty', items: [] })
      await core.start()
      fake.emit(shown('minute-boundary'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(fake.client.searchApps).mockClear()

      core.text({
        kind: 'ordinaryInput',
        control: core.getSnapshot().queryControl,
        value: 'NewYork time',
        inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot()).toMatchObject({
        querySequence: 2,
        searchPending: false,
        results: [{ title: '10:34', iconKind: 'timeZone', hasDefaultAction: true }],
      })
      expect(fake.client.searchApps).toHaveBeenCalledTimes(1)

      await vi.advanceTimersByTimeAsync(29_999)
      expect(fake.client.searchApps).toHaveBeenCalledTimes(1)
      await vi.advanceTimersByTimeAsync(1)

      expect(fake.client.searchApps).toHaveBeenCalledTimes(2)
      expect(fake.client.searchApps).toHaveBeenLastCalledWith({
        query: 'NewYork time',
        invocationId: 'minute-boundary',
        querySequence: 3,
      })
      expect(core.getSnapshot()).toMatchObject({
        querySequence: 3,
        searchPending: false,
        results: [{ title: '10:35', iconKind: 'timeZone', hasDefaultAction: true }],
      })
    } finally {
      core.destroy()
      vi.useRealTimers()
    }
  })

  it.each([
    ['an impossible calendar date', '2026-02-30T14:35:00Z'],
    ['a non-UTC offset', '2026-09-03T14:35:00+00:00'],
    ['a lowercase zone suffix', '2026-09-03T14:35:00z'],
    ['a stale instant', currentTime],
  ])('ignores %s in minute refresh metadata while retaining the valid row', async (_label, refreshAtUtc) => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === 'NewYork time'
        ? timeZoneResponse('time-invalid-refresh', '10:34', refreshAtUtc)
        : { requestId: 'empty', items: [] })
      await core.start()
      fake.emit(shown('invalid-refresh'))
      await vi.advanceTimersByTimeAsync(0)
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'NewYork time', inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot().results).toMatchObject([{
        title: '10:34', iconKind: 'timeZone', hasDefaultAction: true,
      }])

      vi.mocked(fake.client.searchApps).mockClear()
      await vi.advanceTimersByTimeAsync(10 * 60_000)
      expect(fake.client.searchApps).not.toHaveBeenCalled()
    } finally {
      core.destroy()
      vi.useRealTimers()
    }
  })

  it('never infers minute refresh work from the time zone icon or visible text', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    try {
      vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === 'NewYork time'
        ? timeZoneResponse('time-without-refresh', '10:34')
        : { requestId: 'empty', items: [] })
      await core.start()
      fake.emit(shown('no-inferred-refresh'))
      await vi.advanceTimersByTimeAsync(0)
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'NewYork time', inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(fake.client.searchApps).mockClear()

      await vi.advanceTimersByTimeAsync(60_000)
      expect(fake.client.searchApps).not.toHaveBeenCalled()
      expect(core.getSnapshot().results).toMatchObject([{ title: '10:34', iconKind: 'timeZone' }])
    } finally {
      core.destroy()
      vi.useRealTimers()
    }
  })

  it('suppresses minute refresh when the raw control value changes', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'compositionStart', control })
      core.text({ kind: 'compositionInput', control, value: 'NewYork timex', inputType: 'insertCompositionText' })

      await vi.advanceTimersByTimeAsync(30_000)
      expect(started.fake.client.searchApps).not.toHaveBeenCalled()
      expect(core.getSnapshot().queryControlValue).toBe('NewYork timex')
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('suppresses minute refresh when a newer invocation takes ownership', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      started.fake.emit(shown('minute-owner-new'))
      await vi.advanceTimersByTimeAsync(0)
      vi.mocked(started.fake.client.searchApps).mockClear()

      await vi.advanceTimersByTimeAsync(30_000)
      expect(started.fake.client.searchApps).not.toHaveBeenCalled()
      expect(core.getSnapshot().invocationId).toBe('minute-owner-new')
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('suppresses minute refresh after navigating away from the launcher view', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      core.navigate('settings')

      await vi.advanceTimersByTimeAsync(30_000)
      expect(started.fake.client.searchApps).not.toHaveBeenCalled()
      expect(core.getSnapshot().view).toBe('settings')
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('suppresses minute refresh when the query sequence is exhausted', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore(2)
      core = started.core

      await vi.advanceTimersByTimeAsync(30_000)
      expect(started.fake.client.searchApps).not.toHaveBeenCalled()
      expect(core.getSnapshot()).toMatchObject({
        querySequence: 2,
        results: [],
        selectedIndex: -1,
        status: '查询次数已达上限，请重新打开主界面。',
      })
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('cancels minute refresh on hide request and native hidden notification', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let firstCore: ReturnType<typeof createLauncherCore> | undefined
    let secondCore: ReturnType<typeof createLauncherCore> | undefined
    try {
      const requested = await scheduledTimeCore()
      firstCore = requested.core
      await firstCore.requestHide()
      await vi.advanceTimersByTimeAsync(30_000)
      expect(requested.fake.client.searchApps).not.toHaveBeenCalled()

      vi.setSystemTime(new Date(currentTime))
      const notified = await scheduledTimeCore()
      secondCore = notified.core
      notified.fake.emitHidden()
      await vi.advanceTimersByTimeAsync(30_000)
      expect(notified.fake.client.searchApps).not.toHaveBeenCalled()
    } finally {
      firstCore?.destroy()
      secondCore?.destroy()
      vi.useRealTimers()
    }
  })

  it('cancels minute refresh when the launcher core is destroyed', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    try {
      const started = await scheduledTimeCore()
      started.core.destroy()
      await vi.advanceTimersByTimeAsync(30_000)
      expect(started.fake.client.searchApps).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('performs one minute refresh after a late wake without replaying missed minutes', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      vi.setSystemTime(new Date('2026-09-03T14:40:00Z'))
      await vi.runOnlyPendingTimersAsync()

      expect(started.fake.client.searchApps).toHaveBeenCalledTimes(1)
      expect(started.fake.client.searchApps).toHaveBeenCalledWith({
        query: 'NewYork time', invocationId: 'minute-owner', querySequence: 3,
      })
      await vi.runOnlyPendingTimersAsync()
      expect(started.fake.client.searchApps).toHaveBeenCalledTimes(1)
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('keeps the retained minute row inert without a spinner until the matching replacement arrives', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    let core: ReturnType<typeof createLauncherCore> | undefined
    let mounted: Awaited<ReturnType<typeof mountLauncherView>> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      mounted = await mountLauncherView(core)
      await vi.advanceTimersByTimeAsync(30_000)

      expect(started.fake.client.searchApps).toHaveBeenCalledOnce()
      expect(core.getSnapshot()).toMatchObject({
        querySequence: 3,
        searchPending: false,
        results: [{ title: '10:34', hasDefaultAction: false }],
      })
      expect(mounted.host.querySelector('.ant-spin-spinning')).toBeNull()
      expect(mounted.host.querySelector('.result-title')?.textContent).toBe('10:34')
      core.keyDown('Enter', false)
      await act(async () => mounted?.host.querySelector<HTMLElement>('[role="option"]')?.click())
      expect(started.fake.client.executeResult).not.toHaveBeenCalled()

      started.quiet.resolve(timeZoneResponse('time-replacement', '10:35', '2026-09-03T14:36:00Z'))
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot()).toMatchObject({
        searchPending: false,
        results: [{ title: '10:35', hasDefaultAction: true }],
      })
      expect(mounted.host.querySelector('.result-title')?.textContent).toBe('10:35')
      core.keyDown('Enter', false)
      await vi.advanceTimersByTimeAsync(0)
      expect(started.fake.client.executeResult).toHaveBeenCalledWith({
        requestId: 'time-replacement', resultId: 'time-replacement-result',
      })
    } finally {
      await mounted?.unmount()
      core?.destroy()
      vi.useRealTimers()
    }
  })

  it('keeps every retained default action kind inert during a minute refresh', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    const fake = fakeClient()
    const quiet = deferred<SearchResponse | null>()
    const core = createLauncherCore(fake.client)
    try {
      vi.mocked(fake.client.searchApps).mockImplementation((request) => {
        if (request.query !== 'NewYork time') return Promise.resolve({ requestId: 'empty', items: [] })
        if (request.querySequence > 2) return quiet.promise
        return Promise.resolve({
          requestId: 'retired-completion',
          refreshAtUtc: firstBoundary,
          items: [{
            resultId: 'retired-completion-result',
            title: '10:34',
            iconKind: 'timeZone',
            activation: { kind: 'completion', completionText: '/web-search retired' },
            hasDefaultAction: true,
          }],
        } as unknown as SearchResponse)
      })
      await core.start()
      fake.emit(shown('retired-action-kind'))
      await vi.advanceTimersByTimeAsync(0)
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'NewYork time', inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(30_000)

      expect(core.getSnapshot().results).toMatchObject([{ title: '10:34', hasDefaultAction: false }])
      core.keyDown('Enter', false)
      core.activateResult(0)
      expect(core.getSnapshot().query).toBe('NewYork time')
      expect(fake.client.searchApps).toHaveBeenCalledTimes(3)
    } finally {
      core.destroy()
      vi.useRealTimers()
    }
  })

  it.each(['rejects', 'throws'] as const)(
    '%s a quiet minute refresh without hiding or reviving its retired action',
    async (failureMode) => {
      vi.useFakeTimers()
      vi.setSystemTime(new Date(currentTime))
      const fake = fakeClient()
      const core = createLauncherCore(fake.client)
      try {
        vi.mocked(fake.client.searchApps).mockImplementation((request) => {
          if (request.query !== 'NewYork time') return Promise.resolve({ requestId: 'empty', items: [] })
          if (request.querySequence === 2) {
            return Promise.resolve(timeZoneResponse('time-before-failure', '10:34', firstBoundary))
          }
          if (failureMode === 'throws') throw new Error('quiet refresh threw')
          return Promise.reject(new Error('quiet refresh rejected'))
        })
        await core.start()
        fake.emit(shown(`quiet-${failureMode}`))
        await vi.advanceTimersByTimeAsync(0)
        core.text({
          kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
          value: 'NewYork time', inputType: 'insertText',
        })
        await vi.advanceTimersByTimeAsync(30_000)

        expect(core.getSnapshot()).toMatchObject({
          view: 'launcher',
          query: 'NewYork time',
          searchPending: false,
          results: [{ title: '10:34', hasDefaultAction: false }],
        })
        core.keyDown('Enter', false)
        core.activateResult(0)
        expect(fake.client.executeResult).not.toHaveBeenCalled()
        expect(fake.client.hideLauncher).not.toHaveBeenCalled()
      } finally {
        core.destroy()
        vi.useRealTimers()
      }
    },
  )

  it('does not let an old quiet minute response replace newer ordinary text', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(currentTime))
    let core: ReturnType<typeof createLauncherCore> | undefined
    try {
      const started = await scheduledTimeCore()
      core = started.core
      await vi.advanceTimersByTimeAsync(30_000)
      core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: 'latest text', inputType: 'insertText',
      })
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot()).toMatchObject({
        query: 'latest text',
        results: [{ title: 'latest text' }],
      })

      const current = core.getSnapshot()
      started.quiet.resolve(timeZoneResponse('stale-time', '10:35', '2026-09-03T14:36:00Z'))
      await vi.advanceTimersByTimeAsync(0)
      expect(core.getSnapshot()).toBe(current)
      expect(core.getSnapshot()).toMatchObject({
        query: 'latest text',
        results: [{ title: 'latest text' }],
      })
    } finally {
      core?.destroy()
      vi.useRealTimers()
    }
  })
})

describe('execute and hide ownership', () => {
  it('executes the private current mapping once and never asks the frontend to hide on success', async () => {
    const { core, client, emit } = await startedCore()
    const search: SearchResponse = {
      requestId: 'private-request',
      items: [findLauncherItem('calc'), { resultId: 'private-result', title: 'Calculator', activation: executeActivation }],
    }
    vi.mocked(client.searchApps).mockResolvedValueOnce(search)
    const execute = deferred<ExecuteOutcome>()
    vi.mocked(client.executeResult).mockReturnValueOnce(execute.promise)
    emit(shown('execute'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private-request')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private-result')
    core.keyDown('ArrowDown', false)
    core.keyDown('Enter', false)
    core.keyDown('Enter', false)
    expect(client.executeResult).toHaveBeenCalledOnce()
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'private-request', resultId: 'private-result' })
    execute.resolve({ status: 'launchRequested' })
    await execute.promise
    await Promise.resolve()
    expect(client.hideLauncher).not.toHaveBeenCalled()
  })

  it('executes a current public copy action by Enter or row activation and ignores actionless rows', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'public-copy-request',
      items: [
        findLauncherItem('public'),
        { resultId: 'copy', title: 'Copy', hasDefaultAction: true, activation: executeActivation },
        { resultId: 'info', title: 'Info', hasDefaultAction: false, activation: executeActivation },
      ],
    })
    emit(shown('public-copy'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'public', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    core.keyDown('ArrowDown', false)
    core.keyDown('Enter', false)
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'public-copy-request', resultId: 'copy' })
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    vi.mocked(client.executeResult).mockClear()
    core.activateResult(2)
    expect(client.executeResult).not.toHaveBeenCalled()
    core.activateResult(1)
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'public-copy-request', resultId: 'copy' })
  })
  it('treats host-owned text copy as execute success without frontend hide', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'copy-request',
      items: [findLauncherItem('copy'), { resultId: 'copy-result', title: 'Copy', activation: executeActivation }],
    })
    vi.mocked(client.executeResult).mockResolvedValueOnce({ status: 'textCopied' })
    emit(shown('copy'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'copy', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))

    core.keyDown('ArrowDown', false)
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))

    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'copy-request', resultId: 'copy-result' })
    expect(client.hideLauncher).not.toHaveBeenCalled()
  })

  it('keeps a date quick evaluation visible when exact ISO copy fails', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'date-copy-request',
      items: [{
        resultId: 'opaque-date-copy-id',
        title: '2026-09-10',
        subtitle: 'Thursday · today + 7 days',
        iconKind: 'date',
        hasDefaultAction: true,
        activation: executeActivation,
      }],
      replaceLocalResults: true,
    })
    vi.mocked(client.executeResult).mockRejectedValueOnce({
      code: 'clipboardWriteFailed',
      message: 'private clipboard detail',
    })
    emit(shown('date-copy-failure'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'today + 7 days',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))

    expect(client.executeResult).toHaveBeenCalledWith({
      requestId: 'date-copy-request',
      resultId: 'opaque-date-copy-id',
    })
    expect(client.hideLauncher).not.toHaveBeenCalled()
    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      query: 'today + 7 days',
      status: '无法复制到剪贴板。',
      results: [{ title: '2026-09-10' }],
    })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private clipboard detail')
  })

  it('shares one hide owner, ignores composing Escape, and keeps current state on rejection', async () => {
    const { core, client, emit } = await startedCore()
    const hide = deferred<void>()
    vi.mocked(client.hideLauncher).mockReturnValueOnce(hide.promise)
    emit(shown('hide'))
    core.keyDown('Escape', true)
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.keyDown('Escape', false)
    void core.requestHide()
    expect(client.hideLauncher).toHaveBeenCalledOnce()
    hide.reject({ code: 'windowFailed', message: 'private' })
    await expect(hide.promise).rejects.toBeDefined()
    await vi.waitFor(() => expect(core.getSnapshot().hidePending).toBe(false))
    expect(core.getSnapshot()).toMatchObject({ view: 'launcher', invocationId: 'hide', status: '窗口操作失败。' })
  })
  it('resets hidden non-launcher state before the next native show', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('hide-settings'))
    core.navigate('settings')
    expect(core.getSnapshot().view).toBe('settings')

    await core.requestHide()

    expect(client.hideLauncher).toHaveBeenCalledOnce()
    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      query: '',
      queryControlValue: '',
      results: [],
      hidePending: false,
      status: '',
    })
    expect(core.getSnapshot()).not.toHaveProperty('settingsLoadStatus')
  })
  it('resets non-launcher state after a native blur hide event', async () => {
    const { core, emit, emitHidden } = await startedCore()
    emit(shown('blur-hide-settings'))
    core.navigate('settings')
    expect(core.getSnapshot().view).toBe('settings')

    emitHidden()

    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      query: '',
      queryControlValue: '',
      results: [],
      hidePending: false,
      status: '',
    })
    expect(core.getSnapshot()).not.toHaveProperty('settingsLoadStatus')
  })
  it('keeps an application search owner alive when hide is rejected', async () => {
    const { core, client, emit } = await startedCore()
    const search = deferred<SearchResponse | null>()
    const hide = deferred<void>()
    vi.mocked(client.searchApps).mockReturnValueOnce(search.promise)
    vi.mocked(client.hideLauncher).mockReturnValueOnce(hide.promise)
    emit(shown('hide-rejected-search'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    const hiding = core.requestHide()
    hide.reject({ code: 'windowFailed' })
    await hiding
    search.resolve({
      requestId: 'application-after-hide',
      items: [findLauncherItem('calc'), { resultId: 'result', title: 'Calculator', activation: executeActivation }],
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
  })
})

describe('IME ownership', () => {
  it('permanently retires the pre-composition search even when draft text returns', async () => {
    const { core, client, emit } = await startedCore()
    const old = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(old.promise)
    emit(shown('retire-search'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: 'old', inputType: 'insertText' })
    core.text({ kind: 'compositionStart', control })
    expect(core.getSnapshot()).toMatchObject({ query: 'old', queryControlValue: 'old', querySequence: 2, searchPending: false, results: [] })
    core.text({ kind: 'compositionInput', control, value: '新', inputType: 'insertCompositionText' })
    core.text({ kind: 'compositionInput', control, value: 'old', inputType: 'insertCompositionText' })
    const returned = core.getSnapshot()
    old.resolve({ requestId: 'retired', items: [{ resultId: 'retired', title: 'Retired', activation: executeActivation }] })
    await old.promise
    await Promise.resolve()
    expect(core.getSnapshot()).toBe(returned)
    expect(core.getSnapshot().results).toEqual([])
  })

  it('lets only the new shown auto-search commit across late old composition records', async () => {
    const { core, client, emit } = await startedCore()
    const old = deferred<SearchResponse | null>()
    const current = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise)
    emit(shown('old-invocation'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' })
    core.text({ kind: 'compositionStart', control })
    core.text({ kind: 'compositionInput', control, value: '计算', inputType: 'insertCompositionText' })
    emit(shown('new-invocation'))
    expect(core.getSnapshot()).toMatchObject({ query: '', queryControlValue: '', querySequence: 1, searchPending: false })
    core.text({ kind: 'compositionBoundary', control })
    core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
    await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().searchPending).toBe(true)
    old.resolve({ requestId: 'old', items: [{ resultId: 'old', title: 'Old', activation: executeActivation }] })
    current.resolve({ requestId: 'new', items: [{ resultId: 'new', title: 'New', activation: executeActivation }] })
    await Promise.all([old.promise, current.promise])
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['New'])
  })

  it('keeps an exact empty commit state-idempotent', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('empty-ime'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control })
    const started = core.getSnapshot()
    core.text({ kind: 'compositionBoundary', control })
    expect(core.getSnapshot()).toBe(started)
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(core.getSnapshot()).toMatchObject({ query: '', queryControlValue: '', querySequence: 1, searchPending: false })
  })

  it('retires active ownership and its visible draft idempotently', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('retire-control'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control })
    core.text({ kind: 'compositionInput', control, value: 'late', inputType: 'insertCompositionText' })
    core.retireControl(control)
    const retired = core.getSnapshot()
    core.retireControl(control)
    core.text({ kind: 'compositionBoundary', control })
    core.text({ kind: 'compositionInput', control, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(retired)
    expect(core.getSnapshot().queryControlValue).toBe('')
    expect(client.searchApps).not.toHaveBeenCalled()
  })
})

describe('R3 correlated composition boundary', () => {
  it('commits a launcher draft at a no-tail boundary exactly once', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('r3-launcher'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' }))
    vi.mocked(client.searchApps).mockClear()

    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    expect(core.getSnapshot()).toMatchObject({ query: 'calc', queryControlValue: '\u6d4b\u8bd5', querySequence: 2 })
    expect(client.searchApps).not.toHaveBeenCalled()

    const boundary = r3({ kind: 'compositionBoundary', control })
    expect(Object.keys(boundary).sort()).toEqual(['control', 'kind'])
    core.text(boundary)
    expect(core.getSnapshot()).toMatchObject({ query: '\u6d4b\u8bd5', queryControlValue: '\u6d4b\u8bd5', querySequence: 3 })
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(client.searchApps).toHaveBeenCalledWith({ query: '\u6d4b\u8bd5', invocationId: 'r3-launcher', querySequence: 3 })

    const committed = core.getSnapshot()
    core.text(r3({ kind: 'ordinaryInput', control, value: '\u6d4b\u8bd5', inputType: 'insertText' }))
    core.text(boundary)
    expect(core.getSnapshot()).toBe(committed)
    expect(client.searchApps).toHaveBeenCalledOnce()
  })

  it('commits a settings draft locally and makes its same-value tail a no-op', async () => {
    const { core, client } = await startedSettingsCore()
    const control = core.getSnapshot().settings!.hotkey.key
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    const listener = vi.fn()
    core.subscribe(listener)

    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(listener).toHaveBeenCalledOnce()
    expect(core.getSnapshot().settings!.hotkey.value).toBe('\u6d4b\u8bd5')
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(client.saveSettings).not.toHaveBeenCalled()

    const committed = core.getSnapshot()
    listener.mockClear()
    core.text(r3({ kind: 'ordinaryInput', control, value: '\u6d4b\u8bd5', inputType: 'insertText' }))
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(committed)
    expect(listener).not.toHaveBeenCalled()

    core.text(r3({ kind: 'ordinaryInput', control, value: '\u4e0d\u540c', inputType: 'insertReplacementText' }))
    expect(core.getSnapshot().settings!.hotkey.value).toBe('\u4e0d\u540c')
    expect(listener).toHaveBeenCalledOnce()
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(client.saveSettings).not.toHaveBeenCalled()
  })

  it('commits settings ordinary-before-end and cancel paths once with zero Rust calls', async () => {
    const { core, client } = await startedSettingsCore()
    const control = core.getSnapshot().settings!.hotkey.key
    const listener = vi.fn()
    core.subscribe(listener)

    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'candidate', inputType: 'insertCompositionText' }))
    core.keyDown('Escape', true)
    const beforeCancel = core.getSnapshot()
    expect(client.hideLauncher).not.toHaveBeenCalled()
    core.text(r3({ kind: 'ordinaryInput', control, value: 'alph', inputType: 'deleteContentBackward' }))
    const cancelled = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(cancelled).not.toBe(beforeCancel)
    expect(core.getSnapshot()).toBe(cancelled)
    expect(core.getSnapshot().settings!.hotkey.value).toBe('alph')

    listener.mockClear()
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'ordinary-first', inputType: 'insertCompositionText' }))
    core.text(r3({ kind: 'ordinaryInput', control, value: 'ordinary-first', inputType: 'insertText' }))
    const ordinary = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(ordinary)
    expect(core.getSnapshot().settings!.hotkey.value).toBe('ordinary-first')
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(client.saveSettings).not.toHaveBeenCalled()
  })

  it('lets ordinary input commit before a later zero-effect boundary', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('ordinary-first'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u8ba1\u7b97\u5668', inputType: 'insertCompositionText' }))
    core.text(r3({ kind: 'ordinaryInput', control, value: '\u8ba1\u7b97\u5668', inputType: 'insertText' }))
    expect(client.searchApps).toHaveBeenCalledOnce()
    const committed = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(committed)
    expect(client.searchApps).toHaveBeenCalledOnce()
  })

  it('keeps composing keydown inert and commits a cancel delete once', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('cancel'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' }))
    vi.mocked(client.searchApps).mockClear()
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'calculate', inputType: 'insertCompositionText' }))
    const composing = core.getSnapshot()
    core.keyDown('Escape', true)
    expect(core.getSnapshot()).toBe(composing)
    expect(client.hideLauncher).not.toHaveBeenCalled()

    core.text(r3({ kind: 'ordinaryInput', control, value: 'cal', inputType: 'deleteContentBackward' }))
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(client.searchApps).toHaveBeenCalledWith({ query: 'cal', invocationId: 'cancel', querySequence: 3 })
    const cancelled = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(cancelled)

    core.keyDown('Escape', false)
    await vi.waitFor(() => expect(client.hideLauncher).toHaveBeenCalledOnce())
  })

  it('rejects no-owner, wrong-control, stale, and retired boundaries', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('ownership'))
    const control = core.getSnapshot().queryControl
    const initial = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    core.text(r3({ kind: 'compositionBoundary', control: control + 1000 }))
    expect(core.getSnapshot()).toBe(initial)

    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'draft', inputType: 'insertCompositionText' }))
    emit(shown('replacement'))
    expect(core.getSnapshot().queryControlValue).toBe(core.getSnapshot().query)
    const replaced = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(replaced)

    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'late', inputType: 'insertCompositionText' }))
    core.retireControl(control)
    core.retireControl(control)
    expect(core.getSnapshot().queryControlValue).toBe(core.getSnapshot().query)
    const retired = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'late', inputType: 'insertCompositionText' }))
    expect(core.getSnapshot()).toBe(retired)
    expect(client.searchApps).not.toHaveBeenCalled()
  })

  it('commits only the stored trusted draft, never a boundary sentinel', async () => {
    const { core, emit } = await startedCore()
    emit(shown('sentinel'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    const domOnlySentinel = 'script-sentinel'
    expect(domOnlySentinel).not.toBe('\u6d4b\u8bd5')
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toMatchObject({ query: '\u6d4b\u8bd5', queryControlValue: '\u6d4b\u8bd5' })
  })

  it('restores an unfinished command draft once and keeps exact-value edits idempotent', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('idempotent'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'ordinaryInput', control, value: '/unknown', inputType: 'insertText' }))
    vi.mocked(client.searchApps).mockClear()
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    const listener = vi.fn()
    core.subscribe(listener)

    core.text(r3({ kind: 'ordinaryInput', control, value: '/unknown', inputType: 'insertText' }))
    expect(listener).toHaveBeenCalledOnce()
    expect(client.searchApps).not.toHaveBeenCalled()
    const restored = core.getSnapshot()
    listener.mockClear()
    core.text(r3({ kind: 'ordinaryInput', control, value: '/unknown', inputType: 'insertFromPaste' }))
    expect(core.getSnapshot()).toBe(restored)
    expect(listener).not.toHaveBeenCalled()

    vi.mocked(client.searchApps).mockResolvedValueOnce({ requestId: 'old-empty', items: [] })
    emit(shown('idempotent-rerun', 'launcher', 'settingsFailed'))
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot()).toMatchObject({
      query: '',
      querySequence: 1,
      results: [],
      selectedIndex: -1,
      shownNotice: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
    })

    vi.mocked(client.searchApps).mockClear()
    listener.mockClear()
    core.text(r3({ kind: 'ordinaryInput', control, value: 'other', inputType: 'insertText' }))
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(listener).toHaveBeenCalledOnce()
  })

  it('freezes the four-record protocol and the correlated native end source', () => {
    for (const required of ['compositionStart', 'compositionInput', 'ordinaryInput', 'compositionBoundary']) {
      expect(protocolSource).toContain(required)
    }
    for (const forbidden of ['compositionUpdate', 'compositionEnd']) expect(protocolSource).not.toContain(forbidden)

    // @ts-expect-error A boundary must never carry text.
    const withValue: ClassifiedTextRecord = { kind: 'compositionBoundary', control: 1, value: 'forbidden' }
    // @ts-expect-error A boundary must never carry CompositionEvent data.
    const withData: ClassifiedTextRecord = { kind: 'compositionBoundary', control: 1, data: 'forbidden' }
    // @ts-expect-error A boundary must never carry input metadata.
    const withInputType: ClassifiedTextRecord = { kind: 'compositionBoundary', control: 1, inputType: 'insertText' }
    expect([withValue, withData, withInputType]).toHaveLength(3)

    const endStart = nativeInputSource.indexOf('const onEnd')
    const inputStart = nativeInputSource.indexOf('const onInput', endStart)
    const endBody = nativeInputSource.slice(endStart, inputStart)
    expect(endStart).toBeGreaterThanOrEqual(0)
    expect(inputStart).toBeGreaterThan(endStart)
    expect(endBody.indexOf('compositionActive')).toBeGreaterThanOrEqual(0)
    expect(endBody.indexOf('compositionActive = false')).toBeGreaterThan(endBody.indexOf('compositionActive'))
    expect(endBody.indexOf("kind: 'compositionBoundary'")).toBeGreaterThan(endBody.indexOf('compositionActive = false'))
    expect(endBody).not.toContain('.data')
    expect(endBody).not.toContain('.value')
    expect(nativeInputSource.match(/input\.addEventListener\(/g)).toHaveLength(3)
    expect(nativeInputSource.match(/input\.removeEventListener\(/g)).toHaveLength(3)
  })

  it('keeps untrusted, no-start, wrong-target, and post-unbind DOM events inert', () => {
    const input = document.createElement('input')
    const other = document.createElement('input')
    const emit = vi.fn()
    const unbind = bindNativeTextInput(input, 91, emit)
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    input.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: '\u6d4b' }))
    input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'sentinel' }))
    other.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'sentinel' }))
    expect(emit).not.toHaveBeenCalled()
    unbind()
    unbind()
    input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'sentinel' }))
    expect(emit).not.toHaveBeenCalled()
  })
})

describe('native trust', () => {
  it('emits nothing for untrusted raw DOM events and unbinds idempotently', () => {
    const input = document.createElement('input')
    const emit = vi.fn()
    const unbind = bindNativeTextInput(input, 7, emit)
    input.value = '中'
    input.dispatchEvent(new CompositionEvent('compositionstart', { data: '', bubbles: true }))
    input.dispatchEvent(new CompositionEvent('compositionupdate', { data: '中', bubbles: true }))
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertCompositionText', data: '中', bubbles: true }))
    input.dispatchEvent(new CompositionEvent('compositionend', { data: '中', bubbles: true }))
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'x', bubbles: true }))
    expect(emit).not.toHaveBeenCalled()

    unbind()
    unbind()
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'x', bubbles: true }))
    expect(emit).not.toHaveBeenCalled()
  })
})

describe('settings ownership', () => {
  it('keeps recorder ownership across idle peer unmount and discards a retired arm response', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore()
    const armed = deferred<void>()
    const released = deferred<void>()
    vi.mocked(client.setHotkeyRecording).mockReturnValueOnce(armed.promise).mockReturnValueOnce(released.promise)
    const host = document.createElement('div')
    document.body.append(host)
    const root = createRoot(host)
    const renderRecorders = (labels: string[]) => root.render(<>{labels.map((label) => <HotkeyRecorderInput key={label} label={label} value="" onCommit={() => {}} onRecordingPhase={core.setHotkeyRecordingPhase} />)}</>)
    try {
      await act(async () => renderRecorders(['launcher', 'screenshot', 'plugin command']))
      await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="录制launcher"]')!.click())
      await act(async () => renderRecorders(['launcher', 'plugin command']))
      expect(client.setHotkeyRecording).toHaveBeenCalledExactlyOnceWith({ recording: true })
      const peer = host.querySelector<HTMLButtonElement>('button[aria-label="录制plugin command"]')!
      expect(peer.disabled).toBe(true)
      await act(async () => renderRecorders(['plugin command']))
      await act(async () => armed.resolve())
      expect(client.setHotkeyRecording).toHaveBeenLastCalledWith({ recording: false })
      expect(peer.disabled).toBe(true)
      expect(host.querySelector<HTMLInputElement>('input')!.disabled).toBe(true)
      await act(async () => released.resolve())
      await act(async () => peer.click())
      expect(host.querySelector<HTMLInputElement>('input')!.disabled).toBe(false)
      expect(client.setHotkeyRecording).toHaveBeenNthCalledWith(3, { recording: true })
    } finally {
      await act(async () => { armed.resolve(); released.resolve(); root.unmount() })
      host.remove()
      core.destroy()
    }
  })

  it.each(['keyboard', 'native'] as const)('retries the unavailable saved screenshot chord through %s recording', async (source) => {
    installMatchMedia(false)
    const { core, client } = await settingsCore({ ...settingsFixture, screenshotHotkey: 'Ctrl+P', screenshotHotkeyStatus: 'unavailable' })
    const mounted = await mountLauncherView(core)
    try {
      const input = mounted.host.querySelector<HTMLInputElement>('input[aria-label="截图快捷键"]')!
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制截图快捷键"]')!.click())
      await act(async () => {
        if (source === 'native') window.dispatchEvent(new CustomEvent('uipilot-hotkey-recording-current', { detail: 'Ctrl+P' }))
        else {
          input.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true }))
          input.dispatchEvent(new KeyboardEvent('keyup', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true }))
        }
      })
      expect(client.setScreenshotHotkey).toHaveBeenCalledExactlyOnceWith({ input: { hotkey: 'Ctrl+P' } })
      expect(client.saveHotkey).not.toHaveBeenCalled()
    } finally { await mounted.unmount() }
  })

  it('keeps one recorder owner while arming and releasing before a peer can record', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore()
    const armed = deferred<void>()
    const released = deferred<void>()
    let nativeRecording = false
    vi.mocked(client.setHotkeyRecording).mockImplementationOnce(async () => { await armed.promise; nativeRecording = true })
      .mockImplementationOnce(async () => { await released.promise; nativeRecording = false })
      .mockImplementation(async ({ recording }) => { nativeRecording = recording })
    const mounted = await mountLauncherView(core)
    try {
      const launcher = mounted.host.querySelector<HTMLInputElement>('input[aria-label="快捷键"]')!
      const screenshot = mounted.host.querySelector<HTMLInputElement>('input[aria-label="截图快捷键"]')!
      const first = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制快捷键"]')!
      const second = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制截图快捷键"]')!
      await act(async () => { first.click(); second.click() })
      expect(second.disabled).toBe(true)
      expect(client.setHotkeyRecording).toHaveBeenCalledTimes(1)
      await act(async () => armed.resolve())
      expect(document.activeElement).toBe(launcher)
      expect(launcher.disabled).toBe(false)
      expect(screenshot.disabled).toBe(true)
      expect(nativeRecording).toBe(true)
      await act(async () => launcher.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
      expect(second.disabled).toBe(true)
      await act(async () => released.resolve())
      expect(second.disabled).toBe(false)
      await act(async () => second.click())
      expect(document.activeElement).toBe(screenshot)
      expect(screenshot.disabled).toBe(false)
      expect(nativeRecording).toBe(true)
      expect(client.setHotkeyRecording).toHaveBeenNthCalledWith(3, { recording: true })
      expect(client.startScreenshot).not.toHaveBeenCalled()
    } finally {
      await act(async () => { armed.resolve(); released.resolve() })
      await mounted.unmount()
    }
  })

  it('shows the dedicated screenshot hotkey recorder without a default binding', async () => {
    installMatchMedia(false)
    const { core } = await settingsCore()
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('input[aria-label="截图快捷键"]')
    expect(input).not.toBeNull()
    expect(input?.value).toBe(''); expect(input?.placeholder).toBe('未设置')
    expect(mounted.host.textContent).toContain('截图快捷键')
    await mounted.unmount()
  })
  it('waits for host recording suppression before accepting screenshot shortcut input', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore()
    const armed = deferred<void>()
    vi.mocked(client.setHotkeyRecording).mockReturnValueOnce(armed.promise)
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('input[aria-label="截图快捷键"]')!
    const record = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制截图快捷键"]')!
    await act(async () => record.click())
    expect(client.setHotkeyRecording).toHaveBeenCalledWith({ recording: true })
    expect(input.disabled).toBe(true)
    await act(async () => armed.resolve())
    expect(input.disabled).toBe(false)
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true, cancelable: true }))
      input.dispatchEvent(new KeyboardEvent('keyup', { key: 'p', code: 'KeyP', ctrlKey: true, bubbles: true, cancelable: true }))
    })
    expect(client.setScreenshotHotkey).toHaveBeenCalledWith({ input: { hotkey: 'Ctrl+P' } })
    expect(client.startScreenshot).not.toHaveBeenCalled()
    expect(client.setHotkeyRecording).toHaveBeenLastCalledWith({ recording: false })
    await mounted.unmount()
  })
  it('clears only the screenshot hotkey and reloads its authoritative status', async () => {
    const { core, client } = await settingsCore({ ...settingsFixture, screenshotHotkey: 'Ctrl+P', screenshotHotkeyStatus: 'unavailable' })
    vi.mocked(client.loadSettings).mockResolvedValue(settingsFixture)
    await core.saveScreenshotHotkey(null)
    await vi.waitFor(() => expect(core.getSnapshot().settings?.screenshotHotkeyStatus).toBe('unset'))
    expect(client.setScreenshotHotkey).toHaveBeenCalledWith({ input: { hotkey: null } })
    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(client.setPublicPluginCommandHotkey).not.toHaveBeenCalled()
  })
  it('shows the full screenshot directory and commits picker/reset through independent preferences', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore()
    const mounted = await mountLauncherView(core)
    expect(mounted.host.textContent).toContain('截图默认位置')
    expect(mounted.host.querySelector<HTMLInputElement>('input[aria-label="截图默认位置"]')?.value).toBe('C:\\Users\\demo\\OneDrive\\Desktop')
    vi.mocked(client.selectScreenshotSaveDirectory).mockResolvedValueOnce({ directory: 'D:\\Shots', path: 'D:\\Shots' })
    await act(async () => { await core.selectScreenshotSaveDirectory() })
    expect(core.getSnapshot().settings?.screenshotSaveDirectory).toBe('D:\\Shots')
    expect(client.saveSettings).not.toHaveBeenCalled()
    vi.mocked(client.setScreenshotSaveDirectory).mockResolvedValueOnce({ directory: null, path: 'C:\\Users\\demo\\OneDrive\\Desktop' })
    await act(async () => { await core.restoreScreenshotSaveDirectory() })
    expect(client.setScreenshotSaveDirectory).toHaveBeenCalledWith({ input: { directory: null } })
    expect(core.getSnapshot().settings?.screenshotSaveDirectory).toBeNull()
    await mounted.unmount()
  })
  it('keeps the old screenshot directory on cancellation and persistence failure', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.selectScreenshotSaveDirectory).mockResolvedValueOnce(null)
    await core.selectScreenshotSaveDirectory()
    expect(core.getSnapshot().settings?.screenshotSaveDirectoryPath).toBe('C:\\Users\\demo\\OneDrive\\Desktop')
    vi.mocked(client.selectScreenshotSaveDirectory).mockRejectedValueOnce(new Error('write failed'))
    await core.selectScreenshotSaveDirectory()
    expect(core.getSnapshot().settings?.screenshotSaveDirectoryPath).toBe('C:\\Users\\demo\\OneDrive\\Desktop')
    expect(core.getSnapshot().status).toContain('保存目录失败')
    expect(core.getSnapshot().settings?.operation).toBeUndefined()
  })
  it('ignores the stale screenshot picker after leaving settings and reloads authoritative state', async () => {
    const { core, client, emit } = await settingsCore()
    const pending = deferred<{ directory: string | null; path: string | null } | null>()
    vi.mocked(client.selectScreenshotSaveDirectory).mockReturnValueOnce(pending.promise)
    const saving = core.selectScreenshotSaveDirectory()
    expect(core.getSnapshot().settings?.operation).toBe('screenshotDirectory')
    core.setAutostart(true)
    expect(client.saveSettings).not.toHaveBeenCalled()
    emit(shown('next-launcher', 'launcher'))
    emit(shown('next-settings', 'settings'))
    pending.resolve({ directory: 'D:\\stale', path: 'D:\\stale' })
    await saving
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
    expect(core.getSnapshot().settings?.screenshotSaveDirectoryPath).toBe(settingsFixture.screenshotSaveDirectoryPath)
  })
  async function settingsCore(settings: SettingsView = settingsFixture) {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settings)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('settings', 'settings'))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))
    return { core, ...fake }
  }

  it('keeps a launcher-target settings failure latched after entering settings', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings)
      .mockResolvedValueOnce(settingsFixture)
      .mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()

    fake.emit(shown('notice-launcher', 'launcher', 'settingsFailed'))
    fake.emit(shown('notice-settings', 'settings'))

    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
  })

  it('latches a settings-target lifecycle failure before applying its owner load', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings)
      .mockResolvedValueOnce(settingsFixture)
      .mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()

    fake.emit(shown('notice-settings', 'settings', 'settingsFailed'))

    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
  })

  it('does not let startup settings hydrate the settings view without its epoch owner', async () => {
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    const current = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise).mockReturnValueOnce(current.promise)
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())

    fake.emit(shown('settings-b', 'settings'))
    expect(core.getSnapshot().settings).toBeUndefined()
    startup.resolve({ ...settingsFixture, hotkey: 'DoubleCtrl', filePreviewEnabled: false })
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().settings).toBeUndefined()

    current.resolve({ ...settingsFixture, hotkey: 'DoubleAlt' })
    await starting
    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleAlt'))
  })

  it.skip('hydrates preview from startup after leaving settings for launcher', async () => {
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())

    fake.emit(shown('settings-b', 'settings'))
    fake.emit(shown('launcher-after-b'))
    startup.resolve({ ...settingsFixture, filePreviewEnabled: false })
    await starting

    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find preview', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(false))
  })

  it.skip('does not let startup preview hydration overwrite a newer durable preference', async () => {
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())

    fake.emit(shown('settings-b', 'settings'))
    fake.emit(shown('launcher-after-b'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find preview', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file).toBeDefined())
    core.setFilePreviewEnabled(false)
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: false }),
    )

    startup.resolve({ ...settingsFixture, filePreviewEnabled: true })
    await starting
    expect(core.getSnapshot().file?.previewEnabled).toBe(false)
  })

  it('uses only settings C owner after startup succeeds across B and launcher', async () => {
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    const current = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise).mockReturnValueOnce(current.promise)
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())

    fake.emit(shown('settings-b', 'settings'))
    fake.emit(shown('launcher-between'))
    fake.emit(shown('settings-c', 'settings'))
    startup.resolve({ ...settingsFixture, hotkey: 'DoubleCtrl' })
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    current.resolve({ ...settingsFixture, hotkey: 'DoubleAlt' })
    await starting

    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleAlt'))
  })

  it('uses only settings C owner after startup fails across B and launcher', async () => {
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    const current = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise).mockReturnValueOnce(current.promise)
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())

    fake.emit(shown('settings-b', 'settings'))
    fake.emit(shown('launcher-between'))
    fake.emit(shown('settings-c', 'settings'))
    startup.reject({ code: 'settingsFailed', message: 'private startup error' })
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    expect(core.getSnapshot().status).not.toContain('private startup')
    current.resolve({ ...settingsFixture, hotkey: 'DoubleAlt' })
    await starting

    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleAlt'))
  })

  it('persists autostart immediately and confirms with the authoritative snapshot', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: true })
    core.setAutostart(true)
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'Alt+Space',
        autostart: true,
        theme: 'system',
        webSearchEngine: 'bing',
      },
    })
    await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(true))
  })

  it('saves a search engine immediately and rolls back a failure without leaving settings', async () => {
    const { core, client } = await settingsCore()
    const setEngine = (core as typeof core & {
      setWebSearchEngine(engine: 'bing' | 'baidu' | 'google'): void
    }).setWebSearchEngine
    expect(setEngine).toBeTypeOf('function')
    vi.mocked(client.setWebSearchEngine).mockRejectedValueOnce({
      code: 'settingsFailed',
      message: 'private engine failure',
    })
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)

    setEngine.call(core, 'baidu')

    expect(core.getSnapshot()).toMatchObject({
      view: 'settings',
      settings: {
        webSearchEngine: 'baidu',
        operation: 'webSearchEngine',
        readOnly: true,
      },
    })
    expect(client.setWebSearchEngine).toHaveBeenCalledWith({ preference: { engine: 'baidu' } })
    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    expect(core.getSnapshot()).toMatchObject({
      view: 'settings',
      settings: {
        webSearchEngine: 'bing',
        needsReload: false,
        readOnly: false,
      },
      status: '无法保存搜索引擎设置。',
    })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private engine failure')
  })
  it('publishes theme immediately and persists through the narrow command', async () => {
    const { core, client } = await settingsCore()
    const save = deferred<void>()
    vi.mocked(client.setThemePreference).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: 'dark' })

    core.setThemePreference('dark')

    expect(core.getSnapshot().theme).toBe('dark')
    expect(core.getSnapshot().settings).toMatchObject({ theme: 'dark', operation: 'theme' })
    expect(client.setThemePreference).toHaveBeenCalledWith({ preference: { theme: 'dark' } })
    expect(client.saveSettings).not.toHaveBeenCalled()
    save.resolve()
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
  })

  it('rolls back a failed theme save without requiring restart', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.setThemePreference).mockRejectedValueOnce({
      code: 'settingsFailed',
      message: 'private theme failure',
    })

    core.setThemePreference('dark')

    await vi.waitFor(() => expect(core.getSnapshot().theme).toBe('system'))
    expect(core.getSnapshot().settings).toMatchObject({
      theme: 'system',
      needsReload: false,
      readOnly: false,
    })
    expect(core.getSnapshot().status).toBe('无法保存风格设置。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private theme failure')
  })

  it('reconciles a stale theme mutation through the current settings epoch', async () => {
    const { core, client, emit } = await settingsCore()
    const save = deferred<void>()
    vi.mocked(client.setThemePreference).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: 'dark' })

    core.setThemePreference('dark')
    emit(shown('theme-launcher-between'))
    emit(shown('theme-settings-current', 'settings'))
    save.resolve()

    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    await vi.waitFor(() => expect(core.getSnapshot().settings).toMatchObject({
      theme: 'dark',
      readOnly: false,
      needsReload: false,
    }))
  })

  it('owns the save operation before publishing an optimistic autostart value', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: true })
    const unsubscribe = core.subscribe(() => {
      const settings = core.getSnapshot().settings
      if (settings?.autostart === true && settings.operation === undefined) core.setAutostart(false)
    })

    core.setAutostart(true)
    unsubscribe()

    expect(client.saveSettings).toHaveBeenCalledOnce()
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: { hotkey: 'Alt+Space', autostart: true, theme: 'system', webSearchEngine: 'bing' },
    })
    await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(true))
  })

  it('marks a new settings epoch loading before publishing it to subscribers', async () => {
    const { core, client, emit } = await settingsCore()
    const currentLoad = deferred<SettingsView>()
    vi.mocked(client.loadSettings).mockReturnValueOnce(currentLoad.promise)
    const previousEpoch = core.getSnapshot().viewEpoch
    const unsubscribe = core.subscribe(() => {
      const snapshot = core.getSnapshot()
      if (snapshot.viewEpoch > previousEpoch && snapshot.settings?.readOnly === false) core.setAutostart(true)
    })

    emit(shown('settings-publish-admission', 'settings'))
    unsubscribe()

    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'loading', readOnly: true, operation: 'load' })
    currentLoad.resolve(settingsFixture)
    await vi.waitFor(() => expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'ready', readOnly: false }))
  })

  it('does not let a stale settings load clear the current launcher status', async () => {
    const { core, client, emit } = await settingsCore()
    const staleLoad = deferred<SettingsView>()
    vi.mocked(client.loadSettings).mockReturnValueOnce(staleLoad.promise)

    emit(shown('stale-status-settings', 'settings'))
    emit(shown('stale-status-launcher', 'launcher'))
    core.failInitialization()
    expect(core.getSnapshot().status).toBe('操作不可用，请重试。')

    staleLoad.resolve(settingsFixture)

    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
    expect(core.getSnapshot().status).toBe('操作不可用，请重试。')
  })

  it('does not let a current settings load clear a newer hide failure', async () => {
    const { core, client } = await settingsCore()
    const currentLoad = deferred<SettingsView>()
    vi.mocked(client.loadSettings).mockReturnValueOnce(currentLoad.promise)
    vi.mocked(client.hideLauncher).mockRejectedValueOnce({ code: 'windowFailed', message: 'private hide failure' })

    const loading = core.reloadSettings()
    await core.requestHide()
    expect(core.getSnapshot().status).toBe('窗口操作失败。')
    currentLoad.resolve(settingsFixture)
    await loading

    expect(core.getSnapshot().status).toBe('窗口操作失败。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private hide failure')
  })

  it('applies the authoritative snapshot and fails closed after an autostart save error', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: false })
    core.setAutostart(true)
    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    expect(core.getSnapshot().settings).toMatchObject({ readOnly: true, needsReload: true })
    expect(core.getSnapshot().settings!.autostart).toBe(false)
    expect(core.getSnapshot().status).toBe('快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
  })

  it('resets visible settings through one existing save command', async () => {
    const { core, client } = await settingsCore({ ...settingsFixture, theme: 'dark' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce({
      ...settingsFixture,
      hotkey: 'Shift+Space',
      theme: 'system',
    })

    await core.resetSettings()

    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'Shift+Space',
        autostart: false,
        theme: 'system',
        webSearchEngine: 'bing',
      },
    })
    expect(client.saveSettings).toHaveBeenCalledOnce()
    expect(client.setThemePreference).not.toHaveBeenCalled()
  })

  it('retires form controls before fresh replacement', async () => {
    const { core, client } = await settingsCore()
    const original = core.getSnapshot().settings!
    const oldKey = original.hotkey.key
    core.text({ kind: 'compositionStart', control: oldKey })
    core.text({ kind: 'compositionInput', control: oldKey, value: 'uncommitted', inputType: 'insertCompositionText' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await core.reloadSettings()
    const replacement = core.getSnapshot().settings!
    const replacedSnapshot = core.getSnapshot()
    core.text({ kind: 'compositionBoundary', control: oldKey })
    core.text({ kind: 'compositionInput', control: oldKey, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(replacedSnapshot)
    expect(replacement.hotkey.key).toBeGreaterThan(oldKey)

    const replaceStart = launcherCoreSource.indexOf('function replaceSettings')
    const replaceRetire = launcherCoreSource.indexOf('retireControl(control.key)', replaceStart)
    const replaceAssign = launcherCoreSource.indexOf('model.settings =', replaceStart)
    expect(replaceRetire).toBeGreaterThan(replaceStart)
    expect(replaceAssign).toBeGreaterThan(replaceRetire)
  })

  it('reconciles a stale autostart save through the current settings epoch', async () => {
    const { core, client, emit } = await settingsCore()
    const save = deferred<void>()
    vi.mocked(client.saveSettings).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: true })
    core.setAutostart(true)
    emit(shown('launcher-between'))
    emit(shown('new-settings', 'settings'))
    save.resolve()
    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(true))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: false, readOnly: false })
  })

  it('reconciles a stale autostart failure through the current settings epoch and stays uncertain', async () => {
    const { core, client, emit } = await settingsCore()
    const save = deferred<void>()
    vi.mocked(client.saveSettings).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    core.setAutostart(true)
    emit(shown('autostart-failure-launcher', 'launcher'))
    emit(shown('autostart-failure-settings', 'settings'))

    save.reject({ code: 'settingsFailed', message: 'private stale failure' })

    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.autostart).toBe(false))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true, loadStatus: 'ready' })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private stale failure')
  })

  it('records hotkey via canonical setter without saving', async () => {
    const { core, client } = await settingsCore()
    core.setHotkeyCanonical('DoubleCtrl')
    expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl')
    expect(client.saveSettings).not.toHaveBeenCalled()
  })

  it('records hotkey through dedicated save without invoking full settings save', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, hotkey: 'DoubleCtrl' })

    await core.saveHotkeyCanonical('DoubleCtrl')

    expect(client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } })
    expect(client.saveSettings).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl'))
  })

  it('records DoubleCtrl only after explicit recording and returns focus to the recorder button', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await settingsCore()
    const mounted = await mountLauncherView(core)
    const settings = core.getSnapshot().settings!
    const input = mounted.host.querySelector<HTMLInputElement>(`input[name="settings-hotkey-${settings.hotkey.key}"]`)
    if (!input) throw new Error('settings hotkey input missing')
    const recorder = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')
    if (!recorder) throw new Error('settings hotkey recorder button missing')

    expect(input.disabled).toBe(true)
    expect(input.tabIndex).toBe(-1)
    await act(async () => recorder.click())
    expect(recorder.textContent?.trim()).toBe('取消录制')
    expect(input.disabled).toBe(false)
    expect(document.activeElement).toBe(input)

    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
      input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Control', code: 'ControlLeft', bubbles: true, cancelable: true }))
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
    })

    expect(client.saveHotkey).not.toHaveBeenCalled()
    expect(core.getSnapshot().view).toBe('settings')
    await act(async () => input.dispatchEvent(
      new KeyboardEvent('keyup', { key: 'Control', code: 'ControlLeft', bubbles: true, cancelable: true }),
    ))
    expect(client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } })
    await vi.waitFor(() => expect(input.disabled).toBe(true))
    await vi.waitFor(() => expect(recorder.textContent?.trim()).toBe('重新录制'))
    await vi.waitFor(() => expect(document.activeElement).toBe(recorder))
    await act(async () => emit(shown('same-hotkey-after-recording', 'launcher')))
    expect(core.getSnapshot().view).toBe('settings')
    expect(document.activeElement).toBe(recorder)
    await act(async () => emit(shown('later-hotkey-after-recording', 'launcher')))
    expect(core.getSnapshot().view).toBe('launcher')
    await mounted.unmount()
  })

  it('completes recording without keyup when the captured hotkey matches the current value', async () => {
    installMatchMedia(false)
    const { core, client, emit } = await settingsCore({ ...settingsFixture, hotkey: 'Shift+Space' })
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')
    if (!input) throw new Error('settings hotkey input missing')
    const recorder = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')
    if (!recorder) throw new Error('settings hotkey recorder button missing')

    await act(async () => recorder.click())
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'Shift', code: 'ShiftLeft', shiftKey: true, bubbles: true, cancelable: true,
      }))
      input.dispatchEvent(new KeyboardEvent('keydown', {
        key: ' ', code: 'Space', shiftKey: true, bubbles: true, cancelable: true,
      }))
    })

    expect(client.saveHotkey).not.toHaveBeenCalled()
    expect(input.disabled).toBe(true)
    expect(recorder.textContent?.trim()).toBe('重新录制')
    expect(document.activeElement).toBe(recorder)
    await act(async () => emit(shown('same-hotkey-after-keydown', 'launcher')))
    expect(core.getSnapshot().view).toBe('settings')
    await act(async () => emit(shown('normal-hotkey-after-same-recording', 'launcher')))
    expect(core.getSnapshot().view).toBe('launcher')
    await mounted.unmount()
  })

  it('completes recording when Host suppresses the already registered current hotkey', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore({ ...settingsFixture, hotkey: 'Shift+Space' })
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')
    if (!input) throw new Error('settings hotkey input missing')
    const recorder = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')
    if (!recorder) throw new Error('settings hotkey recorder button missing')

    await act(async () => recorder.click())
    await act(async () => window.dispatchEvent(new Event('uipilot-hotkey-recording-current')))

    expect(client.saveHotkey).not.toHaveBeenCalled()
    expect(core.getSnapshot().view).toBe('settings')
    expect(input.disabled).toBe(true)
    expect(recorder.textContent?.trim()).toBe('重新录制')
    expect(document.activeElement).toBe(recorder)
    await mounted.unmount()
  })

  it('restores the authoritative hotkey and latches uncertainty after dedicated save failure', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.saveHotkey).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)

    await core.saveHotkeyCanonical('DoubleCtrl')

    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    expect(core.getSnapshot().settings!.hotkey.value).toBe('Alt+Space')
    expect(core.getSnapshot().settings!.autostart).toBe(false)
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
  })

  it('recovers a known hotkey conflict and permits recording and general settings again', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.saveHotkey).mockRejectedValueOnce({ code: 'hotkeyConflict', message: 'private conflict detail' })

    await core.saveHotkeyCanonical('Control+N')

    await vi.waitFor(() => expect(core.getSnapshot().settings).toMatchObject({ readOnly: false, needsReload: false, loadStatus: 'ready' }))
    expect(core.getSnapshot().settings!.hotkey.value).toBe('Alt+Space')
    expect(core.getSnapshot().status).toBe('快捷键已被命令占用，请选择其他快捷键。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private conflict')
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, hotkey: 'DoubleCtrl' })
    await core.saveHotkeyCanonical('DoubleCtrl')
    await vi.waitFor(() => expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl'))
    core.setAutostart(true)
    expect(client.saveSettings).toHaveBeenCalledOnce()
  })

  it('restores confirmed preferences after reset conflict even if recovery load fails', async () => {
    const original: SettingsView = { ...settingsFixture, autostart: true, theme: 'dark', webSearchEngine: 'google' }
    const { core, client } = await settingsCore(original)
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'hotkeyConflict', message: 'private reset conflict' })
    vi.mocked(client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private reload failure' })

    await core.resetSettings()

    await vi.waitFor(() => expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'error', readOnly: true, needsReload: false }))
    expect(core.getSnapshot()).toMatchObject({ theme: 'dark', settings: { autostart: true, webSearchEngine: 'google', hotkey: { value: 'Alt+Space' } } })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private')
    await core.reloadSettings()
    expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'ready', readOnly: false, needsReload: false })
    core.setAutostart(false)
    expect(client.saveSettings).toHaveBeenLastCalledWith({ settings: { hotkey: 'Alt+Space', autostart: false, theme: 'dark', webSearchEngine: 'google' } })
  })

  it('recovers a reset conflict through an authoritative reload', async () => {
    const original: SettingsView = { ...settingsFixture, autostart: true, theme: 'dark', webSearchEngine: 'google' }
    const { core, client } = await settingsCore(original)
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'hotkeyConflict' })
    await core.resetSettings()
    await vi.waitFor(() => expect(core.getSnapshot().settings?.readOnly).toBe(false))
    expect(core.getSnapshot()).toMatchObject({ theme: 'dark', settings: { needsReload: false, autostart: true, webSearchEngine: 'google', hotkey: { value: 'Alt+Space' } } })
    expect(core.getSnapshot().status).toBe('快捷键已被命令占用，请选择其他快捷键。')
  })

  it('does not publish a stale conflict over a newer launcher status', async () => {
    const { core, client, emit } = await settingsCore()
    const save = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(save.promise)
    const pending = core.saveHotkeyCanonical('Control+N')
    emit(shown('conflict-new-launcher', 'launcher'))
    core.failInitialization()
    save.reject({ code: 'hotkeyConflict', message: 'private stale conflict' })
    await pending
    expect(core.getSnapshot().status).toBe('操作不可用，请重试。')
    emit(shown('conflict-new-settings', 'settings'))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.readOnly).toBe(false))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: false, hotkey: { value: 'Alt+Space' } })
    expect(core.getSnapshot().status).not.toContain('已被命令占用')
  })

  it('keeps one settings operation while dedicated hotkey save is pending', async () => {
    const { core, client } = await settingsCore()
    const pendingHotkey = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)

    const pending = core.saveHotkeyCanonical('DoubleCtrl')
    core.setAutostart(true)
    void core.saveHotkeyCanonical('DoubleAlt')

    expect(client.saveHotkey).toHaveBeenCalledOnce()
    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(core.getSnapshot().settings).toMatchObject({ operation: 'hotkey' })
    pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
    await pending
  })

  it('reconciles stale dedicated hotkey success through the newer settings view', async () => {
    const { core, client, emit } = await settingsCore()
    const pendingHotkey = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, hotkey: 'DoubleCtrl' })

    const pending = core.saveHotkeyCanonical('DoubleCtrl')
    emit(shown('new-settings', 'settings'))
    pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
    await pending

    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleCtrl'))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: false, readOnly: false })
  })

  it('reconciles stale dedicated hotkey failure through the newer settings view and stays uncertain', async () => {
    const { core, client, emit } = await settingsCore()
    const pendingHotkey = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)

    const pending = core.saveHotkeyCanonical('DoubleCtrl')
    emit(shown('hotkey-failure-launcher', 'launcher'))
    emit(shown('hotkey-failure-settings', 'settings'))
    pendingHotkey.reject({ code: 'settingsFailed', message: 'private stale hotkey failure' })
    await pending

    await vi.waitFor(() => expect(client.loadSettings).toHaveBeenCalledTimes(3))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('Alt+Space'))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true, loadStatus: 'ready' })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private stale hotkey failure')
  })

  it('retries an ordinary settings load error without setting uncertainty', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed' })

    await core.reloadSettings()
    expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'error', needsReload: false, readOnly: true })

    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await core.reloadSettings()
    expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'ready', needsReload: false, readOnly: false })
  })

  it('keeps uncertainty after a failed recovery load later retries successfully', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'settingsFailed' })
    vi.mocked(client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed' })

    core.setAutostart(true)
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('error'))
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })

    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await core.reloadSettings()
    expect(core.getSnapshot().settings).toMatchObject({ loadStatus: 'ready', needsReload: true, readOnly: true })
  })

})

describe('plugin settings ownership', () => {
  const pluginV1 = installedPlugin()
  const pluginV2 = installedPlugin('2.0.0', '# Math 2')

  async function pluginCore(
    list: Promise<PluginInventorySnapshot> | PluginInventorySnapshot = pluginInventory([pluginV1]),
  ) {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPlugins).mockReturnValueOnce(Promise.resolve(list))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('plugin-settings', 'settings'))
    void core.activatePlugins()
    return { core, ...fake }
  }

  it('keeps list loading, error, empty, and retry independent from settings state', async () => {
    const pending = deferred<PluginInventorySnapshot>()
    const { core, client } = await pluginCore(pending.promise)
    expect(core.getSnapshot().plugins).toMatchObject({ status: 'loading', items: [] })

    pending.reject({ code: 'pluginListFailed', message: 'private backend text' })
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('error'))
    expect(core.getSnapshot().plugins?.error).toBe('无法加载插件清单。')
    expect(core.getSnapshot().settings?.autostart).toBe(false)

    vi.mocked(client.listPlugins).mockResolvedValueOnce(pluginInventory())
    await core.reloadPlugins()
    expect(core.getSnapshot().plugins).toMatchObject({ status: 'ready', items: [] })
    expect(core.getSnapshot().settings?.autostart).toBe(false)
  })

  it('ignores an older list response after reentering settings', async () => {
    const first = deferred<PluginInventorySnapshot>()
    const second = deferred<PluginInventorySnapshot>()
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPlugins).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('list-first', 'settings'))
    void core.activatePlugins()
    fake.emit(shown('list-launcher', 'launcher'))
    fake.emit(shown('list-second', 'settings'))
    void core.activatePlugins()

    second.resolve(pluginInventory([pluginV2], '2'))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '2.0.0' }))
    first.resolve(pluginInventory([pluginV1], '1'))
    await first.promise
    expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '2.0.0' })
    expect(fake.client.listPlugins).toHaveBeenCalledTimes(2)
  })

  it('never applies mutation rows directly and reconciles reload and delete outcomes', async () => {
    const { core, client } = await pluginCore()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    vi.mocked(client.listPlugins).mockResolvedValueOnce(pluginInventory([pluginV2], '2'))
    const reloading = core.reloadPlugin(pluginV1.id!)
    expect(core.getSnapshot().plugins?.items[0]).toMatchObject({ operation: 'reload' })
    await reloading
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '2.0.0' }))

    vi.mocked(client.deletePlugin).mockResolvedValueOnce({ revision: '3' })
    vi.mocked(client.listPlugins).mockResolvedValueOnce(pluginInventory([], '3'))
    const deleting = core.deletePlugin(pluginV1.id!)
    expect(core.getSnapshot().plugins?.items[0]).toMatchObject({ operation: 'delete' })
    await deleting
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items).toEqual([]))
  })

  it('keeps a pending plugin mutation owned across refresh and rejects a duplicate command', async () => {
    const { core, client } = await pluginCore()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    const mutation = deferred<{ revision: string }>()
    vi.mocked(client.reloadPlugin).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.listPlugins).mockResolvedValue(pluginInventory([pluginV1], '1'))

    void core.reloadPlugin(pluginV1.id!)
    await core.reloadPlugins()

    expect(core.getSnapshot().plugins?.items[0]).toMatchObject({ operation: 'reload' })
    void core.reloadPlugin(pluginV1.id!)
    expect(client.reloadPlugin).toHaveBeenCalledTimes(1)

    mutation.resolve({ revision: '2' })
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
  })

  it('installs a development-only plugin then reconciles from backend inventory', async () => {
    const source = developmentPlugin()
    const installed = installedPlugin()
    const { core, client } = await pluginCore(pluginInventory([source]))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    vi.mocked(client.listPlugins).mockResolvedValueOnce(pluginInventory([installed], '2'))

    await core.installPlugin(source.id!)

    expect(client.installPlugin).toHaveBeenCalledWith({ pluginId: source.id })
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed.state).toBe('valid'))
  })

  it('reconciles a stale reload after the new view first receives an old snapshot', async () => {
    const { core, client, emit } = await pluginCore()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    const mutation = deferred<{ revision: string }>()
    const enteredList = deferred<PluginInventorySnapshot>()
    const reconciliation = deferred<PluginInventorySnapshot>()
    vi.mocked(client.reloadPlugin).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.listPlugins)
      .mockReturnValueOnce(enteredList.promise)
      .mockReturnValueOnce(reconciliation.promise)
    void core.reloadPlugin(pluginV1.id!)

    emit(shown('plugin-launcher', 'launcher'))
    emit(shown('plugin-settings-next', 'settings'))
    void core.activatePlugins()
    enteredList.resolve(pluginInventory([pluginV1], '1'))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '1.0.0' }))
    mutation.resolve({ revision: '2' })
    await vi.waitFor(() => expect(client.listPlugins).toHaveBeenCalledTimes(3))
    expect(core.getSnapshot().plugins?.status).toBe('loading')
    reconciliation.resolve(pluginInventory([pluginV2], '2'))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '2.0.0' }))
  })

  it('reconciles a stale delete without applying the old row response directly', async () => {
    const { core, client, emit } = await pluginCore()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    const mutation = deferred<{ revision: string }>()
    const enteredList = deferred<PluginInventorySnapshot>()
    const reconciliation = deferred<PluginInventorySnapshot>()
    vi.mocked(client.deletePlugin).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.listPlugins)
      .mockReturnValueOnce(enteredList.promise)
      .mockReturnValueOnce(reconciliation.promise)
    void core.deletePlugin(pluginV1.id!)

    emit(shown('delete-launcher', 'launcher'))
    emit(shown('delete-settings-next', 'settings'))
    void core.activatePlugins()
    enteredList.resolve(pluginInventory([pluginV1], '1'))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items).toHaveLength(1))
    mutation.resolve({ revision: '2' })
    await vi.waitFor(() => expect(client.listPlugins).toHaveBeenCalledTimes(3))
    expect(core.getSnapshot().plugins?.status).toBe('loading')
    reconciliation.resolve(pluginInventory([], '2'))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items).toEqual([]))
  })

  it('drops a stale mutation error and reconciles the current view instead', async () => {
    const { core, client, emit } = await pluginCore()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    const mutation = deferred<{ revision: string }>()
    vi.mocked(client.reloadPlugin).mockReturnValueOnce(mutation.promise)
    vi.mocked(client.listPlugins)
      .mockResolvedValueOnce(pluginInventory([pluginV1], '1'))
      .mockResolvedValueOnce(pluginInventory([pluginV1], '1'))
    void core.reloadPlugin(pluginV1.id!)
    emit(shown('failure-launcher', 'launcher'))
    emit(shown('failure-settings-next', 'settings'))
    void core.activatePlugins()
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))

    mutation.reject({ code: 'pluginReloadFailed', message: 'private old error' })
    await vi.waitFor(() => expect(client.listPlugins).toHaveBeenCalledTimes(3))
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    expect(core.getSnapshot().plugins?.items[0]?.error).toBeUndefined()
    expect(document.body.textContent).not.toContain('private old error')
  })
})

describe('execute and hide continuation', () => {
  it('coalesces a late activation-refused result into the next eligible launcher notice', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'request',
      items: [findLauncherItem('app'), { resultId: 'result', title: 'App', activation: executeActivation }],
    })
    const execute = deferred<ExecuteOutcome>()
    vi.mocked(client.executeResult).mockReturnValueOnce(execute.promise)
    emit(shown('execute-old'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'app', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))
    core.keyDown('ArrowDown', false)
    core.keyDown('Enter', false)
    emit(shown('settings-new', 'settings'))
    execute.resolve({ status: 'activationRefusedLaunchRequested', message: 'raw backend text' })
    await execute.promise
    await Promise.resolve()
    emit(shown('notice-priority', 'launcher', 'settingsFailed'))
    expect(core.getSnapshot().shownNotice).toBe('快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。')
    emit(shown('eligible'))
    expect(core.getSnapshot().shownNotice).toBe('Windows 拒绝了前台切换，已发送启动请求')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('raw backend')
  })
})

describe('React view and accessibility', () => {
  it('groups nonempty searches into applications, favorites, and all features', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: `feature-groups-${request.querySequence}`,
      items: request.query === '' ? [
        {
          ...findLauncherItem(''),
          favorite: { target: { kind: 'builtin', feature: 'find' }, favorite: true },
        },
        {
          resultId: 'favorite-plugin',
          title: '/favorite-plugin',
          activation: {
            kind: 'pluginCompletion', completionText: '/favorite-plugin ',
            pluginId: 'com.uipilot.favorite-plugin', favorite: true,
          },
          favorite: {
            target: { kind: 'publicPlugin', pluginId: 'com.uipilot.favorite-plugin', commandId: 'default' },
            favorite: true,
          },
        },
        {
          resultId: 'web-search',
          title: '/web-search',
          iconKind: 'webSearch',
          activation: { kind: 'completion', completionText: '/web-search ' },
          favorite: { target: { kind: 'builtin', feature: 'webSearch' }, favorite: false },
        },
        { resultId: 'app', title: 'Demo App', activation: executeActivation },
      ] : [
        { resultId: 'app-search', title: 'Matched App', activation: executeActivation },
        {
          resultId: 'favorite-search-plugin',
          title: '/favorite-plugin',
          activation: {
            kind: 'pluginCompletion', completionText: '/favorite-plugin ',
            pluginId: 'com.uipilot.favorite-plugin', favorite: true,
          },
          favorite: {
            target: { kind: 'publicPlugin', pluginId: 'com.uipilot.favorite-plugin', commandId: 'default' },
            favorite: true,
          },
        },
        findLauncherItem(request.query),
      ],
    } as unknown as SearchResponse))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('feature-groups')))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(4))

    const sections = [...mounted.host.querySelectorAll<HTMLElement>('.result-section')]
    expect(sections.map((section) => section.querySelector('.result-section-title')?.textContent)).toEqual([
      '常用', '所有功能',
    ])
    expect(stylesSource).toMatch(
      /\.result-section-title\s*\{[^}]*color:\s*var\(--uipilot-ui-muted-foreground\);[^}]*font-weight:\s*400;/s,
    )
    expect([...sections[0]!.querySelectorAll('.result-title')].map((node) => node.textContent)).toEqual([
      '/find', '/favorite-plugin',
    ])
    expect([...sections[1]!.querySelectorAll('.result-title')].map((node) => node.textContent)).toEqual([
      '/web-search', 'Demo App',
    ])
    expect([...mounted.host.querySelectorAll('.result-title')].map((node) => node.textContent)).toEqual([
      '/find', '/favorite-plugin', '/web-search', 'Demo App',
    ])

    await act(async () => core.text({
      kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
      value: 'matched', inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(3))
    const searchSections = [...mounted.host.querySelectorAll<HTMLElement>('.result-section')]
    expect(searchSections.map((section) => section.querySelector('.result-section-title')?.textContent)).toEqual([
      '应用', '常用', '所有功能',
    ])
    expect(searchSections.map((section) => section.querySelector('.result-title')?.textContent)).toEqual([
      'Matched App', '/favorite-plugin', '/find',
    ])
    await mounted.unmount()
  })

  it('keeps the first result group heading visible when keyboard selection wraps to its first item', async () => {
    installMatchMedia(false)
    const scroll = vi.fn()
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: scroll })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValue({
      requestId: 'group-wrap-scroll',
      items: [
        {
          ...findLauncherItem(''),
          favorite: { target: { kind: 'builtin', feature: 'find' }, favorite: true },
        },
        {
          resultId: 'web-search',
          title: '/web-search',
          iconKind: 'webSearch',
          activation: { kind: 'completion', completionText: '/web-search ' },
          favorite: { target: { kind: 'builtin', feature: 'webSearch' }, favorite: false },
        },
      ],
    } as unknown as SearchResponse)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('group-wrap-scroll')))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(2))
    const favoritesTitle = mounted.host.querySelector<HTMLElement>('#launcher-favorites-title')!

    await act(async () => core.keyDown('ArrowUp', false))
    expect(core.getSnapshot().selectedIndex).toBe(1)
    scroll.mockClear()
    await act(async () => core.keyDown('ArrowDown', false))

    expect(core.getSnapshot().selectedIndex).toBe(0)
    expect(scroll).toHaveBeenLastCalledWith({ block: 'nearest' })
    expect(scroll.mock.instances[scroll.mock.instances.length - 1]).toBe(favoritesTitle)
    await mounted.unmount()
    core.destroy()
  })

  it('cycles launcher Tab focus through query, operations and settings', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValue({
      requestId: 'launcher-tab-focus',
      items: [
        findLauncherItem('tab'),
        {
          resultId: 'demo-return',
          title: '/demo-return',
          subtitle: '返回文本',
          activation: {
            kind: 'pluginCompletion', completionText: '/demo-return ',
            pluginId: 'com.uipilot.demo-return', favorite: true,
          },
        },
        { resultId: 'tab-app', title: 'Tab App', activation: executeActivation },
      ],
    } as unknown as SearchResponse)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('launcher-tab-focus')))
      await act(async () => core.text({
        kind: 'ordinaryInput',
        control: core.getSnapshot().queryControl,
        value: 'tab',
        inputType: 'insertText',
      }))
      await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(3))

      const query = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
      const settings = mounted.host.querySelector<HTMLButtonElement>('.launcher-settings-button')!
      query.focus()
      expect(document.activeElement).toBe(query)

      const tabToSettings = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
      await act(async () => query.dispatchEvent(tabToSettings))
      expect(tabToSettings.defaultPrevented).toBe(true)
      const operations = mounted.host.querySelector<HTMLButtonElement>('.launcher-operations-trigger')!
      expect(document.activeElement).toBe(operations)
      await act(async () => operations.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })))
      expect(document.activeElement).toBe(settings)

      const tabToQuery = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
      await act(async () => settings.dispatchEvent(tabToQuery))
      expect(tabToQuery.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(query)

      const shiftTabToSettings = new KeyboardEvent('keydown', {
        key: 'Tab', shiftKey: true, bubbles: true, cancelable: true,
      })
      await act(async () => query.dispatchEvent(shiftTabToSettings))
      expect(shiftTabToSettings.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(settings)

      const options = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
      expect(options.every((option) => option.tabIndex === -1)).toBe(true)
    } finally {
      await mounted.unmount()
      core.destroy()
    }
  })

  it('renders and owns the public-plugin favorite context menu without activating the row', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const mutation = deferred<void>()
    const secondMutation = deferred<void>()
    let favorite = false
    vi.mocked(fake.client.setPublicPluginFavorite)
      .mockReturnValueOnce(mutation.promise)
      .mockReturnValueOnce(secondMutation.promise)
    vi.mocked(fake.client.searchApps).mockImplementation(async () => {
      const demoWin = {
        resultId: 'demo-win', title: '/demo-win', subtitle: '打开演示子窗口',
        activation: {
          kind: 'pluginCompletion', completionText: '/demo-win ',
          pluginId: 'com.uipilot.demo-win', favorite,
        },
        favorite: {
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win', commandId: 'default' },
          favorite,
        },
      }
      const demoReturn = {
        resultId: 'demo-return', title: '/demo-return', subtitle: '返回文本',
        activation: {
          kind: 'pluginCompletion', completionText: '/demo-return ',
          pluginId: 'com.uipilot.demo-return', favorite: true,
        },
        favorite: {
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-return', commandId: 'default' },
          favorite: true,
        },
      }
      return {
        requestId: 'favorite-menu',
        items: favorite
          ? [demoWin, demoReturn, findLauncherItem(''), { resultId: 'app', title: 'Demo App', activation: executeActivation }]
          : [demoReturn, findLauncherItem(''), demoWin, { resultId: 'app', title: 'Demo App', activation: executeActivation }],
      } as unknown as SearchResponse
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('favorite-menu')))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(4))
    const options = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]

    expect(options[0]?.querySelector('.result-favorite-star')).not.toBeNull()
    expect(options[1]?.querySelector('.result-favorite-star')).toBeNull()
    expect(options[2]?.querySelector('.result-favorite-star')).toBeNull()
    await act(async () => options[3]?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    expect(document.querySelector('[role="menuitem"]')).toBeNull()

    await act(async () => options[2]?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    let menuItem: HTMLElement | null = null
    await vi.waitFor(() => {
      menuItem = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')]
        .find((item) => item.textContent?.trim() === '设为常用') ?? null
      expect(menuItem).not.toBeNull()
    })
    expect(options[2]?.getAttribute('aria-selected')).toBe('true')
    await act(async () => menuItem!.click())
    expect(fake.client.setPublicPluginFavorite).toHaveBeenCalledWith({ commandId: 'default',
      pluginId: 'com.uipilot.demo-win', favorite: true,
    })
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    expect(core.getSnapshot().query).toBe('')

    expect(fake.client.setPublicPluginFavorite).toHaveBeenCalledOnce()

    favorite = true
    await act(async () => {
      mutation.resolve()
      await mutation.promise
    })
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('.result-favorite-star')).toHaveLength(2))
    const refreshed = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    await vi.waitFor(() => expect(document.activeElement).toBe(refreshed[0]))
    expect(refreshed[0]?.getAttribute('aria-selected')).toBe('true')
    expect(stylesSource).toMatch(/\.result-row:focus\s*\{[^}]*outline:\s*none;/s)
    const arrowDown = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
    await act(async () => refreshed[0]?.dispatchEvent(arrowDown))
    expect(arrowDown.defaultPrevented).toBe(true)
    await vi.waitFor(() => expect(refreshed[1]?.getAttribute('aria-selected')).toBe('true'))
    await act(async () => refreshed[0]?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    let cancelItem: HTMLElement | null = null
    await vi.waitFor(() => {
      cancelItem = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')]
        .find((item) => item.textContent?.trim() === '取消常用') ?? null
      expect(cancelItem).not.toBeNull()
    })
    await act(async () => cancelItem!.click())
    expect(fake.client.setPublicPluginFavorite).toHaveBeenCalledTimes(2)
    await act(async () => refreshed[0]?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    await vi.waitFor(() => {
      const pendingItem = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')]
        .find((item) => item.textContent?.trim() === '取消常用')
      expect(pendingItem?.getAttribute('aria-disabled')).toBe('true')
    })
    favorite = false
    await act(async () => {
      secondMutation.resolve()
      await secondMutation.promise
    })
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('.result-favorite-star')).toHaveLength(1))

    await mounted.unmount()
    core.destroy()
  })

  it.each(['quicklinks', 'screenshot'] as const)('renders and owns the %s builtin favorite context menu', async (feature) => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const mutation = deferred<void>()
    vi.mocked(fake.client.setBuiltinFeatureFavorite).mockReturnValueOnce(mutation.promise)
    vi.mocked(fake.client.searchApps).mockImplementation(async () => ({
      requestId: 'quicklinks-favorite-menu',
      items: [{
        resultId: 'quicklinks',
        title: '/quicklinks',
        subtitle: '管理快速链接',
        activation: { kind: feature === 'screenshot' ? 'openScreenshot' : 'openQuicklinks' },
        iconKind: feature,
        favorite: {
          target: { kind: 'builtin', feature },
          favorite: false,
        },
        hasDefaultAction: false,
      }],
      replaceLocalResults: true,
    } as unknown as SearchResponse))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('quicklinks-favorite-menu')))
      await act(async () => core.text({
        kind: 'ordinaryInput', control: core.getSnapshot().queryControl,
        value: '/quicklinks', inputType: 'insertText',
      }))
      await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))
      const option = mounted.host.querySelector<HTMLElement>('[role="option"]')!

      await act(async () => option.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
      let menuItem: HTMLElement | null = null
      await vi.waitFor(() => {
        menuItem = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')]
          .find((item) => item.textContent?.trim() === '设为常用') ?? null
        expect(menuItem).not.toBeNull()
      })
      await act(async () => menuItem!.click())
      expect(fake.client.setBuiltinFeatureFavorite).toHaveBeenCalledWith({
        feature,
        favorite: true,
      })
      expect(fake.client.executeResult).not.toHaveBeenCalled()
      expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    } finally {
      mutation.resolve()
      await mounted.unmount()
      core.destroy()
    }
  })

  it('completes a plugin command and keeps an unselectable command usage hint until submit', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => {
      if (request.query === '/d') {
        return {
          requestId: 'plugin-completions',
          items: [
            {
              resultId: 'demo-return-completion',
              title: '/demo-return',
              subtitle: '返回示例文本到主界面',
              activation: { kind: 'completion', completionText: '/demo-return ' },
              hasDefaultAction: false,
            },
            {
              resultId: 'demo-win-completion',
              title: '/demo-win',
              subtitle: '打开演示子窗口',
              activation: { kind: 'completion', completionText: '/demo-win ' },
              hasDefaultAction: false,
            },
          ],
        } as unknown as SearchResponse
      }
      if (request.query.startsWith('/demo-win ') && !request.submit) {
        return {
          requestId: `demo-win-hint-${request.querySequence}`,
          items: [],
          commandHint: '请输入信息回车',
        } as unknown as SearchResponse
      }
      return null
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('plugin-command-completion')))
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!

    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/d',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(2))
    const initialOptions = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    expect(initialOptions.map((option) => option.textContent)).toEqual([
      '/demo-return返回示例文本到主界面',
      '/demo-win打开演示子窗口',
    ])
    expect(initialOptions[0]?.getAttribute('aria-selected')).toBe('true')

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(input.value).toBe('/demo-win '))
    expect(document.activeElement).toBe(input)
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()

    await vi.waitFor(() => expect(mounted.host.querySelector('.command-hint')?.textContent).toBe('请输入信息回车'))
    const hint = mounted.host.querySelector<HTMLElement>('.command-hint')!
    expect(hint.getAttribute('role')).toBeNull()
    expect(hint.tabIndex).toBe(-1)
    expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(0)
    expect(input.getAttribute('aria-activedescendant')).toBeNull()

    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/demo-win 123',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelector('.command-hint')?.textContent).toBe('请输入信息回车'))
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({
      query: '/demo-win 123',
      submit: true,
    })))
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    await mounted.unmount()
  })

  it('shows an unselectable usage hint while entering an explicit find query', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('find-command-hint')))
    const control = core.getSnapshot().queryControl
    vi.mocked(fake.client.searchApps).mockClear()

    await act(async () => core.text({
      kind: 'ordinaryInput', control, value: '/find ', inputType: 'insertText',
    }))
    expect(mounted.host.querySelector('.command-hint')?.textContent).toBe('请输入信息回车')
    expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(0)
    expect(fake.client.searchApps).not.toHaveBeenCalled()

    await act(async () => core.text({
      kind: 'ordinaryInput', control, value: '/find 微信', inputType: 'insertText',
    }))
    expect(mounted.host.querySelector('.command-hint')?.textContent).toBe('请输入信息回车')
    expect(fake.client.searchApps).not.toHaveBeenCalled()
    await mounted.unmount()
  })

  it('wraps a submitted main-result plugin command in a tag and preserves its argument', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => {
      if (request.query !== '/demo-return' && !request.query.startsWith('/demo-return ')) return null
      const argument = request.query === '/demo-return' ? '' : request.query.slice('/demo-return '.length)
      if (!request.submit) {
        return {
          requestId: `demo-return-hint-${argument}`,
          items: [],
          commandHint: '请输入信息回车',
        } as unknown as SearchResponse
      }
      if (!argument) {
        return {
          requestId: 'demo-return-submit-hint',
          items: [],
          commandHint: '请输入信息回车',
          mainResultCommand: {
            commandId: 'default',
            pluginId: 'com.uipilot.demo-return',
            commandLabel: 'demo-return',
            argument,
          },
        } as unknown as SearchResponse
      }
      return {
        requestId: `demo-return-results-${argument}`,
        items: [{
          resultId: 'demo-return-copy',
          title: argument,
          subtitle: '英译中 · 按 Enter 复制',
          activation: executeActivation,
          hasDefaultAction: true,
        }],
        mainResultCommand: {
          commandId: 'default',
          pluginId: 'com.uipilot.demo-return',
          commandLabel: 'demo-return',
          argument,
        },
      } as unknown as SearchResponse
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('main-result-command-tag')))
    const query = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!

    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/demo-return str',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({
      query: '/demo-return str',
      submit: false,
    })))
    await act(async () => query.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))

    await vi.waitFor(() => {
      const tag = mounted.host.querySelector('.main-result-command-tag')
      expect(tag).not.toBeNull()
      expect(tag?.textContent).toContain('/demo-return')
    })
    const argument = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-return argument"]')!
    expect(argument.value).toBe('str')
    expect(document.activeElement).toBe(argument)
    expect(mounted.host.querySelector('[role="option"]')?.textContent).toContain('str')
    expect(mounted.host.querySelector('.status-region')?.textContent)
      .toBe('1 个结果')
    expect(mounted.host.querySelector('[aria-label="操作 Ctrl+K"]')).not.toBeNull()
    expect(mounted.host.querySelector('[role="option"]')?.textContent).toContain('英译中 · 按 Enter 复制')
    expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({
      query: '/demo-return str',
      submit: true,
    }))

    const suffixControl = core.getSnapshot().mainResultCommand!.suffixControl
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: suffixControl,
      value: 'next',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => {
      expect(core.getSnapshot().searchPending).toBe(false)
      expect(core.getSnapshot().results).toHaveLength(0)
      expect(argument.value).toBe('next')
    })
    await act(async () => argument.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({
      query: '/demo-return next',
      submit: true,
    })))
    await vi.waitFor(() => expect(mounted.host.querySelector('[role="option"]')?.textContent).toContain('next'))

    const exit = mounted.host.querySelector<HTMLButtonElement>('[aria-label="退出 demo-return 命令"]')!
    await act(async () => exit.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand).toBeUndefined())
    const plainQuery = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(plainQuery.value).toBe('')

    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '/demo-return',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    await act(async () => plainQuery.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand?.suffix).toBe(''))
    const emptyArgument = mounted.host.querySelector<HTMLInputElement>('[aria-label="demo-return argument"]')!
    emptyArgument.setSelectionRange(0, 0)
    await act(async () => emptyArgument.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Backspace',
      bubbles: true,
    })))
    await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand).toBeUndefined())
    expect(mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')?.value).toBe('')

    await mounted.unmount()
    core.destroy()
  })

  it('opens Settings from the query suffix and returns through Escape', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: 'view-navigation-search',
      items: request.query === 'calc'
        ? [findLauncherItem('calc'), { resultId: 'calculator', title: 'Calculator', activation: executeActivation }]
        : [findLauncherItem('')],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('view-navigation')))
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'calc',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))

    const settingsButton = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="打开设置"]')
    expect(settingsButton).not.toBeNull()
    expect(settingsButton?.closest('.ant-input-suffix')).not.toBeNull()
    expect(settingsButton?.querySelector('.lucide-settings')).not.toBeNull()
    await act(async () => settingsButton?.click())
    await vi.waitFor(() => expect(core.getSnapshot().view).toBe('settings'))

    const backButton = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="返回主界面"]')
    const titleGroup = mounted.host.querySelector('.settings-title-group')
    expect(backButton).not.toBeNull()
    expect(backButton?.querySelector('.lucide-arrow-left')).not.toBeNull()
    expect(titleGroup?.firstElementChild).toBe(backButton)
    expect(titleGroup?.querySelector('h1')?.textContent).toBe('设置')
    expect(mounted.host.querySelector('button[aria-label="关闭"]')).toBeNull()

    const settingsView = mounted.host.querySelector<HTMLElement>('.settings-view')
    expect(settingsView).not.toBeNull()
    await act(async () => {
      settingsView?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    })
    await vi.waitFor(() => expect(core.getSnapshot().view).toBe('launcher'))
    const query = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')
    await vi.waitFor(() => expect(document.activeElement).toBe(query))
    expect(query?.value).toBe('calc')
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    expect(stylesSource).toMatch(/\.launcher-settings-button\.ant-btn\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;/s)
    expect(stylesSource).toMatch(/\.settings-title-group\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s)
    expect(stylesSource).toMatch(
      /\.settings-tabs \.ant-tabs-tab:not\(\.ant-tabs-tab-active\) \.settings-message-tab-badge\s*\{[^}]*color:\s*var\(--uipilot-ui-muted-foreground\);/s,
    )
    await mounted.unmount()
  })

  it('uses the exact AntD light/dark algorithms and removes the media listener', async () => {
    configCapture.values.length = 0
    const scheme = installMatchMedia(false)
    const { core } = await startedCore()
    const mounted = await mountLauncherView(core)
    expect(scheme.matchMedia).toHaveBeenCalledWith('(prefers-color-scheme: dark)')
    let config = configCapture.values[configCapture.values.length - 1] as { algorithm?: unknown; token?: { motion?: boolean } }
    expect(config.algorithm).toBe(theme.defaultAlgorithm)
    expect(config.token?.motion).toBe(false)
    await act(async () => scheme.emit(true))
    config = configCapture.values[configCapture.values.length - 1] as { algorithm?: unknown; token?: { motion?: boolean } }
    expect(config.algorithm).toBe(theme.darkAlgorithm)
    await mounted.unmount()
    expect(scheme.remove).toHaveBeenCalledTimes(1)
    expect(scheme.remove.mock.calls[0]).toEqual(['change', scheme.add.mock.calls[0]![1]])
  })

  it('selects the dark algorithm on an initially dark host', async () => {
    configCapture.values.length = 0
    installMatchMedia(true)
    const { core } = await startedCore()
    const mounted = await mountLauncherView(core)
    const config = configCapture.values[configCapture.values.length - 1] as { algorithm?: unknown; token?: { motion?: boolean } }
    expect(config.algorithm).toBe(theme.darkAlgorithm)
    expect(config.token?.motion).toBe(false)
    await mounted.unmount()
  })

  it('follows system only for system theme and projects one effective scheme', async () => {
    configCapture.values.length = 0
    const scheme = installMatchMedia(false)
    const { core, emit, client } = await startedCore(settingsFixture)
    const mounted = await mountLauncherView(core)
    expect(document.documentElement.dataset.colorScheme).toBe('light')
    expect(mounted.host.querySelector('.launcher-surface')?.getAttribute('data-color-scheme')).toBe('light')

    await act(async () => scheme.emit(true))
    expect(document.documentElement.dataset.colorScheme).toBe('dark')
    let config = configCapture.values[configCapture.values.length - 1] as { algorithm?: unknown }
    expect(config.algorithm).toBe(theme.darkAlgorithm)

    await act(async () => emit(shown('settings-theme-force', 'settings')))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.readOnly).toBe(false))
    const save = deferred<void>()
    vi.mocked(client.setThemePreference).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: 'light' })
    await act(async () => core.setThemePreference('light'))
    await act(async () => scheme.emit(true))
    expect(document.documentElement.dataset.colorScheme).toBe('light')
    expect(mounted.host.querySelector('.launcher-surface')?.getAttribute('data-color-scheme')).toBe('light')
    config = configCapture.values[configCapture.values.length - 1] as { algorithm?: unknown }
    expect(config.algorithm).toBe(theme.defaultAlgorithm)

    await act(async () => save.resolve())
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
    await mounted.unmount()
    expect(document.documentElement.hasAttribute('data-color-scheme')).toBe(false)
  })

  it('renders ordered theme options and selects Dark immediately', async () => {
    installMatchMedia(false)
    const { core, emit, client } = await startedCore(settingsFixture)
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('settings-theme-select', 'settings')))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.readOnly).toBe(false))
    const save = deferred<void>()
    vi.mocked(client.setThemePreference).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: 'dark' })

    const combobox = mounted.host.querySelector<HTMLElement>('[role="combobox"][aria-label="风格"]')
    expect(combobox).toBeInstanceOf(HTMLElement)
    await act(async () => {
      combobox!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    })
    const options = [...document.body.querySelectorAll<HTMLElement>('.ant-select-item-option')]
    expect(options.map((option) => option.textContent)).toEqual(['跟随系统', 'Dark', 'Light'])
    await act(async () => options[1]!.dispatchEvent(new MouseEvent('click', { bubbles: true })))

    expect(client.setThemePreference).toHaveBeenCalledWith({ preference: { theme: 'dark' } })
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBe('theme'))
    expect(
      mounted.host
        .querySelector('[role="combobox"][aria-label="风格"]')
        ?.closest('.ant-select')
        ?.classList.contains('ant-select-disabled'),
    ).toBe(true)

    await act(async () => save.resolve())
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
    await mounted.unmount()
  })

  it('renders ordered search engine options and locks the select while saving', async () => {
    installMatchMedia(false)
    const { core, emit, client } = await startedCore(settingsFixture)
    const mounted = await mountLauncherView(core)
    await act(async () => emit(shown('settings-engine-select', 'settings')))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.readOnly).toBe(false))
    const save = deferred<void>()
    vi.mocked(client.setWebSearchEngine).mockReturnValueOnce(save.promise)
    vi.mocked(client.loadSettings).mockResolvedValue({ ...settingsFixture, webSearchEngine: 'google' })

    const combobox = mounted.host.querySelector<HTMLElement>('[role="combobox"][aria-label="搜索引擎"]')
    expect(combobox).toBeInstanceOf(HTMLElement)
    await act(async () => {
      combobox!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    })
    const options = [...document.body.querySelectorAll<HTMLElement>('.ant-select-item-option')]
    expect(options.map((option) => option.textContent)).toEqual(['Bing', '百度', 'Google'])
    await act(async () => options[2]!.dispatchEvent(new MouseEvent('click', { bubbles: true })))

    expect(client.setWebSearchEngine).toHaveBeenCalledWith({ preference: { engine: 'google' } })
    expect(core.getSnapshot().view).toBe('settings')
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBe('webSearchEngine'))
    expect(
      mounted.host
        .querySelector('[role="combobox"][aria-label="搜索引擎"]')
        ?.closest('.ant-select')
        ?.classList.contains('ant-select-disabled'),
    ).toBe(true)

    await act(async () => save.resolve())
    await vi.waitFor(() => expect(core.getSnapshot().settings?.operation).toBeUndefined())
    expect(core.getSnapshot().view).toBe('settings')
    await mounted.unmount()
  })
  it('uses native app regions without invoking Tauri mouse capture', () => {
    expect(launcherViewSource).not.toContain('data-tauri-drag-region')
    expect(stylesSource).toMatch(
      /\.launcher-surface,[\s\S]*\.status-region\s*\{[^}]*app-region:\s*drag;/,
    )
    expect(stylesSource).toMatch(
      /button,[\s\S]*\.settings-tabs,[\s\S]*\.settings-tab-panel\s*\{[^}]*app-region:\s*no-drag;/,
    )
    const dragRule = stylesSource.match(/\.launcher-surface,[\s\S]*?app-region:\s*drag;/)?.[0]
    expect(dragRule).not.toContain('.settings-loading')
    expect(stylesSource).toMatch(/\.result-list:empty\s*\{[^}]*app-region:\s*drag;/)
  })

  it('wraps the active section and status region in separate divs', async () => {
    installMatchMedia(false)
    const { core } = await startedCore()
    const mounted = await mountLauncherView(core)
    const surface = mounted.host.querySelector<HTMLElement>('.launcher-surface')!
    const regions = Array.from(surface.children) as HTMLElement[]

    expect(regions.map((region) => region.tagName)).toEqual(['DIV', 'DIV'])
    expect(regions.map((region) => region.className)).toEqual([
      'launcher-region launcher-section-region',
      'launcher-region launcher-status-region',
    ])
    expect(regions[0]?.firstElementChild?.tagName).toBe('SECTION')
    expect(regions[1]?.firstElementChild?.classList.contains('status-region')).toBe(true)
    expect(regions[1]?.lastElementChild?.lastElementChild?.classList.contains('launcher-operations-trigger')).toBe(true)
    expect(regions[1]?.querySelector('.status-region')).not.toBeNull()
    const queryRegion = regions[0]?.querySelector<HTMLElement>('.launcher-query-region')
    expect(queryRegion?.tagName).toBe('DIV')
    expect(queryRegion?.children[0]?.matches('label.visually-hidden')).toBe(true)
    expect(queryRegion?.children[1]?.tagName).toBe('SPAN')
    const resultsRegion = regions[0]?.querySelector<HTMLElement>('.launcher-results-region')
    expect(resultsRegion?.tagName).toBe('DIV')
    expect(resultsRegion?.firstElementChild?.matches('.ant-spin.ant-spin-sm')).toBe(true)
    await mounted.unmount()
  })

  it('keeps launcher chrome separated and gives scrolling only to results', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const style = document.createElement('style')
    style.textContent = stylesSource
    document.head.append(style)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockImplementation(async (request) => ({
      requestId: 'layout',
      items: request.query === 'layout'
        ? [{ resultId: 'layout-icon', title: 'Layout', icon: 'data:image/png;base64,iVBORw==', activation: executeActivation }]
        : [],
    }))
    const mounted = await mountLauncherView(core)
    mounted.host.id = 'app'
    try {
      await act(async () => emit(shown('layout')))
      await act(async () =>
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'layout', inputType: 'insertText' }),
      )
      await vi.waitFor(() => expect(mounted.host.querySelector('.result-icon-image')).toBeInstanceOf(HTMLImageElement))
      const app = mounted.host.querySelector<HTMLElement>(':scope > .ant-app')!
      const surface = app.querySelector<HTMLElement>('.launcher-surface')!
      const launcher = surface.querySelector<HTMLElement>('.launcher-view')!
      const spinRoot = launcher.querySelector<HTMLElement>('.launcher-results-region > .ant-spin')!
      const spinContainer = spinRoot.querySelector<HTMLElement>('.ant-spin-container')!
      const results = spinContainer.querySelector<HTMLElement>('.result-list')!
      const image = results.querySelector<HTMLImageElement>('.result-icon-image')!
      const icon = image.closest<HTMLElement>('.result-icon')!
      const status = surface.querySelector<HTMLElement>('.status-region')!
      const normalized = (value: string) => value.replace(/\s+/g, ' ').trim()
      const isZero = (value: string) => /^0(?:px)?$/.test(value)

      expect(getComputedStyle(app).height).toBe('100%')
      expect(normalized(getComputedStyle(surface).gridTemplateRows)).toBe('minmax(52px, 1fr) minmax(0, auto)')
      expect(normalized(getComputedStyle(launcher).gridTemplateRows)).toBe('60px minmax(0, 1fr)')
      for (const element of [spinRoot, spinContainer, results]) {
        expect(isZero(getComputedStyle(element).minHeight)).toBe(true)
        expect(getComputedStyle(element).height).toBe('100%')
      }
      expect(getComputedStyle(results).overflowY).toBe('auto')
      expect(getComputedStyle(icon).width).toBe('28px')
      expect(getComputedStyle(icon).height).toBe('28px')
      expect(getComputedStyle(icon).alignSelf).toBe('center')
      expect(getComputedStyle(icon).marginTop).toBe('0px')
      expect(getComputedStyle(image).objectFit).toBe('contain')
      expect(getComputedStyle(status).maxHeight).toBe('72px')
      expect(getComputedStyle(status).overflow).toBe('hidden')
      const autoScrollers = [surface, ...surface.querySelectorAll<HTMLElement>('*')].filter(
        (element) => getComputedStyle(element).overflowY === 'auto',
      )
      expect(autoScrollers).toEqual([results])
      expect(stylesSource).toMatch(/\.result-icon \.app-mark::before[\s\S]*border-left:\s*1px solid currentColor;/)
      expect(stylesSource).toMatch(/\.result-icon \.app-mark::after[\s\S]*border-top:\s*1px solid currentColor;/)
      expect(stylesSource).toMatch(
        /@media \(forced-colors: active\)[\s\S]*\.result-icon \.app-mark\s*\{[^}]*forced-color-adjust:\s*none;[^}]*color:\s*ButtonText;/,
      )
    } finally {
      await mounted.unmount()
      style.remove()
    }
  })

  it('lays out settings tabs with a fixed left nav and right scroller', () => {
    expect(stylesSource).toMatch(
      /\.settings-view\s*\{[^}]*height:\s*100%;[^}]*overflow:\s*hidden;/s,
    )
    expect(stylesSource).toMatch(
      /\.settings-tabs\s*\{[^}]*min-width:\s*0;[^}]*min-height:\s*0;[^}]*height:\s*100%;/s,
    )
    expect(stylesSource).toMatch(
      /\.settings-tabs > \.ant-tabs\s*\{[^}]*height:\s*100%;[^}]*overflow:\s*hidden;/s,
    )
    expect(stylesSource).toMatch(
      /\.settings-tabs \.ant-tabs-nav\s*\{[^}]*flex:\s*0 0 144px;[^}]*width:\s*144px;/s,
    )
    expect(stylesSource).toMatch(
      /\.settings-tabs \.ant-tabs-body-holder,\s*\.settings-tabs \.ant-tabs-body,\s*\.settings-tabs \.ant-tabs-content\s*\{[^}]*min-width:\s*0;[^}]*min-height:\s*0;[^}]*height:\s*100%;[^}]*overflow:\s*hidden;/s,
    )
    expect(stylesSource).toMatch(
      /\.settings-tab-panel\s*\{[^}]*height:\s*100%;[^}]*padding:\s*0;[^}]*overflow:\s*hidden;/s,
    )
    expect(stylesSource).toMatch(/\.settings-tab-panel\s*\{[^}]*padding:\s*16px 24px 16px 4px;/s)
    expect(stylesSource).toMatch(/\.settings-scroll-content\s*\{[^}]*min-height:\s*100%;/s)
  })

  it('uses the third-party overlay scrollbar for settings, detail, and quicklinks panels', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPlugins).mockResolvedValueOnce(pluginInventory())
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-overlay-scrollbar', 'settings')))
    expect(mounted.host.querySelector('.settings-general-panel .settings-scroll-content')).toBeTruthy()
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelector('.settings-plugin-panel .settings-scroll-content')).toBeTruthy())

    expect(launcherViewSource).toContain("from 'overlayscrollbars-react'")
    expect(launcherViewSource.match(/<OverlayScrollbarsComponent/g)).toHaveLength(7)
    expect(launcherViewSource).toContain('className="quicklinks-list-scroll"')
    expect(launcherViewSource).toContain('className="quicklinks-editor-scroll"')
    expect(launcherViewSource).not.toContain('./overlay-scroll-area')
    expect(stylesSource).toMatch(/\.os-theme-uipilot\s*\{[^}]*--os-size:\s*8px;[^}]*--os-handle-bg:\s*var\(--result-scrollbar-thumb\);/s)
    expect(stylesSource).not.toContain('.overlay-scroll-')
    await mounted.unmount()
  })

  it('keeps slim native results and third-party settings scrollbars visible without hover', () => {
    expect(stylesSource).toMatch(/\.result-list,\s*\.settings-tab-panel\s*\{[^}]*--result-scrollbar-thumb:\s*var\(--uipilot-ui-scrollbar\);/s)
    expect(stylesSource).toMatch(/\.result-list::-webkit-scrollbar\s*\{[^}]*width:\s*6px;/s)
    expect(stylesSource).toMatch(/\.result-list::-webkit-scrollbar-track\s*\{[^}]*background:\s*transparent;/s)
    expect(stylesSource).toMatch(
      /\.result-list::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*var\(--result-scrollbar-thumb\);[^}]*border-radius:\s*3px;/s,
    )
    expect(stylesSource).not.toMatch(/\.result-list:hover::-webkit-scrollbar-thumb/)
    expect(stylesSource).not.toMatch(/\.launcher-surface\[data-color-scheme="dark"\][\s\S]*--result-scrollbar-thumb:/s)
    expect(stylesSource).not.toContain('@media (prefers-color-scheme: dark)')
    expect(stylesSource).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.result-list::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*ButtonText;/s,
    )
    expect(stylesSource).toMatch(/@media \(forced-colors: active\)[\s\S]*\.os-theme-uipilot\s*\{[^}]*--os-handle-bg:\s*ButtonText;/s)
  })

  it('renders the exact currency icon and existing result icons with stable decorative dimensions', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: 'built-in-icons',
      items: request.query === 'icons' ? [
        findLauncherItem('icons'),
        { resultId: 'quicklinks', title: '/quicklinks', iconKind: 'quicklinks', activation: { kind: 'openQuicklinks' } },
        { resultId: 'calculator', title: '2', iconKind: 'calculator', activation: executeActivation },
        { resultId: 'currency', title: '713.245 CNY', iconKind: 'currency', activation: executeActivation },
        { resultId: 'date', title: '2026-09-10', iconKind: 'date', activation: executeActivation },
        { resultId: 'time-zone', title: '10:35', iconKind: 'timeZone', activation: executeActivation },
        { resultId: 'unit', title: '39.3701 in', iconKind: 'unit', activation: executeActivation },
        { resultId: 'web', title: 'Bing 搜索', iconKind: 'webSearch', activation: executeActivation },
        {
          resultId: 'plugin',
          title: '/demo-win',
          activation: executeActivation,
          pluginIconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
        },
        { resultId: 'app', title: 'App', icon: 'data:image/png;base64,iVBORw==', activation: executeActivation },
        { resultId: 'unknown', title: 'Unknown icon', iconKind: 'unknown', activation: executeActivation },
        { resultId: 'fallback', title: 'Fallback', activation: executeActivation },
      ] : [],
    } as unknown as SearchResponse))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('built-in-icons')))
    await act(async () =>
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icons', inputType: 'insertText' }),
    )
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(12))

    const rows = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    const rowWithTitle = (title: string) =>
      rows.find((row) => row.querySelector('.result-title')?.textContent === title)
    const find = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="find"]')
    const quicklinks = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="quicklinks"]')
    const calculator = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="calculator"]')
    const currency = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="currency"]')
    const date = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="date"]')
    const timeZone = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="timeZone"]')
    const unit = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="unit"]')
    const web = mounted.host.querySelector<HTMLElement>('[data-result-icon-kind="webSearch"]')
    expect(find?.querySelector('.lucide-folder-search')).toBeTruthy()
    expect(quicklinks?.querySelector('.lucide-link-2')).toBeTruthy()
    expect(calculator?.querySelector('.lucide-calculator')).toBeTruthy()
    expect(currency?.querySelector('.lucide-arrow-right-left')?.getAttribute('aria-hidden')).toBe('true')
    expect(date?.querySelector('.lucide-calendar')?.getAttribute('aria-hidden')).toBe('true')
    expect(timeZone?.querySelector('.lucide-clock')?.getAttribute('aria-hidden')).toBe('true')
    expect(unit?.querySelector('.lucide-ruler')?.getAttribute('aria-hidden')).toBe('true')
    expect(web?.querySelector('.lucide-panels-top-left')).toBeTruthy()
    expect(web?.querySelector('.lucide-search')).toBeTruthy()
    for (const icon of [find, quicklinks, calculator, currency, date, timeZone, unit, web]) {
      expect(icon?.closest('.result-icon')?.getAttribute('aria-hidden')).toBe('true')
    }
    expect(rowWithTitle('/demo-win')?.querySelector('.plugin-icon-image')).toBeInstanceOf(HTMLImageElement)
    expect(rowWithTitle('App')?.querySelector('.result-icon-image')).toBeInstanceOf(HTMLImageElement)
    expect(rowWithTitle('Unknown icon')?.querySelector('.result-icon .app-mark:not([hidden])')).toBeTruthy()
    expect(rowWithTitle('Fallback')?.querySelector('.result-icon .app-mark:not([hidden])')).toBeTruthy()
    expect(stylesSource).toMatch(/\.built-in-result-icon\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;/s)
    expect(stylesSource).toMatch(
      /\.launcher-view \.built-in-result-icon-currency\s*\{[^}]*color:\s*var\(--uipilot-ui-icon-green\);[^}]*background:\s*var\(--uipilot-ui-icon-green-soft\);/s,
    )
    expect(stylesSource).toMatch(
      /\.launcher-view \.built-in-result-icon-date\s*\{[^}]*color:\s*var\(--uipilot-ui-icon-blue\);[^}]*background:\s*var\(--uipilot-ui-icon-blue-soft\);/s,
    )
    expect(stylesSource).toMatch(
      /\.launcher-view \.built-in-result-icon-time-zone\s*\{[^}]*color:\s*var\(--uipilot-ui-icon-blue\);[^}]*background:\s*var\(--uipilot-ui-icon-blue-soft\);/s,
    )
    expect(stylesSource).toMatch(
      /\.launcher-view \.built-in-result-icon-unit\s*\{[^}]*color:\s*var\(--uipilot-ui-icon-red\);[^}]*background:\s*var\(--uipilot-ui-icon-red-soft\);/s,
    )
    expect(uiThemeSource.match(/--uipilot-ui-icon-red:/g)).toHaveLength(2)
    expect(uiThemeSource.match(/--uipilot-ui-icon-red-soft:/g)).toHaveLength(2)
    expect(stylesSource).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.built-in-result-icon,[\s\S]*forced-color-adjust:\s*none;/s,
    )
    expect(stylesSource).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.result-row\.is-selected \.built-in-result-icon,[\s\S]*color:\s*HighlightText !important;/s,
    )
    expect(launcherViewSource).toMatch(/import \{[^}]*\bArrowRightLeft\b[^}]*\bClock\b[^}]*\bRuler\b[^}]*\} from 'lucide-react'/s)
    expect(stylesSource).toMatch(/\.built-in-result-icon-badge\s*\{[^}]*position:\s*absolute;/s)
    await mounted.unmount()
  })

  it('keeps a cold currency conversion pending then copies a long stale result without favorite UI', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const response = deferred<SearchResponse | null>()
    const staleSubtitle = `${'100 USD · '.repeat(30)}1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03 · 缓存 · 获取于 2026-09-03T12:34:56Z`
    vi.mocked(fake.client.searchApps).mockImplementation((request) =>
      request.query === '100 USD to CNY'
        ? response.promise
        : Promise.resolve({ requestId: 'currency-empty', items: [] }),
    )
    vi.mocked(fake.client.executeResult).mockResolvedValue({ status: 'textCopied' })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('currency-copy')))
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '100 USD to CNY',
      inputType: 'insertText',
    }))

    await vi.waitFor(() => expect(core.getSnapshot()).toMatchObject({
      searchPending: true,
      results: [],
      selectedIndex: -1,
    }))
    await act(async () => {
      response.resolve({
        requestId: 'currency-copy-request',
        items: [{
          resultId: 'opaque-currency-copy-id',
          title: '713.245 CNY',
          subtitle: staleSubtitle,
          iconKind: 'currency',
          activation: executeActivation,
          hasDefaultAction: true,
        }],
        replaceLocalResults: true,
      } as unknown as SearchResponse)
      await response.promise
    })
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))

    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const option = mounted.host.querySelector<HTMLElement>('[role="option"]')!
    expect(core.getSnapshot()).toMatchObject({ searchPending: false, selectedIndex: 0 })
    expect(option.querySelector('.result-subtitle')?.textContent).toBe(staleSubtitle)
    expect(option.querySelector('.result-favorite-star')).toBeNull()
    expect(core.getSnapshot().results[0]).toMatchObject({ iconKind: 'currency' })
    expect(core.getSnapshot().results[0]).not.toHaveProperty('favorite')
    expect(core.getSnapshot().results[0]).not.toHaveProperty('detail')

    const contextEvent = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    await act(async () => { option.dispatchEvent(contextEvent) })
    expect(contextEvent.defaultPrevented).toBe(false)
    expect(document.querySelector('[role="menuitem"]')).toBeNull()

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(fake.client.executeResult).toHaveBeenLastCalledWith({
      requestId: 'currency-copy-request',
      resultId: 'opaque-currency-copy-id',
    })
    await act(async () => option.click())
    await vi.waitFor(() => expect(fake.client.executeResult).toHaveBeenCalledTimes(2))
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    await mounted.unmount()
    core.destroy()
  })

  it('keeps a currency conversion visible with a Chinese error when clipboard copy fails', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'currency-copy-failure',
      items: [{
        resultId: 'opaque-currency-id',
        title: '713.245 CNY',
        subtitle: '100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03',
        iconKind: 'currency',
        hasDefaultAction: true,
        activation: executeActivation,
      }],
      replaceLocalResults: true,
    } as unknown as SearchResponse)
    vi.mocked(client.executeResult).mockRejectedValueOnce({
      code: 'clipboardWriteFailed',
      message: 'private clipboard detail',
    })
    emit(shown('currency-copy-failure'))
    core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '100 USD to CNY',
      inputType: 'insertText',
    })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))

    expect(client.executeResult).toHaveBeenCalledWith({
      requestId: 'currency-copy-failure',
      resultId: 'opaque-currency-id',
    })
    expect(client.hideLauncher).not.toHaveBeenCalled()
    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      query: '100 USD to CNY',
      status: '无法复制到剪贴板。',
      results: [{ title: '713.245 CNY' }],
    })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private clipboard detail')
  })

  it('activates a successful unit conversion by Enter and click without favorite context UI', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const longSubtitle = '1 gal · 体积换算 · 默认采用美制（US）。若需英制容量（加仑/夸脱/品脱/盎司），请在单位前加 imp，例如 100 imp gal to L。'
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === '1 gal to L'
      ? {
          requestId: 'unit-copy-request',
          items: [{
            resultId: 'opaque-unit-copy-id',
            title: '3.78541 L',
            subtitle: longSubtitle,
            iconKind: 'unit',
            activation: executeActivation,
            hasDefaultAction: true,
          }],
          replaceLocalResults: true,
        } as unknown as SearchResponse
      : { requestId: 'unit-empty', items: [] })
    vi.mocked(fake.client.executeResult).mockResolvedValue({ status: 'textCopied' })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('unit-copy')))
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '1 gal to L',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))

    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const option = mounted.host.querySelector<HTMLElement>('[role="option"]')!
    expect(option.getAttribute('aria-selected')).toBe('true')
    expect(input.getAttribute('aria-activedescendant')).toBe(option.id)
    expect(option.querySelector('.result-subtitle')?.textContent).toBe(longSubtitle)
    expect(option.querySelector('.result-subtitle')?.getAttribute('title')).toBe(longSubtitle)
    expect(stylesSource).toMatch(
      /\.launcher-view \.result-subtitle\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    )
    expect(option.querySelector('.result-favorite-star')).toBeNull()
    expect(core.getSnapshot().results[0]?.favorite).toBeUndefined()
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe(
      '1 个结果',
    )

    const contextEvent = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    await act(async () => { option.dispatchEvent(contextEvent) })
    expect(contextEvent.defaultPrevented).toBe(false)
    expect(document.querySelector('[role="menuitem"]')).toBeNull()

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(fake.client.executeResult).toHaveBeenLastCalledWith({
      requestId: 'unit-copy-request',
      resultId: 'opaque-unit-copy-id',
    })
    await act(async () => option.click())
    await vi.waitFor(() => expect(fake.client.executeResult).toHaveBeenCalledTimes(2))
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    await mounted.unmount()
    core.destroy()
  })

  it('keeps an invalid unit conversion informational and inert', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === '1 kg to L'
      ? {
          requestId: 'unit-information',
          items: [{
            resultId: 'opaque-information-id',
            title: '无法转换单位',
            subtitle: '源单位与目标单位的维度不同',
            iconKind: 'unit',
            activation: { kind: 'informational' },
            hasDefaultAction: false,
          }],
          replaceLocalResults: true,
        } as unknown as SearchResponse
      : { requestId: 'unit-information-empty', items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('unit-information')))
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: '1 kg to L',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))

    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const option = mounted.host.querySelector<HTMLElement>('[role="option"]')!
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await act(async () => option.click())

    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    expect(option.getAttribute('aria-selected')).toBe('true')
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe(
      '1 个结果',
    )
    await mounted.unmount()
    core.destroy()
  })

  it('keeps informational quick evaluation Enter and click inert while announcing the row', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => request.query === 'today + 1 fortnight'
      ? {
          requestId: 'informational-date',
          items: [{
            resultId: '',
            title: '日期表达式无效',
            subtitle: '请使用 today 或 YYYY-MM-DD 加减非负整数日期单位',
            iconKind: 'date',
            activation: { kind: 'informational' },
            hasDefaultAction: false,
          }],
          replaceLocalResults: true,
        }
      : { requestId: 'informational-empty', items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('informational-date')))
    await act(async () => core.text({
      kind: 'ordinaryInput',
      control: core.getSnapshot().queryControl,
      value: 'today + 1 fortnight',
      inputType: 'insertText',
    }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const option = mounted.host.querySelector<HTMLElement>('[role="option"]')!
    expect(option.getAttribute('aria-selected')).toBe('true')
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe(
      '1 个结果',
    )

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await act(async () => option.click())

    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.openFind).not.toHaveBeenCalled()
    expect(fake.client.openPluginPanel).not.toHaveBeenCalled()
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    expect(core.getSnapshot()).toMatchObject({
      view: 'launcher',
      query: 'today + 1 fortnight',
      selectedIndex: 0,
      results: [{ title: '日期表达式无效', hasDefaultAction: false }],
    })
    expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1)
    await mounted.unmount()
  })

  it('shows real icons, falls back on error, and resets the error for a new src', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const firstIcon = 'data:image/png;base64,iVBORw=='
    const siblingIcon = 'data:image/png;base64,QUJDRA=='
    const secondIcon = 'data:image/png;base64,iVBORw0K'
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => {
      if (request.query === 'icon') return {
        requestId: 'first-icons',
        items: [
          findLauncherItem('icon'),
          { resultId: 'with-icon', title: 'With icon', icon: firstIcon, activation: executeActivation },
          { resultId: 'sibling-icon', title: 'Sibling icon', icon: siblingIcon, activation: executeActivation },
          { resultId: 'without-icon', title: 'Without icon', activation: executeActivation },
        ],
      }
      if (request.query === 'new icon') return {
        requestId: 'second-icons',
        items: [findLauncherItem('new icon'), { resultId: 'new-icon', title: 'New icon', icon: secondIcon, activation: executeActivation }],
      }
      return { requestId: 'empty-icons', items: [] }
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('icon-view')))
      await act(async () =>
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icon', inputType: 'insertText' }),
      )
      await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(4))

      const rows = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
      const rowWithTitle = (title: string) =>
        rows.find((row) => row.querySelector('.result-title')?.textContent === title)
      const image = rowWithTitle('With icon')?.querySelector<HTMLImageElement>('.result-icon-image') ?? null
      const fallback = rowWithTitle('With icon')?.querySelector<HTMLElement>('.result-icon .app-mark') ?? null
      const siblingImage = rowWithTitle('Sibling icon')?.querySelector<HTMLImageElement>('.result-icon-image') ?? null
      const siblingFallback = rowWithTitle('Sibling icon')?.querySelector<HTMLElement>('.result-icon .app-mark') ?? null
      const missingImage = rowWithTitle('Without icon')?.querySelector<HTMLImageElement>('.result-icon-image') ?? null
      const missingFallback = rowWithTitle('Without icon')?.querySelector<HTMLElement>('.result-icon .app-mark') ?? null
      expect(image).toBeInstanceOf(HTMLImageElement)
      expect(fallback).toBeInstanceOf(HTMLElement)
      expect(siblingImage).toBeInstanceOf(HTMLImageElement)
      expect(siblingFallback).toBeInstanceOf(HTMLElement)
      expect(image!.alt).toBe('')
      expect(image!.getAttribute('aria-hidden')).toBe('true')
      expect(image!.draggable).toBe(false)
      expect(image!.hidden).toBe(false)
      expect(fallback!.hidden).toBe(true)
      expect(siblingImage!.hidden).toBe(false)
      expect(siblingFallback!.hidden).toBe(true)
      expect(missingImage).toBeNull()
      expect(missingFallback).toBeInstanceOf(HTMLElement)
      expect(missingFallback!.hidden).toBe(false)

      await act(async () => image!.dispatchEvent(new Event('error')))
      expect(image!.hidden).toBe(true)
      expect(fallback!.hidden).toBe(false)
      expect(siblingImage!.hidden).toBe(false)
      expect(siblingFallback!.hidden).toBe(true)

      await act(async () =>
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'new icon', inputType: 'insertText' }),
      )
      await vi.waitFor(() =>
        expect(mounted.host.querySelector<HTMLImageElement>('.result-icon-image')?.src).toContain(secondIcon),
      )
      const nextRow = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
        .find((row) => row.querySelector('.result-title')?.textContent === 'New icon')!
      const nextImage = nextRow.querySelector<HTMLImageElement>('.result-icon-image')!
      const nextFallback = nextRow.querySelector<HTMLElement>('.result-icon .app-mark')!
      expect(nextImage).not.toBe(image)
      expect(nextImage.hidden).toBe(false)
      expect(nextFallback.hidden).toBe(true)
    } finally {
      await mounted.unmount()
    }
  })

  it('renders local combobox/listbox ownership and keeps the active option visible', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: 'private-request',
      items: request.query === 'app' ? [
        { resultId: 'private-one', title: '<b>literal</b>', activation: executeActivation },
        { resultId: 'private-two', title: '非常长的第二个应用名称', subtitle: 'Long subtitle value', activation: executeActivation },
        findLauncherItem('app'),
      ] : [],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    const scroll = vi.fn()
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: scroll })
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('view')))
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(input).toBeTruthy()
    expect(input.disabled).toBe(false)
    expect(input.getAttribute('aria-autocomplete')).toBe('list')
    expect(input.getAttribute('aria-controls')).toBe('launcher-results')
    expect(document.activeElement).toBe(input)

    await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'app', inputType: 'insertText' }))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(3))
    const options = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    expect(mounted.host.querySelector('[role="listbox"]')?.id).toBe('launcher-results')
    expect(input.getAttribute('aria-expanded')).toBe('true')
    expect(options[0]!.getAttribute('aria-selected')).toBe('true')
    expect(options[0]!.textContent).toContain('<b>literal</b>')
    expect(options[0]!.querySelector('b')).toBeNull()
    expect(options[1]!.textContent).toContain('非常长的第二个应用名称')
    expect(options[2]!.textContent).toContain('/find')
    expect(options[2]!.textContent).toContain('搜索文件：app')
    expect(mounted.host.innerHTML).not.toContain('private-request')
    expect(mounted.host.innerHTML).not.toContain('private-one')
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toContain('3 个结果')

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    expect(document.activeElement).toBe(input)
    expect(input.getAttribute('aria-activedescendant')).toBe(options[1]!.id)
    expect(scroll).toHaveBeenCalledWith({ block: 'nearest' })
    await mounted.unmount()
  })


  it('keeps empty startup quiet, announces no results, and gives composing Escape to IME', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: 'empty',
      items: request.query === 'missing' ? [findLauncherItem('missing')] : [],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(input.disabled).toBe(true)
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe('')
    await act(async () => fake.emit(shown('empty-results')))
    expect(input.disabled).toBe(false)
    await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'missing', inputType: 'insertText' }))
    await vi.waitFor(() =>
      expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe('1 个结果'),
    )
    await act(async () =>
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true, isComposing: true })),
    )
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
    expect(fake.client.hideLauncher).toHaveBeenCalledOnce()
    await mounted.unmount()
  })

  it('does not render application aliases in settings', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce({
      hotkey: 'Alt+Space',
      autostart: false,
      filePreviewEnabled: true,
      theme: 'system',
      webSearchEngine: 'bing',
      applications: [{ appId: 'legacy', displayName: 'LiveCaptions', aliases: ['caption'] }],
      screenshotHotkey: null,
      screenshotHotkeyStatus: 'unset',
      screenshotSaveDirectory: null,
      screenshotSaveDirectoryPath: 'C:\\Users\\demo\\OneDrive\\Desktop',
    } as SettingsView)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-no-aliases', 'settings')))

    expect(mounted.host.textContent).not.toContain('LiveCaptions')
    expect(mounted.host.textContent).not.toContain('娣诲姞鍒悕')
    expect(mounted.host.textContent).not.toContain('鍒悕 1')
    await mounted.unmount()
  })

  it('renders ordered snapshot-owned settings tabs, keeps the title unfocusable, and returns on Escape', async () => {
    installMatchMedia(true)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-view', 'settings')))
    const header = mounted.host.querySelector<HTMLElement>('header.settings-header')!
    expect(header.parentElement?.tagName).toBe('DIV')
    expect(header.parentElement?.className).toBe('settings-header-region')
    expect(header.parentElement?.parentElement?.matches('section.settings-view')).toBe(true)
    const footer = mounted.host.querySelector('.settings-status-region')!
    expect(footer.querySelector('[role="status"]')?.textContent).toBe('')
    expect(footer.querySelector('[aria-label="操作 Ctrl+K"]')).not.toBeNull()
    expect(footer.querySelector('.settings-version')).not.toBeNull()
    expect(header.querySelector('.settings-version')).toBeNull()
    const heading = mounted.host.querySelector<HTMLElement>('.settings-header h1')!
    expect(heading.textContent).toBe('设置')
    expect(heading.hasAttribute('tabindex')).toBe(false)
    const tabs = [...mounted.host.querySelectorAll<HTMLElement>('[role="tab"]')]
    expect(tabs).toEqual([
      settingsTab(mounted.host, '通用'),
      settingsTab(mounted.host, '消息'),
      settingsTab(mounted.host, '插件'),
      settingsTab(mounted.host, '权限'),
      settingsTab(mounted.host, '密钥配置'),
    ])
    expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true')
    expect(settingsTab(mounted.host, '消息').getAttribute('aria-selected')).toBe('false')
    expect(settingsTab(mounted.host, '插件').getAttribute('aria-selected')).toBe('false')
    expect(document.activeElement).toBe(settingsTab(mounted.host, '通用'))
    expect(document.activeElement).not.toBe(heading)
    expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()
    expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()
    expect(mounted.host.textContent).toContain('恢复初始化')
    expect(mounted.host.textContent).not.toContain('保存')
    expect(mounted.host.textContent).not.toContain('重新加载设置')
    expect(mounted.host.querySelector('button[aria-label="关闭"]')).toBeNull()
    await activateSettingsTab(mounted.host, '权限')
    expect(core.getSnapshot().settingsTab).toBe('permissions')
    expect(settingsTab(mounted.host, '权限').getAttribute('aria-selected')).toBe('true')
    expect(mounted.host.querySelector('.plugin-permissions-panel')).not.toBeNull()
    await activateSettingsTab(mounted.host, '密钥配置')
    expect(core.getSnapshot().settingsTab).toBe('credentials')
    expect(mounted.host.querySelector('.credential-settings')).not.toBeNull()
    await activateSettingsTab(mounted.host, '通用')
    await act(async () => {
      settingsTab(mounted.host, '通用').dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
      )
    })
    expect(fake.client.hideLauncher).not.toHaveBeenCalled()
    expect(core.getSnapshot().view).toBe('launcher')
    await vi.waitFor(() => expect(document.activeElement).toBe(
      mounted.host.querySelector<HTMLInputElement>('[role="combobox"]'),
    ))
    await mounted.unmount()
  })

  it('pins settings, retries failed native updates, and releases the pin on exit', async () => {
    installMatchMedia(false)
    const { core, emit } = await startedCore(settingsFixture)
    const setPinned = vi.fn<(active: boolean, pinned: boolean) => Promise<void>>().mockResolvedValue(undefined)
    const mounted = await mountLauncherView(core, setPinned)
    await act(async () => emit(shown('settings-pin', 'settings')))
    const button = () => mounted.host.querySelector<HTMLButtonElement>('.settings-header button[aria-pressed]')!
    await vi.waitFor(() => expect(button().disabled).toBe(false))
    setPinned.mockRejectedValueOnce(new Error('native failure'))
    await act(async () => button().click())
    expect(button().getAttribute('aria-pressed')).toBe('false')
    expect(button().getAttribute('aria-label')).toBe('重试置顶设置')
    expect(button().getAttribute('aria-keyshortcuts')).toBe('Control+t')
    const shortcut = (repeat = false) => button().dispatchEvent(new KeyboardEvent('keydown', { key: 't', ctrlKey: true, repeat, bubbles: true, cancelable: true }))
    await act(async () => { shortcut() })
    expect(setPinned).toHaveBeenLastCalledWith(true, true)
    expect(button().getAttribute('aria-pressed')).toBe('true')
    const calls = setPinned.mock.calls.length
    await act(async () => { shortcut(true) })
    expect(setPinned).toHaveBeenCalledTimes(calls)
    await act(async () => { shortcut() })
    expect(setPinned).toHaveBeenLastCalledWith(true, false)
    expect(button().getAttribute('aria-pressed')).toBe('false')
    await act(async () => button().click())
    await act(async () => core.navigate('launcher'))
    expect(setPinned).toHaveBeenLastCalledWith(false, false)
    await mounted.unmount()
    core.destroy()
  })

  it('toggles settings pin while a sidebar tab owns focus', async () => {
    installMatchMedia(false)
    const { core, emit } = await startedCore(settingsFixture)
    const setPinned = vi.fn<(active: boolean, pinned: boolean) => Promise<void>>().mockResolvedValue(undefined)
    const mounted = await mountLauncherView(core, setPinned)
    await act(async () => emit(shown('settings-pin-sidebar', 'settings')))
    const pin = () => mounted.host.querySelector<HTMLButtonElement>('.settings-header button[aria-pressed]')!
    await vi.waitFor(() => expect(pin().disabled).toBe(false))
    const tab = settingsTab(mounted.host, '通用')
    tab.focus()
    const shortcut = () => tab.dispatchEvent(new KeyboardEvent('keydown', {
      key: 't', code: 'KeyT', ctrlKey: true, bubbles: true, cancelable: true,
    }))
    await act(async () => { shortcut() })
    expect(setPinned).toHaveBeenLastCalledWith(true, true)
    expect(pin().getAttribute('aria-pressed')).toBe('true')
    expect(document.activeElement).toBe(tab)
    await act(async () => { shortcut() })
    expect(setPinned).toHaveBeenLastCalledWith(true, false)
    expect(pin().getAttribute('aria-pressed')).toBe('false')
    // Clicking a non-focusable sidebar background can leave the keyboard target on body.
    tab.blur()
    expect(document.activeElement).toBe(document.body)
    await act(async () => document.body.dispatchEvent(new KeyboardEvent('keydown', {
      key: 't', code: 'KeyT', ctrlKey: true, bubbles: true, cancelable: true,
    })))
    expect(setPinned).toHaveBeenLastCalledWith(true, true)
    expect(pin().getAttribute('aria-pressed')).toBe('true')
    await mounted.unmount()
    core.destroy()
  })

  it('isolates settings pin shortcuts while the operations menu is open', async () => {
    installMatchMedia(false)
    const { core, emit } = await startedCore(settingsFixture)
    const setPinned = vi.fn<(active: boolean, pinned: boolean) => Promise<void>>().mockResolvedValue(undefined)
    const mounted = await mountLauncherView(core, setPinned)
    try {
      await act(async () => emit(shown('settings-operation-pin', 'settings')))
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('.launcher-operations-trigger')!.click())
      const calls = setPinned.mock.calls.length
      await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true, cancelable: true })))
      expect(setPinned).toHaveBeenCalledTimes(calls)
      expect(document.querySelector('.uipilot-context-actions')).not.toBeNull()
      await act(async () => window.dispatchEvent(new Event('blur')))
      expect(document.querySelector('.uipilot-context-actions')).toBeNull()
    } finally { await mounted.unmount(); core.destroy() }
  })

  it('leaves Ctrl+T to shortcut recording and ignores it outside settings', async () => {
    installMatchMedia(false)
    const { core, emit } = await startedCore(settingsFixture)
    const setPinned = vi.fn<(active: boolean, pinned: boolean) => Promise<void>>().mockResolvedValue(undefined)
    const mounted = await mountLauncherView(core, setPinned)
    await act(async () => emit(shown('settings-pin-recording', 'settings')))
    const button = mounted.host.querySelector<HTMLButtonElement>('.settings-header button[aria-pressed]')!
    await vi.waitFor(() => expect(button.disabled).toBe(false))
    const record = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制快捷键"]')!
    await act(async () => record.click())
    const calls = setPinned.mock.calls.length
    await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', {
      key: 't', code: 'KeyT', ctrlKey: true, bubbles: true, cancelable: true,
    })))
    expect(setPinned).toHaveBeenCalledTimes(calls)
    expect(button.getAttribute('aria-pressed')).toBe('false')
    await act(async () => core.navigate('launcher'))
    const afterExit = setPinned.mock.calls.length
    await act(async () => mounted.host.querySelector('[role="combobox"]')?.dispatchEvent(new KeyboardEvent('keydown', {
      key: 't', ctrlKey: true, bubbles: true, cancelable: true,
    })))
    expect(setPinned).toHaveBeenCalledTimes(afterExit)
    await mounted.unmount()
    core.destroy()
  })

  it('routes focus right into the active settings panel and left back to its menu tab', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.openMessageCenter).mockResolvedValueOnce(
      messageCenterSnapshot('1', 0, ['focus target']),
    )
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-arrow-focus', 'settings')))

    const generalTab = settingsTab(mounted.host, '通用')
    expect(document.activeElement).toBe(generalTab)
    await act(async () => generalTab.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }),
    ))
    const recorder = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')
    if (!recorder) throw new Error('settings hotkey recorder button missing')
    expect(document.activeElement).toBe(recorder)

    await act(async () => recorder.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }),
    ))
    expect(document.activeElement).toBe(generalTab)

    await act(async () => generalTab.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', code: 'ArrowDown', bubbles: true, cancelable: true }),
    ))
    const messagesTab = settingsTab(mounted.host, '消息')
    await vi.waitFor(() => expect(messagesTab.getAttribute('aria-selected')).toBe('true'))
    expect(document.activeElement).toBe(messagesTab)
    await vi.waitFor(() => expect(fake.client.openMessageCenter).toHaveBeenCalledOnce())
    await act(async () => messagesTab.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }),
    ))
    const clearMessages = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '清空全部')
    if (!clearMessages) throw new Error('clear messages button missing')
    expect(document.activeElement).toBe(clearMessages)

    await act(async () => clearMessages.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }),
    ))
    expect(document.activeElement).toBe(messagesTab)
    await mounted.unmount()
    core.destroy()
  })

  it('opens notification targets on Messages and keeps settings visible across clear failure and success', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.openMessageCenter).mockResolvedValueOnce(messageCenterSnapshot('1', 0, ['first']))
    vi.mocked(fake.client.clearMessages)
      .mockRejectedValueOnce({ code: 'MessageOperationFailed', storeStatus: 'ready' })
      .mockResolvedValueOnce(messageCenterSnapshot('2', 0))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)

    await act(async () => fake.emit(shown('notification-target', 'messages')))
    await vi.waitFor(() => expect(settingsTab(mounted.host, '消息').getAttribute('aria-selected')).toBe('true'))
    await vi.waitFor(() => expect(mounted.host.textContent).toContain('first'))
    expect(fake.client.openMessageCenter).toHaveBeenCalledOnce()

    const clear = () => [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('清空全部'))!
    await act(async () => clear().click())
    await vi.waitFor(() => expect(mounted.host.textContent).toContain('MessageOperationFailed'))
    expect(core.getSnapshot().view).toBe('settings')
    expect(mounted.host.textContent).toContain('first')

    await act(async () => clear().click())
    await vi.waitFor(() => expect(mounted.host.textContent).toContain('暂无消息'))
    expect(core.getSnapshot().view).toBe('settings')
    await mounted.unmount()
    core.destroy()
  })

  it('renders a fixed settings badge for 0, 1, 99, 100, and terminal unavailable', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('message-badge')))
    const badge = () => mounted.host.querySelector<HTMLElement>('.launcher-settings-badge .ant-badge-count')

    expect(badge()).toBeNull()
    for (const [revision, unreadCount, text] of [
      ['1', 1, '1'],
      ['2', 99, '99'],
      ['3', 100, '99+'],
    ] as const) {
      await act(async () => fake.emitMessageState({ status: 'ready', revision, unreadCount }))
      expect(badge()).not.toBeNull()
      if (unreadCount === 100) expect(badge()?.textContent).toBe(text)
      else expect(badge()?.getAttribute('title')).toBe(String(unreadCount))
    }
    await act(async () => fake.emitMessageState({ status: 'unavailable', error: 'MessageStoreUnavailable' }))
    expect(badge()?.textContent).toBe('!')
    await act(async () => fake.emitMessageState({ status: 'ready', revision: '4', unreadCount: 1 }))
    expect(badge()?.textContent).toBe('!')
    expect(mounted.host.querySelector('.launcher-settings-badge')).not.toBeNull()
    expect(stylesSource).toMatch(/\.launcher-settings-control\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;/s)
    expect(stylesSource).toMatch(
      /\.launcher-settings-badge \.ant-badge-count,[\s\S]*?\.settings-message-tab-badge \.ant-badge-count\s*\{[^}]*color:\s*#fff;[^}]*background:\s*var\(--uipilot-ui-destructive\);/,
    )
    expect(stylesSource).not.toMatch(
      /\.launcher-settings-badge \.ant-badge-count,[\s\S]*?\{[^}]*background:\s*var\(--uipilot-ui-primary\);/,
    )
    await mounted.unmount()
    core.destroy()
  })

  it('shares unread count with the Messages tab, clears on entry, and restores for later messages', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.openMessageCenter).mockResolvedValueOnce(
      messageCenterSnapshot('2', 0, ['kept message']),
    )
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('shared-message-badge')))
    await act(async () => fake.emitMessageState({ status: 'ready', revision: '1', unreadCount: 3 }))
    expect(
      mounted.host.querySelector('.launcher-settings-badge .ant-badge-count')?.getAttribute('title'),
    ).toBe('3')

    await act(async () => {
      mounted.host.querySelector<HTMLButtonElement>('button[aria-label="打开设置"]')?.click()
    })
    await vi.waitFor(() => expect(core.getSnapshot().view).toBe('settings'))
    expect(
      settingsTab(mounted.host, '消息').querySelector('.settings-message-tab-badge .ant-badge-count')?.getAttribute('title'),
    ).toBe('3')

    await activateSettingsTab(mounted.host, '消息')
    await vi.waitFor(() => expect(fake.client.openMessageCenter).toHaveBeenCalledOnce())
    expect(mounted.host.textContent).toContain('kept message')
    expect(settingsTab(mounted.host, '消息').querySelector('.ant-badge-count')).toBeNull()
    expect(core.getSnapshot().messageCenter.unreadCount).toBe(0)

    await act(async () => fake.emitMessageState({ status: 'ready', revision: '3', unreadCount: 1 }))
    expect(
      settingsTab(mounted.host, '消息').querySelector('.ant-badge-count')?.getAttribute('title'),
    ).toBe('1')
    expect(mounted.host.textContent).toContain('kept message')

    await act(async () => {
      mounted.host.querySelector<HTMLButtonElement>('button[aria-label="返回主界面"]')?.click()
    })
    await vi.waitFor(() => expect(core.getSnapshot().view).toBe('launcher'))
    expect(
      mounted.host.querySelector('.launcher-settings-badge .ant-badge-count')?.getAttribute('title'),
    ).toBe('1')
    await mounted.unmount()
    core.destroy()
  })

  it('switches settings panels without loading and resets to general for a new view epoch', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.listPlugins).mockResolvedValue(pluginInventory())
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-tabs-first', 'settings')))
    expect(core.getSnapshot().plugins?.status).toBe('idle')

    const settingsLoads = vi.mocked(fake.client.loadSettings).mock.calls.length
    const pluginLoads = vi.mocked(fake.client.listPlugins).mock.calls.length
    const pluginTab = await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
    expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()
    expect(mounted.host.textContent).not.toContain('未发现插件')
    expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeNull()
    expect(fake.client.loadSettings).toHaveBeenCalledTimes(settingsLoads)
    expect(fake.client.listPlugins).toHaveBeenCalledTimes(pluginLoads + 1)

    await act(async () => {
      pluginTab.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'ArrowUp',
          code: 'ArrowUp',
          bubbles: true,
          cancelable: true,
        }),
      )
    })
    await vi.waitFor(() => expect(settingsTab(mounted.host, '消息').getAttribute('aria-selected')).toBe('true'))
    expect(document.activeElement).toBe(settingsTab(mounted.host, '消息'))
    expect(mounted.host.textContent).toContain('暂无消息')
    expect(fake.client.loadSettings).toHaveBeenCalledTimes(settingsLoads)
    expect(fake.client.listPlugins).toHaveBeenCalledTimes(pluginLoads + 1)

    await act(async () => {
      settingsTab(mounted.host, '消息').dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'ArrowUp',
          code: 'ArrowUp',
          bubbles: true,
          cancelable: true,
        }),
      )
    })
    await vi.waitFor(() => expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true'))
    expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()

    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(fake.client.listPlugins).toHaveBeenCalledTimes(pluginLoads + 2))
    await act(async () => fake.emit(shown('settings-tabs-launcher', 'launcher')))
    await act(async () => fake.emit(shown('settings-tabs-second', 'settings')))
    await vi.waitFor(() => expect(document.activeElement).toBe(settingsTab(mounted.host, '通用')))
    expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true')
    expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()
    expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()

    await mounted.unmount()
    core.destroy()
  })

  it('keeps general and plugin loading failures inside their own tab panels', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings)
      .mockResolvedValueOnce(settingsFixture)
      .mockRejectedValueOnce({ code: 'settingsFailed', message: 'private settings error' })
    vi.mocked(fake.client.listPlugins).mockResolvedValueOnce(pluginInventory([installedPlugin('1.0.0', '')]))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-tab-error', 'settings')))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('error'))
    expect(mounted.host.textContent?.replace(/\s/g, '')).toContain('重试')
    expect(mounted.host.querySelector('.plugin-item')).toBeNull()

    await activateSettingsTab(mounted.host, '插件')
    expect(mounted.host.querySelector('.plugin-item h3')?.textContent).toBe('internal.math')
    expect(mounted.host.textContent).not.toContain('private settings error')

    await mounted.unmount()
    core.destroy()
  })

  it('keeps a plugin list failure out of the general settings panel', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.listPlugins).mockRejectedValueOnce({
      code: 'pluginListFailed',
      message: 'private plugin error',
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('plugin-tab-error', 'settings')))

    const hotkey = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')
    expect(hotkey).toBeTruthy()
    expect(hotkey?.disabled).toBe(true)
    expect([...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')?.disabled).toBe(false)
    expect(mounted.host.textContent).not.toContain('无法加载插件清单。')

    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('error'))
    expect(mounted.host.querySelector('[role="alert"]')?.textContent).toBe('无法加载插件清单。')
    expect(mounted.host.textContent).not.toContain('private plugin error')

    await activateSettingsTab(mounted.host, '通用')
    expect(mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')?.disabled).toBe(true)

    await mounted.unmount()
    core.destroy()
  })

  it('keeps a plugin reload running while its tab is hidden', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const plugin = installedPlugin('1.0.0', '')
    const reload = deferred<{ revision: string }>()
    vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(fake.client.listPlugins)
      .mockResolvedValueOnce(pluginInventory([plugin], '1'))
      .mockResolvedValueOnce(pluginInventory([installedPlugin('2.0.0', '')], '2'))
      .mockResolvedValueOnce(pluginInventory([installedPlugin('2.0.0', '')], '2'))
    vi.mocked(fake.client.reloadPlugin).mockReturnValueOnce(reload.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-hidden-plugin', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))

    const reloadButton = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === '重新加载',
    )!
    await act(async () => reloadButton.click())
    await activateSettingsTab(mounted.host, '通用')
    await act(async () => reload.resolve({ revision: '2' }))
    expect(fake.client.listPlugins).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '1.0.0' })
    expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()

    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.installed).toMatchObject({ activeVersion: '2.0.0' }))
    expect(mounted.host.textContent).toContain('2.0.0')

    await mounted.unmount()
    core.destroy()
  })

  it('shows fixed settings load failure and retry without a permanent spinner', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings)
      .mockResolvedValueOnce(settingsFixture)
      .mockRejectedValueOnce({ code: 'settingsFailed', message: 'raw backend' })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('settings-failure', 'settings'))
    const mounted = await mountLauncherView(core)
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('error'))
    expect(mounted.host.querySelector('.launcher-status-region')).not.toBeNull()
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toContain('设置未能确认完成')
    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeNull()
    expect([...mounted.host.querySelectorAll('button')].some((button) => button.textContent?.replace(/\s/g, '') === '重试')).toBe(true)
    expect(mounted.host.textContent).not.toContain('重新加载设置')
    expect(mounted.host.textContent).not.toContain('raw backend')
    await mounted.unmount()
  })

  it('shows only loading during startup hydration and enables retry after failure', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const initial = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings)
      .mockReturnValueOnce(initial.promise)
      .mockRejectedValueOnce({ code: 'settingsFailed', message: 'private' })
      .mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
    fake.emit(shown('settings-loading', 'settings'))
    const mounted = await mountLauncherView(core)
    const retryButton = () =>
      [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.replace(/\s/g, '') === '重试')

    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeTruthy()
    expect(retryButton()).toBeUndefined()

    initial.reject({ code: 'settingsFailed', message: 'private' })
    await act(async () => start)
    await vi.waitFor(() => expect(retryButton()).toBeTruthy())
    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeNull()
    expect(retryButton()).toBeTruthy()

    await act(async () => retryButton()!.click())
    await vi.waitFor(() => expect(core.getSnapshot().settings).toBeDefined())
    expect(fake.client.loadSettings).toHaveBeenCalledTimes(3)
    await mounted.unmount()
  })

  it('keeps showing loading without a snapshot when lifecycle uncertainty is already latched', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const startup = deferred<SettingsView>()
    const current = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(startup.promise).mockReturnValueOnce(current.promise)
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
    fake.emit(shown('settings-uncertain-loading', 'settings', 'settingsFailed'))
    const mounted = await mountLauncherView(core)
    const retryButton = () =>
      [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find(
        (button) => button.textContent?.replace(/\s/g, '') === '重试',
      )

    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeTruthy()
    expect(retryButton()).toBeUndefined()
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toContain('请重启 UiPilot')

    startup.resolve(settingsFixture)
    await starting
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledTimes(2))
    current.resolve(settingsFixture)
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))
    await mounted.unmount()
  })

  it('resets settings only after confirmation and persists the fixed defaults', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const changedSettings = { ...settingsFixture, hotkey: 'DoubleCtrl', autostart: true }
    const initializedSettings = { ...settingsFixture, hotkey: 'Shift+Space', autostart: false }
    vi.mocked(fake.client.loadSettings)
      .mockResolvedValueOnce(changedSettings)
      .mockResolvedValueOnce(changedSettings)
      .mockResolvedValueOnce(initializedSettings)
      .mockResolvedValueOnce({ ...initializedSettings, hotkey: 'DoubleCtrl' })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-reset', 'settings')))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))

    const resetButton = () =>
      [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find(
        (button) => button.textContent?.replace(/\s/g, '') === '恢复初始化',
      )
    const portalButton = (label: string) =>
      [...document.body.querySelectorAll<HTMLButtonElement>('button')].find(
        (button) => button.textContent?.replace(/\s/g, '') === label,
      )

    expect(resetButton()).toBeTruthy()
    await act(async () => resetButton()!.click())
    expect(document.body.textContent).toContain(
      '快捷键将恢复为 Shift+Space，关闭开机启动，将风格恢复为跟随系统，并将搜索引擎恢复为 Bing。',
    )
    await act(async () => portalButton('取消')!.click())
    expect(fake.client.saveSettings).not.toHaveBeenCalled()

    await act(async () => resetButton()!.click())
    await act(async () => portalButton('恢复')!.click())
    await vi.waitFor(() =>
      expect(fake.client.saveSettings).toHaveBeenCalledWith({
        settings: { hotkey: 'Shift+Space', autostart: false, theme: 'system', webSearchEngine: 'bing' },
      }),
    )
    await vi.waitFor(() =>
      expect(core.getSnapshot().settings).toMatchObject({
        hotkey: { value: 'Shift+Space' },
        autostart: false,
        loadStatus: 'ready',
        readOnly: false,
      }),
    )

    const hotkey = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')
    if (!hotkey) throw new Error('settings hotkey input missing after reset')
    expect(hotkey.disabled).toBe(true)
    const recorder = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === '重新录制')
    if (!recorder) throw new Error('settings hotkey recorder button missing after reset')
    await act(async () => recorder.click())
    expect(hotkey.disabled).toBe(false)
    expect(document.activeElement).toBe(hotkey)
    await act(async () => {
      hotkey.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
      hotkey.dispatchEvent(new KeyboardEvent('keyup', { key: 'Control', code: 'ControlLeft', bubbles: true, cancelable: true }))
      hotkey.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
      hotkey.dispatchEvent(new KeyboardEvent('keyup', { key: 'Control', code: 'ControlLeft', bubbles: true, cancelable: true }))
    })
    await vi.waitFor(() => expect(fake.client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } }))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.hotkey.value).toBe('DoubleCtrl'))
    await mounted.unmount()
  })

  it('unbinds the native input before retiring its control and reports ready once', async () => {
    installMatchMedia(false)
    const cleanup: string[] = []
    const bind = vi.spyOn(nativeInput, 'bindNativeTextInput').mockImplementation((_input, control) => () => {
      cleanup.push('native-unbind')
    })
    const { core } = await startedCore()
    const originalRetire = core.retireControl
    vi.spyOn(core, 'retireControl').mockImplementation((control) => {
      cleanup.push(`retire:${control}`)
      originalRetire(control)
    })
    const control = core.getSnapshot().queryControl
    const mounted = await mountLauncherView(core)
    expect(mounted.onReady).toHaveBeenCalledOnce()
    expect(mounted.onReady).toHaveBeenCalledWith('ready')
    await mounted.unmount()
    expect(cleanup).toEqual(['native-unbind', `retire:${control}`])
    expect(bind).toHaveBeenCalledOnce()
    bind.mockRestore()
  })

  it('keeps the native binding and active composition owner across ordinary publishes', async () => {
    installMatchMedia(false)
    const unbind = vi.fn()
    const bind = vi.spyOn(nativeInput, 'bindNativeTextInput').mockReturnValue(unbind)
    const { core, client, emit } = await startedCore()
    emit(shown('stable-binding'))
    const retire = vi.spyOn(core, 'retireControl')
    const control = core.getSnapshot().queryControl
    const mounted = await mountLauncherView(core)

    await act(async () => {
      core.text({ kind: 'compositionStart', control })
      core.text({ kind: 'compositionInput', control, value: '计', inputType: 'insertCompositionText' })
    })

    expect(bind).toHaveBeenCalledOnce()
    expect(unbind).not.toHaveBeenCalled()
    expect(retire).not.toHaveBeenCalled()
    await act(async () => {
      core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
      core.text({ kind: 'compositionBoundary', control })
    })
    expect(client.searchApps).toHaveBeenCalledWith({ query: '计算器', invocationId: 'stable-binding', querySequence: 2 })

    await mounted.unmount()
    expect(unbind).toHaveBeenCalledOnce()
    expect(retire).toHaveBeenCalledOnce()
    bind.mockRestore()
  })

  it('retires the old settings control before a form replacement', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('replacement-view', 'settings')))
    const oldHotkey = core.getSnapshot().settings!.hotkey.key
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await act(async () => core.reloadSettings())
    const replaced = core.getSnapshot()
    expect(replaced.settings!.hotkey.key).toBeGreaterThan(oldHotkey)
    core.text({ kind: 'ordinaryInput', control: oldHotkey, value: 'late', inputType: 'insertText' })
    expect(core.getSnapshot()).toBe(replaced)
    await mounted.unmount()
  })

  it('keeps the React/AntD source boundary exact', () => {
    for (const required of [
      'ConfigProvider',
      'App',
      'Input',
      'Form',
      'Checkbox',
      'Button',
      'Popconfirm',
      'Select',
      'Spin',
      'Tabs',
      'theme',
    ]) {
      expect(launcherViewSource).toContain(required)
    }
    for (const forbidden of [
      '@tauri-apps/api',
      'AutoComplete',
      'Card',
      'Modal',
      'dangerouslySetInnerHTML',
      'appId',
    ]) {
      expect(launcherViewSource).not.toContain(forbidden)
    }
    expect(launcherViewSource).toContain("from './ui-theme'")
    expect(launcherViewSource).toContain('uiThemeConfig(colorScheme)')
    expect(launcherViewSource).toContain("from 'lucide-react'")
    expect(launcherViewSource).not.toContain('@ant-design/icons')
    expect(publicPluginPanelSource).toContain("from 'lucide-react'")
    expect(publicPluginPanelSource).not.toContain('@ant-design/icons')
  })

  it('renders public plugins as compact rows and opens advanced details on demand', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPublicPlugins).mockResolvedValue({
      revision: '1', hotkeyRevision: '0',
      items: [{
        pluginId: 'com.example.demo', name: 'Demo', description: 'A compact plugin description that stays on one row.', version: '1.0.0',
        source: 'localPackage', installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'demo', effectiveName: 'demo', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
        fault: null, generation: 1,
        iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.demo/1/icon.png',
        network: { httpsHosts: ['api.example.com'], credentials: [] },
        permissions: [
          { permission: 'network.https', supported: true, granted: false },
          { permission: 'clipboard.history.read', supported: true, granted: true },
          { permission: 'clipboard.history.paste', supported: false, granted: false },
        ],
        settings: [
          { definition: { type: 'text', key: 'prefix', label: 'Prefix' }, value: 'Hello' },
          { definition: { type: 'number', key: 'limit', label: 'Limit', min: 1, max: 9 }, value: 3 },
          { definition: { type: 'boolean', key: 'loud', label: 'Loud' }, value: false },
          { definition: { type: 'select', key: 'style', label: 'Style', options: [{ value: 'short', label: 'Short' }] }, value: 'short' },
          { definition: { type: 'secret', key: 'token', label: 'Token' }, secretConfigured: true },
        ],
      }],
    })
    const inventory = await fake.client.listPublicPlugins()
    vi.mocked(fake.client.setPublicPluginEffectiveName).mockImplementation(async ({ commandId, nameOverride }) => {
      const command = inventory.items.flatMap((plugin) => plugin.commands).find((command) => command.commandId === commandId)!
      command.nameOverride = nameOverride
      command.effectiveName = nameOverride ?? command.defaultName
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-settings', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())
    const row = mounted.host.querySelector<HTMLElement>('.public-plugin-item')!
    expect(row.querySelector('.public-plugin-icon-cell .plugin-icon-image')?.getAttribute('src'))
      .toBe('uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.demo/1/icon.png')
    expect(row.querySelector('.plugin-title-line')?.textContent).toContain('Demo')
    expect(row.textContent).not.toContain('v1.0.0')
    expect(row.querySelector('.plugin-title-line')?.textContent).not.toContain('启动命令')
    expect(row.querySelector('.public-plugin-command')?.textContent).toContain('/demo')
    expect(row.querySelector('.plugin-description')).toBeNull()
    expect(row.querySelector('.public-name-control')).toBeNull()
    expect(row.querySelector('button[aria-label="查看插件详情"]')).not.toBeNull()
    expect(row.querySelector('button[aria-label="卸载插件"]')).toBeNull()
    expect(row.querySelector('.public-plugin-actions .ant-switch')).not.toBeNull()
    expect(mounted.host.querySelector('.public-plugin-form')).toBeNull()
    expect(mounted.host.querySelector('.public-permissions')).toBeNull()
    expect(mounted.host.querySelector('.public-network-access')).toBeNull()
    const refresh = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="刷新"]')!
    expect(refresh.textContent?.trim()).toBe('')

    await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="查看插件详情"]')!.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-detail')).not.toBeNull())
    const detailView = mounted.host.querySelector<HTMLElement>('.public-plugin-detail-view')!
    const detail = mounted.host.querySelector<HTMLElement>('.public-plugin-detail')!
    expect(mounted.host.querySelector('.settings-tabs')).toBeNull()
    expect(mounted.host.querySelector('header.settings-header h1')?.textContent).toBe('Demo')
    const backButton = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="返回插件列表"]')!
    expect(backButton).not.toBeNull()
    await vi.waitFor(() => expect(document.activeElement).toBe(detailView))
    expect(mounted.host.querySelector('.public-plugin-detail-title')?.textContent).toBe('Demo')
    expect(detail.textContent).toContain('启动')
    expect(detail.textContent).not.toContain('启动名称')
    expect(detail.querySelector('.public-detail-name-term')).not.toBeNull()
    expect(detail.querySelector('.public-detail-name-value')).not.toBeNull()
    expect(stylesSource).toMatch(/\.public-plugin-detail-list\s*\{[^}]*padding:\s*8px 0 0;/s)
    expect(stylesSource).toMatch(/\.public-detail-name-term,\s*\.public-detail-name-value\s*\{[^}]*align-self:\s*center;/s)
    expect(detail.textContent).toContain('版本号')
    expect(detail.textContent).toContain('1.0.0')
    expect(detail.textContent).toContain('插件说明')
    expect(detail.textContent).toContain('A compact plugin description that stays on one row.')
    expect(detail.textContent).toContain('权限列表')
    expect(detail.textContent).toContain('网络访问 · network.https · 已关闭')
    expect(detail.textContent).toContain('剪贴板历史读取 · clipboard.history.read · 已开启')
    expect(detail.textContent).toContain('剪贴板历史粘贴 · clipboard.history.paste · 暂不支持')
    expect(detail.textContent).toContain('会在本机记录该插件可用的剪贴板历史摘要')
    expect(detail.textContent).not.toContain('clipboard.read 是 clipboard.history.read')
    expect(detail.textContent).toContain('网络 Host')
    expect(detail.textContent).toContain('api.example.com')
    expect(detail.textContent).toContain('插件所在目录')
    expect(detail.textContent).toContain('暂未提供插件目录')

    const nameInput = detail.querySelector<HTMLInputElement>('input[aria-label="启动键"]')!
    expect(nameInput.value).toBe('/demo')
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      setter.call(nameInput, 'demo-custom')
      nameInput.dispatchEvent(new Event('input', { bubbles: true }))
      nameInput.dispatchEvent(new Event('change', { bubbles: true }))
      nameInput.dispatchEvent(new FocusEvent('focusout', { bubbles: true }))
    })
    await vi.waitFor(() => expect(fake.client.setPublicPluginEffectiveName).toHaveBeenCalledWith({ commandId: 'default',
      pluginId: 'com.example.demo',
      nameOverride: 'demo-custom',
    }))

    await act(async () => detail.querySelector<HTMLButtonElement>('button[aria-label="恢复默认启动键"]')!.click())
    await vi.waitFor(() => expect(fake.client.setPublicPluginEffectiveName).toHaveBeenLastCalledWith({ commandId: 'default',
      pluginId: 'com.example.demo',
      nameOverride: null,
    }))

    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    await act(async () => document.activeElement?.dispatchEvent(escape))
    expect(escape.defaultPrevented).toBe(true)
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())
    expect(mounted.host.querySelector('.settings-tabs')).not.toBeNull()
    await vi.waitFor(() => expect(document.activeElement).toBe(settingsTab(mounted.host, '插件')))

    await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="查看插件详情"]')!.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-detail')).not.toBeNull())

    await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="返回插件列表"]')!.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())
    expect(mounted.host.querySelector('.settings-tabs')).not.toBeNull()
    await vi.waitFor(() => expect(document.activeElement).toBe(settingsTab(mounted.host, '插件')))
    await mounted.unmount()
    core.destroy()
  })

  it('filters public plugins by name and startup command after a short debounce', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPublicPlugins).mockResolvedValue({
      revision: '1', hotkeyRevision: '0',
      items: [
        {
          pluginId: 'com.example.notes', name: 'Notes', description: null, version: '1.0.0',
          source: 'localPackage', installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'notes', effectiveName: 'notes', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
          fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [],
        },
        {
          pluginId: 'com.example.translate', name: 'Translate', description: null, version: '1.0.0',
          source: 'localPackage', installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'translate', effectiveName: 'translate', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
          fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [],
        },
      ],
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-filter', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('.public-plugin-item')).toHaveLength(2))
    vi.useFakeTimers()
    try {
      const filter = mounted.host.querySelector<HTMLInputElement>('input[aria-label="搜索插件名称或启动命令"]')
      expect(filter).toBeInstanceOf(HTMLInputElement)
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
        setter.call(filter!, 'trans')
        filter!.dispatchEvent(new Event('input', { bubbles: true }))
        filter!.dispatchEvent(new Event('change', { bubbles: true }))
      })
      expect(mounted.host.querySelectorAll('.public-plugin-item')).toHaveLength(2)

      await act(async () => { vi.advanceTimersByTime(149) })
      expect(mounted.host.querySelectorAll('.public-plugin-item')).toHaveLength(2)

      await act(async () => { vi.advanceTimersByTime(1) })
      await vi.waitFor(() => expect(mounted.host.querySelectorAll('.public-plugin-item')).toHaveLength(1))
      expect(mounted.host.querySelector('.plugin-title-line h3')?.textContent).toBe('Translate')
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(filter!, '/notes')
        filter!.dispatchEvent(new Event('input', { bubbles: true }))
      })
      await act(async () => { vi.advanceTimersByTime(150) })
      await vi.waitFor(() => expect(mounted.host.querySelector('.plugin-title-line h3')?.textContent).toBe('Notes'))
    } finally {
      vi.useRealTimers()
      await mounted.unmount()
      core.destroy()
    }
  })

  it('focuses the public plugin filter with Ctrl+F while the plugins tab is active', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPublicPlugins).mockResolvedValue({ revision: '1', hotkeyRevision: '0', items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('public-filter-shortcut', 'settings')))
      await activateSettingsTab(mounted.host, '插件')
      const filter = mounted.host.querySelector<HTMLInputElement>('input[aria-label="搜索插件名称或启动命令"]')!
      const focusSearch = new KeyboardEvent('keydown', {
        key: 'f', ctrlKey: true, bubbles: true, cancelable: true,
      })

      await act(async () => settingsTab(mounted.host, '插件').dispatchEvent(focusSearch))

      expect(focusSearch.defaultPrevented).toBe(true)
      expect(document.activeElement).toBe(filter)
    } finally {
      await mounted.unmount()
      core.destroy()
    }
  })

  it('keeps the compact public row toggle usable without exposing deletion', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const item = {
      pluginId: 'com.example.cleanup', name: 'Cleanup', description: null, version: '1.0.0',
      source: 'localPackage' as const, installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'cleanup', effectiveName: 'cleanup', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
      fault: null, generation: 1, iconUrl: null, permissions: [], settings: [],
      network: null,
    }
    vi.mocked(fake.client.listPublicPlugins)
      .mockResolvedValueOnce({ revision: '1', hotkeyRevision: '0', items: [item] })
      .mockResolvedValue({ revision: '2', hotkeyRevision: '0', items: [{ ...item, enabled: false }] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-cleanup-pending', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())

    expect(mounted.host.querySelector('button[aria-label="卸载插件"]')).toBeNull()
    await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="启用 Cleanup"]')!.click())
    expect(fake.client.setPublicPluginEnabled).toHaveBeenCalledExactlyOnceWith({ pluginId: item.pluginId, enabled: false })
    await vi.waitFor(() => expect(fake.client.listPublicPlugins).toHaveBeenCalledTimes(2))
    expect(mounted.host.querySelector('[aria-label="启用 Cleanup"]')?.getAttribute('aria-checked')).toBe('false')
    expect(fake.client.uninstallPublicPlugin).not.toHaveBeenCalled()
    await mounted.unmount()
    core.destroy()
  })

  it('shows the prepared public plugin icon and restores focus after installation', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.selectPublicPluginDirectory).mockResolvedValueOnce('D:\\demo-win')
    vi.mocked(fake.client.preparePublicPlugin).mockResolvedValueOnce({
      token: 'public-prepare-0000000000000001-0000000000000002',
      pluginId: 'com.uipilot.demo-win',
      name: 'Public Plugin Demo Window',
      version: '1.0.2',
      permissions: ['ui.window', 'clipboard.history.read', 'clipboard.history.paste', 'url.open', 'files.openText'],
      isUpdate: false, operation: 'install', previousVersion: null,
      sourceVerified: false,
      iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/prepared/public-prepare-0000000000000001-0000000000000002/icon.png',
      network: null,
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-prepare-icon', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    const chooseDirectory = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="选择开发目录"]')!
    expect(chooseDirectory.textContent).toContain('选择开发目录')
    expect(mounted.host.querySelector<HTMLButtonElement>('button[aria-label="选择插件包"]')?.textContent).toContain('选择插件包')
    await act(async () => chooseDirectory.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-prepare')).not.toBeNull())
    expect(mounted.host.querySelector<HTMLImageElement>('.public-prepare .plugin-icon-image')?.getAttribute('src'))
      .toBe('uipilot-public-plugin://localhost/__uipilot_icon/prepared/public-prepare-0000000000000001-0000000000000002/icon.png')
    const prepare = mounted.host.querySelector<HTMLElement>('.public-prepare')!
    expect(prepare.textContent).toContain('独立窗口 · ui.window')
    expect(prepare.textContent).toContain('在默认浏览器打开 HTTP(S) 链接 · url.open')
    expect(prepare.textContent).toContain('选择并读取本地 UTF-8 文本文件（最多 8 KiB） · files.openText')
    expect(prepare.textContent).not.toContain('files.openText · files.openText')
    expect(prepare.textContent).toContain('剪贴板历史读取 · clipboard.history.read')
    expect(prepare.textContent).toContain('剪贴板历史粘贴 · clipboard.history.paste')
    expect(prepare.textContent).toContain('会在本机记录该插件可用的剪贴板历史摘要')
    expect(prepare.textContent).not.toContain('clipboard.read')
    const confirm = [...mounted.host.querySelectorAll<HTMLButtonElement>('.public-prepare button')]
      .find((button) => button.textContent?.includes('确认安装'))!
    await act(async () => {
      confirm.focus()
      confirm.click()
    })
    await vi.waitFor(() => expect(fake.client.commitPublicPlugin).toHaveBeenCalledOnce())
    await vi.waitFor(() => expect(document.activeElement).toBe(chooseDirectory))
    await mounted.unmount()
    core.destroy()
  })

  it('shows exact newly requested HTTPS hosts and cancelling consent restores focus without commit', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.selectPublicPluginDirectory).mockResolvedValueOnce('D:\\network-plugin')
    vi.mocked(fake.client.preparePublicPlugin).mockResolvedValueOnce({
      token: 'public-prepare-0000000000000003-0000000000000004',
      pluginId: 'com.example.network',
      name: 'Network Plugin',
      version: '1.1.0',
      permissions: ['network.https'],
      isUpdate: true, operation: 'replace', previousVersion: '1.0.0',
      sourceVerified: false,
      iconUrl: null,
      network: {
        httpsHosts: ['api.example.com', 'auth.example.com'], credentials: [],
        addedHttpsHosts: ['auth.example.com'], addedCredentialIds: [],
        requiresNetworkConsent: true,
      },
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-network-consent', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    const chooseDirectory = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="选择开发目录"]')!
    await act(async () => chooseDirectory.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-network-consent')).not.toBeNull())

    const consent = mounted.host.querySelector('.public-network-consent')!
    expect(consent.textContent).toContain('api.example.com')
    expect(consent.textContent).toContain('auth.example.com')
    expect(consent.textContent).toContain('新增')
    expect(consent.textContent).toContain('Host 代理')
    expect(consent.textContent).toContain('不会开放插件 WebView')
    expect(consent.querySelector('a')).toBeNull()

    const cancel = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="取消安装"]')!
    await act(async () => cancel.click())
    await vi.waitFor(() => expect(fake.client.cancelPublicPlugin).toHaveBeenCalledWith({
      token: 'public-prepare-0000000000000003-0000000000000004',
    }))
    expect(fake.client.commitPublicPlugin).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(document.activeElement).toBe(chooseDirectory))
    await mounted.unmount()
    core.destroy()
  })

  it('marks every fresh-install host as new and does not re-prompt when an update only removes hosts', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.selectPublicPluginDirectory)
      .mockResolvedValueOnce('D:\\network-fresh')
      .mockResolvedValueOnce('D:\\network-narrowed')
    const base = {
      pluginId: 'com.example.network',
      name: 'Network Plugin',
      permissions: ['network.https' as const],
      sourceVerified: false,
      iconUrl: null,
    }
    vi.mocked(fake.client.preparePublicPlugin)
      .mockResolvedValueOnce({
        ...base,
        token: 'public-prepare-0000000000000005-0000000000000006',
        version: '1.0.0',
        isUpdate: false, operation: 'install', previousVersion: null,
        network: {
          httpsHosts: ['api.example.com', 'auth.example.com'], credentials: [],
          addedHttpsHosts: ['api.example.com', 'auth.example.com'], addedCredentialIds: [],
          requiresNetworkConsent: true,
        },
      })
      .mockResolvedValueOnce({
        ...base,
        token: 'public-prepare-0000000000000007-0000000000000008',
        version: '1.1.0',
        isUpdate: true, operation: 'replace', previousVersion: '1.0.0',
        network: {
          httpsHosts: ['api.example.com'], credentials: [],
          addedHttpsHosts: [], addedCredentialIds: [],
          requiresNetworkConsent: false,
        },
      })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-network-fresh-and-narrowed', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    const chooseDirectory = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="选择开发目录"]')!

    await act(async () => chooseDirectory.click())
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('.public-network-added')).toHaveLength(2))
    expect(mounted.host.querySelector('.public-network-consent')?.textContent).toContain('需要确认')
    await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="取消安装"]')!.click())
    await vi.waitFor(() => expect(document.activeElement).toBe(chooseDirectory))

    await act(async () => chooseDirectory.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-network-consent')).not.toBeNull())
    const narrowed = mounted.host.querySelector('.public-network-consent')!
    expect(narrowed.textContent).toContain('api.example.com')
    expect(narrowed.textContent).not.toContain('需要确认')
    expect(narrowed.querySelector('.public-network-added')).toBeNull()
    await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="取消安装"]')!.click())
    await vi.waitFor(() => expect(fake.client.cancelPublicPlugin).toHaveBeenCalledTimes(2))
    await mounted.unmount()
    core.destroy()
  })

  it('keeps public plugin network details out of the compact row', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const networkItem = {
      pluginId: 'com.example.network', name: 'Network', description: null, version: '1.0.0',
      source: 'localPackage' as const, installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'network', effectiveName: 'network', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
      fault: null, generation: 1, iconUrl: null,
      network: { httpsHosts: ['api.example.com', 'auth.example.com'], credentials: [] },
      permissions: [{ permission: 'network.https' as const, supported: true, granted: true }],
      settings: [],
    }
    vi.mocked(fake.client.listPublicPlugins)
      .mockResolvedValueOnce({ revision: '1', hotkeyRevision: '0', items: [networkItem] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-network-toggle', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())

    expect(mounted.host.querySelector('button[aria-label="网络访问"]')).toBeNull()
    expect(mounted.host.textContent).not.toContain('api.example.com')
    expect(mounted.host.textContent).not.toContain('auth.example.com')
    expect(fake.client.setPublicPluginNetworkAccess).not.toHaveBeenCalled()
    await mounted.unmount()
    core.destroy()
  })
  it('renders plugin metadata and safe markdown without links images or raw HTML', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const versionedPlugin = installedPlugin(
        '1.0.0',
        '# Math\n\n- item\n\n**bold** `code` [link](https://example.com) ![pixel](https://example.com/p.png)\n\n<strong>raw</strong>',
      )
    if (versionedPlugin.installed.state !== 'valid') throw new Error('fixture must be installed')
    versionedPlugin.installed.versions = ['0.9.0', '1.0.0']
    vi.mocked(fake.client.listPlugins).mockResolvedValueOnce(pluginInventory([
      versionedPlugin,
      installedPlugin('1.0.0', '', 'plain'),
    ]))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('plugin-markdown', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('.plugin-item')).toHaveLength(2))

    expect(mounted.host.querySelector('.plugin-item h3')?.textContent).toBe('internal.math')
    expect(mounted.host.textContent).toContain('已安装版本：0.9.0、1.0.0')
    expect(mounted.host.querySelector('.plugin-description h1')?.textContent).toBe('Math')
    expect(mounted.host.querySelector('.plugin-description li')?.textContent).toBe('item')
    expect(mounted.host.querySelector('.plugin-description strong')?.textContent).toBe('bold')
    expect(mounted.host.querySelector('.plugin-description code')?.textContent).toBe('code')
    expect(mounted.host.querySelector('.plugin-description a')).toBeNull()
    expect(mounted.host.querySelector('.plugin-description img')).toBeNull()
    expect(
      [...mounted.host.querySelectorAll('.plugin-description strong')].some((element) => element.textContent === 'raw'),
    ).toBe(false)
    expect(mounted.host.textContent).toContain('暂无说明')
    expect([...mounted.host.querySelectorAll('button')].some((button) => button.textContent?.includes('重新加载'))).toBe(true)
    expect(mounted.host.textContent?.replace(/\s/g, '')).toContain('删除')
    await mounted.unmount()
    core.destroy()
  })
})

describe('real adapter and startup', () => {
  function hostSnapshot(platform: 'windows' | 'macos'): import('./host-capabilities').HostCapabilities {
    const enabled = platform === 'windows'
    return { version: 1, platform, plugins: enabled, files: enabled, screenshot: enabled,
      messages: enabled, credentials: enabled, builtinFeatures: enabled, commandHotkeys: enabled,
      autostart: enabled, doubleTap: enabled }
  }
  function resetAdapterDocument() {
    vi.resetModules()
    document.body.innerHTML = '<main id="app"></main>'
    installMatchMedia(false)
    windowCapture.label = 'main'
    tauriCapture.capabilities.mockReset().mockResolvedValue(hostSnapshot('windows'))
    tauriCapture.invoke.mockReset()
    tauriCapture.listen.mockReset()
  }

  async function pagehide() {
    await act(async () => window.dispatchEvent(new Event('pagehide')))
  }

  it('boots the Mac launcher without optional listeners and exposes only supported settings', async () => {
    resetAdapterDocument()
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    tauriCapture.capabilities.mockResolvedValue(hostSnapshot('macos'))
    const subscriptions = new Map<string, { handler: (event: {payload: unknown}) => void; stop: ReturnType<typeof vi.fn> }>()
    tauriCapture.listen.mockImplementation(async (name: string, handler) => {
      expect(tauriCapture.capabilities).toHaveBeenCalledOnce()
      if (!['launcher://shown', 'launcher://hidden', 'hotkey-recording://current'].includes(name)) throw new Error(name)
      const stop = vi.fn()
      subscriptions.set(name, { handler, stop })
      return stop
    })
    tauriCapture.invoke.mockImplementation(async (name) => {
      if (name === 'load_settings') return emptySettings
      if (name === 'execute_result') return { status: 'launchRequested' }
      if (name === 'search_apps') return { requestId: 'current', items: [
        { resultId: 'app', title: 'Calculator', activation: { kind: 'executeResult' } },
        { resultId: 'find', title: '/find', activation: { kind: 'openFind', query: '' } },
      ] }
      throw new Error(name)
    })
    try {
      await act(async () => { await import('./main') })
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
      await act(async () => subscriptions.get('launcher://shown')!.handler({payload: shown('mac-apps')}))
      await vi.waitFor(() => expect(document.body.textContent).toContain('Calculator'))
      expect(document.body.textContent).not.toContain('/find')
      await act(async () => {
        document.querySelector('[role="combobox"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
      })
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('execute_result', {requestId: 'current', resultId: 'app'}))
      await act(async () => subscriptions.get('launcher://shown')!.handler({payload: shown('mac-settings', 'settings')}))
      await vi.waitFor(() => expect(document.querySelectorAll('[role="tab"]')).toHaveLength(1))
      expect(document.querySelector('[role="tab"]')?.textContent).toContain('通用')
      expect(document.body.textContent).not.toMatch(/截图快捷键|开机启动|搜索引擎|恢复初始化/)
      expect(tauriCapture.invoke.mock.calls.map(([name]) => name).every(name => ['load_settings', 'search_apps', 'execute_result'].includes(name))).toBe(true)
      expect(tauriCapture.listen.mock.calls).toHaveLength(3)
    } finally { await pagehide() }
    for (const { stop } of subscriptions.values()) expect(stop).toHaveBeenCalledOnce()
  })

  it('records DoubleMeta on the second release, cancels on blur and recovers the previous setting on permission denial', async () => {
    installMatchMedia(false)
    const { client, emit } = fakeClient()
    vi.mocked(client.loadSettings).mockResolvedValue(settingsFixture)
    vi.mocked(client.saveHotkey).mockRejectedValueOnce({ code: 'hotkeyPermissionRequired' })
    const core = createLauncherCore(client, undefined, { ...hostSnapshot('macos'), doubleTap: true })
    await core.start()
    emit(shown('mac-recording', 'settings'))
    await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('ready'))
    const mounted = await mountLauncherView(core)
    try {
      const input = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')!
      const button = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="录制快捷键"]')!
      const press = (type: 'keydown' | 'keyup') => input.dispatchEvent(new KeyboardEvent(type, {
        key: 'Meta', code: 'MetaLeft', metaKey: type === 'keydown', bubbles: true, cancelable: true,
      }))
      await act(async () => button.click())
      expect(input.value).toContain('双击')
      expect(mounted.host.textContent).toContain('仅启用此手势时需要「输入监控」权限')
      await act(async () => { press('keydown'); press('keyup'); window.dispatchEvent(new Event('blur')) })
      expect(client.saveHotkey).not.toHaveBeenCalled()
      expect(input.disabled).toBe(true)
      await act(async () => button.click())
      const pointer = (type: string) => input.dispatchEvent(new PointerEvent(type, { pointerId: 1, bubbles: true }))
      await act(async () => {
        press('keydown'); press('keyup'); pointer('pointerdown')
        press('keydown'); press('keyup'); pointer('pointerup')
      })
      expect(client.saveHotkey).not.toHaveBeenCalled()
      expect(input.disabled).toBe(false)
      await act(async () => { press('keydown'); press('keyup'); press('keydown') })
      expect(client.saveHotkey).not.toHaveBeenCalled()
      await act(async () => { press('keyup') })
      expect(client.saveHotkey).toHaveBeenCalledExactlyOnceWith({ hotkey: { hotkey: 'DoubleMeta' } })
      await vi.waitFor(() => expect(core.getSnapshot().settings).toMatchObject({ readOnly: false, needsReload: false, loadStatus: 'ready' }))
      expect(input.value).toBe('Option + Space')
      expect(core.getSnapshot().status).toContain('输入监控')
      expect(core.getSnapshot().status).toContain('已保留原快捷键')
    } finally {
      await mounted.unmount()
      core.destroy()
    }
  })

  it('does not start Find when its Mac scope marker is unknown', async () => {
    resetAdapterDocument()
    windowCapture.label = 'find'
    Object.defineProperty(window, '__UIPILOT_MACOS_FIND__', { configurable: true, value: 'unknown' })
    try {
      await act(async () => { await import('./main') })
      expect(document.body.textContent).toContain('文件搜索范围不可用')
      expect(tauriCapture.invoke).not.toHaveBeenCalled()
      expect(tauriCapture.listen).not.toHaveBeenCalled()
    } finally {
      Reflect.deleteProperty(window, '__UIPILOT_MACOS_FIND__')
      windowCapture.label = 'main'
    }
  })

  it.each([
    ['public-preview', '本机公共字体与共享目录'],
    ['indexed-local-qa', '本地 Spotlight 索引（隔离验收）'],
  ])('boots the %s Mac Find adapter, forwards the query and reveals only the selected handle', async (scope, description) => {
    resetAdapterDocument()
    windowCapture.label = 'find'
    Object.defineProperty(window, '__UIPILOT_MACOS_FIND__', { configurable: true, value: scope })
    let forward: ((event: { payload: unknown }) => void) | undefined
    tauriCapture.listen.mockImplementation(async (event, handler) => {
      if (event === 'find://forwarded') forward = handler
      return vi.fn()
    })
    tauriCapture.invoke.mockImplementation(async command => {
      if (command === 'prepare_find_initialization') return {
        status: 'prepared', initialization: {
          initializationToken: 'init', themeRevision: '1', theme: 'system',
          filePreviewRevision: '1', filePreviewEnabled: true, pinned: false,
        },
      }
      if (command === 'commit_find_ready') return {status: 'ready', initializationToken: 'init'}
      if (command === 'search_files') return fileResponse('1', [{
        ...fileItem(), fullPath: '/Users/Shared/example.png', name: 'example.png', canOpenDocument: true,
      }])
      if (command === 'execute_result') return {status: 'fileRevealRequested'}
      if (command === 'get_find_documents_access') return {status: 'notGranted', revision: '0'}
      if (command === 'grant_find_documents_access') return {status: 'granted', revision: '1'}
      if (command === 'revoke_find_documents_access') return {status: 'notGranted', revision: '2'}
      throw new Error(command)
    })
    try {
      await act(async () => { await import('./main') })
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('commit_find_ready', {input: {initializationToken: 'init'}}))
      await act(async () => forward!({payload: {invocationId: 'find-1', forwardSequence: '1', query: 'example'}}))
      expect(tauriCapture.invoke).toHaveBeenCalledWith('search_files', {
        query: 'example', category: 'all', sort: 'modifiedDesc', invocationId: 'find-1', querySequence: 1,
      })
      const row = document.querySelector<HTMLElement>('[role=option]')!
      expect(row.textContent).toContain('example.png')
      expect(document.body.textContent).toContain(description)
      const documents = document.querySelector<HTMLButtonElement>('.find-documents-button')
      if (scope === 'public-preview') {
        expect(tauriCapture.invoke).toHaveBeenCalledWith('get_find_documents_access', { invocationId: 'find-1' })
        await act(async () => documents!.click())
        expect(tauriCapture.invoke).toHaveBeenCalledWith('grant_find_documents_access', { invocationId: 'find-1' })
        await act(async () => documents!.click())
        expect(tauriCapture.invoke).toHaveBeenCalledWith('revoke_find_documents_access', { invocationId: 'find-1' })
      } else {
        expect(documents).toBeNull()
        expect(tauriCapture.invoke.mock.calls.some(([command]) => command.includes('documents_access'))).toBe(false)
      }
      const activeRow = document.querySelector<HTMLElement>('[role=option]')!
      await act(async () => { activeRow.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })
      expect(tauriCapture.invoke).toHaveBeenCalledWith('execute_result', {requestId: 'file-request-1', resultId: 'file-result-1'})
      expect(tauriCapture.invoke.mock.calls.some(([command]) => command === 'load_find_thumbnail')).toBe(false)
      expect(tauriCapture.capabilities).not.toHaveBeenCalled()
    } finally {
      await pagehide()
      Reflect.deleteProperty(window, '__UIPILOT_MACOS_FIND__')
      windowCapture.label = 'main'
    }
  })

  it('enables only Find when a Mac preview reports files without other builtins', async () => {
    const { client, emit } = fakeClient()
    let favorite = false
    vi.mocked(client.setBuiltinFeatureFavorite).mockImplementation(async () => { favorite = true })
    vi.mocked(client.searchApps).mockImplementation(async ({ query }) => ({
      requestId: 'preview-find',
      items: [{
        ...findLauncherItem(query), favorite: { target: { kind: 'builtin', feature: 'find' }, favorite },
      }, { resultId: 'screenshot', title: 'Screenshot', iconKind: 'screenshot', activation: { kind: 'openScreenshot' } }],
    }))
    const core = createLauncherCore(client, undefined, { ...hostSnapshot('macos'), files: true })
    await core.start()
    try {
      emit(shown('mac-find'))
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      core.openPluginContextMenu(0)
      core.setPluginFavorite(0, true)
      core.closePluginContextMenu()
      await vi.waitFor(() => expect(core.getSnapshot().results[0].favorite?.favorite).toBe(true))
      expect(client.setBuiltinFeatureFavorite).toHaveBeenCalledExactlyOnceWith({ feature: 'find', favorite: true })
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/find 字体', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(client.openFind).toHaveBeenCalledWith({ query: '字体', invocationId: 'mac-find', querySequence: 3 }))
      expect(client.startScreenshot).not.toHaveBeenCalled()
    } finally { core.destroy() }
  })

  it('routes Mac text and slash queries through application search only', async () => {
    const { client, emit } = fakeClient()
    client.cancelTranslationSelection = vi.fn(async () => {})
    const core = createLauncherCore(client, undefined, hostSnapshot('macos'))
    await core.start()
    try {
      emit(shown('mac-typing'))
      for (const value of ['Calc', '/find something']) {
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value, inputType: 'insertText' })
        await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledWith(expect.objectContaining({query: value})))
      }
      core.keyDown('Enter', false)
      expect(client.openFind).not.toHaveBeenCalled()
      expect(client.cancelTranslationSelection).not.toHaveBeenCalled()
      await core.requestHide()
      expect(client.hideLauncher).toHaveBeenCalledWith('mac-typing')
    } finally { core.destroy() }
  })

  it('releases only the Mac recording owner after a new invocation', async () => {
    const { client, emit } = fakeClient()
    const core = createLauncherCore(client, undefined, hostSnapshot('macos'))
    await core.start()
    try {
      emit(shown('A', 'settings'))
      await core.setHotkeyRecordingPhase('recording')
      emit(shown('B', 'launcher'))
      expect(core.getSnapshot().view).toBe('launcher')
      await core.setHotkeyRecordingPhase('idle')
      expect(client.setHotkeyRecording).toHaveBeenNthCalledWith(1, { recording: true, invocationId: 'A' })
      expect(client.setHotkeyRecording).toHaveBeenNthCalledWith(2, { recording: false, invocationId: 'A' })
    } finally { core.destroy() }
  })

  it('drops a pending capability snapshot when its window unloads', async () => {
    resetAdapterDocument()
    const result = deferred<ReturnType<typeof hostSnapshot>>()
    tauriCapture.capabilities.mockReturnValue(result.promise)
    await act(async () => { await import('./main') })
    await pagehide()
    await act(async () => { result.resolve(hostSnapshot('macos')) })
    expect(document.querySelector('#app')?.childElementCount).toBe(0)
    expect(tauriCapture.listen).not.toHaveBeenCalled()
  })

  it('retries a failed capability read without mounting optional services', async () => {
    resetAdapterDocument()
    tauriCapture.capabilities.mockResolvedValueOnce({version: 99}).mockResolvedValue(hostSnapshot('macos'))
    tauriCapture.listen.mockResolvedValue(vi.fn())
    tauriCapture.invoke.mockResolvedValue(emptySettings)
    try {
      await act(async () => { await import('./main') })
      expect(tauriCapture.listen).not.toHaveBeenCalled()
      const retry = document.querySelector<HTMLButtonElement>('#app button')
      expect(retry?.textContent).toBe('重试')
      await act(async () => retry!.click())
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
      expect(tauriCapture.capabilities).toHaveBeenCalledTimes(2)
    } finally { await pagehide() }
  })

  it('renders but does not report startup readiness until the focus listener is installed', async () => {
    resetAdapterDocument()
    const focusRegistration = deferred<() => void>()
    const focusUnlisten = vi.fn()
    tauriCapture.listen.mockImplementation((event) =>
      event === 'uipilot-plugin-panel-focus-host-input'
        ? focusRegistration.promise
        : Promise.resolve(vi.fn()),
    )
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(
        command === 'get_message_summary'
          ? { revision: '0', unreadCount: 0 }
          : command === 'load_settings'
            ? emptySettings
            : undefined,
      ),
    )

    await act(async () => {
      await import('./main')
    })
    expect(document.querySelector('[role="combobox"]')).toBeInstanceOf(HTMLInputElement)
    expect(tauriCapture.invoke).not.toHaveBeenCalled()

    focusRegistration.resolve(focusUnlisten)
    await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
    await pagehide()
    expect(focusUnlisten).toHaveBeenCalledOnce()
  })

  it('mounts and resolves the shown listener before loading, then uses the exact invoke table', async () => {
    resetAdapterDocument()
    const registration = deferred<() => void>()
    const load = deferred<SettingsView>()
    const unlisten = vi.fn()
    const order: string[] = []
    let shownHandler: ((event: { payload: unknown }) => void) | undefined
    let hiddenHandler: ((event: { payload: unknown }) => void) | undefined
    tauriCapture.listen.mockImplementation((event, handler) => {
      if (event === 'uipilot-plugin-panel-focus-host-input') {
        expect(document.querySelector('[role="combobox"]')).toBeNull()
      } else {
        expect(document.querySelector('[role="combobox"]')).toBeInstanceOf(HTMLInputElement)
      }
      order.push(String(event))
      if (event === 'launcher://shown') {
        shownHandler = handler as (event: { payload: unknown }) => void
        return registration.promise
      }
      if (event === 'launcher://hidden') {
        hiddenHandler = handler as (event: { payload: unknown }) => void
        return Promise.resolve(vi.fn())
      }
      return Promise.resolve(vi.fn())
    })
    tauriCapture.invoke.mockImplementation((command) => {
      order.push(String(command))
      if (command === 'get_message_summary') return Promise.resolve({ revision: '0', unreadCount: 0 })
      return command === 'load_settings' ? load.promise : Promise.resolve(undefined)
    })

    let main!: { client: LauncherClient }
    await act(async () => {
      main = (await import('./main')) as unknown as { client: LauncherClient }
    })
    await vi.waitFor(() => expect(tauriCapture.listen).toHaveBeenCalledWith('launcher://shown', expect.any(Function)))
    expect(tauriCapture.invoke).not.toHaveBeenCalled()
    registration.resolve(unlisten)
    await vi.waitFor(() => expect(tauriCapture.listen).toHaveBeenCalledWith('launcher://hidden', expect.any(Function)))
    await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
    expect(order.slice(0, 13)).toEqual([
      'uipilot-plugin-panel-focus-host-input',
      'hotkey-recording://current',
      'command-hotkey://entry',
      'launcher://shown',
      'launcher://hidden',
      'builtin-features://changed',
      'uipilot-plugin-panel-error',
      'uipilot-plugin-panel-status',
      'uipilot-plugin-panel-reset',
      'screenshot://error',
      'message-center://state-changed',
      'get_message_summary',
      'load_settings',
    ])

    await act(async () => shownHandler?.({ payload: shown('during-adapter-load', 'settings') }))
    expect(document.querySelector('.settings-view h1')?.textContent).toBe('设置')
    await act(async () => hiddenHandler?.({ payload: null }))
    expect(document.querySelector('.settings-view h1')).toBeNull()
    await act(async () => shownHandler?.({ payload: shown('during-adapter-load', 'settings') }))
    await act(async () => {
      load.resolve(emptySettings)
      await load.promise
    })

    tauriCapture.invoke.mockClear()
    tauriCapture.invoke.mockImplementation((command) => {
      if (command === 'list_plugins') return Promise.resolve(pluginInventory([installedPlugin()]))
      if (command === 'open_plugin_panel' || command === 'submit_plugin_panel') return Promise.resolve({
        sessionEpoch: '7', pluginId: 'com.uipilot.demo-panel', commandLabel: 'demo-panel', hostKeys: [],
      })
      if (command === 'plugin_panel_host_key_enqueue') return Promise.resolve({ outcome: 'droppedQueueFull' })
      if (command === 'install_plugin' || command === 'reload_plugin' || command === 'delete_plugin') {
        return Promise.resolve({ revision: '2' })
      }
      return Promise.resolve(undefined)
    })
    const update = { hotkey: 'Alt+Space', autostart: false, theme: 'system' as const, webSearchEngine: 'bing' as const }
    await main.client.searchApps({
      query: '/demo-win calc', invocationId: 'inv-1', querySequence: 1, submit: false,
      completionOrigin: { phase: 'preview', pluginId: 'com.uipilot.demo-win' },
    })
    await main.client.executeResult({ requestId: 'req-1', resultId: 'result-1' })
    await main.client.executeResultAction({ requestId: 'req-1', resultId: 'result-1', actionId: 'action-1' })
    await main.client.retireResultActions({ requestId: 'req-1' })
    await main.client.loadSettings()
    await main.client.selectScreenshotSaveDirectory()
    await main.client.setScreenshotSaveDirectory({ input: { directory: null } })
    await main.client.saveSettings({ settings: update })
    await main.client.setThemePreference({ preference: { theme: 'dark' } })
    await main.client.setPublicPluginFavorite({ pluginId: 'com.uipilot.demo-win', commandId: 'default', favorite: true })
    await main.client.setPublicPluginEffectiveName({ pluginId: 'com.example.multi', commandId: 'constructor', nameOverride: null })
    await main.client.setBuiltinFeatureFavorite({ feature: 'find', favorite: true })
    await main.client.openPluginPanel({ commandId: 'default', pluginId: 'com.uipilot.demo-panel', argument: 'hello' })
    await main.client.submitPluginPanel({ sessionEpoch: u64('7'), argument: 'hello', uiIntentEpoch: 1 })
    await main.client.enqueuePluginPanelHostKey({
      sessionEpoch: u64('7'), clientSequence: u64('1'), declaration: 'ArrowDown', key: 'ArrowDown',
      ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
    })
    await main.client.setPluginPanelBounds({
      sessionEpoch: u64('7'), bounds: { x: 12, y: 64, width: 696, height: 320 },
    })
    await main.client.closePluginPanel({ sessionEpoch: u64('7') })
    await main.client.acknowledgePluginPanelFocusHostInput({
      sessionEpoch: u64('7'), focusRequestId: u64('9'), focused: true,
    })
    await main.client.selectPublicPluginDirectory()
    await main.client.listPlugins()
    await main.client.installPlugin({ pluginId: 'internal.math' })
    await main.client.reloadPlugin({ pluginId: 'internal.math' })
    await main.client.deletePlugin({ pluginId: 'internal.math' })
    await main.client.hideLauncher()
    const invokeRows = [
      ['search_apps', [{
        query: '/demo-win calc', invocationId: 'inv-1', querySequence: 1, submit: false,
        completionOrigin: { phase: 'preview', pluginId: 'com.uipilot.demo-win' },
      }]],
      ['execute_result', [{ requestId: 'req-1', resultId: 'result-1' }]],
      ['execute_result_action', [{ request: { requestId: 'req-1', resultId: 'result-1', actionId: 'action-1' } }]],
      ['retire_result_actions', [{ request: { requestId: 'req-1' } }]],
      ['load_settings', []],
      ['select_screenshot_save_directory', []],
      ['set_screenshot_save_directory', [{ input: { directory: null } }]],
      ['save_settings', [{ settings: update }]],
      ['set_theme_preference', [{ preference: { theme: 'dark' } }]],
      ['set_plugin_favorite', [{ commandId: 'default', pluginId: 'com.uipilot.demo-win', favorite: true }]],
      ['set_plugin_effective_name', [{ pluginId: 'com.example.multi', commandId: 'constructor', nameOverride: null }]],
      ['set_builtin_feature_favorite', [{ input: { feature: 'find', favorite: true } }]],
      ['open_plugin_panel', [{ input: { commandId: 'default', pluginId: 'com.uipilot.demo-panel', argument: 'hello' } }]],
      ['submit_plugin_panel', [{ input: { sessionEpoch: '7', argument: 'hello', uiIntentEpoch: 1 } }]],
      ['plugin_panel_host_key_enqueue', [{ input: {
        sessionEpoch: '7', clientSequence: '1', declaration: 'ArrowDown', key: 'ArrowDown',
        ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
      } }]],
      ['set_plugin_panel_bounds', [{ input: {
        sessionEpoch: '7', bounds: { x: 12, y: 64, width: 696, height: 320 },
      } }]],
      ['close_plugin_panel', [{ input: { sessionEpoch: '7' } }]],
      ['plugin_panel_focus_host_input_ack', [{ sessionEpoch: '7', focusRequestId: '9', focused: true }]],
      ['select_public_plugin_directory', []],
      ['list_plugins', []],
      ['install_plugin', [{ pluginId: 'internal.math' }]],
      ['reload_plugin', [{ pluginId: 'internal.math' }]],
      ['delete_plugin', [{ pluginId: 'internal.math' }]],
      ['hide_launcher', []],
    ] as const
    expect(tauriCapture.invoke.mock.calls.map(([command, ...args]) => [command, args])).toEqual(invokeRows)
    await pagehide()
  })

  it('fails locally and never listens or loads when native input binding fails', async () => {
    resetAdapterDocument()
    const focusUnlisten = vi.fn()
    tauriCapture.listen.mockResolvedValue(focusUnlisten)
    const originalAdd = HTMLInputElement.prototype.addEventListener
    HTMLInputElement.prototype.addEventListener = function (
      this: HTMLInputElement,
      type: string,
      listener: EventListenerOrEventListenerObject,
      options?: boolean | AddEventListenerOptions,
    ) {
      if (type === 'compositionstart') throw new Error('private native binding failure')
      return originalAdd.call(this, type, listener, options)
    } as typeof originalAdd
    try {
      await act(async () => {
        await import('./main')
      })
      await vi.waitFor(() => expect(document.querySelector('.status-region')?.textContent).toBe('操作不可用，请重试。'))
      expect(document.body.textContent).not.toContain('private')
      expect(tauriCapture.listen).toHaveBeenCalledOnce()
      expect(tauriCapture.listen).toHaveBeenCalledWith('uipilot-plugin-panel-focus-host-input', expect.any(Function))
      await vi.waitFor(() => expect(focusUnlisten).toHaveBeenCalledOnce())
      expect(tauriCapture.invoke).not.toHaveBeenCalled()
    } finally {
      HTMLInputElement.prototype.addEventListener = originalAdd
      await pagehide()
    }
  })

  it('keeps listener failures local and makes zero load calls', async () => {
    resetAdapterDocument()
    tauriCapture.listen.mockRejectedValueOnce(new Error('private listener failure'))
    await act(async () => {
      await import('./main')
    })
    await vi.waitFor(() => expect(document.querySelector('.status-region')?.textContent).toBe('操作不可用，请重试。'))
    expect(document.body.textContent).not.toContain('private')
    expect(tauriCapture.invoke).not.toHaveBeenCalled()
    await pagehide()
  })

  it('shows only fixed local status when React reports a render-phase mount failure', async () => {
    resetAdapterDocument()
    const focusUnlisten = vi.fn()
    tauriCapture.listen.mockResolvedValue(focusUnlisten)
    const privateError = 'private render-phase sentinel'
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    vi.doMock('./launcher-view', () => ({
      LauncherView: () => {
        throw new Error(privateError)
      },
    }))
    try {
      await import('./main')
      await vi.waitFor(() => expect(document.querySelector('.status-region')?.textContent).toBe('操作不可用，请重试。'))
      expect(document.body.textContent).not.toContain(privateError)
      expect(JSON.stringify(consoleError.mock.calls)).not.toContain(privateError)
      expect(tauriCapture.listen).toHaveBeenCalledOnce()
      expect(tauriCapture.listen).toHaveBeenCalledWith('uipilot-plugin-panel-focus-host-input', expect.any(Function))
      expect(focusUnlisten).toHaveBeenCalledOnce()
      expect(tauriCapture.invoke).not.toHaveBeenCalled()
      await pagehide()
      expect(document.querySelector('#app')?.childElementCount).toBe(0)
      await pagehide()
      expect(document.querySelector('#app')?.childElementCount).toBe(0)
    } finally {
      await pagehide()
      vi.doUnmock('./launcher-view')
      vi.resetModules()
      consoleError.mockRestore()
    }
  })

  it('destroys a started core when a later fatal render installs the fixed fallback', async () => {
    resetAdapterDocument()
    const privateError = 'private post-start render sentinel'
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    const shownUnlisten = vi.fn()
    const hiddenUnlisten = vi.fn()
    const builtinFeaturesChangedUnlisten = vi.fn()
    const messageUnlisten = vi.fn()
    const screenshotErrorUnlisten = vi.fn()
    const panelUnlisten = vi.fn()
    const panelResetUnlisten = vi.fn()
    const panelFocusUnlisten = vi.fn()
    const hotkeyRecordingUnlisten = vi.fn()
    const commandHotkeyUnlisten = vi.fn()
    let shownHandler: ((event: { payload: unknown }) => void) | undefined
    let mountedCore: ReturnType<typeof createLauncherCore> | undefined
    let throwFatal = false
    vi.doMock('./launcher-view', async () => {
      const React = await vi.importActual<typeof import('react')>('react')
      return {
        HOTKEY_RECORDING_CURRENT_DOM_EVENT: 'uipilot-hotkey-recording-current',
        LauncherView: ({ core, onReady }: { core: ReturnType<typeof createLauncherCore>; onReady: (result: 'ready') => void }) => {
          mountedCore = core
          const snapshot = React.useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
          React.useLayoutEffect(() => onReady('ready'), [onReady])
          if (throwFatal) throw new Error(privateError)
          return React.createElement('div', null, snapshot.status)
        },
      }
    })
    tauriCapture.listen.mockImplementation(async (event, handler) => {
      if (event === 'launcher://shown') {
        shownHandler = handler as (event: { payload: unknown }) => void
        return shownUnlisten
      }
      if (event === 'launcher://hidden') return hiddenUnlisten
      if (event === 'builtin-features://changed') return builtinFeaturesChangedUnlisten
      if (event === 'uipilot-plugin-panel-status') return () => undefined
      if (event === 'uipilot-plugin-panel-error') return panelUnlisten
      if (event === 'uipilot-plugin-panel-reset') return panelResetUnlisten
      if (event === 'uipilot-plugin-panel-focus-host-input') return panelFocusUnlisten
      if (event === 'hotkey-recording://current') return hotkeyRecordingUnlisten
      if (event === 'command-hotkey://entry') return commandHotkeyUnlisten
      if (event === 'screenshot://error') return screenshotErrorUnlisten
      return messageUnlisten
    })
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(
        command === 'get_message_summary'
          ? { revision: '0', unreadCount: 0 }
          : command === 'load_settings'
            ? emptySettings
            : command === 'search_apps'
              ? null
              : undefined,
      ),
    )
    try {
      await act(async () => {
        await import('./main')
      })
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
      await act(async () => shownHandler?.({ payload: shown('post-start-fatal') }))
      await act(async () => {
        const core = mountedCore!
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
      })
      await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('search_apps', expect.any(Object)))
      const searchCalls = tauriCapture.invoke.mock.calls.filter(([command]) => command === 'search_apps').length

      throwFatal = true
      mountedCore!.failInitialization()
      await vi.waitFor(() => expect(shownUnlisten).toHaveBeenCalledOnce())
      expect(hiddenUnlisten).toHaveBeenCalledOnce()
      expect(builtinFeaturesChangedUnlisten).toHaveBeenCalledOnce()
      expect(messageUnlisten).toHaveBeenCalledOnce()
      expect(screenshotErrorUnlisten).toHaveBeenCalledOnce()
      expect(panelUnlisten).toHaveBeenCalledOnce()
      expect(panelResetUnlisten).toHaveBeenCalledOnce()
      expect(panelFocusUnlisten).toHaveBeenCalledOnce()
      expect(hotkeyRecordingUnlisten).toHaveBeenCalledOnce()
      expect(commandHotkeyUnlisten).toHaveBeenCalledOnce()
      await vi.waitFor(() => expect(document.querySelector('.status-region')?.textContent).toBe('操作不可用，请重试。'))
      expect(document.body.textContent).not.toContain(privateError)
      expect(JSON.stringify(consoleError.mock.calls)).not.toContain(privateError)

      shownHandler?.({ payload: shown('after-fatal') })
      await Promise.resolve()
      expect(tauriCapture.invoke.mock.calls.filter(([command]) => command === 'search_apps')).toHaveLength(searchCalls)
      await pagehide()
      await pagehide()
      expect(shownUnlisten).toHaveBeenCalledOnce()
      expect(hiddenUnlisten).toHaveBeenCalledOnce()
      expect(messageUnlisten).toHaveBeenCalledOnce()
      expect(screenshotErrorUnlisten).toHaveBeenCalledOnce()
      expect(panelUnlisten).toHaveBeenCalledOnce()
      expect(panelResetUnlisten).toHaveBeenCalledOnce()
      expect(panelFocusUnlisten).toHaveBeenCalledOnce()
      expect(hotkeyRecordingUnlisten).toHaveBeenCalledOnce()
    } finally {
      await pagehide()
      vi.doUnmock('./launcher-view')
      vi.resetModules()
      consoleError.mockRestore()
    }
  })

  it('tears down once and keeps the production adapter source narrow', async () => {
    resetAdapterDocument()
    const unlistens = Array.from({ length: 9 }, () => vi.fn())
    tauriCapture.listen.mockImplementation(async (_event, _handler) => unlistens[tauriCapture.listen.mock.calls.length - 1]!)
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(command === 'load_settings' ? emptySettings : undefined),
    )
    await act(async () => {
      await import('./main')
    })
    await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
    const remove = vi.spyOn(HTMLInputElement.prototype, 'removeEventListener')
    await pagehide()
    const removed = remove.mock.calls.length
    for (const unlisten of unlistens) expect(unlisten).toHaveBeenCalledOnce()
    expect(remove.mock.calls.map(([event]) => event)).toEqual(
      expect.arrayContaining(['compositionstart', 'input', 'compositionend']),
    )
    expect(document.querySelector('#app')?.childElementCount).toBe(0)
    await pagehide()
    for (const unlisten of unlistens) expect(unlisten).toHaveBeenCalledOnce()
    expect(remove).toHaveBeenCalledTimes(removed)
    remove.mockRestore()

    for (const command of ['search_apps', 'open_find_window', 'load_settings', 'save_settings', 'save_hotkey', 'hide_launcher']) {
      expect(mainSource.match(new RegExp(`['"]${command}['"]`, 'g'))).toHaveLength(1)
    }
    for (const command of [
      'prepare_find_initialization', 'commit_find_ready', 'get_find_ready_status',
      'search_files', 'load_find_thumbnail', 'set_find_pinned', 'set_find_preview_preference', 'hide_find_window',
    ]) {
      expect(mainSource.match(new RegExp(`['"]${command}['"]`, 'g'))).toHaveLength(1)
    }
    expect(mainSource.match(/['"]execute_result['"]/g)).toHaveLength(2)
    expect(mainSource.match(/['"]plugin_panel_focus_host_input_ack['"]/g)).toHaveLength(1)
    expect(mainSource.match(/['"]launcher:\/\/shown['"]/g)).toHaveLength(1)
    expect(mainSource.match(/['"]launcher:\/\/hidden['"]/g)).toHaveLength(1)
    expect(mainSource.match(/['"]find:\/\/(?:forwarded|theme-changed)['"]/g)).toHaveLength(2)
    expect(mainSource).toContain('getCurrentWindow().label')
    expect(mainSource).not.toContain('.hide(')
    expect(mainSource).not.toMatch(/\b(?:path|pid|hwnd|appId)\b/i)
    expect(mainSource.match(/root\.unmount\(\)/g)).toHaveLength(4)
  })
})

describe('launcher find forwarding ownership', () => {
  it('keeps an ordinary-query find action first and forwards it on Enter', async () => {
    const fake = fakeClient()
    const applications = deferred<SearchResponse | null>()
    vi.mocked(fake.client.searchApps).mockReturnValueOnce(applications.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('find-suggestion'))
    const control = core.getSnapshot().queryControl

    core.text({ kind: 'ordinaryInput', control, value: 'windows', inputType: 'insertText' })
    expect(core.getSnapshot()).toMatchObject({
      query: 'windows',
      selectedIndex: -1,
      searchPending: true,
      results: [],
    })

    applications.resolve({
      requestId: 'application-request',
      items: [findLauncherItem('windows'), { resultId: 'windows-terminal', title: 'Windows Terminal', activation: executeActivation }],
    })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results.map(({ title }) => title)).toEqual(['/find', 'Windows Terminal'])
    expect(core.getSnapshot().selectedIndex).toBe(0)

    const querySequence = core.getSnapshot().querySequence
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
      query: 'windows',
      invocationId: 'find-suggestion',
      querySequence,
    }))
    expect(fake.client.executeResult).not.toHaveBeenCalled()
  })

  it('waits for the ordinary application query ownership before forwarding find', async () => {
    const fake = fakeClient()
    const applications = deferred<SearchResponse | null>()
    vi.mocked(fake.client.searchApps).mockReturnValueOnce(applications.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('find-pending-ownership'))
    const control = core.getSnapshot().queryControl

    core.text({ kind: 'ordinaryInput', control, value: 'windows', inputType: 'insertText' })
    core.keyDown('Enter', false)
    expect(fake.client.openFind).not.toHaveBeenCalled()

    applications.resolve({ requestId: 'application-request', items: [findLauncherItem('windows')] })
    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
      query: 'windows',
      invocationId: 'find-pending-ownership',
      querySequence: core.getSnapshot().querySequence,
    }))
  })

  it('keeps explicit find arguments free of suggestions and establishes ownership only on Enter', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('explicit-windows'))
    const control = core.getSnapshot().queryControl

    core.text({ kind: 'ordinaryInput', control, value: '/find windows', inputType: 'insertText' })
    expect(core.getSnapshot()).toMatchObject({ results: [], selectedIndex: -1, searchPending: false })
    expect(fake.client.searchApps).not.toHaveBeenCalled()

    core.keyDown('Enter', false)
    expect(fake.client.searchApps).toHaveBeenCalledWith({
      query: '/find windows',
      invocationId: 'explicit-windows',
      querySequence: core.getSnapshot().querySequence,
    })
    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
      query: 'windows',
      invocationId: 'explicit-windows',
      querySequence: core.getSnapshot().querySequence,
    }))
    core.destroy()
  })

  it('searches the exact find command and still opens an empty find query on Enter', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      vi.mocked(fake.client.searchApps).mockResolvedValue({
        requestId: 'exact-find',
        items: [findLauncherItem('')],
      })
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('explicit-empty'))
      const control = core.getSnapshot().queryControl

      core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
      await vi.advanceTimersByTimeAsync(150)
      await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith({
        query: '/find',
        invocationId: 'explicit-empty',
        querySequence: core.getSnapshot().querySequence,
        submit: false,
      }))
      await vi.waitFor(() => expect(core.getSnapshot().results.map(({ title }) => title)).toEqual(['/find']))

      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
        query: '',
        invocationId: 'explicit-empty',
        querySequence: core.getSnapshot().querySequence,
      }))
      core.destroy()
    } finally {
      vi.useRealTimers()
    }
  })

  it('clears only an owned forwarded submission and forwards an empty query exactly', async () => {
    for (const [value, query] of [['/find reports', 'reports'], ['/find', '']] as const) {
      const fake = fakeClient()
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown(`forward-${query || 'empty'}`))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value, inputType: 'insertText' })
      const sequence = core.getSnapshot().querySequence
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledWith({
        query,
        invocationId: `forward-${query || 'empty'}`,
        querySequence: sequence,
      }))
      await vi.waitFor(() => expect(core.getSnapshot().query).toBe(''))
      core.destroy()
    }
  })

  it('keeps a superseded submission inert', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.openFind).mockResolvedValueOnce({ status: 'superseded' })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('forward-superseded'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find old', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.openFind).toHaveBeenCalledOnce())
    await Promise.resolve()
    expect(core.getSnapshot().query).toBe('/find old')
    expect(core.getSnapshot().status).toBe('')
  })

  it('does not let late A success or failure mutate edited B', async () => {
    for (const completion of ['success', 'failure'] as const) {
      const fake = fakeClient()
      const pending = deferred<import('./protocol').OpenFindOutcome>()
      vi.mocked(fake.client.openFind).mockReturnValueOnce(pending.promise)
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown(`forward-late-${completion}`))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find A', inputType: 'insertText' })
      core.keyDown('Enter', false)
      core.text({ kind: 'ordinaryInput', control, value: '/find B', inputType: 'insertText' })
      if (completion === 'success') pending.resolve({ status: 'forwarded' })
      else pending.reject(new Error('window unavailable'))
      await pending.promise.catch(() => undefined)
      await Promise.resolve()
      expect(core.getSnapshot().query).toBe('/find B')
      expect(core.getSnapshot().status).toBe('')
    }
  })

  it('reports the fixed failure only to its current owner', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.openFind).mockRejectedValueOnce(new Error('private backend detail'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('forward-failure'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe('文件搜索窗口暂不可用。'))
    expect(core.getSnapshot().query).toBe('/find report')
  })
})

describe('file protocol', () => {
  it('strictly parses exact file search responses', () => {
    const response = fileResponse('18446744073709551615', [fileItem()])
    expect(parseFileSearchResponse(response)).toEqual(response)
  })

  it('rejects malformed file search responses as a whole', () => {
    const valid = fileResponse('7')
    const invalid: unknown[] = [
      { ...valid, extra: true },
      { requestId: valid.requestId, indexRevision: valid.indexRevision, total: valid.total, status: valid.status },
      { ...valid, indexRevision: '01' },
      { ...valid, indexRevision: '18446744073709551616' },
      { ...valid, total: '-1' },
      { ...valid, status: 'unknown' },
      { ...valid, items: [{ ...valid.items[0], sizeBytes: '01' }] },
      { ...valid, items: [{ ...valid.items[0], canOpenDocument: 'yes' }] },
      { ...valid, items: [{ ...valid.items[0], canOpenDocument: true, kind: 'folder', sizeBytes: null }] },
      { ...valid, items: [{ ...valid.items[0], kind: 'directory' }] },
      { ...valid, items: [{ ...valid.items[0], modifiedUtc: '2026-07-22' }] },
      Object.assign(Object.create({ inherited: true }), valid),
      [valid],
    ]
    for (const value of invalid) expect(parseFileSearchResponse(value)).toBeNull()
  })
})
describe.skip('retired embedded file adapter', () => {
  it('uses one shown listener and exact camelCase invoke payloads', async () => {
    vi.resetModules()
    document.body.innerHTML = '<main id="app"></main>'
    installMatchMedia(false)
    tauriCapture.invoke.mockReset()
    tauriCapture.listen.mockReset()
    const shownUnlisten = vi.fn()
    tauriCapture.listen.mockResolvedValue(shownUnlisten)
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(command === 'load_settings' ? emptySettings : command === 'search_files' ? null : undefined),
    )

    const main = (await import('./main')) as unknown as { findClient: FindClient }
    await main.findClient.searchFiles({
      query: 'UiPilot', category: 'all', sort: 'modifiedDesc', invocationId: 'inv-file', querySequence: 2,
      privateExtra: 'must-not-cross-wire',
    } as Parameters<FindClient['searchFiles']>[0])

    expect(tauriCapture.listen).not.toHaveBeenCalled()
    expect(tauriCapture.invoke).toHaveBeenCalledWith('search_files', {
      query: 'UiPilot', category: 'all', sort: 'modifiedDesc', invocationId: 'inv-file', querySequence: 2,
    })
    window.dispatchEvent(new Event('pagehide'))
  })

  it('keeps exactly eight commands and one launcher event', () => {
    for (const command of ['search_apps', 'search_files', 'execute_result', 'load_settings', 'save_settings', 'save_hotkey', 'set_file_preview_preference', 'hide_launcher']) {
      expect(mainSource.match(new RegExp(`['"]${command}['"]`, 'g'))).toHaveLength(1)
    }
    expect(mainSource.match(/['"]launcher:\/\/shown['"]/g)).toHaveLength(1)
    expect(mainSource).not.toMatch(/file-index/)
    expect(mainSource).not.toMatch(/@tauri-apps\/api\/(?:window|webviewWindow)/)
    expect(mainSource).not.toMatch(/console\.|JSON\.stringify\(event/)
  })
})
describe.skip('retired embedded file mode ownership', () => {
  it('searches a nonempty find query immediately without a file-index listener', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('find-immediate'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(1))
    expect(fake.client.searchFiles).toHaveBeenCalledWith({ query: 'report', category: 'all', sort: 'modifiedDesc', invocationId: 'find-immediate', querySequence: 2 })
  })

  it('does not search an empty find query and starts on the next nonempty edit', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('find-empty'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await Promise.resolve()
    expect(fake.client.searchFiles).not.toHaveBeenCalled()
    core.text({ kind: 'ordinaryInput', control, value: 'a', inputType: 'insertText' })
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(1))
  })

  it('reports a building index without promising automatic result updates', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1', [], 'building'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('file-building'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.indexStatus).toBe('building'))
    expect(core.getSnapshot().status).toBe('正在索引。')
    expect(core.getSnapshot().status).not.toContain('持续更新')
    core.destroy()
  })

  it('marks a rejected current file search unavailable and keeps Enter inert', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockRejectedValueOnce({ code: 'searchUnavailable', message: 'private backend text' })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('file-search-rejected'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)

    await vi.waitFor(() => expect(core.getSnapshot().file?.indexStatus).toBe('unavailable'))
    expect(core.getSnapshot().status).toBe('搜索暂不可用。')
    expect(core.getSnapshot().file?.results).toEqual([])

    core.keyDown('Enter', false)
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(core.getSnapshot().status).toBe('搜索暂不可用。')
    core.destroy()
  })

  it('starts no streaming or revision refresh timer', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('find-no-timer'))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.runAllTicks()
      await vi.advanceTimersByTimeAsync(10_000)
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
      core.destroy()
    } finally {
      vi.useRealTimers()
    }
  })

  it('ignores stale late file responses after a newer edit', async () => {
    const fake = fakeClient()
    const stale = deferred<FileSearchResponse | null>()
    const current = deferred<FileSearchResponse | null>()
    vi.mocked(fake.client.searchFiles).mockReturnValueOnce(stale.promise).mockReturnValueOnce(current.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('file-stale'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find first', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledOnce())
    core.text({ kind: 'ordinaryInput', control, value: 'second', inputType: 'insertText' })
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(2))
    current.resolve(fileResponse('2', [fileItem(String.raw`C:\Private\Current.txt`, 'current')]))
    await current.promise
    await vi.waitFor(() => expect(core.getSnapshot().file?.results[0]?.name).toBe('Current.txt'))
    stale.resolve(fileResponse('1', [fileItem(String.raw`C:\Private\Stale.txt`, 'stale')]))
    await stale.promise
    await Promise.resolve()
    expect(core.getSnapshot().file?.results[0]?.name).toBe('Current.txt')
  })

  it('keeps unavailable status and opaque Enter execution', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({ requestId: 'req-0000000000000001', indexRevision: '1', total: '1', status: 'unavailable', items: [{ ...fileItem(String.raw`C:\Private\Report.txt`, 'res-0000000000000001') }] })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('file-enter'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.selected).toBeDefined())
    expect(core.getSnapshot().file?.indexStatus).toBe('unavailable')
    core.keyDown('Enter', false)
    expect(fake.client.executeResult).toHaveBeenCalledWith({ requestId: 'req-0000000000000001', resultId: 'res-0000000000000001' })
  })

  it('requeries a nonempty keyword when the file category changes', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('category-search'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file).toBeDefined())

    core.setFileCategory('pdf')
    expect(fake.client.searchFiles).toHaveBeenCalledTimes(2)
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(
      expect.objectContaining({ query: 'report', category: 'pdf', sort: 'modifiedDesc' }),
    )
    expect(core.getSnapshot().file).toMatchObject({ category: 'pdf', total: '0', results: [] })
  })

  it('cycles categories in both directions and wraps', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('category-cycle'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file).toBeDefined())

    core.cycleFileCategory('previous')
    expect(core.getSnapshot().file?.category).toBe('archive')
    core.cycleFileCategory('next')
    expect(core.getSnapshot().file?.category).toBe('all')
    expect(fake.client.searchFiles).toHaveBeenCalledTimes(3)
  })
})
describe.skip('retired embedded file panel accessibility', () => {
  it('renders results and preview without controls or private result ids', async () => {
    installMatchMedia(false)
    const first = fileItem(String.raw`C:\Private\Quarterly Report.pdf`, 'secret-file-id')
    const second = folderItem(String.raw`C:\Private\Reports`, 'secret-folder-id')
    const { core, mounted, client } = await startedFileView([first, second])
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(mounted.host.querySelector('.file-workspace')).toBeTruthy()
    expect(mounted.host.querySelector('[role="tablist"]')).toBeNull()
    expect([...mounted.host.querySelectorAll('button')].some((button) => /修改时间/.test(button.textContent ?? ''))).toBe(false)
    expect(input.getAttribute('aria-controls')).toBe('file-results')
    expect(mounted.host.querySelector('[aria-label="文件预览"]')?.textContent).toContain('完整路径')
    const options = [...mounted.host.querySelectorAll<HTMLElement>('#file-results [role="option"]')]
    expect(options).toHaveLength(2)
    expect(mounted.host.innerHTML).not.toContain('secret-file-id')
    await act(async () => options[1]!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'file-request-1', resultId: 'secret-folder-id' })
    expect(core.getSnapshot().file?.selected?.fullPath).toBe(String.raw`C:\Private\Reports`)
    await mounted.unmount()
  })

  it('keeps the query input as the only result focus owner', async () => {
    installMatchMedia(false)
    const { mounted } = await startedFileView([fileItem(String.raw`C:\Private\A.txt`, 'a'), fileItem(String.raw`C:\Private\B.txt`, 'b')])
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    expect(document.activeElement).toBe(input)
    expect(input.getAttribute('aria-activedescendant')).toBe('file-result-option-1')
    await mounted.unmount()
  })
})
describe.skip('retired embedded file category navigation', () => {
  it('renders categories and cycles them from the file query input', async () => {
    installMatchMedia(false)
    const { core, mounted } = await startedFileView()
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect([...mounted.host.querySelectorAll('.file-category')].map((button) => button.textContent)).toEqual([
      '全部',
      '文件夹',
      'Excel',
      'Word',
      'PPT',
      'PDF',
      '图片',
      '视频',
      '音频',
      '压缩包',
    ])

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true })))
    expect(core.getSnapshot().file?.category).toBe('folder')
    expect(document.activeElement).toBe(input)

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true })))
    expect(core.getSnapshot().file?.category).toBe('all')
    expect(document.activeElement).toBe(input)
    await mounted.unmount()
  })
})
describe.skip('retired embedded file panel responsive layout', () => {
  it('keeps the file UI in one scoped responsive surface without extra component families', () => {
    expect(launcherViewSource).toContain('className="file-workspace"')
    expect(launcherViewSource).toContain("import {")
    expect(launcherViewSource).not.toContain('@ant-design/icons')
    const antdImport = launcherViewSource.slice(0, launcherViewSource.indexOf("} from 'antd'"))
    for (const forbidden of ['AutoComplete', 'Card', 'Modal']) {
      expect(antdImport).not.toContain(forbidden)
    }
    const filePanelSource = launcherViewSource.slice(
      launcherViewSource.indexOf('const filePanel'),
      launcherViewSource.indexOf('const settings = snapshot.settings'),
    )
    expect(filePanelSource).not.toContain('<Select')
    expect(stylesSource).toContain('.file-workspace')
    expect(stylesSource).toContain('grid-template-areas')
    expect(stylesSource).toContain('categories results preview')
    expect(stylesSource).toContain('.file-categories')
    expect(stylesSource).toContain('.file-preview')
    expect(stylesSource).toContain('@media (max-width: 600px)')
    expect(stylesSource).toContain('@media (forced-colors: active)')
    expect(stylesSource).toContain('overflow-wrap: anywhere')
    expect(stylesSource).toContain('overflow: hidden')
    expect(stylesSource).toContain('.file-workspace .ant-spin-container')
  })
})

describe.skip('retired embedded file preview preference', () => {
  it('renders the preview switch as the single preference control and rolls pending state through the core', async () => {
    installMatchMedia(false)
    const pending = deferred<void>()
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    vi.mocked(fake.client.setFilePreviewPreference).mockReturnValueOnce(pending.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('file-preview')))
    const control = core.getSnapshot().queryControl
    await act(async () => core.text({ kind: 'ordinaryInput', control, value: '/find preview', inputType: 'insertText' }))
    await act(async () => core.keyDown('Enter', false))
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(true))

    const preview = mounted.host.querySelector<HTMLElement>('.file-preview')!
    expect(preview.textContent).toContain('UiPilot.txt')
    expect(preview.textContent).toContain('42')
    const setting = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="设置暂不可用"]')!
    expect(setting.disabled).toBe(true)
    const checkbox = mounted.host.querySelector<HTMLInputElement>('[role="switch"][aria-label="文件预览"]')!
    await act(async () => checkbox.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    expect(fake.client.setFilePreviewPreference).toHaveBeenCalledWith({ preference: { enabled: false } })
    expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: true })
    pending.resolve()
    await pending.promise
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: false }),
    )
    await mounted.unmount()
  })
})

describe('credential consent and settings', () => {
  const credential = { id: 'service-token', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' as const }
  const item = {
    pluginId: 'com.example.credentials', name: 'Credentials', description: null, version: '1.0.0',
    source: 'localPackage' as const, installationDirectory: null, commands: [{ commandId: 'default', defaultName: 'credentials', effectiveName: 'credentials', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true,
    fault: null, generation: 1, iconUrl: null,
    network: { httpsHosts: ['api.example.com'], credentials: [credential] },
    permissions: [{ permission: 'network.https' as const, supported: true, granted: false }],
    settings: [
      { definition: { type: 'secret' as const, key: 'api-token', label: 'Current API token' }, secretConfigured: false },
      { definition: { type: 'secret' as const, key: 'other-token', label: 'Other token' }, secretConfigured: true },
    ],
  }
  async function setup(dark = false, plugin = item) {
    const computedStyle = window.getComputedStyle
    const styleSpy = vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => computedStyle(element))
    installMatchMedia(dark)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, theme: dark ? 'dark' : 'light' })
    vi.mocked(fake.client.listPublicPlugins).mockResolvedValue({ revision: '41', hotkeyRevision: '0', items: [plugin] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('credentials-ui', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    const open = async () => {
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="查看插件详情"]')!.click())
    }
    return { ...fake, ...mounted, core, open, async cleanup() { await mounted.unmount(); core.destroy(); styleSpy.mockRestore() } }
  }
  const button = (label: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
  async function enter(input: HTMLInputElement, value: string) {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
  }

  it('blocks command favorite while saving a secret and releases package busy state afterward', async () => {
    const ui = await setup()
    try {
      await ui.open()
      const pending = deferred<void>()
      vi.mocked(ui.client.savePublicPluginSettings).mockReturnValueOnce(pending.promise)
      const secret = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      await enter(secret, 'synthetic-value')
      await act(async () => button('保存 Current API token').click())
      expect(secret.readOnly).toBe(true)
      const favorite = button('设为常用')
      expect(favorite.matches(':disabled')).toBe(true)
      await act(async () => favorite.click())
      expect(ui.client.setPublicPluginFavorite).not.toHaveBeenCalled()
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '42', hotkeyRevision: '0', items: [item] })
      await act(async () => pending.resolve())
      await vi.waitFor(() => expect(secret.readOnly).toBe(false))
      expect(ui.host.querySelector('.public-plugin-detail')?.getAttribute('aria-busy')).toBe('false')
      expect(favorite.matches(':disabled')).toBe(false)
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '43', hotkeyRevision: '0', items: [{ ...item, commands: item.commands.map((command) => ({ ...command, favorite: true })) }] })
      await act(async () => favorite.click())
      expect(ui.client.setPublicPluginFavorite).toHaveBeenCalledExactlyOnceWith({ pluginId: item.pluginId, commandId: 'default', favorite: true })
      await vi.waitFor(() => expect(button('取消常用')).not.toBeNull())
      expect(secret.readOnly).toBe(false)
    } finally { await ui.cleanup() }
  })
  it('shows candidate binding keys and change markers; cancel preserves the installed version', async () => {
    const ui = await setup()
    try {
      vi.mocked(ui.client.selectPublicPluginDirectory).mockResolvedValue('synthetic-candidate')
      vi.mocked(ui.client.preparePublicPlugin).mockResolvedValue({ token: 'candidate', pluginId: item.pluginId, name: item.name, version: '2.0.0', permissions: ['network.https'], isUpdate: true, operation: 'replace', previousVersion: '1.0.0', sourceVerified: false, iconUrl: null,
        network: { ...item.network, credentials: [{ ...credential, settingKey: 'candidate-token' }], addedHttpsHosts: [], addedCredentialIds: ['service-token'], requiresNetworkConsent: true } })
      await act(async () => button('选择开发目录').click())
      const consent = ui.host.querySelector('.public-network-consent')!
      expect(consent.textContent).toContain('candidate-token')
      expect(consent.textContent).toContain('service-token')
      expect(consent.textContent).toContain('Bearer')
      expect(consent.textContent).toContain('新增或变更')
      expect(consent.textContent).toContain('此凭据会发送给该服务；服务响应可能包含其回显。')
      expect(consent.textContent).not.toContain('Current API token')
      await act(async () => button('取消安装').click())
      expect(ui.client.commitPublicPlugin).not.toHaveBeenCalled()
      await ui.open()
      expect(ui.host.querySelector('.public-plugin-detail')?.textContent).toContain('1.0.0')
    } finally { await ui.cleanup() }
  })

  it.each([false, true])('renders declared bindings and empty password controls with accessible configuration status, dark=%s', async (dark) => {
    const ui = await setup(dark)
    try {
      await ui.open()
      expect(ui.host.textContent).toContain('service-token')
      expect(ui.host.textContent).toContain('Current API token')
      expect(ui.host.textContent).toContain('api-token')
      expect(ui.host.textContent).toContain('Bearer')
      expect(ui.host.textContent).toContain('不代表可用于 Bearer 请求')
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      expect(input?.type).toBe('password')
      expect(input.value).toBe('')
      expect(input.getAttribute('aria-describedby')).toBeTruthy()
      expect(ui.host.textContent).not.toMatch(/\*{3}|•••/)
      expect(button('保存 Current API token').disabled).toBe(true)
      await enter(input, '   ')
      expect(button('保存 Current API token').disabled).toBe(true)
    } finally { await ui.cleanup() }
  })

  it('requires explicit network consent, passes displayed revision, refreshes stale errors without retry and disables directly', async () => {
    const ui = await setup()
    try {
      await ui.open()
      await act(async () => button('网络访问').click())
      expect(ui.client.setPublicPluginNetworkAccess).not.toHaveBeenCalled()
      expect(document.querySelector('[role="dialog"]')?.textContent).toContain('service-token')
      vi.mocked(ui.client.setPublicPluginNetworkAccess).mockRejectedValueOnce({ code: 'staleInventory', message: 'private detail' })
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '42', hotkeyRevision: '0', items: [{ ...item, network: { ...item.network, credentials: [{ ...credential, host: 'new.example.com' }] } }] })
      await act(async () => button('确认开启网络访问').click())
      expect(ui.client.setPublicPluginNetworkAccess).toHaveBeenCalledExactlyOnceWith({ pluginId: item.pluginId, granted: true, expectedInventoryRevision: '41' })
      expect(ui.host.textContent).toContain('new.example.com')
      expect(ui.host.querySelector('[role="alert"]')).not.toBeNull()
      expect(ui.host.textContent).not.toContain('private detail')
      expect(document.querySelector('[role="dialog"]')).toBeNull()
      await act(async () => button('网络访问').click())
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '43', hotkeyRevision: '0', items: [{ ...item, permissions: [{ permission: 'network.https', supported: true, granted: true }] }] })
      await act(async () => button('确认开启网络访问').click())
      expect(ui.client.setPublicPluginNetworkAccess).toHaveBeenLastCalledWith({ pluginId: item.pluginId, granted: true, expectedInventoryRevision: '42' })
      await act(async () => button('网络访问').click())
      expect(ui.client.setPublicPluginNetworkAccess).toHaveBeenLastCalledWith({ pluginId: item.pluginId, granted: false })
      expect(document.querySelector('[role="dialog"]')).toBeNull()
    } finally { await ui.cleanup() }
  })

  it('supports the legal constructor secret key through draft, save and clear without affecting another draft', async () => {
    const ui = await setup(false, { ...item, settings: [
      { definition: { type: 'secret', key: 'constructor', label: 'Constructor token' }, secretConfigured: false },
      item.settings[1]!,
    ] })
    try {
      await ui.open()
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Constructor token"]')!
      const other = ui.host.querySelector<HTMLInputElement>('input[aria-label="Other token"]')!
      expect(input.value).toBe('')
      expect(button('保存 Constructor token').disabled).toBe(true)
      await enter(other, 'other-unsaved-draft')
      await enter(input, 'synthetic-constructor-token')
      await act(async () => button('保存 Constructor token').click())
      expect(ui.client.savePublicPluginSettings).toHaveBeenLastCalledWith({ input: { pluginId: item.pluginId, settings: {}, secrets: { constructor: 'synthetic-constructor-token' } } })
      expect(input.value).toBe('')
      expect(other.value).toBe('other-unsaved-draft')
      await enter(input, 'unsaved-constructor-draft')
      await act(async () => button('清除 Constructor token').click())
      await act(async () => button('确认清除 Constructor token').click())
      expect(ui.client.savePublicPluginSettings).toHaveBeenLastCalledWith({ input: { pluginId: item.pluginId, settings: {}, secrets: { constructor: null } } })
      expect(input.value).toBe('')
      expect(other.value).toBe('other-unsaved-draft')
    } finally { await ui.cleanup() }
  })

  it.each(['network success', 'network failure', 'clear success', 'clear failure'] as const)('restores the confirmation origin after the dialog closes and mutation settles: %s', async (scenario) => {
    const ui = await setup()
    try {
      await ui.open()
      const network = scenario.startsWith('network')
      const origin = button(network ? '网络访问' : '清除 Other token')
      const pending = deferred<void>()
      if (network) vi.mocked(ui.client.setPublicPluginNetworkAccess).mockReturnValueOnce(pending.promise)
      else vi.mocked(ui.client.savePublicPluginSettings).mockReturnValueOnce(pending.promise)
      await act(async () => { origin.focus(); origin.click() })
      const confirm = button(network ? '确认开启网络访问' : '确认清除 Other token')
      await act(async () => { confirm.focus(); confirm.click() })
      expect(origin.disabled).toBe(true)
      await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
      // Model native blur when the focused modal disappears; jsdom alone does not enforce disabled-focus behavior.
      await act(async () => (document.activeElement as HTMLElement).blur())
      expect(document.activeElement).toBe(document.body)
      await act(async () => scenario.endsWith('failure') ? pending.reject(new Error('synthetic failure')) : pending.resolve())
      expect(origin.disabled).toBe(false)
      expect(document.activeElement).toBe(origin)
    } finally { await ui.cleanup() }
  })

  it.each([false, true])('does not restore an operation origin after a deliberate focus move, then blur=%s', async (blur) => {
    const ui = await setup()
    try {
      await ui.open()
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      const other = ui.host.querySelector<HTMLInputElement>('input[aria-label="Other token"]')!
      await enter(input, 'synthetic-token')
      const pending = deferred<void>()
      vi.mocked(ui.client.savePublicPluginSettings).mockReturnValueOnce(pending.promise)
      await act(async () => { button('保存 Current API token').focus(); button('保存 Current API token').click() })
      expect(other.matches(':disabled')).toBe(true)
      const pendingFocus = document.activeElement
      await act(async () => other.focus())
      expect(document.activeElement).toBe(pendingFocus)
      const back = button('返回插件列表')
      expect(back.matches(':disabled')).toBe(false)
      await act(async () => back.focus())
      expect(document.activeElement).toBe(back)
      if (blur) await act(async () => back.blur())
      expect(document.activeElement).toBe(blur ? document.body : back)
      await act(async () => pending.resolve())
      expect(document.activeElement).toBe(blur ? document.body : back)
    } finally { await ui.cleanup() }
  })

  it('returns successful button save to its password input even when the mutation settles immediately', async () => {
    const ui = await setup()
    try {
      await ui.open()
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      await enter(input, 'synthetic-token')
      const save = button('保存 Current API token')
      await act(async () => { save.focus(); save.click() })
      expect(save.disabled).toBe(true)
      expect(document.activeElement).toBe(input)
    } finally { await ui.cleanup() }
  })

  it('saves only the edited key, prevents duplicate saves, clears successful input and keeps focus on refresh', async () => {
    const ui = await setup()
    try {
      await ui.open()
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      const other = ui.host.querySelector<HTMLInputElement>('input[aria-label="Other token"]')!
      await enter(other, 'other-unsaved-draft')
      await enter(input, 'synthetic-token')
      const pending = deferred<void>()
      vi.mocked(ui.client.savePublicPluginSettings).mockReturnValueOnce(pending.promise)
      const save = button('保存 Current API token')
      await act(async () => { save.focus(); save.click(); save.click() })
      expect(ui.client.savePublicPluginSettings).toHaveBeenCalledExactlyOnceWith({ input: { pluginId: item.pluginId, settings: {}, secrets: { 'api-token': 'synthetic-token' } } })
      expect(ui.host.querySelector('.public-plugin-detail')?.getAttribute('aria-busy')).toBe('true')
      expect(input.readOnly).toBe(true)
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '42', hotkeyRevision: '0', items: [{ ...item, settings: item.settings.map((setting) => ({ ...setting, secretConfigured: true })) }] })
      await act(async () => pending.resolve())
      expect(input.value).toBe('')
      expect(other.value).toBe('other-unsaved-draft')
      expect(ui.host.textContent).toContain('已保存')
      expect(save.disabled).toBe(true)
      expect(document.activeElement).toBe(input)
      expect(ui.host.textContent).not.toContain('synthetic-token')
    } finally { await ui.cleanup() }
  })

  it('clears explicitly with null and refreshes partial failures without claiming rollback or permanent revocation', async () => {
    const ui = await setup()
    try {
      await ui.open()
      vi.mocked(ui.client.savePublicPluginSettings).mockRejectedValueOnce(new Error('private token'))
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '42', hotkeyRevision: '0', items: [{ ...item, settings: item.settings.map((setting) => ({ ...setting, secretConfigured: false })) }] })
      await act(async () => button('清除 Other token').click())
      expect(ui.client.savePublicPluginSettings).not.toHaveBeenCalled()
      await act(async () => button('确认清除 Other token').click())
      expect(ui.client.savePublicPluginSettings).toHaveBeenCalledExactlyOnceWith({ input: { pluginId: item.pluginId, settings: {}, secrets: { 'other-token': null } } })
      expect(ui.client.listPublicPlugins).toHaveBeenCalledTimes(2)
      expect(ui.host.textContent).toContain('落盘值可能已改变')
      expect(ui.host.textContent).toContain('后续恢复可能使用实际落盘值')
      expect(ui.host.textContent).not.toContain('private token')
      expect(button('清除 Other token').disabled).toBe(false)
    } finally { await ui.cleanup() }
  })
  it('allows explicit cleanup when configuration is absent, and cancels keyboard consent without closing details', async () => {
    const ui = await setup()
    try {
      await ui.open()
      expect(button('清除 Current API token').disabled).toBe(false)
      await act(async () => button('清除 Current API token').click())
      await act(async () => button('确认清除 Current API token').click())
      expect(ui.client.savePublicPluginSettings).toHaveBeenCalledExactlyOnceWith({ input: { pluginId: item.pluginId, settings: {}, secrets: { 'api-token': null } } })
      expect(ui.host.textContent).toContain('已清除')
      const toggle = button('网络访问')
      await act(async () => { toggle.focus(); toggle.click() })
      const modal = document.querySelector<HTMLElement>('.ant-modal-wrap')!
      await act(async () => modal.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', keyCode: 27, bubbles: true })))
      expect(ui.host.querySelector('.public-plugin-detail')).not.toBeNull()
      expect(ui.client.setPublicPluginNetworkAccess).not.toHaveBeenCalled()
      expect(document.activeElement).toBe(toggle)
    } finally { await ui.cleanup() }
  })

  it.each(['network cancel', 'network Escape', 'clear cancel', 'clear Escape'] as const)('returns to the correct confirmation origin without a mutation: %s', async (scenario) => {
    const ui = await setup()
    try {
      await ui.open()
      const origin = button(scenario.startsWith('network') ? '网络访问' : '清除 Other token')
      await act(async () => { origin.focus(); origin.click() })
      const modal = document.querySelector<HTMLElement>('.ant-modal-wrap')!
      if (scenario.endsWith('Escape')) {
        await act(async () => modal.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', keyCode: 27, bubbles: true })))
      } else {
        const cancel = Array.from(modal.querySelectorAll<HTMLButtonElement>('button')).find((candidate) => candidate.textContent?.replace(/\s/g, '') === '取消')!
        await act(async () => { cancel.focus(); cancel.click() })
      }
      expect(document.querySelector('[role="dialog"]')).toBeNull()
      expect(ui.host.querySelector('.public-plugin-detail')).not.toBeNull()
      expect(ui.client.setPublicPluginNetworkAccess).not.toHaveBeenCalled()
      expect(ui.client.savePublicPluginSettings).not.toHaveBeenCalled()
      expect(document.activeElement).toBe(origin)
    } finally { await ui.cleanup() }
  })

  it('also sends the displayed global revision when enabling a network plugin without credentials', async () => {
    const ui = await setup()
    try {
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '57', hotkeyRevision: '0', items: [{ ...item, network: { ...item.network, credentials: [] } }] })
      await act(async () => button('刷新').click())
      await ui.open()
      await act(async () => button('网络访问').click())
      await act(async () => button('确认开启网络访问').click())
      expect(ui.client.setPublicPluginNetworkAccess).toHaveBeenCalledExactlyOnceWith({ pluginId: item.pluginId, granted: true, expectedInventoryRevision: '57' })
    } finally { await ui.cleanup() }
  })

  it('refreshes a failed password save, keeps unsaved input and does not retry', async () => {
    const ui = await setup()
    try {
      await ui.open()
      const input = ui.host.querySelector<HTMLInputElement>('input[aria-label="Current API token"]')!
      await enter(input, 'synthetic-failed-draft')
      vi.mocked(ui.client.savePublicPluginSettings).mockRejectedValueOnce(new Error('synthetic-failed-draft'))
      vi.mocked(ui.client.listPublicPlugins).mockResolvedValue({ revision: '42', hotkeyRevision: '0', items: [{ ...item, settings: item.settings.map((setting) => ({ ...setting, secretConfigured: true })) }] })
      await act(async () => input.closest('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
      expect(ui.client.savePublicPluginSettings).toHaveBeenCalledOnce()
      expect(ui.client.listPublicPlugins).toHaveBeenCalledTimes(2)
      expect(input.value).toBe('synthetic-failed-draft')
      expect(ui.host.textContent).not.toContain('synthetic-failed-draft')
      expect(ui.host.textContent).not.toContain('已保存')
      expect(ui.host.querySelector(`#${input.getAttribute('aria-describedby')}`)?.textContent).toBe('已配置')
      expect(ui.host.querySelector('.public-plugin-detail')?.getAttribute('aria-busy')).toBe('false')
    } finally { await ui.cleanup() }
  })

})


it('requires a stable command ID for main-result activation and preserves constructor', () => {
  const activation = { kind: 'mainResultActivation', pluginId: 'com.example.multi', commandLabel: 'upper', initialArgument: '', favorite: false }
  for (const commandId of [undefined, null, '', 'Upper', 'with space']) {
    expect(safeLauncherActivation({ ...activation, ...(commandId === undefined ? {} : { commandId }) })).toBeUndefined()
  }
  expect(safeLauncherActivation({ ...activation, commandId: 'constructor' })).toEqual({ ...activation, commandId: 'constructor' })
})


it('preserves constructor command identity and rejects sibling favorite and command contexts', async () => {
  const { core, client, emit } = await startedCore()
  let responseCommandId: unknown = "upper"
  const activation = { kind: 'mainResultActivation' as const, pluginId: 'com.example.multi', commandId: 'constructor', commandLabel: 'lower', initialArgument: 'saved  input', favorite: false }
  vi.mocked(client.searchApps).mockImplementation(async (request) => request.submit ? {
    requestId: 'submit', items: [{ resultId: 'same', title: 'Sibling output', activation: executeActivation }],
    mainResultCommand: { pluginId: activation.pluginId, ...(responseCommandId === undefined ? {} : { commandId: responseCommandId }), commandLabel: 'lower', argument: request.query.slice('/lower '.length) },
  } as unknown as SearchResponse : request.query === 'multi' ? {
    requestId: 'catalog', items: [
      { resultId: 'sibling', title: 'Wrong sibling favorite', activation, favorite: { target: { kind: 'publicPlugin', pluginId: activation.pluginId, commandId: 'upper' }, favorite: false } },
      { resultId: 'constructor', title: 'Package /lower', activation, favorite: { target: { kind: 'publicPlugin', pluginId: activation.pluginId, commandId: 'constructor' }, favorite: false } },
    ],
  } : { requestId: 'hint', items: [], commandHint: 'Text' })
  emit(shown('multi-command-identity'))
  await new Promise((resolve) => setTimeout(resolve, 0))
  core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'multi', inputType: 'insertText' })
  await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
  expect(core.getSnapshot().results[0]?.title).toBe('Package /lower')
  core.keyDown('Enter', false)
  await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand?.commandId).toBe('constructor'))
  expect(Object.isFrozen(core.getSnapshot().mainResultCommand)).toBe(true)
  core.keyDown('Enter', false)
  await vi.waitFor(() => expect(client.searchApps).toHaveBeenCalledWith(expect.objectContaining({ submit: true })))
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(core.getSnapshot().mainResultCommand?.commandId).toBe('constructor')
  expect(core.getSnapshot().results).toHaveLength(0)
  for (const invalid of [undefined, null, '', 'Upper', 'with space', 'unknown']) {
    responseCommandId = invalid
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: `rejected-${String(invalid)}`, inputType: 'insertText' })
    core.keyDown('Enter', false)
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(core.getSnapshot().mainResultCommand?.commandId).toBe('constructor')
    expect(core.getSnapshot().results).toHaveLength(0)
  }
  responseCommandId = 'constructor'
  core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'retry  input', inputType: 'insertText' })
  core.keyDown('Enter', false)
  await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
  expect(core.getSnapshot().mainResultCommand).toMatchObject({ commandId: 'constructor', suffix: 'retry  input' })
})


describe('independent command management', () => {
  async function setup(dark = false, count = 2) {
    installMatchMedia(dark)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, theme: dark ? 'dark' : 'light' })
    let inventory = { revision: '10', hotkeyRevision: '0', items: [{
      pluginId: 'com.example.multi', name: 'Multi', description: null, version: '1.0.0', source: 'localPackage' as const, installationDirectory: null,
      commands: [
        { commandId: 'upper', defaultName: 'upper', effectiveName: 'upper', nameOverride: null as string | null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const },
        { commandId: 'constructor', defaultName: 'lower', effectiveName: 'lower', nameOverride: null as string | null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const },
        ...Array.from({ length: Math.max(0, count - 2) }, (_, index) => ({ commandId: `command-${index + 3}`, defaultName: `long-command-name-number-${index + 3}`, effectiveName: `long-command-name-number-${index + 3}`, nameOverride: null as string | null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const })),
      ], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [],
    }] }
    vi.mocked(fake.client.listPublicPlugins).mockImplementation(async () => structuredClone(inventory))
    const update = (commandId: string, values: Partial<typeof inventory.items[number]['commands'][number]>) => {
      inventory = { ...inventory, revision: String(Number(inventory.revision) + 1), hotkeyRevision: '0', items: inventory.items.map((plugin) => ({ ...plugin,
        commands: plugin.commands.map((command) => command.commandId === commandId ? { ...command, ...values } : command),
      })) }
    }
    vi.mocked(fake.client.setPublicPluginEffectiveName).mockImplementation(async ({ commandId, nameOverride }) => {
      update(commandId, { nameOverride, effectiveName: nameOverride ?? (commandId === 'upper' ? 'upper' : 'lower') })
    })
    vi.mocked(fake.client.setPublicPluginFavorite).mockImplementation(async ({ commandId, favorite }) => update(commandId, { favorite }))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('command-ui', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    const open = async () => { await act(async () => mounted.host.querySelector<HTMLButtonElement>('button[aria-label="查看插件详情"]')!.click()) }
    await open()
    return { ...fake, ...mounted, core, open, update, remove: () => { inventory = { ...inventory, revision: String(Number(inventory.revision) + 1), items: [] } }, snapshot: () => structuredClone(inventory), async cleanup() { await mounted.unmount(); core.destroy() } }
  }
  const button = (host: HTMLElement, label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
  const input = (host: HTMLElement, id: string) => host.querySelector<HTMLInputElement>(`input[aria-label="启动键 ${id}"]`)!
  const enter = async (element: HTMLInputElement, value: string) => {
    await act(async () => {
      element.focus()
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(element, value)
      element.dispatchEvent(new Event('input', { bubbles: true }))
    })
  }
  const select = async (host: HTMLElement, commandId: string) => {
    await act(async () => {
      const selector = host.querySelector<HTMLSelectElement>('[aria-label="选择启动命令"]')!
      selector.value = commandId
      selector.dispatchEvent(new Event('change', { bubbles: true }))
    })
  }
  const submit = async (element: HTMLInputElement) => {
    await act(async () => element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })))
  }
  it.each([false, true])('selects command ownership for the header favorite and startup fields, dark=%s', async (dark) => {
    const ui = await setup(dark)
    try {
      expect(ui.host.querySelector('button[aria-label="保存启动键 upper"]')).toBeNull()
      await select(ui.host, 'constructor')
      expect(input(ui.host, 'constructor').value).toBe('/lower')
      const star = button(ui.host, '设为常用 constructor')
      expect(star.closest('.settings-header')).not.toBeNull()
      expect(star.getAttribute('aria-pressed')).toBe('false')
      await act(async () => star.click())
      expect(ui.client.setPublicPluginFavorite).toHaveBeenCalledWith({ pluginId: 'com.example.multi', commandId: 'constructor', favorite: true })
      expect(button(ui.host, '取消常用 constructor')?.getAttribute('aria-pressed')).toBe('true')
      await select(ui.host, 'upper')
      expect(button(ui.host, '设为常用 upper')).not.toBeNull()
    } finally { await ui.cleanup() }
  })
  it.each([false, true])('keeps cleanup-pending removal feedback on the list and refreshes only reads, readFailure=%s', async (readFailure) => {
    const ui = await setup()
    try {
      vi.mocked(ui.client.uninstallPublicPlugin).mockImplementationOnce(async () => {
        ui.remove()
        throw { code: 'dataCleanupPending', message: 'PRIVATE_SENTINEL' }
      })
      if (readFailure) vi.mocked(ui.client.listPublicPlugins).mockRejectedValueOnce(new Error('read failed'))
      await act(async () => button(ui.host, '删除插件').click())
      await act(async () => document.querySelector<HTMLInputElement>('[role="dialog"] input[type="checkbox"]')!.click())
      await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="确认删除插件"]')!.click())
      expect(ui.host.querySelector('.public-plugin-detail')).toBeNull()
      const list = ui.host.querySelector('.public-plugin-inventory')!
      expect(list.textContent).toContain('插件已删除，但部分插件数据尚未清理')
      expect(list.textContent).not.toContain('PRIVATE_SENTINEL')
      if (readFailure) {
        expect(list.querySelector('[role="alert"]')).not.toBeNull()
        await act(async () => list.querySelector<HTMLButtonElement>('[aria-label="刷新"]')!.click())
        expect(list.querySelector('[role="alert"]')).toBeNull()
        expect(list.textContent).toContain('插件已删除，但部分插件数据尚未清理')
      }
      expect(ui.client.uninstallPublicPlugin).toHaveBeenCalledExactlyOnceWith({ pluginId: 'com.example.multi', retainData: false })
    } finally { await ui.cleanup() }
  })
  it('favorites on the first click while keeping an edited command draft', async () => {
    const ui = await setup()
    try {
      await enter(input(ui.host, 'upper'), '/my-draft')
      const star = button(ui.host, '设为常用 upper')
      await act(async () => {
        input(ui.host, 'upper').dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: star }))
        star.click()
      })
      expect(ui.client.setPublicPluginFavorite).toHaveBeenCalledWith({ pluginId: 'com.example.multi', commandId: 'upper', favorite: true })
      expect(ui.client.setPublicPluginEffectiveName).not.toHaveBeenCalled()
      expect(input(ui.host, 'upper').value).toBe('/my-draft')
      expect(button(ui.host, '取消常用 upper')).not.toBeNull()
    } finally { await ui.cleanup() }
  })
  it('normalizes a full command once on blur and does not rewrite unchanged input', async () => {
    const ui = await setup()
    try {
      await enter(input(ui.host, 'upper'), '  /renamed  ')
      await act(async () => input(ui.host, 'upper').dispatchEvent(new FocusEvent('focusout', { bubbles: true })))
      expect(ui.client.setPublicPluginEffectiveName).toHaveBeenCalledWith({ pluginId: 'com.example.multi', commandId: 'upper', nameOverride: 'renamed' })
      expect(input(ui.host, 'upper').value).toBe('/renamed')
      await submit(input(ui.host, 'upper'))
      expect(ui.client.setPublicPluginEffectiveName).toHaveBeenCalledTimes(1)
    } finally { await ui.cleanup() }
  })
  it('saves before switching and retains the current draft and selection when saving fails', async () => {
    const ui = await setup()
    try {
      vi.mocked(ui.client.setPublicPluginEffectiveName).mockRejectedValueOnce(new Error('invalid'))
      await enter(input(ui.host, 'upper'), '/draft')
      await select(ui.host, 'constructor')
      expect(ui.host.querySelector<HTMLSelectElement>('[aria-label="选择启动命令"]')!.value).toBe('upper')
      expect(input(ui.host, 'upper').value).toBe('/draft')
      expect(ui.host.querySelector('[role="alert"]')?.textContent).toContain('无法保存')
      await select(ui.host, 'constructor')
      expect(input(ui.host, 'constructor').value).toBe('/lower')
      expect(ui.client.setPublicPluginEffectiveName).toHaveBeenLastCalledWith({ pluginId: 'com.example.multi', commandId: 'upper', nameOverride: 'draft' })
    } finally { await ui.cleanup() }
  })
  it('rejects an empty command inline and resets a dirty draft without a competing blur write', async () => {
    const ui = await setup()
    try {
      await enter(input(ui.host, 'upper'), ' ')
      await submit(input(ui.host, 'upper'))
      expect(ui.client.setPublicPluginEffectiveName).not.toHaveBeenCalled()
      expect(ui.host.querySelector('[role="alert"]')?.textContent).toContain('不能为空')
      await enter(input(ui.host, 'upper'), '/dirty')
      const reset = button(ui.host, '恢复默认启动键 upper')
      await act(async () => {
        input(ui.host, 'upper').dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: reset }))
        reset.click()
      })
      expect(ui.client.setPublicPluginEffectiveName).not.toHaveBeenCalled()
      expect(input(ui.host, 'upper').value).toBe('/upper')
      await enter(input(ui.host, 'upper'), '/custom'); await submit(input(ui.host, 'upper'))
      await act(async () => reset.click())
      expect(ui.client.setPublicPluginEffectiveName).toHaveBeenLastCalledWith({ pluginId: 'com.example.multi', commandId: 'upper', nameOverride: null })
    } finally { await ui.cleanup() }
  })
  it.each(['rename', 'favorite'] as const)('gates command switching and all current controls until %s reconciliation completes', async (kind) => {
    const ui = await setup()
    try {
      const refresh = deferred<Awaited<ReturnType<LauncherClient['listPublicPlugins']>>>()
      vi.mocked(ui.client.listPublicPlugins).mockReturnValueOnce(refresh.promise)
      if (kind === 'rename') { await enter(input(ui.host, 'upper'), '/custom'); await submit(input(ui.host, 'upper')) }
      else await act(async () => button(ui.host, '设为常用 upper').click())
      expect(input(ui.host, 'upper').readOnly).toBe(true)
      expect(button(ui.host, '恢复默认启动键 upper').disabled).toBe(true)
      expect(button(ui.host, '设为常用 upper').disabled).toBe(true)
      expect(ui.host.querySelector<HTMLSelectElement>('[aria-label="选择启动命令"]')!.disabled).toBe(true)
      await select(ui.host, 'constructor')
      expect(ui.host.querySelector<HTMLSelectElement>('[aria-label="选择启动命令"]')!.value).toBe('upper')
      await act(async () => refresh.resolve(ui.snapshot()))
      expect(input(ui.host, 'upper').readOnly).toBe(false)
      await select(ui.host, 'constructor')
      expect(input(ui.host, 'constructor').value).toBe('/lower')
    } finally { await ui.cleanup() }
  })
  it('retries only the authoritative read after a write outcome cannot be reconciled', async () => {
    const ui = await setup()
    try {
      vi.mocked(ui.client.listPublicPlugins).mockRejectedValueOnce(new Error('read failed'))
      await enter(input(ui.host, 'upper'), '/custom'); await submit(input(ui.host, 'upper'))
      expect(button(ui.host, '刷新命令状态 upper')).not.toBeNull()
      expect(input(ui.host, 'upper').readOnly).toBe(true)
      expect(button(ui.host, '设为常用 upper').disabled).toBe(true)
      await act(async () => button(ui.host, '刷新命令状态 upper').click())
      expect(ui.client.setPublicPluginEffectiveName).toHaveBeenCalledTimes(1)
      expect(input(ui.host, 'upper').value).toBe('/custom')
      expect(input(ui.host, 'upper').readOnly).toBe(false)
    } finally { await ui.cleanup() }
  })
  it('preserves moved focus when a pending command save completes', async () => {
    const ui = await setup()
    try {
      const pending = deferred<void>()
      vi.mocked(ui.client.setPublicPluginEffectiveName).mockReturnValueOnce(pending.promise)
      await enter(input(ui.host, 'upper'), '/custom'); await submit(input(ui.host, 'upper'))
      const back = button(ui.host, '返回插件列表')
      await act(async () => back.focus())
      await act(async () => pending.resolve())
      expect(document.activeElement).toBe(back)
    } finally { await ui.cleanup() }
  })
  it('does not let an old completion reload or focus a reopened detail', async () => {
    const ui = await setup()
    try {
      const pending = deferred<void>()
      vi.mocked(ui.client.setPublicPluginEffectiveName).mockReturnValueOnce(pending.promise)
      await enter(input(ui.host, 'upper'), '/old'); await submit(input(ui.host, 'upper'))
      await act(async () => button(ui.host, '返回插件列表').click()); await ui.open()
      await enter(input(ui.host, 'upper'), '/new')
      const reads = vi.mocked(ui.client.listPublicPlugins).mock.calls.length
      await act(async () => pending.resolve())
      expect(ui.client.listPublicPlugins).toHaveBeenCalledTimes(reads)
      expect(input(ui.host, 'upper').value).toBe('/new')
      expect(document.activeElement).toBe(input(ui.host, 'upper'))
    } finally { await ui.cleanup() }
  })
})



it.each(['pair', 'removed', 'empty', 'moved'] as const)('restores favorite focus to its exact pair or legal fallback without stealing moved focus: %s', async (scenario) => {
  installMatchMedia(false)
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
  const fake = fakeClient()
  const pending = deferred<void>()
  let refreshed = false
  const result = (commandId: string) => ({ resultId: 'same-result-id', title: 'Same visible title',
    activation: { kind: 'mainResultActivation' as const, pluginId: 'com.example.multi', commandId, commandLabel: commandId, initialArgument: '', favorite: refreshed },
    favorite: { target: { kind: 'publicPlugin' as const, pluginId: 'com.example.multi', commandId }, favorite: refreshed }, hasDefaultAction: false,
  })
  vi.mocked(fake.client.searchApps).mockImplementation(async () => ({ requestId: `menu-${refreshed}`, items: refreshed && scenario === 'empty' ? [] : refreshed && scenario === 'removed' ? [result('upper')] : [result('upper'), result('constructor')] }))
  vi.mocked(fake.client.setPublicPluginFavorite).mockReturnValueOnce(pending.promise)
  const core = createLauncherCore(fake.client); await core.start()
  const mounted = await mountLauncherView(core)
  try {
    await act(async () => fake.emit(shown(`pair-menu-${scenario}`)))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(2))
    const target = mounted.host.querySelectorAll<HTMLElement>('[role="option"]')[1]!
    await act(async () => target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    await vi.waitFor(() => expect(document.querySelector('[role="menuitem"]')).not.toBeNull())
    await act(async () => document.querySelector<HTMLElement>('[role="menuitem"]')!.click())
    expect(fake.client.setPublicPluginFavorite).toHaveBeenCalledWith({ pluginId: 'com.example.multi', commandId: 'constructor', favorite: true })
    const query = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    if (scenario === 'moved') {
      await act(async () => { target.focus(); query.focus() })
    }
    refreshed = true
    await act(async () => pending.resolve())
    await vi.waitFor(() => expect(core.getSnapshot().favoriteMutationPending || core.getSnapshot().searchPending).toBe(false))
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 40)) })
    const options = mounted.host.querySelectorAll<HTMLElement>('[role="option"]')
    await vi.waitFor(() => expect(document.activeElement).toBe(scenario === 'moved' || scenario === 'empty' ? query : scenario === 'pair' ? options[1] : options[0]))
    if (scenario === 'pair') await vi.waitFor(() => expect(options[1]?.getAttribute('aria-selected')).toBe('true'))
  } finally { await mounted.unmount(); core.destroy() }
})





it('rejects sibling favorite payloads and business-result favorites while accepting constructor entries', async () => {
  const { core, client, emit } = await startedCore()
  const activation = { kind: 'mainResultActivation' as const, pluginId: 'com.example.multi', commandId: 'constructor', commandLabel: 'lower', initialArgument: '', favorite: false }
  const favorite = (commandId: string) => ({ target: { kind: 'publicPlugin' as const, pluginId: activation.pluginId, commandId }, favorite: false })
  vi.mocked(client.searchApps).mockResolvedValue({ requestId: 'strict-pairs', items: [
    { resultId: 'same', title: 'Same', activation, favorite: favorite('upper') },
    { resultId: 'same', title: 'Same', activation: executeActivation, favorite: favorite('constructor') },
    { resultId: 'same', title: 'Same', activation, favorite: favorite('constructor') },
  ] })
  emit(shown('strict-pairs'))
  await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
  expect(core.getSnapshot().results[0]?.favorite?.target).toEqual(favorite('constructor').target)
  core.openPluginContextMenu(0); core.setPluginFavorite(0, true)
  expect(client.setPublicPluginFavorite).toHaveBeenCalledWith({ pluginId: activation.pluginId, commandId: 'constructor', favorite: true })
  core.destroy()
})

it('does not let a pending favorite for A refresh or update same-package B with identical result IDs and names', async () => {
  const { core, client, emit } = await startedCore()
  const pending = deferred<void>()
  vi.mocked(client.setPublicPluginFavorite).mockReturnValueOnce(pending.promise)
  vi.mocked(client.searchApps).mockImplementation(async ({ query, querySequence }) => {
    const commandId = query === 'a' ? 'upper' : 'constructor'
    return { requestId: `pair-${querySequence}`, items: [{ resultId: 'same', title: 'Same',
      activation: { kind: 'mainResultActivation', pluginId: 'com.example.multi', commandId, commandLabel: commandId, initialArgument: '', favorite: false },
      favorite: { target: { kind: 'publicPlugin', pluginId: 'com.example.multi', commandId }, favorite: false },
    }] }
  })
  emit(shown('same-package-favorite'))
  core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'a', inputType: 'insertText' })
  await vi.waitFor(() => expect(core.getSnapshot().results[0]?.favorite?.target).toMatchObject({ commandId: 'upper' }))
  core.openPluginContextMenu(0); core.setPluginFavorite(0, true)
  core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'b', inputType: 'insertText' })
  await vi.waitFor(() => expect(core.getSnapshot().results[0]?.favorite?.target).toMatchObject({ commandId: 'constructor' }))
  const reads = vi.mocked(client.searchApps).mock.calls.length
  pending.resolve(); await pending.promise
  await vi.waitFor(() => expect(core.getSnapshot().favoriteMutationPending).toBe(false))
  expect(client.searchApps).toHaveBeenCalledTimes(reads)
  expect(core.getSnapshot().results[0]?.favorite).toMatchObject({ target: { commandId: 'constructor' }, favorite: false })
  core.destroy()
})


it.each([[false, false], [true, false], [false, true], [true, true]])('returns cancelled favorite-menu focus to its pair unless focus moved, dark=%s moved=%s', async (dark, moved) => {
  installMatchMedia(dark)
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: dark ? 'dark' : 'light' })
  vi.mocked(fake.client.searchApps).mockResolvedValue({ requestId: 'cancel-menu', items: ['upper', 'constructor'].map((commandId) => ({
    resultId: commandId, title: commandId,
    activation: { kind: 'mainResultActivation', pluginId: 'com.example.multi', commandId, commandLabel: commandId, initialArgument: '', favorite: false },
    favorite: { target: { kind: 'publicPlugin', pluginId: 'com.example.multi', commandId }, favorite: false },
  })) })
  const core = createLauncherCore(fake.client); await core.start()
  const mounted = await mountLauncherView(core)
  try {
    await act(async () => fake.emit(shown('cancel-menu')))
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(2))
    const target = mounted.host.querySelectorAll<HTMLElement>('[role="option"]')[1]!
    await act(async () => target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })))
    await vi.waitFor(() => expect(document.querySelector('[role="menuitem"]')).not.toBeNull())
    const menu = document.querySelector<HTMLElement>('[role="menuitem"]')!
    const query = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    await act(async () => {
      menu.focus()
      if (moved) query.focus()
      menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', keyCode: 27, bubbles: true, cancelable: true }))
      await new Promise((resolve) => setTimeout(resolve, 80))
    })
    await vi.waitFor(() => expect(document.activeElement).toBe(moved ? query : target))
    expect(fake.client.setPublicPluginFavorite).not.toHaveBeenCalled()
    expect(fake.client.executeResult).not.toHaveBeenCalled()
  } finally { await mounted.unmount(); core.destroy() }
})


describe('command hotkey entry', () => {
  const entry = { pluginId: 'com.example.hotkey', commandId: 'default', hotkey: 'Ctrl+N', bindingRevision: '1' }
  it.each(['before resolve', 'during open', 'after open'] as const)('opens the first hidden Panel hotkey when a default reply arrives %s', async (replyAt) => {
    vi.useFakeTimers()
    const fake = await startedCore()
    const { core, client } = fake
    try {
      const resolved = deferred<import('./protocol').CommandEntryResolution>()
      const defaultSearch = deferred<SearchResponse | null>()
      const opened = deferred<PluginPanelCommandResult>()
      const identity = { pluginId: entry.pluginId, commandLabel: 'notes', sessionEpoch: u64('71'), hostKeys: [] }
      vi.mocked(client.resolvePublicPluginCommandEntry).mockReturnValueOnce(resolved.promise)
      vi.mocked(client.searchApps).mockReturnValueOnce(defaultSearch.promise)
      vi.mocked(client.openPluginPanel).mockReturnValueOnce(opened.promise)
      vi.mocked(client.submitPluginPanel).mockResolvedValue(identity)

      fake.emit({ ...shown('hidden-panel-hotkey'), commandEntry: entry })
      // Native show and command entry happen in one event, before the default-search timer fires.
      await vi.advanceTimersByTimeAsync(0)
      const reply = async () => {
        defaultSearch.resolve({ requestId: 'default-reply', items: [] })
        await defaultSearch.promise
      }
      if (replyAt === 'before resolve') await reply()
      resolved.resolve({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
      await resolved.promise
      expect(client.openPluginPanel).toHaveBeenCalledExactlyOnceWith({ pluginId: entry.pluginId, commandId: entry.commandId, argument: '', stableCommandTarget: entry })
      if (replyAt === 'during open') await reply()
      opened.resolve(identity)
      await opened.promise
      if (replyAt === 'after open') await reply()
      await Promise.resolve()

      expect(core.getSnapshot().panel?.sessionEpoch).toBe('71')
      expect(client.submitPluginPanel).toHaveBeenCalledOnce()
      expect(client.closePluginPanel).not.toHaveBeenCalled()
      expect(client.searchApps).not.toHaveBeenCalled()
    } finally { core.destroy(); vi.useRealTimers() }
  })

  it.each(['opened', 'rejected', 'exit', 'edit', 'newer entry'] as const)('shows a command header without a launcher row during a deferred Panel open: %s', async (outcome) => {
    installMatchMedia(false)
    const fake = await startedCore()
    const { core, client } = fake
    const mounted = await mountLauncherView(core)
    try {
      const opened = deferred<PluginPanelCommandResult>()
      const identity = { pluginId: entry.pluginId, commandLabel: 'notes', sessionEpoch: u64('71'), hostKeys: [] }
      vi.mocked(client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
      vi.mocked(client.openPluginPanel).mockReturnValueOnce(opened.promise)
      vi.mocked(client.submitPluginPanel).mockResolvedValue(identity)
      await act(async () => fake.emit({ ...shown('pending-panel-presentation'), commandEntry: entry }))
      expect(client.openPluginPanel).toHaveBeenCalledOnce()
      expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(0)
      expect(core.getSnapshot().results).toEqual([])
      expect(core.getSnapshot().selectedIndex).toBe(-1)
      expect(mounted.host.querySelector('.panel-command-tag')?.textContent).toContain('/notes')
      expect(mounted.host.querySelector('[role="status"][aria-label="正在打开 notes 命令"]')).not.toBeNull()
      const input = mounted.host.querySelector<HTMLInputElement>('[aria-label="notes argument"]')!
      expect(input.disabled).toBe(false)
      expect(document.activeElement).toBe(input)
      // Enter during an open must neither dispatch again nor replace its private owner.
      await act(async () => core.keyDown('Enter', false))
      expect(client.openPluginPanel).toHaveBeenCalledOnce()
      if (outcome === 'exit') {
        await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="退出 notes 命令"]')!.click())
        expect(core.getSnapshot().executePending).toBe(false)
        expect(core.getSnapshot().mainResultCommand).toBeUndefined()
      } else if (outcome === 'edit') {
        await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'changed', inputType: 'insertText' }))
        expect(core.getSnapshot().executePending).toBe(false)
        expect(core.getSnapshot().mainResultCommand?.suffix).toBe('changed')
      } else if (outcome === 'newer entry') {
        vi.mocked(client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: 'newer', commandLabel: 'newer', outputMode: 'mainResult', activationMode: 'submit', inputRequired: true, inputPlaceholder: '内容' })
        await act(async () => core.commandHotkeyEntry({ ...entry, commandId: 'newer' }))
      }
      await act(async () => {
        if (outcome === 'rejected') opened.reject(new Error('open failed'))
        else opened.resolve(identity)
      })
      expect(mounted.host.querySelector('[role="status"][aria-label="正在打开 notes 命令"]')).toBeNull()
      if (outcome === 'opened') {
        expect(core.getSnapshot().panel?.sessionEpoch).toBe('71')
        expect(client.submitPluginPanel).toHaveBeenCalledOnce()
        expect(client.closePluginPanel).not.toHaveBeenCalled()
      } else {
        expect(core.getSnapshot().panel).toBeUndefined()
        expect(client.submitPluginPanel).not.toHaveBeenCalled()
        if (outcome === 'rejected') {
          expect(core.getSnapshot().status).toBe('操作不可用，请重试。')
          expect(core.getSnapshot().mainResultCommand).toBeUndefined()
        } else expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '71' })
        if (outcome === 'newer entry') expect(core.getSnapshot().mainResultCommand?.commandId).toBe('newer')
        if (outcome === 'edit') {
          expect(core.getSnapshot().mainResultCommand?.suffix).toBe('changed')
          vi.mocked(client.openPluginPanel).mockResolvedValueOnce({ ...identity, sessionEpoch: u64('72') })
          await act(async () => core.keyDown('Enter', false))
          expect(client.openPluginPanel).toHaveBeenLastCalledWith({ pluginId: entry.pluginId, commandId: entry.commandId, argument: 'changed', stableCommandTarget: entry })
        }
      }
    } finally { await mounted.unmount(); core.destroy() }
  })
  it.each([false, true])('still defers default search for an ordinary native show (same-value input: %s)', async (sameValueInput) => {
    vi.useFakeTimers()
    const fake = await startedCore()
    try {
      vi.mocked(fake.client.searchApps).mockResolvedValueOnce({ requestId: 'ordinary-default', items: [panelItem('')] } as unknown as SearchResponse)
      fake.emit(shown('ordinary-native-show'))
      if (sameValueInput) {
        fake.core.text({ kind: 'ordinaryInput', control: fake.core.getSnapshot().queryControl, value: '', inputType: 'insertText' })
      }
      expect(fake.client.searchApps).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(0)
      expect(fake.client.searchApps).toHaveBeenCalledExactlyOnceWith({ query: '', invocationId: 'ordinary-native-show', querySequence: 1 })
      expect(fake.core.getSnapshot().results).toHaveLength(1)
      expect(fake.client.resolvePublicPluginCommandEntry).not.toHaveBeenCalled()
    } finally { fake.core.destroy(); vi.useRealTimers() }
  })

  it.each(['edit', 'composition'] as const)('retires the native-show default search after a real %s', async (change) => {
    vi.useFakeTimers()
    const fake = await startedCore()
    try {
      fake.emit(shown('ordinary-native-show'))
      const control = fake.core.getSnapshot().queryControl
      if (change === 'edit') {
        fake.core.text({ kind: 'ordinaryInput', control, value: 'new query', inputType: 'insertText' })
      } else {
        fake.core.text({ kind: 'compositionStart', control })
      }
      await vi.advanceTimersByTimeAsync(0)
      expect(vi.mocked(fake.client.searchApps).mock.calls.map(([request]) => request.query)).toEqual(change === 'edit' ? ['new query'] : [])
    } finally { fake.core.destroy(); vi.useRealTimers() }
  })

  it.each(['newer entry', 'navigation'] as const)('ignores a hidden command resolve superseded by %s', async (supersededBy) => {
    vi.useFakeTimers()
    const fake = await startedCore()
    const { core, client } = fake
    try {
      const old = deferred<import('./protocol').CommandEntryResolution>()
      vi.mocked(client.resolvePublicPluginCommandEntry).mockReturnValueOnce(old.promise)
      fake.emit({ ...shown('hidden-stale-hotkey'), commandEntry: entry })
      const newer = { ...entry, commandId: 'newer' }
      const next = deferred<import('./protocol').CommandEntryResolution>()
      if (supersededBy === 'newer entry') {
        vi.mocked(client.resolvePublicPluginCommandEntry).mockReturnValueOnce(next.promise)
        core.commandHotkeyEntry(newer)
      } else core.navigate('settings')
      await vi.advanceTimersByTimeAsync(0)
      old.resolve({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
      await old.promise
      expect(client.openPluginPanel).not.toHaveBeenCalled()
      expect(client.searchApps).not.toHaveBeenCalled()
      if (supersededBy === 'newer entry') {
        const identity = { pluginId: newer.pluginId, commandLabel: 'newer', sessionEpoch: u64('72'), hostKeys: [] }
        vi.mocked(client.openPluginPanel).mockResolvedValueOnce(identity)
        vi.mocked(client.submitPluginPanel).mockResolvedValue(identity)
        next.resolve({ pluginId: newer.pluginId, commandId: newer.commandId, commandLabel: 'newer', outputMode: 'panel', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
        await next.promise
        await Promise.resolve()
        expect(core.getSnapshot().panel?.sessionEpoch).toBe('72')
        expect(client.openPluginPanel).toHaveBeenCalledExactlyOnceWith({ pluginId: newer.pluginId, commandId: newer.commandId, argument: '', stableCommandTarget: newer })
      } else expect(core.getSnapshot().view).toBe('settings')
    } finally { core.destroy(); vi.useRealTimers() }
  })

  it('commits a no-input command once with stable identity and no result action', async () => {
    const fake = await startedCore(); const { core } = fake
    fake.emit(shown('hotkey-invocation'))
    await Promise.resolve(); await Promise.resolve()
    vi.mocked(fake.client.searchApps).mockClear()
    core.commandHotkeyEntry(entry)
    await Promise.resolve(); await Promise.resolve()
    expect(fake.client.resolvePublicPluginCommandEntry).toHaveBeenCalledWith({ entry })
    expect(fake.client.searchApps).toHaveBeenCalledTimes(1)
    expect(fake.client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ submit: true, stableCommandTarget: entry }))
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    core.destroy()
  })
  it('supersedes an old search reply immediately while the newer entry resolve awaits', async () => {
    const fake = await startedCore(); const { core } = fake
    fake.emit(shown('hotkey-invocation'))
    await Promise.resolve(); await Promise.resolve()
    const oldSearch = deferred<SearchResponse | null>()
    vi.mocked(fake.client.searchApps).mockReturnValueOnce(oldSearch.promise)
    core.commandHotkeyEntry(entry)
    await Promise.resolve(); await Promise.resolve()
    const newResolve = deferred<import('./protocol').CommandEntryResolution>()
    vi.mocked(fake.client.resolvePublicPluginCommandEntry).mockReturnValueOnce(newResolve.promise)
    core.commandHotkeyEntry(entry)
    oldSearch.resolve({ requestId: 'obsolete-hotkey-window', items: [], windowTransferToken: 'obsolete-transfer' })
    await Promise.resolve(); await Promise.resolve()
    expect(fake.client.commitPluginWindowTransfer).not.toHaveBeenCalled()
    core.destroy()
  })
  it('arms required-input Window without dispatch and rejects an obsolete resolve after navigation', async () => {
    const fake = await startedCore(); const { core } = fake
    fake.emit(shown('hotkey-invocation'))
    await Promise.resolve(); await Promise.resolve()
    vi.mocked(fake.client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'renamed', outputMode: 'window', activationMode: 'submit', inputRequired: true, inputPlaceholder: '内容' })
    vi.mocked(fake.client.searchApps).mockClear()
    core.commandHotkeyEntry(entry)
    await Promise.resolve(); await Promise.resolve()
    expect(core.getSnapshot().mainResultCommand?.commandLabel).toBe('renamed')
    expect(vi.mocked(fake.client.searchApps).mock.calls.every(([input]) => input.submit !== true)).toBe(true)
    const pending = deferred<import('./protocol').CommandEntryResolution>()
    vi.mocked(fake.client.resolvePublicPluginCommandEntry).mockReturnValueOnce(pending.promise)
    core.commandHotkeyEntry(entry)
    core.navigate('settings')
    pending.resolve({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'late', outputMode: 'mainResult', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
    await Promise.resolve(); await Promise.resolve()
    expect(core.getSnapshot().view).toBe('settings')
    expect(core.getSnapshot().mainResultCommand).toBeUndefined()
    core.destroy()
  })
  async function openHotkeyPanel(inputRequired: boolean) {
    const fake = await startedCore()
    fake.emit(shown('panel-hotkey-exit'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    vi.mocked(fake.client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired, inputPlaceholder: '内容' })
    const identity = { pluginId: entry.pluginId, commandLabel: 'notes', sessionEpoch: u64('71'), hostKeys: [] }
    vi.mocked(fake.client.openPluginPanel).mockResolvedValueOnce(identity)
    vi.mocked(fake.client.submitPluginPanel).mockResolvedValue(identity)
    fake.core.commandHotkeyEntry(entry)
    if (inputRequired) {
      await vi.waitFor(() => expect(fake.core.getSnapshot().mainResultCommand).toBeDefined())
      expect(fake.client.openPluginPanel).not.toHaveBeenCalled()
      fake.core.text({ kind: 'ordinaryInput', control: fake.core.getSnapshot().mainResultCommand!.suffixControl, value: 'initial', inputType: 'insertText' })
      fake.core.keyDown('Enter', false)
    }
    await vi.waitFor(() => expect(fake.core.getSnapshot().panel?.sessionEpoch).toBe('71'))
    await vi.waitFor(() => expect(fake.core.getSnapshot().panel?.submitPending).toBe(false))
    expect(fake.client.openPluginPanel).toHaveBeenCalledWith({ pluginId: entry.pluginId, commandId: entry.commandId, argument: inputRequired ? 'initial' : '', stableCommandTarget: entry })
    return fake
  }

  it.each([false, true])('restores default and ordinary search after closing a Panel hotkey with required input %s', async (required) => {
    const fake = await openHotkeyPanel(required)
    const { core, client } = fake
    try {
      vi.mocked(client.searchApps).mockImplementation(async () => ({ requestId: 'after-panel', items: [panelItem('')] } as unknown as SearchResponse))
      vi.mocked(client.searchApps).mockClear()
      await core.closePanel()
      expect(core.getSnapshot().mainResultCommand).toBeUndefined()
      await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
      expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '' }))
      expect(vi.mocked(client.searchApps).mock.lastCall![0]).not.toHaveProperty('stableCommandTarget')
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'ordinary', inputType: 'insertText' })
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'ordinary' })))
      expect(vi.mocked(client.searchApps).mock.lastCall![0]).not.toHaveProperty('stableCommandTarget')
    } finally { core.destroy() }
  })

  it.each(['reset', 'error'] as const)('clears required-input Panel hotkey ownership after host %s', async (exit) => {
    const fake = await openHotkeyPanel(true)
    const { core, client } = fake
    try {
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().panel!.suffixControl, value: 'updated', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(client.submitPluginPanel).toHaveBeenLastCalledWith(expect.objectContaining({ argument: 'updated' })))
      vi.mocked(client.searchApps).mockClear()
      if (exit === 'reset') fake.emitPanelReset({ sessionEpoch: '71' })
      else fake.emitPanelError({ sessionEpoch: '71' })
      expect(core.getSnapshot().panel).toBeUndefined()
      expect(core.getSnapshot().mainResultCommand).toBeUndefined()
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '' })))
      expect(vi.mocked(client.searchApps).mock.lastCall![0]).not.toHaveProperty('stableCommandTarget')
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'ordinary', inputType: 'insertText' })
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'ordinary' })))
      expect(vi.mocked(client.searchApps).mock.lastCall![0]).not.toHaveProperty('stableCommandTarget')
    } finally { core.destroy() }
  })

  it.each([
    ['rejected', false], ['rejected', true], ['mismatched', false], ['mismatched', true],
  ] as const)('releases a %s Panel hotkey open with required input %s', async (outcome, inputRequired) => {
    const fake = await startedCore()
    const { core, client } = fake
    try {
      fake.emit(shown('failed-panel-hotkey'))
      await new Promise((resolve) => setTimeout(resolve, 0))
      vi.mocked(client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired, inputPlaceholder: '内容' })
      if (outcome === 'rejected') vi.mocked(client.openPluginPanel).mockRejectedValueOnce(new Error('open failed'))
      else vi.mocked(client.openPluginPanel).mockResolvedValueOnce({ pluginId: 'com.example.other', commandLabel: 'other', sessionEpoch: u64('72'), hostKeys: [] })
      core.commandHotkeyEntry(entry)
      if (inputRequired) {
        await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand).toBeDefined())
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'initial', inputType: 'insertText' })
        core.keyDown('Enter', false)
      }
      await vi.waitFor(() => expect(core.getSnapshot().status).toBe('操作不可用，请重试。'))
      expect(core.getSnapshot()).toMatchObject({ query: '', results: [], executePending: false })
      expect(core.getSnapshot().panel).toBeUndefined()
      expect(core.getSnapshot().mainResultCommand).toBeUndefined()
      expect(client.submitPluginPanel).not.toHaveBeenCalled()
      if (outcome === 'mismatched') expect(client.closePluginPanel).toHaveBeenCalledWith({ sessionEpoch: '72' })
      vi.mocked(client.searchApps).mockClear()
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'ordinary', inputType: 'insertText' })
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'ordinary' })))
      expect(vi.mocked(client.searchApps).mock.lastCall![0]).not.toHaveProperty('stableCommandTarget')
    } finally { core.destroy() }
  })

  it.each(['rejected', 'mismatched'] as const)('ignores a stale %s Panel hotkey open while a newer resolve awaits', async (outcome) => {
    const fake = await startedCore()
    const { core, client } = fake
    try {
      fake.emit(shown('stale-failed-panel-hotkey'))
      await new Promise((resolve) => setTimeout(resolve, 0))
      vi.mocked(client.resolvePublicPluginCommandEntry).mockResolvedValueOnce({ pluginId: entry.pluginId, commandId: entry.commandId, commandLabel: 'notes', outputMode: 'panel', activationMode: 'submit', inputRequired: false, inputPlaceholder: null })
      const opened = deferred<PluginPanelCommandResult>()
      vi.mocked(client.openPluginPanel).mockReturnValueOnce(opened.promise)
      core.commandHotkeyEntry(entry)
      await vi.waitFor(() => expect(client.openPluginPanel).toHaveBeenCalledOnce())
      const newer = { ...entry, commandId: 'newer' }
      const resolved = deferred<import('./protocol').CommandEntryResolution>()
      vi.mocked(client.resolvePublicPluginCommandEntry).mockReturnValueOnce(resolved.promise)
      core.commandHotkeyEntry(newer)
      if (outcome === 'rejected') opened.reject(new Error('stale failure'))
      else opened.resolve({ pluginId: 'com.example.other', commandLabel: 'other', sessionEpoch: u64('72'), hostKeys: [] })
      await Promise.resolve(); await Promise.resolve()
      expect(core.getSnapshot().status).not.toBe('操作不可用，请重试。')
      resolved.resolve({ pluginId: newer.pluginId, commandId: newer.commandId, commandLabel: 'newer', outputMode: 'mainResult', activationMode: 'submit', inputRequired: true, inputPlaceholder: '内容' })
      await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand?.commandId).toBe('newer'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'kept', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '/newer kept', submit: true, stableCommandTarget: newer })))
    } finally { core.destroy() }
  })

  it('keeps a newer command owner when an old Panel close settles', async () => {
    const fake = await openHotkeyPanel(true)
    const { core, client } = fake
    try {
      const closed = deferred<void>()
      vi.mocked(client.closePluginPanel).mockReturnValueOnce(closed.promise)
      const closing = core.closePanel()
      const newer = { ...entry, commandId: 'newer' }
      const resolved = deferred<import('./protocol').CommandEntryResolution>()
      vi.mocked(client.resolvePublicPluginCommandEntry).mockReturnValueOnce(resolved.promise)
      core.commandHotkeyEntry(newer)
      closed.resolve()
      await closing
      resolved.resolve({ pluginId: newer.pluginId, commandId: newer.commandId, commandLabel: 'newer', outputMode: 'mainResult', activationMode: 'submit', inputRequired: true, inputPlaceholder: '内容' })
      await vi.waitFor(() => expect(core.getSnapshot().mainResultCommand?.commandId).toBe('newer'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().mainResultCommand!.suffixControl, value: 'kept', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(client.searchApps).toHaveBeenLastCalledWith(expect.objectContaining({ query: '/newer kept', submit: true, stableCommandTarget: newer })))
    } finally { core.destroy() }
  })

  it('offers foreground and hidden chords to recording before navigation and suppresses delayed delivery', async () => {
    const fake = await startedSettingsCore(); const { core } = fake
    const consume = vi.fn()
    window.addEventListener('uipilot-hotkey-recording-current', consume)
    core.setHotkeyRecordingPhase('recording')
    core.commandHotkeyEntry(entry)
    expect(consume).toHaveBeenCalledTimes(1)
    expect(fake.client.resolvePublicPluginCommandEntry).not.toHaveBeenCalled()
    core.setHotkeyRecordingPhase('completed')
    fake.emit({ ...shown('recorded'), commandEntry: entry })
    expect(core.getSnapshot().view).toBe('settings')
    expect(fake.client.resolvePublicPluginCommandEntry).not.toHaveBeenCalled()
    window.removeEventListener('uipilot-hotkey-recording-current', consume)
    core.destroy()
  })
})


describe('builtin feature detail navigation', () => {
  it.each(['light', 'dark'] as const)('uses the existing detail shell, Back and Escape in %s theme', async (colorScheme) => {
    installMatchMedia(colorScheme === 'dark')
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValue({ ...settingsFixture, theme: colorScheme })
    const feature = { feature: 'screenshot' as const, name: '截图', command: '/screenshot', defaultCommand: '/screenshot', favorite: false, hotkey: null, hotkeyStatus: 'unset' as const, directory: null, description: '屏幕区域截图', examples: ['/screenshot'], version: '0.14.0', enabled: true }
    vi.mocked(fake.client.listBuiltinFeatures).mockResolvedValue([feature])
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('builtin-settings', 'settings')))
      await activateSettingsTab(mounted.host, '插件')
      const open = async () => {
        await vi.waitFor(() => expect(mounted.host.querySelector('[data-builtin-feature="screenshot"]')).not.toBeNull())
        await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="查看 截图 详情"]')!.click())
        await vi.waitFor(() => expect(mounted.host.querySelector('[aria-label="截图 内置功能详情"]')).not.toBeNull())
        expect(document.activeElement).toBe(mounted.host.querySelector('.public-plugin-detail-view'))
        expect(mounted.host.querySelector('.settings-detail-panel')).not.toBeNull()
        expect(mounted.host.querySelector('.public-plugin-detail')!.textContent).toContain('/screenshot')
        expect(mounted.host.querySelector('[aria-label="卸载插件"]')).toBeNull()
      }
      await open()
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="返回插件列表"]')!.click())
      await vi.waitFor(() => expect(document.activeElement).toBe(mounted.host.querySelector('[role="tab"][aria-selected="true"]')))
      await open()
      await act(async () => mounted.host.querySelector<HTMLElement>('.public-plugin-detail-view')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
      await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-detail-view')).toBeNull())
      expect(fake.client.uninstallPublicPlugin).not.toHaveBeenCalled()
    } finally { await mounted.unmount(); core.destroy() }
  })
})

describe('builtin settings query preservation', () => {
  it('keeps the current query and refreshes results after a settings toggle and local return', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const feature = { feature: 'screenshot' as const, name: '截图', command: '/screenshot', defaultCommand: '/screenshot', favorite: false, hotkey: null, hotkeyStatus: 'unset' as const, directory: null, description: '屏幕区域截图', examples: ['/screenshot'], version: '0.14.0', enabled: true }
    vi.mocked(fake.client.listBuiltinFeatures).mockResolvedValue([feature])
    vi.mocked(fake.client.setBuiltinFeatureEnabled).mockResolvedValue([{ ...feature, enabled: false }])
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('builtin-query')))
      await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/screenshot', inputType: 'insertText' }))
      await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
      await act(async () => core.navigate('settings'))
      await activateSettingsTab(mounted.host, '插件')
      await vi.waitFor(() => expect(mounted.host.querySelector('[data-builtin-feature="screenshot"]')).not.toBeNull())
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="启用 截图"]')!.click())
      await vi.waitFor(() => expect(mounted.host.querySelector('[aria-label="启用 截图"]')!.getAttribute('aria-checked')).toBe('false'))
      vi.mocked(fake.client.searchApps).mockClear()
      await act(async () => mounted.host.querySelector<HTMLButtonElement>('[aria-label="返回主界面"]')!.click())
      expect(core.getSnapshot().query).toBe('/screenshot')
      await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({ query: '/screenshot' })))
      expect(document.activeElement).toBe(mounted.host.querySelector('input[id^="launcher-query-"]'))
    } finally { await mounted.unmount(); core.destroy() }
  })

  it('invalidates visible results and reruns the same query after a builtin state event', async () => {
    const fake = fakeClient()
    const core = createLauncherCore(fake.client)
    await core.start()
    try {
      fake.emit(shown('builtin-event'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/screenshot', inputType: 'insertText' })
      await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({ query: '/screenshot' })))
      vi.mocked(fake.client.searchApps).mockClear()
      fake.emitBuiltinFeaturesChanged()
      expect(core.getSnapshot().query).toBe('/screenshot')
      expect(core.getSnapshot().results).toEqual([])
      await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith(expect.objectContaining({ query: '/screenshot' })))
    } finally { core.destroy() }
  })
})
