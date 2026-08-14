import type {
  PluginHandler,
  PluginInvocation,
  PluginWindowUpdate,
  UiPilotPluginWindowApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const windowHandler: PluginHandler = async (invocation, api) => {
  const previous = await api.storage.get('lastInput')
  await api.storage.set('lastInput', invocation.input)
  return {
    requestId: invocation.requestId,
    data: { previous, current: invocation.input },
  }
}

const mainResultHandler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  results: [
    {
      id: 'copy',
      title: invocation.input,
      defaultAction: { type: 'copyText', text: invocation.input },
    },
  ],
})

const consumeWindowApi = (api: Readonly<UiPilotPluginWindowApiV1>) =>
  api.onUpdate((update: Readonly<PluginWindowUpdate>) => {
    const instance: 1 = update.instanceNumber
    void instance
  })

declare const invocation: Readonly<PluginInvocation>
void windowHandler(invocation, {} as never)
void mainResultHandler(invocation, {} as never)
void consumeWindowApi
