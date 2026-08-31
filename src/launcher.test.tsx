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
// @ts-expect-error Vite supplies the raw source module in Vitest.
import launcherViewSource from './launcher-view.tsx?raw'
import {
  compareDecimalRevision,
  parseFileSearchResponse,
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
  expect(previewSource).toContain('com.uipilot.notes/preview.html')
  expect(previewSource).toContain("querySelector<HTMLElement>('.panel-host-region')")
  expect(previewSource).toContain("frame.style.width = '100%'")
  expect(previewSource).toContain("frame.style.height = '100%'")
  expect(previewSource).toContain('region.replaceChildren(frame)')
  expect(previewSource).not.toContain("frame.style.left = '12px'")
  expect(previewSource).not.toContain('@tauri-apps')
  expect(readFileSync('index.html', 'utf8')).not.toContain('launcher-browser-preview')
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
      source: 'localPackage', defaultName: 'demo', effectiveName: 'demo', enabled: true,
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
    expect(parsePublicPluginInventory({ revision: '1', items: [item] })).toEqual({ revision: '1', items: [item] })
    expect(parsePublicPluginInventory({ revision: '1', items: [{ ...item, iconUrl: null }] })).not.toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', items: [{ ...item, iconUrl: 'https://example.com/icon.png' }] })).toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', items: [{ ...item, outputMode: 'mainResult' }] })).toBeNull()
    expect(parsePublicPluginInventory({ revision: '1', items: [{ ...item, settings: [{ definition: { type: 'secret', key: 'token', label: 'Token' }, secretConfigured: true, value: 'leak' }] }] })).toBeNull()
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
const tauriCapture = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
const windowCapture = vi.hoisted(() => ({ label: 'main' }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauriCapture.invoke }))
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
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
  theme: 'system',
  webSearchEngine: 'bing',
}

