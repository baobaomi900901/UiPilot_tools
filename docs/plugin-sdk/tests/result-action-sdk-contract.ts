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

// @ts-expect-error A result cannot declare both named actions and a legacy default action.
const bothForms: MainResultResponse = { requestId: 'r', results: [{ id: 'x', title: 'x', actions: [], defaultAction: { type: 'copyText', text: 'x' } }] }
// @ts-expect-error A default named action requires the named actions result form.
const missingNamedActions: MainResultResponse = { requestId: 'r', results: [{ id: 'x', title: 'x', defaultActionId: 'copy' }] }
void bothForms
void missingNamedActions
