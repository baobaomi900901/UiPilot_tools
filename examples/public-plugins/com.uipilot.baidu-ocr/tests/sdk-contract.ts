import type { PluginHandler, PluginInvocation } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation, api) => {
  if (!invocation.image || !api.services) return { requestId: invocation.requestId, results: [] }
  const imageId: string = invocation.image.id
  const result = await api.services.baidu.ocr({ imageId })
  const lines: readonly string[] = result.lines
  void lines
  return {
    requestId: invocation.requestId,
    results: [{ id: 'ocr', title: 'OCR', detail: result.text, defaultAction: { type: 'copyText', text: result.text } }],
  }
}

declare const invocation: Readonly<PluginInvocation>
if (invocation.image) {
  // @ts-expect-error Image authority cannot be reassigned by a plugin.
  invocation.image.id = 'another-image'
}
void handler
