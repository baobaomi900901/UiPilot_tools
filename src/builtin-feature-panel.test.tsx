// @vitest-environment jsdom
import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PublicPluginPanel } from './public-plugin-panel'
import { BuiltinFeatureDetail } from './builtin-feature-panel'
import type { BuiltinFeatureView, LauncherClient } from './protocol'

const features: readonly BuiltinFeatureView[] = [
  { feature: 'screenshot', name: '截图', command: '/screenshot', description: '区域和窗口捕获', examples: ['/screenshot'], version: '0.14.0', enabled: true },
  { feature: 'quicklinks', name: '快捷链接', command: '/quicklinks', description: '打开保存的链接', examples: ['/quicklinks'], version: '0.14.0', enabled: true },
  { feature: 'webSearch', name: '网页搜索', command: '/web-search', description: '搜索互联网', examples: ['/web-search cats'], version: '0.14.0', enabled: true },
]
function fixture() {
  let current = features.map((item) => ({ ...item }))
  const listeners = new Set<() => void>()
  return {
    listBuiltinFeatures: vi.fn(async () => current),
    setBuiltinFeatureEnabled: vi.fn(async ({ feature, enabled }) => { current = current.map((item) => item.feature === feature ? { ...item, enabled } : item); return current }),
    listenBuiltinFeaturesChanged: vi.fn(async (handler: () => void) => { listeners.add(handler); return () => { listeners.delete(handler) } }),
    setCurrent(items: readonly BuiltinFeatureView[]) { current = items.map((item) => ({ ...item })) },
    emitChanged() { for (const listener of listeners) listener() },
    listPublicPlugins: vi.fn(async () => ({ revision: '1', hotkeyRevision: '1', items: [] })),
    uninstallPublicPlugin: vi.fn(),
  }
}
let cleanup = () => {}
afterEach(() => { cleanup() })
async function render(element: React.ReactNode) {
  const container = document.createElement('div'); document.body.append(container)
  const root = createRoot(container)
  cleanup = () => { act(() => root.unmount()); container.remove() }
  await act(async () => { root.render(element) })
  return container
}
function click(element: Element | null) { expect(element).not.toBeNull(); act(() => (element as HTMLElement).click()) }

describe('managed builtin settings', () => {
  it('shows fixed builtin order, shared command/description filter and details without uninstall', async () => {
    const api = fixture(); const details = vi.fn()
    const container = await render(<PublicPluginPanel client={api as unknown as LauncherClient} onOpenBuiltinDetails={details} />)
    await vi.waitFor(() => expect(container.querySelectorAll('[data-builtin-feature]')).toHaveLength(3))
    expect([...container.querySelectorAll('[data-builtin-feature]')].map((row) => row.getAttribute('data-builtin-feature'))).toEqual(['screenshot', 'quicklinks', 'webSearch'])
    const row = container.querySelector('[data-builtin-feature="quicklinks"]')!
    expect(row.textContent).toContain('内置'); expect(row.textContent).not.toMatch(/删除|卸载|重新加载/)
    click(row.querySelector('[aria-label="查看 快捷链接 详情"]'))
    expect(details).toHaveBeenCalledWith(features[1])
    const input = container.querySelector<HTMLInputElement>('[aria-label="筛选插件名称"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '互联网')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      input.dispatchEvent(new Event('change', { bubbles: true }))
    })
    await vi.waitFor(() => expect(container.querySelectorAll('[data-builtin-feature]')).toHaveLength(1))
    expect(container.querySelector('[data-builtin-feature="webSearch"]')).not.toBeNull()
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '/quicklinks')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await vi.waitFor(() => expect(container.querySelector('[data-builtin-feature="quicklinks"]')).not.toBeNull())
    expect(container.querySelectorAll('[data-builtin-feature]')).toHaveLength(1)
    expect(api.uninstallPublicPlugin).not.toHaveBeenCalled()
  })

  it('disables and re-enables from authoritative replies and prevents duplicate pending toggles', async () => {
    const api = fixture()
    let resolve!: (items: readonly BuiltinFeatureView[]) => void
    api.setBuiltinFeatureEnabled.mockImplementationOnce(() => new Promise((accept) => { resolve = accept }) as never)
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />)
    await vi.waitFor(() => expect(api.listBuiltinFeatures).toHaveBeenCalled())
    const toggle = container.querySelector('[role="switch"]')!
    click(toggle); click(toggle)
    expect(api.setBuiltinFeatureEnabled).toHaveBeenCalledTimes(1)
    expect(api.setBuiltinFeatureEnabled).toHaveBeenCalledWith({ feature: 'screenshot', enabled: false })
    await act(async () => resolve(features.map((item) => ({ ...item, enabled: false }))))
    expect(toggle.getAttribute('aria-checked')).toBe('false')
    click(toggle)
    await vi.waitFor(() => expect(toggle.getAttribute('aria-checked')).toBe('true'))
    expect(container.textContent).toContain('0.14.0')
    expect(container.textContent).not.toMatch(/权限列表|插件所在目录|凭据设置/)
  })

  it('ignores an older initial read that arrives after a successful toggle', async () => {
    const api = fixture()
    let resolve!: (items: readonly BuiltinFeatureView[]) => void
    api.listBuiltinFeatures.mockImplementationOnce(() => new Promise((accept) => { resolve = accept }) as never)
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />)
    click(container.querySelector('[role="switch"]'))
    await vi.waitFor(() => expect(container.querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('false'))
    await act(async () => resolve(features))
    expect(container.querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('false')
  })

  it('reads back on save failure and leaves a recoverable inline error', async () => {
    const api = fixture()
    api.setBuiltinFeatureEnabled.mockRejectedValueOnce(new Error('save failed'))
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />)
    click(container.querySelector('[role="switch"]'))
    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')?.textContent).toContain('无法更改'))
    expect(api.listBuiltinFeatures).toHaveBeenCalledTimes(2)
    expect(container.querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('true')
    expect(container.querySelector('[role="switch"]')?.hasAttribute('disabled')).toBe(false)
  })

  it('refreshes the list when a detail toggle completes after returning', async () => {
    const api = fixture()
    let resolve!: (items: readonly BuiltinFeatureView[]) => void
    api.setBuiltinFeatureEnabled.mockImplementationOnce(() => new Promise((accept) => { resolve = accept }) as never)
    function Flow() {
      const [detail, setDetail] = useState(true)
      return <><button onClick={() => setDetail(false)}>返回列表</button>{detail
        ? <BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />
        : <PublicPluginPanel client={api as unknown as LauncherClient} />}</>
    }
    const container = await render(<Flow />)
    await vi.waitFor(() => expect(container.querySelector('.public-plugin-detail [role="switch"]')).not.toBeNull())
    click(container.querySelector('.public-plugin-detail [role="switch"]'))
    click(container.querySelector('button'))
    await vi.waitFor(() => expect(container.querySelector('[data-builtin-feature="screenshot"] [role="switch"]')?.getAttribute('aria-checked')).toBe('true'))
    const updated = features.map((item) => item.feature === 'screenshot' ? { ...item, enabled: false } : item)
    await act(async () => {
      api.setCurrent(updated)
      api.emitChanged()
      resolve(updated)
    })
    await vi.waitFor(() => expect(container.querySelector('[data-builtin-feature="screenshot"] [role="switch"]')?.getAttribute('aria-checked')).toBe('false'))
  })
})
