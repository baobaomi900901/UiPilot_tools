// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { PublicPluginRecovery } from './public-plugin-panel'
import type { LauncherClient, PublicPluginDiagnosticReport, PublicPluginInventoryItem, PublicPluginPrepareSummary } from './protocol'

const plugin: PublicPluginInventoryItem = { pluginId: 'com.example.recovery', name: 'Recovery', version: '1.0.0', description: null, source: 'localPackage', commands: [], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [] }
const prepared: PublicPluginPrepareSummary = { token: 'one-use', pluginId: plugin.pluginId, name: 'Recovery', version: '1.0.0', permissions: [], isUpdate: true, operation: 'developmentRefresh', previousVersion: '1.0.0', sourceVerified: false, iconUrl: null, network: null }
const report: PublicPluginDiagnosticReport = { schemaVersion: 1, hostVersion: '0.7.0', platform: 'windows', pluginId: plugin.pluginId, state: null, lastOperation: null }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((a,b) => { resolve=a; reject=b }); return { promise, resolve, reject } }
let root: Root, host: HTMLDivElement
function buttons(text: string) { return [...host.querySelectorAll<HTMLButtonElement>('button')].filter(b => b.textContent === text) }
async function click(text: string) { await act(async () => buttons(text)[0]!.click()) }
function clientFixture() { return { selectPublicPluginDirectory: vi.fn(async () => 'synthetic/package'), preparePublicPlugin: vi.fn(async () => prepared), commitPublicPlugin: vi.fn(async () => {}), cancelPublicPlugin: vi.fn(async () => {}), setPublicPluginEnabled: vi.fn(async () => {}), getPublicPluginDiagnostics: vi.fn(async () => report), copyPublicPluginDiagnostics: vi.fn(async () => {}) } }
async function mount(client: ReturnType<typeof clientFixture>, reload = vi.fn(async () => {}), item = plugin) { await act(async () => root.render(<PublicPluginRecovery key={item.pluginId} client={client as unknown as LauncherClient} plugin={item} inventoryRevision="2" reload={reload} />)); return reload }
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }))
  host = document.createElement('div'); document.body.append(host); root=createRoot(host)
})
afterEach(async () => { await act(async () => root.unmount()); host.remove() })
describe('public plugin recovery', () => {
  it('binds refresh target and consumes confirmation after a failed one-use commit', async () => {
    const client=clientFixture(); client.commitPublicPlugin.mockRejectedValueOnce({ code: 'runtimeNotReady', message: 'PRIVATE_SENTINEL' }); const reload=await mount(client)
    await click('重新载入开发目录')
    expect(client.preparePublicPlugin).toHaveBeenCalledWith({ source: { kind: 'developmentDirectory', path: 'synthetic/package' }, refreshTarget: { pluginId: plugin.pluginId, expectedInventoryRevision: '2' } })
    expect(host.textContent).toContain('1.0.0 → 1.0.0')
    await click('确认重新载入')
    expect(client.commitPublicPlugin).toHaveBeenCalledOnce(); expect(reload).toHaveBeenCalledOnce()
    expect(host.querySelector('.public-prepare')).toBeNull(); expect(host.textContent).toContain('未能就绪'); expect(host.textContent).not.toContain('PRIVATE_SENTINEL')
  })
  it('distinguishes mutation success from inventory failure and blocks duplicate mutation', async () => {
    const client=clientFixture(); const commit=deferred<void>(); client.commitPublicPlugin.mockReturnValueOnce(commit.promise); const reload=vi.fn(async () => { throw new Error('inventory') }); await mount(client,reload)
    await click('重新载入开发目录'); const confirm=buttons('确认重新载入')[0]!
    await act(async () => { confirm.click(); confirm.click() }); expect(client.commitPublicPlugin).toHaveBeenCalledOnce()
    await act(async () => commit.resolve())
    expect(host.textContent).toContain('操作已完成。无法刷新'); expect(host.querySelector('.public-prepare')).toBeNull()
  })
  it('copies the displayed snapshot exactly without refetching', async () => {
    const client=clientFixture(); await mount(client); await click('查看诊断')
    expect(host.querySelector('pre')?.textContent).toBe(JSON.stringify(report,null,2))
    await click('复制此诊断'); expect(client.getPublicPluginDiagnostics).toHaveBeenCalledOnce(); expect(client.copyPublicPluginDiagnostics).toHaveBeenCalledWith({ report }); expect(host.textContent).toContain('状态未确认')
  })
  it('rejects old detail completions and cancels an orphaned prepared token', async () => {
    const client=clientFixture(); const diagnosis=deferred<PublicPluginDiagnosticReport>(); client.getPublicPluginDiagnostics.mockReturnValueOnce(diagnosis.promise); await mount(client); await click('查看诊断')
    await mount(client,undefined,{...plugin,pluginId:'com.example.other'})
    await act(async () => diagnosis.resolve(report)); expect(host.querySelector('pre')).toBeNull()
    const preparation=deferred<PublicPluginPrepareSummary>(); client.preparePublicPlugin.mockReturnValueOnce(preparation.promise)
    await click('重新载入开发目录'); await mount(client,undefined,plugin)
    await act(async () => preparation.resolve(prepared)); expect(client.cancelPublicPlugin).toHaveBeenCalledWith({token:'one-use'}); expect(host.querySelector('.public-prepare')).toBeNull()
  })
  it('offers explicit re-enable only for disabled plugins', async () => {
    const client=clientFixture(); await mount(client); expect(buttons('重新启用')).toHaveLength(0)
    await mount(client,undefined,{...plugin,enabled:false,fault:'runtimeUnavailable'}); await click('重新启用'); expect(client.setPublicPluginEnabled).toHaveBeenCalledWith({pluginId:plugin.pluginId,enabled:true})
  })
})
