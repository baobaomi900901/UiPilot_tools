import { isPrimaryShortcut } from './platform-shortcuts'

export function isWindowPinShortcut(event: KeyboardEvent, platform = 'windows'): boolean {
  return isPrimaryShortcut(event, 't', platform)
}

export const WINDOW_PIN_SHORTCUT_EVENT = 'uipilot-window-pin-shortcut'
