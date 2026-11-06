import type { LauncherResultActivation, ResultItem } from './protocol'

const flags = ['plugins', 'files', 'screenshot', 'messages', 'credentials', 'builtinFeatures',
  'commandHotkeys', 'autostart', 'doubleTap'] as const

export type HostCapabilities = Readonly<{
  version: 1
  platform: 'windows' | 'macos'
} & Record<typeof flags[number], boolean>>

// Standalone clients (browser preview and test fixtures) model the full Windows host.
export const previewCapabilities: HostCapabilities = Object.freeze({
  version: 1, platform: 'windows', plugins: true, files: true, screenshot: true,
  messages: true, credentials: true, builtinFeatures: true, commandHotkeys: true,
  autostart: true, doubleTap: true,
})

export function parseHostCapabilities(value: unknown): HostCapabilities | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  const snapshot = value as Record<string, unknown>
  if (snapshot.version !== 1 || (snapshot.platform !== 'macos' && snapshot.platform !== 'windows') ||
      flags.some(key => !Object.prototype.hasOwnProperty.call(snapshot, key) || typeof snapshot[key] !== 'boolean')) return null
  return Object.freeze({ ...snapshot }) as HostCapabilities
}

export function settingsTabAvailable(host: HostCapabilities, tab: string): boolean {
  switch (tab) {
    case 'general': return true
    case 'permissions':
    case 'plugins': return host.plugins
    case 'credentials': return host.credentials
    case 'messages': return host.messages
    default: return false
  }
}

export function resultAvailable(host: HostCapabilities, activation: LauncherResultActivation, item: ResultItem): boolean {
  if (!host.plugins && (item.actions || item.pluginIconUrl || item.favorite?.target?.kind === 'publicPlugin')) return false
  if (!host.builtinFeatures && (item.iconKind || item.favorite?.target?.kind === 'builtin')) {
    if (activation.kind !== 'openFind' || !host.files || item.iconKind !== 'find' ||
        (item.favorite && (item.favorite.target?.kind !== 'builtin' || item.favorite.target.feature !== 'find'))) return false
  }
  switch (activation.kind) {
    case 'openFind': return host.files
    case 'openScreenshot': return host.screenshot
    case 'openQuicklinks':
    case 'builtinActivation':
    case 'completion':
    case 'informational': return host.builtinFeatures
    case 'pluginCompletion':
    case 'mainResultActivation':
    case 'panelActivation':
    case 'windowActivation': return host.plugins
    case 'executeResult': return true
  }
}
