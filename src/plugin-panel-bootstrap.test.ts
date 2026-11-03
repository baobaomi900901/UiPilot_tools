import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapPanelApi {
  setStatus(input: { message: string; tone?: 'info' | 'success' | 'error' }): Promise<void>
  onReady(handler: (event: { sessionEpoch: string; requestId: string }) => void | Promise<void>): () => void
  onUpdate(handler: (update: unknown) => void | Promise<void>): () => void
  onHostKey(handler: (event: unknown) => void): () => void
  readonly clipboardHistory: {
    list(input?: { query?: string }): Promise<unknown>
    copy(input: { id: string }): Promise<unknown>
    onChanged(handler: (snapshot: unknown) => void | Promise<void>): () => void
    paste(input: { id: string, routeSequence: string }): Promise<unknown>
    remove(input: { id: string }): Promise<void>
    clear(): Promise<void>
  }
  requestHide(): Promise<void>
}

function panelBootstrapSource(capabilities = { clipboard: true, speech: true }): string {
  const rust = readFileSync('src-tauri/src/plugin_panel.rs', 'utf8')
  const template = rust.match(/PUBLIC_PANEL_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)?.[1]
  if (!template) throw new Error('panel bootstrap template is missing')
  return template
    .replace('__CONTEXT_ACTIONS_UI__', readFileSync('src/context-actions-ui.js', 'utf8').replace('export function', 'function') + '\n' + readFileSync('src/panel-context-actions.js', 'utf8').replace('export function', 'function').replace('export function', 'function'))
    .replace('__PANEL_EFFECTS__', readFileSync('src-tauri/src/panel_effects.js', 'utf8'))
    .replace('__CLIPBOARD_ENABLED__', String(capabilities.clipboard)).replace('__SPEECH_ENABLED__', String(capabilities.speech))
    .replace('__PANEL_JSON__', readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8'))
    .replace('__PANEL_ACTIONS__', 'false')
    .replace('__RUNTIME_ACTIONS__', 'false')
    .replaceAll('__SESSION_EPOCH__', '7')
    .replace('__HOST_KEYS__', '["ArrowDown"]')
}

function executePanelBootstrap(
  invoke = vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => undefined),
  capabilities = { clipboard: true, speech: true },
  enqueue = queueMicrotask,
) {
  const hostWindow: Record<string, unknown> = {
    __TAURI_INTERNALS__: { invoke },
    addEventListener: vi.fn(),
  }
  const document = {
    documentElement: { style: { setProperty: vi.fn() } },
    addEventListener: vi.fn(),
    querySelector: vi.fn(() => null),
  }
  vm.runInNewContext(panelBootstrapSource(capabilities), {
    document,
    TextEncoder,
    clearInterval,
    queueMicrotask: enqueue,
    setInterval,
    setTimeout,
    window: hostWindow,
  })
  return { document, hostWindow, invoke }
}

