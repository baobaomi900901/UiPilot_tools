import type { PluginHandler, PluginInvocation } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const mainResultHandler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  results: [
    {
      id: 'demo-return-copy',
      title: invocation.input,
      defaultAction: { type: 'copyText', text: invocation.input },
    },
  ],
})

declare const invocation: Readonly<PluginInvocation>
void mainResultHandler(invocation, {} as never)
