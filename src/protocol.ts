import { safePublicPluginIconUrl } from './plugin-icon-url'

export type ResultIconKind = 'find' | 'quicklinks' | 'screenshot' | 'calculator' | 'currency' | 'date' | 'timeZone' | 'unit' | 'webSearch'
export type BuiltinFeature = 'find' | 'quicklinks' | 'screenshot' | 'webSearch'
export type ResultFavoriteTarget =
  | Readonly<{ kind: 'publicPlugin'; pluginId: string; commandId: string }>
  | Readonly<{ kind: 'builtin'; feature: BuiltinFeature }>
export type ResultFavorite = Readonly<{ target: ResultFavoriteTarget; favorite: boolean }>

export type LauncherResultActivation =
  | { kind: 'completion'; completionText: string }
  | { kind: 'pluginCompletion'; completionText: string; pluginId: string; favorite: boolean }
  | {
      kind: 'windowActivation'
      pluginId: string
      commandLabel: string
      initialArgument: string
      favorite: boolean
    }
  | {
      kind: 'mainResultActivation'
      commandId: string
      pluginId: string
      commandLabel: string
      initialArgument: string
      favorite: boolean
    }
  | { kind: 'panelActivation'; pluginId: string; initialArgument: string; favorite: boolean }
  | { kind: 'openFind'; query: string }
  | { kind: 'openQuicklinks' }
  | { kind: 'openScreenshot' }
  | { kind: 'executeResult' }
  | { kind: 'informational' }

export type CompletionOrigin = Readonly<{
  phase: 'preview' | 'commit'
  pluginId: string
}>

export interface SearchAppsInput {
  query: string
  invocationId: string
  querySequence: number
  submit?: boolean
  completionOrigin?: CompletionOrigin
  stableCommandTarget?: CommandHotkeyEntry
}

export interface ResultItem {
  resultId: string
  activation: LauncherResultActivation
  title: string
  subtitle?: string
  icon?: string
  pluginIconUrl?: string
  iconKind?: ResultIconKind
  detail?: string
  favorite?: ResultFavorite
  hasDefaultAction?: boolean
  actions?: readonly ResultActionDisplay[]
  defaultActionId?: string
}

export interface ResultActionDisplay { readonly actionId: string; readonly title: string }
export interface ResultActionMetadata {
  readonly actions: readonly ResultActionDisplay[]
  readonly defaultActionId?: string
}
export type ResultActionErrorCode =
  | 'actionBusy' | 'actionInProgress' | 'actionAlreadyExecuted' | 'actionFailed'
  | 'actionInvalidResponse' | 'actionTimedOut' | 'actionUnavailable' | 'actionCancelled'
  | 'staleRequest' | 'unknownResult' | 'pluginPermissionDenied' | 'clipboardWriteFailed' | 'urlOpenFailed' | 'pasteTargetUnavailable' | 'pasteInputBusy' | 'pasteFailed'
export type ResultActionOutcome =
  | { kind: 'copied' }
  | { kind: 'urlOpened' }
  | { kind: 'pasteSent' }
  | { kind: 'resultsUpdated'; response: SearchResponse }
  | { kind: 'failed'; code: ResultActionErrorCode; sourceConsumed: boolean }
export interface SearchResponse {
  requestId: string
  items: ResultItem[]
  refreshAtUtc?: string
  windowTransferToken?: string
  replaceLocalResults?: boolean
  commandHint?: string
  mainResultCommand?: MainResultCommandContext
  autoExecuteResultId?: string
}

export interface MainResultCommandContext {
  commandId: string
  pluginId: string
  commandLabel: string
  argument: string
}

export type ThemePreference = 'system' | 'dark' | 'light'
export type WebSearchEngine = 'bing' | 'baidu' | 'google'

export interface QuicklinkView {
  id: string
  name: string
  command: string
  template: string
  iconDataUrl?: string
  createdAt: string
  updatedAt: string
}

export interface QuicklinkListResponse {
  items: QuicklinkView[]
  loadError?: string
}

export interface QuicklinkSaveInput {
  id?: string
  name: string
  command: string
  template: string
  iconToken?: string | null
}

export interface QuicklinkIconCandidate {
  token: string
  dataUrl: string
}

export type QuicklinksOperation = 'load' | 'save' | 'delete' | 'icon'

export interface QuicklinkDraftSnapshot {
  id?: string
  name: string
  command: string
  template: string
  iconDataUrl?: string
  iconToken?: string
}

export interface QuicklinksSnapshot {
  status: 'loading' | 'ready' | 'error'
  items: readonly QuicklinkView[]
  draft: QuicklinkDraftSnapshot
  draftActive: boolean
  createDraft?: QuicklinkDraftSnapshot
  selectedId?: string
  operation?: QuicklinksOperation
  error?: string
}

export interface SettingsView {
  hotkey: string
  screenshotHotkey: string | null
  screenshotHotkeyStatus: 'unset' | 'active' | 'unavailable'
  screenshotSaveDirectory: string | null
  screenshotSaveDirectoryPath: string | null
  autostart: boolean
  filePreviewEnabled: boolean
  theme: ThemePreference
  webSearchEngine: WebSearchEngine
}

export interface ScreenshotSaveDirectoryView {
  directory: string | null
  path: string | null
}

