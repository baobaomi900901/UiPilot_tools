import { describe, expect, it } from 'vitest'

import {
  parseLauncherShown,
  parseFindForwardPayload,
  parseFindDocumentsAccess,
  parseBuiltinFeatures,
  parseResultActionMetadata,
  parseResultActionOutcome,
  parsePluginTimerStartInput,
  parsePluginTimerState,
  parsePublicPluginInventory,
  parsePublicPluginPrepareSummary,
} from './protocol'

it('distinguishes an active saved grant, a session grant, and a bookmark that needs consent again', () => {
  for (const [status, persistence] of [
    ['notGranted', 'none'], ['granted', 'session'], ['granted', 'remembered'],
    ['needsReauthorization', 'none'], ['needsReauthorization', 'remembered'],
  ]) {
    const reply = { status, persistence, revision: '0' }
    expect(parseFindDocumentsAccess(reply)).toEqual(reply)
  }
  for (const invalid of [
    { status: 'granted', persistence: 'remembered', revision: '01' },
    { status: 'granted', persistence: 'none', revision: '0' },
    { status: 'notGranted', persistence: 'remembered', revision: '0' },
    { status: 'needsReauthorization', persistence: 'session', revision: '0' },
    { status: 'granted', persistence: 'remembered', revision: '1', path: '/private' },
    { status: 'granted', revision: '0' }, null,
  ]) expect(parseFindDocumentsAccess(invalid)).toBeNull()
})

describe('main result action protocol', () => {
  it('accepts only opaque display metadata with consistent explicit defaults', () => {
    const actions = [{ actionId: 'action-1', title: '复制' }]
    expect(parseResultActionMetadata({ actions, defaultActionId: 'action-1', hasDefaultAction: true })).toEqual({ actions, defaultActionId: 'action-1' })
    expect(parseResultActionMetadata({ actions, hasDefaultAction: false })).toEqual({ actions })
    for (const value of [
      { actions, hasDefaultAction: true },
      { actions, defaultActionId: 'missing', hasDefaultAction: true },
      { actions, defaultActionId: null, hasDefaultAction: false },
      { actions: [...actions, ...actions], hasDefaultAction: false },
      { actions: [{ ...actions[0], text: 'secret' }], hasDefaultAction: false },
      { defaultActionId: 'action-1', hasDefaultAction: true },
    ]) expect(parseResultActionMetadata(value)).toBeNull()
  })

  it('rejects unknown outcome fields/codes instead of guessing consumption', () => {
    expect(parseResultActionOutcome({ kind: 'copied' })).toEqual({ kind: 'copied' })
    expect(parseResultActionOutcome({ kind: 'urlOpened' })).toEqual({ kind: 'urlOpened' })
    expect(parseResultActionOutcome({ kind: 'pasteSent' })).toEqual({ kind: 'pasteSent' })
    expect(parseResultActionOutcome({ kind: 'pasteSent', text: 'private' })).toBeNull()
    for (const code of ['pasteTargetUnavailable', 'pasteInputBusy', 'pasteFailed']) {
      expect(parseResultActionOutcome({ kind: 'failed', code, sourceConsumed: false })).not.toBeNull()
    }
    expect(parseResultActionOutcome({ kind: 'failed', code: 'urlOpenFailed', sourceConsumed: true })).not.toBeNull()
    expect(parseResultActionOutcome({ kind: 'urlOpened', url: 'https://private.example' })).toBeNull()
    expect(parseResultActionOutcome({ kind: 'failed', code: 'actionBusy', sourceConsumed: false })).not.toBeNull()
    for (const value of [
      { kind: 'copied', text: 'secret' },
      { kind: 'failed', code: 'newError', sourceConsumed: false },
      { kind: 'failed', code: 'actionBusy' },
      { kind: 'failed', code: 'actionBusy', sourceConsumed: false, payload: null },
      { kind: 'resultsUpdated', response: { requestId: 'req-2', items: [], autoExecuteResultId: 'item-1' } },
    ]) expect(parseResultActionOutcome(value)).toBeNull()
  })

  it('validates the complete replacement before allowing an atomic install', () => {
    const response = { requestId: 'req-2', mainResultCommand: {
      commandId: 'transform', pluginId: 'com.uipilot.actions', commandLabel: 'actions', argument: 'hello',
    }, items: [{ resultId: 'item-2', title: 'Fresh', activation: { kind: 'executeResult' }, hasDefaultAction: false, actions: [] }] }
    expect(parseResultActionOutcome({ kind: 'resultsUpdated', response })).not.toBeNull()
    for (const invalid of [
      { ...response, mainResultCommand: { ...response.mainResultCommand, payload: null } },
      { ...response, mainResultCommand: {} },
      { ...response, items: [{ ...response.items[0], favorite: {} }] },
      { ...response, items: [{ ...response.items[0], activation: { kind: 'informational' } }] },
      { ...response, items: [{ ...response.items[0], subtitle: 42 }] },
      { ...response, items: [response.items[0], response.items[0]] },
    ]) expect(parseResultActionOutcome({ kind: 'resultsUpdated', response: invalid })).toBeNull()
  })
})

