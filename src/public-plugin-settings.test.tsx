// @vitest-environment jsdom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { PublicPluginDetail } from './public-plugin-panel'
import type { LauncherClient, PublicPluginInventoryItem } from './protocol'

const base: PublicPluginInventoryItem = { pluginId: 'com.example.settings', name: 'Settings', version: '1.0.0', description: null, source: 'localPackage', installationDirectory: null, commands: [], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [
  { definition: { type: 'text', key: 'constructor', label: '文本', default: '你好' } },
  { definition: { type: 'number', key: 'count', label: '数量', min: 1, max: 5, step: 1, default: 2 } },
  { definition: { type: 'boolean', key: 'flag', label: '开关' } },
  { definition: { type: 'select', key: 'style', label: '样式', options: [{ value: '', label: '空选项' }, { value: 'boxed', label: '方框' }] } },
  { definition: { type: 'secret', key: 'token', label: '凭据' }, secretConfigured: true },
] }
let root: Root, host: HTMLDivElement
function deferred() { let resolve!: () => void; let reject!: () => void; const promise = new Promise<void>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
const clientFixture = () => ({ savePublicPluginSettings: vi.fn(async (_input: Parameters<LauncherClient['savePublicPluginSettings']>[0]) => {}), selectPublicPluginDirectory: vi.fn(async (): Promise<string | null> => null), cancelPublicPlugin: vi.fn(async () => {}) })
async function render(client: ReturnType<typeof clientFixture>, reload = vi.fn(async (_accept?: (revision: string) => void) => {}), plugin = base, revision = '1') {
  await act(async () => root.render(<PublicPluginDetail key={plugin.pluginId} client={client as unknown as LauncherClient} plugin={plugin} inventoryRevision={revision} reload={reload} />))
  return reload
}
async function input(label: string, value: string) { await act(async () => { const element = host.querySelector<HTMLInputElement | HTMLTextAreaElement>(`input[aria-label="${label}"], textarea[aria-label="${label}"]`)!; Object.getOwnPropertyDescriptor(element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value')!.set!.call(element, value); element.dispatchEvent(new Event('input', { bubbles: true })) }) }
function button(label: string) { return host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)! }
async function click(label: string) { await act(async () => button(label).click()) }
beforeEach(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } }); window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })); host = document.createElement('div'); document.body.append(host); root = createRoot(host) })
afterEach(async () => { await act(async () => root.unmount()); host.remove() })
it('shows global credential availability independently from plugin authorization', async () => {
  const client = { ...clientFixture(), credentials: { list: vi.fn(async () => ({ revision: '1', items: [{ service: 'youdaoTranslation', name: '有道', configured: true, enabled: false }] })) }, setPublicPluginPermissionAccess: vi.fn(async () => {}) }
  const plugin: PublicPluginInventoryItem = { ...base, settings: [], permissions: [{ permission: 'credentials.youdaoTranslation', supported: true, granted: true }, { permission: 'credentials.baiduOcr', supported: true, granted: false }, { permission: 'credentials.deepseek', supported: true, granted: false }] }
  const reload = await render(client, undefined, plugin)
  expect(host.textContent).toContain('主程序密钥')
  expect(host.textContent).toContain('有道翻译密钥 · credentials.youdaoTranslation')
  const rows = [...host.querySelectorAll('.host-credential-row')]
  expect(rows[0].textContent).toContain('配置已关闭'); expect(rows[0].textContent).toContain('已授权')
  expect(rows[1].textContent).toContain('尚未配置'); expect(rows[1].textContent).toContain('未授权')
  await click('有道翻译密钥')
  expect(client.setPublicPluginPermissionAccess).toHaveBeenCalledExactlyOnceWith({ pluginId: base.pluginId, permission: 'credentials.youdaoTranslation', granted: false, expectedInventoryRevision: '1' })
  expect(reload).toHaveBeenCalledOnce()
})
it('requires current credential consent and does not replay an authorization after a revision change', async () => {
  const client = { ...clientFixture(), setPublicPluginPermissionAccess: vi.fn(async () => {}) }
  const plugin: PublicPluginInventoryItem = { ...base, settings: [], permissions: [{ permission: 'credentials.deepseek', supported: true, granted: false }] }
  await render(client, undefined, plugin)
  expect(host.textContent).toContain('配置状态加载失败')
  await click('DeepSeek 密钥')
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('可能消耗 API 额度')
  await render(client, undefined, plugin, '2')
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="确认开启DeepSeek 密钥"]')!.click())
  expect(client.setPublicPluginPermissionAccess).not.toHaveBeenCalled()
  expect(host.textContent).toContain('插件清单已改变')
})
it('refreshes detail permissions from host changes and uses the current read owner', async () => {
  let changed: (() => void) | undefined
  const unsubscribe = vi.fn()
  const client = { ...clientFixture(), listenPublicPluginPermissionsChanged: vi.fn(async (handler: () => void) => { changed = handler; return unsubscribe }) }
  const firstRead = vi.fn(async () => {})
  await render(client, firstRead)
  expect(changed).toBeTypeOf('function')
  await act(async () => changed!())
  expect(firstRead).toHaveBeenCalledTimes(1)
  const nextRead = vi.fn(async () => {})
  await render(client, nextRead)
  await act(async () => changed!())
  expect(nextRead).toHaveBeenCalledTimes(1)
  expect(firstRead).toHaveBeenCalledTimes(1)
  await act(async () => root.render(null))
  expect(unsubscribe).toHaveBeenCalledTimes(1)
  await act(async () => changed!())
  expect(nextRead).toHaveBeenCalledTimes(1)
})
it('submits only dirty ordinary keys safely, keeps unset values absent and excludes secrets', async () => {
  const client = clientFixture(); await render(client)
  expect(button('保存普通设置').disabled).toBe(true)
  await input('文本', '  hello\n  '); await click('保存普通设置')
  expect(client.savePublicPluginSettings).toHaveBeenCalledWith({ input: { pluginId: base.pluginId, settings: { constructor: '  hello\n  ' }, secrets: {} } })
  expect(Object.keys(client.savePublicPluginSettings.mock.calls[0]![0].input.settings)).toEqual(['constructor'])
})
it('rejects empty and out-of-range numbers while step is only a hint', async () => {
  const client = clientFixture(); await render(client)
  await input('数量', ''); expect(host.textContent).toContain('请输入有限数字'); expect(button('保存普通设置').disabled).toBe(true)
  await input('数量', '6'); expect(host.textContent).toContain('不能大于 5')
  await input('数量', '2.5'); await click('保存普通设置'); expect(client.savePublicPluginSettings).toHaveBeenCalledWith({ input: { pluginId: base.pluginId, settings: { count: 2.5 }, secrets: {} } })
})
it('keeps uncertain drafts, only retries reads, and distinguishes a saved write with a failed refresh', async () => {
  const client = clientFixture(); client.savePublicPluginSettings.mockRejectedValueOnce(new Error('private'))
  let readFails = true
  const reload = vi.fn(async (_accept?: (revision: string) => void) => { if (readFails) throw new Error('read') }); await render(client, reload)
  await input('文本', 'draft'); await click('保存普通设置')
  expect(host.textContent).toContain('保存未确认'); expect(host.querySelector<HTMLInputElement>('textarea[aria-label="文本"]')!.value).toBe('draft')
  expect(button('保存普通设置').disabled).toBe(true); await click('重新读取普通设置'); expect(client.savePublicPluginSettings).toHaveBeenCalledOnce(); expect(reload).toHaveBeenCalledTimes(2)
  expect(host.textContent).toContain('保存未确认'); expect(host.textContent).not.toContain('已保存')
  readFails = false; await click('重新读取普通设置')
  expect(host.textContent).toContain('保存未确认'); expect(button('保存普通设置').disabled).toBe(true); expect(client.savePublicPluginSettings).toHaveBeenCalledOnce()
  readFails = true
  await render(client, vi.fn(async () => {}), { ...base, pluginId: 'com.example.other' }); await input('文本', 'saved')
  await render(client, reload, { ...base, pluginId: 'com.example.other' }); await click('保存普通设置'); expect(host.textContent).toContain('已保存，无法刷新当前状态')
  await click('重新读取普通设置')
  expect(host.textContent).toContain('已保存，无法刷新当前状态'); expect(host.textContent).not.toContain('保存未确认'); expect(button('保存普通设置').disabled).toBe(true)
  readFails = false; await click('重新读取普通设置')
  expect(host.textContent).toContain('已保存'); expect(host.textContent).toContain('已重新读取'); expect(host.textContent).not.toContain('保存未确认'); expect(button('保存普通设置').disabled).toBe(true)
  expect(client.savePublicPluginSettings).toHaveBeenCalledTimes(2)
})
it('admits one save and excludes recovery until reconciliation, then ignores a closed detail completion', async () => {
  const client = clientFixture(), save = deferred(); client.savePublicPluginSettings.mockReturnValueOnce(save.promise); const reload = await render(client)
  await input('文本', 'draft'); await act(async () => { button('保存普通设置').click(); button('保存普通设置').click(); [...host.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === '重新载入')!.click() })
  expect(client.savePublicPluginSettings).toHaveBeenCalledOnce(); expect(client.selectPublicPluginDirectory).not.toHaveBeenCalled()
  await render(client, undefined, { ...base, pluginId: 'com.example.other' }); await act(async () => save.resolve()); expect(reload).not.toHaveBeenCalled(); expect(host.textContent).not.toContain('已保存')
})
it('keeps drafts on unrelated inventory updates but requires review after definition/value changes', async () => {
  const client = clientFixture(); await render(client); await input('文本', 'draft'); await render(client, undefined, { ...base, generation: 2 }, '2')
  expect(host.querySelector<HTMLInputElement>('textarea[aria-label="文本"]')!.value).toBe('draft'); expect(button('保存普通设置').disabled).toBe(false)
  await render(client, undefined, { ...base, settings: base.settings.map(s => s.definition.key === 'constructor' ? { ...s, value: 'external' } : s) }, '3')
  expect(host.textContent).toContain('设置状态已变化'); expect(button('保存普通设置').disabled).toBe(true); await click('取消普通设置修改'); expect(host.querySelector<HTMLInputElement>('textarea[aria-label="文本"]')!.value).toBe('external')
})
it('distinguishes a legal empty select option and explicit false from unset', async () => {
  const client = clientFixture(); await render(client)
  expect(host.querySelector('.ant-select-placeholder')?.textContent).toBe('请选择（未设置）')
  await act(async () => host.querySelector<HTMLElement>('[role="combobox"][aria-label="样式"]')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })))
  await act(async () => { [...document.body.querySelectorAll<HTMLElement>('.ant-select-item-option')].find(option => option.textContent === '空选项')!.click(); button('开关').click() })
  await click('开关'); await click('保存普通设置')
  expect(client.savePublicPluginSettings).toHaveBeenCalledWith({ input: { pluginId: base.pluginId, settings: { flag: false, style: '' }, secrets: {} } })
})
it('marks an owned accepted refresh saved, but preserves failed drafts when durable values changed', async () => {
  const client = clientFixture()
  let durable = base
  const reload = vi.fn(async (accept?: (revision: string) => void) => { accept?.('2'); root.render(<PublicPluginDetail key={durable.pluginId} client={client as unknown as LauncherClient} plugin={durable} inventoryRevision="2" reload={reload} />) })
  await render(client, reload); await input('文本', 'saved')
  durable = { ...base, settings: base.settings.map(s => s.definition.key === 'constructor' ? { ...s, value: 'saved' } : s) }
  await click('保存普通设置'); expect(host.textContent).toContain('已保存'); expect(host.textContent).not.toContain('设置状态已变化'); expect(button('保存普通设置').disabled).toBe(true)
  await input('文本', 'failed draft'); client.savePublicPluginSettings.mockRejectedValueOnce(new Error('private'))
  durable = { ...durable, settings: durable.settings.map(s => s.definition.key === 'constructor' ? { ...s, value: 'partial' } : s) }
  await click('保存普通设置'); expect(host.querySelector<HTMLInputElement>('textarea[aria-label="文本"]')!.value).toBe('failed draft'); expect(button('保存普通设置').disabled).toBe(true)
  await click('取消普通设置修改'); expect(host.querySelector<HTMLInputElement>('textarea[aria-label="文本"]')!.value).toBe('partial')
})
it('blocks ordinary editing and saving during a pending recovery confirmation', async () => {
  const client = { ...clientFixture(), selectPublicPluginDirectory: vi.fn(async () => 'synthetic/package'), preparePublicPlugin: vi.fn(async () => ({ token: 'one-use', pluginId: base.pluginId, name: base.name, version: '1.0.0', permissions: [], isUpdate: true, operation: 'developmentRefresh', previousVersion: '1.0.0', sourceVerified: false, iconUrl: null, network: null })) }
  await render(client); await input('文本', 'draft')
  await act(async () => [...host.querySelectorAll<HTMLButtonElement>('button')].find(b => b.textContent === '重新载入')!.click())
  expect(host.textContent).toContain('确认重新载入'); expect(button('保存普通设置').disabled).toBe(true)
  await click('保存普通设置'); expect(client.savePublicPluginSettings).not.toHaveBeenCalled()
})
it('keeps read recovery mandatory when an external definition change retires a pending save', async () => {
  const client = clientFixture(), save = deferred(); client.savePublicPluginSettings.mockReturnValueOnce(save.promise)
  const reload = vi.fn(async () => { throw new Error('read unavailable') }); await render(client, reload); await input('文本', 'draft'); await click('保存普通设置')
  await render(client, reload, { ...base, settings: base.settings.map(s => s.definition.key === 'constructor' ? { ...s, definition: { ...s.definition, label: '已变更文本' } } : s) }, '2')
  await act(async () => save.resolve()); expect(button('重新读取普通设置')).not.toBeNull(); expect(button('取消普通设置修改').disabled).toBe(true); expect(button('保存普通设置').disabled).toBe(true)
})


