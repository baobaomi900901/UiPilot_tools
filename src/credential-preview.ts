import type { CredentialClient, CredentialInventory, CredentialService } from './credential-protocol'

// Browser preview only. No real credentials, filesystem access, or provider requests.
export function createPreviewCredentialClient(): CredentialClient {
  const names: Record<CredentialService, string> = {
    youdaoTranslation: '有道翻译', baiduTranslation: '百度翻译', baiduOcr: '百度 OCR', deepseek: 'DeepSeek',
  }
  let state: CredentialInventory = { revision: '1', items: [
    { service: 'youdaoTranslation', name: names.youdaoTranslation, enabled: true, configured: true },
    { service: 'baiduTranslation', name: names.baiduTranslation, enabled: false, configured: false },
    { service: 'baiduOcr', name: names.baiduOcr, enabled: false, configured: false },
    { service: 'deepseek', name: names.deepseek, enabled: false, configured: false },
  ] }
  const snapshot = () => structuredClone(state)
  const check = (revision: string) => { if (state.revision !== revision) throw { code: 'staleInventory' } }
  const commit = () => { state.revision = (BigInt(state.revision) + 1n).toString(); return snapshot() }
  return {
    list: async () => snapshot(),
    add: async ({ service, expectedRevision }) => {
      check(expectedRevision)
      if (state.items.some((item) => item.service === service)) throw { code: 'invalidInput' }
      state.items.push({ service, name: names[service], enabled: false, configured: false })
      return commit()
    },
    save: async ({ service, name, values, expectedRevision }) => {
      check(expectedRevision)
      const item = state.items.find((entry) => entry.service === service)
      if (!item) throw { code: 'credentialUnavailable' }
      // Only explicitly synthetic text is accepted in the preview's fake validator.
      if (Object.values(values).some((value) => value && !value.startsWith('synthetic-'))) throw { code: 'invalidCredentials' }
      if (!item.configured && !Object.values(values).some(Boolean)) throw { code: 'invalidInput' }
      item.name = name; item.configured = true
      return commit()
    },
    setEnabled: async ({ service, enabled, expectedRevision }) => {
      check(expectedRevision)
      const item = state.items.find((entry) => entry.service === service)
      if (!item) throw { code: 'credentialUnavailable' }
      if (enabled && !item.configured) throw { code: 'notConfigured' }
      item.enabled = enabled
      return commit()
    },
    remove: async ({ service, expectedRevision }) => {
      check(expectedRevision)
      state.items = state.items.filter((entry) => entry.service !== service)
      return commit()
    },
  }
}