describe('public plugin timer protocol', () => {
  it('accepts only exact bounded timer start input', () => {
    expect(parsePluginTimerStartInput({ durationMs: 1_000, completionMessage: ' done ' })).toEqual({
      durationMs: 1_000,
      completionMessage: ' done ',
    })
    expect(parsePluginTimerStartInput({ durationMs: 86_400_000, completionMessage: 'x'.repeat(500) })).not.toBeNull()

    for (const invalid of [
      { durationMs: 999, completionMessage: 'done' },
      { durationMs: 86_400_001, completionMessage: 'done' },
      { durationMs: 1_000.5, completionMessage: 'done' },
      { durationMs: Number.MAX_SAFE_INTEGER + 1, completionMessage: 'done' },
      { durationMs: 1_000, completionMessage: '   ' },
      { durationMs: 1_000, completionMessage: 'line\nbreak' },
      { durationMs: 1_000, completionMessage: 'x'.repeat(501) },
      { durationMs: 1_000, completionMessage: 'done', extra: true },
    ]) {
      expect(parsePluginTimerStartInput(invalid)).toBeNull()
    }
  })

  it('parses exact timer states with canonical u64 revisions', () => {
    expect(parsePluginTimerState({
      timerRevision: '0', phase: 'idle', durationMs: null, remainingMs: null,
    })).toEqual({ timerRevision: '0', phase: 'idle', durationMs: null, remainingMs: null })
    expect(parsePluginTimerState({
      timerRevision: '10', phase: 'running', durationMs: 10_000, remainingMs: 9_000,
    })).not.toBeNull()
    expect(parsePluginTimerState({
      timerRevision: '18446744073709551615', phase: 'fired', durationMs: 10_000, remainingMs: 0,
    })).not.toBeNull()

    for (const invalid of [
      { timerRevision: '01', phase: 'idle', durationMs: null, remainingMs: null },
      { timerRevision: '1', phase: 'claiming', durationMs: 10_000, remainingMs: 0 },
      { timerRevision: '1', phase: 'running', durationMs: 10_000, remainingMs: null },
      { timerRevision: '1', phase: 'running', durationMs: 10_000, remainingMs: 10_001 },
      { timerRevision: '1', phase: 'fired', durationMs: 10_000, remainingMs: 1 },
      { timerRevision: '1', phase: 'idle', durationMs: null, remainingMs: null, extra: true },
    ]) {
      expect(parsePluginTimerState(invalid)).toBeNull()
    }
  })

  it('accepts timer.control in public plugin inventory permissions', () => {
    const inventory = {
      revision: '1', hotkeyRevision: '0',
      items: [{
        pluginId: 'com.example.timer',
        name: 'Timer',
        description: null,
        version: '1.0.0',
        source: 'localPackage', installationDirectory: 'C:/UiPilot/plugins/example',
        commands: [{ commandId: 'default', defaultName: 'timer', effectiveName: 'timer', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }],
        enabled: true,
        fault: null,
        generation: 1,
        iconUrl: null,
        network: null,
        permissions: [{ permission: 'timer.control', supported: true, granted: true }],
        settings: [],
      }],
    }
    expect(parsePublicPluginInventory(inventory)).toEqual(inventory)
    const services = ['credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek']
    const withCredentials = { ...inventory, items: [{ ...inventory.items[0], permissions: services.map((permission) => ({ permission, supported: true, granted: false })) }] }
    expect(parsePublicPluginInventory(withCredentials)).toEqual(withCredentials)
    expect(parsePublicPluginInventory({ ...withCredentials, items: [{ ...withCredentials.items[0], permissions: [{ permission: 'credentials.unknown', supported: true, granted: false }] }] })).toBeNull()
  })

  it('accepts ui.panel in public plugin inventory permissions', () => {
    const inventory = {
      revision: '1', hotkeyRevision: '0',
      items: [{
        pluginId: 'com.example.panel',
        name: 'Panel',
        description: null,
        version: '1.0.0',
        source: 'localPackage', installationDirectory: 'C:/UiPilot/plugins/example',
        commands: [{ commandId: 'default', defaultName: 'panel', effectiveName: 'panel', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }],
        enabled: true,
        fault: null,
        generation: 1,
        iconUrl: null,
        network: null,
        permissions: [{ permission: 'ui.panel', supported: true, granted: true }],
        settings: [],
      }],
    }
    expect(parsePublicPluginInventory(inventory)).toEqual(inventory)
  })

  it('strictly parses sorted exact HTTPS hosts in inventory and prepare summaries', () => {
    const item = {
      pluginId: 'com.example.network',
      name: 'Network',
      description: null,
      version: '1.0.0',
      source: 'localPackage', installationDirectory: 'C:/UiPilot/plugins/example',
      commands: [{ commandId: 'default', defaultName: 'network', effectiveName: 'network', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }],
      enabled: true,
      fault: null,
      generation: 1,
      iconUrl: null,
      network: { httpsHosts: ['api.example.com', 'auth.example.com'], credentials: [] },
      permissions: [{ permission: 'network.https', supported: true, granted: true }],
      settings: [],
    }
    expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [item] })).toEqual({
      revision: '1', hotkeyRevision: '0',
      items: [item],
    })
    for (const network of [
      { httpsHosts: ['auth.example.com', 'api.example.com'], credentials: [] },
      { httpsHosts: ['api.example.com', 'api.example.com'], credentials: [] },
      { httpsHosts: ['https://api.example.com'], credentials: [] },
      { httpsHosts: ['127.0.0.1'], credentials: [] },
      { httpsHosts: ['xn--bcher-kva.example'], credentials: [] },
      { httpsHosts: ['api.example.com'], credentials: [], extra: true },
      { httpsHosts: [], credentials: [] },
    ]) {
      expect(parsePublicPluginInventory({ revision: '1', hotkeyRevision: '0', items: [{ ...item, network }] })).toBeNull()
    }

    const prepared = {
      token: 'public-prepare-0000000000000001-0000000000000002',
      pluginId: 'com.example.network',
      name: 'Network',
      version: '1.1.0',
      permissions: ['network.https'],
      isUpdate: true, operation: 'replace', previousVersion: '1.0.0',
      sourceVerified: false,
      iconUrl: null,
      network: {
        httpsHosts: ['api.example.com', 'auth.example.com'], credentials: [],
        addedHttpsHosts: ['auth.example.com'], addedCredentialIds: [],
        requiresNetworkConsent: true,
      },
    }
    expect(parsePublicPluginPrepareSummary(prepared)).toEqual(prepared)
    const credential = { id: 'service', settingKey: 'token', host: 'api.example.com', type: 'bearer' }
    const credentialNetwork = { ...prepared.network, addedHttpsHosts: [], credentials: [credential], addedCredentialIds: ['service'] }
    expect(parsePublicPluginPrepareSummary({ ...prepared, network: credentialNetwork })).not.toBeNull()
    for (const credentials of [null, 'bad', [credential, credential], [{ ...credential, type: 'oauth' }],
      [{ ...credential, host: 'other.example.com' }], [{ ...credential, secret: 'forbidden' }], [{ ...credential, settingKey: '' }]]) {
      expect(parsePublicPluginPrepareSummary({ ...prepared, network: { ...credentialNetwork, credentials } })).toBeNull()
    }
    expect(parsePublicPluginPrepareSummary({ ...prepared, network: { ...credentialNetwork, addedCredentialIds: ['unknown'] } })).toBeNull()
    expect(parsePublicPluginPrepareSummary({ ...prepared, isUpdate: false, operation: 'install', previousVersion: null, network: { ...credentialNetwork,
      addedHttpsHosts: credentialNetwork.httpsHosts, addedCredentialIds: [] } })).toBeNull()

    expect(parsePublicPluginPrepareSummary({ ...prepared, permissions: [], network: null })).not.toBeNull()
    expect(parsePublicPluginPrepareSummary({
      ...prepared,
      isUpdate: false, operation: 'install', previousVersion: null,
      network: { ...prepared.network, addedHttpsHosts: ['auth.example.com'], addedCredentialIds: [] },
    })).toBeNull()
    for (const network of [
      { ...prepared.network, addedHttpsHosts: ['other.example.com'], addedCredentialIds: [] },
      { ...prepared.network, addedHttpsHosts: [], addedCredentialIds: [], requiresNetworkConsent: true },
      { ...prepared.network, requiresNetworkConsent: false },
      { ...prepared.network, httpsHosts: ['auth.example.com', 'api.example.com'], credentials: [] },
      { ...prepared.network, extra: true },
    ]) {
      expect(parsePublicPluginPrepareSummary({ ...prepared, network })).toBeNull()
    }
  })
})


