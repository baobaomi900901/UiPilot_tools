import type { PluginHandler, PluginNetworkCredential, PluginNetworkErrorName, PluginNetworkRequest, UiPilotPluginApiV1 } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const binding: PluginNetworkCredential = { id: 'service-token', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' }
const request: PluginNetworkRequest = { url: 'https://api.example.com/v1/demo', method: 'GET', credential: binding.id }
const missing: PluginNetworkErrorName = 'CredentialUnavailableError'
const handler: PluginHandler = async (invocation, api) => {
  const configured: boolean = await api.settings.isSecretConfigured(binding.settingKey)
  const response = await api.network?.request(request)
  return { requestId: invocation.requestId, results: [{ id: 'demo', title: String(response?.status ?? configured) }] }
}
function rejectedSurfaces(api: UiPilotPluginApiV1) {
  // @ts-expect-error Credentials are references, never token objects.
  const invalid: PluginNetworkRequest = { url: request.url, method: 'GET', credential: { token: 'synthetic' } }
  // @ts-expect-error There is no direct secret getter.
  void api.settings.getSecret('api-token')
  // @ts-expect-error Runtime cannot supply a setting key or host for secret lookup.
  void api.network?.request({ ...request, settingKey: 'api-token' })
  // @ts-expect-error Manifest bindings support only bearer.
  const unsupported: PluginNetworkCredential = { ...binding, type: 'basic' }
  void invalid
  void unsupported
}
void missing
void handler
void rejectedSurfaces
