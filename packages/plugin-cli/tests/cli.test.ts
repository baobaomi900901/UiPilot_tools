import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { validatePackage } from '../src/source-validation.js'
import { packPackage } from '../src/pack.js'
import { createZip } from './zip-fixture.js'
import { multiCommandManifest } from './multi-command-fixture.js'
import { describe, expect, it } from 'vitest'

import { parseCli, renderCliError, renderHumanReport, runCli } from '../src/cli.js'
import type { PluginValidationReportV1 } from '../src/contracts.js'

const validReport: PluginValidationReportV1 = {
  schemaVersion: 1,
  valid: true,
  source: { kind: 'directory', path: 'plugin' },
  target: { platform: 'windows', hostVersion: '0.11.0', apiVersion: 1 },
  plugin: { pluginId: 'com.example.demo', version: '1.0.0', outputMode: 'window' },
  truncated: false,
  issues: [],
}

describe('parseCli', () => {
  it('parses validate with explicit platform and JSON output', () => {
    expect(parseCli(['validate', 'plugin', '--platform', 'macos', '--json'], 'win32')).toEqual({
      kind: 'validate',
      source: 'plugin',
      platform: 'macos',
      json: true,
    })
  })

  it('defaults to the current supported platform', () => {
    expect(parseCli(['validate', 'plugin'], 'win32')).toMatchObject({ platform: 'windows' })
    expect(parseCli(['validate', 'plugin'], 'darwin')).toMatchObject({ platform: 'macos' })
  })

  it('requires an explicit platform on another operating system', () => {
    expect(parseCli(['validate', 'plugin'], 'linux')).toEqual({
      kind: 'error',
      message: 'Use --platform windows or --platform macos on this operating system.',
    })
  })

  it('rejects unknown, duplicate, and missing arguments', () => {
    for (const args of [
      [],
      ['scan', 'plugin'],
      ['validate'],
      ['validate', 'a', 'b'],
      ['validate', 'a', '--json', '--json'],
      ['validate', 'a', '--platform', 'linux'],
    ]) {
      expect(parseCli(args, 'win32').kind).toBe('error')
    }
  })

  it('supports top-level help for the packed bin smoke test', () => {
    expect(parseCli(['--help'], 'win32')).toEqual({ kind: 'help' })
  })
})

describe('CLI output', () => {
  it('renders concise human success and failure output', () => {
    expect(renderHumanReport(validReport)).toContain('Valid UiPilot public plugin')
    const invalid = { ...validReport, valid: false, issues: [{ code: 'SOURCE_INVALID' as const, message: 'Source is unsafe.' }] }
    expect(renderHumanReport(invalid)).toContain('SOURCE_INVALID: Source is unsafe.')
  })

  it('renders stable internal errors without a stack trace', () => {
    expect(renderCliError('CLI_INTERNAL', 'Validation failed.', true)).toBe(
      '{"schemaVersion":1,"error":{"code":"CLI_INTERNAL","message":"Validation failed."}}',
    )
  })

  it('maps validation reports and unexpected errors to fixed exit codes', async () => {
    await expect(runCli(['validate', 'plugin', '--json'], 'win32', async () => validReport)).resolves.toEqual({
      exitCode: 0,
      stdout: `${JSON.stringify(validReport)}\n`,
    })
    await expect(
      runCli(['validate', 'plugin', '--json'], 'win32', async () => {
        throw new Error('secret stack')
      }),
    ).resolves.toEqual({
      exitCode: 2,
      stdout: '{"schemaVersion":1,"error":{"code":"CLI_INTERNAL","message":"Validation failed."}}\n',
    })
  })
})

