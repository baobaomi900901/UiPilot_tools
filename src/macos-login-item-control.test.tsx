// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { beforeAll, expect, it, vi } from 'vitest'
import { createMacLoginItemClient, type MacLoginItemClient } from './macos-login-item'
import { MacLoginItemControl } from './macos-login-item-control'

beforeAll(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  Object.defineProperty(window, 'matchMedia', { configurable: true, value: () => ({ matches: false, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {} }) })
})
async function mount(client: MacLoginItemClient) {
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host)
  await act(async () => root.render(<MacLoginItemControl client={client} />))
  return { host, async close() { await act(async () => root.unmount()); host.remove() } }
}
const clientFixture = (): MacLoginItemClient => ({ read: vi.fn(async () => ({ status: 'requiresApproval' as const })), set: vi.fn(async () => ({ status: 'notRegistered' as const, outcome: 'applied' as const })), openSettings: vi.fn(async () => {}) })
const button = (host: HTMLElement, name: string) => [...host.querySelectorAll('button')].find(row => row.textContent?.replace(/\s/g, '').includes(name))!

it('shows pending system approval unchecked and can explicitly stop registration or open settings', async () => {
  const client = clientFixture(); const ui = await mount(client)
  try {
    expect(client.set).not.toHaveBeenCalled()
    expect(ui.host.textContent).toContain('等待系统允许')
    expect(ui.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(false)
    await act(async () => button(ui.host, '打开系统登录项').click())
    expect(client.openSettings).toHaveBeenCalledOnce()
    await act(async () => button(ui.host, '停止注册').click())
    expect(client.set).toHaveBeenCalledExactlyOnceWith(false)
    expect(ui.host.textContent).toContain('未启用')
  } finally { await ui.close() }
})

it('refreshes an external revocation without registering and displays actual state after an uncertain mutation', async () => {
  const client = clientFixture()
  vi.mocked(client.read).mockResolvedValueOnce({ status: 'enabled' })
  vi.mocked(client.set).mockResolvedValueOnce({ status: 'enabled', outcome: 'unconfirmed' })
  const ui = await mount(client)
  try {
    expect(ui.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(true)
    await act(async () => ui.host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click())
    expect(client.set).toHaveBeenCalledExactlyOnceWith(false)
    expect(ui.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(true)
    expect(ui.host.textContent).toContain('变更未能确认')
    await act(async () => button(ui.host, '刷新').click())
    expect(ui.host.textContent).toContain('等待系统允许')
    expect(client.set).toHaveBeenCalledTimes(1)
  } finally { await ui.close() }
})

it('ignores an unmounted view completion and disables all mutations while reading', async () => {
  let finish!: (value: { status: 'enabled' }) => void
  const client = clientFixture()
  vi.mocked(client.read).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  const old = await mount(client)
  expect(old.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.disabled).toBe(true)
  await old.close()
  const current = await mount(client)
  try {
    await act(async () => finish({ status: 'enabled' }))
    expect(current.host.textContent).toContain('等待系统允许')
    expect(current.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(false)
    expect(client.set).not.toHaveBeenCalled()
  } finally { await current.close() }
})


it('waits for the old control mutation before the replacement control reads through the shared client', async () => {
  let finish!: (value: unknown) => void
  let reads = 0
  const invoke = vi.fn(async (command: string) => {
    if (command === 'set_macos_login_item') return new Promise(resolve => { finish = resolve })
    return { status: ++reads === 1 ? 'notRegistered' : 'requiresApproval' }
  })
  const client = createMacLoginItemClient(invoke)
  const old = await mount(client)
  await act(async () => old.host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click())
  await old.close()
  const current = await mount(client)
  try {
    expect(reads).toBe(1)
    expect(current.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.disabled).toBe(true)
    await act(async () => finish({ status: 'enabled', outcome: 'applied' }))
    expect(reads).toBe(2)
    expect(current.host.textContent).toContain('等待系统允许')
    expect(current.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(false)
    expect(invoke.mock.calls.filter(([command]) => command === 'set_macos_login_item')).toHaveLength(1)
  } finally { await current.close() }
})

it('keeps notFound visible and only attempts one registration after an explicit click', async () => {
  const client = clientFixture()
  vi.mocked(client.read).mockResolvedValue({ status: 'notFound' })
  vi.mocked(client.set).mockResolvedValue({ status: 'notFound', outcome: 'unconfirmed', issue: 'invalidSignature' })
  const ui = await mount(client)
  try {
    expect(client.set).not.toHaveBeenCalled()
    await act(async () => button(ui.host, '刷新').click())
    expect(client.set).not.toHaveBeenCalled()
    await act(async () => button(ui.host, '尝试启用').click())
    expect(client.set).toHaveBeenCalledExactlyOnceWith(true)
    expect(ui.host.textContent).toContain('签名不满足系统登录项要求')
    expect(ui.host.querySelector<HTMLInputElement>('input[type="checkbox"]')?.checked).toBe(false)
  } finally { await ui.close() }
})
