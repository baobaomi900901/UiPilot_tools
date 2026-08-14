// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPluginWindowCore } from './plugin-window-core'
import { PluginWindowView } from './plugin-window-view'

let root: ReturnType<typeof createRoot> | undefined
let host: HTMLDivElement | undefined

afterEach(() => {
  if (root) act(() => root?.unmount())
  host?.remove()
  root = undefined
  host = undefined
})

describe('plugin window shell view', () => {
  it('renders only host pin/close controls in a drag region', async () => {
    const setPinned = vi.fn(async ({ pinned }: { pinned: boolean }) => ({ pinned }))
    const close = vi.fn(async () => {})
    const core = createPluginWindowCore({ setPinned, close })
    host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    await act(async () => root?.render(<PluginWindowView core={core} />))
    expect(host.querySelector('[data-tauri-drag-region]')).not.toBeNull()
    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')
    const closeButton = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')
    expect(pin).not.toBeNull()
    expect(closeButton).not.toBeNull()
    await act(async () => pin?.click())
    expect(setPinned).toHaveBeenCalledWith({ pinned: true })
    await act(async () => closeButton?.click())
    expect(close).toHaveBeenCalledOnce()
  })
})