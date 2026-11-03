import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

import { normalizedCommands, validateManifest } from '../src/manifest.js'

import { multiCommandManifest } from './multi-command-fixture.js'

function timerManifest(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    pluginId: 'com.example.timer',
    version: '1.0.0',
    apiVersion: 1,
    minimumHostVersion: '0.2.0',
    name: 'Timer',
    supportedPlatforms: ['windows'],
    command: {
      defaultName: 'timer',
      activationMode: 'submit',
      outputMode: 'window',
      inputRequired: false,
    },
    runtime: { entry: 'dist/runtime.js' },
    window: { entry: 'dist/window.html' },
    permissions: ['ui.window', 'notifications.publish', 'timer.control'],
    settings: [],
  }
}

function validate(value: unknown, platform: 'windows' | 'macos' = 'windows') {
  return validateManifest(Buffer.from(JSON.stringify(value)), platform)
}

describe('validateManifest', () => {
  it('accepts the current pomodoro manifest on Windows', async () => {
    const bytes = await readFile(
      resolve('../../examples/public-plugins/com.uipilot.pomodoro/package/plugin.json'),
    )
    const result = validateManifest(bytes, 'windows')
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.manifest.permissions).toContain('timer.control')
  })

  it.each([
    ['live activation', (value: any) => (value.command.activationMode = 'live')],
    ['main result', (value: any) => (value.command.outputMode = 'mainResult')],
    ['missing window', (value: any) => delete value.window],
    ['missing ui.window', (value: any) => (value.permissions = value.permissions.filter((x: string) => x !== 'ui.window'))],
    ['missing notifications.publish', (value: any) => (value.permissions = value.permissions.filter((x: string) => x !== 'notifications.publish'))],
  ])('rejects timer.control with %s', (_name, mutate) => {
    const value = timerManifest()
    mutate(value)
    expect(validate(value)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
  })

  it('accepts a legal panel package and rejects illegal panel combinations', () => {
    const value = {
      schemaVersion: 1,
      pluginId: 'com.example.panel',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.3.0',
      name: 'Panel',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'panel',
        activationMode: 'submit',
        outputMode: 'panel',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      panel: { entry: 'dist/panel.html' },
      permissions: ['ui.panel'],
      settings: [],
    }
    expect(validate(value)).toMatchObject({ ok: true })

    const missingPermission = structuredClone(value)
    missingPermission.permissions = []
    expect(validate(missingPermission)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const missingEntry = structuredClone(value)
    delete (missingEntry as { panel?: unknown }).panel
    expect(validate(missingEntry)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const withTimer = structuredClone(value)
    withTimer.permissions = ['ui.panel', 'notifications.publish', 'timer.control']
    expect(validate(withTimer)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const tooNewHost = structuredClone(value)
    tooNewHost.minimumHostVersion = '0.16.1'
    expect(validate(tooNewHost)).toMatchObject({
      ok: false,
      issues: [{ code: 'API_INCOMPATIBLE' }],
    })

    const lowHostPanel = structuredClone(value)
    lowHostPanel.minimumHostVersion = '0.2.0'
    expect(validate(lowHostPanel)).toMatchObject({
      ok: false,
      issues: [{ code: 'API_INCOMPATIBLE' }],
    })
  })

  it('validates clipboard history permissions only for Windows panel plugins', () => {
    const value = {
      schemaVersion: 1,
      pluginId: 'com.example.clipboard-history',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.3.3',
      name: 'Clipboard History',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'cliphist',
        activationMode: 'submit',
        outputMode: 'panel',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      panel: { entry: 'dist/panel.html' },
      permissions: ['ui.panel', 'clipboard.history.read', 'clipboard.history.paste'],
      settings: [],
    }
    expect(validate(value)).toMatchObject({ ok: true })

    const pasteWithoutRead = structuredClone(value)
    pasteWithoutRead.permissions = ['ui.panel', 'clipboard.history.paste']
    expect(validate(pasteWithoutRead)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const mainResult = structuredClone(value) as any
    mainResult.command.outputMode = 'mainResult'
    delete mainResult.panel
    mainResult.permissions = ['clipboard.history.read']
    expect(validate(mainResult)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }],
    })

    const reservedClipboardRead = structuredClone(value)
    reservedClipboardRead.permissions = ['ui.panel', 'clipboard.read']
    expect(validate(reservedClipboardRead)).toMatchObject({
      ok: false,
      issues: [{ code: 'PERMISSION_UNSUPPORTED' }],
    })

    expect(validate(value, 'macos')).toMatchObject({
      ok: false,
      issues: [
        { code: 'PLATFORM_INCOMPATIBLE' },
        { code: 'PERMISSION_UNSUPPORTED' },
      ],
    })

    const lowHost = structuredClone(value)
    lowHost.minimumHostVersion = '0.3.2'
    expect(validate(lowHost)).toMatchObject({
      ok: false,
      issues: [{ code: 'API_INCOMPATIBLE' }],
    })
  })

  it('validates and canonicalizes panel hostKeys with the 0.3.3 extended key version gate', () => {
    const value = {
      schemaVersion: 1,
      pluginId: 'com.example.panel-keys',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.3.3',
      name: 'Panel Keys',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'panel-keys',
        activationMode: 'submit',
        outputMode: 'panel',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      panel: { entry: 'dist/panel.html', hostKeys: ['Enter', 'Shift+Tab', 'Tab', 'Primary+N', 'ArrowUp', 'ArrowDown'] },
      permissions: ['ui.panel'],
      settings: [],
    }
    const result = validate(value)
    expect(result).toMatchObject({ ok: true })
    if (result.ok) {
      expect(result.manifest.panel?.hostKeys).toEqual(['ArrowDown', 'ArrowUp', 'Primary+N', 'Tab', 'Shift+Tab', 'Enter'])
    }

    for (const hostKeys of [
      'ArrowDown',
      ['Space'],
      ['ArrowDown', 'ArrowDown'],
      ['ArrowDown', 'ArrowUp', 'Primary+N', 'ArrowDown', 'ArrowUp', 'Primary+N', 'ArrowDown', 'ArrowUp', 'Primary+N'],
    ]) {
      const candidate = structuredClone(value) as any
      candidate.panel.hostKeys = hostKeys
      expect(validate(candidate)).toMatchObject({ ok: false })
    }

    const lowHost = structuredClone(value)
    lowHost.minimumHostVersion = '0.3.2'
    expect(validate(lowHost)).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })

    const existingKeys = structuredClone(value)
    existingKeys.minimumHostVersion = '0.3.1'
    existingKeys.panel.hostKeys = ['Primary+N', 'ArrowDown']
    expect(validate(existingKeys)).toMatchObject({ ok: true })
  })

  it('validates optional panel hostKeyFocus with the 0.3.4 version gate', () => {
    const value = {
      schemaVersion: 1,
      pluginId: 'com.example.panel-host-focus',
      version: '1.0.0',
      apiVersion: 1,
      minimumHostVersion: '0.3.4',
      name: 'Panel Host Focus',
      supportedPlatforms: ['windows'],
      command: {
        defaultName: 'panel-host-focus',
        activationMode: 'submit',
        outputMode: 'panel',
        inputRequired: false,
      },
      runtime: { entry: 'dist/runtime.js' },
      panel: {
        entry: 'dist/panel.html',
        hostKeys: ['Tab'],
        hostKeyFocus: 'host',
      },
      permissions: ['ui.panel'],
      settings: [],
    }

    expect(validate(value)).toMatchObject({
      ok: true,
      manifest: { panel: { hostKeyFocus: 'host' } },
    })

    const explicitContent = structuredClone(value)
    explicitContent.panel.hostKeyFocus = 'content'
    expect(validate(explicitContent)).toMatchObject({
      ok: true,
      manifest: { panel: { hostKeyFocus: 'content' } },
    })

    const omitted = structuredClone(value) as any
    omitted.minimumHostVersion = '0.3.3'
    delete omitted.panel.hostKeyFocus
    const omittedResult = validate(omitted)
    expect(omittedResult).toMatchObject({ ok: true })
    if (omittedResult.ok) expect(omittedResult.manifest.panel).not.toHaveProperty('hostKeyFocus')

    const lowHost = structuredClone(value)
    lowHost.minimumHostVersion = '0.3.3'
    expect(validate(lowHost)).toMatchObject({
      ok: false,
      issues: [{ code: 'API_INCOMPATIBLE' }],
    })

    for (const invalid of ['main', false, null]) {
      const candidate = structuredClone(value) as any
      candidate.panel.hostKeyFocus = invalid
      const result = validate(candidate)
      expect(result).toMatchObject({ ok: false })
      if (!result.ok) expect(result.issues[0]?.code).toBe('MANIFEST_SCHEMA_INVALID')
    }
  })

  it('still accepts older non-panel packages that declare minimumHostVersion 0.2.0', () => {
    const value = timerManifest()
    expect(validate(value)).toMatchObject({ ok: true })
  })

  it('rejects timer.control for macOS even when the manifest lists both platforms', () => {
    const value = timerManifest()
    value.supportedPlatforms = ['windows', 'macos']
    expect(validate(value, 'macos')).toMatchObject({
      ok: false,
      issues: [{ code: 'PERMISSION_UNSUPPORTED' }],
    })
  })

  it('distinguishes JSON, Schema, semantic, platform, API, and permission failures', () => {
    expect(validateManifest(Buffer.from('{"x":1,"x":2}'), 'windows')).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_JSON_INVALID' }],
    })

    const schema = timerManifest()
    ;(schema as any).unknown = true
    expect(validate(schema)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SCHEMA_INVALID' }] })

    const semantic = timerManifest()
    ;(semantic as any).pluginId = 'Invalid ID'
    expect(validate(semantic)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })

    const platform = timerManifest()
    const platformResult = validate(platform, 'macos')
    expect(platformResult.ok).toBe(false)
    if (!platformResult.ok) {
      expect(platformResult.issues.map((issue) => issue.code)).toEqual([
        'PLATFORM_INCOMPATIBLE',
        'PERMISSION_UNSUPPORTED',
      ])
    }

    const api = timerManifest()
    ;(api as any).apiVersion = 2
    expect(validate(api)).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })

    const permission = timerManifest()
    ;(permission as any).permissions = ['ui.window', 'network.https']
    expect(validate(permission)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
  })

  it('validates numeric and select setting boundaries', () => {
    const number = timerManifest()
    ;(number as any).settings = [{ type: 'number', key: 'limit', label: 'Limit', min: 10, max: 5 }]
    expect(validate(number)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })

    const select = timerManifest()
    ;(select as any).settings = [{ type: 'select', key: 'mode', label: 'Mode', options: [] }]
    expect(validate(select)).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
  })

  it('validates the strict network contract and canonical host order', () => {
    const value = timerManifest() as any
    value.minimumHostVersion = '0.3.2'
    value.permissions = ['network.https']
    value.command = { defaultName: 'network', activationMode: 'live', outputMode: 'mainResult', inputRequired: false }
    delete value.window
    value.network = {
      httpsHosts: [
        'h.example.com', 'g.example.com', 'f.example.com', 'e.example.com',
        'd.example.com', 'c.example.com', 'b.example.com', 'a.example.com',
      ],
    }
    const result = validate(value)
    expect(result).toMatchObject({ ok: true })
    if (result.ok) {
      expect(result.manifest.network?.httpsHosts).toEqual([
        'a.example.com', 'b.example.com', 'c.example.com', 'd.example.com',
        'e.example.com', 'f.example.com', 'g.example.com', 'h.example.com',
      ])
    }

    for (const mutate of [
      (candidate: any) => { candidate.permissions = [] },
      (candidate: any) => { delete candidate.network },
      (candidate: any) => { candidate.minimumHostVersion = '0.3.1' },
      (candidate: any) => { candidate.network = null },
      (candidate: any) => { candidate.network = { httpsHosts: 'api.example.com' } },
      (candidate: any) => { candidate.network = { httpsHosts: [] } },
      (candidate: any) => { candidate.network = { httpsHosts: ['api.example.com', 'api.example.com'] } },
      (candidate: any) => { candidate.network = { httpsHosts: [
        'a.example.com', 'b.example.com', 'c.example.com', 'd.example.com', 'e.example.com',
        'f.example.com', 'g.example.com', 'h.example.com', 'i.example.com',
      ] } },
      (candidate: any) => { candidate.network.unknown = true },
    ]) {
      const candidate = structuredClone(value)
      mutate(candidate)
      expect(validate(candidate)).toMatchObject({ ok: false })
    }

    const macos = structuredClone(value)
    macos.supportedPlatforms = ['windows', 'macos']
    expect(validate(macos, 'macos')).toMatchObject({
      ok: false,
      issues: [{ code: 'PERMISSION_UNSUPPORTED' }],
    })

    const explicitNull = timerManifest() as any
    explicitNull.permissions = []
    explicitNull.command = { defaultName: 'network', activationMode: 'live', outputMode: 'mainResult', inputRequired: false }
    delete explicitNull.window
    explicitNull.network = null
    expect(validate(explicitNull)).toMatchObject({
      ok: false,
      issues: [{ code: 'MANIFEST_SCHEMA_INVALID' }],
    })
  })

  it('matches the shared network hostname policy fixtures', async () => {
    const fixtures = JSON.parse(
      await readFile(resolve('../../docs/plugin-sdk/tests/network-host-fixtures.json'), 'utf8'),
    ) as Array<{ name: string; host: string; valid: boolean }>
    for (const fixture of fixtures) {
      const value = timerManifest() as any
      value.minimumHostVersion = '0.3.2'
      value.permissions = ['network.https']
      value.command = { defaultName: 'network', activationMode: 'live', outputMode: 'mainResult', inputRequired: false }
      delete value.window
      value.network = { httpsHosts: [fixture.host] }
      expect(validate(value).ok, fixture.name).toBe(fixture.valid)
    }
  })
})

