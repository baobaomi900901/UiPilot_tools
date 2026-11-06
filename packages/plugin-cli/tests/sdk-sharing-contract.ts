import type {
  PluginInvocation, PluginPanelUpdate, PluginShareEntry, PluginSharePayload,
  PluginShareReceiveEvent, PluginShareSourceSnapshot, UiPilotPluginPanelApiV1,
} from '../../../docs/plugin-sdk/uipilot-plugin-api-v1.js'

declare const panel: Readonly<UiPilotPluginPanelApiV1>
declare const invocation: Readonly<PluginInvocation>
declare const update: Readonly<PluginPanelUpdate>
const payload: PluginSharePayload = { type: 'text/plain', text: ' edited text\n' }
const snapshot: PluginShareSourceSnapshot = { revision: '1', payload }
const entry: PluginShareEntry | undefined = invocation.context.entry ?? update.entry
if (entry) {
  const kind: 'sharePrepare' = entry.kind
  const receiverId: string = entry.receiverId
  void [kind, receiverId]
}
if (panel.share) {
  const unsubscribe: () => void = panel.share.onSource(() => snapshot)
  panel.share.onSource(() => null)
  const invalidated: Promise<void> = panel.share.invalidateSource('2')
  panel.share.onReceive(event => {
    const version: 1 = event.version
    const session: string = event.sessionEpoch
    const text: string = event.payload.text
    void [version, session, text]
    return { accepted: event.receiverId === 'translate-text' }
  })
  // @ts-expect-error source providers must be synchronous
  panel.share.onSource(async () => snapshot)
  // @ts-expect-error receive acceptance must be synchronous
  panel.share.onReceive(async () => ({ accepted: true }))
  // @ts-expect-error receiver must explicitly accept or reject
  panel.share.onReceive(() => undefined)
  // @ts-expect-error revision uses U64Decimal, never a lossy number
  panel.share.invalidateSource(2)
  void [unsubscribe, invalidated]
}
declare const event: PluginShareReceiveEvent
// @ts-expect-error payload type is fixed to text/plain
const wrongPayload: PluginSharePayload = { type: 'text/html', text: '<b>text</b>' }
// @ts-expect-error payload fields are readonly
payload.text = 'changed'
// @ts-expect-error nested receive payload is readonly
event.payload.text = 'changed'
// @ts-expect-error source snapshot revision is readonly
snapshot.revision = '2'
// @ts-expect-error entry is readonly
entry!.receiverId = 'changed'
// @ts-expect-error sharing does not expose target enumeration
panel.share?.receivers()
// @ts-expect-error sharing does not expose direct sending
panel.share?.send('target', payload)
void wrongPayload

// Ordinary entry fields remain absent on old Host/plugin calls.
const legacyInvocation: PluginInvocation = {
  apiVersion: 1, requestId: 'legacy', input: '',
  context: { platform: 'windows', theme: 'dark', invokedAt: 'now' },
}
const legacyUpdate: PluginPanelUpdate = {
  requestId: 'legacy', input: '', platform: 'windows', theme: 'dark',
  invokedAt: 'now', sessionEpoch: '1', data: null,
}
void [legacyInvocation, legacyUpdate]
