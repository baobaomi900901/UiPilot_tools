// In-memory browser preview only. No native IPC, installation or plugin execution.
import type { MacPluginManagementClient, MacPluginRow, MacPluginSnapshot } from './macos-plugin-management-client'
export function createPreviewMacPluginManagementClient(): MacPluginManagementClient {
  let revision = 1
  let plugins: MacPluginRow[] = [{ pluginId: 'com.uipilot.demo-settings', name: '普通设置示例', description: '读取普通设置并显示主结果。', version: '1.0.0', enabled: true,
    commands: [{ id: 'default', name: 'settings-demo', defaultName: 'settings-demo', nameOverride: null, favorite: false }], permissions: [{ permission: 'clipboard.write', granted: false }],
    settings: [{ type: 'text', key: 'greeting', label: '问候文本', default: '你好' }, { type: 'number', key: 'count', label: '数量', min: 1, max: 5, default: 2 }, { type: 'boolean', key: 'uppercase', label: '转为大写', default: false }, { type: 'select', key: 'style', label: '显示样式', options: [{ label: '普通', value: 'plain' }, { label: '方框', value: 'boxed' }], default: 'plain' }], values: { greeting: '你好', count: 2, uppercase: false, style: 'plain' } }]
  const snapshot = (): MacPluginSnapshot => structuredClone({ revision: String(revision), plugins })
  function change(expected: string, update: () => void) { if (expected !== String(revision)) throw 'pluginManagementStale'; update(); revision++; return snapshot() }
  return {
    list: async () => snapshot(),
    prepare: async input => ({ token: 'browser-preview-only', expectedRevision: input.expectedRevision, plugin: plugins[0], requiredPermissions: [] }),
    cancel: async () => {}, commit: async input => change(input.expectedRevision, () => {}),
    enabled: async input => change(input.expectedRevision, () => { plugins = plugins.map(p => p.pluginId === input.pluginId ? { ...p, enabled: input.enabled } : p) }),
    permission: async input => change(input.expectedRevision, () => { plugins = plugins.map(p => p.pluginId === input.pluginId ? { ...p, permissions: [{ permission: input.permission, granted: input.granted }] } : p) }),
    settings: async input => change(input.expectedRevision, () => { plugins = plugins.map(p => p.pluginId === input.pluginId ? { ...p, values: { ...p.values, ...input.updates } } : p) }),
    command: async input => change(input.expectedRevision, () => { plugins = plugins.map(p => p.pluginId === input.pluginId ? { ...p, commands: p.commands.map(c => c.id === input.commandId ? { ...c, nameOverride: input.nameOverride, name: input.nameOverride ?? c.defaultName, favorite: input.favorite } : c) } : p) }),
    remove: async input => change(input.expectedRevision, () => { plugins = plugins.filter(p => p.pluginId !== input.pluginId) }),
  }
}