function credentialManifest() {
  const value = timerManifest()
  return { ...value, minimumHostVersion: '0.4.0', permissions: ['network.https'],
    command: { defaultName: 'credentials', activationMode: 'submit', outputMode: 'mainResult', inputRequired: false },
    window: undefined, settings: [{ type: 'secret', key: 'api-token', label: 'Token' }],
    network: { httpsHosts: ['api.example.com'], credentials: [
      { id: 'b', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' },
      { id: 'a', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' },
    ] } }
}

describe('credential manifest contract', () => {
  const future = (v: unknown) => validateManifest(Buffer.from(JSON.stringify(v)), 'windows', [0, 4, 0])
  it('accepts on explicit 0.4.0 and rejects explicit 0.3.4 while canonicalizing', () => {
    const v = credentialManifest()
    const result = future(v)
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.manifest.network?.credentials?.map(x => x.id)).toEqual(['a', 'b'])
    expect(validateManifest(Buffer.from(JSON.stringify(v)), 'windows', [0, 3, 4])).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })
    expect(validate(v)).toEqual(result)
    v.network.credentials.reverse()
    expect(future(v)).toEqual(result)
  })
  it.each([
    ['null', (v: any) => v.network.credentials = null],
    ['empty', (v: any) => v.network.credentials = []],
    ['too many', (v: any) => v.network.credentials = Array(9).fill(v.network.credentials[0])],
    ['duplicate id', (v: any) => v.network.credentials[1].id = 'b'],
    ['unknown key', (v: any) => v.network.credentials[0].extra = true],
    ['missing key', (v: any) => delete v.network.credentials[0].id],
    ['newline id', (v: any) => v.network.credentials[0].id = 'token\n'],
    ['newline host', (v: any) => { v.network.httpsHosts = ['api.example.com\n']; v.network.credentials.forEach((c: any) => c.host = 'api.example.com\n') }],
    ['invalid id', (v: any) => v.network.credentials[0].id = ' A'],
    ['long id', (v: any) => v.network.credentials[0].id = 'a'.repeat(65)],
    ['missing secret', (v: any) => v.network.credentials[0].settingKey = 'missing'],
    ['ordinary setting', (v: any) => v.settings[0].type = 'text'],
    ['other host', (v: any) => v.network.credentials[0].host = 'other.example.com'],
    ['invalid host', (v: any) => v.network.credentials[0].host = 'https://api.example.com'],
    ['wrong type', (v: any) => v.network.credentials[0].type = 'basic'],
    ['old minimum', (v: any) => v.minimumHostVersion = '0.3.4'],
  ])('rejects %s', (_, mutate) => {
    const v = credentialManifest(); mutate(v); expect(future(v).ok).toBe(false)
  })
  it('rejects duplicate JSON keys', () => {
    const raw = JSON.stringify(credentialManifest()).replace('"id":"a"', '"id":"a","id":"a"')
    expect(validateManifest(Buffer.from(raw), 'windows', [0, 4, 0])).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_JSON_INVALID' }] })
  })
})

