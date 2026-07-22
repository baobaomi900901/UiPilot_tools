// @vitest-environment jsdom

// @ts-expect-error Vitest provides the Node standard library without project-wide Node types.
import { readFileSync } from 'node:fs'

import { describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { theme } from 'antd'

import { createLauncherCore } from './launcher-core'
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
  parseFileIndexChanged,
  parseFileSearchResponse,
  parseLauncherShown,
  type ClassifiedTextRecord,
  type ControlKey,
  type ExecuteOutcome,
  type ExportOutcome,
  type FileResultItem,
  type FileSearchResponse,
  type LauncherClient,
  type LauncherShown,
  type SearchResponse,
  type SettingsView,
} from './protocol'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import protocolSource from './protocol.ts?raw'

const stylesSource = readFileSync('src/styles.css', 'utf8')

const configCapture = vi.hoisted(() => ({ values: [] as unknown[] }))
const tauriCapture = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauriCapture.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: tauriCapture.listen }))

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
}

const settingsFixture: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
}

function fakeClient() {
  let shownHandler: ((payload: unknown) => void) | undefined
  let fileHandler: ((payload: unknown) => void) | undefined
  const unlisten = vi.fn()
  const fileUnlisten = vi.fn()
  const client: LauncherClient = {
    listenShown: vi.fn(async (handler) => {
      shownHandler = handler
      return unlisten
    }),
    listenFileIndexChanged: vi.fn(async (handler) => {
      fileHandler = handler
      return fileUnlisten
    }),
    searchApps: vi.fn(async () => null),
    searchFiles: vi.fn(async () => null),
    setFilePreviewPreference: vi.fn(async () => undefined),
    executeResult: vi.fn(async () => ({ status: 'launchRequested' }) satisfies ExecuteOutcome),
    loadSettings: vi.fn(async () => emptySettings),
    saveSettings: vi.fn(async () => undefined),
    saveHotkey: vi.fn(async (input: { hotkey: { hotkey: string } }) => ({ hotkey: input.hotkey.hotkey })),
    rescanApps: vi.fn(async () => undefined),
    exportValidationData: vi.fn(async () => ({ status: 'cancelled' }) satisfies ExportOutcome),
    clearValidationData: vi.fn(async () => undefined),
    hideLauncher: vi.fn(async () => undefined),
  }
  return {
    client,
    emit(payload: unknown) {
      if (!shownHandler) throw new Error('shown listener is not installed')
      shownHandler(payload)
    },
    emitFile(payload: unknown) {
      if (!fileHandler) throw new Error('file listener is not installed')
      fileHandler(payload)
    },
    unlisten,
    fileUnlisten,
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

function shown(invocationId: string, target: 'launcher' | 'settings' = 'launcher', notice: LauncherShown['notice'] = null) {
  return { invocationId, target, notice }
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

async function startedCore() {
  const fake = fakeClient()
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
    for (const target of ['launcher', 'settings'] as const) {
      for (const notice of [null, 'settingsFailed', 'validationFailed'] as const) {
        const value = shown('invocation', target, notice)
        expect(parseLauncherShown(value)).toEqual(value)
      }
    }

    for (const value of [
      null,
      [],
      {},
      { ...shown('x'), extra: true },
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

  it('blocks reload while startup hydration owns settings and permits retry after failure', async () => {
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
    expect(core.getSnapshot().status).toBe('设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。')

    const allowedRetry = core.reloadSettings()
    expect(fake.client.loadSettings).toHaveBeenCalledTimes(2)
    retry.resolve({ ...settingsFixture, autostart: true })
    await allowedRetry
    expect(core.getSnapshot().settings?.autostart).toBe(true)
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
    vi.mocked(fake.client.listenShown).mockReturnValueOnce(registration.promise)
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
    expect(fake.client.searchApps).toHaveBeenCalledWith({ query: 'calc', invocationId: 'launcher', querySequence: 1 })
  })
})

describe('shown and search ownership', () => {
  it('uses the exact shown reset and preserved-query search rules', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('first'))
    expect(client.searchApps).not.toHaveBeenCalled()
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    vi.mocked(client.searchApps).mockClear()

    emit(shown('second', 'launcher', 'settingsFailed'))
    expect(core.getSnapshot()).toMatchObject({
      invocationId: 'second',
      query: 'calc',
      queryControlValue: 'calc',
      querySequence: 1,
      selectedIndex: -1,
      shownNotice: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
    })
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(client.searchApps).toHaveBeenCalledWith({ query: 'calc', invocationId: 'second', querySequence: 1 })

    vi.mocked(client.searchApps).mockClear()
    emit(shown('settings', 'settings'))
    expect(client.searchApps).not.toHaveBeenCalled()
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
    expect(core.getSnapshot()).toMatchObject({ query: 'ab', querySequence: 2, results: [], searchPending: true, status: '' })
    first.resolve({ requestId: 'old-request', items: [{ resultId: 'old', title: 'old' }] })
    await first.promise
    await Promise.resolve()
    expect(core.getSnapshot()).not.toBe(beforeSecond)
    expect(core.getSnapshot().results).toEqual([])

    second.resolve({
      requestId: 'request',
      items: [
        { resultId: 'one', title: 'One' },
        { resultId: 'two', title: 'Two', subtitle: 'Second' },
      ],
    })
    await second.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['One', 'Two'])
    expect(core.getSnapshot().selectedIndex).toBe(0)
    core.keyDown('ArrowUp', false)
    expect(core.getSnapshot().selectedIndex).toBe(1)
    core.keyDown('ArrowDown', false)
    expect(core.getSnapshot().selectedIndex).toBe(0)

    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '', inputType: 'deleteContentBackward' })
    expect(core.getSnapshot()).toMatchObject({ query: '', querySequence: 3, results: [], selectedIndex: -1, searchPending: false, status: '' })
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
        { resultId: 'valid', title: 'Valid', icon: valid },
        ...invalid.map((icon, index) => ({ resultId: `bad-${index}`, title: `Bad ${index}`, icon })),
      ],
    })
    emit(shown('icons'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icon', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

    expect(core.getSnapshot().results[0]?.icon).toBe(valid)
    expect(core.getSnapshot().results.slice(1).every((item) => item.icon === undefined)).toBe(true)
  })
})