export interface UserSettingsUpdate {
  hotkey: string
  autostart: boolean
  theme: ThemePreference
  webSearchEngine: WebSearchEngine
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
  | 'ui.panel'
  | 'clipboard.write'
  | 'clipboard.paste'
  | 'url.open'
  | 'files.openText'
  | 'clipboard.read'
  | 'clipboard.history.read'
  | 'clipboard.history.paste'
  | 'network.https'
  | 'files.userSelected'
  | 'files.index.readAll'
  | 'notifications.publish'
  | 'timer.control'
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

export interface PluginNetworkCredential {
  readonly id: string
  readonly settingKey: string
  readonly host: string
  readonly type: 'bearer'
}

export interface PublicPluginInventoryNetwork {
  readonly credentials: readonly PluginNetworkCredential[]
  readonly httpsHosts: readonly string[]
}

export interface PublicPluginPrepareNetwork extends PublicPluginInventoryNetwork {
  readonly addedHttpsHosts: readonly string[]
  readonly addedCredentialIds: readonly string[]
  readonly requiresNetworkConsent: boolean
}

export interface PublicPluginInventoryCommand {
  commandId: string
  defaultName: string
  effectiveName: string
  nameOverride: string | null
  favorite: boolean
  hotkey: string | null
  hotkeyStatus: 'unset' | 'active' | 'suspended' | 'unavailable'
}

export interface PublicPluginInventoryItem {
  pluginId: string
  name: string
  description: string | null
  version: string
  source: 'localPackage'
  commands: readonly PublicPluginInventoryCommand[]
  enabled: boolean
  fault: PublicPluginFault | null
  generation: number
  iconUrl: string | null
  network: PublicPluginInventoryNetwork | null
  permissions: readonly PublicPermissionView[]
  settings: readonly PublicSettingView[]
}

export interface PublicPluginInventory {
  hotkeyRevision: string
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
  operation: 'install' | 'replace' | 'developmentRefresh'
  previousVersion: string | null
  sourceVerified: boolean
  iconUrl: string | null
  network: PublicPluginPrepareNetwork | null
}

export interface PublicPluginWindowIdentity {
  name: string
  iconUrl: string | null
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
  | 'hotkeyConflict'
  | 'windowFailed'
  | 'invalidFileQuery'
  | 'fileSearchWorkerFailed'
  | 'fileSearchStarting'
  | 'searchUnavailable'
  | 'clipboardWriteFailed'
  | 'pluginPermissionDenied'
  | 'pluginListFailed'
  | 'pluginInstallFailed'
  | 'pluginReloadFailed'
  | 'pluginDeleteFailed'
  | 'dataCleanupPending'
  | 'fileNotFound'
  | 'fileOpenFailed'
  | 'webSearchFailed'

export interface CommandError {
  code: CommandErrorCode
  message: string
}

export type ShowTarget = 'launcher' | 'settings' | 'messages'
export type LifecycleNotice = 'settingsFailed'

export interface CommandHotkeyEntry { pluginId: string; commandId: string; hotkey: string; bindingRevision: string }
export interface CommandEntryResolution { pluginId: string; commandId: string; commandLabel: string; outputMode: 'mainResult' | 'window' | 'panel'; activationMode: 'live' | 'submit'; inputRequired: boolean; inputPlaceholder: string | null }

export interface LauncherShown {
  invocationId: string
  target: ShowTarget
  notice: LifecycleNotice | null
  commandEntry?: CommandHotkeyEntry
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
  validateYoudaoTranslation(input: { appId: string; appKey: string }): Promise<{ valid: boolean; message: string }>
  listenShown(handler: (payload: unknown) => void): Promise<() => void>
  listenHidden(handler: () => void): Promise<() => void>
  listenMessageStateChanged(handler: (payload: unknown) => void): Promise<() => void>
  listenPluginPanelError(handler: (payload: unknown) => void): Promise<() => void>
  listenPluginPanelReset(handler: (payload: unknown) => void): Promise<() => void>
  listenPluginPanelFocusHostInput(handler: (payload: unknown) => void): Promise<() => void>
  getMessageSummary(): Promise<unknown>
  openMessageCenter(): Promise<unknown>
  readMessageCenter(): Promise<unknown>
  clearMessages(): Promise<unknown>
  searchApps(input: SearchAppsInput): Promise<SearchResponse | null>
  listQuicklinks(): Promise<QuicklinkListResponse>
  saveQuicklink(input: { input: QuicklinkSaveInput }): Promise<QuicklinkView>
  deleteQuicklink(input: { id: string }): Promise<void>
  chooseQuicklinkIcon(): Promise<QuicklinkIconCandidate | null>
  openFind(input: OpenFindInput): Promise<OpenFindOutcome>
  startScreenshot(input: { mode: 'region' | 'window'; invocationId: string; querySequence: number }): Promise<void>
  listenScreenshotError(handler: (payload: unknown) => void): Promise<() => void>
  executeResult(input: { requestId: string; resultId: string }): Promise<ExecuteOutcome>
  executeResultAction(input: { requestId: string; resultId: string; actionId: string }): Promise<unknown>
  retireResultActions(input: { requestId: string }): Promise<void>
  openPluginPanel(input: { pluginId: string; argument: string; stableCommandTarget?: CommandHotkeyEntry }): Promise<PluginPanelCommandResult>
  submitPluginPanel(input: {
    sessionEpoch: U64Decimal
    argument: string
    uiIntentEpoch: number
  }): Promise<PluginPanelCommandResult>
  enqueuePluginPanelHostKey(input: PluginPanelHostKeyEnqueueInput): Promise<PluginPanelHostKeyEnqueueResult>
  setPluginPanelBounds(input: { sessionEpoch: U64Decimal; bounds: PluginPanelBounds }): Promise<void>
  closePluginPanel(input: { sessionEpoch: U64Decimal }): Promise<void>
  acknowledgePluginPanelFocusHostInput(input: PluginPanelFocusHostInputEvent & { focused: boolean }): Promise<void>
  commitPluginWindowTransfer(input: { transferToken: string }): Promise<void>
  setPublicPluginCommandHotkey(input: { pluginId: string; commandId: string; hotkey: string | null }): Promise<void>
  resolvePublicPluginCommandEntry(input: { entry: CommandHotkeyEntry }): Promise<CommandEntryResolution>
  listPublicPlugins(): Promise<PublicPluginInventory>
  selectPublicPluginArchive(): Promise<string | null>
  selectPublicPluginDirectory(): Promise<string | null>
  preparePublicPlugin(input: { source: { kind: 'archive' | 'developmentDirectory'; path: string }; refreshTarget?: { pluginId: string; expectedInventoryRevision: string } }): Promise<PublicPluginPrepareSummary>
  commitPublicPlugin(input: { input: { token: string; permissionGrants: readonly PublicPermission[] } }): Promise<void>
  getPublicPluginDiagnostics(input: { pluginId: string }): Promise<PublicPluginDiagnosticReport>
  copyPublicPluginDiagnostics(input: { report: PublicPluginDiagnosticReport }): Promise<void>
  cancelPublicPlugin(input: { token: string }): Promise<void>
  setPublicPluginEnabled(input: { pluginId: string; enabled: boolean }): Promise<void>
  setPublicPluginNetworkAccess(input: { pluginId: string; granted: boolean; expectedInventoryRevision?: U64Decimal }): Promise<void>
  setPublicPluginFavorite(input: { pluginId: string; commandId: string; favorite: boolean }): Promise<void>
  setBuiltinFeatureFavorite(input: { feature: BuiltinFeature; favorite: boolean }): Promise<void>
  setPublicPluginEffectiveName(input: { pluginId: string; commandId: string; nameOverride: string | null }): Promise<void>
  savePublicPluginSettings(input: { input: { pluginId: string; settings: Readonly<Record<string, string | number | boolean>>; secrets: Readonly<Record<string, string | null>> } }): Promise<void>
  uninstallPublicPlugin(input: { pluginId: string; retainData: boolean }): Promise<void>
  listPlugins(): Promise<PluginInventorySnapshot>
  installPlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  reloadPlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  deletePlugin(input: { pluginId: string }): Promise<PluginMutationOutcome>
  loadSettings(): Promise<SettingsView>
  saveSettings(input: { settings: UserSettingsUpdate }): Promise<void>
  saveHotkey(input: { hotkey: HotkeySettingsUpdate }): Promise<HotkeySettingsView>
  selectScreenshotSaveDirectory(): Promise<ScreenshotSaveDirectoryView | null>
  setScreenshotSaveDirectory(input: { input: { directory: string | null } }): Promise<ScreenshotSaveDirectoryView>
  setScreenshotHotkey(input: { input: { hotkey: string | null } }): Promise<void>
  setHotkeyRecording(input: { recording: boolean }): Promise<void>
  setThemePreference(input: { preference: { theme: ThemePreference } }): Promise<void>
  setWebSearchEngine(input: { preference: { engine: WebSearchEngine } }): Promise<void>
  hideLauncher(): Promise<void>
}

export interface PluginWindowClient {
  getIdentity(): Promise<PublicPluginWindowIdentity>
  setPinned(input: { pinned: boolean }): Promise<{ pinned: boolean }>
  close(): Promise<void>
}
export type U64Decimal = string & { readonly __u64Decimal: unique symbol }
export type PluginTimerPhase = 'idle' | 'running' | 'paused' | 'fired'
export interface PluginTimerStartInput {
  readonly durationMs: number
  readonly completionMessage: string
}
export type PluginTimerState =
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'idle'
      durationMs: number | null
      remainingMs: number | null
    }>
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'running' | 'paused'
      durationMs: number
      remainingMs: number
    }>
  | Readonly<{
      timerRevision: U64Decimal
      phase: 'fired'
      durationMs: number
      remainingMs: 0
    }>
