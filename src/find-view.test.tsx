// @vitest-environment jsdom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createFindCore, type FindCore } from './find-core'
import { FindView } from './find-view'
import type { ExecuteOutcome, FileSearchResponse, FindClient } from './protocol'

function response(): FileSearchResponse {
  return {
    requestId: 'request-1',
    indexRevision: '1',
    total: '1',
    status: 'ready',
    items: [{
      resultId: 'secret-result-id',
      name: 'Quarterly.pdf',
      kind: 'file',
      sizeBytes: '42',
      modifiedUtc: '2026-08-11T01:02:03Z',
      fullPath: String.raw`C:\Private\Quarterly.pdf`,
    }],
  }
}

function fakeClient() {
  let forward: ((payload: unknown) => void) | undefined
  const client: FindClient = {
    listenForward: vi.fn(async (handler) => { forward = handler; return vi.fn() }),
    listenThemeChanged: vi.fn(async () => vi.fn()),
    prepareInitialization: vi.fn(async () => ({
      status: 'prepared',
      initialization: {
        initializationToken: 'init-1',
        themeRevision: '1',
        theme: 'system',
        filePreviewRevision: '1',
        filePreviewEnabled: true,
        pinned: false,
      },
    })),
    commitReady: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    getReadyStatus: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    searchFiles: vi.fn(async () => response()),
    executeResult: vi.fn(async () => ({ status: 'fileRevealRequested' }) satisfies ExecuteOutcome),
    setPinned: vi.fn(async ({ pinned }) => ({ pinned })),
    setPreviewPreference: vi.fn(async ({ preference }) => ({
      filePreviewRevision: '2',
      filePreviewEnabled: preference.enabled,
    })),
    hide: vi.fn(async () => undefined),
  }
  return {
    client,
    emitForward(payload = { invocationId: 'inv-1', forwardSequence: '1', query: 'quarterly' }) {
      if (!forward) throw new Error('forward listener unavailable')
      forward(payload)
    },
  }
}

let mountedRoot: Root | undefined

async function mount(core: FindCore) {
  const host = document.createElement('div')
  document.body.append(host)
  mountedRoot = createRoot(host)
  await act(async () => mountedRoot!.render(<FindView core={core} />))
  return host
}

afterEach(async () => {
  if (mountedRoot) await act(async () => mountedRoot!.unmount())
  mountedRoot = undefined
  document.body.replaceChildren()
})

Object.defineProperty(window, 'matchMedia', {
  configurable: true,
  value: vi.fn(() => ({
    matches: false,
    media: '(prefers-color-scheme: dark)',
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  })),
})

describe('FindView', () => {
  it('keeps controls disabled until readiness is confirmed', async () => {
    const fake = fakeClient()
    let resolveForward!: (unlisten: () => void) => void
    vi.mocked(fake.client.listenForward).mockReturnValueOnce(new Promise((resolve) => { resolveForward = resolve }))
    const core = createFindCore(fake.client)
    const host = await mount(core)
    expect(host.querySelector<HTMLInputElement>('[role="combobox"]')?.disabled).toBe(true)
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')?.disabled).toBe(true)
    resolveForward(vi.fn())
  })

  it('renders accessible fixed icon controls and forced close', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' }))
    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!
    expect(pin.getAttribute('aria-pressed')).toBe('false')
    expect(pin.classList.contains('find-icon-button')).toBe(true)
    await act(async () => pin.click())
    await vi.waitFor(() => expect(host.querySelector('button[aria-label="取消固定"]')).toBeTruthy())
    const close = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!
    await act(async () => close.click())
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: true })
  })

  it('keeps pinned Escape inert and sends ordinary hide when unpinned', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' }))
    const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: false })
    vi.mocked(fake.client.hide).mockClear()
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!.click())
    await vi.waitFor(() => expect(core.getSnapshot().pinned).toBe(true))
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).not.toHaveBeenCalled()
  })

  it('keeps result identity private while category, list, and preview interact', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(host.querySelectorAll('#find-results [role="option"]')).toHaveLength(1))
    expect(host.textContent).toContain('Quarterly.pdf')
    expect(host.querySelector('[aria-label="文件预览"]')?.textContent).toContain('完整路径')
    expect(host.innerHTML).not.toContain('secret-result-id')
    await act(async () => host.querySelector<HTMLButtonElement>('.find-category:nth-of-type(6)')!.click())
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'pdf' }))
  })
})
