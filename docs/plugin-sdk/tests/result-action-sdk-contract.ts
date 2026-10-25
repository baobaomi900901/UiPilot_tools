import type { PluginRuntimeModule, PluginActionHandler, MainResultResponse } from '../uipilot-plugin-api-v1.js'
const onAction: PluginActionHandler = async (invocation, api): Promise<MainResultResponse> => {
  const command: string = invocation.commandId
  const result: string = invocation.resultId
  await api.storage.set('saved', invocation.payload)
  return { requestId: invocation.requestId, results: [{ id: result, title: command, actions: [
    { id: 'copy', title: 'Copy', type: 'copyText', text: invocation.input },
    { id: 'read', title: 'Read', type: 'runtime', payload: null },
  ], defaultActionId: 'copy' }] }
}
const module: PluginRuntimeModule = { onCommand: async invocation => ({ requestId: invocation.requestId, results: [] }), onAction }
void module
