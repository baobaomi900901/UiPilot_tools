export type CredentialService = 'youdaoTranslation' | 'baiduTranslation' | 'baiduOcr' | 'deepseek'
export interface CredentialEntry {
  service: CredentialService
  name: string
  enabled: boolean
  configured: boolean
}
export interface CredentialInventory { revision: string; items: CredentialEntry[] }
export interface CredentialClient {
  list(): Promise<CredentialInventory>
  add(input: { service: CredentialService; expectedRevision: string }): Promise<CredentialInventory>
  save(input: { service: CredentialService; name: string; values: Record<string, string>; expectedRevision: string }): Promise<CredentialInventory>
  setEnabled(input: { service: CredentialService; enabled: boolean; expectedRevision: string }): Promise<CredentialInventory>
  remove(input: { service: CredentialService; expectedRevision: string }): Promise<CredentialInventory>
}
