import { Button, Input, type InputRef } from 'antd'
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { formatHotkeyDisplay, reduceHotkeyRecorder, type RecorderState } from './hotkey-recorder'
import type { HotkeyRecordingPhase } from './launcher-core'
export const HOTKEY_RECORDING_CURRENT_DOM_EVENT = 'uipilot-hotkey-recording-current'
const noopPhase = (_phase: HotkeyRecordingPhase) => {}

interface HotkeyRecorderInputProps {
  onCommit: (canonical: string) => void | Promise<void>
  onRecordingPhase?: (phase: HotkeyRecordingPhase) => void
  allowDoubleTap?: boolean
  label?: string
  value: string
  disabled?: boolean
  id?: string
  name?: string
}

export function HotkeyRecorderInput({ onCommit, onRecordingPhase = noopPhase, allowDoubleTap = true, label = '快捷键', value, disabled, id, name }: HotkeyRecorderInputProps): React.JSX.Element {
  const [recorderState, setRecorderState] = useState<RecorderState>(() => ({ status: 'idle', baseline: value }))
  const recorderStateRef = useRef(recorderState)
  const inputRef = useRef<InputRef>(null)
  const actionRef = useRef<HTMLButtonElement>(null)
  const returnFocusToAction = useRef(false)
  const pendingCommit = useRef<{ canonical: string; finalState: RecorderState } | undefined>(undefined)
  const recording = recorderState.status === 'recording'

  const updateRecorderState = useCallback((next: RecorderState) => {
    recorderStateRef.current = next
    setRecorderState(next)
  }, [])

  useEffect(() => {
    const current = recorderStateRef.current
    if (current.status === 'recording') return
    updateRecorderState({ status: 'idle', baseline: value, pendingTap: undefined })
  }, [updateRecorderState, value])

  useEffect(() => () => onRecordingPhase('idle'), [onRecordingPhase])

  useEffect(() => {
    const completeCurrentHotkeyRecording = (event: Event) => {
      const canonical = (event as CustomEvent<unknown>).detail
      if (typeof canonical === 'string' && (!allowDoubleTap && canonical.startsWith('Double'))) return
      if (recorderStateRef.current.status !== 'recording') return
      pendingCommit.current = undefined
      returnFocusToAction.current = true
      onRecordingPhase(typeof canonical === 'string' ? 'completed' : 'idle')
      if (typeof canonical === 'string') {
        updateRecorderState({ status: 'idle', baseline: canonical })
        if (canonical !== value) void onCommit(canonical)
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
  }, [onRecordingPhase, onCommit, allowDoubleTap, value, updateRecorderState])

  const display =
    recording ? '按下快捷键…' : value ? formatHotkeyDisplay(value) : '未设置'

  useLayoutEffect(() => {
    if (recording) {
      inputRef.current?.input?.focus()
      return
    }
    if (!disabled && returnFocusToAction.current) {
      returnFocusToAction.current = false
      actionRef.current?.focus()
    }
  }, [disabled, recording])

  const toggleRecording = useCallback(() => {
    if (disabled) return
    pendingCommit.current = undefined
    onRecordingPhase(recording ? 'idle' : 'recording')
    updateRecorderState(reduceHotkeyRecorder(
      recorderStateRef.current,
      recording ? { type: 'cancel' } : { type: 'start', baseline: value },
    ).state)
  }, [onRecordingPhase, disabled, recording, updateRecorderState, value])

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Tab') {
      pendingCommit.current = undefined
      onRecordingPhase('idle')
      updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'cancel' }).state)
      event.stopPropagation()
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      pendingCommit.current = undefined
      returnFocusToAction.current = true
      onRecordingPhase('idle')
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
      if (result.commit === value) {
        returnFocusToAction.current = true
        onRecordingPhase('completed')
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
    onRecordingPhase('completed')
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
        id={id}
        name={name}
        disabled={disabled || !recording}
        tabIndex={recording ? 0 : -1}
        onBlur={() => {
          if (recorderStateRef.current.status !== 'recording') return
          pendingCommit.current = undefined
          onRecordingPhase('idle')
          updateRecorderState(reduceHotkeyRecorder(recorderStateRef.current, { type: 'blur' }).state)
        }}
        onKeyDown={handleKeyDown}
        onKeyUp={handleKeyUp}
      />
      <Button ref={actionRef} aria-label={`录制${label}`} disabled={disabled} onClick={toggleRecording}>
        {recording ? '取消录制' : '重新录制'}
      </Button>
    </div>
  )
}
