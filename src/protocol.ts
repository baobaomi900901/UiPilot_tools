export interface ResultItem {
  resultId: string
  title: string
  subtitle?: string
  icon?: string
  detail?: string
  hasDefaultAction?: boolean
}
export interface SearchResponse {
  requestId: string
  items: ResultItem[]
  windowTransferToken?: string
}

export type ThemePreference = 'system' | 'dark' | 'light'

export interface SettingsView {
  hotkey: string
  autostart: boolean
  filePreviewEnabled: boolean
  theme: ThemePreference
}

export interface UserSettingsUpdate {
  hotkey: string
  autostart: boolean
  theme: ThemePreference
}

export interface HotkeySettingsUpdate {
  hotkey: string
}

export interface HotkeySettingsView {
  hotkey: string
}

export type InstalledPluginView =
  | { state: 'absent' }
  | { state: 'valid'; activeVersion: string; versions: string[]; trigger: string }
  | { state: 'invalid'; issue: string; activeVersion: string | null; versions: string[] }

export type DevelopmentPluginView =
  | { state: 'absent' }
  | { state: 'valid'; version: string; trigger: string }
  | { state: 'invalid'; reason: string }

export type PluginDescriptionView =
  | { state: 'unavailable' }
  | { state: 'available'; source: 'installed' | 'development'; markdown: string }

export interface PluginInventoryView {
  key: string
  id: string | null
  displayName: string
  installed: InstalledPluginView
  development: DevelopmentPluginView
  description: PluginDescriptionView
}

export interface PluginInventorySnapshot {
  revision: string
  items: PluginInventoryView[]
}

export type PublicPluginFault = 'runtimeUnavailable' | 'consecutiveFailures'
export type PublicPermission =
  | 'ui.window'
  | 'clipboard.write'
  | 'clipboard.read'
  | 'network.https'
  | 'files.userSelected'
  | 'files.index.readAll'
  | 'notifications.publish'
  | 'background.schedule'

export interface PublicPermissionView {
  permission: PublicPermission
  supported: boolean
  granted: boolean
}

export type PublicSettingDefinition =
  | { type: 'text'; key: string; label: string; default?: string }
  | { type: 'secret'; key: string; label: string }
  | { type: 'number'; key: string; label: string; default?: number; min?: number; max?: number; step?: number }
  | { type: 'boolean'; key: string; label: string; default?: boolean }
  | { type: 'select'; key: string; label: string; options: readonly { value: string; label: string }[]; default?: string }

export interface PublicSettingView {
  definition: PublicSettingDefinition
  value?: string | number | boolean
  secretConfigured?: boolean
}

export interface PublicPluginInventoryItem {
  pluginId: string
  name: string
  description: string | null
  version: string
  source: 'localPackage'
  defaultName: string
  effectiveName: string
  enabled: boolean
  fault: PublicPluginFault | null
  generation: number
  permissions: readonly PublicPermissionView[]
  settings: readonly PublicSettingView[]
}

export interface PublicPluginInventory {
  revision: string
  items: readonly PublicPluginInventoryItem[]
}

export interface PublicPluginPrepareSummary {
  token: string
  pluginId: string
  name: string
  version: string
  permissions: readonly PublicPermission[]
  isUpdate: boolean
  sourceVerified: boolean
}
export interface PluginMutationOutcome {
  revision: string
}

export type ExecuteOutcome =
  | { status: 'launchRequested' }
  | { status: 'activationRequested' }
  | { status: 'activationRefusedLaunchRequested'; message: string }
  | { status: 'textCopied' }
  | { status: 'fileRevealRequested' }
  | { status: 'folderOpenRequested' }

export type CommandErrorCode =
  | 'invalidCaller'
  | 'staleRequest'
  | 'unknownResult'
  | 'applicationEntryUnavailable'
  | 'settingsFailed'
  | 'windowFailed'
  | 'invalidFileQuery'
  | 'fileSearchWorkerFailed'
  | 'searchUnavailable'
  | 'clipboardWriteFailed'
  | 'pluginPermissionDenied'
  | 'pluginListFailed'
  | 'pluginInstallFailed'
  | 'pluginReloadFailed'
  | 'pluginDeleteFailed'
  | 'fileNotFound'
  | 'fileOpenFailed'

export interface CommandError {
  code: CommandErrorCode
  message: string
}

export type ShowTarget = 'launcher' | 'settings' | 'messages'
export type LifecycleNotice = 'settingsFailed'

export interface LauncherShown {
  invocationId: string
  target: ShowTarget
  notice: LifecycleNotice | null
}

