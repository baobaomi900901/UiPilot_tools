// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ConfigProvider } from 'antd'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PluginPermissionsPanel } from './plugin-permissions-panel'
import type { LauncherClient, PublicPluginInventory, PublicPluginInventoryItem, PublicPermission } from './protocol'

;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function plugin(permissions: PublicPermission[], overrides: Partial<PublicPluginInventoryItem> = {}): PublicPluginInventoryItem {
  return { pluginId: 'test.plugin', name: '测试插件', description: null, version: '1.0.0', source: 'localPackage',
    installationDirectory: null, commands: [{ commandId: 'main', defaultName: 'test', effectiveName: 'findme', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' }],
    enabled: true, fault: null, generation: 1, iconUrl: null, network: null, settings: [],
    permissions: permissions.map((permission) => ({ permission, granted: false, supported: true })), ...overrides }
}
function snapshot(items: PublicPluginInventoryItem[], revision = '1'): PublicPluginInventory { return { revision, hotkeyRevision: '0', items } }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }

const cleanups: (() => void)[] = []
beforeEach(() => {
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: vi.fn(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })) })
  const computedStyle = window.getComputedStyle.bind(window)
  vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => computedStyle(element))
})
afterEach(async () => { await act(async () => { for (const cleanup of cleanups.splice(0)) cleanup() }); document.body.innerHTML = ''; vi.restoreAllMocks() })
async function setup(initial: PublicPluginInventory, active = true, parentEscape = () => {}) {
  let changed = () => {}
  const unsubscribe = vi.fn()
  const list = vi.fn(async () => initial)
  const write = vi.fn(async (_input: Parameters<LauncherClient['setPublicPluginPermissionAccess']>[0]) => {})
  const client = { listPublicPlugins: list, setPublicPluginPermissionAccess: write,
    listenPublicPluginPermissionsChanged: vi.fn(async (handler: () => void) => { changed = handler; return unsubscribe }) } as unknown as LauncherClient
  const host = document.createElement('div'); document.body.append(host); const root = createRoot(host)
  const render = async (nextActive: boolean) => { await act(async () => root.render(<ConfigProvider theme={{ token: { motion: false } }}><div onKeyDown={(event) => { if (event.key === 'Escape') parentEscape() }}><PluginPermissionsPanel client={client} active={nextActive} /></div></ConfigProvider>)) }
  cleanups.push(() => root.unmount()); await render(active)
  return { host, client, list, write, unsubscribe, render, change: async () => { await act(async () => changed()) } }
}
function switches(host: HTMLElement) { return [...host.querySelectorAll<HTMLButtonElement>('[role="switch"]')] }
async function click(element: HTMLElement) { await act(async () => element.click()) }
function button(label: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find((entry) => entry.getAttribute('aria-label') === label || entry.textContent?.replace(/\s/g, '') === label)! }
async function search(host: HTMLElement, value: string) {
  const input = host.querySelector<HTMLInputElement>('input')!
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })) })
}