describe('pack CLI', () => {
  it('parses explicit output and platform, including options before the source', () => {
    expect(parseCli(['pack', '--output', 'out file.uipilot-plugin', 'plugin', '--json', '--platform', 'macos'], 'win32')).toEqual({
      kind: 'pack', source: 'plugin', output: 'out file.uipilot-plugin', platform: 'macos', json: true,
    })
    expect(parseCli(['pack', 'plugin', '--output', 'out.uipilot-plugin'], 'win32')).toMatchObject({ platform: 'windows' })
    expect(parseCli(['pack', 'plugin', '--output', 'out.uipilot-plugin'], 'darwin')).toMatchObject({ platform: 'macos' })
    expect(parseCli(['pack', 'plugin', '--output', 'out.uipilot-plugin'], 'linux').kind).toBe('error')
  })

  it('rejects missing, repeated and unknown pack arguments', async () => {
    for (const args of [
      ['pack'], ['pack', 'plugin'], ['pack', 'plugin', '--output'],
      ['pack', 'plugin', '--output', '--json'], ['pack', 'plugin', '--output', ''],
      ['pack', 'plugin', '--output', 'a', '--output', 'b'],
      ['pack', 'a', 'b', '--output', 'out'], ['pack', 'a', '--output', 'out', '--force'],
      ['pack', 'a', '--output', 'out', '--json', '--json'],
      ['pack', 'a', '--output', 'out', '--platform', 'windows', '--platform', 'macos'],
      ['pack', 'a', '--output', 'out', '--platform', 'linux'],
      ['validate', 'a', '--output', 'out'],
    ]) {
      expect(parseCli(args, 'win32').kind).toBe('error')
      const result = await runCli([...args, '--json'], 'win32', async () => validReport)
      expect(result.exitCode).toBe(2)
      expect(JSON.parse(result.stdout).error.code).toBe('CLI_USAGE')
    }
  })

  it('advertises both commands in top-level help', async () => {
    const result = await runCli(['--help'], 'win32', async () => validReport)
    expect(result.stdout).toContain('uipilot-plugin validate')
    expect(result.stdout).toContain('uipilot-plugin pack <source-directory> --output <file.uipilot-plugin>')
  })

  const success = {
    schemaVersion: 1 as const, created: true as const,
    output: { path: 'C:/out file.uipilot-plugin', sha256: 'a'.repeat(64) }, validation: validReport,
  }
  const args = ['pack', 'plugin', '--output', 'out file.uipilot-plugin']

  it('passes the request to pack and preserves its JSON success report', async () => {
    const result = await runCli([...args, '--json'], 'win32', async () => { throw new Error('validate must not run') }, async (request) => {
      expect(request).toEqual({ source: 'plugin', output: 'out file.uipilot-plugin', platform: 'windows' })
      return success
    })
    expect(result).toEqual({ exitCode: 0, stdout: `${JSON.stringify(success)}\n` })
  })

  it('prints the output path, hash and cleanup warnings while retaining success', async () => {
    const report = { ...success, warnings: [{ code: 'TEMP_CLEANUP_FAILED' as const, message: 'Temporary file could not be removed.' }] }
    const json = await runCli([...args, '--json'], 'win32', async () => validReport, async () => report)
    expect(json).toEqual({ exitCode: 0, stdout: `${JSON.stringify(report)}\n` })
    const human = await runCli(args, 'win32', async () => validReport, async () => report)
    expect(human.exitCode).toBe(0)
    expect(human.stdout).toContain(success.output.path)
    expect(human.stdout).toContain(success.output.sha256)
    expect(human.stdout).toContain('TEMP_CLEANUP_FAILED: Temporary file could not be removed.')
  })

  it('returns expected failure JSON unchanged and concise human errors', async () => {
    for (const code of ['SOURCE_INVALID', 'PACKAGE_INVALID', 'OUTPUT_INVALID', 'OUTPUT_EXISTS', 'OUTPUT_WRITE_FAILED'] as const) {
      const report = { schemaVersion: 1 as const, created: false as const, output: { path: 'out' }, error: { code, message: 'Cannot create package.' }, ...(code === 'PACKAGE_INVALID' ? { validation: { ...validReport, valid: false } } : {}) }
      expect(await runCli([...args, '--json'], 'win32', async () => validReport, async () => report)).toEqual({ exitCode: 1, stdout: `${JSON.stringify(report)}\n` })
      expect(await runCli(args, 'win32', async () => validReport, async () => report)).toEqual({ exitCode: 1, stdout: `${code}: Cannot create package.\n` })
    }
  })

  it('sanitizes unknown pack failures and an absent pack dependency', async () => {
    for (const pack of [undefined, async () => { throw new Error('secret credential stack') }]) {
      expect(await runCli([...args, '--json'], 'win32', async () => validReport, pack)).toEqual({
        exitCode: 2, stdout: '{"schemaVersion":1,"error":{"code":"CLI_INTERNAL","message":"Packing failed."}}\n',
      })
    }
  })

  it('retains three-argument validate invalid and usage behavior', async () => {
    const invalid = { ...validReport, valid: false }
    expect(await runCli(['validate', 'plugin', '--json'], 'win32', async () => invalid)).toEqual({ exitCode: 1, stdout: `${JSON.stringify(invalid)}\n` })
    expect((await runCli(['validate'], 'win32', async () => validReport)).stdout).toBe('CLI_USAGE: Provide exactly one source.\n')
  })
})


