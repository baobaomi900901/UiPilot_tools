import { Button, Input, type InputRef } from 'antd'
import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { formatHotkeyDisplay, reduceHotkeyRecorder, type RecorderState } from './hotkey-recorder'
import type { HotkeyRecordingPhase } from './launcher-core'
export const HOTKEY_RECORDING_CURRENT_DOM_EVENT = 'uipilot-hotkey-recording-current'
const noopPhase = (_phase: HotkeyRecordingPhase) => {}

// The native suppression flag belongs to the main window, so all its recorder
// instances share one lease, including while IPC is arming or releasing it.
let recordingOwner: { id: symbol; releasing: boolean } | undefined
const ownerListeners = new Set<() => void>()
const subscribeOwner = (listener: () => void) => {
  ownerListeners.add(listener)
  return () => { ownerListeners.delete(listener) }
}
const getOwner = () => recordingOwner
function publishOwner(owner: typeof recordingOwner): void {
  recordingOwner = owner
  ownerListeners.forEach((listener) => listener())
}

interface HotkeyRecorderInputProps {
  onCommit: (canonical: string) => void | Promise<void>
  onRecordingPhase?: (phase: HotkeyRecordingPhase) => void | Promise<void>
  platform?: 'windows' | 'macos'
  allowDoubleTap?: boolean
  allowSameValue?: boolean
  label?: string
  value: string
  disabled?: boolean
  id?: string
  name?: string
}

