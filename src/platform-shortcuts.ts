import { isCompositionKey } from './ime-keyboard'

export function primaryModifierPressed(event: Pick<KeyboardEvent, 'ctrlKey' | 'metaKey'>, platform: string): boolean {
  return platform === 'macos' ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey
}
export function primaryShortcutLabel(platform: string, key: string): string {
  return `${platform === 'macos' ? '⌘' : 'Ctrl'}+${key}`
}
export function isPrimaryShortcut(event: KeyboardEvent, key: string, platform: string): boolean {
  return primaryModifierPressed(event, platform) && !event.altKey && !event.shiftKey &&
    !isCompositionKey(event) && event.key.toLowerCase() === key.toLowerCase()
}