it('command hotkey clear retries only reload and recovers keyboard focus', async () => {
  const client = Object.assign(clientFixture(), { setPublicPluginCommandHotkey: vi.fn(async () => {}) })
  const plugin = { ...base, commands: [{ commandId: 'default', defaultName: 'settings', effectiveName: 'settings', nameOverride: null, favorite: false, hotkey: 'control+KeyN', hotkeyStatus: 'suspended' as const }] }
  let readFails = true
  const reload = vi.fn(async () => { if (readFails) throw new Error('read') })
  await render(client, reload, plugin)
  expect(host.textContent).toContain('已暂停')
  button('清除命令快捷键').focus()
  await click('清除命令快捷键')
  expect(client.setPublicPluginCommandHotkey).toHaveBeenCalledWith({ pluginId: base.pluginId, commandId: 'default', hotkey: null })
  expect(host.textContent).toContain('无法刷新当前状态')
  expect(button('清除命令快捷键').disabled).toBe(true)
  readFails = false
  await click('刷新命令状态')
  expect(client.setPublicPluginCommandHotkey).toHaveBeenCalledOnce()
  expect(reload).toHaveBeenCalledTimes(2)
  expect(button('清除命令快捷键').disabled).toBe(false)
})

it('command hotkey shared recorder deduplicates DOM/native capture and disallows double tap', async () => {
  const client = Object.assign(clientFixture(), { setPublicPluginCommandHotkey: vi.fn(async () => {}) })
  const plugin = { ...base, commands: [{ commandId: 'default', defaultName: 'settings', effectiveName: 'settings', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }] }
  await render(client, undefined, plugin)
  await click('录制命令快捷键')
  const recorder = host.querySelector<HTMLInputElement>('input[aria-label="命令快捷键"]')!
  await act(async () => {
    recorder.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true }))
    recorder.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', code: 'ControlLeft', ctrlKey: true, bubbles: true }))
  })
  expect(client.setPublicPluginCommandHotkey).not.toHaveBeenCalled()
  await act(async () => {
    recorder.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', code: 'KeyN', ctrlKey: true, bubbles: true }))
    window.dispatchEvent(new CustomEvent('uipilot-hotkey-recording-current', { detail: 'control+KeyN' }))
    recorder.dispatchEvent(new KeyboardEvent('keyup', { key: 'n', code: 'KeyN', ctrlKey: true, bubbles: true }))
    window.dispatchEvent(new CustomEvent('uipilot-hotkey-recording-current', { detail: 'control+KeyN' }))
  })
  expect(client.setPublicPluginCommandHotkey).toHaveBeenCalledOnce()
  expect(client.setPublicPluginCommandHotkey).toHaveBeenCalledWith({ pluginId: base.pluginId, commandId: 'default', hotkey: 'control+KeyN' })
})