export type PluginTimerErrorName =
  | 'InvalidCaller'
  | 'PermissionDenied'
  | 'ExpiredWindowSessionError'
  | 'InvalidTimerInput'
  | 'TimerInputRequired'
  | 'TimerInputNotAllowed'
  | 'MessageStoreUnavailable'
  | 'TimerUnavailable'
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
  loadThumbnail(input: { requestId: string; resultId: string }): Promise<unknown>
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
  pluginIconUrl?: string
  iconKind?: ResultIconKind
  detail?: string
  hasDefaultAction?: boolean
  favorite?: ResultFavorite
  actions?: readonly ResultActionDisplay[]
  panelActivation?: Readonly<{ pluginId: string; initialArgument: string }>
}

export interface PluginPanelCommandResult {
  sessionEpoch: U64Decimal
  pluginId: string
  commandLabel: string
  inputPlaceholder?: string
  hostKeys: readonly PanelHostKeyDeclaration[]
}

/** Physical client pixels, converted from DOM coordinates by the main WebView. */
export interface PluginPanelBounds {
  x: number
  y: number
  width: number
  height: number
}

export type PanelHostKeyDeclaration = 'ArrowDown' | 'ArrowUp' | 'Primary+N' | 'Tab' | 'Shift+Tab' | 'Enter'
export type PluginPanelHostKey = 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter'

export interface PluginPanelHostKeyEnqueueInput {
  sessionEpoch: U64Decimal
  clientSequence: U64Decimal
  declaration: PanelHostKeyDeclaration
  key: PluginPanelHostKey
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
}

export type PluginPanelHostKeyEnqueueResult =
  | { outcome: 'enqueued'; routeSequence: U64Decimal }
  | { outcome: 'droppedQueueFull' | 'noop' | 'protocolViolation' }

export interface PluginPanelErrorEvent {
  sessionEpoch: U64Decimal
}

export interface PluginPanelFocusHostInputEvent {
  sessionEpoch: U64Decimal
  focusRequestId: U64Decimal
}

export interface PluginPanelSnapshot {
  pluginId: string
  commandLabel: string
  inputPlaceholder?: string
  sessionEpoch: U64Decimal
  hostKeys: readonly PanelHostKeyDeclaration[]
  suffixControl: ControlKey
  suffix: string
  submitPending: boolean
  closePending: boolean
  focusRequestId?: U64Decimal
}
export interface MainResultCommandSnapshot {
  commandId: string
  pluginId: string
  commandLabel: string
  suffixControl: ControlKey
  suffix: string
}
export interface TextControlView {
  key: ControlKey
  value: string
}

export type SettingsLoadStatus = 'loading' | 'ready' | 'error'

export interface SettingsSnapshot {
  hotkey: TextControlView
  screenshotHotkey: string | null
  screenshotHotkeyStatus: 'unset' | 'active' | 'unavailable'
  screenshotSaveDirectory: string | null
  screenshotSaveDirectoryPath: string | null
  autostart: boolean
  theme: ThemePreference
  webSearchEngine: WebSearchEngine
  loadStatus: SettingsLoadStatus
  readOnly: boolean
  operation?: 'load' | 'save' | 'hotkey' | 'theme' | 'webSearchEngine' | 'screenshotDirectory'
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

export type SettingsTabKey = 'general' | 'plugins' | 'messages' | 'credentials'

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
  favoriteMutationPending: boolean
  resultActionsDisabled: boolean
  resultActionMenu?: Readonly<{ resultKey: number }>
  resultActionRerun: boolean
  shownNotice?: string
  commandHint?: string
  status: string
  settingsLoadStatus?: SettingsLoadStatus
  settings?: SettingsSnapshot
  plugins?: PluginListSnapshot
  publicPlugins?: PublicPluginInventory
  file?: FileSnapshot
  mainResultCommand?: MainResultCommandSnapshot
  panel?: PluginPanelSnapshot
  quicklinks?: QuicklinksSnapshot
}

