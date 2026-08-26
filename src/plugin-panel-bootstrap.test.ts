import { readFileSync } from 'node:fs'
import vm from 'node:vm'

import { describe, expect, it, vi } from 'vitest'

interface BootstrapPanelApi {
  onUpdate(handler: (update: unknown) => void): () => void
  onHostKey(handler: (event: unknown) => void): () => void
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
    queueMicrotask,
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
})
