import { describe, expect, it } from 'vitest'

import { parseCli, renderCliError, renderHumanReport, runCli } from '../src/cli.js'
import type { PluginValidationReportV1 } from '../src/contracts.js'

const validReport: PluginValidationReportV1 = {
  schemaVersion: 1,
  valid: true,
  source: { kind: 'directory', path: 'plugin' },
  target: { platform: 'windows', hostVersion: '0.3.4', apiVersion: 1 },
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