// Exercise the actual shipped example, including the independent historical boundary.
it('validates the credential reference example on 0.4.0 and rejects 0.3.4', async () => {
  const bytes = await readFile(resolve(process.cwd(), '../../examples/public-plugins/com.uipilot.demo-credentials/package/plugin.json'))
  expect(validateManifest(bytes, 'windows', [0, 3, 4])).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })
  expect(validateManifest(bytes, 'windows', [0, 4, 0]).ok).toBe(true)
  expect(validateManifest(bytes, 'windows').ok).toBe(true)
})


describe('multi-command shared manifest contract', () => {
  it('matches Rust/schema classification on every shared fixture', async () => {
    const fixtures = JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json'), 'utf8'))
    for (const fixture of fixtures) {
      const result = validateManifest(Buffer.from(JSON.stringify(fixture.manifest)), 'windows', fixture.hostVersion.split('.').map(Number))
      expect(result.ok, fixture.name).toBe(fixture.cliCode === null)
      if (!result.ok) expect(result.issues.map((issue) => issue.code), fixture.name).toContain(fixture.cliCode)
      expect(result.manifest !== undefined, fixture.name).toBe(fixture.schemaValid)
    }
  })
})


it('preserves normalized command IDs and new wire without a default or prototype fallback', async () => {
  const fixtures = JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json'), 'utf8'))
  for (const fixture of fixtures.filter((item: any) => item.cliCode === null)) {
    const result = validateManifest(Buffer.from(JSON.stringify(fixture.manifest)), 'windows', [0, 5, 0])
    expect(result.ok, fixture.name).toBe(true)
    if (!result.ok) continue
    expect(normalizedCommands(result.manifest).map((item) => item.commandId)).toEqual(
      fixture.manifest.commands?.map((item: any) => item.id) ?? ['default'],
    )
    expect(Object.hasOwn(result.manifest, 'command')).toBe(!Object.hasOwn(fixture.manifest, 'commands'))
  }
})


