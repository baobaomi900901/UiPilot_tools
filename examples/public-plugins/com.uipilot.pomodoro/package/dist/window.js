const U64_MAX = '18446744073709551615'

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

if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  const elements = {
    time: document.querySelector('#time'),
    status: document.querySelector('#status'),
    start: document.querySelector('#start'),
    pause: document.querySelector('#pause'),
    reset: document.querySelector('#reset'),
    error: document.querySelector('#error'),
  }
  let timer = null
  let unsubscribe = null
  let state = null
  let initialDurationMs = 10_000
  let completionMessage = '番茄钟完成'
  let anchorAt = performance.now()
  let anchorRemainingMs = initialDurationMs
  let pending = false

  const sampledRemaining = () => {
    if (!state) return initialDurationMs
    if (state.phase !== 'running') return state.remainingMs ?? initialDurationMs
    return Math.max(0, anchorRemainingMs - (performance.now() - anchorAt))
  }

  const render = () => {
    const phase = state?.phase ?? 'idle'
    elements.time.textContent = formatDuration(sampledRemaining())
    elements.status.textContent = {
      idle: '准备',
      running: '进行中',
      paused: '已暂停',
      fired: '已完成',
    }[phase]
    elements.start.textContent = phase === 'paused' ? '继续' : phase === 'fired' ? '重新开始' : '开始'
    elements.start.disabled = pending || phase === 'running'
    elements.pause.disabled = pending || phase !== 'running'
    elements.reset.disabled = pending || (!state && phase === 'idle')
  }

  const applyState = (next, allowEqualRunning = false) => {
    const merged = mergeTimerState(state, next, allowEqualRunning)
    if (merged !== state) {
      state = merged
      anchorAt = performance.now()
      anchorRemainingMs = state.remainingMs ?? initialDurationMs
    }
    render()
  }

  const run = async (operation) => {
    if (!timer || pending) return
    pending = true
    elements.error.textContent = ''
    render()
    try {
      applyState(await operation())
    } catch (error) {
      elements.error.textContent = error?.code || error?.message || '计时操作失败'
    } finally {
      pending = false
      render()
    }
  }

  elements.start.addEventListener('click', () => void run(() => {
    if (state?.phase === 'paused') return timer.start()
    return timer.start({ durationMs: initialDurationMs, completionMessage })
  }))
  elements.pause.addEventListener('click', () => void run(() => timer.stop()))
  elements.reset.addEventListener('click', () => void run(() => timer.reset()))

  window.uipilotPluginWindow.onUpdate(async (update) => {
    initialDurationMs = Number.isSafeInteger(update.data?.initialDurationMs)
      ? update.data.initialDurationMs
      : 10_000
    completionMessage = typeof update.data?.completionMessage === 'string'
      ? update.data.completionMessage
      : '番茄钟完成'
    state = null
    anchorAt = performance.now()
    anchorRemainingMs = initialDurationMs
    unsubscribe?.()
    timer = window.uipilotPluginWindow.timer
    unsubscribe = timer.onStateChanged((next) => applyState(next))
    render()
    applyState(await timer.getState(), true)
  })

  setInterval(render, 100)
  render()
}
