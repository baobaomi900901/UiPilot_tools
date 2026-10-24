import type {
  JsonValue,
  PluginHandler,
  PluginPanelActionHandler,
  PluginRuntimeModule,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const onCommand: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: { ok: true, templates: [], filter: invocation.input },
})
const onPanelAction: PluginPanelActionHandler = async (invocation, api) => {
  const action: string = invocation.action
  const platform: 'windows' = invocation.context.platform
  const payload: JsonValue = invocation.payload
  await api.storage.get('templates.catalog-v1')
  void action
  void platform
  return { requestId: invocation.requestId, data: payload }
}
const runtime: PluginRuntimeModule = { onCommand, onPanelAction }

async function preview(panel: Readonly<UiPilotPluginPanelApiV1>) {
  if (!panel.request) return
  const data: JsonValue = await panel.request({ action: 'preview', payload: { body: '{{constructor}}', values: [{ name: 'constructor', value: '' }] } })
  // @ts-expect-error payload is required, including an explicit null for empty payloads.
  await panel.request({ action: 'preview' })
  // @ts-expect-error panel callers cannot select another plugin or session.
  await panel.request({ action: 'load', payload: { id: 'template-1' }, pluginId: 'other' })
  void data
}
void runtime
void preview
