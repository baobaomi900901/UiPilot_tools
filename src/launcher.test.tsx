// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { theme } from 'antd'

import { createLauncherCore } from './launcher-core'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import launcherCoreSource from './launcher-core.ts?raw'
import { bindNativeTextInput } from './native-input'
import * as nativeInput from './native-input'
import { LauncherView } from './launcher-view'
// @ts-expect-error Vite supplies the raw source module in Vitest.
import launcherViewSource from './launcher-view.tsx?raw'
import {
  parseLauncherShown,
  type ExecuteOutcome,
  type ExportOutcome,
  type LauncherClient,
  type LauncherShown,
  type SearchResponse,
  type SettingsView,
} from './protocol'

const configCapture = vi.hoisted(() => ({ values: [] as unknown[] }))

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
  applications: [],
}

const settingsFixture: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  applications: [
    { appId: 'private-app-id-a', displayName: '同名应用', aliases: ['alpha'] },
    { appId: 'private-app-id-b', displayName: '同名应用', aliases: [] },
  ],
}

function fakeClient() {
  let shownHandler: ((payload: unknown) => void) | undefined
  const unlisten = vi.fn()
  const client: LauncherClient = {
    listenShown: vi.fn(async (handler) => {
      shownHandler = handler
      return unlisten
    }),
    searchApps: vi.fn(async () => null),
    executeResult: vi.fn(async () => ({ status: 'launchRequested' }) satisfies ExecuteOutcome),
    loadSettings: vi.fn(async () => emptySettings),
    saveSettings: vi.fn(async () => undefined),
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
    unlisten,
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
})

