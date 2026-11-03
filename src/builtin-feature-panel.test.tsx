// @vitest-environment jsdom
import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PublicPluginPanel } from './public-plugin-panel'
import { BuiltinFeatureDetail } from './builtin-feature-panel'
import type { BuiltinFeatureView, LauncherClient } from './protocol'

const features: readonly BuiltinFeatureView[] = [
  { feature: 'screenshot', name: '截图', command: '/screenshot', defaultCommand: '/screenshot', description: '区域和窗口捕获', examples: ['/screenshot'], version: '0.14.0', enabled: true, favorite: false, hotkey: null, hotkeyStatus: 'unset', directory: null },
  { feature: 'quicklinks', name: '快捷链接', command: '/quicklinks', defaultCommand: '/quicklinks', description: '打开保存的链接', examples: ['/quicklinks'], version: '0.14.0', enabled: true, favorite: false, hotkey: null, hotkeyStatus: 'unset', directory: null },
  { feature: 'webSearch', name: '网页搜索', command: '/web-search', defaultCommand: '/web-search', description: '搜索互联网', examples: ['/web-search cats'], version: '0.14.0', enabled: true, favorite: false, hotkey: null, hotkeyStatus: 'unset', directory: null },
]
function fixture() {
  let current = features.map((item) => ({ ...item }))
  const listeners = new Set<() => void>()
  return {
    setBuiltinFeatureName: vi.fn(async ({ feature, nameOverride }: Parameters<LauncherClient['setBuiltinFeatureName']>[0]) => { current = current.map((item) => item.feature === feature ? { ...item, command: nameOverride ? `/${nameOverride}` : item.defaultCommand } : item) }),
    setBuiltinFeatureHotkey: vi.fn(async ({ feature, hotkey }: Parameters<LauncherClient['setBuiltinFeatureHotkey']>[0]) => { current = current.map((item) => item.feature === feature ? { ...item, hotkey, hotkeyStatus: hotkey ? 'active' : 'unset' } : item) }),
    setBuiltinFeatureFavorite: vi.fn(async ({ feature, favorite }: Parameters<LauncherClient['setBuiltinFeatureFavorite']>[0]) => { current = current.map((item) => item.feature === feature ? { ...item, favorite } : item) }),
    listBuiltinFeatures: vi.fn(async () => current),
    setBuiltinFeatureEnabled: vi.fn(async ({ feature, enabled }) => { current = current.map((item) => item.feature === feature ? { ...item, enabled } : item); return current }),
    listenBuiltinFeaturesChanged: vi.fn(async (handler: () => void) => { listeners.add(handler); return () => { listeners.delete(handler) } }),
    setCurrent(items: readonly BuiltinFeatureView[]) { current = items.map((item) => ({ ...item })) },
    emitChanged() { for (const listener of listeners) listener() },
    listPublicPlugins: vi.fn(async () => ({ revision: '1', hotkeyRevision: '1', items: [] })),
    uninstallPublicPlugin: vi.fn(),
  }
}
beforeEach(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true }); window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })) })
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
  it('shows compact builtin rows and filters by name and command without list metadata or uninstall', async () => {
    const api = fixture(); const details = vi.fn()
    const container = await render(<PublicPluginPanel client={api as unknown as LauncherClient} onOpenBuiltinDetails={details} />)
    await vi.waitFor(() => expect(container.querySelectorAll('[data-builtin-feature]')).toHaveLength(3))
    expect([...container.querySelectorAll('[data-builtin-feature]')].map((row) => row.getAttribute('data-builtin-feature'))).toEqual(['screenshot', 'quicklinks', 'webSearch'])
    const row = container.querySelector('[data-builtin-feature="quicklinks"]')!
    expect(row.textContent).not.toMatch(/内置|0\.14\.0|打开保存的链接|删除|卸载|重新加载/)
    click(row.querySelector('[aria-label="查看 快捷链接 详情"]'))
    expect(details).toHaveBeenCalledWith(features[1])
    const input = container.querySelector<HTMLInputElement>('[aria-label="搜索插件名称或启动命令"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '网页')
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

  it('edits startup, resets only the command and exposes no plugin removal actions', async () => {
    const api = fixture()
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />)
    const input = container.querySelector<HTMLInputElement>('[aria-label="启动键"]')!
    expect(input).not.toBeNull()
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, ' /capture ')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => input.dispatchEvent(new FocusEvent('focusout', { bubbles: true })))
    expect(api.setBuiltinFeatureName).toHaveBeenCalledWith({ feature: 'screenshot', nameOverride: 'capture' })
    expect(input.value).toBe('/capture')
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="恢复默认启动键"]')!.click())
    await vi.waitFor(() => expect(api.setBuiltinFeatureName).toHaveBeenLastCalledWith({ feature: 'screenshot', nameOverride: null }))
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="设为常用"]')!.click())
    await vi.waitFor(() => expect(api.setBuiltinFeatureFavorite).toHaveBeenCalledWith({ feature: 'screenshot', favorite: true }))
    expect(container.querySelector('[aria-label="取消常用"]')?.getAttribute('aria-pressed')).toBe('true')
    expect(container.textContent).toContain('宿主内置')
    expect(container.textContent).not.toMatch(/查看诊断|删除|重新载入/)
    expect(api.setBuiltinFeatureHotkey).not.toHaveBeenCalled()
  })

  it.each(features)('clears only the $feature hotkey and cancels recording through the host phase callback', async (feature) => {
    const api = fixture(), phase = vi.fn(async () => {})
    api.setCurrent(features.map(item => item.feature === feature.feature ? { ...item, hotkey: 'Ctrl+Alt+P', hotkeyStatus: 'active' } : item))
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={feature} onRecordingPhase={phase} />)
    const clear = container.querySelector<HTMLButtonElement>('[aria-label="清除命令快捷键"]')!
    expect(clear.disabled).toBe(false)
    await act(async () => clear.click())
    expect(api.setBuiltinFeatureHotkey).toHaveBeenCalledWith({ feature: feature.feature, hotkey: null })
    const input = container.querySelector<HTMLInputElement>('[aria-label="命令快捷键"]')!
    expect(input.value).toBe(''); expect(input.placeholder).toBe('未设置')
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="录制命令快捷键"]')!.click())
    expect(phase).toHaveBeenCalledWith('recording')
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(phase).toHaveBeenCalledWith('idle')
    expect(api.setBuiltinFeatureHotkey).toHaveBeenCalledTimes(1)
    expect(api.setBuiltinFeatureName).not.toHaveBeenCalled()
  })

  it('keeps startup controls gated when a change event supersedes the mutation read', async () => {
    const api = fixture()
    const container = await render(<BuiltinFeatureDetail client={api as unknown as LauncherClient} feature={features[0]!} />)
    let olderResolve!: (items: readonly BuiltinFeatureView[]) => void, newerResolve!: (items: readonly BuiltinFeatureView[]) => void
    const older = new Promise<readonly BuiltinFeatureView[]>(resolve => { olderResolve = resolve })
    const newer = new Promise<readonly BuiltinFeatureView[]>(resolve => { newerResolve = resolve })
    api.listBuiltinFeatures.mockImplementationOnce(() => older as never).mockImplementationOnce(() => newer as never)
    const input = container.querySelector<HTMLInputElement>('[aria-label="启动键"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '/capture')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    await act(async () => api.emitChanged())
    await act(async () => olderResolve(features))
    expect(input.readOnly).toBe(true)
    expect(container.querySelector<HTMLButtonElement>('[aria-label="设为常用"]')!.disabled).toBe(true)
    await act(async () => newerResolve(features.map(item => item.feature === 'screenshot' ? { ...item, command: '/capture' } : item)))
    expect(input.readOnly).toBe(false)
    expect(input.value).toBe('/capture')
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
    expect(container.textContent).toContain('宿主内置'); expect(container.textContent).not.toMatch(/凭据设置|查看诊断|删除/)
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