describe('execute and hide ownership', () => {
  it('executes the private current mapping once and never asks the frontend to hide on success', async () => {
    const { core, client, emit } = await startedCore()
    const search: SearchResponse = { requestId: 'private-request', items: [{ resultId: 'private-result', title: 'Calculator' }] }
    vi.mocked(client.searchApps).mockResolvedValueOnce(search)
    const execute = deferred<ExecuteOutcome>()
    vi.mocked(client.executeResult).mockReturnValueOnce(execute.promise)
    emit(shown('execute'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'calc', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private-request')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private-result')
    core.keyDown('Enter', false)
    core.keyDown('Enter', false)
    expect(client.executeResult).toHaveBeenCalledOnce()
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'private-request', resultId: 'private-result' })
    execute.resolve({ status: 'launchRequested' })
    await execute.promise
    await Promise.resolve()
    expect(client.hideLauncher).not.toHaveBeenCalled()
  })

  it('treats host-owned text copy as execute success without frontend hide', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'copy-request',
      items: [{ resultId: 'copy-result', title: 'Copy' }],
    })
    vi.mocked(client.executeResult).mockResolvedValueOnce({ status: 'textCopied' })
    emit(shown('copy'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'copy', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))

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
    search.resolve({ requestId: 'application-after-hide', items: [{ resultId: 'result', title: 'Calculator' }] })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
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
    expect(core.getSnapshot()).toMatchObject({ query: 'old', queryControlValue: 'old', querySequence: 1, searchPending: false, results: [] })
    core.text({ kind: 'compositionInput', control, value: '新', inputType: 'insertCompositionText' })
    core.text({ kind: 'compositionInput', control, value: 'old', inputType: 'insertCompositionText' })
    const returned = core.getSnapshot()
    old.resolve({ requestId: 'retired', items: [{ resultId: 'retired', title: 'Retired' }] })
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
    expect(core.getSnapshot()).toMatchObject({ query: 'calc', queryControlValue: 'calc', querySequence: 1, searchPending: true })
    core.text({ kind: 'compositionBoundary', control })
    core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
    expect(client.searchApps).toHaveBeenCalledTimes(2)
    old.resolve({ requestId: 'old', items: [{ resultId: 'old', title: 'Old' }] })
    current.resolve({ requestId: 'new', items: [{ resultId: 'new', title: 'New' }] })
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
    expect(core.getSnapshot()).toMatchObject({ query: '', queryControlValue: '', querySequence: 0, searchPending: false })
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
    expect(core.getSnapshot()).toMatchObject({ query: 'calc', queryControlValue: '\u6d4b\u8bd5', querySequence: 1 })
    expect(client.searchApps).not.toHaveBeenCalled()

    const boundary = r3({ kind: 'compositionBoundary', control })
    expect(Object.keys(boundary).sort()).toEqual(['control', 'kind'])
    core.text(boundary)
    expect(core.getSnapshot()).toMatchObject({ query: '\u6d4b\u8bd5', queryControlValue: '\u6d4b\u8bd5', querySequence: 2 })
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(client.searchApps).toHaveBeenCalledWith({ query: '\u6d4b\u8bd5', invocationId: 'r3-launcher', querySequence: 2 })

    const committed = core.getSnapshot()
    core.text(r3({ kind: 'ordinaryInput', control, value: '\u6d4b\u8bd5', inputType: 'insertText' }))
    core.text(boundary)
    expect(core.getSnapshot()).toBe(committed)
    expect(client.searchApps).toHaveBeenCalledOnce()
  })

  it('commits a settings draft locally and makes its same-value tail a no-op', async () => {
    const { core, client } = await startedSettingsCore()
    const control = core.getSnapshot().settings!.researchId.key
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    const listener = vi.fn()
    core.subscribe(listener)

    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(listener).toHaveBeenCalledOnce()
    expect(core.getSnapshot().settings!.researchId.value).toBe('\u6d4b\u8bd5')
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(client.saveSettings).not.toHaveBeenCalled()

    const committed = core.getSnapshot()
    listener.mockClear()
    core.text(r3({ kind: 'ordinaryInput', control, value: '\u6d4b\u8bd5', inputType: 'insertText' }))
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(committed)
    expect(listener).not.toHaveBeenCalled()

    core.text(r3({ kind: 'ordinaryInput', control, value: '\u4e0d\u540c', inputType: 'insertReplacementText' }))
    expect(core.getSnapshot().settings!.researchId.value).toBe('\u4e0d\u540c')
    expect(listener).toHaveBeenCalledOnce()
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(client.saveSettings).not.toHaveBeenCalled()
  })

  it('commits settings ordinary-before-end and cancel paths once with zero Rust calls', async () => {
    const { core, client } = await startedSettingsCore()
    const control = core.getSnapshot().settings!.researchId.key
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
    expect(core.getSnapshot().settings!.researchId.value).toBe('alph')

    listener.mockClear()
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: 'ordinary-first', inputType: 'insertCompositionText' }))
    core.text(r3({ kind: 'ordinaryInput', control, value: 'ordinary-first', inputType: 'insertText' }))
    const ordinary = core.getSnapshot()
    core.text(r3({ kind: 'compositionBoundary', control }))
    expect(core.getSnapshot()).toBe(ordinary)
    expect(core.getSnapshot().settings!.researchId.value).toBe('ordinary-first')
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
    expect(client.searchApps).toHaveBeenCalledWith({ query: 'cal', invocationId: 'cancel', querySequence: 2 })
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

  it('restores an unfinished draft once and keeps exact-value edits idempotent', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('idempotent'))
    const control = core.getSnapshot().queryControl
    core.text(r3({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' }))
    vi.mocked(client.searchApps).mockClear()
    core.text(r3({ kind: 'compositionStart', control }))
    core.text(r3({ kind: 'compositionInput', control, value: '\u6d4b\u8bd5', inputType: 'insertCompositionText' }))
    const listener = vi.fn()
    core.subscribe(listener)

    core.text(r3({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' }))
    expect(listener).toHaveBeenCalledOnce()
    expect(client.searchApps).not.toHaveBeenCalled()
    const restored = core.getSnapshot()
    listener.mockClear()
    core.text(r3({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertFromPaste' }))
    expect(core.getSnapshot()).toBe(restored)
    expect(listener).not.toHaveBeenCalled()

    vi.mocked(client.searchApps).mockResolvedValueOnce({ requestId: 'old-empty', items: [] })
    emit(shown('idempotent-rerun', 'launcher', 'validationFailed'))
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot()).toMatchObject({
      query: 'calc',
      querySequence: 1,
      results: [],
      selectedIndex: -1,
      shownNotice: '本地验证数据操作失败。',
    })

    const rerun = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(rerun.promise)
    const searchCalls = vi.mocked(client.searchApps).mock.calls.length
    core.keyDown('Enter', false)
    expect(core.getSnapshot()).toMatchObject({
      query: 'calc',
      querySequence: 2,
      results: [],
      selectedIndex: -1,
      searchPending: true,
      status: '',
    })
    expect(core.getSnapshot().shownNotice).toBeUndefined()
    expect(client.searchApps).toHaveBeenCalledTimes(searchCalls + 1)
    expect(client.searchApps).toHaveBeenLastCalledWith({ query: 'calc', invocationId: 'idempotent-rerun', querySequence: 2 })
    expect(client.executeResult).not.toHaveBeenCalled()

    core.keyDown('Enter', false)
    expect(client.searchApps).toHaveBeenCalledTimes(searchCalls + 1)
    expect(client.executeResult).not.toHaveBeenCalled()
    rerun.resolve(null)
    await rerun.promise
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))

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
  async function settingsCore() {
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('settings', 'settings'))
    return { core, ...fake }
  }

  it('saves exact hotkey, autostart, and research ID', async () => {
    const { core, client } = await settingsCore()
    const settings = core.getSnapshot().settings!
    core.setHotkeyCanonical('Ctrl+Space')
    core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
    core.setAutostart(true)
    await core.saveSettings()
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'Ctrl+Space',
        autostart: true,
        researchId: 'research_1',
      },
    })
  })

  it('preserves edits and fails closed after a save error', async () => {
    const { core, client } = await settingsCore()
    const researchId = core.getSnapshot().settings!.researchId
    core.text({ kind: 'ordinaryInput', control: researchId.key, value: 'research_1', inputType: 'insertText' })
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })
    await core.saveSettings()
    expect(client.loadSettings).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().settings).toMatchObject({ readOnly: true, needsReload: true })
    expect(core.getSnapshot().settings!.researchId.value).toBe('research_1')
    expect(core.getSnapshot().status).toBe('设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
  })

  it('retires form controls before fresh replacement', async () => {
    const { core, client } = await settingsCore()
    const original = core.getSnapshot().settings!
    const oldKeys = [original.hotkey.key, original.researchId.key]
    core.text({ kind: 'compositionStart', control: original.researchId.key })
    core.text({ kind: 'compositionInput', control: original.researchId.key, value: 'uncommitted', inputType: 'insertCompositionText' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await core.reloadSettings()
    const replacement = core.getSnapshot().settings!
    const replacedSnapshot = core.getSnapshot()
    core.text({ kind: 'compositionBoundary', control: original.researchId.key })
    core.text({ kind: 'compositionInput', control: original.researchId.key, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(replacedSnapshot)
    const newKeys = [replacement.hotkey.key, replacement.researchId.key]
    expect(Math.min(...newKeys)).toBeGreaterThan(Math.max(...oldKeys))

    const replaceStart = launcherCoreSource.indexOf('function replaceSettings')
    const replaceRetire = launcherCoreSource.indexOf('retireControl(control.key)', replaceStart)
    const replaceAssign = launcherCoreSource.indexOf('model.settings =', replaceStart)
    expect(replaceRetire).toBeGreaterThan(replaceStart)
    expect(replaceAssign).toBeGreaterThan(replaceRetire)
  })

  it('marks a stale save for explicit reload without a follow-up call', async () => {
    const { core, client, emit } = await settingsCore()
    const save = deferred<void>()
    vi.mocked(client.saveSettings).mockReturnValueOnce(save.promise)
    const pending = core.saveSettings()
    emit(shown('new-settings', 'settings'))
    save.resolve()
    await pending
    expect(client.loadSettings).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
  })

  it('keeps one global settings operation and makes stale rescan require reload', async () => {
    const { core, client, emit } = await settingsCore()
    const rescan = deferred<void>()
    vi.mocked(client.rescanApps).mockReturnValueOnce(rescan.promise)
    const pending = core.rescanApps()
    void core.saveSettings()
    void core.exportValidation()
    core.beginClearValidation()
    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(client.exportValidationData).not.toHaveBeenCalled()
    expect(core.getSnapshot().settings).toMatchObject({ operation: 'rescan', clearConfirmation: false })
    emit(shown('stale-rescan', 'settings'))
    rescan.resolve()
    await pending
    expect(client.loadSettings).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
  })

  it('also makes a stale rejected rescan require reload', async () => {
    const { core, client, emit } = await settingsCore()
    const rescan = deferred<void>()
    vi.mocked(client.rescanApps).mockReturnValueOnce(rescan.promise)
    const pending = core.rescanApps()
    emit(shown('stale-rescan-error', 'settings'))
    rescan.reject({ code: 'scanFailed', message: 'raw' })
    await pending
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
    expect(core.getSnapshot().status).toBe('')
  })

  it('clears a shown notice on a settings text edit', async () => {
    const { core, emit } = await settingsCore()
    emit(shown('settings-notice', 'settings', 'validationFailed'))
    expect(core.getSnapshot().shownNotice).toBe('本地验证数据操作失败。')
    core.setHotkeyCanonical('Ctrl+Space')
    expect(core.getSnapshot().shownNotice).toBeUndefined()
  })

  it('records hotkey via canonical setter without saving', async () => {
    const { core, client } = await settingsCore()
    core.setHotkeyCanonical('DoubleCtrl')
    expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl')
    expect(client.saveSettings).not.toHaveBeenCalled()
  })

  it('records hotkey through dedicated save without saving other drafts', async () => {
    const { core, client } = await settingsCore()
    const settings = core.getSnapshot().settings!
    core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
    core.setAutostart(true)

    await core.saveHotkeyCanonical('DoubleCtrl')

    expect(client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } })
    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(core.getSnapshot().settings!.hotkey.value).toBe('DoubleCtrl')
    expect(core.getSnapshot().settings!.researchId.value).toBe('research_1')
    expect(core.getSnapshot().settings!.autostart).toBe(true)
  })

  it('records DoubleCtrl from the settings hotkey input', async () => {
    installMatchMedia(false)
    const { core, client } = await settingsCore()
    const mounted = await mountLauncherView(core)
    const settings = core.getSnapshot().settings!
    const input = mounted.host.querySelector<HTMLInputElement>(`input[name="settings-hotkey-${settings.hotkey.key}"]`)
    if (!input) throw new Error('settings hotkey input missing')

    await act(async () => input.focus())
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
      input.dispatchEvent(new KeyboardEvent('keyup', { key: 'Control', code: 'ControlLeft', bubbles: true, cancelable: true }))
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true, cancelable: true }))
    })

    expect(client.saveHotkey).toHaveBeenCalledWith({ hotkey: { hotkey: 'DoubleCtrl' } })
    await mounted.unmount()
  })

  it('restores durable hotkey and preserves other drafts after dedicated save failure', async () => {
    const { core, client } = await settingsCore()
    const settings = core.getSnapshot().settings!
    core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
    vi.mocked(client.saveHotkey).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })

    await core.saveHotkeyCanonical('DoubleCtrl')

    expect(core.getSnapshot().settings!.hotkey.value).toBe('Alt+Space')
    expect(core.getSnapshot().settings!.researchId.value).toBe('research_1')
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
  })

  it('keeps one settings operation while dedicated hotkey save is pending', async () => {
    const { core, client } = await settingsCore()
    const pendingHotkey = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)

    const pending = core.saveHotkeyCanonical('DoubleCtrl')
    void core.saveSettings()
    void core.saveHotkeyCanonical('DoubleAlt')

    expect(client.saveHotkey).toHaveBeenCalledOnce()
    expect(client.saveSettings).not.toHaveBeenCalled()
    expect(core.getSnapshot().settings).toMatchObject({ operation: 'hotkey' })
    pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
    await pending
  })

  it('does not let stale dedicated hotkey response overwrite a newer settings view', async () => {
    const { core, client, emit } = await settingsCore()
    const pendingHotkey = deferred<{ hotkey: string }>()
    vi.mocked(client.saveHotkey).mockReturnValueOnce(pendingHotkey.promise)

    const pending = core.saveHotkeyCanonical('DoubleCtrl')
    emit(shown('new-settings', 'settings'))
    pendingHotkey.resolve({ hotkey: 'DoubleCtrl' })
    await pending

    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
  })

  it('save persists DoubleCtrl through save_settings payload', async () => {
    const { core, client } = await settingsCore()
    core.setHotkeyCanonical('DoubleCtrl')
    await core.saveSettings()
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'DoubleCtrl',
        autostart: false,
      },
    })
  })

  it('keeps rescan failure editable but fails closed when its reload fails', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.rescanApps).mockRejectedValueOnce({ code: 'scanFailed', message: 'raw' })
    await core.rescanApps()
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: false, readOnly: false })
    expect(core.getSnapshot().status).toBe('重新扫描失败。')

    vi.mocked(client.rescanApps).mockResolvedValueOnce(undefined)
    vi.mocked(client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'raw' })
    await core.rescanApps()
    expect(core.getSnapshot().settings).toMatchObject({ needsReload: true, readOnly: true })
    expect(core.getSnapshot().status).toBe('设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。')
  })

  it('runs rescan reload, export, and inline clear with one settings owner', async () => {
    const { core, client } = await settingsCore()
    vi.mocked(client.loadSettings).mockResolvedValueOnce({ ...settingsFixture, autostart: true })
    await core.rescanApps()
    expect(client.rescanApps).toHaveBeenCalledOnce()
    expect(client.loadSettings).toHaveBeenCalledTimes(2)
    expect(core.getSnapshot().settings?.autostart).toBe(true)

    vi.mocked(client.exportValidationData).mockResolvedValueOnce({ status: 'exported' })
    await core.exportValidation()
    expect(client.exportValidationData).toHaveBeenCalledOnce()

    core.beginClearValidation()
    expect(core.getSnapshot().settings?.clearConfirmation).toBe(true)
    core.cancelClearValidation()
    expect(core.getSnapshot().settings?.clearConfirmation).toBe(false)
    core.beginClearValidation()
    await core.confirmClearValidation()
    expect(client.clearValidationData).toHaveBeenCalledOnce()
    expect(core.getSnapshot().settings?.clearConfirmation).toBe(false)
  })
})