const settingsFixture: SettingsView = {
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
  let messageStateHandler: ((payload: unknown) => void) | undefined
  let panelErrorHandler: ((payload: unknown) => void) | undefined
  let panelResetHandler: ((payload: unknown) => void) | undefined
  let panelFocusHandler: ((payload: unknown) => void) | undefined
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
    listenMessageStateChanged: vi.fn(async (handler) => {
      messageStateHandler = handler
      return vi.fn()
    }),
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
    commitPluginWindowTransfer: vi.fn(async () => {}),
    openFind: vi.fn(async () => ({ status: 'forwarded' as const })),
    searchFiles: vi.fn(async () => null),
    setFilePreviewPreference: vi.fn(async () => undefined),
    setThemePreference: vi.fn(async () => undefined),
    setWebSearchEngine: vi.fn(async () => undefined),
    executeResult: vi.fn(async () => ({ status: 'launchRequested' }) satisfies ExecuteOutcome),
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
    listPublicPlugins: vi.fn(async () => ({ revision: '0', items: [] })),
    selectPublicPluginArchive: vi.fn(async () => null),
    selectPublicPluginDirectory: vi.fn(async () => null),
    preparePublicPlugin: vi.fn(async () => { throw new Error('not prepared') }),
    commitPublicPlugin: vi.fn(async () => undefined),
    cancelPublicPlugin: vi.fn(async () => undefined),
    setPublicPluginEnabled: vi.fn(async () => undefined),
    setPublicPluginNetworkAccess: vi.fn(async () => undefined),
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
    emitMessageState(payload: unknown) {
      if (!messageStateHandler) throw new Error('message state listener is not installed')
      messageStateHandler(payload)
    },
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

function panelItem(initialArgument: string, pluginId = 'com.uipilot.demo-panel') {
  return {
    resultId: `panel-${pluginId}`,
    title: '/demo-panel',
    subtitle: '打开面板',
    activation: {
      kind: 'panelActivation' as const,
      pluginId,
      initialArgument,
      favorite: false,
    },
    favorite: {
      target: { kind: 'publicPlugin' as const, pluginId },
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

async function mountLauncherView(core: ReturnType<typeof createLauncherCore>) {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  const onReady = vi.fn()
  await act(async () => root.render(<LauncherView core={core} onReady={onReady} />))
  return {
    host,
    onReady,
    async unmount() {
      await act(async () => root.unmount())
      host.remove()
    },
  }
}

function settingsTab(host: HTMLElement, label: '通用' | '消息' | '插件'): HTMLElement {
  const tab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find(
    (candidate) => label === '消息'
      ? candidate.querySelector('.settings-message-tab-badge') !== null
      : candidate.textContent?.trim().endsWith(label),
  )
  if (!tab) throw new Error(`settings tab missing: ${label}`)
  return tab
}

async function activateSettingsTab(host: HTMLElement, label: '通用' | '消息' | '插件'): Promise<HTMLElement> {
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
      query: '', invocationId: 'first-native', querySequence: 3,
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
        kind: 'windowActivation',
        pluginId: 'com.uipilot.pomodoro',
        commandLabel: 'pomodoro',
        initialArgument: 'focus',
        favorite: true,
      },
      {
        kind: 'mainResultActivation',
        pluginId: 'com.uipilot.demo-return',
        commandLabel: 'demo-return',
        initialArgument: 'hello',
        favorite: false,
      },
      {
        kind: 'panelActivation',
        pluginId: 'com.uipilot.demo-panel',
        initialArgument: 'hello',
        favorite: true,
      },
      { kind: 'openFind', query: ' da  value ' },
      { kind: 'executeResult' },
    ]
    for (const activation of valid) expect(safeLauncherActivation(activation)).toEqual(activation)

    const invalid = [
      undefined,
      { kind: 'unknown' },
      { kind: 'executeResult', query: 'borrowed' },
      { kind: 'openFind' },
      { kind: 'openFind', query: 'query', completionText: '/demo-win ' },
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
      { kind: 'windowActivation', pluginId: 'Invalid Plugin', commandLabel: 'pomodoro', initialArgument: '', favorite: false },
      { kind: 'windowActivation', pluginId: 'com.uipilot.pomodoro', commandLabel: 'Pomodoro', initialArgument: '', favorite: false },
      { kind: 'windowActivation', pluginId: 'com.uipilot.pomodoro', commandLabel: 'pomodoro', initialArgument: ' bad', favorite: false },
      { kind: 'windowActivation', pluginId: 'com.uipilot.pomodoro', commandLabel: 'pomodoro', initialArgument: '', favorite: false, extra: true },
      { kind: 'mainResultActivation', pluginId: 'Invalid Plugin', commandLabel: 'demo-return', initialArgument: '', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'Demo-return', initialArgument: '', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'demo-return', initialArgument: ' bad', favorite: false },
      { kind: 'mainResultActivation', pluginId: 'com.uipilot.demo-return', commandLabel: 'demo-return', initialArgument: '', favorite: false, extra: true },
      { kind: 'panelActivation', pluginId: 'Invalid Plugin', initialArgument: '', favorite: true },
      { kind: 'panelActivation', pluginId: 'com.uipilot.demo-panel', initialArgument: 'bad\narg', favorite: false },
      { kind: 'panelActivation', pluginId: 'com.uipilot.demo-panel', initialArgument: ' hello', favorite: false },
    ]
    for (const activation of invalid) expect(safeLauncherActivation(activation)).toBeUndefined()
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
          kind: 'windowActivation',
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
      completionOrigin: { phase: 'commit', pluginId: 'com.uipilot.pomodoro' },
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

  it('navigates between launcher and settings without hiding and refreshes the empty launcher catalog', async () => {
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
      query: '',
      queryControlValue: '',
      querySequence: 3,
      searchPending: false,
    })
    await vi.waitFor(() => expect(fake.client.searchApps).toHaveBeenCalledWith({
      query: '',
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

  it('carries exact result icon kinds and falls back for injected unknown values', async () => {
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
        { resultId: 'calculator', title: '2', iconKind: 'calculator', activation: executeActivation },
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
      { title: '2', iconKind: 'calculator', icon: undefined, pluginIconUrl: undefined },
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
      pluginId: 'com.uipilot.demo-panel',
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
      expect(declaredProperty(statusRegion!, 'display')).toBe('none')
      expect(declaredProperty(surface!, 'grid-template-rows')).toBe('minmax(52px, 1fr)')
      expect(declaredProperty(tag!, 'border')).toMatch(/^0(?:px)?$/)
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

  it('syncs the panel host region bounds once per changed animation frame and cleans up observers', async () => {
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
        bounds: { x: 12, y: 64, width: 696, height: 320 },
      })

      observer!.callback([], observer as unknown as ResizeObserver)
      observer!.callback([], observer as unknown as ResizeObserver)
      window.dispatchEvent(new Event('resize'))
      expect(requestFrame).toHaveBeenCalledTimes(2)
      await flushFrames()
      expect(client.setPluginPanelBounds).toHaveBeenCalledTimes(1)

      bounds = { x: 8.5, y: 62.25, width: 703.5, height: 329.75 }
      observer!.callback([], observer as unknown as ResizeObserver)
      window.dispatchEvent(new Event('resize'))
      await flushFrames()
      expect(client.setPluginPanelBounds).toHaveBeenLastCalledWith({
        sessionEpoch: '1',
        bounds,
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
          pluginId: 'com.uipilot.demo-return',
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
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win' },
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
    expect(client.setPublicPluginFavorite).toHaveBeenCalledWith({
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
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win' },
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
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win' },
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
              target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-a' },
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
              target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-b' },
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
  it('keeps only strict bounded PNG data icons', async () => {
    const { core, client, emit } = await startedCore()
    const valid = `data:image/png;base64,${'A'.repeat(65_512)}`
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
      `data:image/png;base64,${'A'.repeat(65_516)}`,
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
  it('groups empty-query favorites without duplicating results and keeps nonempty searches flat', async () => {
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
            target: { kind: 'publicPlugin', pluginId: 'com.uipilot.favorite-plugin' },
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
      ] : [{ resultId: 'app-search', title: 'Matched App', activation: executeActivation }],
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
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(1))
    expect(mounted.host.querySelector('.result-section')).toBeNull()
    expect(mounted.host.querySelector('[role="option"]')?.textContent).toContain('Matched App')
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

  it('cycles launcher Tab focus only between the query input and settings button', async () => {
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
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-win' },
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
          target: { kind: 'publicPlugin', pluginId: 'com.uipilot.demo-return' },
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
    expect(fake.client.setPublicPluginFavorite).toHaveBeenCalledWith({
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
      .toBe('1 个结果 · 英译中 · 按 Enter 复制')
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
    expect(query?.value).toBe('')
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
      /\.settings-tabs \.ant-tabs-nav\s*\{[^}]*flex:\s*0 0 112px;[^}]*width:\s*112px;/s,
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

  it('uses the third-party overlay scrollbar for settings and detail panels', async () => {
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
    expect(launcherViewSource.match(/<OverlayScrollbarsComponent/g)).toHaveLength(3)
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

  it('renders built-in, public plugin, application, and fallback result icons', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockImplementation(async (request) => ({
      requestId: 'built-in-icons',
      items: request.query === 'icons' ? [
        findLauncherItem('icons'),
        { resultId: 'calculator', title: '2', iconKind: 'calculator', activation: executeActivation },
        { resultId: 'web', title: 'Bing 搜索', iconKind: 'webSearch', activation: executeActivation },
        {
          resultId: 'plugin',
          title: '/demo-win',
          activation: executeActivation,
          pluginIconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
        },
        { resultId: 'app', title: 'App', icon: 'data:image/png;base64,iVBORw==', activation: executeActivation },
        { resultId: 'fallback', title: 'Fallback', activation: executeActivation },
      ] : [],
    }))
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('built-in-icons')))
    await act(async () =>
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icons', inputType: 'insertText' }),
    )
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(6))

    const rows = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    const find = rows[0]!.querySelector<HTMLElement>('[data-result-icon-kind="find"]')
    const calculator = rows[1]!.querySelector<HTMLElement>('[data-result-icon-kind="calculator"]')
    const web = rows[2]!.querySelector<HTMLElement>('[data-result-icon-kind="webSearch"]')
    expect(find?.querySelector('.lucide-folder-search')).toBeTruthy()
    expect(calculator?.querySelector('.lucide-calculator')).toBeTruthy()
    expect(web?.querySelector('.lucide-panels-top-left')).toBeTruthy()
    expect(web?.querySelector('.lucide-search')).toBeTruthy()
    for (const icon of [find, calculator, web]) {
      expect(icon?.closest('.result-icon')?.getAttribute('aria-hidden')).toBe('true')
    }
    expect(rows[3]!.querySelector('.plugin-icon-image')).toBeInstanceOf(HTMLImageElement)
    expect(rows[4]!.querySelector('.result-icon-image')).toBeInstanceOf(HTMLImageElement)
    expect(rows[5]!.querySelector('.result-icon .app-mark:not([hidden])')).toBeTruthy()
    expect(stylesSource).toMatch(/\.built-in-result-icon\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;/s)
    expect(stylesSource).toMatch(/\.built-in-result-icon-badge\s*\{[^}]*position:\s*absolute;/s)
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
      const image = rows[1]!.querySelector<HTMLImageElement>('.result-icon-image')
      const fallback = rows[1]!.querySelector<HTMLElement>('.result-icon .app-mark')
      const siblingImage = rows[2]!.querySelector<HTMLImageElement>('.result-icon-image')
      const siblingFallback = rows[2]!.querySelector<HTMLElement>('.result-icon .app-mark')
      const missingImage = rows[3]!.querySelector<HTMLImageElement>('.result-icon-image')
      const missingFallback = rows[3]!.querySelector<HTMLElement>('.result-icon .app-mark')
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
      const nextRow = mounted.host.querySelectorAll<HTMLElement>('[role="option"]')[1]!
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
        findLauncherItem('app'),
        { resultId: 'private-one', title: '<b>literal</b>', activation: executeActivation },
        { resultId: 'private-two', title: '非常长的第二个应用名称', subtitle: 'Long subtitle value', activation: executeActivation },
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
    expect(options[0]!.textContent).toContain('/find')
    expect(options[0]!.textContent).toContain('搜索文件：app')
    expect(options[1]!.textContent).toContain('<b>literal</b>')
    expect(options[1]!.querySelector('b')).toBeNull()
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
      expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe('1 个结果。/find，搜索文件：missing'),
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
    expect(mounted.host.querySelector('.launcher-status-region')).toBeNull()
    const heading = mounted.host.querySelector<HTMLElement>('.settings-header h1')!
    expect(heading.textContent).toBe('设置')
    expect(heading.hasAttribute('tabindex')).toBe(false)
    const tabs = [...mounted.host.querySelectorAll<HTMLElement>('[role="tab"]')]
    expect(tabs).toEqual([
      settingsTab(mounted.host, '通用'),
      settingsTab(mounted.host, '消息'),
      settingsTab(mounted.host, '插件'),
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
      revision: '1',
      items: [{
        pluginId: 'com.example.demo', name: 'Demo', description: 'A compact plugin description that stays on one row.', version: '1.0.0',
        source: 'localPackage', defaultName: 'demo', effectiveName: 'demo', enabled: true,
        fault: null, generation: 1,
        iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.example.demo/1/icon.png',
        network: { httpsHosts: ['api.example.com'] },
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
    expect(row.querySelector('.plugin-title-line')?.textContent).toContain('·')
    expect(row.querySelector('.plugin-title-line')?.textContent).toContain('v1.0.0')
    expect(row.querySelector('.plugin-title-line')?.textContent).not.toContain('启动命令')
    expect(row.querySelector('.plugin-title-line')?.textContent).toContain('/demo')
    expect(row.querySelector('.plugin-description')?.textContent).toBe('A compact plugin description that stays on one row.')
    expect(row.querySelector('.public-name-control')).toBeNull()
    expect(row.querySelector('button[aria-label="查看插件详情"]')).not.toBeNull()
    expect(row.querySelector('button[aria-label="卸载插件"]')).not.toBeNull()
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
    expect(detail.textContent).toContain('启动键')
    expect(detail.textContent).not.toContain('启动名称')
    expect(detail.querySelector('.public-detail-name-term')).not.toBeNull()
    expect(detail.querySelector('.public-detail-name-value')).not.toBeNull()
    expect(stylesSource).toMatch(/\.public-plugin-detail-list\s*\{[^}]*padding:\s*20px 0 0 42px;/s)
    expect(stylesSource).toMatch(/\.public-detail-name-term,\s*\.public-detail-name-value\s*\{[^}]*align-self:\s*center;/s)
    expect(detail.textContent).toContain('版本号')
    expect(detail.textContent).toContain('1.0.0')
    expect(detail.textContent).toContain('插件说明')
    expect(detail.textContent).toContain('A compact plugin description that stays on one row.')
    expect(detail.textContent).toContain('权限列表')
    expect(detail.textContent).toContain('网络访问 · network.https · 未授权')
    expect(detail.textContent).toContain('剪贴板历史读取 · clipboard.history.read · 已授权')
    expect(detail.textContent).toContain('剪贴板历史粘贴 · clipboard.history.paste · 不支持')
    expect(detail.textContent).toContain('会在本机记录该插件可用的剪贴板历史摘要')
    expect(detail.textContent).not.toContain('clipboard.read 是 clipboard.history.read')
    expect(detail.textContent).toContain('网络 Host')
    expect(detail.textContent).toContain('api.example.com')
    expect(detail.textContent).toContain('插件所在目录')
    expect(detail.textContent).toContain('暂未提供插件目录')

    const nameInput = detail.querySelector<HTMLInputElement>('input[aria-label="启动键"]')!
    expect(nameInput.value).toBe('demo')
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      setter.call(nameInput, 'demo-custom')
      nameInput.dispatchEvent(new Event('input', { bubbles: true }))
      nameInput.dispatchEvent(new Event('change', { bubbles: true }))
      nameInput.dispatchEvent(new FocusEvent('focusout', { bubbles: true }))
    })
    await vi.waitFor(() => expect(fake.client.setPublicPluginEffectiveName).toHaveBeenCalledWith({
      pluginId: 'com.example.demo',
      nameOverride: 'demo-custom',
    }))

    await act(async () => detail.querySelector<HTMLButtonElement>('button[aria-label="恢复默认启动键"]')!.click())
    await vi.waitFor(() => expect(fake.client.setPublicPluginEffectiveName).toHaveBeenLastCalledWith({
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

  it('filters public plugins by name after a short debounce', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    vi.mocked(fake.client.listPublicPlugins).mockResolvedValue({
      revision: '1',
      items: [
        {
          pluginId: 'com.example.notes', name: 'Notes', description: null, version: '1.0.0',
          source: 'localPackage', defaultName: 'notes', effectiveName: 'notes', enabled: true,
          fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [],
        },
        {
          pluginId: 'com.example.translate', name: 'Translate', description: null, version: '1.0.0',
          source: 'localPackage', defaultName: 'translate', effectiveName: 'translate', enabled: true,
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
      const filter = mounted.host.querySelector<HTMLInputElement>('input[aria-label="筛选插件名称"]')
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
    } finally {
      vi.useRealTimers()
      await mounted.unmount()
      core.destroy()
    }
  })

  it('treats pending public owner cleanup as a committed uninstall', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const item = {
      pluginId: 'com.example.cleanup', name: 'Cleanup', description: null, version: '1.0.0',
      source: 'localPackage' as const, defaultName: 'cleanup', effectiveName: 'cleanup', enabled: true,
      fault: null, generation: 1, iconUrl: null, permissions: [], settings: [],
      network: null,
    }
    vi.mocked(fake.client.listPublicPlugins)
      .mockResolvedValueOnce({ revision: '1', items: [item] })
      .mockResolvedValueOnce({ revision: '2', items: [] })
    vi.mocked(fake.client.uninstallPublicPlugin).mockRejectedValueOnce({
      code: 'dataCleanupPending',
      message: 'private cleanup path',
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('public-cleanup-pending', 'settings')))
    await activateSettingsTab(mounted.host, '插件')
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-plugin-item')).not.toBeNull())

    const deleteButton = mounted.host
      .querySelector<HTMLButtonElement>('button[aria-label="卸载插件"]')!
    await act(async () => deleteButton.click())
    let confirm: HTMLButtonElement | undefined
    await vi.waitFor(() => {
      confirm = [...document.querySelectorAll<HTMLButtonElement>('button')]
        .find((button) => button.textContent?.replace(/\s/g, '') === '全部卸载')
      expect(confirm).toBeTruthy()
    })
    await act(async () => confirm!.click())

    await vi.waitFor(() => expect(fake.client.listPublicPlugins).toHaveBeenCalledTimes(2))
    expect(mounted.host.querySelector('.public-plugin-item')).toBeNull()
    expect(mounted.host.textContent).toContain('插件已卸载，数据清理将在下次启动时重试')
    expect(mounted.host.textContent).not.toContain('操作不可用')
    expect(mounted.host.textContent).not.toContain('private cleanup path')
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
      permissions: ['ui.window', 'clipboard.history.read', 'clipboard.history.paste'],
      isUpdate: false,
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
    await act(async () => chooseDirectory.click())
    await vi.waitFor(() => expect(mounted.host.querySelector('.public-prepare')).not.toBeNull())
    expect(mounted.host.querySelector<HTMLImageElement>('.public-prepare .plugin-icon-image')?.getAttribute('src'))
      .toBe('uipilot-public-plugin://localhost/__uipilot_icon/prepared/public-prepare-0000000000000001-0000000000000002/icon.png')
    const prepare = mounted.host.querySelector<HTMLElement>('.public-prepare')!
    expect(prepare.textContent).toContain('独立窗口 · ui.window')
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
      isUpdate: true,
      sourceVerified: false,
      iconUrl: null,
      network: {
        httpsHosts: ['api.example.com', 'auth.example.com'],
        addedHttpsHosts: ['auth.example.com'],
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
        isUpdate: false,
        network: {
          httpsHosts: ['api.example.com', 'auth.example.com'],
          addedHttpsHosts: ['api.example.com', 'auth.example.com'],
          requiresNetworkConsent: true,
        },
      })
      .mockResolvedValueOnce({
        ...base,
        token: 'public-prepare-0000000000000007-0000000000000008',
        version: '1.1.0',
        isUpdate: true,
        network: {
          httpsHosts: ['api.example.com'],
          addedHttpsHosts: [],
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
      source: 'localPackage' as const, defaultName: 'network', effectiveName: 'network', enabled: true,
      fault: null, generation: 1, iconUrl: null,
      network: { httpsHosts: ['api.example.com', 'auth.example.com'] },
      permissions: [{ permission: 'network.https' as const, supported: true, granted: true }],
      settings: [],
    }
    vi.mocked(fake.client.listPublicPlugins)
      .mockResolvedValueOnce({ revision: '1', items: [networkItem] })
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
  function resetAdapterDocument() {
    vi.resetModules()
    document.body.innerHTML = '<main id="app"></main>'
    installMatchMedia(false)
    tauriCapture.invoke.mockReset()
    tauriCapture.listen.mockReset()
  }

  async function pagehide() {
    await act(async () => window.dispatchEvent(new Event('pagehide')))
  }

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
    expect(order.slice(0, 9)).toEqual([
      'uipilot-plugin-panel-focus-host-input',
      'hotkey-recording://current',
      'launcher://shown',
      'launcher://hidden',
      'uipilot-plugin-panel-error',
      'uipilot-plugin-panel-reset',
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
    await main.client.loadSettings()
    await main.client.saveSettings({ settings: update })
    await main.client.setThemePreference({ preference: { theme: 'dark' } })
    await main.client.setPublicPluginFavorite({ pluginId: 'com.uipilot.demo-win', favorite: true })
    await main.client.setBuiltinFeatureFavorite({ feature: 'find', favorite: true })
    await main.client.openPluginPanel({ pluginId: 'com.uipilot.demo-panel', argument: 'hello' })
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
      ['load_settings', []],
      ['save_settings', [{ settings: update }]],
      ['set_theme_preference', [{ preference: { theme: 'dark' } }]],
      ['set_plugin_favorite', [{ pluginId: 'com.uipilot.demo-win', favorite: true }]],
      ['set_builtin_feature_favorite', [{ input: { feature: 'find', favorite: true } }]],
      ['open_plugin_panel', [{ input: { pluginId: 'com.uipilot.demo-panel', argument: 'hello' } }]],
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
    const messageUnlisten = vi.fn()
    const panelUnlisten = vi.fn()
    const panelResetUnlisten = vi.fn()
    const panelFocusUnlisten = vi.fn()
    const hotkeyRecordingUnlisten = vi.fn()
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
      if (event === 'uipilot-plugin-panel-error') return panelUnlisten
      if (event === 'uipilot-plugin-panel-reset') return panelResetUnlisten
      if (event === 'uipilot-plugin-panel-focus-host-input') return panelFocusUnlisten
      if (event === 'hotkey-recording://current') return hotkeyRecordingUnlisten
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
      expect(messageUnlisten).toHaveBeenCalledOnce()
      expect(panelUnlisten).toHaveBeenCalledOnce()
      expect(panelResetUnlisten).toHaveBeenCalledOnce()
      expect(panelFocusUnlisten).toHaveBeenCalledOnce()
      expect(hotkeyRecordingUnlisten).toHaveBeenCalledOnce()
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
    const unlistens = Array.from({ length: 7 }, () => vi.fn())
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
    expect(mainSource.match(/root\.unmount\(\)/g)).toHaveLength(3)
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
