import type {
  PluginHandler,
  PluginPanelUpdate,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: { echo: invocation.input },
})

const consumePanelApi = (api: Readonly<UiPilotPluginPanelApiV1>) => api.onUpdate(
  async (update: Readonly<PluginPanelUpdate>) => {
    await api.storage.set('demo-panel.last-input', update.input)
    await api.storage.get('demo-panel.last-input')
    await api.storage.remove('demo-panel.last-input')
  },
)

const focusTaggedInput = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  await api.focusHostInput()
  // @ts-expect-error focusHostInput does not expose session or request identifiers.
  await api.focusHostInput('1')
}

void handler
void consumePanelApi
void focusTaggedInput