it('parses the full command inventory with constructor and rejects malformed command identities', () => {
  const command = { commandId: 'constructor', defaultName: 'lower', effectiveName: 'lower', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' as const }
  const item = { pluginId: 'com.example.multi', name: 'Multi', description: null, version: '1.0.0', source: 'localPackage', installationDirectory: 'C:/UiPilot/plugins/example', commands: [command, { ...command, commandId: 'upper', defaultName: 'upper', effectiveName: 'custom', nameOverride: 'custom', favorite: true, hotkey: null, hotkeyStatus: 'unset' as const }], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [] }
  const inventory = (commands: unknown) => ({ revision: '1', hotkeyRevision: '0', items: [{ ...item, commands }] })
  expect(parsePublicPluginInventory(inventory(item.commands))).toEqual(inventory(item.commands))
  for (const commandId of [undefined, null, '', 'Upper', 'bad id']) {
    const { commandId: _id, ...rest } = command
    expect(parsePublicPluginInventory(inventory([{ ...rest, ...(commandId === undefined ? {} : { commandId }) }]))).toBeNull()
  }
  for (const commands of [[], [command, command], [{ ...command, extra: true }], [{ ...command, effectiveName: 'forged' }], Array(17).fill(command)]) {
    expect(parsePublicPluginInventory(inventory(commands))).toBeNull()
  }
  expect(parsePublicPluginInventory(inventory([{ ...command, nameOverride: 'lower' }]))).toEqual(inventory([{ ...command, nameOverride: 'lower' }]))
})


describe('command hotkey protocol', () => {
  it('accepts builtin entry only on launcher and never alongside a public command', () => {
    const event = { invocationId: 'invocation-3', target: 'launcher', notice: null, builtinEntry: 'webSearch' }
    expect(parseLauncherShown(event)).toEqual(event)
    expect(parseLauncherShown({ ...event, target: 'settings' })).toBeNull()
    expect(parseLauncherShown({ ...event, builtinEntry: 'find' })).toBeNull()
    expect(parseLauncherShown({ ...event, commandEntry: { pluginId: 'com.example.notes', commandId: 'default', hotkey: 'Ctrl+N', bindingRevision: '1' } })).toBeNull()
  })
  it('preserves an exact stable entry on the same shown invocation', () => {
    const commandEntry = { pluginId: 'com.example.notes', commandId: 'default', hotkey: 'Ctrl+N', bindingRevision: '1' }
    const shown = { invocationId: 'invocation-2', target: 'launcher', notice: null, commandEntry }
    expect(parseLauncherShown(shown)).toEqual(shown)
    for (const entry of [null, { ...commandEntry, bindingRevision: '01' }, { ...commandEntry, commandId: '' }, { ...commandEntry, extra: true }, { ...commandEntry, hotkey: 'DoubleCtrl' }]) {
      expect(parseLauncherShown({ ...shown, commandEntry: entry })).toBeNull()
    }
    expect(parseLauncherShown({ ...shown, target: 'settings' })).toBeNull()
  })
})

describe('managed builtins protocol', () => {
  const items = ['screenshot', 'quicklinks', 'webSearch'].map((feature) => ({ feature, name: feature, command: '/test', defaultCommand: '/test', favorite: false, hotkey: null, hotkeyStatus: 'unset', directory: 'C:/UiPilot', description: 'Description', examples: ['/test'], version: '1.0.0', enabled: true }))
  it('accepts the complete fixed ordered host metadata', () => {
    expect(parseBuiltinFeatures(items)).toEqual(items)
  })
  it('rejects unmanaged identities, duplicates, wrong fields and malformed metadata', () => {
    for (const value of [[], items.slice(1), [...items].reverse(), [items[0], items[0], items[2]], items.map((item, i) => i ? item : { ...item, feature: 'find' }), items.map((item, i) => i ? item : { ...item, pluginId: 'fake' }), items.map((item, i) => i ? item : { ...item, enabled: 1 }), items.map((item, i) => i ? item : { ...item, examples: [1] })]) expect(parseBuiltinFeatures(value)).toBeNull()
  })
})


describe('plugin detail configuration protocol', () => {
  const builtins = ['screenshot', 'quicklinks', 'webSearch'].map((feature) => ({
    feature, name: feature, command: '/shared', defaultCommand: '/default', favorite: true,
    hotkey: 'Ctrl+Alt+P', hotkeyStatus: 'active', directory: 'C:/UiPilot',
    description: 'Description', examples: ['/shared'], version: '1.0.0', enabled: true,
  }))
  it('accepts duplicate builtin labels while keeping distinct identities and shortcut state', () => {
    expect(parseBuiltinFeatures(builtins)).toEqual(builtins)
  })
  it('rejects malformed shortcut states, commands, directories and missing detail fields', () => {
    for (const override of [
      { hotkey: null }, { hotkeyStatus: 'unknown' }, { hotkeyStatus: 'unset' },
      { hotkey: 'DoubleCtrl' }, { favorite: 1 }, { directory: 1 },
      { command: 'missing-slash' }, { defaultCommand: '/invalid command' },
    ]) expect(parseBuiltinFeatures(builtins.map((item, i) => i ? item : { ...item, ...override }))).toBeNull()
    const { defaultCommand: _, ...missing } = builtins[0]!
    expect(parseBuiltinFeatures([missing, ...builtins.slice(1)])).toBeNull()
  })
  it('accepts shared public names but rejects duplicate stable command ids and invalid directories', () => {
    const command = { commandId: 'one', defaultName: 'shared', effectiveName: 'shared', nameOverride: null, favorite: false, hotkey: null, hotkeyStatus: 'unset' }
    const item = { pluginId: 'com.example.multi', name: 'Multi', description: null, version: '1.0.0', source: 'localPackage', installationDirectory: 'C:/UiPilot/plugins/example', commands: [command, { ...command, commandId: 'two' }], enabled: true, fault: null, generation: 1, iconUrl: null, network: null, permissions: [], settings: [] }
    const inventory = { revision: '1', hotkeyRevision: '0', items: [item] }
    expect(parsePublicPluginInventory(inventory)).toEqual(inventory)
    expect(parsePublicPluginInventory({ ...inventory, items: [{ ...item, commands: [command, command] }] })).toBeNull()
    expect(parsePublicPluginInventory({ ...inventory, items: [{ ...item, installationDirectory: 42 }] })).toBeNull()
  })
})


it('limits Find session preservation to an empty Mac focus-only forward', () => {
  const payload = {invocationId:'find-1', forwardSequence:'2', query:'', pinned:true, preserveSession:true}
  expect(parseFindForwardPayload(payload, true)).toEqual(payload)
  expect(parseFindForwardPayload(payload)).toBeNull()
  expect(parseFindForwardPayload({...payload, query:'new'}, true)).toBeNull()
  expect(parseFindForwardPayload({...payload, preserveSession:'true'}, true)).toBeNull()
})
