import type { PanelImageSnapshot, PluginPanelImageChangedEvent, PluginPanelRequest, PluginPanelActionInvocation, UiPilotPluginPanelApiV1 } from '../../../docs/plugin-sdk/uipilot-plugin-api-v1.js'
declare const panel: UiPilotPluginPanelApiV1
declare const snapshot: PanelImageSnapshot
declare const event: PluginPanelImageChangedEvent
declare const invocation: PluginPanelActionInvocation
const legacy: PluginPanelRequest = { action: 'save', payload: null }
const image: PluginPanelRequest = { action: 'recognize', imageId: snapshot.id, payload: null }
panel.onImageChanged?.(change => { const id: string | undefined = change.image?.id; void id })
void panel.clearImage?.()
// @ts-expect-error image snapshots are readonly
snapshot.previewDataUrl = ''
// @ts-expect-error event snapshots are deeply readonly
event.image!.width = 0
// @ts-expect-error revision is readonly
event.revision = '1'
// @ts-expect-error action images are readonly
invocation.image!.id = ''
void [legacy, image]