describe('IME ownership', () => {
  it.each([
    ['input-before-end', true],
    ['end-before-input', false],
  ])('finalizes %s exactly once', async (_name, inputFirst) => {
    const { core, client, emit } = await startedCore()
    emit(shown('ime'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control, value: '' })
    core.text({ kind: 'compositionUpdate', control, value: '计' })
    expect(core.getSnapshot()).toMatchObject({ query: '', queryControlValue: '计', querySequence: 0 })
    expect(client.searchApps).not.toHaveBeenCalled()
    if (inputFirst) core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
    core.text({ kind: 'compositionEnd', control, value: '计算器' })
    if (!inputFirst) {
      await Promise.resolve()
      core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
    }
    expect(client.searchApps).toHaveBeenCalledOnce()
    expect(client.searchApps).toHaveBeenCalledWith({ query: '计算器', invocationId: 'ime', querySequence: 1 })
    expect(core.getSnapshot()).toMatchObject({ query: '计算器', queryControlValue: '计算器', querySequence: 1 })
  })

  it('commits an empty final value with zero Rust calls', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('empty-ime'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control, value: '' })
    core.text({ kind: 'compositionUpdate', control, value: '中' })
    core.text({ kind: 'compositionEnd', control, value: '' })
    expect(client.searchApps).not.toHaveBeenCalled()
    expect(core.getSnapshot()).toMatchObject({ query: '', queryControlValue: '', querySequence: 1, searchPending: false })
  })

  it('permanently retires the pre-composition search even when draft text returns', async () => {
    const { core, client, emit } = await startedCore()
    const old = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(old.promise)
    emit(shown('retire-search'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: 'old', inputType: 'insertText' })
    core.text({ kind: 'compositionStart', control, value: 'old' })
    expect(core.getSnapshot()).toMatchObject({ query: 'old', queryControlValue: 'old', querySequence: 1, searchPending: false, results: [] })
    core.text({ kind: 'compositionUpdate', control, value: '新' })
    core.text({ kind: 'compositionUpdate', control, value: 'old' })
    const returned = core.getSnapshot()
    old.resolve({ requestId: 'retired', items: [{ resultId: 'retired', title: 'Retired' }] })
    await old.promise
    await Promise.resolve()
    expect(core.getSnapshot()).toBe(returned)
    expect(core.getSnapshot().results).toEqual([])
  })

  it('lets only the new shown auto-search commit across a late old end and input', async () => {
    const { core, client, emit } = await startedCore()
    const old = deferred<SearchResponse | null>()
    const current = deferred<SearchResponse | null>()
    vi.mocked(client.searchApps).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise)
    emit(shown('old-invocation'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'ordinaryInput', control, value: 'calc', inputType: 'insertText' })
    core.text({ kind: 'compositionStart', control, value: 'calc' })
    core.text({ kind: 'compositionUpdate', control, value: '计算' })
    emit(shown('new-invocation'))
    expect(core.getSnapshot()).toMatchObject({ query: 'calc', queryControlValue: 'calc', querySequence: 1, searchPending: true })
    core.text({ kind: 'compositionEnd', control, value: '计算器' })
    core.text({ kind: 'compositionInput', control, value: '计算器', inputType: 'insertCompositionText' })
    expect(client.searchApps).toHaveBeenCalledTimes(2)
    old.resolve({ requestId: 'old', items: [{ resultId: 'old', title: 'Old' }] })
    current.resolve({ requestId: 'new', items: [{ resultId: 'new', title: 'New' }] })
    await Promise.all([old.promise, current.promise])
    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().results.map((item) => item.title)).toEqual(['New'])
  })

  it('suppresses one associated composition input but treats later ordinary input as a new edit', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('marker'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control, value: '' })
    core.text({ kind: 'compositionEnd', control, value: '中文' })
    await Promise.resolve()
    core.text({ kind: 'compositionInput', control, value: '中文', inputType: 'insertCompositionText' })
    expect(client.searchApps).toHaveBeenCalledTimes(1)
    core.text({ kind: 'ordinaryInput', control, value: '中文', inputType: 'insertFromPaste' })
    expect(client.searchApps).toHaveBeenCalledTimes(2)
    expect(core.getSnapshot().querySequence).toBe(2)
  })

  it('does not indefinitely suppress a same-value ordinary retry when no post-end input arrives', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('retry'))
    const control = core.getSnapshot().queryControl
    core.text({ kind: 'compositionStart', control, value: '' })
    core.text({ kind: 'compositionEnd', control, value: 'same' })
    core.text({ kind: 'ordinaryInput', control, value: 'same', inputType: 'insertText' })
    expect(client.searchApps).toHaveBeenCalledTimes(2)
    expect(core.getSnapshot().querySequence).toBe(2)
  })

  it('retires active, suppression, and stale ownership idempotently', async () => {
    const { core, client, emit } = await startedCore()
    emit(shown('retire-control'))
    const control = core.getSnapshot().queryControl
    const listener = vi.fn()
    core.subscribe(listener)

    core.text({ kind: 'compositionStart', control, value: '' })
    listener.mockClear()
    core.retireControl(control)
    core.retireControl(control)
    const retiredActive = core.getSnapshot()
    core.text({ kind: 'compositionEnd', control, value: 'late' })
    core.text({ kind: 'compositionInput', control, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(retiredActive)
    expect(listener).not.toHaveBeenCalled()
    expect(client.searchApps).not.toHaveBeenCalled()

    core.text({ kind: 'compositionStart', control, value: '' })
    core.text({ kind: 'compositionEnd', control, value: 'done' })
    core.retireControl(control)
    const retiredMarker = core.getSnapshot()
    core.text({ kind: 'compositionInput', control, value: 'done', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(retiredMarker)

    core.text({ kind: 'compositionStart', control, value: 'done' })
    emit(shown('replacement'))
    core.retireControl(control)
    const retiredTombstone = core.getSnapshot()
    core.text({ kind: 'compositionEnd', control, value: 'stale' })
    core.text({ kind: 'compositionInput', control, value: 'stale', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(retiredTombstone)
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

  it('projects all current applications with local keys and saves the complete private map', async () => {
    const { core, client } = await settingsCore()
    const settings = core.getSnapshot().settings
    expect(settings?.applications.map((application) => [application.displayName, application.aliases.map((alias) => alias.value)])).toEqual([
      ['同名应用 (1)', ['alpha']],
      ['同名应用 (2)', ['']],
    ])
    expect(settings?.applications[0]?.key).not.toBe(settings?.applications[1]?.key)
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private-app-id')

    const second = settings!.applications[1]!
    core.text({ kind: 'ordinaryInput', control: second.aliases[0]!.key, value: 'beta', inputType: 'insertText' })
    await core.saveSettings()
    expect(client.saveSettings).toHaveBeenCalledOnce()
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'Alt+Space',
        autostart: false,
        aliases: { 'private-app-id-a': ['alpha'], 'private-app-id-b': ['beta'] },
      },
    })
  })

  it('saves exact hotkey, autostart, research ID, and ordered aliases', async () => {
    const { core, client } = await settingsCore()
    const settings = core.getSnapshot().settings!
    core.text({ kind: 'ordinaryInput', control: settings.hotkey.key, value: 'Ctrl+Space', inputType: 'insertText' })
    core.text({ kind: 'ordinaryInput', control: settings.researchId.key, value: 'research_1', inputType: 'insertText' })
    core.setAutostart(true)
    const second = settings.applications[1]!
    core.text({ kind: 'ordinaryInput', control: second.aliases[0]!.key, value: 'beta', inputType: 'insertText' })
    core.addAlias(second.key)
    const added = core.getSnapshot().settings!.applications[1]!.aliases[1]!
    core.text({ kind: 'ordinaryInput', control: added.key, value: 'beta-two', inputType: 'insertText' })
    await core.saveSettings()
    expect(client.saveSettings).toHaveBeenCalledWith({
      settings: {
        hotkey: 'Ctrl+Space',
        autostart: true,
        researchId: 'research_1',
        aliases: { 'private-app-id-a': ['alpha'], 'private-app-id-b': ['beta', 'beta-two'] },
      },
    })
  })

  it('preserves edits and fails closed after a save error', async () => {
    const { core, client } = await settingsCore()
    const alias = core.getSnapshot().settings!.applications[1]!.aliases[0]!
    core.text({ kind: 'ordinaryInput', control: alias.key, value: 'beta', inputType: 'insertText' })
    vi.mocked(client.saveSettings).mockRejectedValueOnce({ code: 'settingsFailed', message: 'private backend text' })
    await core.saveSettings()
    expect(client.loadSettings).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().settings).toMatchObject({ readOnly: true, needsReload: true })
    expect(core.getSnapshot().settings!.applications[1]!.aliases[0]!.value).toBe('beta')
    expect(core.getSnapshot().status).toBe('设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('private backend')
  })

  it.each(['active', 'suppression', 'tombstone'] as const)('retires removed %s ownership before deletion', async (owner) => {
    const { core, emit } = await settingsCore()
    const application = core.getSnapshot().settings!.applications[0]!
    const alias = application.aliases[0]!
    core.text({ kind: 'compositionStart', control: alias.key, value: alias.value })
    if (owner === 'suppression') core.text({ kind: 'compositionEnd', control: alias.key, value: 'finished' })
    if (owner === 'tombstone') emit(shown('replacement-event', 'settings'))
    core.removeAlias(application.key, alias.key)
    const removed = core.getSnapshot()
    core.text({ kind: 'compositionEnd', control: alias.key, value: 'late' })
    core.text({ kind: 'compositionInput', control: alias.key, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(removed)
  })

  it('preserves unrelated ownership and retires form controls before fresh replacement', async () => {
    const { core, client } = await settingsCore()
    const original = core.getSnapshot().settings!
    const firstApplication = original.applications[0]!
    const removed = firstApplication.aliases[0]!
    const unrelated = original.applications[1]!.aliases[0]!
    core.text({ kind: 'compositionStart', control: unrelated.key, value: '' })
    core.removeAlias(firstApplication.key, removed.key)
    core.text({ kind: 'compositionEnd', control: unrelated.key, value: 'owned' })
    expect(core.getSnapshot().settings!.applications[1]!.aliases[0]!.value).toBe('owned')

    const oldKeys = [
      original.hotkey.key,
      original.researchId.key,
      ...original.applications.flatMap((application) => [application.key, ...application.aliases.map((alias) => alias.key)]),
    ]
    core.text({ kind: 'compositionStart', control: original.hotkey.key, value: original.hotkey.value })
    core.text({ kind: 'compositionUpdate', control: original.hotkey.key, value: 'uncommitted' })
    vi.mocked(client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await core.reloadSettings()
    const replacement = core.getSnapshot().settings!
    const replacedSnapshot = core.getSnapshot()
    core.text({ kind: 'compositionEnd', control: original.hotkey.key, value: 'late' })
    core.text({ kind: 'compositionInput', control: original.hotkey.key, value: 'late', inputType: 'insertCompositionText' })
    expect(core.getSnapshot()).toBe(replacedSnapshot)
    const newKeys = [
      replacement.hotkey.key,
      replacement.researchId.key,
      ...replacement.applications.flatMap((application) => [application.key, ...application.aliases.map((alias) => alias.key)]),
    ]
    expect(Math.min(...newKeys)).toBeGreaterThan(Math.max(...oldKeys))

    const removeStart = launcherCoreSource.indexOf('function removeAlias')
    const removeRetire = launcherCoreSource.indexOf('retireControl(alias)', removeStart)
    const removeDelete = launcherCoreSource.indexOf('.splice(', removeStart)
    expect(removeStart).toBeGreaterThanOrEqual(0)
    expect(removeRetire).toBeGreaterThan(removeStart)
    expect(removeDelete).toBeGreaterThan(removeRetire)
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
    const hotkey = core.getSnapshot().settings!.hotkey
    core.text({ kind: 'ordinaryInput', control: hotkey.key, value: 'Ctrl+Space', inputType: 'insertText' })
    expect(core.getSnapshot().shownNotice).toBeUndefined()
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

  it('renders the settings projection without app IDs and closes only through the core hide owner', async () => {
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
    expect(mounted.host.textContent).toContain('同名应用 (1)')
    expect(mounted.host.textContent).toContain('同名应用 (2)')
    expect(mounted.host.innerHTML).not.toContain('private-app-id')
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
      core.text({ kind: 'compositionStart', control, value: '' })
      core.text({ kind: 'compositionUpdate', control, value: '计' })
    })

    expect(bind).toHaveBeenCalledOnce()
    expect(unbind).not.toHaveBeenCalled()
    expect(retire).not.toHaveBeenCalled()
    await act(async () => core.text({ kind: 'compositionEnd', control, value: '计算器' }))
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
    const oldHotkey = core.getSnapshot().settings!.hotkey.key
    cleanup.length = 0
    vi.mocked(fake.client.loadSettings).mockResolvedValueOnce(settingsFixture)
    await act(async () => core.reloadSettings())
    const unbindIndex = cleanup.indexOf(`native-unbind:${oldHotkey}`)
    const retireIndex = cleanup.indexOf(`retire:${oldHotkey}`)
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
