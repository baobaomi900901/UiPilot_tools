import type { PluginHandler } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'
const handler: PluginHandler = async (invocation, api) => {
 const response = await api.services?.youdao.translate({text: invocation.input, from: 'en', to: 'zh-CHS'})
 const text = response?.translations.join('\n') ?? ''
 return {requestId: invocation.requestId, results: [{id: 'translation', title: text, defaultAction: {type: 'copyText', text}}]}
}
void handler
