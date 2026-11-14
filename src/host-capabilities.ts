import type { LauncherResultActivation, ResultItem } from './protocol'

const flags = ['plugins', 'files', 'screenshot', 'messages', 'credentials', 'builtinFeatures',
  'commandHotkeys', 'autostart', 'doubleTap', 'webSearch', 'loginItems'] as const

export type HostCapabilities = Readonly<{
  version: 1
  platform: 'windows' | 'macos'
  pluginManagement?: boolean
  quicklinks?: boolean
} & Record<typeof flags[number], boolean>>

// Standalone clients (browser preview and test fixtures) model the full Windows host.
export const previewCapabilities: HostCapabilities = Object.freeze({
  version: 1, platform: 'windows', quicklinks: true, plugins: true, files: true, screenshot: true,
  messages: true, credentials: true, builtinFeatures: true, commandHotkeys: true,
  autostart: true, doubleTap: true, webSearch: true, loginItems: false,
})

export function parseHostCapabilities(value: unknown): HostCapabilities | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  const snapshot = value as Record<string, unknown>
  if (snapshot.version !== 1 || (snapshot.platform !== 'macos' && snapshot.platform !== 'windows') ||
      flags.some(key => !Object.prototype.hasOwnProperty.call(snapshot, key) || typeof snapshot[key] !== 'boolean')) return null
  if (snapshot.pluginManagement !== undefined && typeof snapshot.pluginManagement !== 'boolean') return null
  if (snapshot.quicklinks !== undefined && typeof snapshot.quicklinks !== 'boolean') return null
  return Object.freeze({ ...snapshot }) as HostCapabilities
}

export function settingsTabAvailable(host: HostCapabilities, tab: string): boolean {
  switch (tab) {
    case 'general': return true
    case 'permissions': return host.plugins
    case 'plugins': return host.plugins || host.pluginManagement === true
    case 'credentials': return host.credentials
    case 'messages': return host.messages
    default: return false
  }
}

export function resultAvailable(host: HostCapabilities, activation: LauncherResultActivation, item: ResultItem): boolean {
  if (activation.kind === 'unavailablePlugin') return host.platform === 'macos' && host.pluginManagement === true && !host.plugins && !item.actions && !item.pluginIconUrl && !item.iconKind && item.hasDefaultAction === false
  if (!host.plugins && (item.actions || item.pluginIconUrl || item.favorite?.target?.kind === 'publicPlugin')) return false
  if (host.platform === 'macos' && host.quicklinks === true) {
    if (activation.kind === 'openQuicklinks' && item.title === '/quicklinks' && item.iconKind === 'quicklinks' && item.hasDefaultAction === false &&
        item.favorite?.target.kind === 'builtin' && item.favorite.target.feature === 'quicklinks') return true
    if (activation.kind === 'completion' && !item.iconKind && !item.favorite && item.hasDefaultAction === false &&
        /^\/[a-z][a-z0-9-]{0,31}$/.test(item.title) && activation.completionText === `${item.title} `) return true
  }
  if (host.platform === 'macos' && host.webSearch && item.iconKind === 'webSearch' &&
      activation.kind === 'completion' && activation.completionText === '/web-search ' && item.title === '/web-search' &&
      item.hasDefaultAction === false && item.favorite?.target.kind === 'builtin' && item.favorite.target.feature === 'webSearch') return true
  if (host.webSearch && !item.favorite && item.iconKind === 'webSearch' && activation.kind === 'executeResult') return true
  // Mac supports query values before the full reusable built-in command system.
  // Currency is an unavailable informational row until its service is enabled.
  if (host.platform === 'macos' && !item.favorite && (
    (['calculator', 'unit', 'date', 'timeZone'].includes(item.iconKind ?? '') &&
      (activation.kind === 'executeResult' || activation.kind === 'informational')) ||
    (item.iconKind === 'currency' && activation.kind === 'informational')
  )) return true
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
