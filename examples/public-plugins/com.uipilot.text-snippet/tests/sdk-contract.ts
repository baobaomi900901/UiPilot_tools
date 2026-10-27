import type { MainResultResponse, PluginActionHandler, PluginActionResult, PluginHandler, PluginResultAction, PluginRuntimeModule } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const copy: PluginResultAction = { id: 'copy', title: '复制', type: 'copyText', text: '' }
const save: PluginResultAction = { id: 'save', title: '保存为最新片段', type: 'runtime', payload: { text: '', expectedRevision: 0 } }
const read: PluginResultAction = { id: 'read', title: '重新读取', type: 'runtime', payload: null }
const open: PluginResultAction = { id: 'open', title: '在浏览器打开', type: 'openUrl', url: 'https://example.com' }
const paste: PluginResultAction = { id: 'paste', title: '粘贴到原应用', type: 'pasteText', text: '汉字' }
const row: PluginActionResult = { id: 'text', title: '原文', actions: [copy, save, read, open, paste], defaultActionId: 'copy' }
const onCommand: PluginHandler = async invocation => ({ requestId: invocation.requestId, results: [row] })
const onAction: PluginActionHandler = async (invocation, api) => {
  const commandId: string = invocation.commandId
  const resultId: string = invocation.resultId
  const actionId: string = invocation.actionId
  const platform: 'windows' = invocation.context.platform
  await api.storage.get('snippet.latest')
  const reply: MainResultResponse = { requestId: invocation.requestId, results: [row] }
  void [commandId, resultId, actionId, platform]
  return reply
}
const runtime: PluginRuntimeModule = { onCommand, onAction }
// @ts-expect-error Runtime actions require a payload even when no data is needed.
const missingPayload: PluginResultAction = { id: 'read', title: '重新读取', type: 'runtime' }
// @ts-expect-error New rows cannot mix actions with the legacy defaultAction field.
const mixed: PluginActionResult = { ...row, defaultAction: { type: 'copyText', text: '' } }
void [runtime, missingPayload, mixed]