export interface MessageSummary {
  revision: U64Decimal
  unreadCount: number
}

export interface MessageView {
  id: U64Decimal
  pluginId: string
  pluginNameSnapshot: string
  pluginIconUrl: string | null
  createdAt: string
  content: string
  readAt: string | null
}

export interface MessageCenterSnapshot extends MessageSummary {
  messages: readonly MessageView[]
}

export type MessageHostStateChanged =
  | { status: 'ready'; revision: U64Decimal; unreadCount: number }
  | { status: 'unavailable'; error: 'MessageStoreUnavailable' }

export type MessageHostCommandError =
  | { code: 'MessageOperationFailed'; storeStatus: 'ready' }
  | { code: 'MessageStoreUnavailable'; storeStatus: 'unavailable' }

export type ControlKey = number

export type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'compositionBoundary'; control: ControlKey }

export interface LauncherClient {
  listenShown(handler: (payload: unknown) => void): Promise<() => void>
  listenMessageStateChanged(handler: (payload: unknown) => void): Promise<() => void>
  getMessageSummary(): Promise<unknown>
  openMessageCenter(): Promise<unknown>
  readMessageCenter(): Promise<unknown>
  clearMessages(): Promise<unknown>
  searchApps(input: { query: string; invocationId: string; querySequence: number; submit?: boolean }): Promise<SearchResponse | null>
  openFind(input: OpenFindInput): Promise<OpenFindOutcome>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  commitPluginWindowTransfer(input: { transferToken: string }): Promise<void>
  listPublicPlugins(): Promise<PublicPluginInventory>
  selectPublicPluginArchive(): Promise<string | null>
  selectPublicPluginDirectory(): Promise<string | null>
  preparePublicPlugin(input: { source: { kind: 'archive' | 'developmentDirectory'; path: string } }): Promise<PublicPluginPrepareSummary>
  commitPublicPlugin(input: { input: { token: string; permissionGrants: readonly PublicPermission[] } }): Promise<void>
  cancelPublicPlugin(input: { token: string }): Promise<void>
  setPublicPluginEnabled(input: { pluginId: string; enabled: boolean }): Promise<void>
  setPublicPluginEffectiveName(input: { pluginId: string; nameOverride: string | null }): Promise<void>
  savePublicPluginSettings(input: { input: { pluginId: string; settings: Readonly<Record<string, string | number | boolean>>; secrets: Readonly<Record<string, string | null>> } }): Promise<void>
  uninstallPublicPlugin(input: { pluginId: string; retainData: boolean }): Promise<void>
  listPlugins(): Promise<PluginInventorySnapshot>
  installPlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  reloadPlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  deletePlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  loadSettings(): Promise<SettingsView>
  saveSettings(input: { settings: UserSettingsUpdate }): Promise<void>
  saveHotkey(input: { hotkey: HotkeySettingsUpdate }): Promise<HotkeySettingsView>
  setThemePreference(input: { preference: { theme: ThemePreference } }): Promise<void>
  hideLauncher(): Promise<void>
}

export interface PluginWindowClient {
  setPinned(input: { pinned: boolean }): Promise<{ pinned: boolean }>
  close(): Promise<void>
}
export type U64Decimal = string & { readonly __u64Decimal: unique symbol }
export interface FindForwardPayload { invocationId: string; forwardSequence: U64Decimal; query: string }
export type OpenFindOutcome = { status: 'forwarded' } | { status: 'superseded' }
export interface OpenFindInput { query: string; invocationId: string; querySequence: number }
export interface FindInitializationPrepared {
  initializationToken: string
  themeRevision: U64Decimal
  theme: ThemePreference
  filePreviewRevision: U64Decimal
  filePreviewEnabled: boolean
  pinned: boolean
}
export type FindReadyOutcome =
  | { status: 'prepared'; initialization: FindInitializationPrepared }
  | { status: 'ready'; initializationToken: string }
  | { status: 'superseded' }
export interface FindPreviewPreferenceResult { filePreviewRevision: U64Decimal; filePreviewEnabled: boolean }
export interface FindThemeChanged { themeRevision: U64Decimal; theme: ThemePreference }
export interface FindClient {
  listenForward(handler: (payload: unknown) => void): Promise<() => void>
  listenThemeChanged(handler: (payload: unknown) => void): Promise<() => void>
  prepareInitialization(): Promise<unknown>
  commitReady(input: { initializationToken: string }): Promise<unknown>
  getReadyStatus(input: { initializationToken: string }): Promise<unknown>
  searchFiles(input: {
    query: string
    category: FileCategory
    sort: FileSort
    invocationId: string
    querySequence: number
  }): Promise<FileSearchResponse | null>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  setPinned(input: { invocationId: string; pinned: boolean }): Promise<{ pinned: boolean }>
  setPreviewPreference(input: { preference: { enabled: boolean } }): Promise<unknown>
  hide(input: { invocationId: string; force: boolean }): Promise<void>
}

