import type {
  PluginHandler,
  PluginPanelUpdate,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const notesHandler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: {},
})

const consumePanelApi = (api: Readonly<UiPilotPluginPanelApiV1>) =>
  api.onUpdate(async (update: Readonly<PluginPanelUpdate>) => {
    const previous = await api.storage.get('notes.entries')
    await api.storage.set('notes.entries', previous ?? [])
    await api.storage.remove('notes.entries')
    void update.input
    void update.theme
    void update.sessionEpoch
  })

const focusTaggedInput = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  await api.focusHostInput()
  // @ts-expect-error focusHostInput does not expose session or request identifiers.
  await api.focusHostInput('1')
}

void notesHandler
void consumePanelApi
void focusTaggedInput
