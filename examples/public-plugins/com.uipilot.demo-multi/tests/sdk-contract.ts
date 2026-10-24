import type { PluginHandler, PluginInvocation, PluginResult, PluginRuntimeModule } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const legacy: PluginInvocation = {
  apiVersion: 1, requestId: 'legacy', input: 'I am  Jack',
  context: { platform: 'windows', theme: 'dark', invokedAt: '2026-10-24T12:00:00+08:00' },
}
const selected: PluginInvocation = { ...legacy, commandId: 'upper' }
const optional: string | undefined = legacy.commandId
// @ts-expect-error v1 also covers legacy invocations without commandId
const required: string = selected.commandId
if (selected.commandId !== undefined) {
  const narrowed: string = selected.commandId
  void narrowed
}
// @ts-expect-error commandId must be a string
const numeric: PluginInvocation = { ...legacy, commandId: 1 }
// @ts-expect-error absence is optional; null is not a command ID
const nullable: PluginInvocation = { ...legacy, commandId: null }
const handler: PluginHandler = async invocation => {
  switch (invocation.commandId) {
    case 'upper':
    case 'lower':
      return { requestId: invocation.requestId, results: [{ id: 'copy', title: invocation.input }] }
    default:
      throw new Error('Unsupported command ID')
  }
}
const runtime: PluginRuntimeModule = { onCommand: handler }
// @ts-expect-error onAction is not a v1 runtime export
const withAction: PluginRuntimeModule = { onCommand: handler, onAction: handler }
// @ts-expect-error result actions arrays are not supported
const actions: PluginResult = { id: 'copy', title: 'text', actions: [] }
// @ts-expect-error per-command hotkeys are not invocation API
const hotkey: PluginInvocation = { ...legacy, hotkey: 'Ctrl+U' }
// @ts-expect-error no multi-command panel invocation API
const panel: PluginInvocation = { ...legacy, panel: { commandId: 'upper' } }
void [runtime, optional, required, numeric, nullable, withAction, actions, hotkey, panel]
