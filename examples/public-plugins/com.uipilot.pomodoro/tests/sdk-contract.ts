import type {
  JsonValue,
  PluginHandler,
  PluginTimerStartInput,
  PluginTimerState,
  PluginWindowUpdate,
  UiPilotPluginWindowApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: { completionMessage: invocation.input || '番茄钟完成' },
})

const consumeWindowApi = (api: Readonly<UiPilotPluginWindowApiV1>) => api.onUpdate(
  async (update: Readonly<PluginWindowUpdate>) => {
    if (update.data === null || typeof update.data !== 'object' || Array.isArray(update.data)) return
    const message = update.data.completionMessage
    if (typeof message !== 'string') return
    const stored: JsonValue | null = await api.storage.get('pomodoro.duration-minutes')
    const durationMinutes = typeof stored === 'number' ? stored : 10
    const input: Readonly<PluginTimerStartInput> = {
      durationMs: durationMinutes * 60_000,
      completionMessage: message,
    }
    const unsubscribe = api.timer.onStateChanged((state: Readonly<PluginTimerState>) => {
      void state.timerRevision
    })
    await api.timer.getState()
    await api.timer.start(input)
    await api.timer.stop()
    await api.timer.start()
    await api.timer.reset()
    await api.storage.set('pomodoro.duration-minutes', durationMinutes)
    await api.storage.remove('pomodoro.duration-minutes')
    unsubscribe()
  },
)

void handler
void consumeWindowApi