export interface ViewResult {
  key: number
  title: string
  subtitle?: string
  icon?: string
  detail?: string
  hasDefaultAction?: boolean
}
export interface TextControlView {
  key: ControlKey
  value: string
}

export type SettingsLoadStatus = 'loading' | 'ready' | 'error'

export interface SettingsSnapshot {
  hotkey: TextControlView
  autostart: boolean
  theme: ThemePreference
  loadStatus: SettingsLoadStatus
  readOnly: boolean
  operation?: 'load' | 'save' | 'hotkey' | 'theme'
  needsReload: boolean
}

export type PluginListStatus = 'idle' | 'loading' | 'ready' | 'error'
export type PluginMutationKind = 'install' | 'reload' | 'delete'

export interface PluginItemSnapshot extends PluginInventoryView {
  operation?: PluginMutationKind
  error?: string
}

export interface PluginListSnapshot {
  status: PluginListStatus
  items: readonly PluginItemSnapshot[]
  error?: string
}

export type FileCategory = 'all' | 'folder' | 'excel' | 'word' | 'ppt' | 'pdf' | 'image' | 'video' | 'audio' | 'archive'
export type FileSort = 'modifiedDesc' | 'modifiedAsc'
export type FileIndexStatus = 'building' | 'ready' | 'partial' | 'rebuilding' | 'unavailable'
export type FileResultKind = 'file' | 'folder'

export interface FileResultItem {
  resultId: string
  name: string
  kind: FileResultKind
  sizeBytes: string | null
  modifiedUtc: string
  fullPath: string
}

export interface FileSearchResponse {
  requestId: string
  indexRevision: string
  total: string
  status: FileIndexStatus
  items: FileResultItem[]
}


export interface FileResultView {
  key: string
  name: string
  kind: FileResultKind
  sizeBytes: string | null
  modifiedUtc: string
  fullPath: string
}

export interface FileSnapshot {
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  preferencePending: boolean
  total: string
  indexStatus: FileIndexStatus
  results: readonly FileResultView[]
  selected?: FileResultView
}

export type SettingsTabKey = 'general' | 'plugins' | 'messages'

export interface MessageCenterStateSnapshot {
  readonly status: 'unknown' | 'ready' | 'unavailable'
  readonly unreadCount?: number
  readonly summaryRevision?: U64Decimal
  readonly snapshotRevision?: U64Decimal
  readonly messages: readonly MessageView[]
  readonly clearPending: boolean
  readonly operationError: boolean
}

export interface LauncherSnapshot {
  view: 'launcher' | 'settings'
  settingsTab: SettingsTabKey
  messageCenter: MessageCenterStateSnapshot
  viewEpoch: number
  theme: ThemePreference
  invocationId?: string
  queryControl: ControlKey
  query: string
  queryControlValue: string
  querySequence: number
  results: readonly ViewResult[]
  selectedIndex: number
  searchPending: boolean
  executePending: boolean
  hidePending: boolean
  shownNotice?: string
  status: string
  settingsLoadStatus?: SettingsLoadStatus
  settings?: SettingsSnapshot
  plugins?: PluginListSnapshot
  publicPlugins?: PublicPluginInventory
  file?: FileSnapshot
}

const shownKeys = ['invocationId', 'notice', 'target']

export function parseLauncherShown(value: unknown): LauncherShown | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  const prototype = Object.getPrototypeOf(value)
  if (prototype !== Object.prototype && prototype !== null) return null
  const candidate = value as Record<string, unknown>
  const keys = Object.keys(candidate).sort()
  if (keys.length !== shownKeys.length || keys.some((key, index) => key !== shownKeys[index])) return null
  if (typeof candidate.invocationId !== 'string') return null
  if (candidate.target !== 'launcher' && candidate.target !== 'settings' && candidate.target !== 'messages') return null
  if (candidate.notice !== null && candidate.notice !== 'settingsFailed') return null
  return candidate as unknown as LauncherShown
}

