import { describe, expect, it } from 'vitest'

import { parseCli, renderCliError, renderHumanReport, runCli } from '../src/cli.js'
import type { PluginValidationReportV1 } from '../src/contracts.js'

const validReport: PluginValidationReportV1 = {
  schemaVersion: 1,
  valid: true,
  source: { kind: 'directory', path: 'plugin' },
  target: { platform: 'windows', hostVersion: '0.2.0', apiVersion: 1 },
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
