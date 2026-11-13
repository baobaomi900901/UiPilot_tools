import { expect, it, vi } from 'vitest'
import { createMacPluginManagementClient, parseMacPluginSnapshot, parseMacPluginPreview } from './macos-plugin-management-client'
import { parseHostCapabilities, previewCapabilities, settingsTabAvailable, resultAvailable } from './host-capabilities'
const plugin = { pluginId: 'com.test.plugin', name: 'Test', description: null, version: '1.0.0', enabled: false, commands: [{ id: 'default', name: 'test', defaultName: 'test', nameOverride: null, favorite: false }], permissions: [{ permission: 'clipboard.write', granted: false }], settings: [{ type: 'text', key: 'greeting', label: 'Text', default: 'Hello' }], values: { greeting: '你好' } }
const snapshot = { revision: '9007199254740993', plugins: [plugin] }
it('keeps management independent from execution and privilege settings', () => {
  const host = { ...previewCapabilities, platform: 'macos' as const, plugins: false, pluginManagement: true }
  expect(settingsTabAvailable(host, 'plugins')).toBe(true)
  expect(settingsTabAvailable(host, 'permissions')).toBe(false)
  expect(resultAvailable(host, { kind: 'windowActivation' } as never, {} as never)).toBe(false)
  expect(parseHostCapabilities({ ...host, pluginManagement: 'yes' })).toBeNull()
  expect(parseHostCapabilities(previewCapabilities)).not.toBeNull()
})
it('validates bounded management replies without coercing revisions or exposing secrets', () => {
  expect(parseMacPluginSnapshot(snapshot)).toEqual(snapshot)
  for (const bad of [ { ...snapshot, revision: 1 }, { ...snapshot, revision: '18446744073709551616' }, { ...snapshot, plugins: [plugin, plugin] }, { ...snapshot, extra: true }, { ...snapshot, plugins: [{ ...plugin, values: { secret: 'hidden' } }] }, { ...snapshot, plugins: [{ ...plugin, settings: [{ type: 'secret', key: 'token', label: 'Token' }], values: {} }] } ]) expect(parseMacPluginSnapshot(bad)).toBeNull()
  expect(parseMacPluginPreview({ token: 'opaque', expectedRevision: snapshot.revision, plugin, requiredPermissions: ['clipboard.write'] })).not.toBeNull()
  expect(parseMacPluginPreview({ token: 'opaque', expectedRevision: '1', plugin, requiredPermissions: ['network.https'] })).toBeNull()
})
it('serializes mutations and new page reads even after a failed write, never sends a file path', async () => {
  let reject!: (e: unknown) => void
  const write = new Promise<unknown>((_, fail) => { reject = fail })
  const invoke = vi.fn(async (command: string) => command === 'set_macos_plugin_enabled' ? write : snapshot)
  const client = createMacPluginManagementClient(invoke)
  const mutation = client.enabled({ pluginId: plugin.pluginId, enabled: true, expectedRevision: snapshot.revision })
  const read = client.list()
  await Promise.resolve()
  expect(invoke).toHaveBeenCalledTimes(1)
  reject('pluginManagementCommitUncertain')
  await expect(mutation).rejects.toBe('pluginManagementCommitUncertain')
  expect(await read).toEqual(snapshot)
  expect(invoke.mock.calls.map(c => c[0])).toEqual(['set_macos_plugin_enabled', 'list_macos_plugins'])
  const chooser = vi.fn(async () => null)
  await createMacPluginManagementClient(chooser).prepare({ kind: 'archive', expectedRevision: '1' })
  expect(chooser).toHaveBeenCalledWith('prepare_macos_plugin_install', { input: { kind: 'archive', expectedRevision: '1' } })
})

it('rejects mismatched command names and untyped favorites, and sends stable ID preference transactions', async () => {
  for (const c of [
    { ...plugin.commands[0], name: 'different' },
    { ...plugin.commands[0], favorite: 'yes' },
    { ...plugin.commands[0], nameOverride: 7 },
  ]) expect(parseMacPluginSnapshot({ ...snapshot, plugins: [{ ...plugin, commands: [c] }] })).toBeNull()
  const invoke = vi.fn(async () => snapshot)
  const input = { pluginId: plugin.pluginId, commandId: 'default', nameOverride: 'renamed', favorite: true, expectedRevision: snapshot.revision }
  expect(await createMacPluginManagementClient(invoke).command(input)).toEqual(snapshot)
  expect(invoke).toHaveBeenCalledWith('save_macos_plugin_command', { input })
})
