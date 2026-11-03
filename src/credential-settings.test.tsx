// @vitest-environment jsdom
import { act, createRef } from 'react'
import { createRoot } from 'react-dom/client'
import { ConfigProvider } from 'antd'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CredentialSettings, type CredentialSettingsHandle } from './credential-settings'
import type { CredentialClient, CredentialEntry, CredentialInventory } from './credential-protocol'

;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
const cleanups: (() => void)[] = []
const entry: CredentialEntry = { service: 'youdaoTranslation', name: '有道翻译', enabled: false, configured: false }
const inventory = (items = [entry], revision = '1'): CredentialInventory => ({ revision, items })
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }
beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: vi.fn(() => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} })) })
  const computedStyle = window.getComputedStyle.bind(window)
  vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => computedStyle(element))
})
afterEach(async () => { await act(async () => cleanups.splice(0).forEach((cleanup) => cleanup())); document.body.innerHTML = ''; vi.restoreAllMocks(); vi.unstubAllGlobals() })
async function setup(initial = inventory(), unavailable = false, guardedTab = false) {
  const client = { list: vi.fn(async () => initial), add: vi.fn<CredentialClient['add']>(async () => initial), save: vi.fn<CredentialClient['save']>(async () => initial), remove: vi.fn<CredentialClient['remove']>(async () => inventory([], '2')), setEnabled: vi.fn<CredentialClient['setEnabled']>(async () => initial) } satisfies CredentialClient
  const host = document.createElement('div'); document.body.append(host); const root = createRoot(host)
  const ref = createRef<CredentialSettingsHandle>(); const status = vi.fn(); const leave = vi.fn()
  await act(async () => root.render(<ConfigProvider theme={{ token: { motion: false } }}><div onFocusCapture={(event) => {
    if ((event.target as HTMLElement).getAttribute('role') === 'tab') ref.current?.requestLeave(leave)
  }}>{guardedTab ? <button type="button" role="tab">权限</button> : null}<CredentialSettings ref={ref} client={unavailable ? undefined : client} onStatusChange={status} /></div></ConfigProvider>))
  cleanups.push(() => root.unmount()); return { client, host, ref, status, leave }
}
function button(label: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find((node) => node.getAttribute('aria-label') === label || node.textContent?.replace(/\s/g, '') === label)! }
async function click(node: HTMLElement) { await act(async () => node.click()) }
async function input(label: string, value: string) {
  const control = document.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(control, value); control.dispatchEvent(new Event('input', { bubbles: true })) })
}

