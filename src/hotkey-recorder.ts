export type RecorderStatus = 'idle' | 'recording'

export type RecorderState = {
  status: RecorderStatus
  baseline: string
  pendingTap?: { modifier: 'Ctrl' | 'Alt'; atMs: number }
  command?: {
    held: string[]
    down?: { code: string; atMs: number }
    firstTap?: { code: string; atMs: number }
  }
}

type RecorderKeyEvent = {
  type: 'keydown' | 'keyup'
  key: string
  code: string
  ctrl: boolean
  alt: boolean
  shift: boolean
  meta: boolean
  repeat: boolean
  allowDoubleTap?: boolean
  platform?: 'windows' | 'macos'
  nowMs: number
}

export type RecorderEvent =
  | { type: 'start'; baseline: string }
  | { type: 'blur' | 'cancel' }
  | { type: 'pointer'; id: number; down: boolean }
  | RecorderKeyEvent

export type RecorderResult = {
  state: RecorderState
  commit?: string
  display: string
}

export const DOUBLE_TAP_WINDOW_MS = 400

const RECORDING_DISPLAY = '按下快捷键…'

export function formatHotkeyDisplay(canonical: string, platform: 'windows' | 'macos' = 'windows'): string {
  if (canonical === 'DoubleMeta' && platform === 'macos') return '双击 Command'
  if (canonical === 'DoubleCtrl') return '双击 Ctrl'
  if (canonical === 'DoubleAlt') return '双击 Alt'
  if (canonical.includes('+')) {
    return canonical.split('+').map(key => {
      if (platform === 'macos' && key === 'Alt') return 'Option'
      if (platform === 'macos' && key === 'Meta') return 'Command'
      return key
    }).join(' + ')
  }
  return canonical
}

function isCtrlKey(event: RecorderKeyEvent): boolean {
  return (
    event.key === 'Control' ||
    event.code === 'ControlLeft' ||
    event.code === 'ControlRight'
  )
}

function isAltKey(event: RecorderKeyEvent): boolean {
  return (
    event.key === 'Alt' || event.code === 'AltLeft' || event.code === 'AltRight'
  )
}

function isModifierOnlyKey(event: RecorderKeyEvent): boolean {
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

function buildChordCanonical(event: RecorderKeyEvent): string | undefined {
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
    state: { status: 'idle', baseline: state.baseline },
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

function handleKeydown(state: RecorderState, event: RecorderKeyEvent): RecorderResult {
  if (event.repeat) {
    return recordingResult(state)
  }

  if (event.platform !== 'macos' && isCtrlKey(event) && event.allowDoubleTap !== false) {
    return handleDoubleTap(state, 'Ctrl', event.nowMs)
  }

  if (event.platform !== 'macos' && isAltKey(event) && event.allowDoubleTap !== false) {
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

function recordMacKey(state: RecorderState, event: RecorderKeyEvent): RecorderResult {
  const previous = state.command ?? { held: [] }
  const code = event.code || event.key
  const held = previous.held.filter(key => key !== code)
  const command: NonNullable<RecorderState['command']> = { held }
  const standaloneCommand = (code === 'MetaLeft' || code === 'MetaRight') &&
    !event.ctrl && !event.alt && !event.shift

  if (event.type === 'keydown') {
    held.push(code)
    if (standaloneCommand && previous.held.length === 0 && !event.repeat) {
      command.down = { code, atMs: event.nowMs }
      command.firstTap = previous.firstTap
    }
    return handleKeydown({ ...state, command }, event)
  }

  if (standaloneCommand && !event.meta && held.length === 0 && previous.down?.code === code &&
      event.nowMs - previous.down.atMs <= DOUBLE_TAP_WINDOW_MS) {
    if (previous.firstTap?.code === code && event.nowMs - previous.firstTap.atMs <= DOUBLE_TAP_WINDOW_MS) {
      return { state: { status: 'idle', baseline: 'DoubleMeta' }, commit: 'DoubleMeta', display: '双击 Command' }
    }
    command.firstTap = previous.down
  }
  return recordingResult({ ...state, command })
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
    case 'pointer': {
      if (state.status !== 'recording') return idleResult(state)
      const id = `pointer-${event.id}`
      const held = (state.command?.held ?? []).filter(key => key !== id)
      if (event.down) held.push(id)
      return recordingResult({ ...state, command: { held } })
    }
    case 'keydown':
    case 'keyup':
      if (state.status !== 'recording') {
        return {
          state,
          display: formatHotkeyDisplay(state.baseline),
        }
      }
      if (event.platform === 'macos' && event.allowDoubleTap !== false) return recordMacKey(state, event)
      return event.type === 'keydown' ? handleKeydown(state, event) : recordingResult(state)
  }
}