const U64_MAX = 18_446_744_073_709_551_615n
const DECIMAL_U64 = /^(0|[1-9][0-9]*)$/
const UTC_RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/
const fileStatuses = new Set<FileIndexStatus>(['building', 'ready', 'partial', 'rebuilding', 'unavailable'])

function plainRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  const prototype = Object.getPrototypeOf(value)
  return prototype === Object.prototype || prototype === null ? (value as Record<string, unknown>) : null
}

function exactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Reflect.ownKeys(value)
  if (keys.some((key) => typeof key !== 'string')) return false
  const sorted = (keys as string[]).sort()
  return sorted.length === expected.length && sorted.every((key, index) => key === expected[index])
}

function exactDenseArray(value: unknown[]): boolean {
  const keys = Object.getOwnPropertyNames(value)
  if (Object.getOwnPropertySymbols(value).length !== 0 || keys.length !== value.length + 1) return false
  return keys.every((key, index) => (index < value.length ? key === String(index) : key === 'length'))
}

const installedIssues = new Set([
  'stateMissing', 'stateMalformed', 'unsupportedStateSchema', 'stateInvariantViolation',
  'packageMissing', 'packageIdentityMismatch', 'packageDigestMismatch', 'packageInvalid',
  'transactionRecoveryRequired', 'migrationConflict', 'triggerConflict',
])
const developmentIssues = new Set([
  'invalidManifest', 'invalidId', 'invalidVersion', 'incompatibleHost', 'missingRuntime',
  'unsafePath', 'duplicateTrigger', 'resourceLimitExceeded', 'versionContentCollision',
])
const CANONICAL_VERSION = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/

function canonicalVersion(value: unknown): value is string {
  if (typeof value !== 'string') return false
  const match = CANONICAL_VERSION.exec(value)
  return match !== null && match.slice(1).every((part) => BigInt(part) <= 4_294_967_295n)
}

function parseVersions(value: unknown): string[] | null {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype || !exactDenseArray(value)) return null
  const versions: string[] = []
  for (const version of value) {
    if (!canonicalVersion(version) || versions.includes(version)) return null
    versions.push(version)
  }
  return versions
}

function parseInstalled(value: unknown): InstalledPluginView | null {
  const item = plainRecord(value)
  if (!item || typeof item.state !== 'string') return null
  if (item.state === 'absent') return exactKeys(item, ['state']) ? { state: 'absent' } : null
  if (item.state === 'valid') {
    if (!exactKeys(item, ['activeVersion', 'state', 'trigger', 'versions'])) return null
    const versions = parseVersions(item.versions)
    if (!canonicalVersion(item.activeVersion) || !versions?.includes(item.activeVersion) || typeof item.trigger !== 'string' || item.trigger.length === 0) return null
    return { state: 'valid', activeVersion: item.activeVersion, versions, trigger: item.trigger }
  }
  if (item.state === 'invalid') {
    if (!exactKeys(item, ['activeVersion', 'issue', 'state', 'versions'])) return null
    const versions = parseVersions(item.versions)
    if (!versions || typeof item.issue !== 'string' || !installedIssues.has(item.issue)) return null
    if (item.activeVersion !== null && !canonicalVersion(item.activeVersion)) return null
    return { state: 'invalid', issue: item.issue, activeVersion: item.activeVersion, versions }
  }
  return null
}

function parseDevelopment(value: unknown): DevelopmentPluginView | null {
  const item = plainRecord(value)
  if (!item || typeof item.state !== 'string') return null
  if (item.state === 'absent') return exactKeys(item, ['state']) ? { state: 'absent' } : null
  if (item.state === 'valid') {
    if (!exactKeys(item, ['state', 'trigger', 'version']) || !canonicalVersion(item.version) || typeof item.trigger !== 'string' || item.trigger.length === 0) return null
    return { state: 'valid', version: item.version, trigger: item.trigger }
  }
  if (item.state === 'invalid') {
    if (!exactKeys(item, ['reason', 'state']) || typeof item.reason !== 'string' || !developmentIssues.has(item.reason)) return null
    return { state: 'invalid', reason: item.reason }
  }
  return null
}

function parseDescription(value: unknown): PluginDescriptionView | null {
  const item = plainRecord(value)
  if (!item || typeof item.state !== 'string') return null
  if (item.state === 'unavailable') return exactKeys(item, ['state']) ? { state: 'unavailable' } : null
  if (item.state !== 'available' || !exactKeys(item, ['markdown', 'source', 'state'])) return null
  if ((item.source !== 'installed' && item.source !== 'development') || typeof item.markdown !== 'string') return null
  return { state: 'available', source: item.source, markdown: item.markdown }
}