it('rejects duplicate JSON command fields before schema/normalization', async () => {
  const fixtures = JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/multi-command-manifest-fixtures.json'), 'utf8'))
  const raw = JSON.stringify(fixtures[0].manifest)
  for (const [from, to] of [['"id":"default"', '"id":"default","id":"other"'],
    ['"defaultName":"upper"', '"defaultName":"upper","defaultName":"other"']]) {
    expect(validateManifest(Buffer.from(raw.replace(from, to)), 'windows', [0, 5, 0])).toMatchObject({
      ok: false, issues: [{ code: 'MANIFEST_JSON_INVALID' }],
    })
  }
})


// Diagnostic identity is independent of display names and manifest position.

describe('multi-command diagnostics', () => {
  const check = (value: unknown) => validateManifest(Buffer.from(JSON.stringify(value)), 'windows', [0, 5, 0])
  const semantic = (value: unknown) => check(value).issues.filter(issue => issue.code === 'MANIFEST_SEMANTIC_INVALID')

  it('retains sibling and multiple same-item failures with exact pointers and stable identity', () => {
    const value = multiCommandManifest()
    value.commands[0].summary = ''
    value.commands[0].inputRequired = true
    value.commands[1].summary = 'bad\u0000'
    const findings = semantic(value)
    expect(findings.map(issue => [issue.commandId, issue.location?.value])).toEqual([
      ['upper', '/commands/0/summary'], ['upper', '/commands/0/inputPlaceholder'],
      ['constructor', '/commands/1/summary'],
    ])
    value.commands.reverse()
    expect(semantic(value).map(issue => [issue.commandId, issue.location?.value])).toEqual([
      ['constructor', '/commands/0/summary'], ['upper', '/commands/1/summary'], ['upper', '/commands/1/inputPlaceholder'],
    ])
  })

  it('locates every duplicate ID including the first and never attributes its other failures', () => {
    const value = multiCommandManifest()
    value.commands[1].id = 'upper'
    value.commands[0].summary = ''
    value.commands[1].inputRequired = true
    const findings = semantic(value)
    expect(findings.filter(issue => issue.location?.value.endsWith('/id')).map(issue => issue.location?.value)).toEqual([
      '/commands/0/id', '/commands/1/id',
    ])
    expect(findings).toHaveLength(4)
    expect(findings.every(issue => !Object.hasOwn(issue, 'commandId'))).toBe(true)
  })

  it('attributes each duplicate name to its valid unique ID', () => {
    const value = multiCommandManifest()
    value.commands[1].defaultName = 'upper'
    expect(semantic(value).map(issue => [issue.commandId, issue.location?.value])).toEqual([
      ['upper', '/commands/0/defaultName'], ['constructor', '/commands/1/defaultName'],
    ])
  })

  it.each([undefined, '', 'BAD', null, 3])('locates missing/invalid ID %s without inventing identity or metadata', id => {
    const value = multiCommandManifest()
    const raw = JSON.parse(JSON.stringify(value))
    if (id === undefined) delete raw.commands[0].id
    else raw.commands[0].id = id
    const result = check(raw)
    expect(result.manifest).toBeUndefined()
    expect(result.issues).toContainEqual(expect.objectContaining({ location: { kind: 'jsonPointer', value: '/commands/0/id' } }))
    expect(result.issues.filter(issue => issue.location?.value === '/commands/0/id').every(issue => !Object.hasOwn(issue, 'commandId'))).toBe(true)
  })

  it('uses raw unique valid IDs for other schema findings, but never duplicated IDs', () => {
    const raw = JSON.parse(JSON.stringify(multiCommandManifest()))
    delete raw.commands[1].inputRequired
    expect(check(raw).issues).toContainEqual(expect.objectContaining({ commandId: 'constructor', location: { kind: 'jsonPointer', value: '/commands/1/inputRequired' } }))
    raw.commands[0].id = 'constructor'
    expect(check(raw).issues.every(issue => !Object.hasOwn(issue, 'commandId'))).toBe(true)
  })

  it('keeps package and compatibility findings unowned and locatable', () => {
    const value = multiCommandManifest()
    value.runtime.entry = '../runtime.js'
    value.minimumHostVersion = '0.4.0'
    const result = check(value)
    expect(result.issues).toContainEqual(expect.objectContaining({ location: { kind: 'jsonPointer', value: '/runtime/entry' } }))
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'API_INCOMPATIBLE', location: { kind: 'jsonPointer', value: '/minimumHostVersion' } }))
    expect(result.issues.every(issue => !Object.hasOwn(issue, 'commandId'))).toBe(true)
  })

  it('preserves strict JSON UTF-8 byte offsets', () => {
    expect(validateManifest(Buffer.from('{"name":"\u4e2d",}'), 'windows')).toEqual({ ok: false, issues: [{
      code: 'MANIFEST_JSON_INVALID', message: 'plugin.json is not strict JSON.', location: { kind: 'byteOffset', value: '14' },
    }] })
  })
})


