import type {
  PluginHandler,
  PluginTimerStartInput,
  PluginTimerState,
  PluginWindowUpdate,
  UiPilotPluginWindowApiV1,
} from '../../../../docs/plugin-sdk/uipilot-plugin-api-v1'

const handler: PluginHandler = async (invocation) => ({
  requestId: invocation.requestId,
  data: { initialDurationMs: 10_000, completionMessage: invocation.input || '番茄钟完成' },
})

const consumeWindowApi = (api: Readonly<UiPilotPluginWindowApiV1>) => api.onUpdate(
  async (update: Readonly<PluginWindowUpdate>) => {
    if (update.data === null || typeof update.data !== 'object' || Array.isArray(update.data)) return
    const duration = update.data.initialDurationMs
    const message = update.data.completionMessage
    if (typeof duration !== 'number' || typeof message !== 'string') return
    const input: Readonly<PluginTimerStartInput> = {
      durationMs: duration,
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
    unsubscribe()
  },
)

void handler
void consumeWindowApi
