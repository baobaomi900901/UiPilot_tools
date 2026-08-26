import validateSchema, { type StandaloneValidationError } from './generated/manifest-validator.mjs'
import { normalizeNfc } from './unicode.js'
import { StrictJsonError, parseStrictJson } from './strict-json.js'
import type { PluginPlatform, PluginValidationIssueCode } from './contracts.js'

export type PublicPermission =
  | 'ui.window'
  | 'ui.panel'
  | 'clipboard.write'
  | 'clipboard.read'
  | 'network.https'
  | 'files.userSelected'
  | 'files.index.readAll'
  | 'notifications.publish'
  | 'timer.control'
  | 'background.schedule'

export type PanelHostKeyDeclaration = 'ArrowDown' | 'ArrowUp' | 'Primary+N'

export interface PublicManifestV1 {
  schemaVersion: number
  pluginId: string
  version: string
  apiVersion: number
  minimumHostVersion: string
  name: string
  description?: string | null
  supportedPlatforms: PluginPlatform[]
  command: {
    defaultName: string
    summary?: string | null
    activationMode: 'live' | 'submit'
    outputMode: 'mainResult' | 'window' | 'panel'
    inputRequired: boolean
    inputPlaceholder?: string | null
  }
  runtime: { entry: string }
  window?: { entry: string } | null
  panel?: { entry: string; hostKeys?: PanelHostKeyDeclaration[] } | null
  permissions: PublicPermission[]
  settings?: PublicSettingV1[]
}

type PublicSettingV1 =
  | { type: 'text'; key: string; label: string; default?: string | null }
  | { type: 'secret'; key: string; label: string }
  | {
      type: 'number'
      key: string
      label: string
      default?: number | null
      min?: number | null
      max?: number | null
      step?: number | null
    }
  | { type: 'boolean'; key: string; label: string; default?: boolean | null }
  | {
      type: 'select'
      key: string
      label: string
      default?: string | null
      options: Array<{ value: string; label: string }>
    }

export interface ManifestValidationIssue {
  code: Extract<
    PluginValidationIssueCode,
    | 'MANIFEST_JSON_INVALID'
    | 'MANIFEST_SCHEMA_INVALID'
    | 'MANIFEST_SEMANTIC_INVALID'
    | 'PLATFORM_INCOMPATIBLE'
    | 'API_INCOMPATIBLE'
    | 'PERMISSION_UNSUPPORTED'
  >
  message: string
  location?: { kind: 'jsonPointer' | 'byteOffset' | 'name'; value: string }
}

export type ManifestValidationResult =
  | { ok: true; manifest: PublicManifestV1; issues: [] }
  | { ok: false; manifest?: PublicManifestV1; issues: ManifestValidationIssue[] }

function plainText(value: string): boolean {
  return ![...value].some(
    (character) => /\p{Cc}/u.test(character) && character !== '\r' && character !== '\n' && character !== '\t',
  )
}

function nonemptyPlainText(value: string): boolean {
  return value.trim().length > 0 && plainText(value)
}

function canonicalVersion(value: string): readonly [number, number, number] | undefined {
  const parts = value.split('.')
  if (parts.length !== 3) return undefined
  const numbers = parts.map((part) => {
    if (!/^(?:0|[1-9][0-9]*)$/u.test(part)) return undefined
    const number = Number(part)
    return Number.isInteger(number) && number <= 0xffffffff ? number : undefined
  })
  if (numbers.some((number) => number === undefined)) return undefined
  return numbers as [number, number, number]
}

function versionGreater(left: readonly number[], right: readonly number[]): boolean {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] > right[index]
  }
  return false
}

