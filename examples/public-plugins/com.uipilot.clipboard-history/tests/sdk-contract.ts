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
  const snapshot: Readonly<ClipboardHistorySnapshot> = await api.clipboardHistory.list({ query: '关键词' })
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
    const key: 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter' | 'Delete' = event.key
    void key

  })

  const unsubscribeActions = api.onActions(() => ({
    context: [{ id: 'delete-entry', title: '删除当前记录', shortcut: 'Delete', icon: 'trash', run: () => api.clipboardHistory.remove({ id: 'entry-1' }) }],
    global: [
      { id: 'next-category', title: '下一分类', shortcut: 'Tab', icon: 'arrow-right-to-line', run() {} },
      { id: 'previous-category', title: '上一分类', shortcut: 'Shift+Tab', icon: 'arrow-left-to-line', run() {} },
    ],
  }))
  await api.setStatus({ message: '已删除', tone: 'success' })
  unsubscribeActions()

  const copied: 'copied' = (await api.clipboardHistory.copy({ id: 'entry-1' })).outcome
  void copied
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