function parsePluginInventoryView(value: unknown): PluginInventoryView | null {
  const item = plainRecord(value)
  if (!item || !exactKeys(item, ['description', 'development', 'displayName', 'id', 'installed', 'key'])) return null
  const installed = parseInstalled(item.installed)
  const development = parseDevelopment(item.development)
  const description = parseDescription(item.description)
  if (typeof item.key !== 'string' || item.key.length === 0 || typeof item.displayName !== 'string' || item.displayName.length === 0) return null
  if (item.id !== null && (typeof item.id !== 'string' || item.id.length === 0)) return null
  if (!installed || !development || !description || (item.id === null && development.state !== 'invalid')) return null
  return { key: item.key, id: item.id, displayName: item.displayName, installed, development, description }
}

export function parsePluginInventorySnapshot(value: unknown): PluginInventorySnapshot | null {
  const snapshot = plainRecord(value)
  if (!snapshot || !exactKeys(snapshot, ['items', 'revision']) || !canonicalU64(snapshot.revision)) return null
  if (!Array.isArray(snapshot.items) || Object.getPrototypeOf(snapshot.items) !== Array.prototype || !exactDenseArray(snapshot.items)) return null
  const keys = new Set<string>()
  const items: PluginInventoryView[] = []
  for (const valueItem of snapshot.items) {
    const item = parsePluginInventoryView(valueItem)
    if (!item || keys.has(item.key)) return null
    keys.add(item.key)
    items.push(item)
  }
  return { revision: snapshot.revision, items }
}

const publicPermissions = new Set<PublicPermission>([
  'ui.window', 'clipboard.write', 'clipboard.read', 'network.https',
  'files.userSelected', 'files.index.readAll', 'notifications.publish', 'background.schedule',
])
const publicFaults = new Set<PublicPluginFault>(['runtimeUnavailable', 'consecutiveFailures'])

function parsePublicSettingDefinition(value: unknown): PublicSettingDefinition | null {
  const setting = plainRecord(value)
  if (!setting || typeof setting.type !== 'string' || typeof setting.key !== 'string' || setting.key.length === 0 || typeof setting.label !== 'string') return null
  const optional = (key: string, type: 'string' | 'number' | 'boolean') => setting[key] === undefined || typeof setting[key] === type
  if (setting.type === 'text' && exactKeys(setting, setting.default === undefined ? ['key', 'label', 'type'] : ['default', 'key', 'label', 'type']) && optional('default', 'string')) return setting as unknown as PublicSettingDefinition
  if (setting.type === 'secret' && exactKeys(setting, ['key', 'label', 'type'])) return setting as unknown as PublicSettingDefinition
  if (setting.type === 'boolean' && exactKeys(setting, setting.default === undefined ? ['key', 'label', 'type'] : ['default', 'key', 'label', 'type']) && optional('default', 'boolean')) return setting as unknown as PublicSettingDefinition
  if (setting.type === 'number') {
    const keys = ['key', 'label', 'type', ...['default', 'min', 'max', 'step'].filter((key) => setting[key] !== undefined)].sort()
    if (!exactKeys(setting, keys) || !['default', 'min', 'max', 'step'].every((key) => optional(key, 'number'))) return null
    if (['default', 'min', 'max', 'step'].some((key) => typeof setting[key] === 'number' && !Number.isFinite(setting[key]))) return null
    return setting as unknown as PublicSettingDefinition
  }
  if (setting.type === 'select') {
    const keys = setting.default === undefined ? ['key', 'label', 'options', 'type'] : ['default', 'key', 'label', 'options', 'type']
    if (!exactKeys(setting, keys) || !optional('default', 'string') || !Array.isArray(setting.options) || !exactDenseArray(setting.options)) return null
    const seen = new Set<string>()
    for (const option of setting.options) {
      const record = plainRecord(option)
      if (!record || !exactKeys(record, ['label', 'value']) || typeof record.value !== 'string' || typeof record.label !== 'string' || !seen.add(record.value)) return null
    }
    return setting as unknown as PublicSettingDefinition
  }
  return null
}

