export type RecorderStatus = 'idle' | 'recording'

export type RecorderState = {
  status: RecorderStatus
  baseline: string
  pendingTap?: { modifier: 'Ctrl' | 'Alt'; atMs: number }
}

export type RecorderEvent =
  | { type: 'start'; baseline: string }
  | { type: 'blur' }
  | {
      type: 'keydown'
      key: string
      code: string
      ctrl: boolean
      alt: boolean
      shift: boolean
      meta: boolean
      repeat: boolean
      nowMs: number
    }
  | { type: 'cancel' }

export type RecorderResult = {
  state: RecorderState
  commit?: string
  display: string
}

export const DOUBLE_TAP_WINDOW_MS = 400

const RECORDING_DISPLAY = '按下快捷键…'

type KeydownEvent = Extract<RecorderEvent, { type: 'keydown' }>

export function formatHotkeyDisplay(canonical: string): string {
  if (canonical === 'DoubleCtrl') return '双击 Ctrl'
  if (canonical === 'DoubleAlt') return '双击 Alt'
  if (canonical.includes('+')) {
    return canonical.split('+').join(' + ')
  }
  return canonical
}

function isCtrlKey(event: KeydownEvent): boolean {
  return (
    event.key === 'Control' ||
    event.code === 'ControlLeft' ||
    event.code === 'ControlRight'
  )
}

function isAltKey(event: KeydownEvent): boolean {
  return (
    event.key === 'Alt' || event.code === 'AltLeft' || event.code === 'AltRight'
  )
}

function isModifierOnlyKey(event: KeydownEvent): boolean {
  return (
    isCtrlKey(event) ||
    isAltKey(event) ||
    event.key === 'Shift' ||
    event.code.startsWith('Shift') ||
    event.key === 'Meta' ||
    event.code.startsWith('Meta')
  )
}

function buildMainKey(key: string, code: string): string {
  if (key === ' ' || code === 'Space') return 'Space'
  if (key.length === 1 && /[a-zA-Z]/.test(key)) return key.toUpperCase()
  if (/^F\d+$/.test(key)) return key
  if (key && key !== 'Unidentified') return key
  return code
}

function buildChordCanonical(event: KeydownEvent): string | undefined {
  if (isModifierOnlyKey(event)) return undefined

  const modifiers: string[] = []
  if (event.ctrl) modifiers.push('Ctrl')
  if (event.shift) modifiers.push('Shift')
  if (event.alt) modifiers.push('Alt')
  if (event.meta) modifiers.push('Meta')

  if (modifiers.length === 0) return undefined

  return [...modifiers, buildMainKey(event.key, event.code)].join('+')
}

function idleResult(state: RecorderState): RecorderResult {
  return {
    state: { ...state, status: 'idle', pendingTap: undefined },
    display: formatHotkeyDisplay(state.baseline),
  }
}

function recordingResult(state: RecorderState): RecorderResult {
  return {
    state,
    display: RECORDING_DISPLAY,
  }
}

function handleDoubleTap(
  state: RecorderState,
  modifier: 'Ctrl' | 'Alt',
  nowMs: number,
): RecorderResult {
  const pending = state.pendingTap
  if (
    pending?.modifier === modifier &&
    nowMs - pending.atMs <= DOUBLE_TAP_WINDOW_MS
  ) {
    const canonical = modifier === 'Ctrl' ? 'DoubleCtrl' : 'DoubleAlt'
    return {
      state: { status: 'idle', baseline: canonical, pendingTap: undefined },
      commit: canonical,
      display: formatHotkeyDisplay(canonical),
    }
  }

  return recordingResult({
    ...state,
    pendingTap: { modifier, atMs: nowMs },
  })
}

function handleKeydown(state: RecorderState, event: KeydownEvent): RecorderResult {
  if (event.repeat) {
    return recordingResult(state)
  }

  if (isCtrlKey(event)) {
    return handleDoubleTap(state, 'Ctrl', event.nowMs)
  }

  if (isAltKey(event)) {
    return handleDoubleTap(state, 'Alt', event.nowMs)
  }

  const nextState: RecorderState = { ...state, pendingTap: undefined }

  if (isModifierOnlyKey(event)) {
    return recordingResult(nextState)
  }

  const commit = buildChordCanonical(event)
  if (!commit) {
    return recordingResult(nextState)
  }

  return {
    state: { status: 'idle', baseline: commit, pendingTap: undefined },
    commit,
    display: formatHotkeyDisplay(commit),
  }
}

export function reduceHotkeyRecorder(
  state: RecorderState,
  event: RecorderEvent,
): RecorderResult {
  switch (event.type) {
    case 'start':
      return recordingResult({
        status: 'recording',
        baseline: event.baseline,
        pendingTap: undefined,
      })
    case 'blur':
    case 'cancel':
      return idleResult(state)
    case 'keydown':
      if (state.status !== 'recording') {
        return {
          state,
          display: formatHotkeyDisplay(state.baseline),
        }
      }
      return handleKeydown(state, event)
  }
}
