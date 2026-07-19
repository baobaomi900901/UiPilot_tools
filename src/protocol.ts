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
  | { kind: 'compositionStart'; control: ControlKey; value: string }
  | { kind: 'compositionUpdate'; control: ControlKey; value: string }
  | { kind: 'compositionInput'; control: ControlKey; value: string; inputType: 'insertCompositionText' }
  | { kind: 'compositionEnd'; control: ControlKey; value: string }
  | { kind: 'ordinaryInput'; control: ControlKey; value: string; inputType: string }

export interface LauncherClient {
  listenShown(handler: (payload: unknown) => void): Promise<() => void>
  searchApps(input: { query: string; invocationId: string; querySequence: number }): Promise<SearchResponse | null>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  loadSettings(): Promise<SettingsView>
  saveSettings(input: { settings: UserSettingsUpdate }): Promise<void>
  rescanApps(): Promise<void>
  exportValidationData(): Promise<ExportOutcome>
  clearValidationData(): Promise<void>
  hideLauncher(): Promise<void>
}

export interface ViewResult {
  key: number
  title: string
  subtitle?: string
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
