// @vitest-environment jsdom
import { act, createRef } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { MacPluginManagement, type MacPluginManagementHandle } from './macos-plugin-management'
import type { MacPluginManagementClient, MacPluginRow, MacPluginSnapshot } from './macos-plugin-management-client'
const row: MacPluginRow = { pluginId: 'com.test.settings', name: '设置示例', description: '普通设置', version: '1.0.0', enabled: false, commands: [{ id: 'default', name: 'settings-demo', defaultName: 'settings-demo', nameOverride: null, favorite: false }], permissions: [{ permission: 'clipboard.write', granted: false }], settings: [{ type: 'text', key: 'greeting', label: '问候', default: '你好' }, { type: 'number', key: 'count', label: '数量', min: 1, max: 5 }], values: { greeting: '你好', count: 2 } }
const snapshot: MacPluginSnapshot = { revision: '12', plugins: [row] }
function fixture() { return { list: vi.fn(async () => snapshot), prepare: vi.fn(async () => ({ token: 'token-1', expectedRevision: '12', plugin: { ...row, version: '1.0.1' }, requiredPermissions: ['clipboard.write' as const] })), commit: vi.fn(async () => ({ ...snapshot, revision: '13' })), cancel: vi.fn(async () => {}), enabled: vi.fn(async () => snapshot), permission: vi.fn(async () => snapshot), settings: vi.fn(async () => ({ ...snapshot, revision: '13' })), command: vi.fn<MacPluginManagementClient['command']>(async () => snapshot), remove: vi.fn(async () => ({ revision: '13', plugins: [] })) } satisfies MacPluginManagementClient }
let root: Root, host: HTMLDivElement
beforeEach(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } }); window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })); host = document.createElement('div'); document.body.append(host); root = createRoot(host) })
afterEach(async () => { await act(async () => root.unmount()); host.remove(); document.body.innerHTML = '' })
function button(name: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => b.getAttribute('aria-label') === name || b.textContent?.replace(/\s/g, '') === name.replace(/\s/g, ''))! }
async function click(name: string) { await act(async () => button(name).click()) }
async function select() { await act(async () => host.querySelector<HTMLButtonElement>('.mac-plugin-row')!.click()) }
async function input(label: string, value: string) { await act(async () => { const e = host.querySelector<HTMLInputElement | HTMLTextAreaElement>(`input[aria-label="${label}"],textarea[aria-label="${label}"]`)!; Object.getOwnPropertyDescriptor(e instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value')!.set!.call(e, value); e.dispatchEvent(new Event('input', { bubbles: true })) }) }
async function render(client = fixture(), ref = createRef<MacPluginManagementHandle>()) { const onStatusChange = vi.fn(); await act(async () => root.render(<MacPluginManagement client={client} ref={ref} onStatusChange={onStatusChange} />)); return { client, ref, onStatusChange } }
it('lists inactive installed packages without executing them and saves ordinary settings while disabled', async () => {
  const { client, onStatusChange } = await render(); expect(host.textContent).toContain('当前 Mac 插件暂不能运行'); await select()
  await input('问候', '中文 😀'); await click('保存设置')
  expect(client.settings).toHaveBeenCalledWith({ pluginId: row.pluginId, expectedRevision: '12', updates: { greeting: '中文 😀' } })
  expect(onStatusChange).toHaveBeenLastCalledWith('设置已保存。')
  expect(host.textContent).not.toContain('设置已保存。')
  await input('问候', '新的草稿')
  expect(onStatusChange).toHaveBeenLastCalledWith('')
  expect(client.enabled).not.toHaveBeenCalled()
})
it('requires explicit installation confirmation with exactly the prepared permission set and revision', async () => {
  const { client } = await render(); await click('导入插件包')
  expect(client.prepare).toHaveBeenCalledWith({ kind: 'archive', expectedRevision: '12' })
  expect(client.commit).not.toHaveBeenCalled(); expect(document.body.textContent).toContain('替换已安装插件')
  expect(document.body.textContent).toContain('当前版本：1.0.0')
  expect(document.body.textContent).toContain('目标版本：1.0.1')
  await click('确认替换')
  expect(client.commit).toHaveBeenCalledWith({ token: 'token-1', expectedRevision: '12', approvedPermissions: ['clipboard.write'] })
  expect(host.textContent).toContain('当前暂不能运行')
})
it('moves focus only among visible plugin rows and preserves input method and modifier keys', async () => {
  const client = fixture()
  client.list.mockResolvedValueOnce({ ...snapshot, plugins: [row, { ...row, pluginId: 'com.test.second', name: '第二个插件' }, { ...row, pluginId: 'com.test.third', name: '第三个插件' }] })
  await render(client)
  const rows = () => [...host.querySelectorAll<HTMLButtonElement>('.mac-plugin-row')]
  const key = async (name: string, options: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...options })
    await act(async () => { document.activeElement!.dispatchEvent(event) })
    return event
  }
  rows()[0].focus()
  expect((await key('ArrowDown')).defaultPrevented).toBe(true)
  expect(document.activeElement).toBe(rows()[1])
  expect((await key('ArrowUp', { isComposing: true })).defaultPrevented).toBe(false)
  expect((await key('ArrowUp', { keyCode: 229 })).defaultPrevented).toBe(false)
  expect((await key('ArrowUp', { metaKey: true })).defaultPrevented).toBe(false)
  expect(document.activeElement).toBe(rows()[1])
  await key('End'); expect(document.activeElement).toBe(rows()[2])
  await key('ArrowDown'); expect(document.activeElement).toBe(rows()[2])
  await key('Home'); expect(document.activeElement).toBe(rows()[0])
  await input('搜索插件名称或启动命令', '第二个')
  rows()[0].focus(); await key('ArrowDown'); expect(document.activeElement).toBe(rows()[0])
  expect(rows()).toHaveLength(1)
  await act(async () => rows()[0].click())
  expect(host.querySelector('h2[tabindex="-1"]')?.textContent).toBe('第二个插件')
  await click('返回插件列表'); expect(document.activeElement).toBe(rows()[0])
  expect(client.enabled).not.toHaveBeenCalled(); expect(client.commit).not.toHaveBeenCalled()
})
it('labels a new install without implying an existing version and cancels without writing', async () => {
  const client = fixture(); client.list.mockResolvedValueOnce({ revision: '12', plugins: [] })
  await render(client); await click('导入插件包')
  expect(document.body.textContent).not.toContain('当前版本：')
  expect(button('确认安装')).toBeDefined()
  await click('取消'); expect(client.commit).not.toHaveBeenCalled()
  expect(host.textContent).toContain('尚未安装插件')
})
it('cancels install tokens explicitly and cancels a late chooser result after unmount', async () => {
  const client = fixture(); await render(client); await click('导入开发目录'); await click('取消')
  expect(client.cancel).toHaveBeenCalledWith({ token: 'token-1' })
  let resolve!: (v: Awaited<ReturnType<typeof client.prepare>>) => void
  client.prepare.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  await click('导入插件包'); await act(async () => root.render(null))
  await act(async () => resolve({ token: 'late', expectedRevision: '12', plugin: row, requiredPermissions: [] }))
  expect(client.cancel).toHaveBeenCalledWith({ token: 'late' }); expect(client.commit).not.toHaveBeenCalled()
})
it('preserves dirty inputs and selection when leaving is cancelled, then leaves only after discard', async () => {
  const { ref } = await render(); await select(); await input('问候', '草稿')
  const leave = vi.fn(); await act(async () => ref.current!.requestLeave(leave)); await click('取消')
  expect(leave).not.toHaveBeenCalled(); expect(host.querySelector<HTMLInputElement>('[aria-label="问候"]')!.value).toBe('草稿')
  await act(async () => ref.current!.requestLeave(leave)); await click('放弃修改'); expect(leave).toHaveBeenCalledOnce()
})
it('rejects invalid ordinary values locally and prevents stale writes from being retried', async () => {
  const client = fixture(); client.settings.mockRejectedValueOnce('pluginManagementStale'); await render(client); await select()
  await input('数量', '9'); expect(button('保存设置').disabled).toBe(true); expect(client.settings).not.toHaveBeenCalled()
  await input('数量', '3'); await click('保存设置'); expect(host.textContent).toContain('插件配置已变化')
  expect(button('保存设置').disabled).toBe(true); expect(host.querySelector<HTMLInputElement>('[aria-label="数量"]')!.value).toBe('3')
  expect(client.settings).toHaveBeenCalledOnce()
})
it('guards double submissions and only publishes confirmed permission configuration', async () => {
  const client = fixture(); let resolve!: (s: MacPluginSnapshot) => void
  client.permission.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  await render(client); await select()
  const toggle = button('允许写入剪贴板'); await act(async () => { toggle.click(); toggle.click() })
  expect(client.permission).toHaveBeenCalledOnce()
  expect(client.permission).toHaveBeenCalledWith({ pluginId: row.pluginId, permission: 'clipboard.write', granted: true, expectedRevision: '12' })
  await act(async () => resolve({ ...snapshot, revision: '13', plugins: [{ ...row, permissions: [{ permission: 'clipboard.write', granted: true }] }] }))
  expect(button('允许写入剪贴板').getAttribute('aria-checked')).toBe('true')
})
it('describes storage retention accurately and removes only after confirmation', async () => {
  const { client } = await render(); await select(); await click('移除插件')
  expect(document.body.textContent).toContain('安装和普通设置，保留插件存储数据')
  expect(client.remove).not.toHaveBeenCalled(); await click('取消'); expect(client.remove).not.toHaveBeenCalled()
  await click('移除插件'); await click('移除安装和设置')
  expect(client.remove).toHaveBeenCalledWith({ pluginId: row.pluginId, expectedRevision: '12' })
  expect(host.textContent).toContain('尚未安装插件')
})
it('keeps optional constructor unset and saves multiline text without flattening it', async () => {
  const client = fixture(); client.list.mockResolvedValueOnce({ ...snapshot, plugins: [{ ...row, settings: [{ type: 'text', key: 'constructor', label: '可选文本' }], values: {} }] })
  await render(client); await select()
  expect(host.querySelector<HTMLTextAreaElement>('[aria-label="可选文本"]')!.value).toBe('')
  await input('可选文本', '中文\n第二行'); await click('保存设置')
  expect(client.settings).toHaveBeenCalledWith({ pluginId: row.pluginId, expectedRevision: '12', updates: { constructor: '中文\n第二行' } })
})
it('does not allow a late read from an old client session to replace the new inventory', async () => {
  const old = fixture(); let resolve!: (s: MacPluginSnapshot) => void
  old.list.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  await render(old)
  const next = fixture(); next.list.mockResolvedValueOnce({ revision: '20', plugins: [] })
  await render(next)
  await act(async () => resolve(snapshot))
  expect(host.textContent).toContain('尚未安装插件')
  expect(host.querySelector('.mac-plugin-row')).toBeNull()
})
it('Escape cancels install confirmation and preserves a dirty draft when cancelling discard', async () => {
  const { client, ref } = await render(); await click('导入插件包')
  const escape = async () => { await act(async () => { document.querySelector<HTMLElement>('.ant-modal-wrap')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true })) }) }
  await escape(); expect(client.cancel).toHaveBeenCalledWith({ token: 'token-1' }); expect(client.commit).not.toHaveBeenCalled()
  await select(); await input('问候', '草稿'); const leave = vi.fn()
  await act(async () => ref.current!.requestLeave(leave)); await escape()
  expect(leave).not.toHaveBeenCalled(); expect(host.querySelector<HTMLTextAreaElement>('[aria-label="问候"]')!.value).toBe('草稿')
})