it('historical Host 0.4 rejects both two-item and single-item commands arrays', () => {
  const value = multiCommandManifest()
  for (const commands of [value.commands, value.commands.slice(0, 1)]) {
    expect(validateManifest(Buffer.from(JSON.stringify({ ...value, commands })), 'windows', [0, 4, 0]))
      .toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })
  }
})


describe('panel Runtime action opt-in', () => {
  it('enforces booleans, minimum version and Windows-only declaration', () => {
    const value: any = timerManifest()
    value.command.outputMode = 'panel'
    value.minimumHostVersion = '0.6.0'
    value.permissions = ['ui.panel']
    delete value.window
    value.panel = { entry: 'dist/panel.html' }
    expect(validate(value).ok).toBe(true)
    for (const opt of [false, true]) {
      value.panel.runtimeActions = opt
      expect(validate(value).ok).toBe(true)
    }
    for (const opt of [null, 1, 'true']) {
      value.panel.runtimeActions = opt
      expect(validate(value).ok).toBe(false)
    }
    value.panel.runtimeActions = true
    value.minimumHostVersion = '0.5.0'
    expect(validate(value).ok).toBe(false)
    value.minimumHostVersion = '0.6.0'
    value.supportedPlatforms = ['windows', 'macos']
    expect(validate(value).ok).toBe(false)
  })
})


