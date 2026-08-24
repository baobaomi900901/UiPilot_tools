import type {
  PluginCliErrorV1,
  PluginPlatform,
  PluginValidationReportV1,
  ValidatePackage,
} from './contracts.js'

export type ParsedCli =
  | { kind: 'help' }
  | { kind: 'error'; message: string }
  | { kind: 'validate'; source: string; platform: PluginPlatform; json: boolean }

export interface CliResult {
  exitCode: 0 | 1 | 2
  stdout: string
}

export const HELP_TEXT = `Usage: uipilot-plugin validate <source> [--platform windows|macos] [--json]

Validate a UiPilot public plugin directory or .uipilot-plugin archive.
`

function defaultPlatform(platform: NodeJS.Platform): PluginPlatform | undefined {
  if (platform === 'win32') return 'windows'
  if (platform === 'darwin') return 'macos'
  return undefined
}

export function parseCli(argv: readonly string[], hostPlatform: NodeJS.Platform): ParsedCli {
  if (argv.length === 1 && (argv[0] === '--help' || argv[0] === '-h')) return { kind: 'help' }
  if (argv[0] !== 'validate') return { kind: 'error', message: HELP_TEXT.trimEnd() }

  let source: string | undefined
  let selectedPlatform: PluginPlatform | undefined
  let json = false

  for (let index = 1; index < argv.length; index += 1) {
    const value = argv[index]
    if (value === '--json') {
      if (json) return { kind: 'error', message: 'The --json option may be used only once.' }
      json = true
      continue
    }
    if (value === '--platform') {
      if (selectedPlatform) return { kind: 'error', message: 'The --platform option may be used only once.' }
      const platform = argv[index + 1]
      if (platform !== 'windows' && platform !== 'macos') {
        return { kind: 'error', message: 'The --platform option must be windows or macos.' }
      }
      selectedPlatform = platform
      index += 1
      continue
    }
    if (value?.startsWith('-') || source) return { kind: 'error', message: 'Provide exactly one source.' }
    source = value
  }

  if (!source) return { kind: 'error', message: 'Provide exactly one source.' }
  const platform = selectedPlatform ?? defaultPlatform(hostPlatform)
  if (!platform) {
    return {
      kind: 'error',
      message: 'Use --platform windows or --platform macos on this operating system.',
    }
  }
  return { kind: 'validate', source, platform, json }
}

export function renderCliError(
  code: PluginCliErrorV1['error']['code'],
  message: string,
  json: boolean,
): string {
  if (!json) return `${code}: ${message}`
  return JSON.stringify({ schemaVersion: 1, error: { code, message } } satisfies PluginCliErrorV1)
}

export function renderHumanReport(report: PluginValidationReportV1): string {
  if (report.valid) {
    const identity = report.plugin ? ` ${report.plugin.pluginId}@${report.plugin.version}` : ''
    return `Valid UiPilot public plugin${identity}.`
  }
  const lines = [`Invalid UiPilot public plugin (${report.issues.length} issue${report.issues.length === 1 ? '' : 's'}).`]
  for (const issue of report.issues) {
    const path = issue.path ? `${issue.path}: ` : ''
    lines.push(`${path}${issue.code}: ${issue.message}`)
  }
  if (report.truncated) lines.push('Additional issues were omitted.')
  return lines.join('\n')
}

export async function runCli(
  argv: readonly string[],
  hostPlatform: NodeJS.Platform,
  validatePackage: ValidatePackage,
): Promise<CliResult> {
  const parsed = parseCli(argv, hostPlatform)
  const wantsJson = argv.includes('--json')
  if (parsed.kind === 'help') return { exitCode: 0, stdout: HELP_TEXT }
  if (parsed.kind === 'error') {
    return { exitCode: 2, stdout: `${renderCliError('CLI_USAGE', parsed.message, wantsJson)}\n` }
  }

  try {
    const report = await validatePackage({ source: parsed.source, platform: parsed.platform })
    return {
      exitCode: report.valid ? 0 : 1,
      stdout: `${parsed.json ? JSON.stringify(report) : renderHumanReport(report)}\n`,
    }
  } catch {
    return {
      exitCode: 2,
      stdout: `${renderCliError('CLI_INTERNAL', 'Validation failed.', parsed.json)}\n`,
    }
  }
}
