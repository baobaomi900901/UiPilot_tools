import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapPanelApi {
  setStatus(input: { message: string; tone?: 'info' | 'success' | 'error' }): Promise<void>
  onUpdate(handler: (update: unknown) => void): () => void
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

function panelBootstrapSource(): string {
  const rust = readFileSync('src-tauri/src/plugin_panel.rs', 'utf8')
  const template = rust.match(/PUBLIC_PANEL_BOOTSTRAP_TEMPLATE: &str = r#"([\s\S]*?)"#;/u)?.[1]
  if (!template) throw new Error('panel bootstrap template is missing')
  return template
    .replace('__CONTEXT_ACTIONS_UI__', readFileSync('src/context-actions-ui.js', 'utf8').replace('export function', 'function') + '\n' + readFileSync('src/panel-context-actions.js', 'utf8').replace('export function', 'function').replace('export function', 'function'))
    .replace('__PANEL_JSON__', readFileSync('src-tauri/src/public_plugins/panel_action_json.js', 'utf8'))
    .replace('__PANEL_ACTIONS__', 'false')
    .replace('__RUNTIME_ACTIONS__', 'false')
    .replaceAll('__SESSION_EPOCH__', '7')
    .replace('__HOST_KEYS__', '["ArrowDown"]')
}

function executePanelBootstrap(
  invoke = vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => undefined),
) {
  const hostWindow: Record<string, unknown> = {
    __TAURI_INTERNALS__: { invoke },
  }
  const document = {
    documentElement: { style: { setProperty: vi.fn() } },
    addEventListener: vi.fn(),
    querySelector: vi.fn(() => null),
  }
  vm.runInNewContext(panelBootstrapSource(), {
    document,
    TextEncoder,
    clearInterval,
    queueMicrotask,
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