const shownKeys = ['invocationId', 'notice', 'target']

export function parseLauncherShown(value: unknown): LauncherShown | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null
  const prototype = Object.getPrototypeOf(value)
  if (prototype !== Object.prototype && prototype !== null) return null
  const candidate = value as Record<string, unknown>
  const keys = Object.keys(candidate).filter((key) => key !== 'commandEntry').sort()
  if (keys.length !== shownKeys.length || keys.some((key, index) => key !== shownKeys[index])) return null
  if (Object.prototype.hasOwnProperty.call(candidate, 'commandEntry') && (candidate.target !== 'launcher' || !parseCommandHotkeyEntry(candidate.commandEntry))) return null
  if (typeof candidate.invocationId !== 'string') return null
  if (candidate.target !== 'launcher' && candidate.target !== 'settings' && candidate.target !== 'messages') return null
  if (candidate.notice !== null && candidate.notice !== 'settingsFailed') return null
  return candidate as unknown as LauncherShown
}

export function parseCommandHotkeyEntry(value: unknown): CommandHotkeyEntry | null {
  const entry = plainRecord(value)
  if (!entry || !exactKeys(entry, ['bindingRevision', 'commandId', 'hotkey', 'pluginId']) ||
    typeof entry.pluginId !== 'string' || !PUBLIC_PLUGIN_ID.test(entry.pluginId) ||
    typeof entry.commandId !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(entry.commandId) ||
    typeof entry.hotkey !== 'string' || (!/^(?:(?:Ctrl|Control|Shift|Alt|Meta|Super)\+)*[A-Za-z][A-Za-z0-9]*$/iu.test(entry.hotkey) || entry.hotkey.startsWith('Double')) ||
    !canonicalU64(entry.bindingRevision) || entry.bindingRevision === '0') return null
  return entry as unknown as CommandHotkeyEntry
}
export function parseCommandEntryResolution(value: unknown): CommandEntryResolution | null {
  const entry = plainRecord(value)
  if (!entry || !exactKeys(entry, ['activationMode', 'commandId', 'commandLabel', 'inputPlaceholder', 'inputRequired', 'outputMode', 'pluginId']) ||
    typeof entry.pluginId !== 'string' || !PUBLIC_PLUGIN_ID.test(entry.pluginId) || typeof entry.commandId !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(entry.commandId) ||
    typeof entry.commandLabel !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(entry.commandLabel) || typeof entry.inputRequired !== 'boolean' ||
    (entry.inputPlaceholder !== null && typeof entry.inputPlaceholder !== 'string') || !['mainResult', 'window', 'panel'].includes(entry.outputMode as string) || !['live', 'submit'].includes(entry.activationMode as string)) return null
  return entry as unknown as CommandEntryResolution
}

const U64_MAX = 18_446_744_073_709_551_615n
const DECIMAL_U64 = /^(0|[1-9][0-9]*)$/
const PUBLIC_PLUGIN_ID = /^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/
const PUBLIC_HTTPS_HOST_LABEL = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/
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

const resultActionErrors = new Set<ResultActionErrorCode>([
  'actionBusy', 'actionInProgress', 'actionAlreadyExecuted', 'actionFailed',
  'actionInvalidResponse', 'actionTimedOut', 'actionUnavailable', 'actionCancelled',
  'staleRequest', 'unknownResult', 'pluginPermissionDenied', 'clipboardWriteFailed', 'urlOpenFailed', 'pasteTargetUnavailable', 'pasteInputBusy', 'pasteFailed',
])

function opaqueResultHandle(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= 128 && !/[\p{Cc}\s]/u.test(value)
}

/** Reads only the additive metadata from a row. Undefined means a legacy row. */
export function parseResultActionMetadata(value: unknown): ResultActionMetadata | null | undefined {
  const row = plainRecord(value)
  if (!row) return null
  if (!Object.prototype.hasOwnProperty.call(row, 'actions')) return Object.prototype.hasOwnProperty.call(row, 'defaultActionId') ? null : undefined
  if (!Array.isArray(row.actions) || !exactDenseArray(row.actions) || row.actions.length > 8) return null
  const actions: ResultActionDisplay[] = []
  const handles = new Set<string>()
  for (const value of row.actions) {
    const action = plainRecord(value)
    if (!action || !exactKeys(action, ['actionId', 'title']) || !opaqueResultHandle(action.actionId) ||
        handles.has(action.actionId) || typeof action.title !== 'string' ||
        !action.title.trim() || [...action.title].length > 80 || /\p{Cc}/u.test(action.title)) return null
    handles.add(action.actionId)
    actions.push(Object.freeze({ actionId: action.actionId, title: action.title }))
  }
  const hasDefault = Object.prototype.hasOwnProperty.call(row, 'defaultActionId')
  if (row.hasDefaultAction !== hasDefault || (hasDefault &&
      (!opaqueResultHandle(row.defaultActionId) || !handles.has(row.defaultActionId)))) return null
  return Object.freeze({ actions: Object.freeze(actions), ...(hasDefault ? { defaultActionId: row.defaultActionId as string } : {}) })
}

