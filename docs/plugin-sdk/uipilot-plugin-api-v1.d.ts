export type JsonPrimitive = null | boolean | number | string
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue }

export interface PluginInvocation {
  apiVersion: 1
  requestId: string
  /** Host 0.5.0+: commands packages always receive the selected ID.
   * Legacy command packages omit this key entirely (not null or default).
   * Dispatch within one onCommand by ID, never by startup name or array order.
   */
  commandId?: string
  input: string
  context: {
    platform: 'windows' | 'macos'
    theme: 'dark' | 'light'
    invokedAt: string
  }
}

export type PluginNetworkRequestBody =
  | Readonly<{ type: 'json'; value: unknown }>
  | Readonly<{ type: 'text'; value: string }>
  | Readonly<{ type: 'form'; value: Readonly<Record<string, string>> }>

/** Manifest metadata only; no token getter. The destination service may echo a consumed token. */
export interface PluginNetworkCredential {
  readonly id: string
  readonly settingKey: string
  readonly host: string
  readonly type: 'bearer'
}

export interface PluginNetworkRequest {
  readonly url: string
  readonly method: 'GET' | 'POST'
  readonly headers?: Readonly<Record<string, string>>
  readonly body?: PluginNetworkRequestBody
  /** Credential ID declared for this host (Host 0.4.0+). The service receives the token and may echo it in its response. */
  readonly credential?: string
}

export interface PluginNetworkResponse {
  readonly status: number
  readonly headers: Readonly<Record<string, readonly string[]>>
  readonly body: string
}

export interface PluginNetworkApi {
  /** Request-bound. Secret saves cancel old contexts; failures do not promise rollback or permanently disable recovery.
   * The service receives an injected bearer token and may echo it in ordinary response headers/body.
   * Fixed rejection names are PluginNetworkErrorName; CredentialUnavailableError hides record/format causes.
   */
  request(input: Readonly<PluginNetworkRequest>): Promise<Readonly<PluginNetworkResponse>>
}

export type PluginNetworkErrorName =
  | 'InvalidNetworkRequestError'
  | 'PermissionDeniedError'
  | 'NetworkTargetDeniedError'
  | 'NetworkTimeoutError'
  | 'NetworkFailureError'
  | 'CredentialUnavailableError'
  | 'NetworkResponseTooLargeError'
  | 'NetworkResponseInvalidError'
  | 'NetworkLimitExceededError'
  | 'ExpiredRequestError'

export interface UiPilotPluginApiV1 {
  readonly network?: Readonly<PluginNetworkApi>
  readonly storage: {
    get(key: string): Promise<JsonValue | null>
    set(key: string, value: JsonValue): Promise<void>
    remove(key: string): Promise<void>
  }
  readonly settings: {
    get(key: string): Promise<JsonValue | null>
    /** Presence only, not bearer validity or network consent. No plaintext getter exists. */
    isSecretConfigured(key: string): Promise<boolean>
  }
  readonly notifications: {
    publish(input: Readonly<PluginNotificationPublishInput>): Promise<void>
    schedule(input: Readonly<PluginNotificationScheduleInput>): Promise<void>
  }
}

export interface PluginNotificationPublishInput {
  readonly content: string
}

export interface PluginNotificationScheduleInput {
  readonly content: string
  readonly delayMs: number
}

export interface CopyTextDefaultAction {
  type: 'copyText'
  text: string
}

export interface PluginResult {
  actions?: never
  defaultActionId?: never
  id: string
  title: string
  subtitle?: string | null
  detail?: string | null
  defaultAction?: CopyTextDefaultAction | null
}

export interface MainResultResponse {
  requestId: string
  results: (PluginResult | PluginActionResult)[]
}

export interface WindowResponse {
  requestId: string
  data: JsonValue
}

export interface PanelResponse {
  requestId: string
  data: JsonValue
}

export type PluginResponse = MainResultResponse | WindowResponse | PanelResponse

export type PluginHandler = (
  invocation: Readonly<PluginInvocation>,
  api: Readonly<UiPilotPluginApiV1>,
) => Promise<PluginResponse>

