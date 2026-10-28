export function isWindowPinShortcut(event: KeyboardEvent): boolean {
  return event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey &&
    !event.isComposing && event.key.toLowerCase() === 't'
}

export const WINDOW_PIN_SHORTCUT_EVENT = 'uipilot-window-pin-shortcut'