function parsePublicSettingView(value: unknown): PublicSettingView | null {
  const view = plainRecord(value)
  if (!view || !Object.prototype.hasOwnProperty.call(view, 'definition')) return null
  const definition = parsePublicSettingDefinition(view.definition)
  if (!definition) return null
  if (definition.type === 'secret') {
    if (!exactKeys(view, ['definition', 'secretConfigured']) || typeof view.secretConfigured !== 'boolean') return null
    return { definition, secretConfigured: view.secretConfigured }
  }
  const expected = view.value === undefined ? ['definition'] : ['definition', 'value']
  if (!exactKeys(view, expected)) return null
  const valueType = definition.type === 'text' || definition.type === 'select' ? 'string' : definition.type
  if (view.value !== undefined && (typeof view.value !== valueType || (valueType === 'number' && !Number.isFinite(view.value)))) return null
  return view.value === undefined ? { definition } : { definition, value: view.value as string | number | boolean }
}

function parsePublicPluginItem(value: unknown): PublicPluginInventoryItem | null {
  const item = plainRecord(value)
  const keys = ['defaultName', 'description', 'effectiveName', 'enabled', 'fault', 'generation', 'name', 'permissions', 'pluginId', 'settings', 'source', 'version']
  if (!item || !exactKeys(item, keys) || typeof item.pluginId !== 'string' || item.pluginId.length === 0 || typeof item.name !== 'string' || typeof item.version !== 'string' || typeof item.defaultName !== 'string' || typeof item.effectiveName !== 'string' || typeof item.enabled !== 'boolean' || item.source !== 'localPackage' || !Number.isSafeInteger(item.generation) || (item.description !== null && typeof item.description !== 'string') || (item.fault !== null && (typeof item.fault !== 'string' || !publicFaults.has(item.fault as PublicPluginFault)))) return null
  if (!Array.isArray(item.permissions) || !exactDenseArray(item.permissions) || !Array.isArray(item.settings) || !exactDenseArray(item.settings)) return null
  const permissions: PublicPermissionView[] = []
  for (const valuePermission of item.permissions) {
    const permission = plainRecord(valuePermission)
    if (!permission || !exactKeys(permission, ['granted', 'permission', 'supported']) || typeof permission.permission !== 'string' || !publicPermissions.has(permission.permission as PublicPermission) || typeof permission.supported !== 'boolean' || typeof permission.granted !== 'boolean') return null
    permissions.push(permission as unknown as PublicPermissionView)
  }
  const settings: PublicSettingView[] = []
  const settingKeys = new Set<string>()
  for (const valueSetting of item.settings) {
    const setting = parsePublicSettingView(valueSetting)
    if (!setting || !settingKeys.add(setting.definition.key)) return null
    settings.push(setting)
  }
  return { ...(item as unknown as PublicPluginInventoryItem), permissions, settings }
}

export function parsePublicPluginInventory(value: unknown): PublicPluginInventory | null {
  const inventory = plainRecord(value)
  if (!inventory || !exactKeys(inventory, ['items', 'revision']) || !canonicalU64(inventory.revision) || !Array.isArray(inventory.items) || !exactDenseArray(inventory.items)) return null
  const items: PublicPluginInventoryItem[] = []
  const ids = new Set<string>()
  for (const valueItem of inventory.items) {
    const item = parsePublicPluginItem(valueItem)
    if (!item || !ids.add(item.pluginId)) return null
    items.push(item)
  }
  return { revision: inventory.revision, items }
}
export function parsePluginMutationOutcome(value: unknown): PluginMutationOutcome | null {
  const outcome = plainRecord(value)
  return outcome && exactKeys(outcome, ['revision']) && canonicalU64(outcome.revision)
    ? { revision: outcome.revision }
    : null
}

export function compareU64Decimal(left: string, right: string): -1 | 0 | 1 {
  if (!canonicalU64(left) || !canonicalU64(right)) throw new TypeError('invalid decimal revision')
  if (left.length !== right.length) return left.length < right.length ? -1 : 1
  return left === right ? 0 : left < right ? -1 : 1
}

export const compareDecimalRevision = compareU64Decimal

export function parseU64Decimal(value: unknown): U64Decimal | null {
  if (typeof value !== 'string' || !DECIMAL_U64.test(value)) return null
  return BigInt(value) <= U64_MAX ? value as U64Decimal : null
}

function canonicalU64(value: unknown): value is string {
  return parseU64Decimal(value) !== null
}

function validUnreadCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 100
}

export function parseMessageSummary(value: unknown): MessageSummary | null {
  const summary = plainRecord(value)
  const revision = summary ? parseU64Decimal(summary.revision) : null
  if (!summary || !revision || !exactKeys(summary, ['revision', 'unreadCount']) || !validUnreadCount(summary.unreadCount)) {
    return null
  }
  return { revision, unreadCount: summary.unreadCount }
}

