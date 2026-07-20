import { describe, expect, it } from 'vitest'

import {
  DOUBLE_TAP_WINDOW_MS,
  formatHotkeyDisplay,
  reduceHotkeyRecorder,
  type RecorderState,
} from './hotkey-recorder'

describe('formatHotkeyDisplay', () => {
  it('maps double-tap canonical values', () => {
    expect(formatHotkeyDisplay('DoubleCtrl')).toBe('双击 Ctrl')
    expect(formatHotkeyDisplay('DoubleAlt')).toBe('双击 Alt')
  })

  it('inserts spaces around plus in chords', () => {
    expect(formatHotkeyDisplay('Ctrl+Space')).toBe('Ctrl + Space')
    expect(formatHotkeyDisplay('Ctrl+Shift+Space')).toBe('Ctrl + Shift + Space')
  })
})

describe('reduceHotkeyRecorder', () => {
  it('commits Ctrl+Space chord', () => {
    let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
    state = r.state
    expect(r.display).toBe('按下快捷键…')

    r = reduceHotkeyRecorder(state, {
      type: 'keydown',
      key: ' ',
      code: 'Space',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: false,
      nowMs: 0,
    })
    expect(r.commit).toBe('Ctrl+Space')
    expect(r.state.status).toBe('idle')
    expect(r.display).toBe('Ctrl + Space')
  })

  it('commits DoubleCtrl within 400ms', () => {
    let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
    state = r.state

    const ctrlDown = {
      type: 'keydown' as const,
      key: 'Control',
      code: 'ControlLeft',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: false,
      nowMs: 0,
    }

    r = reduceHotkeyRecorder(state, ctrlDown)
    expect(r.commit).toBeUndefined()
    expect(r.state.pendingTap).toEqual({ modifier: 'Ctrl', atMs: 0 })
    state = r.state

    r = reduceHotkeyRecorder(state, { ...ctrlDown, nowMs: DOUBLE_TAP_WINDOW_MS - 1 })
    expect(r.commit).toBe('DoubleCtrl')
    expect(r.state.status).toBe('idle')
    expect(r.display).toBe('双击 Ctrl')
  })

  it('does not commit DoubleCtrl outside 400ms on second press alone', () => {
    let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
    state = r.state

    const ctrlDown = {
      type: 'keydown' as const,
      key: 'Control',
      code: 'ControlLeft',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: false,
      nowMs: 0,
    }

    r = reduceHotkeyRecorder(state, ctrlDown)
    state = r.state

    r = reduceHotkeyRecorder(state, { ...ctrlDown, nowMs: DOUBLE_TAP_WINDOW_MS + 1 })
    expect(r.commit).toBeUndefined()
    expect(r.state.status).toBe('recording')
    expect(r.state.pendingTap).toEqual({ modifier: 'Ctrl', atMs: DOUBLE_TAP_WINDOW_MS + 1 })
  })

  it('Escape restores baseline without commit', () => {
    let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
    state = r.state

    r = reduceHotkeyRecorder(state, {
      type: 'keydown',
      key: 'Control',
      code: 'ControlLeft',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: false,
      nowMs: 0,
    })
    state = r.state

    r = reduceHotkeyRecorder(state, { type: 'cancel' })
    expect(r.commit).toBeUndefined()
    expect(r.state.status).toBe('idle')
    expect(r.state.pendingTap).toBeUndefined()
    expect(r.display).toBe('Alt + Space')
  })

  it('blur cancels without commit', () => {
    let state: RecorderState = { status: 'idle', baseline: 'DoubleCtrl' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'DoubleCtrl' })
    state = r.state

    r = reduceHotkeyRecorder(state, {
      type: 'keydown',
      key: 'Control',
      code: 'ControlLeft',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: false,
      nowMs: 0,
    })
    state = r.state

    r = reduceHotkeyRecorder(state, { type: 'blur' })
    expect(r.commit).toBeUndefined()
    expect(r.state.status).toBe('idle')
    expect(r.state.pendingTap).toBeUndefined()
    expect(r.display).toBe('双击 Ctrl')
  })

  it('ignores repeat keydown events', () => {
    let state: RecorderState = { status: 'idle', baseline: 'Alt+Space' }
    let r = reduceHotkeyRecorder(state, { type: 'start', baseline: 'Alt+Space' })
    state = r.state

    r = reduceHotkeyRecorder(state, {
      type: 'keydown',
      key: 'Control',
      code: 'ControlLeft',
      ctrl: true,
      alt: false,
      shift: false,
      meta: false,
      repeat: true,
      nowMs: 100,
    })
    expect(r.commit).toBeUndefined()
    expect(r.state.pendingTap).toBeUndefined()
    expect(r.state.status).toBe('recording')
  })
})