function validEntry(value: string, extension: 'js' | 'html'): boolean {
  if (
    !value ||
    value.startsWith('/') ||
    value.includes('\\') ||
    Buffer.byteLength(value, 'utf8') > 240 ||
    value.split('/').length > 8
  ) {
    return false
  }
  const components = value.split('/')
  if (
    components.some(
      (component) =>
        !component ||
        component === '.' ||
        component === '..' ||
        Buffer.byteLength(component, 'utf8') > 100 ||
        component.endsWith('.') ||
        component.endsWith(' ') ||
        component.includes(':') ||
        normalizeNfc(component) !== component,
    )
  ) {
    return false
  }
  const parts = components.at(-1)!.split('.')
  return parts.length === 2 && parts[0].length > 0 && parts[1] === extension
}

function duplicates(values: readonly unknown[]): boolean {
  return new Set(values).size !== values.length
}

const panelHostKeyOrder: Readonly<Record<PanelHostKeyDeclaration, number>> = {
  ArrowDown: 0,
  ArrowUp: 1,
  'Primary+N': 2,
}

function canonicalPanelHostKeys(values: readonly PanelHostKeyDeclaration[]): PanelHostKeyDeclaration[] {
  return [...values].sort((left, right) => panelHostKeyOrder[left] - panelHostKeyOrder[right])
}

function validSettingKey(value: string): boolean {
  return /^[a-z][a-z0-9.-]{0,63}$/u.test(value)
}

function validSetting(setting: PublicSettingV1): boolean {
  if (!validSettingKey(setting.key) || !nonemptyPlainText(setting.label)) return false
  if (setting.type === 'text') return setting.default == null || plainText(setting.default)
  if (setting.type === 'secret' || setting.type === 'boolean') return true
  if (setting.type === 'number') {
    const values = [setting.default, setting.min, setting.max, setting.step].filter(
      (value): value is number => value != null,
    )
    return (
      values.every(Number.isFinite) &&
      (setting.min == null || setting.max == null || setting.min <= setting.max) &&
      (setting.step == null || setting.step > 0) &&
      (setting.default == null || setting.min == null || setting.default >= setting.min) &&
      (setting.default == null || setting.max == null || setting.default <= setting.max)
    )
  }
  const values = new Set<string>()
  return (
    setting.options.length > 0 &&
    setting.options.every(
      (option) =>
        plainText(option.value) && nonemptyPlainText(option.label) && !values.has(option.value) && !!values.add(option.value),
    ) &&
    (setting.default == null || values.has(setting.default))
  )
}

function semanticValid(manifest: PublicManifestV1): boolean {
  const settings = manifest.settings ?? []
  const permissions = manifest.permissions
  if (
    manifest.schemaVersion !== 1 ||
    !/^[a-z0-9](?:[a-z0-9.-]{0,62}[a-z0-9])?$/u.test(manifest.pluginId) ||
    !canonicalVersion(manifest.version) ||
    !nonemptyPlainText(manifest.name) ||
    (manifest.description != null && !plainText(manifest.description)) ||
    manifest.supportedPlatforms.length === 0 ||
    duplicates(manifest.supportedPlatforms) ||
    !/^[a-z][a-z0-9-]{0,31}$/u.test(manifest.command.defaultName) ||
    (manifest.command.summary != null &&
      (manifest.command.summary.trim().length === 0 ||
        [...manifest.command.summary].length > 512 ||
        /\p{Cc}/u.test(manifest.command.summary))) ||
    (manifest.command.inputRequired && !manifest.command.inputPlaceholder?.trim()) ||
    (manifest.command.inputPlaceholder != null && !plainText(manifest.command.inputPlaceholder)) ||
    !validEntry(manifest.runtime.entry, 'js') ||
    (manifest.window != null && !validEntry(manifest.window.entry, 'html')) ||
    (manifest.panel != null && !validEntry(manifest.panel.entry, 'html')) ||
    (manifest.panel?.hostKeys != null &&
      (manifest.panel.hostKeys.length > 8 || duplicates(manifest.panel.hostKeys))) ||
    duplicates(permissions) ||
    settings.some((setting) => !validSetting(setting)) ||
    duplicates(settings.map((setting) => setting.key))
  ) {
    return false
  }
  if (manifest.command.outputMode === 'window') {
    if (
      manifest.command.activationMode !== 'submit' ||
      manifest.window == null ||
      manifest.panel != null ||
      !permissions.includes('ui.window') ||
      permissions.includes('ui.panel')
    ) {
      return false
    }
  } else if (manifest.command.outputMode === 'panel') {
    if (
      manifest.command.activationMode !== 'submit' ||
      manifest.panel == null ||
      manifest.window != null ||
      !permissions.includes('ui.panel') ||
      permissions.includes('ui.window') ||
      permissions.includes('timer.control')
    ) {
      return false
    }
  } else if (
    manifest.window != null ||
    manifest.panel != null ||
    permissions.includes('ui.window') ||
    permissions.includes('ui.panel')
  ) {
    return false
  }
  if (
    permissions.includes('timer.control') &&
    (manifest.command.activationMode !== 'submit' ||
      manifest.command.outputMode !== 'window' ||
      manifest.window == null ||
      manifest.panel != null ||
      !permissions.includes('ui.window') ||
      permissions.includes('ui.panel') ||
      !permissions.includes('notifications.publish'))
  ) {
    return false
  }
  return true
}

