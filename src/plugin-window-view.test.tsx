import { readFileSync } from 'node:fs'
// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPluginWindowCore } from './plugin-window-core'
import { PluginWindowView } from './plugin-window-view'

const pluginWindowViewSource = readFileSync('src/plugin-window-view.tsx', 'utf8')
const stylesSource = readFileSync('src/styles.css', 'utf8')

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
    const getIdentity = vi.fn(async () => ({
      name: 'Public Plugin Demo Window',
      iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
    }))
    const setPinned = vi.fn(async ({ pinned }: { pinned: boolean }) => ({ pinned }))
    const close = vi.fn(async () => {})
    const core = createPluginWindowCore({ getIdentity, setPinned, close })
    await core.start()
    host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    await act(async () => root?.render(<PluginWindowView core={core} />))
    expect(host.querySelector('[data-tauri-drag-region]')).not.toBeNull()
    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')
    const closeButton = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')
    expect(pin).not.toBeNull()
    expect(closeButton).not.toBeNull()
    expect(pin?.getAttribute('aria-pressed')).toBe('false')
    expect(pin?.querySelector('.lucide-pin')).not.toBeNull()
    expect(closeButton?.querySelector('.lucide-x')).not.toBeNull()
    expect(host.querySelector('.plugin-window-title')?.textContent).toBe('Public Plugin Demo Window')
    expect(host.querySelector<HTMLImageElement>('.plugin-window-shell .plugin-icon-image')?.getAttribute('src'))
      .toBe('uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png')
    await act(async () => pin?.click())
    expect(setPinned).toHaveBeenCalledWith({ pinned: true })
    expect(host.querySelector('button[aria-label="取消固定"]')?.getAttribute('aria-pressed')).toBe('true')
    await act(async () => closeButton?.click())
    expect(close).toHaveBeenCalledOnce()
    expect(pluginWindowViewSource).toContain("from 'lucide-react'")
    expect(pluginWindowViewSource).not.toContain('@ant-design/icons')
    expect(stylesSource).toMatch(
      /\.plugin-window-shell\s*\{[^}]*color:\s*var\(--uipilot-ui-foreground\);[^}]*background:\s*var\(--uipilot-ui-surface\);[^}]*border-bottom:\s*1px solid var\(--uipilot-ui-border\);/s,
    )
  })
})
