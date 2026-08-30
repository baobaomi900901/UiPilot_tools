import type {
  ClipboardHistoryPasteErrorName,
  ClipboardHistorySnapshot,
  PluginHandler,
  PluginPanelHostKeyEvent,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: {},
})

const consumeClipboardHistory = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  const snapshot: Readonly<ClipboardHistorySnapshot> = await api.clipboardHistory.list()
  const revision: string = snapshot.revision

  for (const entry of snapshot.entries) {
    const id: string = entry.id
    const capturedAt: string = entry.capturedAt
    if (entry.kind === 'text') {
      const preview: string = entry.textPreview
      void preview
    } else if (entry.kind === 'image') {
      const previewDataUrl: string = entry.previewDataUrl
      const dimensions: readonly [number, number] = [entry.width, entry.height]
      void previewDataUrl
      void dimensions
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

  const unsubscribeHistory = api.clipboardHistory.onChanged((next) => {
    const nextRevision: string = next.revision
    void nextRevision
  })
  const unsubscribeKeys = api.onHostKey(async (event: Readonly<PluginPanelHostKeyEvent>) => {
    const key: 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter' = event.key
    if (key === 'Enter') {
      const result = await api.clipboardHistory.paste({
        id: snapshot.entries[0]?.id ?? 'missing',
        routeSequence: event.routeSequence,
      })
      const outcome: 'admitted' = result.outcome
      void outcome
    }
  })

  await api.clipboardHistory.remove({ id: 'entry-1' })
  await api.clipboardHistory.clear()

  // @ts-expect-error paste requires the routed Enter sequence.
  await api.clipboardHistory.paste({ id: 'entry-1' })
  // @ts-expect-error list accepts no plugin-supplied ownership identifiers.
  await api.clipboardHistory.list({ pluginId: 'com.example' })

  const errorName: ClipboardHistoryPasteErrorName = 'RecordUnavailable'
  void errorName
  void revision
  unsubscribeHistory()
  unsubscribeKeys()
}

void handler
void consumeClipboardHistory
