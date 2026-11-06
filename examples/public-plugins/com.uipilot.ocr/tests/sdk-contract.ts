import type { PluginHandler, PluginPanelActionHandler, PluginPanelActionResponse, UiPilotPluginPanelApiV1, PluginPanelImageChangedEvent } from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'
const onCommand: PluginHandler = async invocation => ({ requestId: invocation.requestId, data: {} })
const onPanelAction: PluginPanelActionHandler = async (invocation, api): Promise<PluginPanelActionResponse> => {
 if (!invocation.image || !api.services) return { requestId: invocation.requestId, data: { ok: false } }
 const result = await api.services.baidu.ocr({ imageId: invocation.image.id })
 const lines: readonly string[] = result.lines
 // @ts-expect-error Request image authority is immutable.
 invocation.image.id = 'forged'
 return { requestId: invocation.requestId, data: { ok: true, text: result.text, lines: [...lines] } }
}
declare const panel: Readonly<UiPilotPluginPanelApiV1>
const unsubscribe = panel.onImageChanged?.((event: Readonly<PluginPanelImageChangedEvent>) => {
 if (event.image) {
  const preview: string = event.image.previewDataUrl
  const revision: string = event.revision
  void preview; void revision
  // @ts-expect-error Host image snapshots are immutable.
  event.image.id = 'forged'
  void panel.request?.({ action: 'recognize', imageId: event.image.id, payload: { providerId: 'baidu' } })
 }
})
panel.onReady?.(event => { const epoch: string = event.sessionEpoch; void epoch })
panel.onHostKey(event => { if (event.key === 'Enter') void panel.clipboard?.writeText({ text: 'current edited text' }) })
void panel.clearImage?.(); void unsubscribe; void onCommand; void onPanelAction
