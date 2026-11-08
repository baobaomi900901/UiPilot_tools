import { describe, expect, it, vi } from 'vitest'
import { createMacLoginItemClient, parseLoginItemMutation, parseLoginItemSnapshot } from './macos-login-item'

const deferred = () => {
  let resolve!: (value: unknown) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<unknown>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

describe('Mac login item protocol and cross-view mutation ownership', () => {
  it('accepts exact system states and rejects fabricated or partial confirmations', () => {
    for (const status of ['notRegistered', 'enabled', 'requiresApproval', 'notFound', 'unavailable']) {
      expect(parseLoginItemSnapshot({ status })).toEqual({ status })
      expect(parseLoginItemMutation({ status, outcome: 'unconfirmed' })).toEqual({ status, outcome: 'unconfirmed' })
    }
    for (const value of [null, [], {}, { status: true }, { status: 'allowed' }, { status: 'enabled', path: '/private' }]) {
      expect(parseLoginItemSnapshot(value)).toBeNull()
    }
    for (const value of [{ status: 'enabled' }, { status: 'enabled', outcome: 'success' }, { status: 'enabled', outcome: 'applied', error: 'private' }]) {
      expect(parseLoginItemMutation(value)).toBeNull()
    }
  })

  it('accepts only fixed issue classifications on unconfirmed mutations', () => {
    expect(parseLoginItemMutation({ status: 'notFound', outcome: 'unconfirmed', issue: 'invalidSignature' })).toEqual({ status: 'notFound', outcome: 'unconfirmed', issue: 'invalidSignature' })
    for (const issue of ['native error text', null, 3]) expect(parseLoginItemMutation({ status: 'notFound', outcome: 'unconfirmed', issue })).toBeNull()
    expect(parseLoginItemMutation({ status: 'enabled', outcome: 'applied', issue: 'invalidSignature' })).toBeNull()
  })

  it('waits for a previous view mutation before reading and rejects duplicate mutation without IPC', async () => {
    const update = deferred()
    const invoke = vi.fn(async (command: string) => command === 'set_macos_login_item' ? update.promise : { status: 'enabled' })
    const client = createMacLoginItemClient(invoke)
    expect(invoke).not.toHaveBeenCalled()
    const setting = client.set(true)
    const reading = client.read()
    await expect(client.set(false)).rejects.toMatchObject({ code: 'loginItemBusy' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))
    expect(invoke).toHaveBeenCalledWith('set_macos_login_item', { input: { enabled: true } })
    update.resolve({ status: 'enabled', outcome: 'applied' })
    await expect(setting).resolves.toEqual({ status: 'enabled', outcome: 'applied' })
    await expect(reading).resolves.toEqual({ status: 'enabled' })
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['set_macos_login_item', 'get_macos_login_item'])
  })

  it('still reads system truth after a rejected old mutation without exposing native errors', async () => {
    const update = deferred()
    const invoke = vi.fn(async (command: string) => command === 'set_macos_login_item' ? update.promise : { status: 'requiresApproval' })
    const client = createMacLoginItemClient(invoke)
    const failed = expect(client.set(true)).rejects.toMatchObject({ code: 'loginItemUnavailable' })
    const reading = client.read()
    update.reject(new Error('private native NSError'))
    await failed
    expect(await reading).toEqual({ status: 'requiresApproval' })
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  it('releases pending ownership after synchronous throw and never registers on refresh or opening settings', async () => {
    const invoke = vi.fn<(command: string, input?: Record<string, unknown>) => Promise<unknown>>()
      .mockImplementationOnce(() => { throw new Error('private native NSError') })
      .mockResolvedValueOnce({ status: 'notRegistered' })
      .mockResolvedValueOnce(undefined)
    const client = createMacLoginItemClient(invoke)
    await expect(client.set(true)).rejects.toMatchObject({ code: 'loginItemUnavailable' })
    expect(await client.read()).toEqual({ status: 'notRegistered' })
    await client.openSettings()
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['set_macos_login_item', 'get_macos_login_item', 'open_macos_login_item_settings'])
  })
})