describe('PluginPermissionsPanel', () => {
  it('confirms host credential quota use and keeps service grants independent', async () => {
    const { host, write } = await setup(snapshot([plugin(['credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'])]))
    expect([...host.querySelectorAll('h3')].map((entry) => entry.textContent)).toEqual(['有道翻译密钥 / Youdao Translation', '百度 OCR 密钥 / Baidu OCR', 'DeepSeek 密钥 / DeepSeek'])
    const trigger = switches(host)[0]; trigger.focus(); await click(trigger)
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('可能消耗 API 额度')
    await click(button('取消')); expect(write).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
    expect(document.activeElement).toBe(trigger)
    await click(trigger); await click(button('确认开启有道翻译密钥'))
    expect(write).toHaveBeenCalledExactlyOnceWith({ pluginId: 'test.plugin', permission: 'credentials.youdaoTranslation', granted: true, expectedInventoryRevision: '1' })
    expect(switches(host)[1].getAttribute('aria-checked')).toBe('false')
  })
  it('groups only declared capabilities and keeps history read and paste separate', async () => {
    const { host } = await setup(snapshot([plugin(['clipboard.history.read', 'clipboard.history.paste', 'ui.panel', 'url.open'])]))
    expect([...host.querySelectorAll('h3')].map((heading) => heading.textContent)).toEqual(['打开网页 / Open URLs', '剪贴板历史读取 / Read Clipboard History', '剪贴板历史粘贴 / Paste Clipboard History', '启动器面板 / Launcher Panel'])
    expect(switches(host).map((entry) => entry.getAttribute('aria-label'))).toEqual(['测试插件：打开网页', '测试插件：剪贴板历史读取', '测试插件：剪贴板历史粘贴', '测试插件：启动器面板'])
    expect(host.textContent).toContain('关闭后会关闭受影响的插件面板')
  })
  it('searches command names, plugin names, and concrete permission names', async () => {
    const { host } = await setup(snapshot([plugin(['clipboard.write', 'ui.window'])]))
    await search(host, '/findme'); expect(switches(host)).toHaveLength(2)
    await search(host, '测试'); expect(switches(host)).toHaveLength(2)
    await search(host, 'ui.window'); expect(switches(host)).toHaveLength(1)
    await search(host, 'missing'); expect(host.textContent).toContain('没有匹配的权限')
  })
  it('disables unsupported capabilities but allows grants for disabled plugins', async () => {
    const item = plugin(['clipboard.read', 'clipboard.write'], { enabled: false })
    item.permissions = item.permissions.map((entry) => ({ ...entry, supported: entry.permission !== 'clipboard.read' }))
    const { host, write } = await setup(snapshot([item])); expect(switches(host)[0].disabled).toBe(true); expect(switches(host)[1].disabled).toBe(false)
    expect(switches(host)[1].getAttribute('aria-description')).toContain('插件已停用'); await click(switches(host)[1])
    expect(write).toHaveBeenCalledExactlyOnceWith({ pluginId: 'test.plugin', permission: 'clipboard.write', granted: true, expectedInventoryRevision: '1' })
  })
  it('locks all switches for that plugin until the authoritative read settles', async () => {
    const { host, list, write } = await setup(snapshot([plugin(['clipboard.write', 'ui.panel']), plugin(['timer.control'], { pluginId: 'other', name: '其他插件' })]))
    const mutation = deferred<void>(); const read = deferred<PublicPluginInventory>(); write.mockReturnValueOnce(mutation.promise); list.mockReturnValueOnce(read.promise)
    await click(switches(host)[0]); expect(switches(host).map((entry) => entry.disabled)).toEqual([true, true, false])
    await click(switches(host)[1]); expect(write).toHaveBeenCalledTimes(1)
    await act(async () => mutation.resolve()); expect(switches(host)[0].disabled).toBe(true)
    await act(async () => read.resolve(snapshot([plugin(['clipboard.write', 'ui.panel'], { permissions: [{ permission: 'clipboard.write', supported: true, granted: true }, { permission: 'ui.panel', supported: true, granted: false }] })], '2')))
    expect(switches(host)[0].getAttribute('aria-checked')).toBe('true'); expect(switches(host)[0].disabled).toBe(false)
  })
  it('keeps uncertain permissions locked after write and reread failure until refresh succeeds', async () => {
    const { host, list, write } = await setup(snapshot([plugin(['clipboard.write'])]))
    write.mockRejectedValueOnce({ code: 'staleInventory' }); list.mockRejectedValueOnce(new Error('offline'))
    await click(switches(host)[0]); expect(switches(host)[0].disabled).toBe(true); expect(host.textContent).toContain('无法刷新当前状态')
    await click(button('刷新权限')); expect(switches(host)[0].disabled).toBe(false); expect(write).toHaveBeenCalledTimes(1)
  })
  it('discloses network hosts and credential targets and cancels without a write', async () => {
    const item = plugin(['network.https'], { network: { httpsHosts: ['api.example.com'], credentials: [{ id: 'api', settingKey: 'token', host: 'api.example.com', type: 'bearer' }] }, settings: [{ definition: { type: 'secret', key: 'token', label: '服务令牌' }, secretConfigured: true }] })
    const { host, write } = await setup(snapshot([item])); await click(button('查看 测试插件 的网络范围'))
    expect(host.textContent).toContain('api.example.com'); expect(host.textContent).toContain('服务令牌')
    const trigger = switches(host)[0]; trigger.focus(); await click(trigger)
    const dialog = document.querySelector('[role="dialog"]')!; expect(dialog.textContent).toContain('此凭据会发送给该服务'); expect(dialog.textContent).toContain('token')
    await click(button('取消')); expect(write).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
    expect(document.activeElement).toBe(trigger)
  })
  it('binds network confirmation to inventory revision and requires a new confirmation after change', async () => {
    const item = plugin(['network.https'], { network: { httpsHosts: ['old.example.com'], credentials: [] } })
    const { host, list, write, change } = await setup(snapshot([item])); await click(switches(host)[0])
    list.mockResolvedValueOnce(snapshot([{ ...item, network: { httpsHosts: ['new.example.com'], credentials: [] } }], '2')); await change()
    await click(button('确认开启网络访问')); expect(write).not.toHaveBeenCalled(); expect(host.textContent).toContain('重新确认')
    await click(switches(host)[0]); await click(button('确认开启网络访问'))
    expect(write).toHaveBeenCalledExactlyOnceWith({ pluginId: 'test.plugin', permission: 'network.https', granted: true, expectedInventoryRevision: '2' })
  })
  it('owns Escape in the network modal without invoking the parent settings handler', async () => {
    const parentEscape = vi.fn()
    const { host, write } = await setup(snapshot([plugin(['network.https'], { network: { httpsHosts: ['api.example.com'], credentials: [] } })]), true, parentEscape)
    const trigger = switches(host)[0]; trigger.focus(); await click(trigger)
    const confirm = button('确认开启网络访问'); confirm.focus()
    await act(async () => { confirm.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true })) })
    expect(parentEscape).not.toHaveBeenCalled(); expect(write).not.toHaveBeenCalled()
    expect(document.querySelector('[role="dialog"]')).toBeNull(); expect(document.activeElement).toBe(trigger)
  })
  it('leaves Ctrl+Shift+F to the host shortcut rules', async () => {
    const { host } = await setup(snapshot([plugin(['clipboard.write'])]))
    const event = new KeyboardEvent('keydown', { key: 'F', ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true })
    await act(async () => { document.dispatchEvent(event) }); expect(event.defaultPrevented).toBe(false); expect(document.activeElement).not.toBe(host.querySelector('input'))
  })
  it('does not let a late inventory response overwrite a newer one', async () => {
    const { host, list, change } = await setup(snapshot([plugin(['clipboard.write'])]))
    const old = deferred<PublicPluginInventory>(); list.mockReturnValueOnce(old.promise); await change()
    list.mockResolvedValueOnce(snapshot([plugin(['timer.control'])], '3')); await change()
    await act(async () => old.resolve(snapshot([plugin(['ui.window'])], '2')))
    expect(switches(host)[0].getAttribute('aria-label')).toContain('计时器控制')
  })
  it('returns keyboard focus after a direct toggle without stealing a later focus choice', async () => {
    const { host, write } = await setup(snapshot([plugin(['clipboard.write'])])); const mutation = deferred<void>(); write.mockReturnValueOnce(mutation.promise)
    const trigger = switches(host)[0]; trigger.focus(); await click(trigger)
    document.body.tabIndex = -1; document.body.focus()
    await act(async () => mutation.resolve()); expect(document.activeElement).toBe(trigger)
    const second = deferred<void>(); write.mockReturnValueOnce(second.promise); await click(trigger)
    const searchInput = host.querySelector<HTMLInputElement>('input')!; searchInput.focus()
    await act(async () => second.resolve()); expect(document.activeElement).toBe(searchInput)
  })
  it('revokes network immediately with the current revision and no confirmation', async () => {
    const item = plugin(['network.https'], { network: { httpsHosts: ['api.example.com'], credentials: [] }, permissions: [{ permission: 'network.https', supported: true, granted: true }] })
    const { host, write } = await setup(snapshot([item], '9')); await click(switches(host)[0])
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(write).toHaveBeenCalledExactlyOnceWith({ pluginId: 'test.plugin', permission: 'network.https', granted: false, expectedInventoryRevision: '9' })
  })
  it('ignores a write that completes after deactivation and never reloads a later activation for it', async () => {
    const { host, list, write, render } = await setup(snapshot([plugin(['clipboard.write'])])); const pendingWrite = deferred<void>(); write.mockReturnValueOnce(pendingWrite.promise)
    await click(switches(host)[0]); await render(false); await render(true)
    expect(list).toHaveBeenCalledTimes(2); await act(async () => pendingWrite.resolve()); expect(list).toHaveBeenCalledTimes(2)
    expect(switches(host)[0].disabled).toBe(false)
  })
  it('keeps the read started before another write finished from unlocking that plugin', async () => {
    const { host, list, write, change } = await setup(snapshot([plugin(['clipboard.write'])])); const mutation = deferred<void>(); write.mockReturnValueOnce(mutation.promise)
    await click(switches(host)[0]); const old = deferred<PublicPluginInventory>(); list.mockReturnValueOnce(old.promise); await change()
    const newest = deferred<PublicPluginInventory>(); list.mockReturnValueOnce(newest.promise); await act(async () => mutation.resolve())
    await act(async () => old.resolve(snapshot([plugin(['clipboard.write'])]))); expect(switches(host)[0].disabled).toBe(true)
    await act(async () => newest.resolve(snapshot([plugin(['clipboard.write'])], '2'))); expect(switches(host)[0].disabled).toBe(false)
  })
  it('scopes Ctrl+F and subscriptions to the active page', async () => {
    const { host, render, unsubscribe } = await setup(snapshot([plugin(['clipboard.write'])]))
    const key = () => { const event = new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true, cancelable: true }); document.dispatchEvent(event); return event }
    expect(key().defaultPrevented).toBe(true); expect(document.activeElement).toBe(host.querySelector('input'))
    await render(false); expect(unsubscribe).toHaveBeenCalledTimes(1); expect(key().defaultPrevented).toBe(false); expect(switches(host)).toHaveLength(0)
  })
  it('cleans up a listener whose subscription resolves after deactivation', async () => {
    const { client, render } = await setup(snapshot([]), false); const pending = deferred<() => void>(); const off = vi.fn()
    vi.mocked(client.listenPublicPluginPermissionsChanged!).mockReturnValueOnce(pending.promise)
    await render(true); await render(false); await act(async () => pending.resolve(off)); expect(off).toHaveBeenCalledTimes(1)
  })
  it('distinguishes no plugins, no declarations, and load failure', async () => {
    const { host, list, change } = await setup(snapshot([])); expect(host.textContent).toContain('尚未安装公开插件')
    list.mockResolvedValueOnce(snapshot([plugin([])])); await change(); expect(host.textContent).toContain('没有申报权限')
    list.mockRejectedValueOnce(new Error('offline')); await change(); expect(host.textContent).toContain('加载权限失败')
  })
})

it('shows history copy as a separate searchable grant', async () => {
  const { host, write } = await setup(snapshot([plugin(['clipboard.history.read', 'clipboard.history.copy'])]))
  await search(host, 'clipboard.history.copy')
  expect(switches(host)).toHaveLength(1)
  expect(host.textContent).toContain('剪贴板历史复制')
  await click(switches(host)[0])
  expect(write).toHaveBeenCalledExactlyOnceWith({ pluginId: 'test.plugin', permission: 'clipboard.history.copy', granted: true, expectedInventoryRevision: '1' })
})