/** Action replacements have no navigation or auto-execution instructions. */
export function parseResultActionOutcome(value: unknown): ResultActionOutcome | null {
  const outcome = plainRecord(value)
  if (!outcome) return null
  if (outcome.kind === 'copied' || outcome.kind === 'urlOpened' || outcome.kind === 'pasteSent') return exactKeys(outcome, ['kind']) ? { kind: outcome.kind } : null
  if (outcome.kind === 'failed') {
    return exactKeys(outcome, ['code', 'kind', 'sourceConsumed']) &&
      resultActionErrors.has(outcome.code as ResultActionErrorCode) && typeof outcome.sourceConsumed === 'boolean'
      ? { kind: 'failed', code: outcome.code as ResultActionErrorCode, sourceConsumed: outcome.sourceConsumed } : null
  }
  if (outcome.kind !== 'resultsUpdated' || !exactKeys(outcome, ['kind', 'response'])) return null
  const response = plainRecord(outcome.response)
  if (!response || !Object.keys(response).every((key) => ['requestId', 'items', 'mainResultCommand', 'commandHint'].includes(key)) ||
      !opaqueResultHandle(response.requestId) || !Array.isArray(response.items) ||
      !exactDenseArray(response.items) || response.items.length > 20 || !plainRecord(response.mainResultCommand) ||
      (Object.prototype.hasOwnProperty.call(response, 'commandHint') && typeof response.commandHint !== 'string')) return null
  const command = plainRecord(response.mainResultCommand)
  if (!command || !exactKeys(command, ['argument', 'commandId', 'commandLabel', 'pluginId']) ||
      ['argument', 'commandId', 'commandLabel', 'pluginId'].some((key) => typeof command[key] !== 'string')) return null
  const handles = new Set<string>()
  for (const value of response.items) {
    const row = plainRecord(value)
    const activation = row && plainRecord(row.activation)
    if (!row || !Object.keys(row).every((key) => [
      'resultId', 'activation', 'title', 'subtitle', 'pluginIconUrl', 'detail',
      'hasDefaultAction', 'actions', 'defaultActionId',
    ].includes(key)) || !opaqueResultHandle(row.resultId) || handles.has(row.resultId) ||
        typeof row.title !== 'string' || !activation || !exactKeys(activation, ['kind']) ||
        activation.kind !== 'executeResult' ||
        parseResultActionMetadata(row) == null) return null
    if (['subtitle', 'detail', 'pluginIconUrl'].some((key) =>
      Object.prototype.hasOwnProperty.call(row, key) && typeof row[key] !== 'string')) return null
    handles.add(row.resultId)
  }
  return { kind: 'resultsUpdated', response: response as unknown as SearchResponse }
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
  'files.openText', 'ui.window', 'ui.panel', 'clipboard.write', 'clipboard.paste', 'url.open', 'clipboard.read', 'clipboard.history.read', 'clipboard.history.paste', 'network.https',
  'files.userSelected', 'files.index.readAll', 'notifications.publish', 'timer.control', 'background.schedule',
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
  const keys = ['commands', 'description', 'enabled', 'fault', 'generation', 'iconUrl', 'name', 'network', 'permissions', 'pluginId', 'settings', 'source', 'version']
  if (!item || !exactKeys(item, keys) || typeof item.pluginId !== 'string' || item.pluginId.length === 0 || typeof item.name !== 'string' || typeof item.version !== 'string' || typeof item.enabled !== 'boolean' || item.source !== 'localPackage' || !Number.isSafeInteger(item.generation) || (item.description !== null && typeof item.description !== 'string') || (item.iconUrl !== null && safePublicPluginIconUrl(item.iconUrl) === undefined) || (item.fault !== null && (typeof item.fault !== 'string' || !publicFaults.has(item.fault as PublicPluginFault)))) return null
  if (!Array.isArray(item.permissions) || !exactDenseArray(item.permissions) || !Array.isArray(item.settings) || !exactDenseArray(item.settings)) return null
  if (!Array.isArray(item.commands) || !exactDenseArray(item.commands) || item.commands.length < 1 || item.commands.length > 16) return null
  const commands: PublicPluginInventoryCommand[] = []
  const commandIds = new Set<string>()
  const effectiveNames = new Set<string>()
  for (const valueCommand of item.commands) {
    const command = plainRecord(valueCommand)
    if (!command || !exactKeys(command, ['commandId', 'defaultName', 'effectiveName', 'favorite', 'hotkey', 'hotkeyStatus', 'nameOverride']) ||
      typeof command.commandId !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(command.commandId) || commandIds.has(command.commandId) ||
      typeof command.defaultName !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(command.defaultName) ||
      typeof command.effectiveName !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(command.effectiveName) || effectiveNames.has(command.effectiveName) ||
      (command.nameOverride !== null && (typeof command.nameOverride !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/u.test(command.nameOverride))) ||
      command.effectiveName !== (command.nameOverride ?? command.defaultName) || typeof command.favorite !== 'boolean' ||
      (command.hotkey !== null && (typeof command.hotkey !== 'string' || !parseCommandHotkeyEntry({ pluginId: item.pluginId, commandId: command.commandId, hotkey: command.hotkey, bindingRevision: '1' }))) ||
      !['unset', 'active', 'suspended', 'unavailable'].includes(command.hotkeyStatus as string) || ((command.hotkey === null) !== (command.hotkeyStatus === 'unset'))) return null
    commandIds.add(command.commandId)
    effectiveNames.add(command.effectiveName)
    commands.push(command as unknown as PublicPluginInventoryCommand)
  }
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
  const network = parseInventoryNetwork(item.network)
  if (network === undefined) return null
  const networkPermission = permissions.find(({ permission }) => permission === 'network.https')
  if ((network === null) !== (networkPermission === undefined)) return null
  return { ...(item as unknown as PublicPluginInventoryItem), commands, network, permissions, settings }
}

export function parsePublicPluginInventory(value: unknown): PublicPluginInventory | null {
  const inventory = plainRecord(value)
  if (!inventory || !exactKeys(inventory, ['hotkeyRevision', 'items', 'revision']) || !canonicalU64(inventory.hotkeyRevision) || !canonicalU64(inventory.revision) || !Array.isArray(inventory.items) || !exactDenseArray(inventory.items)) return null
  const items: PublicPluginInventoryItem[] = []
  const ids = new Set<string>()
  for (const valueItem of inventory.items) {
    const item = parsePublicPluginItem(valueItem)
    if (!item || !ids.add(item.pluginId)) return null
    items.push(item)
  }
  return { revision: inventory.revision, hotkeyRevision: inventory.hotkeyRevision as string, items }
}

