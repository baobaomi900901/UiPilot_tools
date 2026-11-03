// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { PublicPluginPanel, PublicPluginRecovery } from './public-plugin-panel'
import type { LauncherClient, PublicPluginDiagnosticReport, PublicPluginInventoryItem, PublicPluginPrepareSummary } from './protocol'

const plugin: PublicPluginInventoryItem = { pluginId: 'com.example.recovery', name: 'Recovery', version: '1.0.0', description: null, source: 'localPackage', installationDirectory: null, commands: [], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [] }
const prepared: PublicPluginPrepareSummary = { token: 'one-use', pluginId: plugin.pluginId, name: 'Recovery', version: '1.0.0', permissions: [], isUpdate: true, operation: 'developmentRefresh', previousVersion: '1.0.0', sourceVerified: false, iconUrl: null, network: null }
const report: PublicPluginDiagnosticReport = { schemaVersion: 1, hostVersion: '0.7.0', platform: 'windows', pluginId: plugin.pluginId, state: null, lastOperation: null }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((a,b) => { resolve=a; reject=b }); return { promise, resolve, reject } }
let root: Root, host: HTMLDivElement
function buttons(text: string) { return [...host.querySelectorAll<HTMLButtonElement>('button')].filter(b => b.textContent === text) }
async function click(text: string) { await act(async () => buttons(text)[0]!.click()) }
function clientFixture() { return { selectPublicPluginDirectory: vi.fn(async () => 'synthetic/package'), preparePublicPlugin: vi.fn(async () => prepared), commitPublicPlugin: vi.fn(async () => {}), cancelPublicPlugin: vi.fn(async () => {}), setPublicPluginEnabled: vi.fn(async () => {}), getPublicPluginDiagnostics: vi.fn(async () => report), copyPublicPluginDiagnostics: vi.fn(async () => {}) } }
async function mount(client: ReturnType<typeof clientFixture>, reload = vi.fn(async () => {}), item = plugin) { await act(async () => root.render(<PublicPluginRecovery key={item.pluginId} client={client as unknown as LauncherClient} plugin={item} inventoryRevision="2" reload={reload} />)); return reload }
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } })
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }))
  host = document.createElement('div'); document.body.append(host); root=createRoot(host)
})
afterEach(async () => { await act(async () => root.unmount()); host.remove() })
describe('public plugin list commit feedback', () => {
  it.each([false, true])('leaves service permissions for explicit detail consent on install/update (update=%s)', async (isUpdate) => {
    const client = { ...clientFixture(), listPublicPlugins: vi.fn(async () => ({ revision: '2', hotkeyRevision: '0', items: [] })) }
    client.preparePublicPlugin.mockResolvedValue({ ...prepared, isUpdate, operation: isUpdate ? 'replace' : 'install', previousVersion: isUpdate ? '0.9.0' : null, permissions: ['ui.panel', 'credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'] })
    await act(async () => root.render(<PublicPluginPanel client={client as unknown as LauncherClient} />))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="选择开发目录"]')!.click())
    expect(host.textContent).toContain('主程序密钥授权默认关闭')
    await click(isUpdate ? '确认更新' : '确认安装')
    expect(client.commitPublicPlugin).toHaveBeenCalledExactlyOnceWith({ input: { token: prepared.token, permissionGrants: ['ui.panel'] } })
  })
  it.each([false, true])('leaves service permissions for explicit detail consent on install/update (update=%s)', async (isUpdate) => {
    const client = { ...clientFixture(), listPublicPlugins: vi.fn(async () => ({ revision: '2', hotkeyRevision: '0', items: [] })) }
    client.preparePublicPlugin.mockResolvedValue({ ...prepared, isUpdate, operation: isUpdate ? 'replace' : 'install', previousVersion: isUpdate ? '0.9.0' : null, permissions: ['ui.panel', 'credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'] })
    await act(async () => root.render(<PublicPluginPanel client={client as unknown as LauncherClient} />))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="选择开发目录"]')!.click())
    expect(host.textContent).toContain('主程序密钥授权默认关闭')
    await click(isUpdate ? '确认更新' : '确认安装')
    expect(client.commitPublicPlugin).toHaveBeenCalledExactlyOnceWith({ input: { token: prepared.token, permissionGrants: ['ui.panel'] } })
  })
  it.each([false, true])('keeps safe failure after successful inventory reload (update=%s) and consumes confirmation', async (isUpdate) => {
    const client = {
      ...clientFixture(),
      listPublicPlugins: vi.fn(async () => ({ revision: '2', items: [] })),
    }
    client.preparePublicPlugin.mockResolvedValue({ ...prepared, isUpdate, operation: isUpdate ? 'replace' : 'install', previousVersion: isUpdate ? '0.9.0' : null })
    client.commitPublicPlugin.mockRejectedValueOnce({ code: 'runtimeNotReady', message: 'PRIVATE_SENTINEL' })
    await act(async () => root.render(<PublicPluginPanel client={client as unknown as LauncherClient} />))
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="选择开发目录"]')!.click())
    await click(isUpdate ? '确认更新' : '确认安装')
    expect(client.commitPublicPlugin).toHaveBeenCalledOnce()
    expect(client.listPublicPlugins).toHaveBeenCalledTimes(2)
    expect(host.querySelector('.public-prepare')).toBeNull()
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('未能就绪')
    expect(host.textContent).not.toContain('PRIVATE_SENTINEL')
    const select = host.querySelector<HTMLButtonElement>('button[aria-label="选择开发目录"]')!
    expect(select.disabled).toBe(false)
    await act(async () => select.click())
    expect(client.preparePublicPlugin).toHaveBeenCalledTimes(2)
    expect(host.querySelector('.public-prepare')).not.toBeNull()
  })
})

