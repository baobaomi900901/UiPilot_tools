export interface ResultItem {
  resultId: string
  title: string
  subtitle?: string
  icon?: string
}

export interface SearchResponse {
  requestId: string
  items: ResultItem[]
}

export interface AppAliasTarget {
  appId: string
  displayName: string
  icon?: string
  aliases: string[]
}

export interface SettingsView {
  hotkey: string
  autostart: boolean
  filePreviewEnabled: boolean
  researchId?: string
  applications: AppAliasTarget[]
}

export interface UserSettingsUpdate {
  hotkey: string
  autostart: boolean
  researchId?: string | null
  aliases: Record<string, string[]>
}

export type ExecuteOutcome =
  | { status: 'launchRequested' }
  | { status: 'activationRequested' }
  | { status: 'activationRefusedLaunchRequested'; message: string }
  | { status: 'fileRevealRequested' }
  | { status: 'folderOpenRequested' }

export type ExportOutcome = { status: 'cancelled' } | { status: 'exported' }

export type CommandErrorCode =
  | 'invalidCaller'
  | 'staleRequest'
  | 'unknownResult'
  | 'applicationEntryUnavailable'
  | 'settingsFailed'
  | 'validationFailed'
  | 'windowFailed'
  | 'scanFailed'
  | 'scanWorkerFailed'
  | 'mainThreadDispatchFailed'
  | 'exportFailed'
  | 'exportWorkerFailed'
  | 'invalidFileQuery'
  | 'fileSearchWorkerFailed'
  | 'searchUnavailable'
  | 'fileNotFound'
  | 'fileOpenFailed'

export interface CommandError {
  code: CommandErrorCode
  message: string
}

export type ShowTarget = 'launcher' | 'settings'
export type LifecycleNotice = 'settingsFailed' | 'validationFailed'

export interface LauncherShown {
  invocationId: string
  target: ShowTarget
  notice: LifecycleNotice | null
}

export type ControlKey = number

export type ClassifiedTextRecord =
  | { kind: 'compositionStart'; control: ControlKey }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }
  | { kind: 'compositionBoundary'; control: ControlKey }

export interface LauncherClient {
  listenShown(handler: (payload: unknown) => void): Promise<() => void>
  listenFileIndexChanged(handler: (payload: unknown) => void): Promise<() => void>
  searchApps(input: { query: string; invocationId: string; querySequence: number }): Promise<SearchResponse | null>
  searchFiles(input: {
    query: string
    category: FileCategory
    sort: FileSort
    invocationId: string
    querySequence: number
  }): Promise<FileSearchResponse | null>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  loadSettings(): Promise<SettingsView>
  saveSettings(input: { settings: UserSettingsUpdate }): Promise<void>
  setFilePreviewPreference(input: { preference: { enabled: boolean } }): Promise<void>
  rescanApps(): Promise<void>
  exportValidationData(): Promise<ExportOutcome>
  clearValidationData(): Promise<void>
  hideLauncher(): Promise<void>
}

export interface ViewResult {
  key: number
  title: string
  subtitle?: string
  icon?: string
}

export interface AliasControlView {
  key: ControlKey
  value: string
}

export interface ApplicationAliasView {
  key: ControlKey
  displayName: string
  aliases: readonly AliasControlView[]
}

export interface SettingsSnapshot {
  hotkey: AliasControlView
  researchId: AliasControlView
  autostart: boolean
  applications: readonly ApplicationAliasView[]
  readOnly: boolean
  operation?: 'load' | 'save' | 'rescan' | 'export' | 'clear'
  clearConfirmation: boolean
  needsReload: boolean
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

export interface FileIndexChanged {
  revision: string
  status: FileIndexStatus
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

export interface LauncherSnapshot {
  view: 'launcher' | 'settings'
  viewEpoch: number
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
  settings?: SettingsSnapshot
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
  if (candidate.target !== 'launcher' && candidate.target !== 'settings') return null
  if (candidate.notice !== null && candidate.notice !== 'settingsFailed' && candidate.notice !== 'validationFailed') return null
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

function canonicalU64(value: unknown): value is string {
  if (typeof value !== 'string' || !DECIMAL_U64.test(value)) return false
  return BigInt(value) <= U64_MAX
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

export function parseFileIndexChanged(value: unknown): FileIndexChanged | null {
  const event = plainRecord(value)
  if (!event || !exactKeys(event, ['revision', 'status'])) return null
  if (!canonicalU64(event.revision) || !fileStatus(event.status)) return null
  return event as unknown as FileIndexChanged
}
