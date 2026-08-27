export type JsonPrimitive = null | boolean | number | string
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue }

export interface PluginInvocation {
  apiVersion: 1
  requestId: string
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

export interface PluginNetworkRequest {
  readonly url: string
  readonly method: 'GET' | 'POST'
  readonly headers?: Readonly<Record<string, string>>
  readonly body?: PluginNetworkRequestBody
}

export interface PluginNetworkResponse {
  readonly status: number
  readonly headers: Readonly<Record<string, readonly string[]>>
  readonly body: string
}

export interface PluginNetworkApi {
  request(input: Readonly<PluginNetworkRequest>): Promise<Readonly<PluginNetworkResponse>>
}

export type PluginNetworkErrorName =
  | 'InvalidNetworkRequestError'
  | 'PermissionDeniedError'
  | 'NetworkTargetDeniedError'
  | 'NetworkTimeoutError'
  | 'NetworkFailureError'
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
  id: string
  title: string
  subtitle?: string
  detail?: string
  defaultAction?: CopyTextDefaultAction
}

export interface MainResultResponse {
  requestId: string
  results: PluginResult[]
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

export interface PluginRuntimeModule {
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

export interface UiPilotPluginWindowApiV1 {
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

export type PluginPanelHostKey = 'ArrowDown' | 'ArrowUp' | 'n'

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
  onUpdate(
    handler: (update: Readonly<PluginPanelUpdate>) => void | Promise<void>,
  ): () => void
  onHostKey(
    handler: (event: Readonly<PluginPanelHostKeyEvent>) => void | Promise<void>,
  ): () => void
  focusHostInput(): Promise<void>
  requestHide(): Promise<void>
  readonly storage: Readonly<UiPilotPluginPanelStorageApiV1>
}

declare global {
  interface Window {
    readonly uipilotPluginWindow: Readonly<UiPilotPluginWindowApiV1>
    readonly uipilotPluginPanel: Readonly<UiPilotPluginPanelApiV1>
  }
}
