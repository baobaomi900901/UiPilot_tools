import { describe, expect, it, vi } from 'vitest'
import { createCredentialClient } from './credential-client'

const inventory = { revision: '2', items: [{ service: 'deepseek', name: 'DeepSeek', enabled: false, configured: true }] }
describe('credential IPC client', () => {
  it('uses the managed commands and keeps expected revisions on mutations', async () => {
    const invoke = vi.fn(async () => inventory)
    const client = createCredentialClient(invoke)
    expect(await client.list()).toEqual(inventory)
    expect(invoke).toHaveBeenLastCalledWith('list_credentials', undefined)
    const input = { service: 'deepseek' as const, name: '模型', values: { apiKey: 'synthetic-key' }, expectedRevision: '1' }
    await client.save(input)
    expect(invoke).toHaveBeenLastCalledWith('save_credential', { input })
    await client.add({ service: 'baiduOcr', expectedRevision: '2' })
    expect(invoke).toHaveBeenLastCalledWith('add_credential', { input: { service: 'baiduOcr', expectedRevision: '2' } })
    await client.setEnabled({ service: 'deepseek', enabled: true, expectedRevision: '2' })
    expect(invoke).toHaveBeenLastCalledWith('set_credential_enabled', { input: { service: 'deepseek', enabled: true, expectedRevision: '2' } })
    await client.remove({ service: 'deepseek', expectedRevision: '2' })
    expect(invoke).toHaveBeenLastCalledWith('delete_credential', { input: { service: 'deepseek', expectedRevision: '2' } })
  })
  it.each([
    { ...inventory, revision: '18446744073709551616' },
    { ...inventory, revision: '01' },
    { ...inventory, items: [...inventory.items, ...inventory.items] },
    { ...inventory, items: [{ ...inventory.items[0], service: 'other' }] },
    { ...inventory, items: [{ ...inventory.items[0], values: { apiKey: 'must-not-reach-UI' } }] },
    { ...inventory, items: [{ ...inventory.items[0], enabled: true, configured: false }] },
  ])('rejects malformed or secret-bearing responses', async (response) => {
    await expect(createCredentialClient(async () => response).list()).rejects.toEqual({ code: 'invalidResponse', message: 'credential response is invalid' })
  })
})
