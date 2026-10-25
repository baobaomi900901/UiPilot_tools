import type {
  JsonValue,
  PluginHandler,
  PluginRuntimeModule,
  PluginWindowActionHandler,
  PluginWindowRequest,
  UiPilotPluginWindowApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const onCommand: PluginHandler = async (invocation, api) => ({
  requestId: invocation.requestId,
  data: await api.storage.get('workbench.draft'),
})
const onWindowAction: PluginWindowActionHandler = async (invocation, api) => {
  const platform: 'windows' = invocation.context.platform
  const payload: JsonValue = invocation.payload
  await api.storage.get('workbench.draft')
  if (invocation.action === 'save') await api.storage.set('workbench.draft', { schemaVersion: 1, revision: 1, text: '' })
  void platform
  return { requestId: invocation.requestId, data: payload }
}
const runtime: PluginRuntimeModule = { onCommand, onWindowAction }

function content(api: Readonly<UiPilotPluginWindowApiV1>) {
  let request: typeof api.request
  api.onUpdate(update => {
    request = api.request
    const theme: 'dark' | 'light' = update.theme
    void theme
    // Return promptly; invoke the captured request only from a later user action.
  })
  return async () => {
    if (!request) return
    const load: PluginWindowRequest = { action: 'load', payload: null }
    const data: JsonValue = await request(load)
    await request({ action: 'format', payload: { text: '{}' } })
    await request({ action: 'save', payload: { text: '{draft', expectedRevision: 0 } })
    // @ts-expect-error payload is required even for load.
    await request({ action: 'load' })
    // @ts-expect-error callers cannot select another plugin or session.
    await request({ action: 'load', payload: null, pluginId: 'other' })
    // @ts-expect-error request is a readonly optional capability.
    api.request = request
    void data
  }
}
void runtime
void content