describe('public plugin recovery', () => {
  it('does not automatically grant host credentials when refreshing a development plugin', async () => {
    const client = clientFixture()
    client.preparePublicPlugin.mockResolvedValue({ ...prepared, permissions: ['network.https', 'credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'] })
    await mount(client); await click('重新载入'); await click('确认重新载入')
    expect(client.commitPublicPlugin).toHaveBeenCalledExactlyOnceWith({ input: { token: prepared.token, permissionGrants: ['network.https'] } })
  })
  it('does not automatically grant host credentials when refreshing a development plugin', async () => {
    const client = clientFixture()
    client.preparePublicPlugin.mockResolvedValue({ ...prepared, permissions: ['network.https', 'credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'] })
    await mount(client); await click('重新载入'); await click('确认重新载入')
    expect(client.commitPublicPlugin).toHaveBeenCalledExactlyOnceWith({ input: { token: prepared.token, permissionGrants: ['network.https'] } })
  })
  it('binds refresh target and consumes confirmation after a failed one-use commit', async () => {
    const client=clientFixture(); client.commitPublicPlugin.mockRejectedValueOnce({ code: 'runtimeNotReady', message: 'PRIVATE_SENTINEL' }); const reload=await mount(client)
    await click('重新载入')
    expect(client.preparePublicPlugin).toHaveBeenCalledWith({ source: { kind: 'developmentDirectory', path: 'synthetic/package' }, refreshTarget: { pluginId: plugin.pluginId, expectedInventoryRevision: '2' } })
    expect(host.textContent).toContain('1.0.0 → 1.0.0')
    await click('确认重新载入')
    expect(client.commitPublicPlugin).toHaveBeenCalledOnce(); expect(reload).toHaveBeenCalledOnce()
    expect(host.querySelector('.public-prepare')).toBeNull(); expect(host.textContent).toContain('未能就绪'); expect(host.textContent).not.toContain('PRIVATE_SENTINEL')
  })
  it('distinguishes mutation success from inventory failure and blocks duplicate mutation', async () => {
    const client=clientFixture(); const commit=deferred<void>(); client.commitPublicPlugin.mockReturnValueOnce(commit.promise); const reload=vi.fn(async () => { throw new Error('inventory') }); await mount(client,reload)
    await click('重新载入'); const confirm=buttons('确认重新载入')[0]!
    await act(async () => { confirm.click(); confirm.click() }); expect(client.commitPublicPlugin).toHaveBeenCalledOnce()
    await act(async () => commit.resolve())
    expect(host.textContent).toContain('操作已完成。无法刷新'); expect(host.querySelector('.public-prepare')).toBeNull()
  })
  it('copies the displayed snapshot exactly without refetching', async () => {
    const client=clientFixture(); await mount(client); await click('查看诊断')
    expect(host.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe(JSON.stringify(report,null,2))
    await click('复制此诊断'); expect(client.getPublicPluginDiagnostics).toHaveBeenCalledOnce(); expect(client.copyPublicPluginDiagnostics).toHaveBeenCalledWith({ report }); expect(host.textContent).toContain('状态未确认')
  })
  it.each([true, false])('deletes only after confirmation with retainData=%s', async (retainData) => {
    const client = { ...clientFixture(), uninstallPublicPlugin: vi.fn(async () => {}) }
    const removed = vi.fn(), reload = vi.fn(async () => {})
    await act(async () => root.render(<PublicPluginRecovery client={client as unknown as LauncherClient} plugin={plugin} inventoryRevision="2" reload={reload} onRemoved={removed} />))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="删除插件"]')!.click())
    expect(client.uninstallPublicPlugin).not.toHaveBeenCalled()
    expect(document.querySelector<HTMLInputElement>('[role="dialog"] input[type="checkbox"]')!.checked).toBe(true)
    await act(async () => [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(b => b.textContent === '取 消' || b.textContent === '取消')!.click())
    expect(client.uninstallPublicPlugin).not.toHaveBeenCalled()
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="删除插件"]')!.click())
    if (!retainData) await act(async () => document.querySelector<HTMLInputElement>('[role="dialog"] input[type="checkbox"]')!.click())
    await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="确认删除插件"]')!.click())
    expect(client.uninstallPublicPlugin).toHaveBeenCalledWith({ pluginId: plugin.pluginId, retainData })
    expect(removed).toHaveBeenCalledOnce()
  })
  it('keeps safe deletion failure feedback visible inside the confirmation', async () => {
    const client = { ...clientFixture(), uninstallPublicPlugin: vi.fn(async () => { throw new Error('PRIVATE_SENTINEL') }) }
    const removed = vi.fn()
    await act(async () => root.render(<PublicPluginRecovery client={client as unknown as LauncherClient} plugin={plugin} inventoryRevision="2" reload={async () => {}} onRemoved={removed} />))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="删除插件"]')!.click())
    await act(async () => document.querySelector<HTMLInputElement>('[role="dialog"] input[type="checkbox"]')!.click())
    await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="确认删除插件"]')!.click())
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.querySelector('[role="alert"]')?.textContent).toContain('无法删除插件')
    expect(dialog.textContent).not.toContain('PRIVATE_SENTINEL')
    expect(dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(false)
    expect(client.uninstallPublicPlugin).toHaveBeenCalledOnce()
    expect(removed).not.toHaveBeenCalled()
  })
  it('rejects old detail completions and cancels an orphaned prepared token', async () => {
    const client=clientFixture(); const diagnosis=deferred<PublicPluginDiagnosticReport>(); client.getPublicPluginDiagnostics.mockReturnValueOnce(diagnosis.promise); await mount(client); await click('查看诊断')
    await mount(client,undefined,{...plugin,pluginId:'com.example.other'})
    await act(async () => diagnosis.resolve(report)); expect(host.querySelector('textarea')).toBeNull()
    const preparation=deferred<PublicPluginPrepareSummary>(); client.preparePublicPlugin.mockReturnValueOnce(preparation.promise)
    await click('重新载入'); await mount(client,undefined,plugin)
    await act(async () => preparation.resolve(prepared)); expect(client.cancelPublicPlugin).toHaveBeenCalledWith({token:'one-use'}); expect(host.querySelector('.public-prepare')).toBeNull()
  })
  it('offers explicit re-enable only for disabled plugins', async () => {
    const client=clientFixture(); await mount(client); expect(buttons('重新启用')).toHaveLength(0)
    await mount(client,undefined,{...plugin,enabled:false,fault:'runtimeUnavailable'}); await click('重新启用'); expect(client.setPublicPluginEnabled).toHaveBeenCalledWith({pluginId:plugin.pluginId,enabled:true})
  })
})