async function chooseCommand(id: string) {
  await act(async () => host.querySelector('[aria-label="选择命令"]')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })))
  const option = [...document.querySelectorAll<HTMLElement>('.ant-select-item-option')].find(o => o.textContent?.startsWith(`${id} ·`))!
  expect(option).toBeDefined()
  await act(async () => option.click())
}
it('keeps command favorites independent and saves rename by stable ID with the current revision', async () => {
  const client = fixture()
  let current = { ...snapshot, plugins: [{ ...row, commands: [
    { id: 'lower', name: 'demo-lower', defaultName: 'demo-lower', nameOverride: null as string | null, favorite: false },
    { id: 'upper', name: 'demo-upper', defaultName: 'demo-upper', nameOverride: null as string | null, favorite: true },
  ] }] }
  client.list.mockImplementation(async () => current)
  client.command.mockImplementation(async input => {
    current = { ...current, revision: String(Number(current.revision) + 1), plugins: current.plugins.map(p => ({ ...p, commands: p.commands.map(c => c.id === input.commandId ? { ...c, name: input.nameOverride ?? c.defaultName, nameOverride: input.nameOverride, favorite: input.favorite } : c) })) }
    return current
  })
  await render(client); await select()
  expect(button('设为常用').getAttribute('aria-pressed')).toBe('false')
  await chooseCommand('upper')
  expect(button('取消常用').getAttribute('aria-pressed')).toBe('true')
  await input('启动名称', '/mac-upper'); await click('保存启动名称')
  expect(client.command).toHaveBeenLastCalledWith({ pluginId: row.pluginId, commandId: 'upper', nameOverride: 'mac-upper', favorite: true, expectedRevision: '12' })
  expect(button('取消常用').getAttribute('aria-pressed')).toBe('true')
  await click('取消常用')
  expect(client.command).toHaveBeenLastCalledWith({ pluginId: row.pluginId, commandId: 'upper', nameOverride: 'mac-upper', favorite: false, expectedRevision: '13' })
  await chooseCommand('lower')
  expect(host.querySelector<HTMLInputElement>('[aria-label="启动名称"]')!.value).toBe('demo-lower')
  expect(button('设为常用').getAttribute('aria-pressed')).toBe('false')
  await chooseCommand('upper'); await click('恢复默认名称'); await click('保存启动名称')
  expect(client.command).toHaveBeenLastCalledWith({ pluginId: row.pluginId, commandId: 'upper', nameOverride: null, favorite: false, expectedRevision: '14' })
})
it('guards unsaved names and invalid input, and never retries a conflicting name automatically', async () => {
  const client = fixture(); await render(client); await select()
  await input('启动名称', 'Two Words'); expect(button('保存启动名称').disabled).toBe(true)
  expect(button('设为常用').disabled).toBe(true)
  await click('返回插件列表'); await click('取消')
  expect(host.querySelector<HTMLInputElement>('[aria-label="启动名称"]')!.value).toBe('Two Words')
  await input('启动名称', 'find')
  client.command.mockRejectedValueOnce('pluginManagementConflict')
  await click('保存启动名称')
  expect(host.textContent).toContain('启动命令与内置命令或已安装插件冲突')
  expect(button('保存启动名称').disabled).toBe(true)
  expect(host.querySelector<HTMLInputElement>('[aria-label="启动名称"]')!.value).toBe('find')
  expect(client.command).toHaveBeenCalledOnce()
  await click('刷新插件列表'); await click('放弃修改')
  expect(host.querySelector<HTMLInputElement>('[aria-label="启动名称"]')!.value).toBe('settings-demo')
})