export interface PluginPanelRequest { action: string; payload: JsonValue }
export interface PluginPanelActionInvocation {
  apiVersion: 1
  requestId: string
  action: string
  payload: JsonValue
  context: { platform: 'windows'; theme: 'dark' | 'light'; invokedAt: string }
}
export interface PluginPanelActionResponse { requestId: string; data: JsonValue }
export type PluginPanelActionHandler = (
  invocation: Readonly<PluginPanelActionInvocation>,
  api: Readonly<UiPilotPluginApiV1>,
) => Promise<PluginPanelActionResponse>

export interface PluginWindowRequest { action: string; payload: JsonValue }
export interface PluginWindowActionInvocation {
  apiVersion: 1
  requestId: string
  action: string
  payload: JsonValue
  context: { platform: 'windows'; theme: 'dark' | 'light'; invokedAt: string }
}
export interface PluginWindowActionResponse { requestId: string; data: JsonValue }
export type PluginWindowActionHandler = (
  invocation: Readonly<PluginWindowActionInvocation>,
  api: Readonly<UiPilotPluginApiV1>,
) => Promise<PluginWindowActionResponse>

export interface PluginRuntimeModule {
  /** Required at readiness when runtime.resultActions is true (Windows Host 0.10.0+). */
  onAction?: PluginActionHandler
  /** Required at readiness only when window.runtimeActions is true (Windows Host 0.9.0+). */
  onWindowAction?: PluginWindowActionHandler
  /** Required at readiness only when panel.runtimeActions is true (Windows Host 0.6.0+). */
  onPanelAction?: PluginPanelActionHandler
  onCommand: PluginHandler
}

export interface PluginWindowUpdate {
  requestId: string
  input: string
  platform: 'windows' | 'macos'
  theme: 'dark' | 'light'
  invokedAt: string
  instanceNumber: 1
  data: JsonValue
}

export type U64Decimal = string

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

export interface UiPilotPluginWindowTimerApiV1 {
  getState(): Promise<PluginTimerState>
  start(input?: Readonly<PluginTimerStartInput>): Promise<PluginTimerState>
  stop(): Promise<PluginTimerState>
  reset(): Promise<PluginTimerState>
  onStateChanged(handler: (state: Readonly<PluginTimerState>) => void): () => void
}

export interface UiPilotPluginWindowStorageApiV1 {
  get(key: string): Promise<JsonValue | null>
  set(key: string, value: JsonValue): Promise<void>
  remove(key: string): Promise<void>
}

/** Windows Host 0.13.0+, declared and granted files.openText. Maximum 8192 original UTF-8 bytes; null means cancellation. */
export interface UiPilotPluginWindowFilesApiV1 {
  openText(): Promise<Readonly<{ name: string; text: string; byteLength: number }> | null>
}
export interface UiPilotPluginWindowApiV1 {
  readonly files?: Readonly<UiPilotPluginWindowFilesApiV1>
  /** Session-bound; call after onUpdate has returned and Host has activated the window. */
  readonly request?: (input: Readonly<PluginWindowRequest>) => Promise<JsonValue>
  onUpdate(
    handler: (update: Readonly<PluginWindowUpdate>) => void | Promise<void>,
  ): () => void
  readonly timer: Readonly<UiPilotPluginWindowTimerApiV1>
  readonly storage: Readonly<UiPilotPluginWindowStorageApiV1>
  close(): Promise<void>
}

export interface PluginPanelUpdate {
  requestId: string
  input: string
  platform: 'windows' | 'macos'
  theme: 'dark' | 'light'
  invokedAt: string
  sessionEpoch: U64Decimal
  data: JsonValue
}

export interface UiPilotPluginPanelStorageApiV1 {
  get(key: string): Promise<JsonValue | null>
  set(key: string, value: JsonValue): Promise<void>
  remove(key: string): Promise<void>
}

export type ClipboardHistoryEntrySummary =
  | Readonly<{
      id: string
      kind: 'text'
      capturedAt: string
      textPreview: string
    }>
  | Readonly<{
      id: string
      kind: 'image'
      capturedAt: string
      previewDataUrl: string
      width: number
      height: number
    }>
  | Readonly<{
      id: string
      kind: 'files'
      capturedAt: string
      firstFileName: string
      fileCount: number
      available: boolean
    }>

