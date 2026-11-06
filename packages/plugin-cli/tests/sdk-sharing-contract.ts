import type { PluginSharePayload, PluginShareReceiveEvent, UiPilotPluginPanelApiV1 } from '../../../docs/plugin-sdk/uipilot-plugin-api-v1'
declare const panel: UiPilotPluginPanelApiV1
const payload: PluginSharePayload = { type: 'text/plain', text: ' edited\ntext ' }
if (panel.share) {
  const off: () => void = panel.share.onSource(() => payload)
  panel.share.onSource(() => null)
  panel.share.onReceive((event: PluginShareReceiveEvent) => ({ accepted: event.payload.text.length > 0 }))
  off()
  // @ts-expect-error synchronous receive only
  panel.share.onReceive(async () => ({ accepted: true }))
  // @ts-expect-error no source revision protocol
  panel.share.invalidateSource('2')
  // @ts-expect-error no direct plugin send
  panel.share.send(payload)
}
