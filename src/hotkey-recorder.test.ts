import { describe, expect, it } from 'vitest'

import {
  DOUBLE_TAP_WINDOW_MS,
  formatHotkeyDisplay,
  reduceHotkeyRecorder,
  type RecorderState,
} from './hotkey-recorder'

describe('formatHotkeyDisplay', () => {
  it('shows Mac modifier names without changing canonical settings', () => {
    expect(formatHotkeyDisplay('Meta+Alt+Space', 'macos')).toBe('Command + Option + Space')
    expect(formatHotkeyDisplay('Meta+Alt+Space', 'windows')).toBe('Meta + Alt + Space')
  })

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

describe('macOS launcher Command taps', () => {
  function key(type: 'keydown' | 'keyup', nowMs: number, code = 'MetaLeft', repeat = false) {
    return { type, key: code.startsWith('Meta') ? 'Meta' : code, code,
      platform: 'macos' as const, meta: type === 'keydown' && code.startsWith('Meta'),
      repeat, ctrl: false, alt: false, shift: false, nowMs }
  }
  const start: RecorderState = { status: 'recording', baseline: 'Alt+Space' }
  const down = (time: number, code?: string) => key('keydown', time, code)
  const up = (time: number, code?: string) => key('keyup', time, code)
  function commits(events: Array<ReturnType<typeof key>>) {
    let state = start
    return events.map(event => {
      const result = reduceHotkeyRecorder(state, event)
      state = result.state
      return result.commit
    }).filter(Boolean)
  }
  it.each(['MetaLeft', 'MetaRight'])('requires the final release of two standalone %s presses', code => {
    const three = [down(0, code), up(30, code), down(200, code)]
    expect(commits(three)).toEqual([])
    expect(commits([...three, up(400, code)])).toEqual(['DoubleMeta'])
    expect(formatHotkeyDisplay('DoubleMeta', 'macos')).toBe('双击 Command')
  })
  it.each([
    ['timeout', [down(0), up(100), down(200), up(401)]],
    ['long hold', [down(0), up(500), down(600), up(650)]],
    ['mixed sides', [down(0), up(20), down(80, 'MetaRight'), up(100, 'MetaRight')]],
    ['overlap', [down(0), down(10, 'MetaRight'), up(20), up(30, 'MetaRight'), down(40), up(50)]],
    ['repeat', [down(0), key('keydown', 10, 'MetaLeft', true), up(20), down(40), up(50)]],
    ['intervening key', [down(0), up(20), down(30, 'x'), up(40, 'x'), down(50), up(60)]],
    ['held letter', [down(0, 'x'), down(10), up(20), down(30), up(40)]],
    ['Ctrl', [down(0, 'ControlLeft'), up(10, 'ControlLeft'), down(15, 'ControlLeft'), up(20, 'ControlLeft')]],
    ['Alt', [down(0, 'AltLeft'), up(10, 'AltLeft'), down(15, 'AltLeft'), up(20, 'AltLeft')]],
  ] as const)('rejects %s', (_name, events) => {
    expect(commits([...events])).toEqual([])
  })
  it('keeps ordinary Command chords available during recording', () => {
    expect(commits([down(0), { ...down(10, 'c'), meta: true }])).toEqual(['Meta+C'])
  })
})
