import type {
  PluginHandler,
  PluginInvocation,
  PluginWindowUpdate,
  UiPilotPluginWindowApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const noteHandler: PluginHandler = async (invocation, api) => {
  const previous = await api.storage.get('notes.entries')
  await api.storage.set('notes.entries', previous ?? [])
  return {
    requestId: invocation.requestId,
    data: {},
  }
}

const consumeWindowApi = (api: Readonly<UiPilotPluginWindowApiV1>) =>
  api.onUpdate((update: Readonly<PluginWindowUpdate>) => {
    const instance: 1 = update.instanceNumber
    void instance
    void api.storage.get('notes.entries')
    void api.close()
  })

declare const invocation: Readonly<PluginInvocation>
void noteHandler(invocation, {} as never)
void consumeWindowApi