it('renders command context and pointer-only findings from command metadata', async () => {
  const issues = [
    { code: 'MANIFEST_SEMANTIC_INVALID' as const, path: 'plugin.json', location: { kind: 'jsonPointer' as const, value: '/commands/1/inputPlaceholder' }, commandId: 'lower', message: 'Placeholder is required.' },
    { code: 'MANIFEST_SCHEMA_INVALID' as const, path: 'plugin.json', location: { kind: 'jsonPointer' as const, value: '/commands/0/id' }, message: 'Manifest must have required property id.' },
  ]
  const report = { ...validReport, plugin: { ...validReport.plugin!, commands: [] }, valid: false, issues }
  const lines = 'plugin.json: MANIFEST_SEMANTIC_INVALID [command lower at /commands/1/inputPlaceholder]: Placeholder is required.\n' +
    'plugin.json: MANIFEST_SCHEMA_INVALID [/commands/0/id]: Manifest must have required property id.'
  expect(renderHumanReport(report)).toBe('Invalid UiPilot public plugin (2 issues).\n' + lines)
  const packed = { schemaVersion: 1 as const, created: false as const, output: { path: 'out' }, error: { code: 'PACKAGE_INVALID' as const, message: 'Cannot create package.' }, validation: report }
  expect(await runCli(['pack', 'plugin', '--output', 'out'], 'win32', async () => report, async () => packed)).toEqual({
    exitCode: 1, stdout: 'PACKAGE_INVALID: Cannot create package.\n' + lines + '\n',
  })
})


it('locks actual legacy directory/archive JSON and human bytes and pack embedded reports to baseline goldens', async () => {
  const root = await mkdtemp(join(tmpdir(), 'uipilot-legacy-golden-'))
  try {
    for (const status of ['valid', 'invalid']) {
      const manifest = await readFile(`tests/fixtures/legacy-reports/${status}-manifest.json`)
      const directory = join(root, status)
      await mkdir(join(directory, 'dist'), { recursive: true })
      await writeFile(join(directory, 'plugin.json'), manifest)
      await writeFile(join(directory, 'dist/runtime.js'), 'export {}')
      const archive = join(root, `${status}.uipilot-plugin`)
      await writeFile(archive, createZip([{ name: 'plugin.json', data: manifest }, { name: 'dist/runtime.js', data: Buffer.from('export {}') }]))
      const validate = (request: { source: string; platform: 'windows' | 'macos' }) => validatePackage(request, { hostPlatform: 'linux' })
      for (const [kind, source] of [['directory', directory], ['archive', archive]] as const) {
        const report = await validate({ source, platform: 'windows' })
        const canonical = { ...report, source: { kind, path: '<source>' } }
        expect(JSON.stringify(canonical) + '\n').toBe(await readFile(`tests/fixtures/legacy-reports/${status}-${kind}.json`, 'utf8'))
        const expectedText = await readFile(`tests/fixtures/legacy-reports/${status}-${kind}.txt`, 'utf8')
        expect(await runCli(['validate', source], 'win32', validate)).toEqual({ exitCode: status === 'valid' ? 0 : 1, stdout: expectedText })
        expect(await runCli(['validate', source, '--json'], 'win32', validate)).toEqual({ exitCode: status === 'valid' ? 0 : 1, stdout: JSON.stringify(report) + '\n' })
      }
      const packed = await packPackage({ source: directory, output: join(root, `${status}-packed.uipilot-plugin`), platform: 'windows' }, { hostPlatform: 'linux' })
      expect(packed.validation).toEqual(await validate({ source: directory, platform: 'windows' }))
    }
  } finally { await rm(root, { recursive: true, force: true }) }
}, 30_000)