describe('window Runtime action opt-in', () => {
  it('requires boolean opt-in, Windows and Host 0.9.0', () => {
    const value: any = timerManifest()
    value.minimumHostVersion = '0.9.0'
    value.permissions = ['ui.window']
    for (const opt of [false, true]) {
      value.window.runtimeActions = opt
      expect(validate(value).ok).toBe(true)
    }
    for (const opt of [null, 1, 'true']) {
      value.window.runtimeActions = opt
      expect(validate(value).ok).toBe(false)
    }
    value.window.runtimeActions = true
    value.minimumHostVersion = '0.8.0'
    expect(validate(value).ok).toBe(false)
    value.minimumHostVersion = '0.9.0'
    value.supportedPlatforms = ['windows', 'macos']
    expect(validate(value).ok).toBe(false)
  })
})

it('matches the Rust result-action manifest parity fixtures', async () => {
  const cases = JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/result-action-manifest-fixtures.json'), 'utf8'))
  for (const item of cases) {
    const value: any = multiCommandManifest()
    value.minimumHostVersion = item.minimum
    if (item.minimum === '0.2.0') { value.command = value.commands[0]; delete value.command.id; delete value.commands }
    value.runtime = item.runtime; value.supportedPlatforms = item.platforms
    if (item.permissions) value.permissions = item.permissions
    expect(validate(value).ok, item.name).toBe(item.valid)
  }
})

