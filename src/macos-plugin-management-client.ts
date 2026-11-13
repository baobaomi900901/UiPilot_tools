import { parsePublicSettingView, parseU64Decimal } from './protocol'
import type { PublicSettingDefinition } from './protocol'

export type OrdinarySetting = Exclude<PublicSettingDefinition, { type: 'secret' }>
export type SettingValues = Readonly<Record<string, string | number | boolean>>
export interface MacPluginRow {
  pluginId: string; name: string; description: string | null; version: string; enabled: boolean
  commands: readonly { id: string; name: string; defaultName: string; nameOverride: string | null; favorite: boolean }[]
  permissions: readonly { permission: 'clipboard.write'; granted: boolean }[]
  settings: readonly OrdinarySetting[]; values: SettingValues
}
export interface MacPluginSnapshot { revision: string; plugins: readonly MacPluginRow[] }
export interface MacPluginPreview { token: string; expectedRevision: string; plugin: MacPluginRow; requiredPermissions: readonly 'clipboard.write'[] }
export interface MacPluginManagementClient {
  list(): Promise<MacPluginSnapshot>
  prepare(input: { kind: 'archive' | 'directory'; expectedRevision: string }): Promise<MacPluginPreview | null>
  commit(input: { token: string; expectedRevision: string; approvedPermissions: readonly 'clipboard.write'[] }): Promise<MacPluginSnapshot>
  cancel(input: { token: string }): Promise<void>
  enabled(input: { pluginId: string; enabled: boolean; expectedRevision: string }): Promise<MacPluginSnapshot>
  permission(input: { pluginId: string; permission: 'clipboard.write'; granted: boolean; expectedRevision: string }): Promise<MacPluginSnapshot>
  settings(input: { pluginId: string; updates: SettingValues; expectedRevision: string }): Promise<MacPluginSnapshot>
  command(input: { pluginId: string; commandId: string; nameOverride: string | null; favorite: boolean; expectedRevision: string }): Promise<MacPluginSnapshot>
  remove(input: { pluginId: string; expectedRevision: string }): Promise<MacPluginSnapshot>
}
function record(v: unknown): v is Record<string, unknown> { return typeof v === 'object' && v !== null && !Array.isArray(v) }
function exact(v: Record<string, unknown>, keys: readonly string[]) { return Object.keys(v).length === keys.length && keys.every(k => Object.prototype.hasOwnProperty.call(v, k)) }
function text(v: unknown): v is string { return typeof v === 'string' && v.length > 0 && v.length <= 4096 }
function row(v: unknown): MacPluginRow | null {
  if (!record(v) || !exact(v, ['pluginId', 'name', 'description', 'version', 'enabled', 'commands', 'permissions', 'settings', 'values']) ||
      !text(v.pluginId) || !text(v.name) || !text(v.version) || !(v.description === null || typeof v.description === 'string') || typeof v.enabled !== 'boolean' ||
      !Array.isArray(v.commands) || v.commands.length < 1 || v.commands.length > 16 || !Array.isArray(v.permissions) || v.permissions.length > 1 ||
      !Array.isArray(v.settings) || v.settings.length > 128 || !record(v.values)) return null
  const commands = new Set<string>()
  for (const c of v.commands) {
    if (!record(c) || !exact(c, ['id', 'name', 'defaultName', 'nameOverride', 'favorite']) || !text(c.id) || !text(c.name) || !text(c.defaultName) || !(c.nameOverride === null || text(c.nameOverride)) || c.name !== (c.nameOverride ?? c.defaultName) || typeof c.favorite !== 'boolean' || commands.has(c.id)) return null
    commands.add(c.id)
  }
  for (const p of v.permissions) if (!record(p) || !exact(p, ['permission', 'granted']) || p.permission !== 'clipboard.write' || typeof p.granted !== 'boolean') return null
  const keys = new Set<string>()
  for (const d of v.settings) {
    if (!record(d) || typeof d.key !== 'string' || keys.has(d.key) || d.type === 'secret' || !parsePublicSettingView({ definition: d, ...(Object.prototype.hasOwnProperty.call(v.values, d.key) ? { value: v.values[d.key] } : {}) })) return null
    keys.add(d.key)
  }
  if (Object.keys(v.values).some(k => !keys.has(k))) return null
  return v as unknown as MacPluginRow
}
export function parseMacPluginSnapshot(v: unknown): MacPluginSnapshot | null {
  if (!record(v) || !exact(v, ['revision', 'plugins']) || parseU64Decimal(v.revision) === null || !Array.isArray(v.plugins)) return null
  const ids = new Set<string>(); const plugins: MacPluginRow[] = []
  for (const item of v.plugins) { const p = row(item); if (!p || ids.has(p.pluginId)) return null; ids.add(p.pluginId); plugins.push(p) }
  return { revision: v.revision as string, plugins }
}
export function parseMacPluginPreview(v: unknown): MacPluginPreview | null {
  if (!record(v) || !exact(v, ['token', 'expectedRevision', 'plugin', 'requiredPermissions']) || !text(v.token) || parseU64Decimal(v.expectedRevision) === null ||
      !Array.isArray(v.requiredPermissions) || v.requiredPermissions.length > 1 || v.requiredPermissions.some(p => p !== 'clipboard.write')) return null
  const plugin = row(v.plugin)
  if (!plugin || v.requiredPermissions.some(p => !plugin.permissions.some(allowed => allowed.permission === p))) return null
  return { token: v.token, expectedRevision: v.expectedRevision as string, plugin, requiredPermissions: v.requiredPermissions as 'clipboard.write'[] }
}
export function createMacPluginManagementClient(invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>): MacPluginManagementClient {
  // Shared by mounted pages. A new inventory read waits for an earlier write,
  // including a write whose initiating page has already unmounted.
  let tail: Promise<unknown> = Promise.resolve()
  function call<T>(command: string, input: unknown, parse: (value: unknown) => T): Promise<T> {
    const next = tail.then(() => invoke(command, input === undefined ? undefined : { input })).then(parse)
    tail = next.catch(() => undefined)
    return next
  }
  const snapshot = (v: unknown) => { const parsed = parseMacPluginSnapshot(v); if (!parsed) throw 'pluginManagementUnavailable'; return parsed }
  return {
    list: () => call('list_macos_plugins', undefined, snapshot),
    prepare: input => call('prepare_macos_plugin_install', input, v => { if (v === null) return null; const parsed = parseMacPluginPreview(v); if (!parsed) throw 'pluginManagementUnavailable'; return parsed }),
    commit: input => call('commit_macos_plugin_install', input, snapshot),
    cancel: input => call('cancel_macos_plugin_install', input, () => undefined),
    enabled: input => call('set_macos_plugin_enabled', input, snapshot),
    permission: input => call('set_macos_plugin_permission', input, snapshot),
    settings: input => call('save_macos_plugin_settings', input, snapshot),
    command: input => call('save_macos_plugin_command', input, snapshot),
    remove: input => call('remove_macos_plugin', input, snapshot),
  }
}
