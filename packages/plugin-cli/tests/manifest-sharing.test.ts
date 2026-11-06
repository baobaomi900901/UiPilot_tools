import { describe, expect, it } from 'vitest'
import { readFile } from 'node:fs/promises'
import { build } from 'esbuild'
import { Ajv2020 } from 'ajv/dist/2020.js'
import validateSchema from '../src/generated/manifest-validator.mjs'
import { normalizedCommands, validateManifest } from '../src/manifest.js'

function receiver() {
  return { id: 'translate-text', title: '翻译文本', commandId: 'default', types: ['text/plain'], maxBytes: 8192 }
}

function manifest(): any {
  return {
    schemaVersion: 1, pluginId: 'com.example.share', version: '1.0.0', apiVersion: 1,
    minimumHostVersion: '0.22.0', name: 'Share', supportedPlatforms: ['windows'],
    command: { defaultName: 'share', activationMode: 'submit', outputMode: 'panel', inputRequired: false },
    runtime: { entry: 'dist/runtime.js' }, panel: { entry: 'dist/panel.html' }, permissions: ['ui.panel'],
    share: { version: 1, source: true, receivers: [receiver()] },
  }
}

function validate(value: unknown, hostVersion?: readonly [number, number, number]) {
  return validateManifest(Buffer.from(JSON.stringify(value)), 'windows', hostVersion)
}