describe('execute and hide continuation', () => {
  it('coalesces a late activation-refused result into the next eligible launcher notice', async () => {
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({ requestId: 'request', items: [{ resultId: 'result', title: 'App' }] })
    const execute = deferred<ExecuteOutcome>()
    vi.mocked(client.executeResult).mockReturnValueOnce(execute.promise)
    emit(shown('execute-old'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'app', inputType: 'insertText' })
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    core.keyDown('Enter', false)
    emit(shown('settings-new', 'settings'))
    execute.resolve({ status: 'activationRefusedLaunchRequested', message: 'raw backend text' })
    await execute.promise
    await Promise.resolve()
    emit(shown('notice-priority', 'launcher', 'validationFailed'))
    expect(core.getSnapshot().shownNotice).toBe('本地验证数据操作失败。')
    emit(shown('eligible'))
    expect(core.getSnapshot().shownNotice).toBe('Windows 拒绝了前台切换，已发送启动请求')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('raw backend')
  })
})

describe('React view and accessibility', () => {
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

  it('uses native app regions without invoking Tauri mouse capture', () => {
    expect(launcherViewSource).not.toContain('data-tauri-drag-region')
    expect(stylesSource).toMatch(
      /\.launcher-surface,[\s\S]*\.status-region\s*\{[^}]*app-region:\s*drag;/,
    )
    expect(stylesSource).toMatch(
      /button,[\s\S]*\.settings-form\s*\{[^}]*app-region:\s*no-drag;/,
    )
    expect(stylesSource).toMatch(/\.result-list:empty\s*\{[^}]*app-region:\s*drag;/)
  })

  it('keeps launcher chrome separated and gives scrolling only to results', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const style = document.createElement('style')
    style.textContent = stylesSource
    document.head.append(style)
    const { core, client, emit } = await startedCore()
    vi.mocked(client.searchApps).mockResolvedValueOnce({
      requestId: 'layout',
      items: [{ resultId: 'layout-icon', title: 'Layout', icon: 'data:image/png;base64,iVBORw==' }],
    })
    const mounted = await mountLauncherView(core)
    mounted.host.id = 'app'
    try {
      await act(async () => emit(shown('layout')))
      await act(async () =>
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'layout', inputType: 'insertText' }),
      )
      await vi.waitFor(() => expect(mounted.host.querySelector('.result-icon')).toBeInstanceOf(HTMLElement))
      const app = mounted.host.querySelector<HTMLElement>(':scope > .ant-app')!
      const surface = app.querySelector<HTMLElement>('.launcher-surface')!
      const launcher = surface.querySelector<HTMLElement>('.launcher-view')!
      const spinRoot = launcher.querySelector<HTMLElement>(':scope > .ant-spin')!
      const spinContainer = spinRoot.querySelector<HTMLElement>('.ant-spin-container')!
      const results = spinContainer.querySelector<HTMLElement>('.result-list')!
      const icon = results.querySelector<HTMLElement>('.result-icon')!
      const image = icon.querySelector<HTMLImageElement>('.result-icon-image')!
      const status = surface.querySelector<HTMLElement>('.status-region')!
      const normalized = (value: string) => value.replace(/\s+/g, ' ').trim()
      const isZero = (value: string) => /^0(?:px)?$/.test(value)

      expect(getComputedStyle(app).height).toBe('100%')
      expect(normalized(getComputedStyle(surface).gridTemplateRows)).toBe('minmax(52px, 1fr) minmax(24px, auto)')
      expect(normalized(getComputedStyle(launcher).gridTemplateRows)).toBe('44px minmax(0, 1fr)')
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

  it('keeps the slim result scrollbar visible without hover', () => {
    expect(stylesSource).toMatch(/\.result-list\s*\{[^}]*--result-scrollbar-thumb:\s*rgba\(64, 64, 64, 0\.48\);/s)
    expect(stylesSource).toMatch(/\.result-list::-webkit-scrollbar\s*\{[^}]*width:\s*6px;/s)
    expect(stylesSource).toMatch(/\.result-list::-webkit-scrollbar-track\s*\{[^}]*background:\s*transparent;/s)
    expect(stylesSource).toMatch(
      /\.result-list::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*var\(--result-scrollbar-thumb\);[^}]*border-radius:\s*3px;/s,
    )
    expect(stylesSource).not.toMatch(/\.result-list:hover::-webkit-scrollbar-thumb/)
    expect(stylesSource).toMatch(
      /@media \(prefers-color-scheme: dark\)[\s\S]*\.result-list\s*\{[^}]*--result-scrollbar-thumb:\s*rgba\(217, 217, 217, 0\.55\);/s,
    )
    expect(stylesSource).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.result-list::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*ButtonText;/s,
    )
  })

  it('shows real icons, falls back on error, and resets the error for a new src', async () => {
    installMatchMedia(false)
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    const fake = fakeClient()
    const firstIcon = 'data:image/png;base64,iVBORw=='
    const siblingIcon = 'data:image/png;base64,QUJDRA=='
    const secondIcon = 'data:image/png;base64,iVBORw0K'
    vi.mocked(fake.client.searchApps)
      .mockResolvedValueOnce({
        requestId: 'first-icons',
        items: [
          { resultId: 'with-icon', title: 'With icon', icon: firstIcon },
          { resultId: 'sibling-icon', title: 'Sibling icon', icon: siblingIcon },
          { resultId: 'without-icon', title: 'Without icon' },
        ],
      })
      .mockResolvedValueOnce({
        requestId: 'second-icons',
        items: [{ resultId: 'new-icon', title: 'New icon', icon: secondIcon }],
      })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    try {
      await act(async () => fake.emit(shown('icon-view')))
      await act(async () =>
        core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'icon', inputType: 'insertText' }),
      )
      await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(3))

      const rows = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
      const image = rows[0]!.querySelector<HTMLImageElement>('.result-icon-image')
      const fallback = rows[0]!.querySelector<HTMLElement>('.result-icon .app-mark')
      const siblingImage = rows[1]!.querySelector<HTMLImageElement>('.result-icon-image')
      const siblingFallback = rows[1]!.querySelector<HTMLElement>('.result-icon .app-mark')
      const missingImage = rows[2]!.querySelector<HTMLImageElement>('.result-icon-image')
      const missingFallback = rows[2]!.querySelector<HTMLElement>('.result-icon .app-mark')
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
      const nextImage = mounted.host.querySelector<HTMLImageElement>('.result-icon-image')!
      const nextFallback = mounted.host.querySelector<HTMLElement>('.result-icon .app-mark')!
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
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({
      requestId: 'private-request',
      items: [
        { resultId: 'private-one', title: '<b>literal</b>' },
        { resultId: 'private-two', title: '非常长的第二个应用名称', subtitle: 'Long subtitle value' },
      ],
    })
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
    await vi.waitFor(() => expect(mounted.host.querySelectorAll('[role="option"]')).toHaveLength(2))
    const options = [...mounted.host.querySelectorAll<HTMLElement>('[role="option"]')]
    expect(mounted.host.querySelector('[role="listbox"]')?.id).toBe('launcher-results')
    expect(input.getAttribute('aria-expanded')).toBe('true')
    expect(options[0]!.getAttribute('aria-selected')).toBe('true')
    expect(options[0]!.textContent).toContain('<b>literal</b>')
    expect(options[0]!.querySelector('b')).toBeNull()
    expect(mounted.host.innerHTML).not.toContain('private-request')
    expect(mounted.host.innerHTML).not.toContain('private-one')
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toContain('2 个结果')

    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    expect(document.activeElement).toBe(input)
    expect(input.getAttribute('aria-activedescendant')).toBe(options[1]!.id)
    expect(scroll).toHaveBeenCalledWith({ block: 'nearest' })
    await mounted.unmount()
  })

  it('keeps empty startup quiet, announces no results, and gives composing Escape to IME', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.searchApps).mockResolvedValueOnce({ requestId: 'empty', items: [] })
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(input.disabled).toBe(true)
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe('')
    await act(async () => fake.emit(shown('empty-results')))
    expect(input.disabled).toBe(false)
    await act(async () => core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: 'missing', inputType: 'insertText' }))
    await vi.waitFor(() => expect(mounted.host.querySelector('[role="status"]')?.textContent).toBe('未找到应用'))
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

  it('renders settings controls and closes only through the core hide owner', async () => {
    installMatchMedia(true)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('settings-view', 'settings')))
    const heading = mounted.host.querySelector<HTMLElement>('h1')!
    expect(heading.textContent).toBe('设置')
    expect(document.activeElement).toBe(heading)
    expect(mounted.host.querySelector('input[maxlength="64"][pattern="[A-Za-z0-9_-]{1,64}"]')).toBeTruthy()
    const close = mounted.host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!
    expect(close.getAttribute('aria-label')).toBe('关闭')
    await act(async () => close.click())
    expect(fake.client.hideLauncher).toHaveBeenCalledOnce()
    expect(core.getSnapshot().view).toBe('settings')
    await mounted.unmount()
  })

  it('shows fixed settings load failure and retry without a permanent spinner', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'raw backend' })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('settings-failure', 'settings'))
    const mounted = await mountLauncherView(core)
    expect(mounted.host.querySelector('[role="status"]')?.textContent).toContain('设置未能确认完成')
    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeNull()
    expect([...mounted.host.querySelectorAll('button')].some((button) => button.textContent?.includes('重新加载设置'))).toBe(true)
    expect(mounted.host.textContent).not.toContain('raw backend')
    await mounted.unmount()
  })

  it('shows only loading during startup hydration and enables retry after failure', async () => {
    installMatchMedia(false)
    const fake = fakeClient()
    const initial = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(initial.promise).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
    fake.emit(shown('settings-loading', 'settings'))
    const mounted = await mountLauncherView(core)
    const retryButton = () =>
      [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find((button) => button.textContent?.includes('重新加载设置'))

    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeTruthy()
    expect(retryButton()).toBeUndefined()

    initial.reject({ code: 'settingsFailed', message: 'private' })
    await act(async () => start)
    expect(mounted.host.querySelector('.ant-spin-spinning')).toBeNull()
    expect(retryButton()).toBeTruthy()

    await act(async () => retryButton()!.click())
    await vi.waitFor(() => expect(core.getSnapshot().settings).toBeDefined())
    expect(fake.client.loadSettings).toHaveBeenCalledTimes(2)
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
    expect(client.searchApps).toHaveBeenCalledWith({ query: '计算器', invocationId: 'stable-binding', querySequence: 1 })

    await mounted.unmount()
    expect(unbind).toHaveBeenCalledOnce()
    expect(retire).toHaveBeenCalledOnce()
    bind.mockRestore()
  })

  it('unbinds and retires old settings controls before a form replacement', async () => {
    installMatchMedia(false)
    const cleanup: string[] = []
    const bind = vi.spyOn(nativeInput, 'bindNativeTextInput').mockImplementation((_input, control) => () => {
      cleanup.push(`native-unbind:${control}`)
    })
    const fake = fakeClient()
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    const core = createLauncherCore(fake.client)
    const originalRetire = core.retireControl
    vi.spyOn(core, 'retireControl').mockImplementation((control) => {
      cleanup.push(`retire:${control}`)
      originalRetire(control)
    })
    await core.start()
    const mounted = await mountLauncherView(core)
    await act(async () => fake.emit(shown('replacement-view', 'settings')))
    const oldResearchId = core.getSnapshot().settings!.researchId.key
    cleanup.length = 0
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await act(async () => core.reloadSettings())
    const unbindIndex = cleanup.indexOf(`native-unbind:${oldResearchId}`)
    const retireIndex = cleanup.indexOf(`retire:${oldResearchId}`)
    expect(unbindIndex).toBeGreaterThanOrEqual(0)
    expect(retireIndex).toBeGreaterThan(unbindIndex)
    await mounted.unmount()
    bind.mockRestore()
  })

  it('keeps the React/AntD source boundary exact', () => {
    for (const required of ['ConfigProvider', 'App', 'Input', 'Form', 'Checkbox', 'Button', 'Alert', 'Spin', 'theme']) {
      expect(launcherViewSource).toContain(required)
    }
    for (const forbidden of [
      '@tauri-apps/api',
      '@ant-design/icons',
      'AutoComplete',
      'Select',
      'Card',
      'Modal',
      'Popconfirm',
      'dangerouslySetInnerHTML',
      'appId',
    ]) {
      expect(launcherViewSource).not.toContain(forbidden)
    }
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

  it('mounts and resolves the shown listener before loading, then uses the exact invoke table', async () => {
    resetAdapterDocument()
    const registration = deferred<() => void>()
    const load = deferred<SettingsView>()
    const unlisten = vi.fn()
    const order: string[] = []
    let shownHandler: ((event: { payload: unknown }) => void) | undefined
    tauriCapture.listen.mockImplementation((event, handler) => {
      expect(document.querySelector('[role="combobox"]')).toBeInstanceOf(HTMLInputElement)
      order.push(String(event))
      shownHandler = handler as (event: { payload: unknown }) => void
      return registration.promise
    })
    tauriCapture.invoke.mockImplementation((command) => {
      order.push(String(command))
      return command === 'load_settings' ? load.promise : Promise.resolve(undefined)
    })

    let main!: { client: LauncherClient }
    await act(async () => {
      main = (await import('./main')) as unknown as { client: LauncherClient }
    })
    await vi.waitFor(() => expect(tauriCapture.listen).toHaveBeenCalledWith('launcher://shown', expect.any(Function)))
    expect(tauriCapture.invoke).not.toHaveBeenCalled()
    registration.resolve(unlisten)
    await vi.waitFor(() => expect(tauriCapture.invoke).toHaveBeenCalledWith('load_settings'))
    expect(order.slice(0, 2)).toEqual(['launcher://shown', 'load_settings'])

    await act(async () => shownHandler?.({ payload: shown('during-adapter-load', 'settings') }))
    expect(document.querySelector('.settings-view h1')?.textContent).toBe('设置')
    await act(async () => {
      load.resolve(emptySettings)
      await load.promise
    })

    tauriCapture.invoke.mockClear()
    const update = { hotkey: 'Alt+Space', autostart: false }
    await main.client.searchApps({ query: 'calc', invocationId: 'inv-1', querySequence: 1 })
    await main.client.executeResult({ requestId: 'req-1', resultId: 'result-1' })
    await main.client.loadSettings()
    await main.client.saveSettings({ settings: update })
    await main.client.rescanApps()
    await main.client.exportValidationData()
    await main.client.clearValidationData()
    await main.client.hideLauncher()
    const invokeRows = [
      ['search_apps', [{ query: 'calc', invocationId: 'inv-1', querySequence: 1 }]],
      ['execute_result', [{ requestId: 'req-1', resultId: 'result-1' }]],
      ['load_settings', []],
      ['save_settings', [{ settings: update }]],
      ['rescan_apps', []],
      ['export_validation_data', []],
      ['clear_validation_data', []],
      ['hide_launcher', []],
    ] as const
    expect(tauriCapture.invoke.mock.calls.map(([command, ...args]) => [command, args])).toEqual(invokeRows)
    await pagehide()
  })

  it('fails locally and never listens or loads when native input binding fails', async () => {
    resetAdapterDocument()
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
      expect(tauriCapture.listen).not.toHaveBeenCalled()
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
      expect(tauriCapture.listen).not.toHaveBeenCalled()
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
    const unlisten = vi.fn()
    let shownHandler: ((event: { payload: unknown }) => void) | undefined
    let mountedCore: ReturnType<typeof createLauncherCore> | undefined
    let throwFatal = false
    vi.doMock('./launcher-view', async () => {
      const React = await vi.importActual<typeof import('react')>('react')
      return {
        LauncherView: ({ core, onReady }: { core: ReturnType<typeof createLauncherCore>; onReady: (result: 'ready') => void }) => {
          mountedCore = core
          const snapshot = React.useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
          React.useLayoutEffect(() => onReady('ready'), [onReady])
          if (throwFatal) throw new Error(privateError)
          return React.createElement('div', null, snapshot.status)
        },
      }
    })
    tauriCapture.listen.mockImplementation(async (_event, handler) => {
      shownHandler = handler as (event: { payload: unknown }) => void
      return unlisten
    })
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(command === 'load_settings' ? emptySettings : command === 'search_apps' ? null : undefined),
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
      await vi.waitFor(() => expect(unlisten).toHaveBeenCalledOnce())
      await vi.waitFor(() => expect(document.querySelector('.status-region')?.textContent).toBe('操作不可用，请重试。'))
      expect(document.body.textContent).not.toContain(privateError)
      expect(JSON.stringify(consoleError.mock.calls)).not.toContain(privateError)

      shownHandler?.({ payload: shown('after-fatal') })
      await Promise.resolve()
      expect(tauriCapture.invoke.mock.calls.filter(([command]) => command === 'search_apps')).toHaveLength(searchCalls)
      await pagehide()
      await pagehide()
      expect(unlisten).toHaveBeenCalledOnce()
    } finally {
      await pagehide()
      vi.doUnmock('./launcher-view')
      vi.resetModules()
      consoleError.mockRestore()
    }
  })

  it('tears down once and keeps the production adapter source narrow', async () => {
    resetAdapterDocument()
    const unlisten = vi.fn()
    tauriCapture.listen.mockResolvedValueOnce(unlisten)
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
    expect(unlisten).toHaveBeenCalledOnce()
    expect(remove.mock.calls.map(([event]) => event)).toEqual(
      expect.arrayContaining(['compositionstart', 'input', 'compositionend']),
    )
    expect(document.querySelector('#app')?.childElementCount).toBe(0)
    await pagehide()
    expect(unlisten).toHaveBeenCalledOnce()
    expect(remove).toHaveBeenCalledTimes(removed)
    remove.mockRestore()

    for (const command of [
      'search_apps',
      'execute_result',
      'load_settings',
      'save_settings',
      'rescan_apps',
      'export_validation_data',
      'clear_validation_data',
      'hide_launcher',
    ]) {
      expect(mainSource.match(new RegExp(`['"]${command}['"]`, 'g'))).toHaveLength(1)
    }
    expect(mainSource.match(/['"]launcher:\/\/shown['"]/g)).toHaveLength(1)
    expect(mainSource).not.toMatch(/@tauri-apps\/api\/(?:window|webviewWindow)/)
    expect(mainSource).not.toContain('.hide(')
    expect(mainSource).not.toMatch(/\b(?:path|pid|hwnd|appId)\b/i)
    expect(mainSource.indexOf('core.destroy()')).toBeLessThan(mainSource.indexOf('root.unmount()'))
    expect(mainSource.match(/root\.unmount\(\)/g)).toHaveLength(1)
  })
})

describe('file protocol', () => {
  it('strictly parses exact file responses and revision events', () => {
    const response = fileResponse('18446744073709551615', [
      fileItem(),
      {
        resultId: 'folder-result',
        name: 'Folder',
        kind: 'folder',
        sizeBytes: null,
        modifiedUtc: '2026-07-22T00:00:01Z',
        fullPath: String.raw`C:\Private\Folder`,
      },
    ])
    expect(parseFileSearchResponse(response)).toEqual(response)
    expect(parseFileIndexChanged({ revision: '9', status: 'partial' })).toEqual({ revision: '9', status: 'partial' })
  })

  it('rejects extra missing inherited malformed decimal date and enum values as a whole', () => {
    const valid = fileResponse('7')
    const hiddenExtra = Object.defineProperty({ ...valid }, 'hidden', { value: true })
    const symbolExtraItem = { ...valid.items[0], [Symbol('extra')]: true }
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
      { ...valid, items: [{ ...valid.items[0], modifiedUtc: '2026-02-31T00:00:00Z' }] },
      Object.assign(Object.create({ inherited: true }), valid),
      [valid],
      { ...valid, items: Array(1) },
      { ...valid, items: Object.assign([...valid.items], { extra: true }) },
      hiddenExtra,
      { ...valid, items: [symbolExtraItem] },
    ]
    for (const value of invalid) expect(parseFileSearchResponse(value)).toBeNull()
    for (const value of [
      { revision: '01', status: 'ready' },
      { revision: '1', status: 'ready', extra: true },
      Object.assign(Object.create({ inherited: true }), { revision: '1', status: 'ready' }),
      { revision: '1', status: 'unknown' },
    ]) {
      expect(parseFileIndexChanged(value)).toBeNull()
    }
  })

  it('keeps the frozen file category and sort unions in source', () => {
    for (const literal of ['all', 'folder', 'excel', 'word', 'ppt', 'pdf', 'image', 'video', 'audio', 'archive']) {
      expect(protocolSource).toContain(`'${literal}'`)
    }
    expect(protocolSource).toContain("'modifiedDesc' | 'modifiedAsc'")
    expect(protocolSource).not.toMatch(/Number\((?:revision|total|sizeBytes)/)
  })
})

describe('launcher real file adapter', () => {
  it('uses one exact file listener and exact camelCase invoke payloads', async () => {
    vi.resetModules()
    document.body.innerHTML = '<main id="app"></main>'
    installMatchMedia(false)
    tauriCapture.invoke.mockReset()
    tauriCapture.listen.mockReset()
    const shownUnlisten = vi.fn()
    const fileUnlisten = vi.fn()
    tauriCapture.listen.mockImplementation((event) =>
      Promise.resolve(event === 'file-index://changed' ? fileUnlisten : shownUnlisten),
    )
    tauriCapture.invoke.mockImplementation((command) =>
      Promise.resolve(command === 'load_settings' ? emptySettings : command === 'search_files' ? null : undefined),
    )

    const main = (await import('./main')) as unknown as { client: LauncherClient }
    const handler = vi.fn()
    const release = await main.client.listenFileIndexChanged(handler)
    await main.client.searchFiles({
      query: 'UiPilot',
      category: 'all',
      sort: 'modifiedDesc',
      invocationId: 'inv-file',
      querySequence: 2,
      privateExtra: 'must-not-cross-wire',
    } as Parameters<LauncherClient['searchFiles']>[0])
    await main.client.setFilePreviewPreference({
      preference: { enabled: false, privateExtra: 'must-not-cross-wire' },
      privateOuter: 'must-not-cross-wire',
    } as Parameters<LauncherClient['setFilePreviewPreference']>[0])

    expect(tauriCapture.listen).toHaveBeenCalledWith('file-index://changed', expect.any(Function))
    expect(tauriCapture.invoke).toHaveBeenCalledWith('search_files', {
      query: 'UiPilot',
      category: 'all',
      sort: 'modifiedDesc',
      invocationId: 'inv-file',
      querySequence: 2,
    })
    expect(tauriCapture.invoke).toHaveBeenCalledWith('set_file_preview_preference', {
      preference: { enabled: false },
    })
    release()
    expect(fileUnlisten).toHaveBeenCalledOnce()
    window.dispatchEvent(new Event('pagehide'))
  })

  it('keeps exactly ten commands two events and no window or payload logging', () => {
    for (const command of [
      'search_apps',
      'search_files',
      'execute_result',
      'load_settings',
      'save_settings',
      'set_file_preview_preference',
      'rescan_apps',
      'export_validation_data',
      'clear_validation_data',
      'hide_launcher',
    ]) {
      expect(mainSource.match(new RegExp(`['"]${command}['"]`, 'g'))).toHaveLength(1)
    }
    for (const event of ['launcher://shown', 'file-index://changed']) {
      expect(mainSource.match(new RegExp(`['"]${event}['"]`, 'g'))).toHaveLength(1)
    }
    expect(mainSource).not.toMatch(/@tauri-apps\/api\/(?:window|webviewWindow)/)
    expect(mainSource.match(/event\.payload/g)).toHaveLength(2)
    expect(mainSource).not.toMatch(/console\.|JSON\.stringify\(event/)
  })
})

describe('file mode ownership', () => {
  it('enters only on exact non-composing slash command and continues the same sequence', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('same-invocation'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find UiPilot', inputType: 'insertText' })
    expect(fake.client.searchApps).toHaveBeenLastCalledWith({
      query: '/find UiPilot',
      invocationId: 'same-invocation',
      querySequence: 1,
    })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledOnce())
    expect(fake.client.searchFiles).toHaveBeenCalledWith({
      query: 'UiPilot',
      category: 'all',
      sort: 'modifiedDesc',
      invocationId: 'same-invocation',
      querySequence: 2,
    })
    expect(core.getSnapshot().file?.results[0]).toEqual({
      key: String.raw`C:\Private\UiPilot.txt`,
      name: 'UiPilot.txt',
      kind: 'file',
      sizeBytes: '42',
      modifiedUtc: '2026-07-22T00:00:00.000Z',
      fullPath: String.raw`C:\Private\UiPilot.txt`,
    })
    expect(core.getSnapshot().file?.results[0]).not.toHaveProperty('resultId')
    expect(core.getSnapshot().file?.selected).toBe(core.getSnapshot().file?.results[0])
    expect(Object.keys(core.getSnapshot().file!.results[0]!).sort()).toEqual([
      'fullPath',
      'key',
      'kind',
      'modifiedUtc',
      'name',
      'sizeBytes',
    ])
    expect(Object.isFrozen(core.getSnapshot().file)).toBe(true)
    expect(Object.isFrozen(core.getSnapshot().file!.results)).toBe(true)
    expect(Object.isFrozen(core.getSnapshot().file!.results[0])).toBe(true)

    fake.emit(shown('next-show'))
    expect(core.getSnapshot().querySequence).toBe(0)
    core.text({ kind: 'ordinaryInput', control, value: '/finder', inputType: 'insertText' })
    core.keyDown('Enter', false)
    expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
  })

  it('registers before empty search and listener failure performs zero file calls', async () => {
    const fake = fakeClient()
    const order: string[] = []
    vi.mocked(fake.client.listenFileIndexChanged).mockImplementationOnce(async () => {
      order.push('listen')
      return fake.fileUnlisten
    })
    vi.mocked(fake.client.searchFiles).mockImplementationOnce(async () => {
      order.push('search')
      return fileResponse('0', [], 'building')
    })
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('empty-file'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(order).toEqual(['listen', 'search']))
    expect(fake.client.searchFiles).toHaveBeenCalledWith(expect.objectContaining({ query: '' }))

    const rejected = fakeClient()
    vi.mocked(rejected.client.listenFileIndexChanged).mockRejectedValueOnce(new Error('private listener failure'))
    const rejectedCore = createLauncherCore(rejected.client)
    await rejectedCore.start()
    rejected.emit(shown('listener-failure'))
    rejectedCore.text({
      kind: 'ordinaryInput',
      control: rejectedCore.getSnapshot().queryControl,
      value: '/find ',
      inputType: 'insertText',
    })
    rejectedCore.keyDown('Enter', false)
    await Promise.resolve()
    expect(rejected.client.searchFiles).not.toHaveBeenCalled()
  })

  it('executes the selected private file result without exposing its result id', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('4'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('file-execute'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find result', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.selected).toBeDefined())
    expect(core.getSnapshot().file?.selected).not.toHaveProperty('resultId')
    core.keyDown('Enter', false)
    expect(fake.client.executeResult).toHaveBeenCalledWith({
      requestId: 'file-request-4',
      resultId: 'file-result-1',
    })
  })

  it('holds edits category and sort behind the pending first listener', async () => {
    const fake = fakeClient()
    const registration = deferred<() => void>()
    vi.mocked(fake.client.listenFileIndexChanged).mockReturnValueOnce(registration.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('pending-listener'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find initial', inputType: 'insertText' })
    core.keyDown('Enter', false)
    core.text({ kind: 'ordinaryInput', control, value: 'latest', inputType: 'insertText' })
    core.setFileCategory('pdf')
    core.setFileSort('modifiedAsc')
    expect(fake.client.searchFiles).not.toHaveBeenCalled()

    registration.resolve(fake.fileUnlisten)
    await vi.waitFor(() =>
      expect(fake.client.searchFiles).toHaveBeenCalledWith({
        query: 'latest',
        category: 'pdf',
        sort: 'modifiedAsc',
        invocationId: 'pending-listener',
        querySequence: 2,
      }),
    )
  })

  it('keeps snapshots immutable and rolls preview preference back on failure', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('immutable-file'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.results).toHaveLength(1))
    const before = core.getSnapshot()
    const beforeFile = before.file
    const beforeResults = before.file?.results
    core.keyDown('ArrowDown', false)
    expect(core.getSnapshot()).toBe(before)
    expect(core.getSnapshot().file).toBe(beforeFile)
    expect(core.getSnapshot().file?.results).toBe(beforeResults)

    vi.mocked(fake.client.setFilePreviewPreference).mockRejectedValueOnce({ code: 'settingsFailed' })
    core.setFilePreviewEnabled(false)
    expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: true })
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: true, preferencePending: false }),
    )
    expect(core.getSnapshot().status).toBe('无法保存文件预览设置。')
  })
  it('keeps one preview write pending across views and applies its durable success', async () => {
    const fake = fakeClient()
    const preference = deferred<void>()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    vi.mocked(fake.client.setFilePreviewPreference).mockReturnValueOnce(preference.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('preview-first'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file).toBeDefined())
    core.setFilePreviewEnabled(false)
    fake.emit(shown('preview-settings', 'settings'))
    expect(core.getSnapshot().file).toBeUndefined()
    fake.emit(shown('preview-next'))
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: true }),
    )
    core.setFilePreviewEnabled(true)
    expect(fake.client.setFilePreviewPreference).toHaveBeenCalledOnce()
    preference.resolve()
    await preference.promise
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: false }),
    )
    fake.emit(shown('preview-final'))
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(false))
  })

  it('rolls one cross-view preview write back on failure without issuing a second write', async () => {
    const fake = fakeClient()
    const preference = deferred<void>()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    vi.mocked(fake.client.setFilePreviewPreference).mockReturnValueOnce(preference.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('preview-failure-old'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(true))
    core.setFilePreviewEnabled(false)
    fake.emit(shown('preview-failure-new'))
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: true }),
    )
    core.setFilePreviewEnabled(true)
    expect(fake.client.setFilePreviewPreference).toHaveBeenCalledOnce()
    preference.reject({ code: 'settingsFailed' })
    await preference.promise.catch(() => undefined)
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: true, preferencePending: false }),
    )
    fake.emit(shown('preview-failure-final'))
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(true))
  })

  it('does not let an older settings load overwrite a completed preview preference', async () => {
    const fake = fakeClient()
    const loaded = deferred<SettingsView>()
    vi.mocked(fake.client.loadSettings).mockReturnValueOnce(loaded.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const core = createLauncherCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.loadSettings).toHaveBeenCalledOnce())
    fake.emit(shown('late-load-old'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(true))
    core.setFilePreviewEnabled(false)
    await vi.waitFor(() =>
      expect(core.getSnapshot().file).toMatchObject({ previewEnabled: false, preferencePending: false }),
    )

    loaded.resolve(emptySettings)
    await starting
    expect(core.getSnapshot().file?.previewEnabled).toBe(false)
    fake.emit(shown('late-load-next'))
    core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.previewEnabled).toBe(false))
  })

  it('lets category and sort replace older file owners without accepting stale rows', async () => {
    const fake = fakeClient()
    const category = deferred<FileSearchResponse | null>()
    vi.mocked(fake.client.searchFiles)
      .mockResolvedValueOnce(fileResponse('1', [fileItem(String.raw`C:\Private\Initial.txt`, 'initial')]))
      .mockReturnValueOnce(category.promise)
      .mockResolvedValueOnce(fileResponse('3', [fileItem(String.raw`C:\Private\Sorted.txt`, 'sorted')]))
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('filter-owner'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find filter', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().file?.results[0]?.name).toBe('Initial.txt'))
    core.setFileCategory('pdf')
    core.setFileSort('modifiedAsc')
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith({
      query: 'filter',
      category: 'pdf',
      sort: 'modifiedAsc',
      invocationId: 'filter-owner',
      querySequence: 4,
    })
    await vi.waitFor(() => expect(core.getSnapshot().file?.results[0]?.name).toBe('Sorted.txt'))
    category.resolve(fileResponse('2', [fileItem(String.raw`C:\Private\Stale.txt`, 'stale')]))
    await category.promise
    await Promise.resolve()
    expect(core.getSnapshot().file?.results[0]?.name).toBe('Sorted.txt')
  })
})

