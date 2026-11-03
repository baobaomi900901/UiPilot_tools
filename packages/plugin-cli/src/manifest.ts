import validateSchema, { type StandaloneValidationError } from './generated/manifest-validator.mjs'
import { normalizeNfc } from './unicode.js'
import { withCommandDiagnostics } from './report.js'
import { StrictJsonError, parseStrictJson } from './strict-json.js'
import { PLUGIN_CLI_HOST_VERSION, type PluginPlatform, type PluginValidationIssueCode } from './contracts.js'

export type PublicPermission =
  | 'selection.read'
  | 'ui.window'
  | 'ui.panel'
  | 'speech.speak'
  | 'clipboard.write'
  | 'clipboard.paste'
  | 'credentials.youdaoTranslation'
  | 'credentials.baiduOcr'
  | 'credentials.deepseek'
  | 'url.open'
  | 'files.openText'
  | 'clipboard.read'
  | 'clipboard.history.read'
  | 'clipboard.history.paste'
  | 'clipboard.history.copy'
  | 'network.https'
  | 'files.userSelected'
  | 'files.index.readAll'
  | 'notifications.publish'
  | 'timer.control'
  | 'background.schedule'

export type PanelHostKeyDeclaration = 'ArrowDown' | 'ArrowUp' | 'Primary+N' | 'Tab' | 'Shift+Tab' | 'Enter' | 'Delete'
export type PanelHostKeyFocus = 'content' | 'host'

export interface PluginNetworkCredential {
  readonly id: string
  readonly settingKey: string
  readonly host: string
  readonly type: 'bearer'
}

export interface PublicNetworkV1 {
  httpsHosts: string[]
  credentials?: PluginNetworkCredential[]
}

interface PublicManifestBase {
  schemaVersion: number
  pluginId: string
  version: string
  apiVersion: number
  minimumHostVersion: string
  name: string
  description?: string | null
  supportedPlatforms: PluginPlatform[]
  runtime: { entry: string; resultActions?: boolean }
  window?: { entry: string; runtimeActions?: boolean } | null
  panel?: {
    entry: string
    hostKeys?: PanelHostKeyDeclaration[]
    hostKeyFocus?: PanelHostKeyFocus
    runtimeActions?: boolean
  } | null
  network?: PublicNetworkV1
  permissions: PublicPermission[]
  settings?: PublicSettingV1[]
}

export interface PublicCommandV1 {
  defaultName: string
  summary?: string | null
  activationMode: 'live' | 'submit'
  outputMode: 'mainResult' | 'window' | 'panel'
  inputRequired: boolean
  inputPlaceholder?: string | null
}

export interface PublicNamedCommandV1 extends PublicCommandV1 {
  id: string
  outputMode: 'mainResult'
}

export type PublicManifestV1 = PublicManifestBase & (
  | { command: PublicCommandV1; commands?: never }
  | { command?: never; commands: PublicNamedCommandV1[] }
)

export const MULTI_COMMAND_MIN_HOST = [0, 5, 0] as const
export const MAX_PUBLIC_COMMANDS = 16

export function normalizedCommands(manifest: PublicManifestV1): ReadonlyArray<{ commandId: string; command: PublicCommandV1 }> {
  return manifest.commands !== undefined
    ? manifest.commands.map((command) => ({ commandId: command.id, command }))
    : [{ commandId: 'default', command: manifest.command }]
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
  commandId?: string
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
  Tab: 3,
  'Shift+Tab': 4,
  Enter: 5,
  Delete: 6,
}

function canonicalPanelHostKeys(values: readonly PanelHostKeyDeclaration[]): PanelHostKeyDeclaration[] {
  return [...values].sort((left, right) => panelHostKeyOrder[left] - panelHostKeyOrder[right])
}

function canonicalHttpsHosts(values: readonly string[]): string[] {
  return [...values].sort((left, right) => Buffer.from(left).compare(Buffer.from(right)))
}