export function parsePublicPluginPrepareSummary(value: unknown): PublicPluginPrepareSummary | null {
  const summary = plainRecord(value)
  const keys = ['iconUrl', 'isUpdate', 'name', 'network', 'operation', 'permissions', 'pluginId', 'previousVersion', 'sourceVerified', 'token', 'version']
  if (!summary || !exactKeys(summary, keys)) return null
  if (typeof summary.token !== 'string' || summary.token.length === 0 || typeof summary.pluginId !== 'string' || !PUBLIC_PLUGIN_ID.test(summary.pluginId)) return null
  if (typeof summary.name !== 'string' || summary.name.length === 0 || !canonicalVersion(summary.version)) return null
  if (!['install', 'replace', 'developmentRefresh'].includes(summary.operation as string)) return null
  if (summary.operation === 'install' ? summary.isUpdate !== false || summary.previousVersion !== null : summary.isUpdate !== true || !canonicalVersion(summary.previousVersion)) return null
  if (summary.operation === 'developmentRefresh' && summary.previousVersion !== summary.version) return null
  if (typeof summary.isUpdate !== 'boolean' || typeof summary.sourceVerified !== 'boolean') return null
  if (summary.iconUrl !== null && safePublicPluginIconUrl(summary.iconUrl) === undefined) return null
  if (!Array.isArray(summary.permissions) || Object.getPrototypeOf(summary.permissions) !== Array.prototype || !exactDenseArray(summary.permissions)) return null
  const permissions: PublicPermission[] = []
  for (const permission of summary.permissions) {
    if (typeof permission !== 'string' || !publicPermissions.has(permission as PublicPermission) || permissions.includes(permission as PublicPermission)) return null
    permissions.push(permission as PublicPermission)
  }
  const network = parsePrepareNetwork(summary.network)
  if (network === undefined) return null
  if ((network === null) !== !permissions.includes('network.https')) return null
  if (network !== null && !summary.isUpdate) {
    if (!network.requiresNetworkConsent || network.addedHttpsHosts.length !== network.httpsHosts.length) return null
    if (network.addedHttpsHosts.some((host, index) => host !== network.httpsHosts[index])) return null
    if (network.addedCredentialIds.length !== network.credentials.length
      || network.addedCredentialIds.some((id, index) => id !== network.credentials[index].id)) return null
  }
  return { ...(summary as unknown as PublicPluginPrepareSummary), network, permissions }
}

export function parsePublicPluginWindowIdentity(value: unknown): PublicPluginWindowIdentity | null {
  const identity = plainRecord(value)
  if (!identity || !exactKeys(identity, ['iconUrl', 'name']) || typeof identity.name !== 'string' || identity.name.length === 0) return null
  if (identity.iconUrl !== null && safePublicPluginIconUrl(identity.iconUrl) === undefined) return null
  return identity as unknown as PublicPluginWindowIdentity
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

function canonicalHttpsHost(value: unknown): value is string {
  if (typeof value !== 'string' || value.length === 0 || value.length > 253) return false
  if (value === 'localhost' || value.endsWith('.localhost') || value.endsWith('.local')) return false
  const labels = value.split('.')
  if (labels.length < 2) return false
  if (labels.some((label) => label.length === 0 || label.length > 63 || label.startsWith('xn--') || !PUBLIC_HTTPS_HOST_LABEL.test(label))) return false
  if (labels.length === 4 && labels.every((label) => /^\d+$/.test(label))) {
    const octets = labels.map(Number)
    if (octets.every((octet) => octet <= 255)) return false
  }
  return true
}

function parseHttpsHosts(value: unknown, allowEmpty = false): string[] | null {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype || !exactDenseArray(value)) return null
  if (value.length > 8 || (!allowEmpty && value.length === 0)) return null
  const hosts: string[] = []
  for (const host of value) {
    if (!canonicalHttpsHost(host) || (hosts.length > 0 && hosts[hosts.length - 1] >= host)) return null
    hosts.push(host)
  }
  return hosts
}

function parseNetworkCredentials(value: unknown, hosts: readonly string[]): PluginNetworkCredential[] | null {
  if (!Array.isArray(value) || !exactDenseArray(value) || value.length > 8) return null
  const credentials: PluginNetworkCredential[] = []
  for (const item of value) {
    const c = plainRecord(item)
    if (!c || !exactKeys(c, ['host', 'id', 'settingKey', 'type'])
      || typeof c.id !== 'string' || !/^[a-z][a-z0-9.-]{0,63}$/.test(c.id)
      || typeof c.settingKey !== 'string' || !/^[a-z][a-z0-9.-]{0,63}$/.test(c.settingKey)
      || typeof c.host !== 'string' || !hosts.includes(c.host) || c.type !== 'bearer'
      || (credentials.length > 0 && credentials[credentials.length - 1].id >= c.id)) return null
    credentials.push({ id: c.id, settingKey: c.settingKey, host: c.host, type: 'bearer' })
  }
  return credentials
}

function parseInventoryNetwork(value: unknown): PublicPluginInventoryNetwork | null | undefined {
  if (value === null) return null
  const network = plainRecord(value)
  if (!network || !exactKeys(network, ['credentials', 'httpsHosts'])) return undefined
  const httpsHosts = parseHttpsHosts(network.httpsHosts)
  const credentials = httpsHosts && parseNetworkCredentials(network.credentials, httpsHosts)
  return httpsHosts && credentials ? { httpsHosts, credentials } : undefined
}

function parsePrepareNetwork(value: unknown): PublicPluginPrepareNetwork | null | undefined {
  if (value === null) return null
  const network = plainRecord(value)
  if (!network || !exactKeys(network, ['addedCredentialIds', 'addedHttpsHosts', 'credentials', 'httpsHosts', 'requiresNetworkConsent'])) return undefined
  const httpsHosts = parseHttpsHosts(network.httpsHosts)
  const addedHttpsHosts = parseHttpsHosts(network.addedHttpsHosts, true)
  if (!httpsHosts || !addedHttpsHosts || typeof network.requiresNetworkConsent !== 'boolean') return undefined
  const credentials = parseNetworkCredentials(network.credentials, httpsHosts)
  const addedCredentialIds = network.addedCredentialIds
  if (!credentials || !Array.isArray(addedCredentialIds) || !exactDenseArray(addedCredentialIds)
    || addedCredentialIds.some((id, i) => typeof id !== 'string' || !credentials.some((c) => c.id === id) || (i > 0 && addedCredentialIds[i - 1] >= id))) return undefined
  if (addedHttpsHosts.some((host) => !httpsHosts.includes(host))) return undefined
  if (network.requiresNetworkConsent !== (addedHttpsHosts.length > 0 || addedCredentialIds.length > 0)) return undefined
  return { httpsHosts, credentials, addedHttpsHosts, addedCredentialIds, requiresNetworkConsent: network.requiresNetworkConsent }
}