export function parseMessageView(value: unknown): MessageView | null {
  const message = plainRecord(value)
  const id = message ? parseU64Decimal(message.id) : null
  if (
    !message ||
    !id ||
    !exactKeys(message, [
      'content', 'createdAt', 'id', 'pluginIconUrl', 'pluginId', 'pluginNameSnapshot', 'readAt',
    ]) ||
    typeof message.pluginId !== 'string' ||
    message.pluginId.length === 0 ||
    typeof message.pluginNameSnapshot !== 'string' ||
    message.pluginNameSnapshot.length === 0 ||
    (message.pluginIconUrl !== null &&
      (typeof message.pluginIconUrl !== 'string' || message.pluginIconUrl.length === 0)) ||
    typeof message.createdAt !== 'string' ||
    !strictUtcRfc3339(message.createdAt) ||
    typeof message.content !== 'string' ||
    message.content.length === 0 ||
    (message.readAt !== null && (typeof message.readAt !== 'string' || !strictUtcRfc3339(message.readAt)))
  ) {
    return null
  }
  return {
    id,
    pluginId: message.pluginId,
    pluginNameSnapshot: message.pluginNameSnapshot,
    pluginIconUrl: message.pluginIconUrl,
    createdAt: message.createdAt,
    content: message.content,
    readAt: message.readAt,
  }
}

export function parseMessageCenterSnapshot(value: unknown): MessageCenterSnapshot | null {
  const snapshot = plainRecord(value)
  const revision = snapshot ? parseU64Decimal(snapshot.revision) : null
  if (
    !snapshot ||
    !revision ||
    !exactKeys(snapshot, ['messages', 'revision', 'unreadCount']) ||
    !validUnreadCount(snapshot.unreadCount) ||
    !Array.isArray(snapshot.messages) ||
    Object.getPrototypeOf(snapshot.messages) !== Array.prototype ||
    !exactDenseArray(snapshot.messages)
  ) {
    return null
  }
  const ids = new Set<string>()
  const messages: MessageView[] = []
  for (const valueMessage of snapshot.messages) {
    const message = parseMessageView(valueMessage)
    if (!message || ids.has(message.id)) return null
    ids.add(message.id)
    messages.push(message)
  }
  return { revision, unreadCount: snapshot.unreadCount, messages }
}

export function parseMessageHostStateChanged(value: unknown): MessageHostStateChanged | null {
  const event = plainRecord(value)
  if (!event || typeof event.status !== 'string') return null
  if (event.status === 'unavailable') {
    return exactKeys(event, ['error', 'status']) && event.error === 'MessageStoreUnavailable'
      ? { status: 'unavailable', error: 'MessageStoreUnavailable' }
      : null
  }
  const revision = parseU64Decimal(event.revision)
  return event.status === 'ready' &&
    revision !== null &&
    exactKeys(event, ['revision', 'status', 'unreadCount']) &&
    validUnreadCount(event.unreadCount)
    ? { status: 'ready', revision, unreadCount: event.unreadCount }
    : null
}

export function parseMessageHostCommandError(value: unknown): MessageHostCommandError | null {
  const error = plainRecord(value)
  if (!error || !exactKeys(error, ['code', 'storeStatus'])) return null
  if (error.code === 'MessageOperationFailed' && error.storeStatus === 'ready') {
    return { code: 'MessageOperationFailed', storeStatus: 'ready' }
  }
  if (error.code === 'MessageStoreUnavailable' && error.storeStatus === 'unavailable') {
    return { code: 'MessageStoreUnavailable', storeStatus: 'unavailable' }
  }
  return null
}

export function parseFindForwardPayload(value: unknown): FindForwardPayload | null {
  const payload = plainRecord(value)
  const forwardSequence = payload ? parseU64Decimal(payload.forwardSequence) : null
  if (!payload || !exactKeys(payload, ['forwardSequence', 'invocationId', 'query']) ||
      typeof payload.invocationId !== 'string' || payload.invocationId.length === 0 ||
      typeof payload.query !== 'string' || !forwardSequence) return null
  return { invocationId: payload.invocationId, forwardSequence, query: payload.query }
}

