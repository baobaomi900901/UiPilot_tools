import type {
  PluginHandler,
  PluginPanelHostKeyEvent,
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

const consumeHostKeysAndHide = (api: Readonly<UiPilotPluginPanelApiV1>) => {
  const unsubscribe = api.onHostKey(async (event: Readonly<PluginPanelHostKeyEvent>) => {
    const key: 'ArrowDown' | 'ArrowUp' | 'n' = event.key
    const sequence: string = event.routeSequence
    void key
    void sequence
  })
  void api.requestHide()
  // @ts-expect-error requestHide does not accept plugin-supplied identifiers.
  void api.requestHide('1')
  return unsubscribe
}

void handler
void consumePanelApi
void focusTaggedInput
void consumeHostKeysAndHide