function isIpv4Literal(value: string): boolean {
  const labels = value.split('.')
  return labels.length === 4 && labels.every(
    (label) => /^[0-9]+$/u.test(label) && Number(label) <= 255,
  )
}

function validHttpsHost(value: string): boolean {
  if (
    value.length === 0 ||
    Buffer.byteLength(value, 'utf8') > 253 ||
    !/^[\x00-\x7f]+$/u.test(value) ||
    value !== value.toLowerCase() ||
    !value.includes('.') ||
    value === 'localhost' ||
    value.endsWith('.localhost') ||
    value.endsWith('.local') ||
    isIpv4Literal(value) ||
    value.includes(':')
  ) {
    return false
  }
  return value.split('.').every(
    (label) =>
      label.length >= 1 &&
      Buffer.byteLength(label, 'ascii') <= 63 &&
      !label.startsWith('-') &&
      !label.endsWith('-') &&
      !label.startsWith('xn--') &&
      /^[a-z0-9-]+$/u.test(label),
  )
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

function validCommand(command: PublicCommandV1): boolean {
  return /^[a-z][a-z0-9-]{0,31}$/u.test(command.defaultName) &&
    (command.summary == null || (command.summary.trim().length > 0 && [...command.summary].length <= 512 && !/\p{Cc}/u.test(command.summary))) &&
    (!command.inputRequired || !!command.inputPlaceholder?.trim()) &&
    (command.inputPlaceholder == null || plainText(command.inputPlaceholder))
}

function sharedSemanticPointers(manifest: PublicManifestV1): string[] {
  const settings = manifest.settings ?? []
  const rules: Array<readonly [boolean, string]> = [
    [manifest.schemaVersion !== 1, '/schemaVersion'],
    [!/^[a-z0-9](?:[a-z0-9.-]{0,62}[a-z0-9])?$/u.test(manifest.pluginId), '/pluginId'],
    [!canonicalVersion(manifest.version), '/version'],
    [!nonemptyPlainText(manifest.name), '/name'],
    [manifest.description != null && !plainText(manifest.description), '/description'],
    [manifest.supportedPlatforms.length === 0 || duplicates(manifest.supportedPlatforms), '/supportedPlatforms'],
    [!validEntry(manifest.runtime.entry, 'js'), '/runtime/entry'],
    [manifest.window != null && !validEntry(manifest.window.entry, 'html'), '/window/entry'],
    [manifest.panel != null && !validEntry(manifest.panel.entry, 'html'), '/panel/entry'],
    [manifest.panel?.hostKeys != null && (manifest.panel.hostKeys.length > 8 || duplicates(manifest.panel.hostKeys)), '/panel/hostKeys'],
    [manifest.network != null && (manifest.network.httpsHosts.length === 0 || manifest.network.httpsHosts.length > 8 ||
      duplicates(manifest.network.httpsHosts) || manifest.network.httpsHosts.some(host => !validHttpsHost(host))), '/network/httpsHosts'],
    [duplicates(manifest.permissions), '/permissions'],
    [settings.some(setting => !validSetting(setting)) || duplicates(settings.map(setting => setting.key)), '/settings'],
    [(manifest.network != null) !== manifest.permissions.includes('network.https'), '/network'],
  ]
  if (manifest.network?.credentials) {
    const credentials = manifest.network.credentials
    rules.push([credentials.length === 0 || credentials.length > 8 ||
      duplicates(credentials.map(credential => credential.id)) || credentials.some(credential =>
        !validSettingKey(credential.id) || !validSettingKey(credential.settingKey) || !validHttpsHost(credential.host) ||
        !manifest.network!.httpsHosts.includes(credential.host) ||
        !settings.some(setting => setting.key === credential.settingKey && setting.type === 'secret'),
      ), '/network/credentials'])
  }
  return rules.filter(([invalid]) => invalid).map(([, pointer]) => pointer)
}

function semanticValid(manifest: PublicManifestV1): boolean {
  const permissions = manifest.permissions
  const hasClipboardHistoryRead = permissions.includes('clipboard.history.read')
  const hasClipboardHistoryPaste = permissions.includes('clipboard.history.paste')
  const hasClipboardHistoryCopy = permissions.includes('clipboard.history.copy')
  if (sharedSemanticPointers(manifest).length > 0 ||
    normalizedCommands(manifest).some(({ command }) => !validCommand(command))) return false
  const command = manifest.command
  if (command) {
    if (command.outputMode === 'window') {
      if (
        command.activationMode !== 'submit' ||
        manifest.window == null ||
        manifest.panel != null ||
        !permissions.includes('ui.window') ||
        permissions.includes('ui.panel')
      ) {
        return false
      }
    } else if (command.outputMode === 'panel') {
      if (
        command.activationMode !== 'submit' ||
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
      (command.activationMode !== 'submit' ||
        command.outputMode !== 'window' ||
        manifest.window == null ||
        manifest.panel != null ||
        !permissions.includes('ui.window') ||
        permissions.includes('ui.panel') ||
        !permissions.includes('notifications.publish'))
    ) {
      return false
    }
    if (
      (hasClipboardHistoryRead || hasClipboardHistoryPaste || hasClipboardHistoryCopy) &&
      (command.activationMode !== 'submit' ||
        command.outputMode !== 'panel' ||
        manifest.panel == null ||
        manifest.window != null ||
        !permissions.includes('ui.panel') ||
        permissions.includes('ui.window') ||
        ((hasClipboardHistoryPaste || hasClipboardHistoryCopy) && !hasClipboardHistoryRead))
    ) {
      return false
    }
  }
  return true
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

// Count the whole raw array first: no occurrence of a duplicate may claim identity,
// even while schema errors prevent exposing declaration metadata.
function diagnosticCommandIds(value: unknown): Array<string | undefined> {
  if (!record(value) || !Array.isArray(value.commands)) return []
  const ids = value.commands.map((command: unknown) => record(command) && typeof command.id === 'string' &&
    /^[a-z][a-z0-9-]{0,31}$/u.test(command.id) ? command.id : undefined)
  const counts = new Map<string, number>()
  for (const id of ids) if (id !== undefined) counts.set(id, (counts.get(id) ?? 0) + 1)
  return ids.map(id => id !== undefined && counts.get(id) === 1 ? id : undefined)
}

function namedCommandIssues(manifest: PublicManifestV1 & { commands: PublicNamedCommandV1[] }): ManifestValidationIssue[] {
  const identities = diagnosticCommandIds(manifest)
  const names = new Map<string, number>()
  for (const command of manifest.commands) names.set(command.defaultName, (names.get(command.defaultName) ?? 0) + 1)
  const issues: ManifestValidationIssue[] = []
  manifest.commands.forEach((command, index) => {
    const rules: Array<readonly [boolean, string, string]> = [
      [identities[index] === undefined, 'id', 'Command ID must be valid and unique.'],
      [!/^[a-z][a-z0-9-]{0,31}$/u.test(command.defaultName), 'defaultName', 'Command defaultName is invalid.'],
      [(names.get(command.defaultName) ?? 0) > 1, 'defaultName', 'Command defaultName must be unique.'],
      [command.summary != null && (command.summary.trim().length === 0 || [...command.summary].length > 512 ||
        /\p{Cc}/u.test(command.summary)), 'summary', 'Command summary must be nonempty, at most 512 Unicode scalars, and contain no control characters.'],
      [command.inputRequired && !command.inputPlaceholder?.trim(), 'inputPlaceholder', 'A command requiring input must have a nonempty placeholder.'],
      [command.inputPlaceholder != null && !plainText(command.inputPlaceholder), 'inputPlaceholder', 'Command inputPlaceholder must be plain text.'],
    ]
    for (const [invalid, field, message] of rules) if (invalid) issues.push({
      code: 'MANIFEST_SEMANTIC_INVALID', message,
      location: { kind: 'jsonPointer', value: `/commands/${index}/${field}` },
      ...(identities[index] !== undefined ? { commandId: identities[index] } : {}),
    })
  })
  for (const pointer of sharedSemanticPointers(manifest)) issues.push({
    code: 'MANIFEST_SEMANTIC_INVALID', message: 'Manifest fields or output relationships are invalid.',
    location: { kind: 'jsonPointer', value: pointer },
  })
  return issues
}

function schemaIssue(error: StandaloneValidationError, identities: readonly (string | undefined)[]): ManifestValidationIssue {
  let pointer = error.instancePath || ''
  // Preserve legacy pointers/messages; only new-form item errors gain field precision.
  const item = /^\/commands\/(0|[1-9][0-9]*)(?:\/|$)/u.exec(pointer)
  if (item && (error.keyword === 'required' || error.keyword === 'additionalProperties')) {
    const field = error.params[error.keyword === 'required' ? 'missingProperty' : 'additionalProperty']
    if (typeof field === 'string') pointer += '/' + field.replaceAll('~', '~0').replaceAll('/', '~1')
  }
  const commandId = item ? identities[Number(item[1])] : undefined
  return {
    code: 'MANIFEST_SCHEMA_INVALID',
    message: error.message ? `Manifest ${error.message}.` : 'Manifest does not match Schema.',
    location: { kind: 'jsonPointer', value: pointer },
    ...(commandId !== undefined ? { commandId } : {}),
  }
}

function permissionAvailable(permission: PublicPermission, platform: PluginPlatform): boolean {
  return (
    permission === 'ui.window' ||
    permission === 'ui.panel' ||
    permission === 'clipboard.write' ||
    ((permission === 'clipboard.history.read' || permission === 'clipboard.history.paste' || permission === 'clipboard.history.copy') && platform === 'windows') ||
    ((permission === 'speech.speak' || permission === 'selection.read' || permission.startsWith('credentials.') || permission === 'clipboard.paste' || permission === 'network.https' || permission === 'url.open' || permission === 'files.openText') && platform === 'windows') ||
    ((permission === 'notifications.publish' || permission === 'timer.control') && platform === 'windows')
  )
}

export function validateManifest(
  bytes: Uint8Array,
  platform: PluginPlatform,
  hostVersion: readonly [number, number, number] = canonicalVersion(PLUGIN_CLI_HOST_VERSION)!,
): ManifestValidationResult {
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
  const result = validateParsedManifest(value, platform, hostVersion)
  return withCommandDiagnostics(result, record(value) && Object.hasOwn(value, 'commands'))
}

function validateParsedManifest(
  value: unknown,
  platform: PluginPlatform,
  hostVersion: readonly [number, number, number],
): ManifestValidationResult {
  if (!validateSchema(value)) {
    const identities = diagnosticCommandIds(value)
    return { ok: false, issues: (validateSchema.errors ?? []).map(error => schemaIssue(error, identities)) }
  }

  const manifest = value as PublicManifestV1
  const issues: ManifestValidationIssue[] = []
  if (manifest.commands !== undefined) {
    issues.push(...namedCommandIssues(manifest as PublicManifestV1 & { commands: PublicNamedCommandV1[] }))
  } else if (!semanticValid(manifest)) {
    issues.push({
      code: 'MANIFEST_SEMANTIC_INVALID',
      message: 'Manifest fields or output relationships are invalid.',
    })
  }
  if (!manifest.supportedPlatforms.includes(platform)) {
    issues.push({ code: 'PLATFORM_INCOMPATIBLE', message: `Plugin does not support ${platform}.`,
      ...(manifest.commands !== undefined ? { location: { kind: 'jsonPointer' as const, value: '/supportedPlatforms' } } : {}),
    })
  }
  const minimumHost = canonicalVersion(manifest.minimumHostVersion)
  if (
    manifest.apiVersion !== 1 ||
    !minimumHost ||
    (manifest.permissions.includes('files.openText') && (versionGreater([0, 13, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows' || !manifest.permissions.includes('ui.window') || !manifest.window || manifest.command?.activationMode !== 'submit' || manifest.command?.outputMode !== 'window' || manifest.commands !== undefined)) ||
    (manifest.permissions.includes('speech.speak') && (versionGreater([0, 18, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows' || !manifest.permissions.includes('ui.panel') || !manifest.panel || manifest.command?.activationMode !== 'submit' || manifest.command?.outputMode !== 'panel' || manifest.commands !== undefined)) ||
    (manifest.permissions.includes('clipboard.history.copy') && (versionGreater([0, 17, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.permissions.includes('selection.read') && (versionGreater([0, 16, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.permissions.some(permission => permission.startsWith('credentials.')) && (versionGreater([0, 15, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.permissions.includes('clipboard.paste') && (versionGreater([0, 14, 0], minimumHost) || !manifest.runtime.resultActions || !manifest.permissions.includes('clipboard.write') || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.permissions.includes('url.open') && (versionGreater([0, 12, 0], minimumHost) || !manifest.runtime.resultActions || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.runtime.resultActions === true && (versionGreater([0, 10, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows' || normalizedCommands(manifest).some(({ command }) => command.outputMode !== 'mainResult'))) ||
    (manifest.window?.runtimeActions === true && (versionGreater([0, 9, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    (manifest.panel?.runtimeActions === true && (versionGreater([0, 6, 0], minimumHost) || manifest.supportedPlatforms.length !== 1 || manifest.supportedPlatforms[0] !== 'windows')) ||
    versionGreater(minimumHost, hostVersion) ||
    (manifest.commands !== undefined && versionGreater(MULTI_COMMAND_MIN_HOST, minimumHost)) ||
    (manifest.command?.outputMode === 'panel' && versionGreater([0, 3, 0], minimumHost)) ||
    ((manifest.panel?.hostKeys?.length ?? 0) > 0 && versionGreater([0, 3, 1], minimumHost)) ||
    ((manifest.panel?.hostKeys ?? []).some((key) => key === 'Tab' || key === 'Shift+Tab' || key === 'Enter') &&
      versionGreater([0, 3, 3], minimumHost)) ||
    ((manifest.panel?.hostKeys ?? []).includes('Delete') && versionGreater([0, 16, 0], minimumHost)) ||
    (manifest.panel?.hostKeyFocus !== undefined && versionGreater([0, 3, 4], minimumHost)) ||
    ((manifest.permissions.includes('clipboard.history.read') ||
      manifest.permissions.includes('clipboard.history.paste')) &&
      versionGreater([0, 3, 3], minimumHost)) ||
    (manifest.network?.credentials !== undefined && versionGreater([0, 4, 0], minimumHost)) ||
    (manifest.network != null && versionGreater([0, 3, 2], minimumHost))
  ) {
    issues.push({ code: 'API_INCOMPATIBLE', message: 'Plugin API or minimum host version is incompatible.',
      ...(manifest.commands !== undefined ? { location: { kind: 'jsonPointer' as const, value: manifest.apiVersion !== 1 ? '/apiVersion' : '/minimumHostVersion' } } : {}),
    })
  }
  if (manifest.permissions.some((permission) => !permissionAvailable(permission, platform))) {
    issues.push({ code: 'PERMISSION_UNSUPPORTED', message: `Plugin declares a permission unavailable on ${platform}.`,
      ...(manifest.commands !== undefined ? { location: { kind: 'jsonPointer' as const, value: '/permissions' } } : {}),
    })
  }
  if (issues.length > 0) return { ok: false, manifest, issues }
  if (manifest.panel?.hostKeys) manifest.panel.hostKeys = canonicalPanelHostKeys(manifest.panel.hostKeys)
  if (manifest.network) {
    manifest.network.httpsHosts = canonicalHttpsHosts(manifest.network.httpsHosts)
    manifest.network.credentials?.sort((a, b) => Buffer.from(a.id).compare(Buffer.from(b.id)))
  }
  return { ok: true, manifest, issues: [] }
}
