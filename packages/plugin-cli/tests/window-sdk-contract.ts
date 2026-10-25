import type { JsonValue, PluginWindowActionHandler, PluginRuntimeModule, UiPilotPluginWindowApiV1 } from '../../../docs/plugin-sdk/uipilot-plugin-api-v1.js'
const handler: PluginWindowActionHandler = async (invocation, api) => {
  const platform: 'windows' = invocation.context.platform
  await api.storage.get('draft')
  void platform
  return { requestId: invocation.requestId, data: invocation.payload }
}
const runtime: PluginRuntimeModule = { onCommand: async (i) => ({ requestId: i.requestId, data: null }), onWindowAction: handler }
async function request(api: Readonly<UiPilotPluginWindowApiV1>) {
  if (!api.request) return
  const data: JsonValue = await api.request({ action: 'load', payload: null })
  // @ts-expect-error payload is mandatory
  await api.request({ action: 'load' })
  // @ts-expect-error no caller-selected ownership or plugin
  await api.request({ action: 'load', payload: null, pluginId: 'other' })
  // @ts-expect-error capability is readonly
  api.request = undefined
  void data
}
void runtime; void request
