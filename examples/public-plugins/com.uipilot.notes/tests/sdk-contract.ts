import type {
  PluginHandler,
  PluginPanelHostKeyEvent,
  PluginPanelUpdate,
  UiPilotPluginPanelApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const notesHandler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: {},
})

const consumePanelApi = (api: Readonly<UiPilotPluginPanelApiV1>) =>
  api.onUpdate(async (update: Readonly<PluginPanelUpdate>) => {
    const previous = await api.storage.get('notes.entries')
    await api.storage.set('notes.entries', previous ?? [])
    await api.storage.remove('notes.entries')
    void update.input
    void update.theme
    void update.sessionEpoch
  })

const focusTaggedInput = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  await api.focusHostInput()
  // @ts-expect-error focusHostInput does not expose session or request identifiers.
  await api.focusHostInput('1')
}

const consumeHostKeysAndHide = (api: Readonly<UiPilotPluginPanelApiV1>) => {
  const unsubscribe = api.onHostKey(async (event: Readonly<PluginPanelHostKeyEvent>) => {
    const key: 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter' | 'Delete' = event.key
    const sequence: string = event.routeSequence
    void key
    void sequence
    void event.ctrlKey
    void event.metaKey
    void event.sessionEpoch
  })
  void api.requestHide()
  // @ts-expect-error requestHide does not accept plugin-supplied identifiers.
  void api.requestHide('1')
  return unsubscribe
}

void notesHandler
const reportPanelStatus = async (api: Readonly<UiPilotPluginPanelApiV1>) => {
  await api.setStatus({ message: '已保存', tone: 'success' })
  await api.setStatus({ message: '' })
  // @ts-expect-error Status tones are a closed set.
  await api.setStatus({ message: '保存失败', tone: 'danger' })
  // @ts-expect-error The host owns the target panel identity.
  await api.setStatus({ message: '已保存', sessionEpoch: '1' })
}
void reportPanelStatus
void consumePanelApi
void focusTaggedInput
void consumeHostKeysAndHide

const provideOperations = (api: Readonly<UiPilotPluginPanelApiV1>) => api.onActions(({ source }) => ({
  context: source === 'content' ? [{ id: 'save', title: '保存', shortcut: 'Ctrl+S', icon: 'save', run: async () => {} }] : [],
  global: [{ id: 'new', title: '新建笔记', shortcut: 'Ctrl+N', run() {} }],
}))
void provideOperations