describe('file index refresh', () => {
  it('accepts newer revisions only and coalesces trailing refresh with a one second maximum', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      vi.mocked(fake.client.searchFiles)
        .mockResolvedValueOnce(fileResponse('1', [fileItem(String.raw`C:\Private\A.txt`, 'a')]))
        .mockResolvedValue(fileResponse('3', [fileItem(String.raw`C:\Private\A.txt`, 'a')]))
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('refresh-owner'))
      core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/find', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(1))
      const stable = core.getSnapshot()

      fake.emitFile({ revision: '1', status: 'ready' })
      fake.emitFile({ revision: '2', status: 'ready' })
      await vi.advanceTimersByTimeAsync(249)
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
      fake.emitFile({ revision: '3', status: 'partial' })
      await vi.advanceTimersByTimeAsync(250)
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(2)
      expect(core.getSnapshot()).not.toBe(stable)

      for (const revision of ['4', '5', '6', '7', '8']) {
        fake.emitFile({ revision, status: 'ready' })
        await vi.advanceTimersByTimeAsync(200)
      }
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(3)
    } finally {
      vi.useRealTimers()
    }
  })

  it('preserves selection by full path and rejects stale response event view and query owners', async () => {
    const fake = fakeClient()
    const first = deferred<FileSearchResponse | null>()
    const refresh = deferred<FileSearchResponse | null>()
    vi.mocked(fake.client.searchFiles).mockReturnValueOnce(first.promise).mockReturnValueOnce(refresh.promise)
    const core = createLauncherCore(fake.client)
    await core.start()
    fake.emit(shown('selection-owner'))
    core.text({ kind: 'ordinaryInput', control: core.getSnapshot().queryControl, value: '/find', inputType: 'insertText' })
    core.keyDown('Enter', false)
    first.resolve(
      fileResponse('1', [
        fileItem(String.raw`C:\Private\A.txt`, 'a'),
        fileItem(String.raw`C:\Private\B.txt`, 'b'),
      ]),
    )
    await vi.waitFor(() => expect(core.getSnapshot().file?.results).toHaveLength(2))
    core.keyDown('ArrowDown', false)
    expect(core.getSnapshot().file?.selected?.fullPath).toBe(String.raw`C:\Private\B.txt`)
    fake.emitFile({ revision: '2', status: 'ready' })
    await new Promise((resolve) => setTimeout(resolve, 260))
    refresh.resolve(
      fileResponse('2', [
        fileItem(String.raw`C:\Private\B.txt`, 'b'),
        fileItem(String.raw`C:\Private\C.txt`, 'c'),
      ]),
    )
    await vi.waitFor(() => expect(core.getSnapshot().file?.selected?.fullPath).toBe(String.raw`C:\Private\B.txt`))

    const after = core.getSnapshot()
    fake.emitFile({ revision: '2', status: 'ready' })
    fake.emit(shown('replacement-view', 'settings'))
    await new Promise((resolve) => setTimeout(resolve, 260))
    expect(core.getSnapshot().file).toBeUndefined()
    expect(core.getSnapshot()).not.toBe(after)
  })

  it('cancels a pending refresh when the query owner changes', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('refresh-cancel'))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find first', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(1))
      fake.emitFile({ revision: '2', status: 'ready' })
      core.text({ kind: 'ordinaryInput', control, value: 'second', inputType: 'insertText' })
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(2)
      await vi.advanceTimersByTimeAsync(1_000)
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('cancels a pending refresh when the current response already covers its revision', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      const current = deferred<FileSearchResponse | null>()
      vi.mocked(fake.client.searchFiles).mockReturnValueOnce(current.promise)
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('refresh-covered'))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find covered', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledOnce())
      fake.emitFile({ revision: '2', status: 'ready' })
      current.resolve(fileResponse('3'))
      await current.promise
      await Promise.resolve()
      await vi.advanceTimersByTimeAsync(1_000)
      expect(fake.client.searchFiles).toHaveBeenCalledOnce()
      expect(core.getSnapshot().file?.indexStatus).toBe('ready')
    } finally {
      vi.useRealTimers()
    }
  })

  it('invalidates refresh and response owners at hide and unlistens once at destroy', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      const stale = deferred<FileSearchResponse | null>()
      const hidden = deferred<void>()
      vi.mocked(fake.client.searchFiles).mockReturnValueOnce(stale.promise)
      vi.mocked(fake.client.hideLauncher).mockReturnValueOnce(hidden.promise)
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('hide-owner'))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find stale', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledOnce())
      fake.emitFile({ revision: '2', status: 'partial' })
      const hiding = core.requestHide()
      expect(core.getSnapshot().file).toBeUndefined()
      await vi.advanceTimersByTimeAsync(1_000)
      expect(fake.client.searchFiles).toHaveBeenCalledOnce()
      stale.resolve(fileResponse('2'))
      hidden.resolve()
      await hiding
      expect(core.getSnapshot().file).toBeUndefined()
      core.destroy()
      core.destroy()
      expect(fake.fileUnlisten).toHaveBeenCalledOnce()
      expect(fake.unlisten).toHaveBeenCalledOnce()
    } finally {
      vi.useRealTimers()
    }
  })

  it('hides exactly once instead of sending an imprecise file sequence', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
    const hidden = deferred<void>()
    vi.mocked(fake.client.hideLauncher).mockReturnValueOnce(hidden.promise)
    const core = createLauncherCore(fake.client, 2)
    await core.start()
    fake.emit(shown('sequence-overflow'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: '/find overflow', inputType: 'insertText' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledWith(expect.objectContaining({ querySequence: 2 })))
    core.setFileCategory('pdf')
    core.setFileSort('modifiedAsc')
    expect(fake.client.hideLauncher).toHaveBeenCalledOnce()
    expect(fake.client.searchFiles).toHaveBeenCalledOnce()
    expect(core.getSnapshot().file).toBeUndefined()
    hidden.resolve()
  })

  it('executes the current request before a scheduled refresh replaces its mapping', async () => {
    vi.useFakeTimers()
    try {
      const fake = fakeClient()
      vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse('1'))
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown('refresh-enter'))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find execute', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(core.getSnapshot().file?.selected).toBeDefined())
      fake.emitFile({ revision: '2', status: 'ready' })
      core.keyDown('Enter', false)
      expect(fake.client.executeResult).toHaveBeenCalledWith({
        requestId: 'file-request-1',
        resultId: 'file-result-1',
      })
      await vi.advanceTimersByTimeAsync(250)
      expect(fake.client.searchFiles).toHaveBeenCalledTimes(2)
      expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ querySequence: 3 }))
    } finally {
      vi.useRealTimers()
    }
  })

  it('file execute outcomes are path-free success', async () => {
    for (const outcome of [
      { status: 'fileRevealRequested' },
      { status: 'folderOpenRequested' },
    ] as const) {
      const fake = fakeClient()
      vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('1'))
      vi.mocked(fake.client.executeResult).mockResolvedValueOnce(outcome)
      const core = createLauncherCore(fake.client)
      await core.start()
      fake.emit(shown(`file-execute-${outcome.status}`))
      const control = core.getSnapshot().queryControl
      core.text({ kind: 'ordinaryInput', control, value: '/find report', inputType: 'insertText' })
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(core.getSnapshot().file?.selected).toBeDefined())
      core.keyDown('Enter', false)
      await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
      expect(core.getSnapshot().status).toBe('')
      expect(JSON.stringify(outcome)).not.toMatch(/[A-Za-z]:\\|fullPath|path/i)
      core.destroy()
    }

    expect(protocolSource).toContain("{ status: 'fileRevealRequested' }")
    expect(protocolSource).toContain("{ status: 'folderOpenRequested' }")
  })
})