describe('public plugin panel bootstrap', () => {
  it('keeps query snapshots separate from the full-history cache at the same revision', async () => {
    const invoke = vi.fn(async (command: string, args?: unknown): Promise<unknown> => {
      if (command !== 'plugin_panel_clipboard_history_list') return undefined
      const query = (args as { query?: string }).query
      return { revision: '4', entries: [{ id: query ?? 'all', kind: 'text', capturedAt: 'now', textPreview: query ?? 'all' }] }
    })
    const { hostWindow } = executePanelBootstrap(invoke)
    const history = (hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory
    await expect(history.list()).resolves.toMatchObject({ entries: [{ id: 'all' }] })
    await expect(history.list({ query: 'alpha' })).resolves.toMatchObject({ entries: [{ id: 'alpha' }] })
    await expect(history.list({ query: 'beta' })).resolves.toMatchObject({ entries: [{ id: 'beta' }] })
    await expect(history.list()).resolves.toMatchObject({ entries: [{ id: 'all' }] })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_list', { sessionEpoch: '7', query: 'beta' })
  })

  it('validates query code-point length and unknown fields before invoking', async () => {
    const invoke = vi.fn(async () => ({ revision: '1', entries: [] }))
    const { hostWindow } = executePanelBootstrap(invoke)
    const history = (hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory
    await history.list({ query: '🙂'.repeat(512) })
    expect(invoke).toHaveBeenCalledTimes(1)
    await expect(history.list({ query: '🙂'.repeat(513) })).rejects.toMatchObject({ name: 'InvalidQuery' })
    // @ts-expect-error unknown query fields must not cross the bridge
    await expect(history.list({ pluginId: 'another' })).rejects.toThrow()
    // @ts-expect-error query must be text
    await expect(history.list({ query: 1 })).rejects.toThrow()
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('copies by id without an Enter ticket or a hide/paste side effect', async () => {
    const invoke = vi.fn(async () => ({ outcome: 'copied' }))
    const { hostWindow } = executePanelBootstrap(invoke)
    const history = (hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory
    const result = await history.copy({ id: 'text-1' })
    expect(result).toEqual({ outcome: 'copied' })
    expect(Object.isFrozen(result)).toBe(true)
    expect(invoke.mock.calls).toEqual([['plugin_panel_clipboard_history_copy', { sessionEpoch: '7', id: 'text-1' }]])
    // @ts-expect-error copy cannot accept paste authority
    await expect(history.copy({ id: 'text-1', routeSequence: '1' })).rejects.toThrow()
  })

  it('rejects copy and query completions from a superseded panel session', async () => {
    let complete: (value: unknown) => void = () => undefined
    const invoke = vi.fn(() => new Promise(resolve => { complete = resolve }))
    const { hostWindow } = executePanelBootstrap(invoke)
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    const prepare = hostWindow.__UIPILOT_PLUGIN_PANEL_PREPARE__ as (input: { sessionEpoch: string }) => void
    const oldHistory = api.clipboardHistory
    const copy = oldHistory.copy({ id: 'text-1' })
    prepare({ sessionEpoch: '8' })
    complete({ outcome: 'copied' })
    await expect(copy).rejects.toMatchObject({ name: 'ExpiredPanelSession' })
    await expect(oldHistory.copy({ id: 'text-1' })).rejects.toMatchObject({ name: 'ExpiredPanelSession' })
    const list = api.clipboardHistory.list({ query: 'x' })
    prepare({ sessionEpoch: '9' })
    complete({ revision: '1', entries: [] })
    await expect(list).rejects.toMatchObject({ name: 'ExpiredPanelSession' })
  })

  it('maps copy permission errors and rejects admitted-only responses', async () => {
    const invoke = vi.fn(async () => { throw { code: 'permissionDenied' } })
    const { hostWindow } = executePanelBootstrap(invoke)
    const history = (hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory
    await expect(history.copy({ id: 'text-1' })).rejects.toMatchObject({ name: 'PermissionDenied' })
    const other = executePanelBootstrap(vi.fn(async () => ({ outcome: 'admitted' })))
    await expect((other.hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory.copy({ id: 'text-1' })).rejects.toThrow('invalid clipboard history copy outcome')
  })
  it('publishes bounded status with document ordering and the captured session', async () => {
    const { hostWindow, invoke } = executePanelBootstrap()
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    await api.setStatus({ message: '保存成功', tone: 'success' })
    await api.setStatus({ message: '' })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_set_status', {
      sessionEpoch: '7', sequence: '1', status: { message: '保存成功', tone: 'success' },
    })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_set_status', {
      sessionEpoch: '7', sequence: '2', status: { message: '', tone: 'info' },
    })
    await expect(api.setStatus({ message: 'x'.repeat(513) })).rejects.toThrow()
    const oldSetStatus = api.setStatus
    const prepare = hostWindow.__UIPILOT_PLUGIN_PANEL_PREPARE__ as (input: { sessionEpoch: string }) => void
    prepare({ sessionEpoch: '8' })
    await expect(oldSetStatus({ message: 'old save' })).rejects.toThrow('ExpiredPanelSession')
    await api.setStatus({ message: 'new save' })
    expect(invoke).toHaveBeenLastCalledWith('plugin_panel_set_status', {
      sessionEpoch: '8', sequence: '3', status: { message: 'new save', tone: 'info' },
    })
  })
  it('executes the generated Host-key bridge and reaches content ready', async () => {
    const { hostWindow, invoke } = executePanelBootstrap()

    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    expect(api).toBeDefined()
    api.onUpdate(() => undefined)
    api.onHostKey(() => undefined)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('plugin_panel_content_ready', {
      sessionEpoch: '7',
      hostKeyReceiverRegistered: true,
      hostKeyRegistrationViolation: false,
    }))
  })

  it('prevents native browser find without stopping plugin key handlers', () => {
    const { document } = executePanelBootstrap()
    const keydown = document.addEventListener.mock.calls.find(([type]) => type === 'keydown')?.[1]
    const preventDefault = vi.fn()
    const stopPropagation = vi.fn()

    keydown({
      key: 'f',
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
      isComposing: false,
      defaultPrevented: false,
      preventDefault,
      stopPropagation,
    })

    expect(preventDefault).toHaveBeenCalledOnce()
    expect(stopPropagation).not.toHaveBeenCalled()
  })

  it('uses the prepared live session epoch for the complete hide transaction', async () => {
    vi.useFakeTimers()
    try {
      const invoke = vi.fn(async (command: string): Promise<unknown> => {
        if (command === 'plugin_panel_request_hide_admit') {
          return { outcome: 'admitted', hideTicketId: '11' }
        }
        return undefined
      })
      const { hostWindow } = executePanelBootstrap(invoke)
      const prepare = hostWindow.__UIPILOT_PLUGIN_PANEL_PREPARE__ as (
        input: { sessionEpoch: string }
      ) => void
      prepare({ sessionEpoch: '8' })

      await (hostWindow.uipilotPluginPanel as BootstrapPanelApi).requestHide()

      expect(invoke).toHaveBeenCalledWith('plugin_panel_request_hide_admit', {
        sessionEpoch: '8',
      })
      expect(invoke).toHaveBeenCalledWith('plugin_panel_request_hide_admit_observed', {
        sessionEpoch: '8',
        hideTicketId: '11',
      })
      await vi.runAllTimersAsync()
      expect(invoke).toHaveBeenCalledWith('plugin_panel_request_hide_commit', {
        sessionEpoch: '8',
        hideTicketId: '11',
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('exposes clipboard history list and normalizes stale reads without leaking raw clipboard APIs', async () => {
    let resolveSlow: (value: unknown) => void = () => undefined
    const slow = new Promise<unknown>((resolve) => {
      resolveSlow = resolve
    })
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command !== 'plugin_panel_clipboard_history_list') return undefined
      if (invoke.mock.calls.filter(([name]) => name === command).length === 1) return slow
      return {
        revision: '2',
        entries: [{ id: 'b', kind: 'text', capturedAt: '2026-08-30T02:00:00Z', textPreview: 'new' }],
      }
    })
    const { hostWindow } = executePanelBootstrap(invoke)
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi

    const first = api.clipboardHistory.list()
    const second = api.clipboardHistory.list()
    await expect(second).resolves.toMatchObject({ revision: '2' })
    resolveSlow({
      revision: '1',
      entries: [{ id: 'a', kind: 'text', capturedAt: '2026-08-30T01:00:00Z', textPreview: 'old' }],
    })

    await expect(first).resolves.toMatchObject({ revision: '2' })
    expect(hostWindow.navigator).toBeUndefined()
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_list', {
      sessionEpoch: '7',
    })
  })

  it('delivers clipboard history changes asynchronously, survives handler errors, and unsubscribes', async () => {
    vi.useFakeTimers()
    try {
      let revision = 0
      const invoke = vi.fn(async (command: string): Promise<unknown> => {
        if (command !== 'plugin_panel_clipboard_history_list') return undefined
        revision += 1
        return {
          revision: String(revision),
          entries: [{ id: String(revision), kind: 'files', capturedAt: '2026-08-30T01:00:00Z', firstFileName: 'a.txt', fileCount: 1, available: true }],
        }
      })
      const { hostWindow } = executePanelBootstrap(invoke)
      const snapshots: unknown[] = []
      const unsubscribe = (hostWindow.uipilotPluginPanel as BootstrapPanelApi).clipboardHistory.onChanged((snapshot) => {
        snapshots.push(snapshot)
        if (snapshots.length === 1) throw new Error('plugin handler failed')
      })

      await vi.advanceTimersByTimeAsync(0)
      expect(snapshots).toHaveLength(1)
      await vi.advanceTimersByTimeAsync(500)
      expect(snapshots).toHaveLength(2)

      unsubscribe()
      await vi.advanceTimersByTimeAsync(1000)
      expect(snapshots).toHaveLength(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('refreshes clipboard history after remove and clear mutations', async () => {
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command === 'plugin_panel_clipboard_history_list') {
        return { revision: '3', entries: [] }
      }
      return undefined
    })
    const { hostWindow } = executePanelBootstrap(invoke)
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi

    await api.clipboardHistory.remove({ id: 'entry-1' })
    await api.clipboardHistory.clear()

    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_remove', {
      sessionEpoch: '7',
      id: 'entry-1',
    })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_clear', {
      sessionEpoch: '7',
    })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_list', {
      sessionEpoch: '7',
    })
  })

  it('admits clipboard history paste with route sequence and maps stable paste errors', async () => {
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command === 'plugin_panel_clipboard_history_paste') {
        if (invoke.mock.calls.filter(([name]) => name === command).length === 1) {
          return { outcome: 'admitted' }
        }
        const error = new Error('record unavailable')
        ;(error as Error & { code?: string }).code = 'RecordUnavailable'
        throw error
      }
      return undefined
    })
    const { hostWindow } = executePanelBootstrap(invoke)
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi

    await expect(api.clipboardHistory.paste({ id: 'entry-1', routeSequence: '12' })).resolves.toEqual({
      outcome: 'admitted',
    })
    await expect(api.clipboardHistory.paste({ id: 'entry-1', routeSequence: '12' })).rejects.toMatchObject({
      name: 'RecordUnavailable',
    })
    await expect(api.clipboardHistory.paste({ id: 'entry-1', routeSequence: '012' })).rejects.toThrow('invalid clipboard history paste input')
    await expect(api.clipboardHistory.paste({ id: 'entry-1' } as { id: string, routeSequence: string })).rejects.toThrow('invalid clipboard history paste input')
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_history_paste', {
      sessionEpoch: '7',
      id: 'entry-1',
      routeSequence: '12',
    })
  })
})