it('files.openText requires Windows Host 0.13 and legacy submit window with ui.window without runtimeActions',()=>{
 const value:any=timerManifest();value.minimumHostVersion='0.13.0';value.permissions=['ui.window','files.openText'];delete value.window.runtimeActions
 expect(validate(value).ok).toBe(true)
 for(const patch of [{minimumHostVersion:'0.12.0'},{supportedPlatforms:['windows','macos']},{permissions:['files.openText']},{command:{...value.command,activationMode:'live'}},{command:{...value.command,outputMode:'mainResult'}}])expect(validate({...value,...patch}).ok).toBe(false)
})

it('matches Rust file-open manifest parity fixtures',async()=>{
 const cases=JSON.parse(await readFile(resolve('../../docs/plugin-sdk/tests/file-open-manifest-fixtures.json'),'utf8'))
 for(const item of cases)expect(validate(item.manifest).ok,item.name).toBe(item.valid)
})


describe('host credential service permissions', () => {
 it.each(['credentials.youdaoTranslation', 'credentials.baiduOcr', 'credentials.deepseek'])('accepts %s only on Windows Host 0.15+', (permission) => {
  const value = timerManifest()
  value.minimumHostVersion = '0.15.0'
  value.permissions = ['ui.window', permission]
  expect(validate(value).issues).toEqual([])
  value.minimumHostVersion = '0.14.0'
  expect(validate(value).issues.length).toBeGreaterThan(0)
  value.minimumHostVersion = '0.15.0'
  value.supportedPlatforms = ['windows', 'macos']
  expect(validate(value, 'macos').issues.length).toBeGreaterThan(0)
 })
})