describe('sharing manifest v1', () => {
  it('bundles the standalone validator without Ajv runtime dependencies', async () => {
    const result = await build({
      entryPoints: ['src/generated/manifest-validator.mjs'], bundle: true,
      platform: 'node', format: 'esm', write: false, metafile: true,
    })
    expect(Object.keys(result.metafile!.inputs).filter(path => /node_modules[/\\]ajv[/\\]/u.test(path))).toEqual([])
  })

  it('keeps canonical Unicode title length and exact types-array semantics', async () => {
    const schema = JSON.parse(await readFile(new URL('../schema/uipilot-plugin-v1.schema.json', import.meta.url), 'utf8'))
    const properties = schema.$defs.PublicShareReceiverV1.properties
    const canonical = new Ajv2020({ strict: true }).compile({
      type: 'object', required: ['title', 'types'],
      properties: { title: properties.title, types: properties.types },
    })
    const titles = ['', 'a', '😀', '😀'.repeat(80), 'a'.repeat(79) + '😀', 'a'.repeat(80),
      '😀'.repeat(81), 'a'.repeat(80) + '😀', 'e\u0301'.repeat(40), 'e\u0301'.repeat(40) + 'x',
      ...['\n', '\r', '\u2028', '\u2029'].map(end => 'a'.repeat(80) + end),
      '\ud800', '\udfff', 'a\0', '\u2028', '\u2029']
    const typesCases = [['text/plain'], [], ['text/plain', 'text/plain'], ['text/plain', 'text/html'],
      ['text/html', 'text/plain'], ['TEXT/PLAIN'], 'text/plain', null, {}, [null]]
    for (const title of titles) for (const types of typesCases) {
      const value = manifest()
      Object.assign(value.share.receivers[0], { title, types })
      expect(validateSchema(value), JSON.stringify({ title, types })).toBe(canonical({ title, types }))
    }
  })

  it.each(['source', 'receivers', 'both'])('accepts %s and keeps legacy routing identity internal', (kind) => {
    const value = manifest()
    if (kind === 'source') delete value.share.receivers
    if (kind === 'receivers') delete value.share.source
    const result = validate(value)
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(normalizedCommands(result.manifest)[0].commandId).toBe('default')
      expect(result.manifest.command).not.toHaveProperty('id')
      expect(result.manifest.share).toEqual(value.share)
    }
  })

  it('accepts scalar title and receiver/byte boundaries', () => {
    const value = manifest()
    value.share.receivers = Array.from({ length: 8 }, (_, i) => ({ ...receiver(), id: i === 0 ? 'a'.repeat(64) : `r${i}`, title: '😀'.repeat(80), maxBytes: i === 0 ? 1 : 16384 }))
    expect(validate(value).ok).toBe(true)
  })

  const invalid: Array<[string, (v: any) => void]> = [
    ['null share', v => { v.share = null }],
    ['missing version', v => { delete v.share.version }],
    ['wrong version', v => { v.share.version = 2 }],
    ['null version', v => { v.share.version = null }],
    ['empty declaration', v => { v.share = { version: 1 } }],
    ['false source', v => { v.share.source = false }],
    ['null source', v => { v.share.source = null }],
    ['unknown share field', v => { v.share.send = true }],
    ['null receivers', v => { v.share.receivers = null }],
    ['empty receivers', v => { v.share.receivers = [] }],
    ['too many receivers', v => { v.share.receivers = Array.from({ length: 9 }, (_, i) => ({ ...receiver(), id: `r${i}` })) }],
    ['duplicate receiver ID', v => { v.share.receivers.push({ ...receiver(), title: 'Other' }) }],
    ['unknown receiver field', v => { v.share.receivers[0].input = '' }],
    ['null receiver', v => { v.share.receivers[0] = null }],
    ...['', 'Upper', 'a/b', 'a_b', 'é', 'a'.repeat(65), '__proto__', 'prototype', 'constructor'].map(id => [`unsafe ID ${id}`, (v: any) => { v.share.receivers[0].id = id }] as [string, (v: any) => void]),
    ...['', '😀'.repeat(81), 'a\n', 'a\t', 'a\0', 'a\x7f', 'a\x85', '\ud800'].map(title => [`invalid title ${JSON.stringify(title)}`, (v: any) => { v.share.receivers[0].title = title }] as [string, (v: any) => void]),
    ['missing commandId', v => { delete v.share.receivers[0].commandId }],
    ['wrong commandId', v => { v.share.receivers[0].commandId = 'translate' }],
    ...[null, [], ['text/html'], ['text/plain', 'text/plain']].map(types => [`invalid types ${JSON.stringify(types)}`, (v: any) => { v.share.receivers[0].types = types }] as [string, (v: any) => void]),
    ...[null, 0, -1, 16385, 1.5, '8192'].map(maxBytes => [`invalid maxBytes ${maxBytes}`, (v: any) => { v.share.receivers[0].maxBytes = maxBytes }] as [string, (v: any) => void]),
    ['old host floor', v => { v.minimumHostVersion = '0.21.0' }],
    ['macOS', v => { v.supportedPlatforms = ['windows', 'macos'] }],
    ['live', v => { v.command.activationMode = 'live' }],
    ['mainResult', v => { v.command.outputMode = 'mainResult'; delete v.panel; v.permissions = [] }],
    ['window', v => { v.command.outputMode = 'window'; delete v.panel; v.window = { entry: 'dist/window.html' }; v.permissions = ['ui.window'] }],
    ['missing panel', v => { delete v.panel }],
    ['missing permission', v => { v.permissions = [] }],
    ['named commands', v => { v.commands = [{ ...v.command, id: 'share', outputMode: 'mainResult' }]; delete v.command; delete v.panel; v.permissions = [] }],
  ]
  it.each(invalid)('rejects %s', (_label, mutate) => {
    const value = manifest()
    mutate(value)
    expect(validate(value).ok).toBe(false)
  })

  it.each(['version', 'maxBytes', 'schemaVersion'])('enforces %s bounds in the generated schema validator', field => {
    const value = manifest()
    if (field === 'version') value.share.version = 2
    else if (field === 'maxBytes') value.share.receivers[0].maxBytes = 16385
    else value.schemaVersion = 4294967296
    expect(validate(value)).toMatchObject({ ok: false, issues: expect.arrayContaining([{ code: 'MANIFEST_SCHEMA_INVALID', message: expect.any(String), location: expect.any(Object) }]) })
  })

  it('rejects older installed host, duplicate JSON keys, and leaves old packages valid', () => {
    const value = manifest()
    expect(validate(value, [0, 21, 0]).ok).toBe(false)
    const bytes = JSON.stringify(value).replace('"version":1,"source"', '"version":1,"version":1,"source"')
    expect(validateManifest(Buffer.from(bytes), 'windows').ok).toBe(false)
    delete value.share
    value.minimumHostVersion = '0.3.0'
    expect(validate(value).ok).toBe(true)
  })
})
