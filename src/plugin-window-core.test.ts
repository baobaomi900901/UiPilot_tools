import { describe, expect, it, vi } from 'vitest'
import { createPluginWindowCore } from './plugin-window-core'
import type { PluginWindowClient } from './protocol'

function client(): PluginWindowClient {
  return {
    getIdentity: vi.fn(async () => ({
      name: 'Public Plugin Demo Window',
      iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
    })),
    setPinned: vi.fn(async ({ pinned }) => ({ pinned })),
    close: vi.fn(async () => {}),
  }
}

describe('plugin window shell core', () => {
  it('keeps pin host-owned and close always clears process-local pin state', async () => {
    const port = client()
    const core = createPluginWindowCore(port)
    await core.start()
    expect(core.getSnapshot()).toMatchObject({
      name: 'Public Plugin Demo Window',
      iconUrl: 'uipilot-public-plugin://localhost/__uipilot_icon/installed/com.uipilot.demo-win/1/icon.png',
    })
    await core.togglePinned()
    expect(core.getSnapshot()).toMatchObject({ pinned: true, pending: false })
    expect(port.setPinned).toHaveBeenCalledWith({ pinned: true })
    await core.close()
    expect(core.getSnapshot()).toMatchObject({ pinned: false, pending: false })
    expect(port.close).toHaveBeenCalledOnce()
  })

  it('keeps the previous state and reports a fixed error when a host operation fails', async () => {
    const port = client()
    vi.mocked(port.setPinned).mockRejectedValueOnce(new Error('private detail'))
    const core = createPluginWindowCore(port)
    await core.togglePinned()
    expect(core.getSnapshot()).toMatchObject({ pinned: false, pending: false, error: '插件窗口操作失败。' })
  })

  it('keeps the fallback identity when identity loading fails', async () => {
    const port = client()
    vi.mocked(port.getIdentity).mockRejectedValueOnce(new Error('private detail'))
    const core = createPluginWindowCore(port)
    await core.start()
    expect(core.getSnapshot().name).toBe('插件')
    expect(core.getSnapshot().iconUrl).toBeUndefined()
  })
})