export function parsePluginPanelCommandResult(value: unknown): PluginPanelCommandResult | null {
  const record = plainRecord(value)
  if (!record) return null
  const keys = record.inputPlaceholder === undefined
    ? ['commandLabel', 'hostKeys', 'pluginId', 'sessionEpoch']
    : ['commandLabel', 'hostKeys', 'inputPlaceholder', 'pluginId', 'sessionEpoch']
  if (!exactKeys(record, keys)) return null
  const sessionEpoch = parseU64Decimal(record.sessionEpoch)
  if (!Array.isArray(record.hostKeys) || !exactDenseArray(record.hostKeys)) return null
  const order: Readonly<Record<PanelHostKeyDeclaration, number>> = {
    ArrowDown: 0,
    ArrowUp: 1,
    'Primary+N': 2,
    Tab: 3,
    'Shift+Tab': 4,
    Enter: 5,
  }
  const hostKeys: PanelHostKeyDeclaration[] = []
  const seen = new Set<PanelHostKeyDeclaration>()
  for (const key of record.hostKeys) {
    if (typeof key !== 'string' || !(key in order)) return null
    const declaration = key as PanelHostKeyDeclaration
    if (seen.has(declaration)) return null
    seen.add(declaration)
    hostKeys.push(declaration)
  }
  if (
    sessionEpoch === null ||
    sessionEpoch === '0' ||
    typeof record.pluginId !== 'string' ||
    record.pluginId.length > 64 ||
    !PUBLIC_PLUGIN_ID.test(record.pluginId) ||
    typeof record.commandLabel !== 'string' ||
    !/^[a-z][a-z0-9-]{0,31}$/u.test(record.commandLabel) ||
    (record.inputPlaceholder !== undefined && typeof record.inputPlaceholder !== 'string')
  ) return null
  hostKeys.sort((left, right) => order[left] - order[right])
  return {
    sessionEpoch,
    pluginId: record.pluginId,
    commandLabel: record.commandLabel,
    ...(record.inputPlaceholder === undefined ? {} : { inputPlaceholder: record.inputPlaceholder }),
    hostKeys,
  }
}

export function parsePluginPanelHostKeyEnqueueResult(value: unknown): PluginPanelHostKeyEnqueueResult | null {
  const record = plainRecord(value)
  if (!record || typeof record.outcome !== 'string') return null
  if (record.outcome === 'enqueued') {
    if (!exactKeys(record, ['outcome', 'routeSequence'])) return null
    const routeSequence = parseU64Decimal(record.routeSequence)
    return routeSequence === null || routeSequence === '0' ? null : { outcome: 'enqueued', routeSequence }
  }
  if (!exactKeys(record, ['outcome'])) return null
  return ['droppedQueueFull', 'noop', 'protocolViolation'].includes(record.outcome)
    ? { outcome: record.outcome as 'droppedQueueFull' | 'noop' | 'protocolViolation' }
    : null
}

export function parsePluginPanelErrorEvent(value: unknown): PluginPanelErrorEvent | null {
  const record = plainRecord(value)
  if (!record || !exactKeys(record, ['sessionEpoch'])) return null
  const sessionEpoch = parseU64Decimal(record.sessionEpoch)
  return sessionEpoch === null || sessionEpoch === '0' ? null : { sessionEpoch }
}

export function parsePluginPanelFocusHostInputEvent(value: unknown): PluginPanelFocusHostInputEvent | null {
  const record = plainRecord(value)
  if (!record || !exactKeys(record, ['focusRequestId', 'sessionEpoch'])) return null
  const sessionEpoch = parseU64Decimal(record.sessionEpoch)
  const focusRequestId = parseU64Decimal(record.focusRequestId)
  return sessionEpoch === null || sessionEpoch === '0' || focusRequestId === null || focusRequestId === '0'
    ? null
    : { sessionEpoch, focusRequestId }
}

function validTimerDuration(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 1_000 && value <= 86_400_000
}

function validTimerMessage(value: unknown): value is string {
  if (typeof value !== 'string' || value.trim().length === 0) return false
  let scalars = 0
  for (const character of value) {
    const point = character.codePointAt(0)
    if (point === undefined || (point >= 0xd800 && point <= 0xdfff) || point <= 0x1f || (point >= 0x7f && point <= 0x9f)) {
      return false
    }
    scalars += 1
    if (scalars > 500) return false
  }
  return true
}

export function parsePluginTimerStartInput(value: unknown): PluginTimerStartInput | null {
  const input = plainRecord(value)
  if (
    !input ||
    !exactKeys(input, ['completionMessage', 'durationMs']) ||
    !validTimerDuration(input.durationMs) ||
    !validTimerMessage(input.completionMessage)
  ) {
    return null
  }
  return { durationMs: input.durationMs, completionMessage: input.completionMessage }
}