it('keeps legacy pointer and byte-offset human formatting and pack failure envelope exact', async () => {
  const report = { ...validReport, valid: false, issues: [
    { code: 'MANIFEST_SCHEMA_INVALID' as const, path: 'plugin.json', location: { kind: 'jsonPointer' as const, value: '/command' }, message: 'Manifest must have required property inputRequired.' },
    { code: 'MANIFEST_JSON_INVALID' as const, path: 'plugin.json', location: { kind: 'byteOffset' as const, value: '14' }, message: 'plugin.json is not strict JSON.' },
  ] }
  expect(renderHumanReport(report)).toBe('Invalid UiPilot public plugin (2 issues).\n' +
    'plugin.json: MANIFEST_SCHEMA_INVALID: Manifest must have required property inputRequired.\n' +
    'plugin.json: MANIFEST_JSON_INVALID: plugin.json is not strict JSON.')
  const packed = { schemaVersion: 1 as const, created: false as const, output: { path: 'out' }, error: { code: 'PACKAGE_INVALID' as const, message: 'Package validation failed.' }, validation: report }
  expect(await runCli(['pack', 'plugin', '--output', 'out'], 'win32', async () => report, async () => packed)).toEqual({
    exitCode: 1, stdout: 'PACKAGE_INVALID: Package validation failed.\n',
  })
})

it('shares command-local findings across real directory/archive validation and human pack output', async () => {
  const root = await mkdtemp(join(tmpdir(), 'uipilot-command-diagnostics-'))
  try {
    const value = multiCommandManifest()
    value.minimumHostVersion = '0.11.1'
    value.commands[0].summary = ''
    value.commands[1].inputRequired = true
    const manifest = Buffer.from(JSON.stringify(value))
    const directory = join(root, 'package')
    await mkdir(join(directory, 'dist'), { recursive: true })
    await writeFile(join(directory, 'plugin.json'), manifest)
    await writeFile(join(directory, 'dist/runtime.js'), 'export {}')
    const archive = join(root, 'package.uipilot-plugin')
    await writeFile(archive, createZip([{ name: 'plugin.json', data: manifest }, { name: 'dist/runtime.js', data: Buffer.from('export {}') }]))
    const validate = (request: { source: string; platform: 'windows' | 'macos' }) => validatePackage(request, { hostPlatform: 'linux' })
    const first = await validate({ source: directory, platform: 'windows' })
    const second = await validate({ source: archive, platform: 'windows' })
    expect({ ...second, source: first.source }).toEqual(first)
    const lines = [
      'plugin.json: MANIFEST_SEMANTIC_INVALID [command upper at /commands/0/summary]: Command summary must be nonempty, at most 512 Unicode scalars, and contain no control characters.',
      'plugin.json: MANIFEST_SEMANTIC_INVALID [command constructor at /commands/1/inputPlaceholder]: A command requiring input must have a nonempty placeholder.',
      'plugin.json: API_INCOMPATIBLE [/minimumHostVersion]: Plugin API or minimum host version is incompatible.',
    ].join('\n')
    expect((await runCli(['validate', archive], 'win32', validate)).stdout).toBe('Invalid UiPilot public plugin (3 issues).\n' + lines + '\n')
    const packed = await runCli(['pack', directory, '--output', join(root, 'out.uipilot-plugin')], 'win32', validate, request => packPackage(request, { hostPlatform: 'linux' }))
    expect(packed).toEqual({ exitCode: 1, stdout: 'PACKAGE_INVALID: Package validation failed.\n' + lines + '\n' })
  } finally { await rm(root, { recursive: true, force: true }) }
}, 30_000)


