import type {
  PluginHandler,
  PluginPanelHostKeyEvent,
  PluginPanelUpdate,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: { echo: invocation.input },
})

const consumePanelApi = (api: Readonly<UiPilotPluginPanelApiV1>) => api.onUpdate(
  async (update: Readonly<PluginPanelUpdate>) => {
    await api.storage.set('demo-panel.last-input', update.input)
    await api.storage.get('demo-panel.last-input')
    await api.storage.remove('demo-panel.last-input')
  },
)

const focusTaggedInput = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  await api.focusHostInput()
  // @ts-expect-error focusHostInput does not expose session or request identifiers.
  await api.focusHostInput('1')
}

const consumeHostKeysAndHide = (api: Readonly<UiPilotPluginPanelApiV1>) => {
  const unsubscribe = api.onHostKey(async (event: Readonly<PluginPanelHostKeyEvent>) => {
    const key: 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter' = event.key
    const sequence: string = event.routeSequence
    void key
    void sequence
    void event.shiftKey
  })
  void api.requestHide()
  // @ts-expect-error requestHide does not accept plugin-supplied identifiers.
  void api.requestHide('1')
  return unsubscribe
}

const consumeClipboardHistory = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  const snapshot = await api.clipboardHistory.list()
  const revision: string = snapshot.revision
  for (const entry of snapshot.entries) {
    const id: string = entry.id
    const capturedAt: string = entry.capturedAt
    if (entry.kind === 'text') {
      const preview: string = entry.textPreview
      void preview
    } else if (entry.kind === 'image') {
      const previewDataUrl: string = entry.previewDataUrl
      const width: number = entry.width
      const height: number = entry.height
      void previewDataUrl
      void width
      void height
    } else {
      const firstFileName: string = entry.firstFileName
      const fileCount: number = entry.fileCount
      const available: boolean = entry.available
      void firstFileName
      void fileCount
      void available
    }
    void id
    void capturedAt
  }

  const unsubscribe = api.clipboardHistory.onChanged((next) => {
    const nextRevision: string = next.revision
    void nextRevision
  })
  await api.clipboardHistory.remove({ id: 'entry-1' })
  await api.clipboardHistory.clear()
  const paste = await api.clipboardHistory.paste({ id: 'entry-1', routeSequence: '2' })
  const outcome: 'admitted' = paste.outcome

  // @ts-expect-error clipboardHistory.list does not accept plugin-supplied identifiers.
  await api.clipboardHistory.list({ pluginId: 'com.example' })
  // @ts-expect-error clipboardHistory.paste requires a routeSequence.
  await api.clipboardHistory.paste({ id: 'entry-1' })
  void revision
  void outcome
  return unsubscribe
}

void handler
void consumePanelApi
void focusTaggedInput
void consumeHostKeysAndHide
void consumeClipboardHistory
