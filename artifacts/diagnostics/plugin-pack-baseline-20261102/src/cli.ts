import { hasCommandDiagnostics } from './report.js'
import type {
  PackPackage,
  PluginCliErrorV1,
  PluginPlatform,
  PluginValidationIssue,
  PluginValidationReportV1,
  ValidatePackage,
} from './contracts.js'

export type ParsedCli =
  | { kind: 'help' }
  | { kind: 'error'; message: string }
  | { kind: 'validate'; source: string; platform: PluginPlatform; json: boolean }
  | { kind: 'pack'; source: string; output: string; platform: PluginPlatform; json: boolean }

export interface CliResult {
  exitCode: 0 | 1 | 2
  stdout: string
}

export const HELP_TEXT = `Usage: uipilot-plugin validate <source> [--platform windows|macos] [--json]

Validate a UiPilot public plugin directory or .uipilot-plugin archive.

       uipilot-plugin pack <source-directory> --output <file.uipilot-plugin> [--platform windows|macos] [--json]

Create a validated local plugin archive without overwriting an existing path.
`

function defaultPlatform(platform: NodeJS.Platform): PluginPlatform | undefined {
  if (platform === 'win32') return 'windows'
  if (platform === 'darwin') return 'macos'
  return undefined
}

export function parseCli(argv: readonly string[], hostPlatform: NodeJS.Platform): ParsedCli {
  if (argv.length === 1 && (argv[0] === '--help' || argv[0] === '-h')) return { kind: 'help' }
  const command = argv[0]
  if (command !== 'validate' && command !== 'pack') return { kind: 'error', message: HELP_TEXT.trimEnd() }

  let source: string | undefined
  let output: string | undefined
  let selectedPlatform: PluginPlatform | undefined
  let json = false

  for (let index = 1; index < argv.length; index += 1) {
    const value = argv[index]
    if (value === '--output' && command === 'pack') {
      if (output !== undefined) return { kind: 'error', message: 'The --output option may be used only once.' }
      const destination = argv[index + 1]
      if (!destination || destination.startsWith('-')) {
        return { kind: 'error', message: 'Provide an output path after --output.' }
      }
      output = destination
      index += 1
      continue
    }
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
  if (command === 'pack' && output === undefined) return { kind: 'error', message: 'Provide an explicit --output path.' }
  const platform = selectedPlatform ?? defaultPlatform(hostPlatform)
  if (!platform) {
    return {
      kind: 'error',
      message: 'Use --platform windows or --platform macos on this operating system.',
    }
  }
  if (command === 'pack' && output !== undefined) return { kind: 'pack', source, output, platform, json }
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

function hasCommandContext(report: PluginValidationReportV1): boolean {
  return hasCommandDiagnostics(report) || report.plugin?.commands !== undefined
}

function renderHumanIssue(issue: PluginValidationIssue, commandContext: boolean): string {
  const path = issue.path ? `${issue.path}: ` : ''
  const pointer = commandContext && issue.location?.kind === 'jsonPointer' ? issue.location.value : undefined
  const context = pointer !== undefined ? ` [${issue.commandId !== undefined ? `command ${issue.commandId} at ` : ''}${pointer}]` : ''
  return `${path}${issue.code}${context}: ${issue.message}`
}

export function renderHumanReport(report: PluginValidationReportV1): string {
  if (report.valid) {
    const identity = report.plugin ? ` ${report.plugin.pluginId}@${report.plugin.version}` : ''
    return `Valid UiPilot public plugin${identity}.`
  }
  const lines = [`Invalid UiPilot public plugin (${report.issues.length} issue${report.issues.length === 1 ? '' : 's'}).`]
  for (const issue of report.issues) {
    lines.push(renderHumanIssue(issue, hasCommandContext(report)))
  }
  if (report.truncated) lines.push('Additional issues were omitted.')
  return lines.join('\n')
}

export async function runCli(
  argv: readonly string[],
  hostPlatform: NodeJS.Platform,
  validatePackage: ValidatePackage,
  packPackage?: PackPackage,
): Promise<CliResult> {
  const parsed = parseCli(argv, hostPlatform)
  const wantsJson = argv.includes('--json')
  if (parsed.kind === 'help') return { exitCode: 0, stdout: HELP_TEXT }
  if (parsed.kind === 'error') {
    return { exitCode: 2, stdout: `${renderCliError('CLI_USAGE', parsed.message, wantsJson)}\n` }
  }

  try {
    if (parsed.kind === 'pack') {
      if (!packPackage) throw new Error('Pack dependency is unavailable.')
      const report = await packPackage({ source: parsed.source, output: parsed.output, platform: parsed.platform })
      let text: string
      if (parsed.json) text = JSON.stringify(report)
      else if (!report.created) {
        text = report.error.code + ': ' + report.error.message
        if (report.validation && hasCommandContext(report.validation)) {
          const validation = report.validation
          for (const issue of validation.issues) {
            text += '\n' + renderHumanIssue(issue, true)
          }
          if (validation.truncated && validation.issues.length > 0) {
            text += '\nAdditional issues were omitted.'
          }
        }
      }
      else {
        text = 'Created ' + report.output.path + '\nSHA-256: ' + report.output.sha256
        for (const warning of report.warnings ?? []) text += '\nWarning: ' + warning.code + ': ' + warning.message
      }
      return { exitCode: report.created ? 0 : 1, stdout: text + '\n' }
    }
    const report = await validatePackage({ source: parsed.source, platform: parsed.platform })
    return {
      exitCode: report.valid ? 0 : 1,
      stdout: `${parsed.json ? JSON.stringify(report) : renderHumanReport(report)}\n`,
    }
  } catch {
    return {
      exitCode: 2,
      stdout: `${renderCliError('CLI_INTERNAL', parsed.kind === 'pack' ? 'Packing failed.' : 'Validation failed.', parsed.json)}\n`,
    }
  }
}
