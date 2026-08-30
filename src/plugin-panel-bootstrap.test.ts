import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapPanelApi {
  onUpdate(handler: (update: unknown) => void): () => void
  onHostKey(handler: (event: unknown) => void): () => void
  readonly clipboardHistory: {
    list(): Promise<unknown>
    onChanged(handler: (snapshot: unknown) => void | Promise<void>): () => void
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
    clearInterval,
    queueMicrotask,
    setInterval,
    setTimeout,
    window: hostWindow,
  })
  return { document, hostWindow, invoke }
}

describe('public plugin panel bootstrap', () => {
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
})