describe('file panel accessibility', () => {
  it('renders file categories results and preview without leaking private result ids', async () => {
    installMatchMedia(false)
    const first = fileItem(String.raw`C:\Private\Quarterly Report.pdf`, 'secret-file-id')
    const second = folderItem(String.raw`C:\Private\Reports`, 'secret-folder-id')
    const { core, mounted, client } = await startedFileView([first, second])
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    expect(mounted.host.querySelector('.file-workspace')).toBeTruthy()
    expect(input.getAttribute('aria-controls')).toBe('file-results')
    expect(input.getAttribute('aria-expanded')).toBe('true')
    expect(document.activeElement).toBe(input)

    const tabs = [...mounted.host.querySelectorAll<HTMLElement>('[role="tab"]')]
    expect(tabs.map((tab) => tab.textContent?.replaceAll(' ', ''))).toEqual([
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
    expect(tabs.filter((tab) => tab.tabIndex === 0)).toHaveLength(1)
    await act(async () => tabs[5]!.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    expect(client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'pdf' }))

    const options = [...mounted.host.querySelectorAll<HTMLElement>('#file-results [role="option"]')]
    expect(options).toHaveLength(2)
    expect(options[0]!.id).toBe('file-result-option-0')
    expect(options[0]!.tabIndex).toBe(-1)
    expect(input.getAttribute('aria-activedescendant')).toBe(options[0]!.id)
    expect(mounted.host.innerHTML).not.toContain('secret-file-id')
    expect(mounted.host.innerHTML).not.toContain('secret-folder-id')

    await act(async () => options[1]!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })))
    expect(document.activeElement).toBe(input)
    expect(core.getSnapshot().file?.selected?.fullPath).toBe(String.raw`C:\Private\Reports`)
    expect(mounted.host.querySelector('.file-preview')?.textContent).toContain(String.raw`C:\Private\Reports`)
    expect(mounted.host.querySelector('.file-preview')?.textContent).toContain('--')
    await mounted.unmount()
  })

  it('keeps the query input as the only result focus owner and executes selected files from rows', async () => {
    installMatchMedia(false)
    const { core, mounted, client } = await startedFileView([
      fileItem(String.raw`C:\Private\A.txt`, 'a'),
      fileItem(String.raw`C:\Private\B.txt`, 'b'),
    ])
    const input = mounted.host.querySelector<HTMLInputElement>('[role="combobox"]')!
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    expect(document.activeElement).toBe(input)
    expect(input.getAttribute('aria-activedescendant')).toBe('file-result-option-1')

    const second = mounted.host.querySelector<HTMLElement>('#file-result-option-1')!
    await act(async () => second.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    expect(client.executeResult).toHaveBeenCalledWith({ requestId: 'file-request-1', resultId: 'b' })
    expect(core.getSnapshot().file?.selected?.fullPath).toBe(String.raw`C:\Private\B.txt`)
    await mounted.unmount()
  })
})

describe('file panel responsive layout', () => {
  it('keeps the file UI in one scoped responsive surface without extra component families', () => {
    expect(launcherViewSource).toContain('className="file-workspace"')
    expect(launcherViewSource).toContain("import {")
    expect(launcherViewSource).not.toContain('@ant-design/icons')
    const antdImport = launcherViewSource.slice(0, launcherViewSource.indexOf("} from 'antd'"))
    for (const forbidden of ['AutoComplete', 'Select', 'Card', 'Modal', 'Popconfirm']) {
      expect(antdImport).not.toContain(forbidden)
    }
    expect(stylesSource).toContain('.file-workspace')
    expect(stylesSource).toContain('grid-template-areas')
    expect(stylesSource).toContain('.file-category-strip')
    expect(stylesSource).toContain('.file-preview')
    expect(stylesSource).toContain('@media (max-width: 600px)')
    expect(stylesSource).toContain('@media (forced-colors: active)')
    expect(stylesSource).toContain('overflow-wrap: anywhere')
    expect(stylesSource).toContain('overflow-x: hidden')
  })
})

describe('file preview preference', () => {
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
    await act(async () => core.text({ kind: 'ordinaryInput', control, value: '/find', inputType: 'insertText' }))
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