export interface ClipboardHistorySnapshot {
  revision: U64Decimal
  entries: readonly ClipboardHistoryEntrySummary[]
}

export type ClipboardHistoryPasteErrorName =
  | 'PermissionDenied'
  | 'ExpiredPanelSession'
  | 'RecordNotFound'
  | 'RecordUnavailable'
  | 'PasteTargetUnavailable'
  | 'ClipboardWriteFailed'

export interface UiPilotPluginPanelClipboardHistoryApiV1 {
  list(): Promise<Readonly<ClipboardHistorySnapshot>>
  onChanged(
    handler: (snapshot: Readonly<ClipboardHistorySnapshot>) => void | Promise<void>,
  ): () => void
  paste(input: Readonly<{
    id: string
    routeSequence: U64Decimal
  }>): Promise<Readonly<{ outcome: 'admitted' }>>
  remove(input: Readonly<{ id: string }>): Promise<void>
  clear(): Promise<void>
}

export type PluginPanelHostKey = 'ArrowDown' | 'ArrowUp' | 'n' | 'Tab' | 'Enter'

export interface PluginPanelHostKeyEvent {
  key: PluginPanelHostKey
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
  sessionEpoch: U64Decimal
  routeSequence: U64Decimal
}

export interface UiPilotPluginPanelApiV1 {
  /** Present only with panel.runtimeActions. Fresh frozen JSON; one idle-only call, no retry.
   * Do not await inside onUpdate/onHostKey. Cancellation may follow a committed write: reload.
   */
  readonly request?: (input: Readonly<PluginPanelRequest>) => Promise<JsonValue>
  onUpdate(
    handler: (update: Readonly<PluginPanelUpdate>) => void | Promise<void>,
  ): () => void
  onHostKey(
    handler: (event: Readonly<PluginPanelHostKeyEvent>) => void | Promise<void>,
  ): () => void
  focusHostInput(): Promise<void>
  requestHide(): Promise<void>
  readonly storage: Readonly<UiPilotPluginPanelStorageApiV1>
  readonly clipboardHistory: Readonly<UiPilotPluginPanelClipboardHistoryApiV1>
}

declare global {
  interface Window {
    readonly uipilotPluginWindow: Readonly<UiPilotPluginWindowApiV1>
    readonly uipilotPluginPanel: Readonly<UiPilotPluginPanelApiV1>
  }
}

/** Windows Host 0.10.0+, runtime.resultActions: true. IDs are business strings, not Host handles. */
export type PluginResultAction =
  | { id: string; title: string; type: 'copyText'; text: string }
  /** Windows Host 0.14.0+, clipboard.paste + clipboard.write. 1..8192 UTF-8 bytes, no NUL. */
  | { id: string; title: string; type: 'pasteText'; text: string }
  | { id: string; title: string; type: 'runtime'; payload: JsonValue }
  /** Windows Host 0.12.0+, declared and granted url.open. Explicit user action only. */
  | { id: string; title: string; type: 'openUrl'; url: string }
export interface PluginActionResult {
  defaultAction?: never
  id: string
  title: string
  subtitle?: string | null
  detail?: string | null
  actions: PluginResultAction[]
  /** Omit to require an explicit menu selection. Never defaults to the first action. */
  defaultActionId?: string
}
export interface PluginActionInvocation {
  apiVersion: 1
  requestId: string
  /** Always the normalized saved ID; opted-in legacy command uses default. */
  commandId: string
  resultId: string
  actionId: string
  payload: JsonValue
  input: string
  context: { platform: 'windows'; theme: 'dark' | 'light'; invokedAt: string }
}
/** Export onAction as well as onCommand, including copy-only packages. Fresh frozen request-bound API. */
export type PluginActionHandler = (
  invocation: Readonly<PluginActionInvocation>, api: Readonly<UiPilotPluginApiV1>,
) => Promise<MainResultResponse>
