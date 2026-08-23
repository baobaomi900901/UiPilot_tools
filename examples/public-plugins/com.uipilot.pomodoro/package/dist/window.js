const U64_MAX = '18446744073709551615'
const DEFAULT_DURATION_MINUTES = 10
const MINUTE_MS = 60_000
const STORAGE_KEY = 'pomodoro.duration-minutes'

export const DURATION_OPTIONS = Object.freeze([10, 15, 25, 30, 45])

export function compareU64Decimal(left, right) {
  if (left.length !== right.length) return left.length < right.length ? -1 : 1
  return left === right ? 0 : left < right ? -1 : 1
}

export function validU64Decimal(value) {
  return typeof value === 'string'
    && /^(0|[1-9][0-9]*)$/.test(value)
    && compareU64Decimal(value, U64_MAX) <= 0
}

export function mergeTimerState(current, next, allowEqualRunning = false) {
  if (!validU64Decimal(next?.timerRevision)) return current
  if (!current) return Object.freeze({ ...next })
  const order = compareU64Decimal(next.timerRevision, current.timerRevision)
  if (order > 0) return Object.freeze({ ...next })
  if (
    order === 0
    && allowEqualRunning
    && current.phase === 'running'
    && next.phase === 'running'
    && current.durationMs === next.durationMs
  ) {
    return Object.freeze({ ...next })
  }
  return current
}