export function HotkeyRecorderInput({ onCommit, onRecordingPhase = noopPhase, platform, allowDoubleTap = true, allowSameValue = false, label = '快捷键', value, disabled, id, name }: HotkeyRecorderInputProps): React.JSX.Element {
  const [ownerId] = useState(() => Symbol('hotkey-recorder'))
  const owner = useSyncExternalStore(subscribeOwner, getOwner, getOwner)
  const blockedByOwner = owner !== undefined && (owner.id !== ownerId || owner.releasing)
  const [recorderState, setRecorderState] = useState<RecorderState>(() => ({ status: 'idle', baseline: value }))
  const [arming, setArming] = useState(false)
  const [armingFailed, setArmingFailed] = useState(false)
  const armRevision = useRef(0)
  const recorderStateRef = useRef(recorderState)
  const inputRef = useRef<InputRef>(null)
  const actionRef = useRef<HTMLButtonElement>(null)
  const returnFocusToAction = useRef(false)
  const pendingCommit = useRef<{ canonical: string; finalState: RecorderState } | undefined>(undefined)
  const recording = recorderState.status === 'recording'

  const releaseRecording = useCallback((phase: 'idle' | 'completed') => {
    if (recordingOwner?.id !== ownerId || recordingOwner.releasing) return
    publishOwner({ id: ownerId, releasing: true })
    void (async () => {
      try { await onRecordingPhase(phase) } catch { /* The next arm retries host synchronization. */ }
      finally { if (recordingOwner?.id === ownerId) publishOwner(undefined) }
    })()
  }, [onRecordingPhase, ownerId])

  const updateRecorderState = useCallback((next: RecorderState) => {
    recorderStateRef.current = next
    setRecorderState(next)
  }, [])

  useEffect(() => {
    const current = recorderStateRef.current
    if (current.status === 'recording') return
    updateRecorderState({ status: 'idle', baseline: value, pendingTap: undefined })
  }, [updateRecorderState, value])

  useEffect(() => () => { armRevision.current += 1; releaseRecording('idle') }, [releaseRecording])

  useEffect(() => {
    const completeCurrentHotkeyRecording = (event: Event) => {
      const canonical = (event as CustomEvent<unknown>).detail
      if (typeof canonical === 'string' && (!allowDoubleTap && canonical.startsWith('Double'))) return
      if (recorderStateRef.current.status !== 'recording' || recordingOwner?.id !== ownerId || recordingOwner.releasing) return
      pendingCommit.current = undefined
      returnFocusToAction.current = true
      releaseRecording(typeof canonical === 'string' ? 'completed' : 'idle')
      if (typeof canonical === 'string') {
        updateRecorderState({ status: 'idle', baseline: canonical })
        if (canonical !== value || allowSameValue) void onCommit(canonical)
        return
      }
      updateRecorderState(
        reduceHotkeyRecorder(recorderStateRef.current, { type: 'cancel' }).state,
      )
    }
    window.addEventListener(HOTKEY_RECORDING_CURRENT_DOM_EVENT, completeCurrentHotkeyRecording)
    return () => {
      window.removeEventListener(HOTKEY_RECORDING_CURRENT_DOM_EVENT, completeCurrentHotkeyRecording)
    }
  }, [releaseRecording, ownerId, onCommit, allowDoubleTap, allowSameValue, value, updateRecorderState])

  const recordingPrompt = platform === 'macos' && !allowDoubleTap
    ? '按下组合键，如 ⌥ + 空格' : '按下快捷键…'
  const display = recording ? recordingPrompt : value ? formatHotkeyDisplay(value, platform) : ''

  useLayoutEffect(() => {
    if (recording) {
      inputRef.current?.input?.focus()
      return
    }
    if (!disabled && !blockedByOwner && returnFocusToAction.current) {
      returnFocusToAction.current = false
      actionRef.current?.focus()
    }
  }, [disabled, blockedByOwner, recording])

  const toggleRecording = useCallback(async () => {
    if (disabled) return
    if (recording) {
      releaseRecording('idle')
      updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'cancel' }).state)
      return
    }
    // Reserve synchronously: two clicks in one render cannot both arm the host.
    if (recordingOwner) return
    publishOwner({ id: ownerId, releasing: false })
    pendingCommit.current = undefined
    const revision = ++armRevision.current
    setArmingFailed(false)
    setArming(true)
    try {
      await onRecordingPhase('recording')
    } catch {
      releaseRecording('idle')
      if (armRevision.current === revision) { setArming(false); setArmingFailed(true) }
      return
    }
    const currentOwner = getOwner()
    if (armRevision.current !== revision || currentOwner?.id !== ownerId || currentOwner.releasing) return
    setArming(false)
    updateRecorderState(reduceHotkeyRecorder(
      recorderStateRef.current,
      { type: 'start', baseline: value },
    ).state)
  }, [onRecordingPhase, releaseRecording, ownerId, disabled, recording, updateRecorderState, value])

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Tab') {
      pendingCommit.current = undefined
      releaseRecording('idle')
      updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'cancel' }).state)
      event.stopPropagation()
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      pendingCommit.current = undefined
      returnFocusToAction.current = true
      releaseRecording('idle')
      updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'cancel' }).state)
      return
    }
    if (pendingCommit.current || recorderStateRef.current.status !== 'recording') return
    const result = reduceHotkeyRecorder(recorderStateRef.current, {
      type: 'keydown',
      key: event.key,
      code: event.code,
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
      repeat: event.repeat,
      nowMs: Date.now(),
      allowDoubleTap,
    })
    if (result.commit) {
      if (result.commit === value && !allowSameValue) {
        returnFocusToAction.current = true
        releaseRecording('completed')
        updateRecorderState(result.state)
        return
      }
      pendingCommit.current = { canonical: result.commit, finalState: result.state }
      updateRecorderState({ ...result.state, status: 'recording' })
      return
    }
    updateRecorderState(result.state)
  }

  const handleKeyUp = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    event.preventDefault()
    event.stopPropagation()
    const pending = pendingCommit.current
    if (!pending) return
    pendingCommit.current = undefined
    returnFocusToAction.current = true
    releaseRecording('completed')
    updateRecorderState(pending.finalState)
    void onCommit(pending.canonical)
  }

  return (
    <div className="settings-hotkey-recorder">
      <Input
        ref={inputRef}
        readOnly
        aria-label={label}
        value={display}
        placeholder="未设置"
        id={id}
        name={name}
        disabled={disabled || !recording || blockedByOwner}
        tabIndex={recording ? 0 : -1}
        onBlur={() => {
          if (recorderStateRef.current.status !== 'recording') return
          pendingCommit.current = undefined
          releaseRecording('idle')
          updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'blur' }).state)
        }}
        onKeyDown={handleKeyDown}
        onKeyUp={handleKeyUp}
      />
      <Button ref={actionRef} aria-label={`录制${label}`} disabled={disabled || arming || blockedByOwner} onClick={() => void toggleRecording()}>
        {recording ? '取消录制' : value ? '重新录制' : '录制'}
      </Button>
      {armingFailed && <span role="status">无法开始录制，请重试。</span>}
    </div>
  )
}