describe('Panel text effects', () => {
  it('keeps the bridge language whitelist exactly aligned with the catalog', () => {
    const catalog = JSON.parse(readFileSync('docs/plugin-sdk/youdao-languages.json', 'utf8')) as { languages: { id: string }[] }
    const source = readFileSync('src-tauri/src/panel_effects.js', 'utf8')
    const literal = source.match(/const speechLanguages = new Set\((\[[^;]+\])\);/u)?.[1]
    expect(literal).toBeDefined()
    expect(JSON.parse(literal!)).toEqual(catalog.languages.map(({ id }) => id))
  })
  it('keeps missing voices unavailable and discards unknown-language host states', async () => {
    const { hostWindow, invoke } = executePanelBootstrap(vi.fn(async (command: string): Promise<unknown> => {
      if (command === 'plugin_panel_speech_voices') return []
      throw { code: 'SpeechVoiceUnavailable', message: 'native private detail' }
    }))
    const speech = (hostWindow.uipilotPluginPanel as { speech: { voices(): Promise<unknown>, play(input: unknown): Promise<unknown>, onState(handler: (state: unknown) => void): void } }).speech
    await expect(speech.voices()).resolves.toEqual([])
    await expect(speech.play({ text: 'hello', language: 'ja' })).rejects.toMatchObject({ name: 'SpeechVoiceUnavailable', message: 'SpeechVoiceUnavailable' })
    const handler = vi.fn()
    speech.onState(handler)
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_SPEECH_STATE__ as (input: unknown) => void)({ sessionEpoch: '7', state: { playbackId: 'speech-1', status: 'playing', language: 'xx' } })
    expect(handler).not.toHaveBeenCalled()
    invoke.mockResolvedValueOnce([{ id: 'bad', name: 'bad', language: 'xx' }])
    await expect(speech.voices()).rejects.toMatchObject({ name: 'SpeechFailed' })
  })
  it('round trips every catalog language through play, voice listings and state events', async () => {
    const catalog = JSON.parse(readFileSync('docs/plugin-sdk/youdao-languages.json', 'utf8')) as { languages: { id: string }[] }
    const voices = catalog.languages.map(({ id }) => ({ id: `voice-${id}`, name: id, language: id }))
    const { hostWindow, invoke } = executePanelBootstrap(vi.fn(async (command: string, args?: unknown): Promise<unknown> => {
      if (command === 'plugin_panel_speech_voices') return voices
      return { playbackId: 'speech-1', status: 'playing', language: (args as { input: { language: string } }).input.language }
    }))
    const speech = (hostWindow.uipilotPluginPanel as { speech: { voices(): Promise<unknown>, play(input: unknown): Promise<unknown>, onState(handler: (state: unknown) => void): void } }).speech
    await expect(speech.voices()).resolves.toEqual(voices)
    const handler = vi.fn()
    speech.onState(handler)
    for (const { id } of catalog.languages) {
      await expect(speech.play({ text: 'hello', language: id })).resolves.toMatchObject({ language: id })
      ;(hostWindow.__UIPILOT_PLUGIN_PANEL_SPEECH_STATE__ as (input: unknown) => void)({ sessionEpoch: '7', state: { playbackId: 'speech-1', status: 'completed', language: id } })
      expect(handler).toHaveBeenLastCalledWith({ playbackId: 'speech-1', status: 'completed', language: id })
    }
    const calls = invoke.mock.calls.length
    for (const language of ['auto', 'xx', 'JA', 'zh-cn', '', 'sr']) {
      await expect(speech.play({ text: 'hello', language })).rejects.toMatchObject({ name: 'InvalidPanelText' })
    }
    expect(invoke).toHaveBeenCalledTimes(calls)
  })

  it('omits undeclared text capabilities', () => {
    const { hostWindow, invoke } = executePanelBootstrap(undefined, { clipboard: false, speech: false })
    expect(Object.prototype.hasOwnProperty.call(hostWindow.uipilotPluginPanel, 'clipboard')).toBe(false)
    expect(Object.prototype.hasOwnProperty.call(hostWindow.uipilotPluginPanel, 'speech')).toBe(false)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('copies exact whitespace and rejects extra fields without IPC', async () => {
    const { hostWindow, invoke } = executePanelBootstrap()
    const api = hostWindow.uipilotPluginPanel as { clipboard: { writeText(input: unknown): Promise<void> } }
    await api.clipboard.writeText({ text: ' \n ' })
    expect(invoke).toHaveBeenCalledWith('plugin_panel_clipboard_write_text', { sessionEpoch: '7', input: { text: ' \n ' } })
    await expect(api.clipboard.writeText({ text: '', x: true })).rejects.toMatchObject({ name: 'InvalidPanelText' })
    await expect(api.clipboard.writeText({ text: '中'.repeat(5462) })).rejects.toMatchObject({ name: 'InvalidPanelText' })
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('isolates speech callbacks, old facades and late copy completion by session', async () => {
    let resolve: (value: unknown) => void = () => undefined
    const { hostWindow } = executePanelBootstrap(vi.fn(() => new Promise(done => { resolve = done })))
    const api = hostWindow.uipilotPluginPanel as { clipboard: { writeText(input: unknown): Promise<void> }, speech: { play(input: unknown): Promise<unknown>, onState(fn: (state: unknown) => void): () => void } }
    const old = api.speech
    const handler = vi.fn()
    old.onState(handler)
    const pending = api.clipboard.writeText({ text: 'hello' })
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_PREPARE__ as (input: unknown) => void)({ sessionEpoch: '8' })
    resolve(undefined)
    await expect(pending).rejects.toMatchObject({ name: 'ExpiredPanelSession' })
    await expect(old.play({ text: 'hello', language: 'en' })).rejects.toMatchObject({ name: 'ExpiredPanelSession' })
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_SPEECH_STATE__ as (input: unknown) => void)({ sessionEpoch: '7', state: { playbackId: 'a', status: 'completed', language: 'en' } })
    expect(handler).not.toHaveBeenCalled()
  })
  it('rejects voice overrides and sanitizes native failures', async () => {
    const { hostWindow, invoke } = executePanelBootstrap(vi.fn(async () => { throw { code: 'SpeechUnavailable', message: 'private native details' } }))
    const speech = (hostWindow.uipilotPluginPanel as { speech: { play(input: unknown): Promise<unknown> } }).speech
    await expect(speech.play({ text: 'hello', language: 'en', voiceId: 'x' })).rejects.toMatchObject({ name: 'InvalidPanelText' })
    await expect(speech.play({ text: 'hello', language: 'en' })).rejects.toMatchObject({ name: 'SpeechUnavailable', message: 'SpeechUnavailable' })
    expect(invoke).toHaveBeenCalledTimes(1)
  })
})


describe('Panel settled readiness', () => {
  const update = (id: string, epoch = '7') => ({ requestId: id, sessionEpoch: epoch, theme: 'light' })
  it('requires boot registration and delivers a frozen notice once only after host settlement', async () => {
    const { hostWindow, invoke } = executePanelBootstrap()
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    const ready = vi.fn()
    api.onReady(ready)
    expect(() => api.onReady(ready)).toThrow()
    api.onUpdate(() => undefined)
    const deliver = hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function
    const notify = hostWindow.__UIPILOT_PLUGIN_PANEL_READY__ as Function
    await deliver(update('a'))
    await new Promise(resolve => setTimeout(resolve, 510))
    expect(ready).not.toHaveBeenCalled()
    notify(update('wrong'))
    notify(update('a', '8'))
    notify(update('a'))
    notify(update('a'))
    await Promise.resolve()
    expect(ready).toHaveBeenCalledExactlyOnceWith({ sessionEpoch: '7', requestId: 'a' })
    expect(Object.isFrozen(ready.mock.calls[0]![0])).toBe(true)
    expect(invoke.mock.calls.filter(([c]) => c === 'plugin_panel_content_ack')).toHaveLength(1)
    expect(() => api.onReady(() => undefined)).toThrow()
  })
  it.each(['update', 'prepare', 'unsubscribe', 'teardown'])('invalidates a queued notice on %s', async kind => {
    const queue: (() => void)[] = []
    const { hostWindow } = executePanelBootstrap(undefined, undefined, callback => queue.push(callback))
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    const ready = vi.fn()
    const unsubscribe = api.onReady(ready)
    api.onUpdate(() => undefined)
    await (hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function)(update('a'))
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_READY__ as Function)(update('a'))
    if (kind === 'update') await (hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function)(update('b'))
    if (kind === 'prepare') (hostWindow.__UIPILOT_PLUGIN_PANEL_PREPARE__ as Function)({ sessionEpoch: '7' })
    if (kind === 'unsubscribe') unsubscribe()
    if (kind === 'teardown') (hostWindow.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(([name]) => name === 'pagehide')![1]()
    queue.forEach(callback => callback())
    expect(ready).not.toHaveBeenCalled()
  })
  it.each([false, true])('contains ready receiver errors (async=%s) without ACK or retries', async asyncFailure => {
    const { hostWindow, invoke } = executePanelBootstrap()
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    const ready = vi.fn(() => { if (asyncFailure) return Promise.reject(new Error('secret')); throw new Error('secret') })
    api.onReady(ready)
    api.onUpdate(() => undefined)
    await (hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function)(update('a'))
    const count = invoke.mock.calls.length
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_READY__ as Function)(update('a'))
    await Promise.resolve(); await Promise.resolve()
    expect(ready).toHaveBeenCalledOnce()
    expect(invoke.mock.calls).toHaveLength(count)
  })
  it('assigns new update ownership before its asynchronous handler settles', async () => {
    const queue: (() => void)[] = []
    const { hostWindow } = executePanelBootstrap(undefined, undefined, callback => queue.push(callback))
    const api = hostWindow.uipilotPluginPanel as BootstrapPanelApi
    const ready = vi.fn()
    let release!: () => void
    api.onReady(ready)
    api.onUpdate((value: any) => value.requestId === 'b' ? new Promise<void>(resolve => { release = resolve }) : undefined)
    await (hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function)(update('a'))
    ;(hostWindow.__UIPILOT_PLUGIN_PANEL_READY__ as Function)(update('a'))
    const b = (hostWindow.__UIPILOT_PLUGIN_PANEL_UPDATE__ as Function)(update('b'))
    queue.forEach(callback => callback())
    expect(ready).not.toHaveBeenCalled()
    release(); await b
  })
})
