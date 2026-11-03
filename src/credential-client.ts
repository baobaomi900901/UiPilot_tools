import type { CredentialClient, CredentialInventory, CredentialService } from './credential-protocol'

const services = new Set<CredentialService>(['youdaoTranslation', 'baiduTranslation', 'baiduOcr', 'deepseek'])
function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
function exactKeys(value: Record<string, unknown>, keys: string[]) {
  return Object.keys(value).length === keys.length && keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
}
export function parseCredentialInventory(value: unknown): CredentialInventory | null {
  if (!record(value) || !exactKeys(value, ['revision', 'items']) || typeof value.revision !== 'string' ||
    !/^(0|[1-9][0-9]{0,19})$/.test(value.revision) || BigInt(value.revision) > 18446744073709551615n ||
    !Array.isArray(value.items) || value.items.length > 4) return null
  const seen = new Set<string>()
  const items: CredentialInventory['items'] = []
  for (const item of value.items) {
    if (!record(item) || !exactKeys(item, ['service', 'name', 'enabled', 'configured']) ||
      typeof item.service !== 'string' || !services.has(item.service as CredentialService) || seen.has(item.service) ||
      typeof item.name !== 'string' || !item.name.trim() || [...item.name].length > 80 ||
      typeof item.enabled !== 'boolean' || typeof item.configured !== 'boolean' || (item.enabled && !item.configured)) return null
    seen.add(item.service)
    items.push({ service: item.service as CredentialService, name: item.name, enabled: item.enabled, configured: item.configured })
  }
  return { revision: value.revision, items }
}

export function createCredentialClient(invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>): CredentialClient {
  const request = async (command: string, input?: object) => {
    const parsed = parseCredentialInventory(await invoke(command, input ? { input } : undefined))
    if (!parsed) throw { code: 'invalidResponse', message: 'credential response is invalid' }
    return parsed
  }
  return {
    list: () => request('list_credentials'),
    add: (input) => request('add_credential', input),
    save: (input) => request('save_credential', input),
    setEnabled: (input) => request('set_credential_enabled', input),
    remove: (input) => request('delete_credential', input),
  }
}