describe('CredentialSettings', () => {
  it('loads persisted entries in the three groups and prevents unconfigured enable', async () => {
    const { host } = await setup()
    expect([...host.querySelectorAll('h3')].map((node) => node.textContent)).toEqual(['翻译 / Translation', '文字识别 / OCR', '大模型 / LLM'])
    expect(button('启用有道翻译').disabled).toBe(true)
    expect(host.textContent).not.toContain('有道 OCR')
  })
  it('shows a static unavailable error when there is no credential client', async () => {
    const { host } = await setup(inventory(), true)
    expect(host.textContent).toContain('密钥配置服务不可用')
    expect(button('添加配置').disabled).toBe(true)
  })
  it('validates required fields and waits for persisted success before clearing secrets', async () => {
    const { client, status, ref } = await setup(); await click(button('有道翻译'))
    expect(button('验证并保存').disabled).toBe(true)
    await input('App ID', 'synthetic-app'); await input('App Key', 'synthetic-secret')
    const pending = deferred<CredentialInventory>(); client.save.mockReturnValueOnce(pending.promise)
    await click(button('验证并保存'))
    expect(client.save).toHaveBeenCalledExactlyOnceWith({ service: 'youdaoTranslation', name: '有道翻译', values: { appId: 'synthetic-app', appKey: 'synthetic-secret' }, expectedRevision: '1' })
    expect(document.querySelector<HTMLInputElement>('input[aria-label="App Key"]')!.disabled).toBe(true)
    const leave = vi.fn(); await act(async () => ref.current!.requestLeave(leave)); expect(leave).not.toHaveBeenCalled()
    expect(status.mock.calls[status.mock.calls.length - 1]?.[0].message).toContain('请等待')
    await act(async () => pending.resolve(inventory([{ ...entry, configured: true }], '2')))
    expect(status.mock.calls[status.mock.calls.length - 1]?.[0].message).toBe('验证成功，已保存')
    expect(document.querySelector<HTMLInputElement>('input[aria-label="App Key"]')!.value).toBe('')
    await input('名称', '工作翻译'); await click(button('验证并保存'))
    expect(client.save.mock.calls[client.save.mock.calls.length - 1]?.[0]).toEqual({ service: 'youdaoTranslation', name: '工作翻译', values: {}, expectedRevision: '2' })
  })
  it('retains failed draft and never exposes raw error messages', async () => {
    const { client, host } = await setup(); await click(button('有道翻译'))
    await input('App ID', 'synthetic-app'); await input('App Key', 'synthetic-secret')
    client.save.mockRejectedValueOnce({ code: 'invalidCredentials', message: 'raw-private-response' })
    await click(button('验证并保存'))
    expect(host.textContent).toContain('凭据验证未通过')
    expect(document.body.textContent).not.toContain('raw-private-response')
    expect(document.querySelector<HTMLInputElement>('input[aria-label="App Key"]')!.value).toBe('synthetic-secret')
  })
  it('asks before discarding edits and cancellation retains inputs', async () => {
    const { ref } = await setup(); await click(button('有道翻译')); await input('名称', '未保存名称')
    const leave = vi.fn(); await act(async () => ref.current!.requestLeave(leave))
    expect(leave).not.toHaveBeenCalled(); expect(document.querySelector('[role="dialog"]')).not.toBeNull()
    await click(button('取消')); expect(document.querySelector<HTMLInputElement>('input[aria-label="名称"]')!.value).toBe('未保存名称')
    await act(async () => ref.current!.requestLeave(leave)); await click(button('放弃修改'))
    expect(leave).toHaveBeenCalledOnce()
  })
  it('cancels navigation from a focus-activated settings tab without reopening the modal', async () => {
    const { leave } = await setup(inventory(), false, true)
    await click(button('有道翻译')); await input('App Key', 'synthetic-unsaved')
    await act(async () => button('权限').focus())
    expect(document.querySelector('[role="dialog"]')).not.toBeNull()
    await act(async () => button('取消').focus()); await click(button('取消'))
    await act(async () => new Promise<void>((resolve) => window.setTimeout(resolve, 100)))
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(document.activeElement).toBe(button('返回密钥配置'))
    expect(document.querySelector<HTMLInputElement>('input[aria-label="App Key"]')!.value).toBe('synthetic-unsaved')
    expect(leave).not.toHaveBeenCalled()
  })
  it('confirms deletion and updates from the returned inventory only', async () => {
    const { client, host } = await setup(); await click(button('删除有道翻译')); expect(client.remove).not.toHaveBeenCalled()
    const pending = deferred<CredentialInventory>(); client.remove.mockReturnValueOnce(pending.promise)
    await click(button('删除配置')); expect(button('有道翻译')).toBeDefined()
    expect(client.remove).toHaveBeenCalledExactlyOnceWith({ service: 'youdaoTranslation', expectedRevision: '1' })
    await act(async () => pending.resolve(inventory([], '2'))); expect(host.textContent).not.toContain('有道翻译')
  })
  it('refreshes a stale revision but requires review before retrying a draft', async () => {
    const { client, host } = await setup(inventory([{ ...entry, configured: true }]))
    await click(button('有道翻译')); await input('名称', '新名称')
    client.save.mockRejectedValueOnce({ code: 'staleInventory' }); client.list.mockResolvedValueOnce(inventory([{ ...entry, configured: true, name: '其他窗口修改' }], '8'))
    await click(button('验证并保存'))
    expect(host.textContent).toContain('配置已变化')
    expect(document.querySelector<HTMLInputElement>('input[aria-label="名称"]')!.value).toBe('新名称')
    await click(button('验证并保存')); expect(client.save.mock.calls[client.save.mock.calls.length - 1]?.[0].expectedRevision).toBe('8')
  })
  it('offers only missing services and opens the item returned after adding', async () => {
    const { client } = await setup(); await click(button('添加配置'))
    const menu = document.querySelector('[role="menu"]')!
    expect(menu.textContent).not.toContain('有道'); expect(menu.textContent).toContain('DeepSeek')
    client.add.mockResolvedValueOnce(inventory([entry, { service: 'deepseek', name: 'DeepSeek', configured: false, enabled: false }], '2'))
    await click([...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((item) => item.textContent === 'DeepSeek')!)
    expect(client.add).toHaveBeenCalledExactlyOnceWith({ service: 'deepseek', expectedRevision: '1' })
    expect(document.querySelector<HTMLInputElement>('input[aria-label="API Key"]')!.type).toBe('password')
    expect(document.querySelector('input[aria-label="App ID"]')).toBeNull()
    expect(document.body.textContent).toContain('不验证余额或生成额度')
  })
  it('does not optimistically toggle an enabled configuration', async () => {
    const { client } = await setup(inventory([{ ...entry, configured: true }]))
    const pending = deferred<CredentialInventory>(); client.setEnabled.mockReturnValueOnce(pending.promise)
    await click(button('启用有道翻译')); expect(button('启用有道翻译').getAttribute('aria-checked')).toBe('false')
    expect(button('启用有道翻译').disabled).toBe(true)
    await act(async () => pending.resolve(inventory([{ ...entry, configured: true, enabled: true }], '2')))
    expect(button('启用有道翻译').getAttribute('aria-checked')).toBe('true')
    expect(client.setEnabled).toHaveBeenCalledExactlyOnceWith({ service: 'youdaoTranslation', enabled: true, expectedRevision: '1' })
  })
  it('Escape cancels discard without losing input, then returns focus to the list', async () => {
    const { ref } = await setup(); button('有道翻译').focus(); await click(button('有道翻译')); await input('名称', '未保存')
    await click(button('返回密钥配置'))
    const cancel = button('取消'); cancel.focus()
    await act(async () => cancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true })))
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(document.querySelector<HTMLInputElement>('input[aria-label="名称"]')!.value).toBe('未保存')
    await input('名称', '有道翻译'); await act(async () => { expect(ref.current!.back()).toBe(true) })
    await act(async () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())))
    expect(document.activeElement).toBe(button('有道翻译'))
  })
  it('locks writes after an uncertain failure until a refresh succeeds', async () => {
    const { client } = await setup(inventory([{ ...entry, configured: true }]))
    await click(button('有道翻译')); await input('名称', '新名称')
    client.save.mockRejectedValueOnce({ code: 'storageFailed' }); client.list.mockRejectedValueOnce(new Error('private response'))
    await click(button('验证并保存')); expect(button('验证并保存').disabled).toBe(true)
    expect(document.body.textContent).not.toContain('private response')
    client.list.mockResolvedValueOnce(inventory([{ ...entry, configured: true }], '9'))
    await click(button('刷新配置')); expect(button('验证并保存').disabled).toBe(false)
    await click(button('验证并保存')); expect(client.save.mock.calls[client.save.mock.calls.length - 1][0].expectedRevision).toBe('9')
  })
})