it('selection.read requires Windows Host 0.16 and is independent of clipboard permissions', () => {
 const value = timerManifest()
 value.minimumHostVersion = '0.16.0'
 value.permissions = ['ui.window', 'selection.read']
 expect(validate(value).issues).toEqual([])
 expect(validate({ ...value, minimumHostVersion: '0.15.0' }).ok).toBe(false)
 expect(validate({ ...value, supportedPlatforms: ['windows', 'macos'] }).ok).toBe(false)
 expect(validate({ ...value, supportedPlatforms: ['macos'] }, 'macos').ok).toBe(false)
})

it('requires Host 0.16.0 for Delete host keys', async () => {
  const manifest = JSON.parse(await readFile(resolve('../../examples/public-plugins/com.uipilot.clipboard-history/package/plugin.json'), 'utf8'))
  manifest.panel.hostKeys = ['Delete', 'Tab']
  manifest.minimumHostVersion = '0.16.0'
  const accepted = validate(manifest)
  expect(accepted.ok).toBe(true)
  if (accepted.ok) expect(accepted.manifest.panel?.hostKeys).toEqual(['Tab', 'Delete'])
  manifest.minimumHostVersion = '0.15.0'
  expect(validate(manifest).issues.some((issue) => issue.code === 'API_INCOMPATIBLE')).toBe(true)
})

 it('requires Host 0.17.0, read permission, and Windows submit Panel for history copy', () => {
   const value = { schemaVersion: 1, pluginId: 'com.example.copy', version: '1.0.0', apiVersion: 1,
     minimumHostVersion: '0.17.0', name: 'Copy', supportedPlatforms: ['windows'],
     command: { defaultName: 'copy', activationMode: 'submit', outputMode: 'panel', inputRequired: false },
     runtime: { entry: 'dist/runtime.js' }, panel: { entry: 'dist/panel.html' },
     permissions: ['ui.panel', 'clipboard.history.read', 'clipboard.history.copy'], settings: [] }
   expect(validate(value)).toMatchObject({ ok: true })
   expect(validate({ ...value, minimumHostVersion: '0.16.0' })).toMatchObject({ ok: false, issues: [{ code: 'API_INCOMPATIBLE' }] })
   expect(validate({ ...value, permissions: ['ui.panel', 'clipboard.history.copy'] })).toMatchObject({ ok: false, issues: [{ code: 'MANIFEST_SEMANTIC_INVALID' }] })
   expect(validate({ ...value, supportedPlatforms: ['windows', 'macos'] })).toMatchObject({ ok: false })
   expect(validate({ ...value, command: { ...value.command, activationMode: 'live' } })).toMatchObject({ ok: false })
   expect(validate({ ...value, command: { ...value.command, outputMode: 'mainResult' }, panel: undefined })).toMatchObject({ ok: false })
 })