export function formatDuration(remainingMs) {
  const totalSeconds = Math.max(0, Math.ceil(remainingMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
}

const legalDuration = (value) => Number.isInteger(value) && DURATION_OPTIONS.includes(value)
const timerErrorText = (error) => error?.code || error?.message || '计时操作失败'

export function createPomodoroModel({
  timer,
  storage,
  completionMessage,
  now = () => performance.now(),
  onChange = () => {},
}) {
  let active = true
  let initialized = false
  let unsubscribe = null
  let effectiveDurationMinutes = DEFAULT_DURATION_MINUTES
  let persistedDurationMinutes = null
  let pendingDurationMinutes = null
  let durationReadPending = true
  let timerState = null
  let timerPending = false
  let storageError = ''
  let timerError = ''
  let anchorAt = now()
  let anchorRemainingMs = DEFAULT_DURATION_MINUTES * MINUTE_MS
  let saveToken = 0
  let timerOperationToken = 0

  const createSnapshot = () => Object.freeze({
    effectiveDurationMinutes,
    persistedDurationMinutes,
    pendingDurationMinutes,
    selectedDurationMinutes: pendingDurationMinutes ?? effectiveDurationMinutes,
    durationReadPending,
    timerState,
    timerPending,
    error: timerError || storageError,
  })
  let snapshot = createSnapshot()

  const emit = () => {
    if (!active) return
    snapshot = createSnapshot()
    try { onChange(snapshot) } catch (_) {}
  }

  const applyTimerState = (next, allowEqualRunning = false) => {
    if (!active) return
    const merged = mergeTimerState(timerState, next, allowEqualRunning)
    if (merged !== timerState) {
      timerState = merged
      anchorAt = now()
      anchorRemainingMs = timerState.remainingMs
        ?? timerState.durationMs
        ?? effectiveDurationMinutes * MINUTE_MS
    }
    emit()
  }

  const initialize = async () => {
    if (!active || initialized) return
    initialized = true
    try {
      unsubscribe = timer.onStateChanged((next) => applyTimerState(next))
    } catch (error) {
      timerError = timerErrorText(error)
      emit()
    }

    let timerRead
    try {
      timerRead = Promise.resolve(timer.getState())
        .then((next) => applyTimerState(next, true))
        .catch((error) => {
          if (!active) return
          timerError = timerErrorText(error)
          emit()
        })
    } catch (error) {
      timerError = timerErrorText(error)
      emit()
      timerRead = Promise.resolve()
    }

    let durationRead
    try {
      durationRead = Promise.resolve(storage.get(STORAGE_KEY))
        .then((value) => {
          if (!active) return
          if (legalDuration(value)) {
            effectiveDurationMinutes = value
            persistedDurationMinutes = value
          } else {
            effectiveDurationMinutes = DEFAULT_DURATION_MINUTES
            persistedDurationMinutes = null
          }
          durationReadPending = false
          emit()
        })
        .catch(() => {
          if (!active) return
          effectiveDurationMinutes = DEFAULT_DURATION_MINUTES
          persistedDurationMinutes = null
          durationReadPending = false
          storageError = '无法读取计时长度'
          emit()
        })
    } catch (_) {
      effectiveDurationMinutes = DEFAULT_DURATION_MINUTES
      persistedDurationMinutes = null
      durationReadPending = false
      storageError = '无法读取计时长度'
      emit()
      durationRead = Promise.resolve()
    }

    await Promise.allSettled([timerRead, durationRead])
  }

  const selectDuration = async (minutes) => {
    if (
      !active
      || durationReadPending
      || pendingDurationMinutes !== null
      || !legalDuration(minutes)
    ) return
    const token = ++saveToken
    pendingDurationMinutes = minutes
    storageError = ''
    emit()
    try {
      await storage.set(STORAGE_KEY, minutes)
      if (!active || token !== saveToken) return
      effectiveDurationMinutes = minutes
      persistedDurationMinutes = minutes
      pendingDurationMinutes = null
      emit()
    } catch (_) {
      if (!active || token !== saveToken) return
      effectiveDurationMinutes = persistedDurationMinutes ?? DEFAULT_DURATION_MINUTES
      pendingDurationMinutes = null
      storageError = '无法保存计时长度'
      emit()
    }
  }

  const runTimer = async (operation) => {
    if (!active || timerPending) return
    const token = ++timerOperationToken
    timerPending = true
    timerError = ''
    emit()
    try {
      const next = await operation()
      if (!active || token !== timerOperationToken) return
      applyTimerState(next)
    } catch (error) {
      if (!active || token !== timerOperationToken) return
      timerError = timerErrorText(error)
    } finally {
      if (active && token === timerOperationToken) {
        timerPending = false
        emit()
      }
    }
  }

  const start = async () => {
    const phase = timerState?.phase ?? 'idle'
    if (!active || phase === 'running') return
    if (phase === 'paused') return runTimer(() => timer.start())
    if (durationReadPending || pendingDurationMinutes !== null) return
    return runTimer(() => timer.start({
      durationMs: effectiveDurationMinutes * MINUTE_MS,
      completionMessage,
    }))
  }

  const pause = async () => {
    if (timerState?.phase !== 'running') return
    return runTimer(() => timer.stop())
  }

  const reset = async () => {
    if (!timerState) return
    return runTimer(() => timer.reset())
  }

  return Object.freeze({
    getSnapshot: () => snapshot,
    initialize,
    selectDuration,
    start,
    pause,
    reset,
    sampledRemaining() {
      if (!timerState || timerState.phase === 'idle') {
        return effectiveDurationMinutes * MINUTE_MS
      }
      if (timerState.phase !== 'running') {
        return timerState.remainingMs ?? timerState.durationMs ?? effectiveDurationMinutes * MINUTE_MS
      }
      return Math.max(0, anchorRemainingMs - (now() - anchorAt))
    },
    dispose() {
      if (!active) return
      active = false
      saveToken += 1
      timerOperationToken += 1
      unsubscribe?.()
      unsubscribe = null
    },
  })
}

if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  const elements = {
    duration: document.querySelector('#duration'),
    time: document.querySelector('#time'),
    status: document.querySelector('#status'),
    start: document.querySelector('#start'),
    pause: document.querySelector('#pause'),
    reset: document.querySelector('#reset'),
    error: document.querySelector('#error'),
  }
  let viewEpoch = 0
  let model = null

  const render = () => {
    if (!model) return
    const snapshot = model.getSnapshot()
    const phase = snapshot.timerState?.phase ?? 'idle'
    elements.duration.value = String(snapshot.selectedDurationMinutes)
    elements.duration.disabled = snapshot.durationReadPending
      || snapshot.pendingDurationMinutes !== null
    elements.time.textContent = formatDuration(model.sampledRemaining())
    elements.status.textContent = {
      idle: '准备',
      running: '进行中',
      paused: '已暂停',
      fired: '已完成',
    }[phase]
    elements.start.textContent = phase === 'paused'
      ? '继续'
      : phase === 'fired' ? '重新开始' : '开始'
    const startingNewRound = phase === 'idle' || phase === 'fired'
    elements.start.disabled = snapshot.timerPending
      || phase === 'running'
      || (startingNewRound && (
        snapshot.durationReadPending || snapshot.pendingDurationMinutes !== null
      ))
    elements.pause.disabled = snapshot.timerPending || phase !== 'running'
    elements.reset.disabled = snapshot.timerPending || !snapshot.timerState
    elements.error.textContent = snapshot.error
  }

  elements.duration.addEventListener('change', () => {
    void model?.selectDuration(Number(elements.duration.value))
  })
  elements.start.addEventListener('click', () => void model?.start())
  elements.pause.addEventListener('click', () => void model?.pause())
  elements.reset.addEventListener('click', () => void model?.reset())

  window.uipilotPluginWindow.onUpdate(async (update) => {
    const epoch = ++viewEpoch
    model?.dispose()
    const timer = window.uipilotPluginWindow.timer
    const storage = window.uipilotPluginWindow.storage
    const completionMessage = typeof update.data?.completionMessage === 'string'
      ? update.data.completionMessage
      : '番茄钟完成'
    let nextModel = null
    nextModel = createPomodoroModel({
      timer,
      storage,
      completionMessage,
      onChange: () => {
        if (viewEpoch === epoch && model === nextModel) render()
      },
    })
    model = nextModel
    render()
    await nextModel.initialize()
  })

  setInterval(render, 100)
}