export function parseFindReadyOutcome(value: unknown): FindReadyOutcome | null {
  const outcome = plainRecord(value)
  if (!outcome || typeof outcome.status !== 'string') return null
  if (outcome.status === 'superseded') return exactKeys(outcome, ['status']) ? { status: 'superseded' } : null
  if (outcome.status === 'ready') {
    return exactKeys(outcome, ['initializationToken', 'status']) &&
      typeof outcome.initializationToken === 'string' && outcome.initializationToken.length > 0
      ? { status: 'ready', initializationToken: outcome.initializationToken } : null
  }
  if (outcome.status !== 'prepared' || !exactKeys(outcome, ['initialization', 'status'])) return null
  const initialization = plainRecord(outcome.initialization)
  if (!initialization || !exactKeys(initialization, ['filePreviewEnabled', 'filePreviewRevision', 'initializationToken', 'pinned', 'theme', 'themeRevision'])) return null
  const themeRevision = parseU64Decimal(initialization.themeRevision)
  const filePreviewRevision = parseU64Decimal(initialization.filePreviewRevision)
  if (typeof initialization.initializationToken !== 'string' || initialization.initializationToken.length === 0 ||
      (initialization.theme !== 'system' && initialization.theme !== 'dark' && initialization.theme !== 'light') ||
      typeof initialization.filePreviewEnabled !== 'boolean' || typeof initialization.pinned !== 'boolean' ||
      !themeRevision || !filePreviewRevision) return null
  return { status: 'prepared', initialization: {
    initializationToken: initialization.initializationToken, themeRevision, theme: initialization.theme,
    filePreviewRevision, filePreviewEnabled: initialization.filePreviewEnabled, pinned: initialization.pinned,
  } }
}

export function parseFindThemeChanged(value: unknown): FindThemeChanged | null {
  const event = plainRecord(value)
  const themeRevision = event ? parseU64Decimal(event.themeRevision) : null
  if (!event || !exactKeys(event, ['theme', 'themeRevision']) || !themeRevision ||
      (event.theme !== 'system' && event.theme !== 'dark' && event.theme !== 'light')) return null
  return { themeRevision, theme: event.theme }
}

export function parseFindPreviewPreferenceResult(value: unknown): FindPreviewPreferenceResult | null {
  const result = plainRecord(value)
  const filePreviewRevision = result ? parseU64Decimal(result.filePreviewRevision) : null
  if (!result || !exactKeys(result, ['filePreviewEnabled', 'filePreviewRevision']) ||
      !filePreviewRevision || typeof result.filePreviewEnabled !== 'boolean') return null
  return { filePreviewRevision, filePreviewEnabled: result.filePreviewEnabled }
}

function fileStatus(value: unknown): value is FileIndexStatus {
  return typeof value === 'string' && fileStatuses.has(value as FileIndexStatus)
}

function strictUtcRfc3339(value: string): boolean {
  const match = UTC_RFC3339.exec(value)
  if (!match) return false
  const instant = new Date(value)
  if (Number.isNaN(instant.getTime())) return false
  const [, year, month, day, hour, minute, second] = match
  return (
    instant.getUTCFullYear() === Number(year) &&
    instant.getUTCMonth() + 1 === Number(month) &&
    instant.getUTCDate() === Number(day) &&
    instant.getUTCHours() === Number(hour) &&
    instant.getUTCMinutes() === Number(minute) &&
    instant.getUTCSeconds() === Number(second)
  )
}

function parseFileResultItem(value: unknown): FileResultItem | null {
  const item = plainRecord(value)
  if (!item || !exactKeys(item, ['fullPath', 'kind', 'modifiedUtc', 'name', 'resultId', 'sizeBytes'])) return null
  if (
    typeof item.resultId !== 'string' ||
    typeof item.name !== 'string' ||
    (item.kind !== 'file' && item.kind !== 'folder') ||
    typeof item.modifiedUtc !== 'string' ||
    !strictUtcRfc3339(item.modifiedUtc) ||
    typeof item.fullPath !== 'string'
  ) {
    return null
  }
  if ((item.kind === 'folder' && item.sizeBytes !== null) || (item.kind === 'file' && !canonicalU64(item.sizeBytes))) return null
  return item as unknown as FileResultItem
}

export function parseFileSearchResponse(value: unknown): FileSearchResponse | null {
  const response = plainRecord(value)
  if (!response || !exactKeys(response, ['indexRevision', 'items', 'requestId', 'status', 'total'])) return null
  if (
    typeof response.requestId !== 'string' ||
    !canonicalU64(response.indexRevision) ||
    !canonicalU64(response.total) ||
    !fileStatus(response.status) ||
    !Array.isArray(response.items) ||
    Object.getPrototypeOf(response.items) !== Array.prototype ||
    !exactDenseArray(response.items)
  ) {
    return null
  }
  for (let index = 0; index < response.items.length; index += 1) {
    if (parseFileResultItem(response.items[index]) === null) return null
  }
  return response as unknown as FileSearchResponse
}