export function parsePluginTimerState(value: unknown): PluginTimerState | null {
  const state = plainRecord(value)
  if (!state || !exactKeys(state, ['durationMs', 'phase', 'remainingMs', 'timerRevision'])) return null
  const timerRevision = parseU64Decimal(state.timerRevision)
  if (!timerRevision || !['idle', 'running', 'paused', 'fired'].includes(String(state.phase))) return null

  if (state.phase === 'idle') {
    if (state.durationMs === null && state.remainingMs === null) {
      return { timerRevision, phase: 'idle', durationMs: null, remainingMs: null }
    }
    if (!validTimerDuration(state.durationMs) || state.remainingMs !== state.durationMs) return null
    return { timerRevision, phase: 'idle', durationMs: state.durationMs, remainingMs: state.remainingMs }
  }

  if (!validTimerDuration(state.durationMs) || !Number.isSafeInteger(state.remainingMs)) return null
  if (typeof state.remainingMs !== 'number' || state.remainingMs < 0 || state.remainingMs > state.durationMs) return null
  if (state.phase === 'fired') {
    return state.remainingMs === 0
      ? { timerRevision, phase: 'fired', durationMs: state.durationMs, remainingMs: 0 }
      : null
  }
  return {
    timerRevision,
    phase: state.phase as 'running' | 'paused',
    durationMs: state.durationMs,
    remainingMs: state.remainingMs,
  }
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

const FIND_THUMBNAIL_PREFIX = 'data:image/png;base64,'
const FIND_THUMBNAIL_BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/

export function parseFindThumbnailDataUrl(value: unknown): string | null {
  if (
    typeof value !== 'string' ||
    value.length > 524_320 ||
    !value.startsWith(FIND_THUMBNAIL_PREFIX)
  ) return null
  const payload = value.slice(FIND_THUMBNAIL_PREFIX.length)
  return payload.length > 0 && FIND_THUMBNAIL_BASE64.test(payload) ? value : null
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

export function parseRefreshAtUtc(value: unknown): string | null {
  return typeof value === 'string' && strictUtcRfc3339(value) ? value : null
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


export type PublicPluginManagementCode = 'invalidPackage' | 'incompatiblePlatform' | 'incompatibleApi' | 'unsupportedPermission' | 'permissionDenied' | 'runtimeNotReady' | 'nameConflict' | 'invalidToken' | 'expiredToken' | 'invalidCaller' | 'dataCleanupPending' | 'pluginUnavailable' | 'refreshTargetMismatch' | 'refreshVersionMismatch' | 'staleInventory'
export interface PublicPluginDiagnosticReport {
  schemaVersion: 1
  hostVersion: string
  platform: 'windows' | 'macos'
  pluginId: string
  state: null | { status: 'notInstalled'; inventoryRevision: string } | { status: 'installed'; packageVersion: string; generation: string; inventoryRevision: string; enabled: boolean; fault: PublicPluginFault | null }
  lastOperation: null | { operation: 'prepare' | 'developmentRefresh' | 'replace' | 'enable'; phase: 'validation' | 'readiness' | 'persistence' | 'publication' | 'complete'; errorCode: PublicPluginManagementCode | null; stateCommitOutcome: 'notAttempted' | 'committed' | 'notCommitted' | 'unknown' }
}
const managementCodes = new Set<string>(['invalidPackage', 'incompatiblePlatform', 'incompatibleApi', 'unsupportedPermission', 'permissionDenied', 'runtimeNotReady', 'nameConflict', 'invalidToken', 'expiredToken', 'invalidCaller', 'dataCleanupPending', 'pluginUnavailable', 'refreshTargetMismatch', 'refreshVersionMismatch', 'staleInventory'])
// Copy and preview share this canonical field order with Rust's explicit DTO.
export function formatPublicPluginDiagnostics(report: PublicPluginDiagnosticReport): string { return JSON.stringify(report, null, 2) }
function diagnosticRecord(value: unknown): Record<string, unknown> | null {
  const record = plainRecord(value)
  if (!record || Object.values(Object.getOwnPropertyDescriptors(record)).some((descriptor) => !('value' in descriptor) || !descriptor.enumerable)) return null
  return record
}
export function parsePublicPluginDiagnostics(value: unknown): PublicPluginDiagnosticReport | null {
  const r = diagnosticRecord(value)
  if (!r || !exactKeys(r, ['hostVersion', 'lastOperation', 'platform', 'pluginId', 'schemaVersion', 'state']) || r.schemaVersion !== 1 || !canonicalVersion(r.hostVersion) || !['windows', 'macos'].includes(r.platform as string) || typeof r.pluginId !== 'string' || r.pluginId.length > 64 || !PUBLIC_PLUGIN_ID.test(r.pluginId)) return null
  let state: PublicPluginDiagnosticReport['state'] = null
  if (r.state !== null) {
    const st = diagnosticRecord(r.state)
    if (!st || !canonicalU64(st.inventoryRevision)) return null
    if (st.status === 'notInstalled') {
      if (!exactKeys(st, ['inventoryRevision', 'status'])) return null
      state = { status: 'notInstalled', inventoryRevision: st.inventoryRevision }
    } else if (st.status === 'installed') {
      if (!exactKeys(st, ['enabled', 'fault', 'generation', 'inventoryRevision', 'packageVersion', 'status']) || !canonicalVersion(st.packageVersion) || !canonicalU64(st.generation) || st.generation === '0' || typeof st.enabled !== 'boolean' || (st.fault !== null && st.fault !== 'runtimeUnavailable' && st.fault !== 'consecutiveFailures') || (st.enabled && st.fault !== null)) return null
      state = { status: 'installed', packageVersion: st.packageVersion, generation: st.generation, inventoryRevision: st.inventoryRevision, enabled: st.enabled, fault: st.fault }
    } else return null
  }
  let lastOperation: PublicPluginDiagnosticReport['lastOperation'] = null
  if (r.lastOperation !== null) {
    const op = diagnosticRecord(r.lastOperation)
    if (!op || !exactKeys(op, ['errorCode', 'operation', 'phase', 'stateCommitOutcome']) || !['prepare', 'developmentRefresh', 'replace', 'enable'].includes(op.operation as string) || !['validation', 'readiness', 'persistence', 'publication', 'complete'].includes(op.phase as string) || !['notAttempted', 'committed', 'notCommitted', 'unknown'].includes(op.stateCommitOutcome as string) || (op.errorCode !== null && (typeof op.errorCode !== 'string' || !managementCodes.has(op.errorCode)))) return null
    if ((op.phase === 'validation' || op.phase === 'readiness') && op.stateCommitOutcome !== 'notAttempted') return null
    if (op.phase === 'publication' && op.stateCommitOutcome !== 'committed') return null
    if (op.phase === 'complete' && (!['notAttempted', 'committed'].includes(op.stateCommitOutcome as string) || op.errorCode !== null)) return null
    lastOperation = { operation: op.operation, phase: op.phase, errorCode: op.errorCode, stateCommitOutcome: op.stateCommitOutcome } as PublicPluginDiagnosticReport['lastOperation']
  }
  const report: PublicPluginDiagnosticReport = { schemaVersion: 1, hostVersion: r.hostVersion, platform: r.platform as 'windows' | 'macos', pluginId: r.pluginId, state, lastOperation }
  return new TextEncoder().encode(formatPublicPluginDiagnostics(report)).length <= 16384 ? report : null
}