function schemaIssue(error: StandaloneValidationError): ManifestValidationIssue {
  return {
    code: 'MANIFEST_SCHEMA_INVALID',
    message: error.message ? `Manifest ${error.message}.` : 'Manifest does not match Schema.',
    location: { kind: 'jsonPointer', value: error.instancePath || '' },
  }
}

function permissionAvailable(permission: PublicPermission, platform: PluginPlatform): boolean {
  return (
    permission === 'ui.window' ||
    permission === 'ui.panel' ||
    permission === 'clipboard.write' ||
    ((permission === 'notifications.publish' || permission === 'timer.control') && platform === 'windows')
  )
}

export function validateManifest(bytes: Uint8Array, platform: PluginPlatform): ManifestValidationResult {
  let value: unknown
  try {
    value = parseStrictJson(bytes)
  } catch (error) {
    const byteOffset = error instanceof StrictJsonError ? error.byteOffset : 0
    return {
      ok: false,
      issues: [
        {
          code: 'MANIFEST_JSON_INVALID',
          message: 'plugin.json is not strict JSON.',
          location: { kind: 'byteOffset', value: String(byteOffset) },
        },
      ],
    }
  }
  if (!validateSchema(value)) {
    return { ok: false, issues: (validateSchema.errors ?? []).map(schemaIssue) }
  }

  const manifest = value as PublicManifestV1
  const issues: ManifestValidationIssue[] = []
  if (!semanticValid(manifest)) {
    issues.push({
      code: 'MANIFEST_SEMANTIC_INVALID',
      message: 'Manifest fields or output relationships are invalid.',
    })
  }
  if (!manifest.supportedPlatforms.includes(platform)) {
    issues.push({ code: 'PLATFORM_INCOMPATIBLE', message: `Plugin does not support ${platform}.` })
  }
  const minimumHost = canonicalVersion(manifest.minimumHostVersion)
  if (
    manifest.apiVersion !== 1 ||
    !minimumHost ||
    versionGreater(minimumHost, [0, 3, 1]) ||
    (manifest.command.outputMode === 'panel' && versionGreater([0, 3, 0], minimumHost)) ||
    ((manifest.panel?.hostKeys?.length ?? 0) > 0 && versionGreater([0, 3, 1], minimumHost))
  ) {
    issues.push({ code: 'API_INCOMPATIBLE', message: 'Plugin API or minimum host version is incompatible.' })
  }
  if (manifest.permissions.some((permission) => !permissionAvailable(permission, platform))) {
    issues.push({ code: 'PERMISSION_UNSUPPORTED', message: `Plugin declares a permission unavailable on ${platform}.` })
  }
  if (issues.length > 0) return { ok: false, manifest, issues }
  if (manifest.panel?.hostKeys) manifest.panel.hostKeys = canonicalPanelHostKeys(manifest.panel.hostKeys)
  return { ok: true, manifest, issues: [] }
}