it.each([false, true])('retains schema-invalid source context across directory/archive/pack (local error: %s)', async (localError) => {
  const root = await mkdtemp(join(tmpdir(), 'uipilot-schema-context-'))
  try {
    const value = multiCommandManifest()
    const manifest = Buffer.from(JSON.stringify({
      ...value,
      runtime: { entry: 5 },
      commands: value.commands.map((command, index) => localError && index === 0
        ? { ...command, inputRequired: 5 } : command),
    }))
    const directory = join(root, 'package')
    await mkdir(join(directory, 'dist'), { recursive: true })
    await writeFile(join(directory, 'plugin.json'), manifest)
    await writeFile(join(directory, 'dist/runtime.js'), 'export {}')
    const archive = join(root, 'package.uipilot-plugin')
    await writeFile(archive, createZip([{ name: 'plugin.json', data: manifest }, { name: 'dist/runtime.js', data: Buffer.from('export {}') }]))
    const validate = (request: { source: string; platform: 'windows' | 'macos' }) => validatePackage(request, { hostPlatform: 'linux' })
    const pack = (request: Parameters<typeof packPackage>[0]) => packPackage(request, { hostPlatform: 'linux' })
    const sharedLine = 'plugin.json: MANIFEST_SCHEMA_INVALID [/runtime/entry]: Manifest must be string.'
    const localLine = 'plugin.json: MANIFEST_SCHEMA_INVALID [command upper at /commands/0/inputRequired]: Manifest must be boolean.'
    const first = await validate({ source: directory, platform: 'windows' })
    expect(first.plugin).toBeUndefined()
    expect(first.issues).toHaveLength(localError ? 2 : 1)
    expect(first.issues.find(issue => issue.location?.value === '/runtime/entry')).toEqual({
      code: 'MANIFEST_SCHEMA_INVALID', path: 'plugin.json',
      location: { kind: 'jsonPointer', value: '/runtime/entry' }, message: 'Manifest must be string.',
    })
    for (const source of [directory, archive]) {
      const json = await runCli(['validate', source, '--json'], 'win32', validate)
      expect(json.exitCode).toBe(1)
      const wire = JSON.parse(json.stdout)
      expect(Object.keys(wire)).toEqual(['schemaVersion', 'valid', 'source', 'target', 'truncated', 'issues'])
      expect({ ...wire, source: first.source }).toEqual(JSON.parse(JSON.stringify(first)))
      const human = await runCli(['validate', source], 'win32', validate)
      expect(human.exitCode).toBe(1)
      expect(human.stdout).toContain(sharedLine + '\n')
      if (localError) expect(human.stdout).toContain(localLine + '\n')
      else expect(human.stdout).toBe('Invalid UiPilot public plugin (1 issue).\n' + sharedLine + '\n')
    }
    const args = ['pack', directory, '--output', join(root, 'out.uipilot-plugin')]
    const packedJson = await runCli([...args, '--json'], 'win32', validate, pack)
    expect(packedJson.exitCode).toBe(1)
    const packedWire = JSON.parse(packedJson.stdout)
    expect(Object.keys(packedWire)).toEqual(['schemaVersion', 'created', 'output', 'validation', 'error'])
    expect(packedWire.validation).toEqual(JSON.parse(JSON.stringify(first)))
    const packed = await runCli(args, 'win32', validate, pack)
    expect(packed.exitCode).toBe(1)
    expect(packed.stdout).toContain(sharedLine + '\n')
    if (localError) expect(packed.stdout).toContain(localLine + '\n')
    else expect(packed.stdout).toBe('PACKAGE_INVALID: Package validation failed.\n' + sharedLine + '\n')
  } finally { await rm(root, { recursive: true, force: true }) }
}, 30_000)
