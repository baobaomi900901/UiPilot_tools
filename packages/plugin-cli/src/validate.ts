import { CssValidationError, validateCssReferences } from './css-validator.js'
import type { PluginPlatform, PluginValidationReportV1 } from './contracts.js'
import { validateManifest, type ManifestValidationIssue, type PublicManifestV1 } from './manifest.js'
import type { PackageSnapshot } from './package-policy.js'
import { PngValidationError, validatePluginIcon } from './png-validator.js'
import { IssueCollector } from './report.js'
import { validateTimerAlarmWav, WavValidationError } from './wav-validator.js'

const ALARM_PATH = 'assets/sounds/timer-alarm.wav'

function manifestPhase(issue: ManifestValidationIssue): number {
  if (issue.code === 'MANIFEST_JSON_INVALID') return 20
  if (issue.code === 'MANIFEST_SCHEMA_INVALID') return 30
  if (issue.code === 'MANIFEST_SEMANTIC_INVALID') return 40
  return 50
}

function validateEntries(
  manifest: PublicManifestV1,
  files: ReadonlyMap<string, Buffer>,
  issues: IssueCollector,
): void {
  const entries = [manifest.runtime.entry]
  if (manifest.window) entries.push(manifest.window.entry)
  entries.forEach((path, index) => {
    if (!files.has(path)) {
      issues.add({
        phaseRank: 60,
        ruleRank: index,
        code: 'ENTRY_MISSING',
        path,
        message: `Declared entry does not exist: ${path}`,
      })
    }
  })
}

function validateAlarm(
  manifest: PublicManifestV1,
  files: ReadonlyMap<string, Buffer>,
  issues: IssueCollector,
): void {
  const declared = manifest.permissions.includes('timer.control')
  const alarm = files.get(ALARM_PATH)
  if (declared !== !!alarm) {
    issues.add({
      phaseRank: 60,
      ruleRank: 20,
      code: 'RESOURCE_INVALID',
      path: ALARM_PATH,
      message: declared
        ? 'timer.control requires the fixed timer alarm WAV.'
        : 'The timer alarm WAV requires timer.control.',
    })
    return
  }
  if (!alarm) return
  try {
    validateTimerAlarmWav(alarm)
  } catch (error) {
    if (!(error instanceof WavValidationError)) throw error
    issues.add({
      phaseRank: 60,
      ruleRank: 21,
      code: 'RESOURCE_INVALID',
      path: ALARM_PATH,
      message: error.message,
    })
  }
}

function validateIndependentResources(snapshot: PackageSnapshot, issues: IssueCollector): void {
  const icon = snapshot.files.get('icon.png')
  if (icon) {
    try {
      validatePluginIcon(icon)
    } catch (error) {
      if (!(error instanceof PngValidationError)) throw error
      issues.add({
        phaseRank: 70,
        ruleRank: 1,
        code: 'ICON_INVALID',
        path: 'icon.png',
        message: error.message,
      })
    }
  }

  const publicResources = new Set(snapshot.files.keys())
  publicResources.delete(ALARM_PATH)
  for (const [path, bytes] of snapshot.files) {
    if (!path.endsWith('.css')) continue
    try {
      validateCssReferences(path, bytes, publicResources)
    } catch (error) {
      if (!(error instanceof CssValidationError)) throw error
      issues.add({
        phaseRank: 80,
        ruleRank: 1,
        code: 'CSS_REFERENCE_INVALID',
        path,
        location: { kind: 'byteOffset', value: String(error.byteOffset) },
        message: error.message,
      })
    }
  }
}

export function validateSnapshot(
  snapshot: PackageSnapshot,
  sourcePath: string,
  platform: PluginPlatform,
): PluginValidationReportV1 {
  const issues = new IssueCollector()
  const manifestBytes = snapshot.files.get('plugin.json')
  let manifest: PublicManifestV1 | undefined
  if (!manifestBytes) {
    issues.add({
      phaseRank: 20,
      ruleRank: 0,
      code: 'MANIFEST_MISSING',
      path: 'plugin.json',
      message: 'plugin.json is missing from the package root.',
    })
  } else {
    const result = validateManifest(manifestBytes, platform)
    manifest = result.manifest
    result.issues.forEach((issue, index) => {
      issues.add({
        phaseRank: manifestPhase(issue),
        ruleRank: index,
        code: issue.code,
        path: 'plugin.json',
        location: issue.location,
        message: issue.message,
      })
    })
    if (manifest) {
      validateEntries(manifest, snapshot.files, issues)
      validateAlarm(manifest, snapshot.files, issues)
    }
  }
  validateIndependentResources(snapshot, issues)
  const finished = issues.finish()
  return {
    schemaVersion: 1,
    valid: finished.issues.length === 0,
    source: { kind: snapshot.kind, path: sourcePath },
    target: { platform, hostVersion: '0.2.0', apiVersion: 1 },
    ...(manifest
      ? {
          plugin: {
            pluginId: manifest.pluginId,
            version: manifest.version,
            outputMode: manifest.command.outputMode,
          },
        }
      : {}),
    truncated: finished.truncated,
    issues: finished.issues,
  }
}
