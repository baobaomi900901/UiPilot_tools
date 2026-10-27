import type { PluginHandler, PluginNetworkCredential, PluginNetworkErrorName, PluginNetworkRequest, UiPilotPluginApiV1 } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const binding: PluginNetworkCredential = { id: 'acceptance-token', settingKey: 'test-token', host: 'httpbun.com', type: 'bearer' }
const request: PluginNetworkRequest = { url: 'https://httpbun.com/bearer/uipilot-acceptance-token', method: 'GET', credential: binding.id }
const credentialFailure: PluginNetworkErrorName = 'CredentialUnavailableError'
const handler: PluginHandler = async (invocation, api) => {
  const configured: boolean = await api.settings.isSecretConfigured(binding.settingKey)
  const response = await api.network?.request(request)
  return { requestId: invocation.requestId, results: [{ id: 'auth-check-result', title: String(response?.status ?? configured) }] }
}
function rejectedSurfaces(api: UiPilotPluginApiV1) {
  // @ts-expect-error Credentials are declared IDs, never plaintext token objects.
  const invalid: PluginNetworkRequest = { url: request.url, method: 'GET', credential: { token: 'public-test-only' } }
  // @ts-expect-error Runtime has no plaintext secret getter.
  void api.settings.getSecret('test-token')
  // @ts-expect-error Runtime cannot choose a setting key for secret consumption.
  void api.network?.request({ ...request, settingKey: 'test-token' })
  // @ts-expect-error Manifest credential bindings support only bearer.
  const unsupported: PluginNetworkCredential = { ...binding, type: 'basic' }
  void invalid
  void unsupported
}
void credentialFailure
void handler
void rejectedSurfaces