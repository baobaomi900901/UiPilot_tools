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

export interface UiPilotPluginApiV1 {
  readonly storage: {
    get(key: string): Promise<JsonValue | null>
    set(key: string, value: JsonValue): Promise<void>
    remove(key: string): Promise<void>
  }
  readonly settings: {
    get(key: string): Promise<JsonValue | null>
    isSecretConfigured(key: string): Promise<boolean>
  }
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

export type PluginResponse = MainResultResponse | WindowResponse

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

export interface UiPilotPluginWindowApiV1 {
  onUpdate(
    handler: (update: Readonly<PluginWindowUpdate>) => void | Promise<void>,
  ): () => void
}

declare global {
  interface Window {
    readonly uipilotPluginWindow: Readonly<UiPilotPluginWindowApiV1>
  }
}
