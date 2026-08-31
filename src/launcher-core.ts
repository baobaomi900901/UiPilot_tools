import {
  compareDecimalRevision,
  parseFileSearchResponse,
  parseLauncherShown,
  parsePluginPanelErrorEvent,
  parsePluginPanelFocusHostInputEvent,
  type ClassifiedTextRecord,
  type CommandErrorCode,
  type ControlKey,
  type ExecuteOutcome,
  type FileCategory,
  type FileIndexStatus,
  type FileResultItem,
  type FileResultView,
  type FileSearchResponse,
  type FileSort,
  type FindClient,
  type LauncherClient,
  type LauncherResultActivation,
  type LauncherSnapshot,
  type MainResultCommandContext,
  type MessageCenterStateSnapshot,
  type PluginInventoryView,
  type PluginListStatus,
  type PluginMutationKind,
  type PluginPanelFocusHostInputEvent,
  type PluginPanelBounds,
  type PanelHostKeyDeclaration,
  type PluginPanelHostKey,
  type QuicklinkDraftSnapshot,
  type QuicklinkView,
  type QuicklinksOperation,
  type QuicklinksSnapshot,
  type ResultItem,
  type ResultFavorite,
  type ResultFavoriteTarget,
  type ResultIconKind,
  type SearchResponse,
  type SettingsTabKey,
  type ShowTarget,
  type SettingsLoadStatus,
  type SettingsView,
  type ThemePreference,
  type U64Decimal,
  type UserSettingsUpdate,
  type ViewResult,
  type WebSearchEngine,
} from './protocol'
import { createMessageCenterCore } from './message-center-core'
import { safePublicPluginIconUrl } from './plugin-icon-url'

export interface LauncherCore {
  readonly client: LauncherClient
  readonly getSnapshot: () => LauncherSnapshot
  readonly subscribe: (listener: () => void) => () => void
  readonly start: () => Promise<void>
  readonly preparePanelHostInputFocusListener: () => Promise<boolean>
  readonly failInitialization: () => void
  readonly shown: (payload: unknown) => void
  readonly text: (record: ClassifiedTextRecord) => void
  readonly retireControl: (control: ControlKey) => void
  readonly keyDown: (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean) => void
  readonly routePanelHostKey: (input: PluginPanelHostKeyPhysicalInput) => boolean
  readonly navigate: (target: ShowTarget) => void
  readonly selectSettingsTab: (key: SettingsTabKey) => void
  readonly requestHide: () => Promise<void>
  readonly closeMainResultCommand: () => void
  readonly closePanel: () => Promise<void>
  readonly closeQuicklinks: () => void
  readonly newQuicklink: () => void
  readonly selectQuicklink: (id: string) => void
  readonly completeQuicklink: (id: string) => void
  readonly setQuicklinkDraft: (field: 'name' | 'command' | 'template', value: string) => void
  readonly chooseQuicklinkIcon: () => Promise<void>
  readonly saveQuicklink: () => Promise<void>
  readonly deleteQuicklink: () => Promise<void>
  readonly setPanelBounds: (input: { sessionEpoch: U64Decimal; bounds: PluginPanelBounds }) => void
  readonly settlePanelHostInputFocus: (input: PluginPanelFocusHostInputEvent & { focused: boolean }) => void
  readonly activateResult: (index: number) => void
  readonly openPluginContextMenu: (index: number) => void
  readonly closePluginContextMenu: () => void
  readonly setPluginFavorite: (index: number, favorite: boolean) => void
  readonly setAutostart: (checked: boolean) => void
  readonly setThemePreference: (theme: ThemePreference) => void
  readonly setWebSearchEngine: (engine: WebSearchEngine) => void
  readonly setHotkeyCanonical: (value: string) => void
  readonly setHotkeyRecordingPhase: (phase: HotkeyRecordingPhase) => void
  readonly saveHotkeyCanonical: (value: string) => Promise<void>
  readonly clearMessages: () => Promise<void>
  readonly setFileCategory: (category: FileCategory) => void
  readonly cycleFileCategory: (direction: 'next' | 'previous') => void
  readonly setFileSort: (sort: FileSort) => void
  readonly setFilePreviewEnabled: (enabled: boolean) => void
  readonly resetSettings: () => Promise<void>
  readonly reloadSettings: () => Promise<void>
  readonly activatePlugins: () => Promise<void>
  readonly deactivatePlugins: () => void
  readonly reloadPlugins: () => Promise<void>
  readonly installPlugin: (pluginId: string) => Promise<void>
  readonly reloadPlugin: (pluginId: string) => Promise<void>
  readonly deletePlugin: (pluginId: string) => Promise<void>
  readonly destroy: () => void
}

interface PrivateApplicationResult extends ViewResult {
  kind: 'application'
  resultId: string
  activation: LauncherResultActivation
}

type PrivateResult = PrivateApplicationResult

interface PrivateFileResult {
  resultId: string
  view: FileResultView
}

interface PrivateFileState {
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  durablePreviewEnabled: boolean
  preferencePending: boolean
  total: string
  indexStatus: FileIndexStatus
  latestSeenRevision: bigint
  results: PrivateFileResult[]
  selectedIndex: number
}

export type HotkeyRecordingPhase = 'idle' | 'recording' | 'completed'

const HOTKEY_RECORDING_SHOW_GRACE_MS = 1_000

export interface PluginPanelHostKeyPhysicalInput {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
  isComposing: boolean
  platform: 'windows' | 'macos'
}

interface PrivatePanelState {
  pluginId: string
  commandLabel: string
  sessionEpoch: U64Decimal
  hostKeys: readonly PanelHostKeyDeclaration[]
  suffix: TextControl
  submitPending: boolean
  closePending: boolean
  focusRequestId?: U64Decimal
}

interface PrivateQuicklinksState {
  status: 'loading' | 'ready' | 'error'
  items: QuicklinkView[]
  draft: QuicklinkDraftSnapshot
  selectedId?: string
  operation?: QuicklinksOperation
  error?: string
}

interface PrivateMainResultCommandState {
  pluginId: string
  commandLabel: string
  suffix: TextControl
}

interface Model {
  view: 'launcher' | 'settings'
  settingsTab: SettingsTabKey
  messageCenter: MessageCenterStateSnapshot
  launcherMode: 'applications' | 'files' | 'panel' | 'quicklinks'
  viewEpoch: number
  theme: ThemePreference
  invocationId?: string
  queryControl: ControlKey
  query: string
  queryControlValue: string
  querySequence: number
  requestId?: string
  results: PrivateResult[]
  selectedIndex: number
  searchPending: boolean
  executePending: boolean
  hidePending: boolean
  favoriteMutationPending: boolean
  shownNotice?: string
  commandHint?: string
  status: string
  settings?: PrivateSettings
  settingsOperation?: SettingsOperationKind
  settingsUncertain: boolean
  settingsLoadStatus: SettingsLoadStatus
  settingsLoadError?: string
  file?: PrivateFileState
  mainResultCommand?: PrivateMainResultCommandState
  panel?: PrivatePanelState
  quicklinks?: PrivateQuicklinksState
  plugins: PrivatePluginList
}

interface PrivatePluginItem extends PluginInventoryView {
  operation?: PluginMutationKind
  error?: string
}

interface PrivatePluginList {
  status: PluginListStatus
  items: PrivatePluginItem[]
  error?: string
}

interface CompositionOwner {
  control: ControlKey
  viewEpoch: number
  invocationId?: string
  generation: number
  lastTrustedDraft: string
}

interface TextControl {
  key: ControlKey
  value: string
  draft: string
}

interface PrivateSettings {
  hotkey: TextControl
  autostart: boolean
  webSearchEngine: WebSearchEngine
}

interface FileSearchOwner {
  token: number
  epoch: number
  invocationId: string
  sequence: number
  query: string
  category: FileCategory
  sort: FileSort
}

interface PreviewPreferenceOwner {
  token: number
  enabled: boolean
}

type SettingsOperationKind = 'load' | 'save' | 'hotkey' | 'theme' | 'webSearchEngine'

interface SettingsOperation {
  token: number
  kind: SettingsOperationKind
  owner:
    | { scope: 'startup'; previewGeneration: number; themeGeneration: number }
    | {
        scope: 'view'
        viewEpoch: number
        view: 'launcher' | 'settings'
        previewGeneration?: number
        themeGeneration?: number
      }
}

interface PluginListOwner {
  token: number
  viewEpoch: number
}

interface PluginMutationOwner {
  token: number
  viewEpoch: number
  pluginId: string
  kind: PluginMutationKind
}

const ERROR_TEXT: Record<CommandErrorCode, string> = {
  invalidCaller: '操作不可用，请重试。',
  staleRequest: '搜索结果已过期，请重新搜索。',
  unknownResult: '搜索结果已过期，请重新搜索。',
  applicationEntryUnavailable: '应用入口不可用，请重新扫描。',
  settingsFailed: '设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。',
  windowFailed: '窗口操作失败。',
  invalidFileQuery: '查询无效。',
  fileSearchWorkerFailed: '搜索暂不可用。',
  searchUnavailable: '搜索暂不可用。',
  fileNotFound: '文件已不存在。',
  fileOpenFailed: '无法在资源管理器中打开。',
  webSearchFailed: '操作不可用，请重试。',
  clipboardWriteFailed: '无法复制到剪贴板。',
  pluginPermissionDenied: '插件无权写入剪贴板。',
  pluginListFailed: '无法加载插件清单。',
  pluginInstallFailed: '无法安装插件。',
  pluginReloadFailed: '无法重新加载插件。',
  pluginDeleteFailed: '无法删除插件。',
  dataCleanupPending: '插件已卸载，数据清理将在下次启动时重试',
}

interface CompletionOriginOwner {
  token: number
  phase: 'armed' | 'committing' | 'consumed'
  epoch: number
  invocationId: string
  control: ControlKey
  querySequence: number
  value: string
  resultKey: number
  pluginId: string
  command: string
}

interface ApplicationSearchOwner {
  token: number
  epoch: number
  invocationId: string
  sequence: number
  query: string
  submit: boolean
  completionOrigin?: {
    token: number
    phase: 'preview' | 'commit'
    pluginId: string
  }
}

interface FavoriteInteractionOwner {
  token: number
  epoch: number
  invocationId: string
  control: ControlKey
  querySequence: number
  value: string
  resultKey: number
  target: ResultFavoriteTarget
}

interface FavoriteMutationOwner extends FavoriteInteractionOwner {
  favorite: boolean
}

const NOTICE_TEXT = {
  settingsFailed: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
} as const

const REFUSED_NOTICE = 'Windows 拒绝了前台切换，已发送启动请求'
const FALLBACK_ERROR = '操作不可用，请重试。'
const PANEL_BOUNDS_ACL_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_ACL）。'
const PANEL_BOUNDS_COMMAND_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_COMMAND_NOT_FOUND）。'
const PANEL_BOUNDS_ARGUMENT_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_INVALID_ARGS）。'
const PANEL_BOUNDS_CALLER_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_INVALID_CALLER）。'
const PANEL_BOUNDS_SESSION_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_INVALID_SESSION）。'
const PANEL_BOUNDS_STALE_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_STALE）。'
const PANEL_BOUNDS_WINDOW_NOTICE = 'Panel 布局同步失败（PANEL_BOUNDS_WINDOW_FAILED）。'
const PANEL_SUBMIT_IDENTITY_NOTICE = 'Panel 返回身份不匹配（PANEL_SUBMIT_IDENTITY）。'
const PANEL_SUBMIT_INVOKE_NOTICE = 'Panel 内容提交失败（PANEL_SUBMIT_INVOKE）。'
const FILE_PREVIEW_ERROR = '无法保存文件预览设置。'
const THEME_PREFERENCE_ERROR = '无法保存风格设置。'
const WEB_SEARCH_ENGINE_ERROR = '无法保存搜索引擎设置。'

function panelBoundsInvokeNotice(error: unknown): string {
  const record = typeof error === 'object' && error !== null
    ? error as Record<string, unknown>
    : undefined
  if (record) {
    const code = record.code
    if (code === 'invalidCaller') return PANEL_BOUNDS_CALLER_NOTICE
    if (code === 'pluginQueryFailed') return PANEL_BOUNDS_SESSION_NOTICE
    if (code === 'staleRequest') return PANEL_BOUNDS_STALE_NOTICE
    if (code === 'windowFailed') return PANEL_BOUNDS_WINDOW_NOTICE
  }
  const message = typeof error === 'string'
    ? error
    : error instanceof Error
      ? error.message
      : typeof record?.message === 'string'
        ? record.message
        : ''
  const normalized = message.toLowerCase()
  if (normalized.includes('not allowed by acl')) return PANEL_BOUNDS_ACL_NOTICE
  if (normalized.includes('command set_plugin_panel_bounds not found')) return PANEL_BOUNDS_COMMAND_NOTICE
  if (
    normalized.includes('invalid args') || normalized.includes('missing required key') ||
    normalized.includes('unknown field') || normalized.includes('invalid type')
  ) return PANEL_BOUNDS_ARGUMENT_NOTICE
  if (message) {
    const detail = message.replace(/[^\x20-\x7e]/gu, ' ').slice(0, 160)
    return `Panel 布局同步失败：${detail}`
  }
  if (record) {
    const code = typeof record.code === 'string' && /^[A-Za-z0-9_-]{1,40}$/u.test(record.code)
      ? `CODE_${record.code}`
      : `OBJECT_${Object.keys(record).sort().slice(0, 5).join('_') || 'EMPTY'}`
    return `Panel 布局同步失败（PANEL_BOUNDS_${code}）。`
  }
  if (error === undefined) return 'Panel 布局同步失败（PANEL_BOUNDS_UNDEFINED）。'
  if (error === null) return 'Panel 布局同步失败（PANEL_BOUNDS_NULL）。'
  if (typeof error !== 'string') return `Panel 布局同步失败（PANEL_BOUNDS_TYPE_${typeof error}）。`
  return 'Panel 布局同步失败（PANEL_BOUNDS_EMPTY_STRING）。'
}
const ERROR_CODES = new Set(Object.keys(ERROR_TEXT))
const ICON_PREFIX = 'data:image/png;base64,'
const MAX_ICON_LENGTH = 65_536
export const FILE_CATEGORY_ORDER: readonly FileCategory[] = [
  'all',
  'folder',
  'excel',
  'word',
  'ppt',
  'pdf',
  'image',
  'video',
  'audio',
  'archive',
]
const BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/
const LAUNCHER_COMMAND = /^[a-z][a-z0-9-]{0,31}$/
const UNICODE_CONTROL = /[\p{Cc}\u2028\u2029\uD800-\uDFFF]/u
const OUTER_UNICODE_WHITESPACE = /^(?:\p{White_Space})|(?:\p{White_Space})$/u
const UTF8_ENCODER = new TextEncoder()
const PUBLIC_PLUGIN_ID = /^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/
const QUERY_SEQUENCE_EXHAUSTED = '查询次数已达上限，请重新打开主界面。'

function safeApplicationIcon(value: unknown): string | undefined {
  if (typeof value !== 'string' || value.length > MAX_ICON_LENGTH || !value.startsWith(ICON_PREFIX)) return undefined
  const payload = value.slice(ICON_PREFIX.length)
  return payload.length > 0 && BASE64.test(payload) ? value : undefined
}

function safeResultIconKind(value: unknown): ResultIconKind | undefined {
  return value === 'find' || value === 'calculator' || value === 'webSearch' ? value : undefined
}

function safeResultFavorite(value: unknown): ResultFavorite | undefined {
  const favorite = exactPlainRecord(value, ['favorite', 'target'])
  if (!favorite || typeof favorite.favorite !== 'boolean') return undefined
  const target = typeof favorite.target === 'object' && favorite.target !== null
    ? favorite.target as Record<string, unknown>
    : undefined
  if (target?.kind === 'publicPlugin') {
    const record = exactPlainRecord(target, ['kind', 'pluginId'])
    return record && typeof record.pluginId === 'string' && record.pluginId.length <= 64 &&
        PUBLIC_PLUGIN_ID.test(record.pluginId)
      ? { target: { kind: 'publicPlugin', pluginId: record.pluginId }, favorite: favorite.favorite }
      : undefined
  }
  if (target?.kind === 'builtin') {
    const record = exactPlainRecord(target, ['feature', 'kind'])
    return record && (record.feature === 'find' || record.feature === 'webSearch')
      ? { target: { kind: 'builtin', feature: record.feature }, favorite: favorite.favorite }
      : undefined
  }
  return undefined
}

function validLauncherCompletion(value: string): boolean {
  if (UTF8_ENCODER.encode(value).byteLength > 65_536 || !value.startsWith('/')) return false
  const separator = value.indexOf(' ')
  if (separator < 2 || !LAUNCHER_COMMAND.test(value.slice(1, separator))) return false
  const argument = value.slice(separator + 1)
  return argument.length === 0 || (!OUTER_UNICODE_WHITESPACE.test(argument) && !UNICODE_CONTROL.test(argument))
}

function exactPlainRecord(value: unknown, expectedKeys: readonly string[]): Record<string, unknown> | undefined {
  if (typeof value !== 'object' || value === null || Object.getPrototypeOf(value) !== Object.prototype) return undefined
  const record = value as Record<string, unknown>
  const keys = Object.keys(record).sort()
  const expected = [...expectedKeys].sort()
  return keys.length === expected.length && keys.every((key, index) => key === expected[index]) ? record : undefined
}

export function safeLauncherActivation(value: unknown): LauncherResultActivation | undefined {
  const candidate = typeof value === 'object' && value !== null
    ? value as Record<string, unknown>
    : undefined
  if (candidate?.kind === 'executeResult') {
    return exactPlainRecord(value, ['kind']) ? { kind: 'executeResult' } : undefined
  }
  if (candidate?.kind === 'openFind') {
    const record = exactPlainRecord(value, ['kind', 'query'])
    return record && typeof record.query === 'string' ? { kind: 'openFind', query: record.query } : undefined
  }
  if (candidate?.kind === 'openQuicklinks') {
    return exactPlainRecord(value, ['kind']) ? { kind: 'openQuicklinks' } : undefined
  }
  if (candidate?.kind === 'completion') {
    const record = exactPlainRecord(value, ['completionText', 'kind'])
    return record && typeof record.completionText === 'string' && validLauncherCompletion(record.completionText)
      ? { kind: 'completion', completionText: record.completionText }
      : undefined
  }
  if (candidate?.kind === 'pluginCompletion') {
    const record = exactPlainRecord(value, ['completionText', 'favorite', 'kind', 'pluginId'])
    return record && typeof record.completionText === 'string' && validLauncherCompletion(record.completionText) &&
        typeof record.pluginId === 'string' && record.pluginId.length <= 64 && PUBLIC_PLUGIN_ID.test(record.pluginId) &&
        typeof record.favorite === 'boolean'
      ? {
          kind: 'pluginCompletion',
          completionText: record.completionText,
          pluginId: record.pluginId,
          favorite: record.favorite,
        }
      : undefined
  }
  if (candidate?.kind === 'windowActivation') {
    const record = exactPlainRecord(
      value,
      ['commandLabel', 'favorite', 'initialArgument', 'kind', 'pluginId'],
    )
    return record &&
        typeof record.pluginId === 'string' && record.pluginId.length <= 64 && PUBLIC_PLUGIN_ID.test(record.pluginId) &&
        typeof record.commandLabel === 'string' && LAUNCHER_COMMAND.test(record.commandLabel) &&
        typeof record.initialArgument === 'string' && record.initialArgument.length <= 65_536 &&
        record.initialArgument.trim() === record.initialArgument &&
        ![...record.initialArgument].some(
          (character) =>
            /\p{Cc}/u.test(character) || character === '\u2028' || character === '\u2029',
        ) &&
        typeof record.favorite === 'boolean'
      ? {
          kind: 'windowActivation',
          pluginId: record.pluginId,
          commandLabel: record.commandLabel,
          initialArgument: record.initialArgument,
          favorite: record.favorite,
        }
      : undefined
  }
  if (candidate?.kind === 'mainResultActivation') {
    const record = exactPlainRecord(
      value,
      ['commandLabel', 'favorite', 'initialArgument', 'kind', 'pluginId'],
    )
    return record &&
        typeof record.pluginId === 'string' && record.pluginId.length <= 64 && PUBLIC_PLUGIN_ID.test(record.pluginId) &&
        typeof record.commandLabel === 'string' && LAUNCHER_COMMAND.test(record.commandLabel) &&
        typeof record.initialArgument === 'string' && record.initialArgument.length <= 65_536 &&
        record.initialArgument.trim() === record.initialArgument &&
        ![...record.initialArgument].some(
          (character) =>
            /\p{Cc}/u.test(character) || character === '\u{2028}' || character === '\u{2029}',
        ) &&
        typeof record.favorite === 'boolean'
      ? {
          kind: 'mainResultActivation',
          pluginId: record.pluginId,
          commandLabel: record.commandLabel,
          initialArgument: record.initialArgument,
          favorite: record.favorite,
        }
      : undefined
  }
  if (candidate?.kind === 'panelActivation') {
    const record = exactPlainRecord(value, ['favorite', 'initialArgument', 'kind', 'pluginId'])
    return record &&
        typeof record.pluginId === 'string' &&
        record.pluginId.length <= 64 &&
        PUBLIC_PLUGIN_ID.test(record.pluginId) &&
        typeof record.initialArgument === 'string' &&
        record.initialArgument.length <= 65_536 &&
        record.initialArgument.trim() === record.initialArgument &&
        ![...record.initialArgument].some(
          (character) =>
            /\p{Cc}/u.test(character) || character === '\u{2028}' || character === '\u{2029}',
        ) &&
        typeof record.favorite === 'boolean'
      ? {
          kind: 'panelActivation',
          pluginId: record.pluginId,
          initialArgument: record.initialArgument,
          favorite: record.favorite,
        }
      : undefined
  }
  return undefined
}

function pluginFavoriteActivation(activation: LauncherResultActivation) {
  return activation.kind === 'pluginCompletion' || activation.kind === 'windowActivation' ||
      activation.kind === 'mainResultActivation' || activation.kind === 'panelActivation'
    ? activation
    : undefined
}

function sameFavoriteTarget(left: ResultFavoriteTarget, right: ResultFavoriteTarget): boolean {
  if (left.kind !== right.kind) return false
  return left.kind === 'publicPlugin'
    ? left.pluginId === (right as Extract<ResultFavoriteTarget, { kind: 'publicPlugin' }>).pluginId
    : left.feature === (right as Extract<ResultFavoriteTarget, { kind: 'builtin' }>).feature
}

function favoriteMatchesResult(
  favorite: ResultFavorite,
  activation: LauncherResultActivation,
  iconKind: ResultIconKind | undefined,
): boolean {
  if (favorite.target.kind === 'publicPlugin') {
    const plugin = pluginFavoriteActivation(activation)
    return plugin?.pluginId === favorite.target.pluginId && plugin.favorite === favorite.favorite
  }
  if (favorite.target.feature === 'find') {
    return activation.kind === 'openFind' && iconKind === 'find'
  }
  return iconKind === 'webSearch' &&
    (activation.kind === 'completion' || activation.kind === 'executeResult')
}

function safeCommandHint(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function safeMainResultCommand(value: unknown): MainResultCommandContext | undefined {
  const command = exactPlainRecord(value, ['argument', 'commandLabel', 'pluginId'])
  if (
    !command ||
    typeof command.pluginId !== 'string' ||
    command.pluginId.length > 64 ||
    !PUBLIC_PLUGIN_ID.test(command.pluginId) ||
    typeof command.commandLabel !== 'string' ||
    !/^[a-z][a-z0-9-]{0,31}$/u.test(command.commandLabel) ||
    typeof command.argument !== 'string' ||
    command.argument.length > 65_536 ||
    command.argument.trim() !== command.argument ||
    [...command.argument].some(
      (character) => /\p{Cc}/u.test(character) || character === '\u{2028}' || character === '\u{2029}',
    )
  ) return undefined
  return {
    pluginId: command.pluginId,
    commandLabel: command.commandLabel,
    argument: command.argument,
  }
}

function errorText(value: unknown): string {
  if (typeof value !== 'object' || value === null || !Object.prototype.hasOwnProperty.call(value, 'code')) return FALLBACK_ERROR
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' && ERROR_CODES.has(code) ? ERROR_TEXT[code as CommandErrorCode] : FALLBACK_ERROR
}

function projectSnapshot(model: Model): LauncherSnapshot {
  const results = Object.freeze(
    model.results.map(({ key, title, subtitle, icon, pluginIconUrl, iconKind, detail, favorite, hasDefaultAction, activation }) =>
      Object.freeze({
        key,
        title,
        ...(subtitle === undefined ? {} : { subtitle }),
        ...(icon === undefined ? {} : { icon }),
        ...(pluginIconUrl === undefined ? {} : { pluginIconUrl }),
        ...(iconKind === undefined ? {} : { iconKind }),
        ...(detail === undefined ? {} : { detail }),
        ...(hasDefaultAction === undefined ? {} : { hasDefaultAction }),
        ...(favorite === undefined
          ? {}
          : {
              favorite: Object.freeze({
                target: Object.freeze({ ...favorite.target }),
                favorite: favorite.favorite,
              }),
            }),
        ...(activation.kind === 'panelActivation'
          ? {
              panelActivation: Object.freeze({
                pluginId: activation.pluginId,
                initialArgument: activation.initialArgument,
              }),
            }
          : {}),
      }),
    ),
  )
  const settings = model.settings
    ? Object.freeze({
        hotkey: Object.freeze({ key: model.settings.hotkey.key, value: model.settings.hotkey.draft }),
        autostart: model.settings.autostart,
        theme: model.theme,
        webSearchEngine: model.settings.webSearchEngine,
        loadStatus: model.settingsLoadStatus,
        readOnly:
          model.settingsUncertain || model.settingsLoadStatus !== 'ready' || model.settingsOperation !== undefined,
        ...(model.settingsOperation === undefined ? {} : { operation: model.settingsOperation }),
        needsReload: model.settingsUncertain,
      })
    : undefined
  const fileResults = model.file
    ? Object.freeze(
        model.file.results.map(({ view }) =>
          Object.freeze({
            key: view.key,
            name: view.name,
            kind: view.kind,
            sizeBytes: view.sizeBytes,
            modifiedUtc: view.modifiedUtc,
            fullPath: view.fullPath,
          }),
        ),
      )
    : undefined
  const file = model.file
    ? Object.freeze({
        category: model.file.category,
        sort: model.file.sort,
        previewEnabled: model.file.previewEnabled,
        preferencePending: model.file.preferencePending,
        total: model.file.total,
        indexStatus: model.file.indexStatus,
        results: fileResults!,
        ...(model.file.selectedIndex < 0 ? {} : { selected: fileResults![model.file.selectedIndex] }),
      })
    : undefined
  const panel = model.panel
    ? Object.freeze({
        pluginId: model.panel.pluginId,
        commandLabel: model.panel.commandLabel,
        sessionEpoch: model.panel.sessionEpoch,
        hostKeys: Object.freeze([...model.panel.hostKeys]),
        suffixControl: model.panel.suffix.key,
        suffix: model.panel.suffix.draft,
        submitPending: model.panel.submitPending,
        closePending: model.panel.closePending,
        ...(model.panel.focusRequestId === undefined ? {} : { focusRequestId: model.panel.focusRequestId }),
      })
    : undefined
  const mainResultCommand = model.mainResultCommand
    ? Object.freeze({
        pluginId: model.mainResultCommand.pluginId,
        commandLabel: model.mainResultCommand.commandLabel,
        suffixControl: model.mainResultCommand.suffix.key,
        suffix: model.mainResultCommand.suffix.draft,
      })
    : undefined
  const plugins = Object.freeze({
    status: model.plugins.status,
    items: Object.freeze(
      model.plugins.items.map((plugin) =>
        Object.freeze({ ...plugin }),
      ),
    ),
    ...(model.plugins.error === undefined ? {} : { error: model.plugins.error }),
  })
  const quicklinks: QuicklinksSnapshot | undefined = model.quicklinks
    ? Object.freeze({
        status: model.quicklinks.status,
        items: Object.freeze(model.quicklinks.items.map((item) => Object.freeze({ ...item }))),
        draft: Object.freeze({ ...model.quicklinks.draft }),
        ...(model.quicklinks.selectedId === undefined ? {} : { selectedId: model.quicklinks.selectedId }),
        ...(model.quicklinks.operation === undefined ? {} : { operation: model.quicklinks.operation }),
        ...(model.quicklinks.error === undefined ? {} : { error: model.quicklinks.error }),
      })
    : undefined
  return Object.freeze({
    view: model.view,
    settingsTab: model.settingsTab,
    messageCenter: model.messageCenter,
    viewEpoch: model.viewEpoch,
    theme: model.theme,
    ...(model.invocationId === undefined ? {} : { invocationId: model.invocationId }),
    queryControl: model.queryControl,
    query: model.query,
    queryControlValue: model.queryControlValue,
    querySequence: model.querySequence,
    results,
    selectedIndex: model.selectedIndex,
    searchPending: model.searchPending,
    executePending: model.executePending,
    hidePending: model.hidePending,
    favoriteMutationPending: model.favoriteMutationPending,
    ...(model.shownNotice === undefined ? {} : { shownNotice: model.shownNotice }),
    ...(model.commandHint === undefined ? {} : { commandHint: model.commandHint }),
    status:
      model.view === 'settings' && model.settingsUncertain
        ? NOTICE_TEXT.settingsFailed
        : model.view === 'settings' && model.settingsLoadError
          ? model.settingsLoadError
          : model.status,
    ...(settings === undefined ? {} : { settings }),
    ...(model.view === 'settings' ? { settingsLoadStatus: model.settingsLoadStatus, plugins } : {}),
    ...(file === undefined ? {} : { file }),
    ...(mainResultCommand === undefined ? {} : { mainResultCommand }),
    ...(panel === undefined ? {} : { panel }),
    ...(quicklinks === undefined ? {} : { quicklinks }),
  })
}

export function createLauncherCore(client: LauncherClient, maximumQuerySequence = Number.MAX_SAFE_INTEGER): LauncherCore {
  const messageCenter = createMessageCenterCore(client)
  const model: Model = {
    view: 'launcher',
    settingsTab: 'general',
    messageCenter: messageCenter.getSnapshot(),
    launcherMode: 'applications',
    viewEpoch: 0,
    theme: 'system',
    queryControl: 1,
    query: '',
    queryControlValue: '',
    querySequence: 0,
    results: [],
    selectedIndex: -1,
    searchPending: false,
    executePending: false,
    hidePending: false,
    favoriteMutationPending: false,
    status: '',
    settingsUncertain: false,
    settingsLoadStatus: 'loading',
    plugins: { status: 'idle', items: [] },
  }
  const listeners = new Set<() => void>()
  let snapshot = projectSnapshot(model)
  let destroyed = false
  let started = false
  let unlisten: (() => void) | undefined
  let unlistenHidden: (() => void) | undefined
  let unlistenPanelError: (() => void) | undefined
  let unlistenPanelReset: (() => void) | undefined
  let unlistenPanelFocusHostInput: (() => void) | undefined
  let panelFocusListenerRegistration: Promise<boolean> | undefined
  let pendingPanelFocusRequest: PluginPanelFocusHostInputEvent | undefined
  let unsubscribeMessages: (() => void) | undefined
  let previewPreferenceToken = 0
  let previewPreferencePending: PreviewPreferenceOwner | undefined
  let previewPreferenceDurableGeneration = 0
  let lastLoadedFilePreviewEnabled = true
  let themeDurableGeneration = 0
  let durableTheme: ThemePreference = 'system'
  let durableWebSearchEngine: WebSearchEngine = 'bing'
  let token = 0
  let searchToken = 0
  let quicklinksToken = 0
  let quicklinksOperationToken = 0
  let slashSearchTimer: ReturnType<typeof setTimeout> | undefined
  let executeToken = 0
  let hideToken = 0
  let resultKey = 1
  let controlKey = 2
  let activationNoticePending = false
  let compositionGeneration = 0
  let composition: CompositionOwner | undefined
  let settingsOperation: SettingsOperation | undefined
  let hotkeyRecording = false
  let hotkeyShowConsumedDuringRecording = false
  let hotkeyShowGraceUntil = 0
  let pendingSettingsLoadEpoch: number | undefined
  let pluginListOwner: PluginListOwner | undefined
  const pluginMutationOwners = new Map<string, PluginMutationOwner>()
  const pluginMutationErrors = new Map<string, string>()
  let highestPluginRevision = '0'
  let pluginInventoryActive = false
  const legacyFindClient = client as unknown as Pick<FindClient, 'searchFiles' | 'setPreviewPreference'>
  let findSubmissionToken = 0
  let applicationSearch: {
    invocationId: string
    sequence: number
    query: string
    completion: Promise<SearchResponse | null>
  } | undefined
  let pendingDefaultActivation: {
    epoch: number
    invocationId: string
    sequence: number
    query: string
  } | undefined
  let completionOrigin: CompletionOriginOwner | undefined
  let completionOriginToken = 0
  let sequenceExhausted = false
  let favoriteInteractionToken = 0
  let favoriteInteraction: FavoriteInteractionOwner | undefined
  let favoriteMutation: FavoriteMutationOwner | undefined
  let favoriteMenuConsumed = false
  let panelActionToken = 0
  let panelSubmissionToken = 0
  let panelBoundsToken = 0
  let panelHostKeyEpoch: U64Decimal | undefined
  let nextPanelHostKeyClientSequence = 1n
  let panelHostKeyEnqueueTail: Promise<void> = Promise.resolve()


  function publish(mutated: boolean): void {
    if (!mutated) return
    snapshot = projectSnapshot(model)
    for (const listener of [...listeners]) listener()
  }

  const getSnapshot = () => snapshot
  const subscribe = (listener: () => void) => {
    listeners.add(listener)
    let active = true
    return () => {
      if (!active) return
      active = false
      listeners.delete(listener)
    }
  }

  function newTextControl(value: string): TextControl {
    return { key: controlKey++, value, draft: value }
  }

  function settingsControls(settings: PrivateSettings): TextControl[] {
    return [settings.hotkey]
  }

  function applyLoadedPreferences(
    view: SettingsView,
    previewGeneration: number,
    themeGeneration: number,
  ): void {
    if (previewGeneration === previewPreferenceDurableGeneration) {
      lastLoadedFilePreviewEnabled = view.filePreviewEnabled
    }
    if (themeGeneration === themeDurableGeneration) {
      durableTheme = view.theme
      model.theme = view.theme
    }
    durableWebSearchEngine = view.webSearchEngine
  }

  function replaceSettingsView(view: SettingsView): void {
    if (model.settings) {
      for (const control of settingsControls(model.settings)) retireControl(control.key)
    }
    model.settings = {
      hotkey: newTextControl(view.hotkey),
      autostart: view.autostart,
      webSearchEngine: view.webSearchEngine,
    }
  }

  function findTextControl(control: ControlKey): TextControl | undefined {
    if (!model.settings) return undefined
    if (model.settings.hotkey.key === control) return model.settings.hotkey
    return undefined
  }

  function getControlDraft(control: ControlKey): string | undefined {
    if (control === model.queryControl) return model.queryControlValue
    if (control === model.mainResultCommand?.suffix.key) return model.mainResultCommand.suffix.draft
    if (control === model.panel?.suffix.key) return model.panel.suffix.draft
    return findTextControl(control)?.draft
  }

  function setControlDraft(control: ControlKey, value: string): boolean {
    if (control === model.queryControl) {
      const changed = model.queryControlValue !== value
      model.queryControlValue = value
      return changed
    }
    if (control === model.mainResultCommand?.suffix.key) {
      const changed = model.mainResultCommand.suffix.draft !== value
      model.mainResultCommand.suffix.draft = value
      return changed
    }
    if (control === model.panel?.suffix.key) {
      const changed = model.panel.suffix.draft !== value
      model.panel.suffix.draft = value
      return changed
    }
    const field = findTextControl(control)
    if (!field) return false
    const changed = field.draft !== value
    field.draft = value
    return changed
  }

  function restoreControl(control: ControlKey): boolean {
    if (control === model.queryControl) return setControlDraft(control, model.query)
    if (control === model.mainResultCommand?.suffix.key) {
      return setControlDraft(control, model.mainResultCommand.suffix.value)
    }
    if (control === model.panel?.suffix.key) return setControlDraft(control, model.panel.suffix.value)
    const field = findTextControl(control)
    return field ? setControlDraft(control, field.value) : false
  }

  function commitControl(control: ControlKey, value: string): void {
    if (control === model.queryControl) {
      if (model.query === value) {
        publish(setControlDraft(control, value))
        return
      }
      applyEdit(value)
      return
    }
    if (control === model.mainResultCommand?.suffix.key) {
      applyMainResultEdit(value)
      return
    }
    if (control === model.panel?.suffix.key) {
      const visibleChanged = setControlDraft(control, value)
      if (model.panel.suffix.value === value) {
        publish(visibleChanged)
        return
      }
      model.panel.suffix.value = value
      model.shownNotice = undefined
      model.status = ''
      publish(true)
      return
    }
    const field = findTextControl(control)
    if (!field || model.settingsUncertain || model.settingsLoadStatus !== 'ready' || settingsOperation) return
    const visibleChanged = setControlDraft(control, value)
    if (field.value === value) {
      publish(visibleChanged)
      return
    }
    field.value = value
    model.shownNotice = undefined
    publish(true)
  }

  function settingsEditable(): boolean {
    return (
      model.settings !== undefined &&
      !model.settingsUncertain &&
      model.settingsLoadStatus === 'ready' &&
      settingsOperation === undefined
    )
  }

  function clearResults(): void {
    model.requestId = undefined
    model.results = []
    model.selectedIndex = -1
    model.commandHint = undefined
  }


  function leaveFileMode(): void {
    if (model.launcherMode !== 'files') return
    searchToken = ++token
    model.searchPending = false
    model.launcherMode = 'applications'
    model.file = undefined
    model.query = ''
    model.queryControlValue = ''
  }

  function emptyQuicklinkDraft(): QuicklinkDraftSnapshot {
    return {
      name: '',
      command: '',
      template: '',
    }
  }

  function quicklinkDraft(item: QuicklinkView): QuicklinkDraftSnapshot {
    return {
      id: item.id,
      name: item.name,
      command: item.command,
      template: item.template,
      ...(item.iconDataUrl === undefined ? {} : { iconDataUrl: item.iconDataUrl }),
    }
  }

  function ensureQuicklinksState(): PrivateQuicklinksState {
    if (!model.quicklinks) {
      model.quicklinks = {
        status: 'loading',
        items: [],
        draft: emptyQuicklinkDraft(),
      }
    }
    return model.quicklinks
  }

  function applyQuicklinksList(items: QuicklinkView[], loadError?: string): void {
    const quicklinks = ensureQuicklinksState()
    quicklinks.status = 'ready'
    quicklinks.items = [...items]
    const selected =
      quicklinks.items.find((item) => item.id === quicklinks.selectedId) ??
      quicklinks.items[0]
    if (selected) {
      quicklinks.selectedId = selected.id
      quicklinks.draft = quicklinkDraft(selected)
    } else {
      quicklinks.selectedId = undefined
      quicklinks.draft = emptyQuicklinkDraft()
    }
    quicklinks.operation = undefined
    quicklinks.error = loadError
  }

  function ownsQuicklinksOperation(owner: { viewEpoch: number; token: number; operationToken: number }): boolean {
    return !destroyed &&
      model.view === 'launcher' &&
      model.launcherMode === 'quicklinks' &&
      model.viewEpoch === owner.viewEpoch &&
      quicklinksToken === owner.token &&
      quicklinksOperationToken === owner.operationToken
  }

  async function loadQuicklinks(): Promise<void> {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    quicklinks.status = 'loading'
    quicklinks.operation = 'load'
    quicklinks.error = undefined
    const owner = {
      viewEpoch: model.viewEpoch,
      token: quicklinksToken,
      operationToken: ++quicklinksOperationToken,
    }
    publish(true)
    try {
      const response = await client.listQuicklinks()
      if (!ownsQuicklinksOperation(owner)) return
      applyQuicklinksList(response.items, response.loadError)
      publish(true)
    } catch (error) {
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      quicklinks.status = 'error'
      quicklinks.operation = undefined
      quicklinks.error = errorText(error)
      publish(true)
    }
  }

  function openQuicklinks(): void {
    if (destroyed || model.view !== 'launcher') return
    if (model.launcherMode === 'panel') return
    leaveFileMode()
    quicklinksToken += 1
    quicklinksOperationToken += 1
    cancelSlashSearch()
    searchToken = ++token
    executeToken = ++token
    model.searchPending = false
    model.executePending = false
    model.launcherMode = 'quicklinks'
    model.quicklinks = {
      status: 'loading',
      items: [],
      draft: emptyQuicklinkDraft(),
      operation: 'load',
    }
    model.query = ''
    model.queryControlValue = ''
    model.status = ''
    model.shownNotice = undefined
    clearResults()
    publish(true)
    void loadQuicklinks()
  }

  function discardQuicklinksUi(): boolean {
    if (model.launcherMode !== 'quicklinks' && !model.quicklinks) return false
    quicklinksToken += 1
    quicklinksOperationToken += 1
    model.quicklinks = undefined
    if (model.launcherMode === 'quicklinks') model.launcherMode = 'applications'
    return true
  }

  function closeQuicklinks(): void {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    discardQuicklinksUi()
    model.status = ''
    model.query = ''
    model.queryControlValue = ''
    clearResults()
    if (advanceApplicationSequence()) deferCurrentSearch(false)
    publish(true)
  }

  function newQuicklink(): void {
    if (destroyed || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    quicklinks.selectedId = undefined
    quicklinks.draft = emptyQuicklinkDraft()
    quicklinks.error = undefined
    publish(true)
  }

  function selectQuicklink(id: string): void {
    if (destroyed || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    const item = quicklinks.items.find((candidate) => candidate.id === id)
    if (!item) return
    quicklinks.selectedId = item.id
    quicklinks.draft = quicklinkDraft(item)
    quicklinks.error = undefined
    publish(true)
  }

  function completeQuicklink(id: string): void {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    const item = model.quicklinks?.items.find((candidate) => candidate.id === id)
    if (!item) return
    discardQuicklinksUi()
    model.status = ''
    applyEdit(`/${item.command} `)
  }

  function setQuicklinkDraft(field: 'name' | 'command' | 'template', value: string): void {
    if (destroyed || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    quicklinks.draft = { ...quicklinks.draft, [field]: value }
    quicklinks.error = undefined
    publish(true)
  }

  async function chooseQuicklinkIcon(): Promise<void> {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    quicklinks.operation = 'icon'
    quicklinks.error = undefined
    const owner = {
      viewEpoch: model.viewEpoch,
      token: quicklinksToken,
      operationToken: ++quicklinksOperationToken,
    }
    publish(true)
    try {
      const candidate = await client.chooseQuicklinkIcon()
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      quicklinks.operation = undefined
      if (candidate) {
        quicklinks.draft = {
          ...quicklinks.draft,
          iconDataUrl: candidate.dataUrl,
          iconToken: candidate.token,
        }
      }
      publish(true)
    } catch (error) {
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      quicklinks.operation = undefined
      quicklinks.error = errorText(error)
      publish(true)
    }
  }

  async function saveQuicklink(): Promise<void> {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    const draft = quicklinks.draft
    quicklinks.operation = 'save'
    quicklinks.error = undefined
    const owner = {
      viewEpoch: model.viewEpoch,
      token: quicklinksToken,
      operationToken: ++quicklinksOperationToken,
    }
    publish(true)
    try {
      const saved = await client.saveQuicklink({
        input: {
          ...(draft.id === undefined ? {} : { id: draft.id }),
          name: draft.name,
          command: draft.command,
          template: draft.template,
          iconToken: draft.iconToken ?? null,
        },
      })
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      const nextItems = quicklinks.items.filter((item) => item.id !== saved.id)
      nextItems.push(saved)
      nextItems.sort((left, right) => left.command.localeCompare(right.command))
      quicklinks.items = nextItems
      quicklinks.selectedId = saved.id
      quicklinks.draft = quicklinkDraft(saved)
      quicklinks.operation = undefined
      quicklinks.status = 'ready'
      publish(true)
    } catch (error) {
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      quicklinks.operation = undefined
      quicklinks.error = errorText(error)
      publish(true)
    }
  }

  async function deleteQuicklink(): Promise<void> {
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'quicklinks') return
    const quicklinks = ensureQuicklinksState()
    const id = quicklinks.draft.id
    if (!id) return
    quicklinks.operation = 'delete'
    quicklinks.error = undefined
    const owner = {
      viewEpoch: model.viewEpoch,
      token: quicklinksToken,
      operationToken: ++quicklinksOperationToken,
    }
    publish(true)
    try {
      await client.deleteQuicklink({ id })
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      const deletedIndex = quicklinks.items.findIndex((item) => item.id === id)
      quicklinks.items = quicklinks.items.filter((item) => item.id !== id)
      const nextSelected =
        quicklinks.items[Math.max(0, deletedIndex)] ??
        quicklinks.items[quicklinks.items.length - 1]
      quicklinks.selectedId = nextSelected?.id
      quicklinks.draft = nextSelected ? quicklinkDraft(nextSelected) : emptyQuicklinkDraft()
      quicklinks.operation = undefined
      quicklinks.status = 'ready'
      publish(true)
    } catch (error) {
      if (!ownsQuicklinksOperation(owner)) return
      const quicklinks = ensureQuicklinksState()
      quicklinks.operation = undefined
      quicklinks.error = errorText(error)
      publish(true)
    }
  }

  function fileCommand(value: string): string | null {
    if (value === '/find') return ''
    return value.startsWith('/find ') ? value.slice(6) : null
  }

  function submitFind(query: string): void {
    const invocationId = model.invocationId
    if (!invocationId || model.view !== 'launcher' || model.executePending) return
    const owner = {
      token: ++findSubmissionToken,
      epoch: model.viewEpoch,
      control: model.queryControl,
      value: model.queryControlValue,
      invocationId,
      querySequence: model.querySequence,
    }
    model.status = ''
    model.shownNotice = undefined
    publish(true)
    const ownsSubmission = () =>
      !destroyed && owner.token === findSubmissionToken && owner.epoch === model.viewEpoch &&
      owner.control === model.queryControl && owner.value === model.queryControlValue &&
      owner.invocationId === model.invocationId && owner.querySequence === model.querySequence
    const fail = () => {
      if (!ownsSubmission()) return
      model.status = '文件搜索窗口暂不可用。'
      publish(true)
    }
    const matchingSearch = applicationSearch?.invocationId === invocationId &&
      applicationSearch.sequence === owner.querySequence && applicationSearch.query === owner.value
      ? applicationSearch.completion
      : undefined
    let ownership = matchingSearch
    if (!ownership) {
      try {
        ownership = client.searchApps({
          query: owner.value,
          invocationId,
          querySequence: owner.querySequence,
        })
      } catch (error) {
        ownership = Promise.reject(error)
      }
      applicationSearch = {
        invocationId,
        sequence: owner.querySequence,
        query: owner.value,
        completion: ownership,
      }
    }
    void ownership.then(
      () => {
        if (!ownsSubmission()) return
        let pending
        try {
          pending = client.openFind({ query, invocationId, querySequence: owner.querySequence })
        } catch (error) {
          pending = Promise.reject(error)
        }
        void pending.then(
          (outcome) => {
            if (!ownsSubmission() || outcome.status !== 'forwarded') return
            searchToken = ++token
            model.searchPending = false
            model.query = ''
            model.queryControlValue = ''
            model.status = ''
            clearResults()
            publish(true)
          },
          fail,
        )
      },
      fail,
    )
  }
  function fileStatusText(status: FileIndexStatus, hasResults = true): string {
    if (status === 'building') return '正在索引。'
    if (status === 'partial') return '部分位置无法访问。'
    if (status === 'rebuilding') return '索引正在重建。'
    if (status === 'unavailable') return '搜索暂不可用。'
    if (!hasResults) return '未找到文件'
    return ''
  }

  function nextFileSequence(): boolean {
    if (model.querySequence === maximumQuerySequence) {
      searchToken = ++token
      model.searchPending = false
      void requestHide()
      return false
    }
    model.querySequence += 1
    return true
  }


  function ownsFileSearch(owner: FileSearchOwner): boolean {
    const file = model.file
    return (
      !destroyed &&
      model.view === 'launcher' &&
      model.launcherMode === 'files' &&
      file !== undefined &&
      owner.token === searchToken &&
      owner.epoch === model.viewEpoch &&
      owner.invocationId === model.invocationId &&
      owner.sequence === model.querySequence &&
      owner.query === model.query &&
      owner.query === model.queryControlValue &&
      owner.category === file.category &&
      owner.sort === file.sort
    )
  }

  function beginFileSearch(): void {
    const invocationId = model.invocationId
    const file = model.file
    if (!invocationId || !file) return
    const owner: FileSearchOwner = {
      token: ++token,
      epoch: model.viewEpoch,
      invocationId,
      sequence: model.querySequence,
      query: model.query,
      category: file.category,
      sort: file.sort,
    }
    searchToken = owner.token
    model.searchPending = true
    publish(true)
    let pending: Promise<FileSearchResponse | null>
    try {
      pending = legacyFindClient.searchFiles({
        query: owner.query,
        category: owner.category,
        sort: owner.sort,
        invocationId,
        querySequence: owner.sequence,
      })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (response) => finishFileSearch(owner, response),
      (error: unknown) => failFileSearch(owner, error),
    )
  }

  function finishFileSearch(owner: FileSearchOwner, value: FileSearchResponse | null): void {
    if (!ownsFileSearch(owner)) return
    const file = model.file!
    const response = value === null ? null : parseFileSearchResponse(value)
    if (response === null) {
      model.searchPending = false
      publish(true)
      return
    }
    const revision = BigInt(response.indexRevision)
    const selectedPath = file.results[file.selectedIndex]?.view.fullPath
    const results = response.items.map((item: FileResultItem) => ({
      resultId: item.resultId,
      view: {
        key: item.fullPath,
        name: item.name,
        kind: item.kind,
        sizeBytes: item.sizeBytes,
        modifiedUtc: item.modifiedUtc,
        fullPath: item.fullPath,
      },
    }))
    const selectedIndex = selectedPath === undefined ? -1 : results.findIndex(({ view }) => view.fullPath === selectedPath)
    file.latestSeenRevision = revision
    file.total = response.total
    file.indexStatus = response.status
    file.results = results
    file.selectedIndex = selectedIndex >= 0 ? selectedIndex : results.length ? 0 : -1
    model.requestId = response.requestId
    model.searchPending = false
    model.status = fileStatusText(response.status, results.length > 0)
    publish(true)
  }

  function failFileSearch(owner: FileSearchOwner, error: unknown): void {
    if (!ownsFileSearch(owner)) return
    model.file!.indexStatus = 'unavailable'
    model.searchPending = false
    model.status = errorText(error)
    publish(true)
  }

  async function enterFileMode(query: string): Promise<void> {
    if (!model.invocationId) return
    searchToken = ++token
    clearResults()
    model.launcherMode = 'files'
    model.query = query
    model.queryControlValue = query
    model.status = ''
    model.file = {
      category: 'all',
      sort: 'modifiedDesc',
      previewEnabled: previewPreferencePending?.enabled ?? lastLoadedFilePreviewEnabled,
      durablePreviewEnabled: lastLoadedFilePreviewEnabled,
      preferencePending: previewPreferencePending !== undefined,
      total: '0',
      indexStatus: 'building',
      latestSeenRevision: 0n,
      results: [],
      selectedIndex: -1,
    }
    publish(true)
    if (query.length === 0 || !nextFileSequence()) return
    beginFileSearch()
  }

  function applyFileEdit(value: string): void {
    const file = model.file
    if (!file) return
    model.shownNotice = undefined
    model.query = value
    model.queryControlValue = value
    model.requestId = undefined
    file.results = []
    file.selectedIndex = -1
    file.total = '0'
    model.status = ''
    searchToken = ++token
    model.searchPending = false
    if (value.length === 0 || !nextFileSequence()) {
      publish(true)
      return
    }
    beginFileSearch()
  }

  function cancelSlashSearch(): void {
    if (slashSearchTimer === undefined) return
    clearTimeout(slashSearchTimer)
    slashSearchTimer = undefined
  }

  function completionCommand(value: string): string | undefined {
    if (!validLauncherCompletion(value)) return undefined
    return value.slice(1, value.indexOf(' '))
  }

  function ownsCompletionOrigin(owner: CompletionOriginOwner | undefined): owner is CompletionOriginOwner {
    return owner !== undefined &&
      owner.epoch === model.viewEpoch &&
      owner.invocationId === model.invocationId &&
      owner.control === model.queryControl &&
      owner.querySequence === model.querySequence &&
      owner.value === model.query &&
      owner.value === model.queryControlValue
  }

  function replaceCompletionOrigin(
    source: Pick<CompletionOriginOwner, 'resultKey' | 'pluginId' | 'command'>,
    phase: CompletionOriginOwner['phase'],
  ): void {
    const invocationId = model.invocationId
    if (!invocationId) {
      completionOrigin = undefined
      return
    }
    completionOrigin = {
      token: ++completionOriginToken,
      phase,
      epoch: model.viewEpoch,
      invocationId,
      control: model.queryControl,
      querySequence: model.querySequence,
      value: model.query,
      resultKey: source.resultKey,
      pluginId: source.pluginId,
      command: source.command,
    }
  }

  function enterSequenceExhausted(): void {
    sequenceExhausted = true
    cancelSlashSearch()
    searchToken = ++token
    model.searchPending = false
    pendingDefaultActivation = undefined
    if (ownsCompletionOrigin(completionOrigin)) {
      completionOrigin = { ...completionOrigin, phase: 'consumed' }
    } else {
      completionOrigin = undefined
    }
    clearResults()
    model.status = QUERY_SEQUENCE_EXHAUSTED
    publish(true)
  }

  function advanceApplicationSequence(): boolean {
    if (sequenceExhausted) return false
    if (model.querySequence >= maximumQuerySequence) {
      enterSequenceExhausted()
      return false
    }
    model.querySequence += 1
    return true
  }

  function invalidateFavoriteInteraction(): void {
    favoriteInteractionToken += 1
    favoriteInteraction = undefined
    favoriteMenuConsumed = false
  }

  function ownsFavoriteInteraction(owner: FavoriteInteractionOwner): boolean {
    if (
      destroyed ||
      owner.token !== favoriteInteractionToken ||
      favoriteInteraction?.token !== owner.token ||
      model.view !== 'launcher' ||
      model.launcherMode !== 'applications' ||
      owner.epoch !== model.viewEpoch ||
      owner.invocationId !== model.invocationId ||
      owner.control !== model.queryControl ||
      owner.querySequence !== model.querySequence ||
      owner.value !== model.query ||
      owner.value !== model.queryControlValue
    ) return false
    const selected = model.results[model.selectedIndex]
    return selected?.key === owner.resultKey && selected.favorite !== undefined &&
      sameFavoriteTarget(selected.favorite.target, owner.target)
  }

  unsubscribeMessages = messageCenter.subscribe(() => {
    model.messageCenter = messageCenter.getSnapshot()
    publish(true)
  })

  function beginSearch(submit = false, showPending = true): void {
    const invocationId = model.invocationId
    if (!invocationId || (model.query !== '/find' && fileCommand(model.query) !== null) || sequenceExhausted) return
    let ownedOrigin: ApplicationSearchOwner['completionOrigin']
    if (ownsCompletionOrigin(completionOrigin)) {
      if (!submit && completionOrigin.phase === 'armed') {
        ownedOrigin = { token: completionOrigin.token, phase: 'preview', pluginId: completionOrigin.pluginId }
      } else if (submit && completionOrigin.phase === 'committing') {
        ownedOrigin = { token: completionOrigin.token, phase: 'commit', pluginId: completionOrigin.pluginId }
      } else {
        return
      }
    }
    const captured: ApplicationSearchOwner = {
      token: ++token,
      epoch: model.viewEpoch,
      invocationId,
      sequence: model.querySequence,
      query: model.query,
      submit,
      ...(ownedOrigin === undefined ? {} : { completionOrigin: ownedOrigin }),
    }
    searchToken = captured.token
    if (showPending) model.searchPending = true
    let pending: Promise<SearchResponse | null>
    try {
      pending = client.searchApps({
        query: captured.query,
        invocationId,
        querySequence: captured.sequence,
        ...(captured.query.startsWith('/') ? { submit } : {}),
        ...(captured.completionOrigin === undefined
          ? {}
          : {
              completionOrigin: {
                phase: captured.completionOrigin.phase,
                pluginId: captured.completionOrigin.pluginId,
              },
            }),
      })
    } catch (error) {
      pending = Promise.reject(error)
    }
    if (!captured.query.startsWith('/')) {
      applicationSearch = {
        invocationId,
        sequence: captured.sequence,
        query: captured.query,
        completion: pending,
      }
    }
    void pending.then(
      (response) => finishSearch(captured, response),
      (error: unknown) => failSearch(captured, error),
    )
  }
  function scheduleSearch(): void {
    cancelSlashSearch()
    if (!model.query.startsWith('/')) {
      beginSearch()
      return
    }
    const epoch = model.viewEpoch
    const invocationId = model.invocationId
    const sequence = model.querySequence
    const query = model.query
    slashSearchTimer = setTimeout(() => {
      slashSearchTimer = undefined
      if (
        destroyed ||
        epoch !== model.viewEpoch ||
        invocationId !== model.invocationId ||
        sequence !== model.querySequence ||
        query !== model.query ||
        query !== model.queryControlValue
      ) return
      beginSearch(false)
      publish(true)
    }, 150)
  }
  function deferCurrentSearch(showPending = true): void {
    const owner = {
      epoch: model.viewEpoch,
      invocationId: model.invocationId,
      sequence: model.querySequence,
      query: model.query,
    }
    setTimeout(() => {
      if (
        destroyed || model.view !== 'launcher' || owner.epoch !== model.viewEpoch ||
        owner.invocationId !== model.invocationId || owner.sequence !== model.querySequence ||
        owner.query !== model.query || owner.query !== model.queryControlValue
      ) return
      beginSearch(false, showPending)
      publish(true)
    }, 0)
  }
  function ownsSearch(captured: ApplicationSearchOwner): boolean {
    const ownsBase = (
      !destroyed &&
      captured.token === searchToken &&
      captured.epoch === model.viewEpoch &&
      captured.invocationId === model.invocationId &&
      captured.sequence === model.querySequence &&
      captured.query === model.query &&
      captured.query === model.queryControlValue
    )
    if (!ownsBase || captured.completionOrigin === undefined) return ownsBase
    if (!ownsCompletionOrigin(completionOrigin) || completionOrigin.token !== captured.completionOrigin.token) return false
    return captured.completionOrigin.phase === 'preview'
      ? completionOrigin.phase === 'armed'
      : completionOrigin.phase === 'committing' || completionOrigin.phase === 'consumed'
  }

  function settleCompletionCommit(captured: ApplicationSearchOwner): void {
    if (
      captured.completionOrigin?.phase === 'commit' &&
      ownsCompletionOrigin(completionOrigin) &&
      completionOrigin.token === captured.completionOrigin.token &&
      completionOrigin.phase === 'committing'
    ) {
      completionOrigin = { ...completionOrigin, phase: 'consumed' }
    }
  }

  function finishSearch(
    captured: ApplicationSearchOwner,
    response: import('./protocol').SearchResponse | null,
  ): void {
    if (!ownsSearch(captured)) return
    settleCompletionCommit(captured)
    const transferToken = response?.windowTransferToken
    if (transferToken !== undefined) {
      let pending: Promise<void>
      try {
        pending = client.commitPluginWindowTransfer({ transferToken })
      } catch (error) {
        pending = Promise.reject(error)
      }
      void pending.then(
        () => {
          if (!ownsSearch(captured)) return
          model.searchPending = false
          model.query = ''
          model.queryControlValue = ''
          model.requestId = undefined
          model.status = ''
          clearResults()
          publish(true)
        },
        (error: unknown) => {
          if (!ownsSearch(captured)) return
          model.searchPending = false
          model.status = errorText(error)
          publish(true)
        },
      )
      publish(true)
      return
    }
    model.searchPending = false
    if (response !== null) {
      const mainResultCommand = captured.submit
        ? safeMainResultCommand(response.mainResultCommand)
        : undefined
      if (mainResultCommand) activateMainResultCommand(mainResultCommand)
      model.requestId = response.requestId
      model.commandHint = safeCommandHint(response.commandHint)
      const applications: PrivateApplicationResult[] = response.items.flatMap((item: ResultItem) => {
        const activation = safeLauncherActivation(item.activation)
        if (activation === undefined || typeof item.resultId !== 'string' ||
            (activation.kind === 'executeResult' && item.resultId.length === 0)) return []
        const icon = safeApplicationIcon(item.icon)
        const pluginIconUrl = safePublicPluginIconUrl(item.pluginIconUrl)
        const iconKind = safeResultIconKind(item.iconKind)
        const favorite = item.favorite === undefined ? undefined : safeResultFavorite(item.favorite)
        if (
          (item.favorite !== undefined && favorite === undefined) ||
          (favorite !== undefined && !favoriteMatchesResult(favorite, activation, iconKind))
        ) return []
        return [{
          kind: 'application',
          key: resultKey++,
          resultId: item.resultId,
          activation,
          title: item.title,
          ...(item.subtitle === undefined ? {} : { subtitle: item.subtitle }),
          ...(icon === undefined ? {} : { icon }),
          ...(pluginIconUrl === undefined ? {} : { pluginIconUrl }),
          ...(iconKind === undefined ? {} : { iconKind }),
          ...(item.detail === undefined ? {} : { detail: item.detail }),
          ...(favorite === undefined ? {} : { favorite }),
          ...(item.hasDefaultAction === undefined ? {} : { hasDefaultAction: item.hasDefaultAction }),
        }]
      })
      model.results = applications
      model.selectedIndex = model.results.length ? 0 : -1
      model.status = model.results.length || model.commandHint ? '' : '未找到应用'
      const autoExecuteIndex = captured.submit && typeof response.autoExecuteResultId === 'string'
        ? applications.findIndex((item) =>
            item.resultId === response.autoExecuteResultId &&
            item.activation.kind === 'executeResult' &&
            item.hasDefaultAction !== false)
        : -1
      if (autoExecuteIndex >= 0) model.selectedIndex = autoExecuteIndex
      const activateDefault =
        pendingDefaultActivation?.epoch === captured.epoch &&
        pendingDefaultActivation.invocationId === captured.invocationId &&
        pendingDefaultActivation.sequence === captured.sequence &&
        pendingDefaultActivation.query === captured.query
      if (activateDefault) pendingDefaultActivation = undefined
      publish(true)
      if (
        captured.submit &&
        applications.length === 1 &&
        (applications[0]?.activation.kind === 'panelActivation' ||
          applications[0]?.activation.kind === 'openQuicklinks')
      ) {
        executeSelection()
      } else if (
        autoExecuteIndex >= 0
      ) {
        executeSelection()
      } else if (activateDefault && applications.length > 0) {
        executeSelection()
      }
      return
    }
    publish(true)
  }

  function failSearch(
    captured: ApplicationSearchOwner,
    error: unknown,
  ): void {
    if (!ownsSearch(captured)) return
    settleCompletionCommit(captured)
    model.searchPending = false
    model.status = errorText(error)
    publish(true)
  }

  function mainResultQuery(commandLabel: string, argument: string): string {
    return argument ? `/${commandLabel} ${argument}` : `/${commandLabel}`
  }

  function activateMainResultCommand(context: MainResultCommandContext): void {
    const current = model.mainResultCommand
    if (current?.pluginId === context.pluginId && current.commandLabel === context.commandLabel) {
      current.suffix.value = context.argument
      current.suffix.draft = context.argument
    } else {
      model.mainResultCommand = {
        pluginId: context.pluginId,
        commandLabel: context.commandLabel,
        suffix: newTextControl(context.argument),
      }
    }
    const query = mainResultQuery(context.commandLabel, context.argument)
    model.query = query
    model.queryControlValue = query
    completionOrigin = undefined
  }

  function applyMainResultEdit(value: string): void {
    const command = model.mainResultCommand
    if (!command) return
    invalidateFavoriteInteraction()
    const query = mainResultQuery(command.commandLabel, value)
    if (sequenceExhausted) {
      const changed = command.suffix.value !== value || command.suffix.draft !== value || model.query !== query
      command.suffix.value = value
      command.suffix.draft = value
      model.query = query
      model.queryControlValue = query
      publish(changed)
      return
    }
    if (command.suffix.value === value) {
      publish(setControlDraft(command.suffix.key, value))
      return
    }
    if (!advanceApplicationSequence()) return
    command.suffix.value = value
    command.suffix.draft = value
    model.query = query
    model.queryControlValue = query
    completionOrigin = undefined
    model.shownNotice = undefined
    searchToken = ++token
    model.searchPending = false
    model.status = ''
    clearResults()
    scheduleSearch()
    publish(true)
  }

  function closeMainResultCommand(): void {
    const command = model.mainResultCommand
    if (destroyed || !command) return
    if (composition?.control === command.suffix.key) composition = undefined
    model.mainResultCommand = undefined
    completionOrigin = undefined
    applyEdit('')
  }

  function applyEdit(value: string): void {
    invalidateFavoriteInteraction()
    if (model.launcherMode === 'files') {
      applyFileEdit(value)
      return
    }
    if (sequenceExhausted) {
      const changed = model.query !== value || model.queryControlValue !== value
      model.query = value
      model.queryControlValue = value
      publish(changed)
      return
    }
    const previousOrigin = ownsCompletionOrigin(completionOrigin) ? completionOrigin : undefined
    const retainsOrigin = previousOrigin !== undefined &&
      completionCommand(value) === previousOrigin.command
    if (!advanceApplicationSequence()) return
    model.shownNotice = undefined
    model.query = value
    model.queryControlValue = value
    if (retainsOrigin) replaceCompletionOrigin(previousOrigin, 'armed')
    else completionOrigin = undefined
    searchToken = ++token
    model.searchPending = false
    model.status = ''
    clearResults()
    scheduleSearch()
    publish(true)
  }

  function ownsPluginList(owner: PluginListOwner): boolean {
    return (
      !destroyed &&
      pluginListOwner?.token === owner.token &&
      owner.viewEpoch === model.viewEpoch &&
      model.view === 'settings'
    )
  }

  function projectPluginInventory(items: readonly PluginInventoryView[]): PrivatePluginItem[] {
    return items.map((plugin) => {
      const pluginId = plugin.id
      const owner = pluginId === null ? undefined : pluginMutationOwners.get(pluginId)
      const error = pluginId === null ? undefined : pluginMutationErrors.get(pluginId)
      return {
        ...plugin,
        ...(owner === undefined ? {} : { operation: owner.kind }),
        ...(error === undefined ? {} : { error }),
      }
    })
  }

  function beginPluginList(): Promise<void> | undefined {
    if (destroyed || model.view !== 'settings') return undefined
    const owner = { token: ++token, viewEpoch: model.viewEpoch }
    pluginListOwner = owner
    model.plugins = { status: 'loading', items: model.plugins.items }
    publish(true)
    let pending: ReturnType<LauncherClient['listPlugins']>
    try {
      pending = client.listPlugins()
    } catch (error) {
      pending = Promise.reject(error)
    }
    return pending.then(
      (snapshot) => {
        if (!ownsPluginList(owner)) return
        pluginListOwner = undefined
        if (compareDecimalRevision(snapshot.revision, highestPluginRevision) < 0) {
          model.plugins = { status: 'error', items: model.plugins.items, error: ERROR_TEXT.pluginListFailed }
          publish(true)
          return
        }
        highestPluginRevision = snapshot.revision
        const currentIds = new Set(snapshot.items.flatMap((plugin) => plugin.id === null ? [] : [plugin.id]))
        for (const pluginId of pluginMutationErrors.keys()) {
          if (!currentIds.has(pluginId)) pluginMutationErrors.delete(pluginId)
        }
        model.plugins = { status: 'ready', items: projectPluginInventory(snapshot.items) }
        publish(true)
      },
      (error: unknown) => {
        if (!ownsPluginList(owner)) return
        pluginListOwner = undefined
        model.plugins = { status: 'error', items: model.plugins.items, error: errorText(error) }
        publish(true)
      },
    )
  }

  async function activatePlugins(): Promise<void> {
    if (destroyed || model.view !== 'settings') return
    pluginInventoryActive = true
    await beginPluginList()
  }

  function deactivatePlugins(): void {
    pluginInventoryActive = false
  }

  async function reloadPlugins(): Promise<void> {
    pluginInventoryActive = true
    await beginPluginList()
  }

  function ownsPluginMutation(owner: PluginMutationOwner): boolean {
    return pluginMutationOwners.get(owner.pluginId)?.token === owner.token
  }

  function reconcilePluginsAfterMutation(): void {
    if (!destroyed && model.view === 'settings' && pluginInventoryActive) {
      void beginPluginList()
    } else {
      publish(true)
    }
  }

  async function mutatePlugin(
    pluginId: string,
    kind: PluginMutationKind,
    mutation: () => Promise<{ revision: string }>,
  ): Promise<void> {
    const plugin = model.plugins.items.find((item) => item.id === pluginId)
    if (
      destroyed ||
      model.view !== 'settings' ||
      model.plugins.status !== 'ready' ||
      !plugin ||
      plugin.operation ||
      pluginMutationOwners.has(pluginId)
    ) return
    const owner: PluginMutationOwner = {
      token: ++token,
      viewEpoch: model.viewEpoch,
      pluginId,
      kind,
    }
    pluginMutationOwners.set(pluginId, owner)
    pluginMutationErrors.delete(pluginId)
    plugin.operation = kind
    plugin.error = undefined
    publish(true)
    try {
      const outcome = await mutation()
      if (!ownsPluginMutation(owner)) return
      pluginMutationOwners.delete(pluginId)
      pluginMutationErrors.delete(pluginId)
      if (compareDecimalRevision(outcome.revision, highestPluginRevision) > 0) {
        highestPluginRevision = outcome.revision
      }
      reconcilePluginsAfterMutation()
    } catch (error) {
      if (!ownsPluginMutation(owner)) return
      pluginMutationOwners.delete(pluginId)
      if (model.view === 'settings' && model.viewEpoch === owner.viewEpoch) {
        const current = model.plugins.items.find((item) => item.id === pluginId)
        if (current) {
          current.operation = undefined
          current.error = errorText(error)
          pluginMutationErrors.set(pluginId, current.error)
        }
      }
      reconcilePluginsAfterMutation()
    }
  }

  async function installPlugin(pluginId: string): Promise<void> {
    await mutatePlugin(pluginId, 'install', () => client.installPlugin({ pluginId }))
  }

  async function reloadPlugin(pluginId: string): Promise<void> {
    await mutatePlugin(pluginId, 'reload', () => client.reloadPlugin({ pluginId }))
  }

  async function deletePlugin(pluginId: string): Promise<void> {
    await mutatePlugin(pluginId, 'delete', () => client.deletePlugin({ pluginId }))
  }

  function transitionView(
    target: ShowTarget,
    invocationId: string,
    notice: import('./protocol').LifecycleNotice | null,
    source: 'native' | 'local',
  ): void {
    invalidateFavoriteInteraction()
    const nextView = target === 'launcher' ? 'launcher' : 'settings'
    const nextSettingsTab: SettingsTabKey =
      target === 'messages' ? 'messages' : target === 'settings' ? 'general' : model.settingsTab
    messageCenter.leave()
    if (notice === 'settingsFailed') model.settingsUncertain = true
    if (composition) restoreControl(composition.control)
    composition = undefined
    model.mainResultCommand = undefined
    const preserveDefaultLauncherResults =
      nextView === 'launcher' &&
      source === 'native' &&
      notice === null &&
      !activationNoticePending &&
      model.launcherMode === 'applications' &&
      model.query === '' &&
      model.queryControlValue === '' &&
      model.results.length > 0
    leaveFileMode()
    discardQuicklinksUi()
    model.viewEpoch += 1
    model.invocationId = invocationId
    model.view = nextView
    model.settingsTab = nextSettingsTab
    pluginInventoryActive = false
    completionOrigin = undefined
    if (source === 'native') {
      model.querySequence = 0
      sequenceExhausted = false
    }
    cancelSlashSearch()
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    if (!preserveDefaultLauncherResults) clearResults()
    model.shownNotice = notice === null ? undefined : NOTICE_TEXT[notice]
    if (nextView === 'launcher') pendingSettingsLoadEpoch = undefined
    else queueSettingsLoad()
    if (nextView === 'launcher' && notice === null && activationNoticePending) {
      activationNoticePending = false
      model.shownNotice = REFUSED_NOTICE
    }
    if (nextView === 'launcher') {
      model.query = ''
      model.queryControlValue = ''
      model.querySequence = source === 'native' ? 1 : model.querySequence + 1
      deferCurrentSearch(!preserveDefaultLauncherResults)
    } else {
      model.queryControlValue = model.query
    }
    publish(true)
    if (nextView === 'settings') {
      void drainSettingsLoad()
    }
    if (target === 'messages') void messageCenter.enter()
  }

  function shown(payload: unknown): void {
    if (destroyed) return
    const event = parseLauncherShown(payload)
    if (!event) return
    if (model.view === 'settings' && event.target === 'launcher') {
      if (hotkeyRecording) {
        hotkeyShowConsumedDuringRecording = true
        return
      }
      if (Date.now() <= hotkeyShowGraceUntil) {
        hotkeyShowGraceUntil = 0
        return
      }
      hotkeyShowGraceUntil = 0
    }
    const panelSessionEpoch = model.panel?.sessionEpoch
    discardPanelUi()
    discardQuicklinksUi()
    if (panelSessionEpoch) {
      void client.closePluginPanel({ sessionEpoch: panelSessionEpoch }).catch(() => undefined)
    }
    transitionView(event.target, event.invocationId, event.notice, 'native')
    void messageCenter.refresh()
  }

  function navigate(target: ShowTarget): void {
    const nextView = target === 'launcher' ? 'launcher' : 'settings'
    const nextTab = target === 'messages' ? 'messages' : target === 'settings' ? 'general' : model.settingsTab
    if (
      destroyed ||
      model.invocationId === undefined ||
      (model.view === nextView && (nextView === 'launcher' || model.settingsTab === nextTab))
    ) {
      return
    }
    transitionView(target, model.invocationId, null, 'local')
  }

  function selectSettingsTab(key: SettingsTabKey): void {
    if (destroyed || model.view !== 'settings' || model.settingsTab === key) return
    messageCenter.leave()
    model.settingsTab = key
    publish(true)
    if (key === 'messages') void messageCenter.enter()
  }

  function text(record: ClassifiedTextRecord): void {
    if (destroyed) return
    const queryControl = record.control === model.queryControl
    const mainResultControl = record.control === model.mainResultCommand?.suffix.key
    const panelControl = record.control === model.panel?.suffix.key
    const settingsControl = findTextControl(record.control) !== undefined
    if (!queryControl && !mainResultControl && !panelControl && !settingsControl) return
    if (settingsControl && !settingsEditable()) return
    if (record.kind === 'ordinaryInput') {
      if (ownsComposition(composition, record.control)) composition = undefined
      commitControl(record.control, record.value)
      return
    }
    if (record.kind === 'compositionStart') {
      const restored = composition ? restoreControl(composition.control) : false
      const visibleMutation =
        restored ||
        model.shownNotice !== undefined ||
        ((queryControl || mainResultControl) &&
          (model.searchPending ||
            model.requestId !== undefined ||
            model.results.length > 0 ||
            model.selectedIndex !== -1 ||
            model.status !== ''))
      compositionGeneration += 1
      composition = {
        control: record.control,
        viewEpoch: model.viewEpoch,
        invocationId: model.invocationId,
        generation: compositionGeneration,
        lastTrustedDraft: getControlDraft(record.control) ?? '',
      }
      model.shownNotice = undefined
      if (queryControl || mainResultControl) {
        searchToken = ++token
        model.searchPending = false
        model.status = ''
        clearResults()
      }
      publish(visibleMutation)
      return
    }
    if (record.kind === 'compositionInput') {
      if (ownsComposition(composition, record.control)) {
        composition.lastTrustedDraft = record.value
        publish(setControlDraft(record.control, record.value))
      }
      return
    }
    if (ownsComposition(composition, record.control)) {
      const value = composition.lastTrustedDraft
      composition = undefined
      commitControl(record.control, value)
    }
  }

  function ownsComposition(owner: CompositionOwner | undefined, control: ControlKey): owner is CompositionOwner {
    return (
      owner !== undefined &&
      owner.control === control &&
      owner.viewEpoch === model.viewEpoch &&
      owner.invocationId === model.invocationId &&
      owner.generation === compositionGeneration
    )
  }

  function retireControl(control: ControlKey): void {
    if (composition?.control !== control) return
    const restored = restoreControl(control)
    composition = undefined
    publish(restored)
  }

  function setAutostart(checked: boolean): void {
    if (!settingsEditable() || model.settings!.autostart === checked) return
    const operation = startSettingsOperation('save')
    if (!operation) return
    model.settings!.autostart = checked
    model.shownNotice = undefined
    publish(true)
    void persistSettings(operation, settingsUpdate())
  }

  function setThemePreference(theme: ThemePreference): void {
    if (!settingsEditable() || model.theme === theme) return
    const operation = startSettingsOperation('theme')
    if (!operation) return
    model.theme = theme
    model.status = ''
    publish(true)

    let pending: Promise<void>
    try {
      pending = client.setThemePreference({ preference: { theme } })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      () => finishThemeMutation(operation, theme, false),
      () => finishThemeMutation(operation, theme, true),
    )
  }

  function setWebSearchEngine(engine: WebSearchEngine): void {
    if (!settingsEditable() || model.settings?.webSearchEngine === engine) return
    const operation = startSettingsOperation('webSearchEngine')
    if (!operation) return
    model.settings!.webSearchEngine = engine
    model.status = ''
    publish(true)

    let pending: Promise<void>
    try {
      pending = client.setWebSearchEngine({ preference: { engine } })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      () => finishWebSearchEngineMutation(operation, engine, false),
      () => finishWebSearchEngineMutation(operation, engine, true),
    )
  }

  function setHotkeyCanonical(value: string): void {
    if (!settingsEditable() || !model.settings) return
    const field = model.settings.hotkey
    const hadNotice = model.shownNotice !== undefined
    const changed = setControlDraft(field.key, value)
    const valueChanged = field.value !== value
    if (valueChanged) field.value = value
    model.shownNotice = undefined
    publish(changed || valueChanged || hadNotice)
  }

  function startSettingsOperation(
    kind: SettingsOperationKind,
    owner: SettingsOperation['owner'] = { scope: 'view', viewEpoch: model.viewEpoch, view: model.view },
  ): SettingsOperation | undefined {
    if (destroyed || settingsOperation || (kind !== 'load' && !model.settings)) return undefined
    const operation = { token: ++token, kind, owner }
    settingsOperation = operation
    model.settingsOperation = kind
    model.shownNotice = undefined
    model.status = ''
    publish(true)
    return operation
  }

  function ownsSettingsOperation(operation: SettingsOperation): boolean {
    return !destroyed && settingsOperation?.token === operation.token
  }

  function ownsSettingsView(operation: SettingsOperation): boolean {
    return (
      ownsSettingsOperation(operation) &&
      operation.owner.scope === 'view' &&
      operation.owner.viewEpoch === model.viewEpoch &&
      operation.owner.view === model.view
    )
  }

  function releaseSettingsOperation(operation: SettingsOperation): void {
    if (settingsOperation?.token !== operation.token) return
    settingsOperation = undefined
    model.settingsOperation = undefined
  }

  function settingsUpdate(): UserSettingsUpdate {
    const settings = model.settings!
    return {
      hotkey: settings.hotkey.value,
      autostart: settings.autostart,
      theme: model.theme,
      webSearchEngine: settings.webSearchEngine,
    }
  }

  function drainSettingsLoad(): Promise<void> | undefined {
    const epoch = pendingSettingsLoadEpoch
    if (destroyed || settingsOperation || model.view !== 'settings' || epoch !== model.viewEpoch) return undefined
    pendingSettingsLoadEpoch = undefined
    const operation = startSettingsOperation('load', {
      scope: 'view',
      viewEpoch: epoch,
      view: 'settings',
      previewGeneration: previewPreferenceDurableGeneration,
      themeGeneration: themeDurableGeneration,
    })
    if (!operation) {
      pendingSettingsLoadEpoch = epoch
      return undefined
    }
    return finishSettingsLoad(operation)
  }

  function queueSettingsLoad(): boolean {
    if (destroyed || model.view !== 'settings') return false
    pendingSettingsLoadEpoch = model.viewEpoch
    model.settingsLoadStatus = 'loading'
    model.settingsLoadError = undefined
    return true
  }

  function requestSettingsLoad(): Promise<void> | undefined {
    if (!queueSettingsLoad()) return undefined
    publish(true)
    return drainSettingsLoad()
  }

  async function finishSettingsLoad(operation: SettingsOperation): Promise<void> {
    const previewGeneration =
      operation.owner.scope === 'view' && operation.owner.previewGeneration !== undefined
        ? operation.owner.previewGeneration
        : previewPreferenceDurableGeneration
    const themeGeneration =
      operation.owner.scope === 'view' && operation.owner.themeGeneration !== undefined
        ? operation.owner.themeGeneration
        : themeDurableGeneration
    try {
      const view = await client.loadSettings()
      if (!ownsSettingsOperation(operation)) return
      applyLoadedPreferences(view, previewGeneration, themeGeneration)
      if (ownsSettingsView(operation)) {
        replaceSettingsView(view)
        model.settingsLoadStatus = 'ready'
        model.settingsLoadError = undefined
      }
      releaseSettingsOperation(operation)
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) {
        model.settingsLoadError = errorText(error)
        model.settingsLoadStatus = 'error'
      }
      publish(true)
    }
    void drainSettingsLoad()
  }

  async function reloadSettings(): Promise<void> {
    await requestSettingsLoad()
  }

  function finishSettingsMutation(operation: SettingsOperation, failed: boolean): void {
    if (!ownsSettingsOperation(operation)) return
    if (failed) model.settingsUncertain = true
    releaseSettingsOperation(operation)
    if (model.view === 'settings') {
      void requestSettingsLoad()
    } else {
      publish(true)
    }
  }

  function finishThemeMutation(
    operation: SettingsOperation,
    theme: ThemePreference,
    failed: boolean,
  ): void {
    if (!ownsSettingsOperation(operation)) return
    if (failed) {
      model.theme = durableTheme
    } else {
      durableTheme = theme
      themeDurableGeneration += 1
    }
    releaseSettingsOperation(operation)
    if (model.view !== 'settings') {
      if (failed) model.status = THEME_PREFERENCE_ERROR
      publish(true)
      return
    }
    const reconciliation = requestSettingsLoad()
    if (!failed) return
    void reconciliation?.then(() => {
      if (destroyed) return
      model.status = THEME_PREFERENCE_ERROR
      publish(true)
    })
  }

  function finishWebSearchEngineMutation(
    operation: SettingsOperation,
    engine: WebSearchEngine,
    failed: boolean,
  ): void {
    if (!ownsSettingsOperation(operation)) return
    if (model.settings) {
      model.settings.webSearchEngine = failed ? durableWebSearchEngine : engine
    }
    if (!failed) durableWebSearchEngine = engine
    releaseSettingsOperation(operation)
    if (model.view !== 'settings') {
      if (failed) model.status = WEB_SEARCH_ENGINE_ERROR
      publish(true)
      return
    }
    const reconciliation = requestSettingsLoad()
    if (!failed) return
    void reconciliation?.then(() => {
      if (destroyed) return
      model.status = WEB_SEARCH_ENGINE_ERROR
      publish(true)
    })
  }

  async function persistSettings(operation: SettingsOperation, update: UserSettingsUpdate): Promise<void> {
    try {
      await client.saveSettings({ settings: update })
    } catch {
      finishSettingsMutation(operation, true)
      return
    }
    finishSettingsMutation(operation, false)
  }

  async function resetSettings(): Promise<void> {
    if (!settingsEditable() || !model.settings) return
    const operation = startSettingsOperation('save')
    if (!operation) return
    setControlDraft(model.settings.hotkey.key, 'Shift+Space')
    model.settings.hotkey.value = 'Shift+Space'
    model.settings.autostart = false
    model.settings.webSearchEngine = 'bing'
    model.theme = 'system'
    model.shownNotice = undefined
    publish(true)
    await persistSettings(operation, {
      hotkey: 'Shift+Space',
      autostart: false,
      theme: 'system',
      webSearchEngine: 'bing',
    })
  }

  async function saveHotkeyCanonical(value: string): Promise<void> {
    if (!settingsEditable() || !model.settings) return
    const settings = model.settings
    const operation = startSettingsOperation('hotkey')
    if (!operation) return
    setControlDraft(settings.hotkey.key, value)
    settings.hotkey.value = value
    publish(true)
    try {
      await client.saveHotkey({ hotkey: { hotkey: value } })
    } catch {
      finishSettingsMutation(operation, true)
      return
    }
    finishSettingsMutation(operation, false)
  }

  function discardPanelUi(): boolean {
    pendingPanelFocusRequest = undefined
    panelHostKeyEpoch = undefined
    nextPanelHostKeyClientSequence = 1n
    panelHostKeyEnqueueTail = Promise.resolve()
    if (!model.panel) return false
    if (composition?.control === model.panel?.suffix.key) composition = undefined
    panelActionToken += 1
    panelSubmissionToken += 1
    panelBoundsToken += 1
    searchToken = ++token
    model.searchPending = false
    model.executePending = false
    model.launcherMode = 'applications'
    model.panel = undefined
    model.query = ''
    model.queryControlValue = ''
    clearResults()
    return true
  }

  function resetHiddenNonLauncherView(): boolean {
    if (model.view === 'launcher') return false
    messageCenter.leave()
    if (composition) restoreControl(composition.control)
    composition = undefined
    model.mainResultCommand = undefined
    model.viewEpoch += 1
    model.view = 'launcher'
    pluginInventoryActive = false
    completionOrigin = undefined
    pendingSettingsLoadEpoch = undefined
    cancelSlashSearch()
    searchToken = ++token
    executeToken = ++token
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    model.shownNotice = undefined
    model.query = ''
    model.queryControlValue = ''
    clearResults()
    return true
  }

  function hidden(): void {
    if (destroyed) return
    const discardedPanel = discardPanelUi()
    const discardedQuicklinks = discardQuicklinksUi()
    const wasHiding = model.hidePending
    model.hidePending = false
    const resetView = resetHiddenNonLauncherView()
    publish(discardedPanel || discardedQuicklinks || wasHiding || resetView)
  }

  function setHotkeyRecordingPhase(phase: HotkeyRecordingPhase): void {
    if (destroyed) return
    if (phase === 'recording' && model.view === 'settings') {
      hotkeyRecording = true
      hotkeyShowConsumedDuringRecording = false
      hotkeyShowGraceUntil = 0
      return
    }
    if (phase === 'completed' && hotkeyRecording) {
      hotkeyRecording = false
      hotkeyShowGraceUntil = hotkeyShowConsumedDuringRecording
        ? 0
        : Date.now() + HOTKEY_RECORDING_SHOW_GRACE_MS
      return
    }
    hotkeyRecording = false
    hotkeyShowConsumedDuringRecording = false
    hotkeyShowGraceUntil = 0
  }

  function resetPanelUi(status = ''): void {
    discardPanelUi()
    model.status = status
    if (advanceApplicationSequence()) deferCurrentSearch()
    publish(true)
  }

  function panelIdentityMatches(
    value: { pluginId: string; sessionEpoch: U64Decimal },
    panel: PrivatePanelState,
  ): boolean {
    return value.pluginId === panel.pluginId && value.sessionEpoch === panel.sessionEpoch
  }

  function applyPanelFocusRequest(event: PluginPanelFocusHostInputEvent): boolean {
    const panel = model.panel
    if (!panel || panel.closePending || event.sessionEpoch !== panel.sessionEpoch) return false
    if (
      panel.focusRequestId !== undefined &&
      compareDecimalRevision(event.focusRequestId, panel.focusRequestId) <= 0
    ) return false
    panel.focusRequestId = event.focusRequestId
    return true
  }

  function handlePanelFocusHostInput(payload: unknown): void {
    if (destroyed) return
    const event = parsePluginPanelFocusHostInputEvent(payload)
    if (!event) return
    if (model.panel) {
      publish(applyPanelFocusRequest(event))
      return
    }
    const pending = pendingPanelFocusRequest
    if (
      !pending ||
      compareDecimalRevision(event.sessionEpoch, pending.sessionEpoch) > 0 ||
      (event.sessionEpoch === pending.sessionEpoch &&
        compareDecimalRevision(event.focusRequestId, pending.focusRequestId) > 0)
    ) pendingPanelFocusRequest = event
  }

  function settlePanelHostInputFocus(
    input: PluginPanelFocusHostInputEvent & { focused: boolean },
  ): void {
    const panel = model.panel
    if (
      destroyed || !panel || panel.sessionEpoch !== input.sessionEpoch ||
      panel.closePending || panel.focusRequestId !== input.focusRequestId
    ) return
    void client.acknowledgePluginPanelFocusHostInput(input).catch(() => undefined)
  }

  function setPanelBounds(input: { sessionEpoch: U64Decimal; bounds: PluginPanelBounds }): void {
    const panel = model.panel
    if (
      destroyed || model.view !== 'launcher' || model.launcherMode !== 'panel' ||
      !panel || panel.closePending || panel.sessionEpoch !== input.sessionEpoch
    ) return
    const owner = {
      token: ++panelBoundsToken,
      viewEpoch: model.viewEpoch,
      sessionEpoch: panel.sessionEpoch,
    }
    let pending: ReturnType<LauncherClient['setPluginPanelBounds']>
    try {
      pending = client.setPluginPanelBounds(input)
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.catch((error: unknown) => {
      const current = model.panel
      if (
        destroyed || owner.token !== panelBoundsToken || owner.viewEpoch !== model.viewEpoch ||
        model.launcherMode !== 'panel' || !current || current.closePending ||
        current.sessionEpoch !== owner.sessionEpoch
      ) return
      void closePanelWithError(panelBoundsInvokeNotice(error))
    })
  }

  function preparePanelHostInputFocusListener(): Promise<boolean> {
    if (panelFocusListenerRegistration) return panelFocusListenerRegistration
    panelFocusListenerRegistration = (async () => {
      let registered: (() => void) | undefined
      try {
        registered = await client.listenPluginPanelFocusHostInput(handlePanelFocusHostInput)
      } catch {
        return false
      }
      if (destroyed) {
        registered()
        return false
      }
      unlistenPanelFocusHostInput = registered
      return true
    })()
    return panelFocusListenerRegistration
  }

  function submitPanel(argument: string): void {
    const panel = model.panel
    if (destroyed || model.view !== 'launcher' || model.launcherMode !== 'panel' || !panel || panel.closePending) {
      return
    }
    const owner = {
      token: ++panelSubmissionToken,
      viewEpoch: model.viewEpoch,
      pluginId: panel.pluginId,
      sessionEpoch: panel.sessionEpoch,
      suffixControl: panel.suffix.key,
      argument,
    }
    panel.submitPending = true
    model.status = ''
    publish(true)
    let pending: ReturnType<LauncherClient['submitPluginPanel']>
    try {
      pending = client.submitPluginPanel({
        sessionEpoch: owner.sessionEpoch,
        argument: owner.argument,
        uiIntentEpoch: owner.token,
      })
    } catch (error) {
      pending = Promise.reject(error)
    }
    const owns = () => {
      const current = model.panel
      return !destroyed && owner.token === panelSubmissionToken && owner.viewEpoch === model.viewEpoch &&
        current !== undefined && current.suffix.key === owner.suffixControl && panelIdentityMatches(owner, current)
    }
    void pending.then(
      (result) => {
        if (!owns()) return
        if (!panelIdentityMatches(result, model.panel!)) {
          void closePanelWithError(PANEL_SUBMIT_IDENTITY_NOTICE)
          return
        }
        model.panel!.hostKeys = Object.freeze([...result.hostKeys])
        model.panel!.submitPending = false
        publish(true)
      },
      () => {
        if (owns()) void closePanelWithError(PANEL_SUBMIT_INVOKE_NOTICE)
      },
    )
  }

  function openPanel(result: PrivateApplicationResult): void {
    if (
      destroyed ||
      model.view !== 'launcher' ||
      model.launcherMode !== 'applications' ||
      model.executePending ||
      result.activation.kind !== 'panelActivation' ||
      !model.invocationId
    ) return
    const owner = {
      token: ++panelActionToken,
      viewEpoch: model.viewEpoch,
      invocationId: model.invocationId,
      control: model.queryControl,
      querySequence: model.querySequence,
      query: model.query,
      resultKey: result.key,
      pluginId: result.activation.pluginId,
      argument: result.activation.initialArgument,
    }
    model.executePending = true
    model.status = ''
    publish(true)
    let pending: ReturnType<LauncherClient['openPluginPanel']>
    try {
      pending = client.openPluginPanel({ pluginId: owner.pluginId, argument: owner.argument })
    } catch (error) {
      pending = Promise.reject(error)
    }
    const ownsAction = () => !destroyed && owner.token === panelActionToken &&
      owner.viewEpoch === model.viewEpoch && owner.invocationId === model.invocationId
    const ownsQuery = () => ownsAction() && owner.control === model.queryControl &&
      owner.querySequence === model.querySequence && owner.query === model.query &&
      owner.query === model.queryControlValue && model.results.some(({ key }) => key === owner.resultKey)
    void pending.then(
      (identity) => {
        if (!ownsQuery()) {
          if (ownsAction()) {
            model.executePending = false
            publish(true)
          }
          void client.closePluginPanel({ sessionEpoch: identity.sessionEpoch }).catch(() => undefined)
          return
        }
        if (identity.pluginId !== owner.pluginId) {
          model.executePending = false
          model.status = FALLBACK_ERROR
          publish(true)
          void client.closePluginPanel({ sessionEpoch: identity.sessionEpoch }).catch(() => undefined)
          return
        }
        searchToken = ++token
        model.searchPending = false
        model.executePending = false
        model.launcherMode = 'panel'
        model.query = ''
        model.queryControlValue = ''
        clearResults()
        model.panel = {
          pluginId: identity.pluginId,
          commandLabel: identity.commandLabel,
          sessionEpoch: identity.sessionEpoch,
          hostKeys: Object.freeze([...identity.hostKeys]),
          suffix: newTextControl(owner.argument),
          submitPending: false,
          closePending: false,
        }
        panelHostKeyEpoch = identity.sessionEpoch
        nextPanelHostKeyClientSequence = 1n
        panelHostKeyEnqueueTail = Promise.resolve()
        if (pendingPanelFocusRequest?.sessionEpoch === identity.sessionEpoch) {
          applyPanelFocusRequest(pendingPanelFocusRequest)
        }
        pendingPanelFocusRequest = undefined
        publish(true)
        submitPanel(owner.argument)
      },
      () => {
        if (!ownsAction()) return
        model.executePending = false
        if (ownsQuery()) model.status = FALLBACK_ERROR
        publish(true)
      },
    )
  }

  async function closePanelWithError(notice = FALLBACK_ERROR): Promise<void> {
    await closePanel(FALLBACK_ERROR, notice)
  }

  async function closePanel(status = '', notice?: string): Promise<void> {
    const panel = model.panel
    if (destroyed || model.launcherMode !== 'panel' || !panel || panel.closePending) return
    const owner = {
      token: ++panelActionToken,
      viewEpoch: model.viewEpoch,
      sessionEpoch: panel.sessionEpoch,
      suffixControl: panel.suffix.key,
    }
    panel.closePending = true
    panel.focusRequestId = undefined
    publish(true)
    try {
      await client.closePluginPanel({ sessionEpoch: owner.sessionEpoch })
      const current = model.panel
      if (
        destroyed ||
        owner.token !== panelActionToken ||
        owner.viewEpoch !== model.viewEpoch ||
        current?.sessionEpoch !== owner.sessionEpoch ||
        current.suffix.key !== owner.suffixControl
      ) return
      resetPanelUi(status)
      if (notice !== undefined) {
        model.shownNotice = notice
        publish(true)
      }
    } catch {
      const current = model.panel
      if (
        destroyed || owner.token !== panelActionToken || owner.viewEpoch !== model.viewEpoch ||
        current?.sessionEpoch !== owner.sessionEpoch
      ) return
      current.closePending = false
      current.submitPending = false
      model.status = FALLBACK_ERROR
      publish(true)
    }
  }

  function handlePanelError(payload: unknown): void {
    const event = parsePluginPanelErrorEvent(payload)
    const panel = model.panel
    if (!event || !panel || event.sessionEpoch !== panel.sessionEpoch) return
    resetPanelUi(FALLBACK_ERROR)
  }

  function handlePanelReset(payload: unknown): void {
    const event = parsePluginPanelErrorEvent(payload)
    const panel = model.panel
    if (!event || !panel || event.sessionEpoch !== panel.sessionEpoch) return
    resetPanelUi()
  }

  function applyPluginCompletion(result: PrivateApplicationResult): void {
    if (sequenceExhausted || result.activation.kind !== 'pluginCompletion') return
    const command = completionCommand(result.activation.completionText)
    if (command === undefined || !advanceApplicationSequence()) return
    model.shownNotice = undefined
    model.query = result.activation.completionText
    model.queryControlValue = result.activation.completionText
    model.status = ''
    searchToken = ++token
    model.searchPending = false
    clearResults()
    replaceCompletionOrigin({
      resultKey: result.key,
      pluginId: result.activation.pluginId,
      command,
    }, 'armed')
    scheduleSearch()
    publish(true)
  }

  function commitArmedPluginCompletion(): boolean {
    if (!ownsCompletionOrigin(completionOrigin)) return false
    if (completionOrigin.phase === 'committing' || completionOrigin.phase === 'consumed') return true
    const previous = completionOrigin
    cancelSlashSearch()
    searchToken = ++token
    model.searchPending = false
    if (!advanceApplicationSequence()) return true
    replaceCompletionOrigin(previous, 'committing')
    model.shownNotice = undefined
    model.status = ''
    clearResults()
    beginSearch(true)
    publish(true)
    return true
  }

  function applyMainResultActivation(result: PrivateApplicationResult): void {
    if (sequenceExhausted || result.activation.kind !== 'mainResultActivation') return
    const activation = result.activation
    if (!advanceApplicationSequence()) return
    cancelSlashSearch()
    searchToken = ++token
    model.searchPending = false
    model.shownNotice = undefined
    model.status = ''
    clearResults()
    activateMainResultCommand({
      pluginId: activation.pluginId,
      commandLabel: activation.commandLabel,
      argument: activation.initialArgument,
    })
    replaceCompletionOrigin({
      resultKey: result.key,
      pluginId: activation.pluginId,
      command: activation.commandLabel,
    }, 'armed')
    scheduleSearch()
    publish(true)
  }

  function applyWindowActivation(result: PrivateApplicationResult): void {
    if (sequenceExhausted || result.activation.kind !== 'windowActivation') return
    const activation = result.activation
    if (!advanceApplicationSequence()) return
    cancelSlashSearch()
    searchToken = ++token
    model.searchPending = false
    model.shownNotice = undefined
    model.status = ''
    model.query = activation.initialArgument
      ? `/${activation.commandLabel} ${activation.initialArgument}`
      : `/${activation.commandLabel}`
    model.queryControlValue = model.query
    clearResults()
    replaceCompletionOrigin({
      resultKey: result.key,
      pluginId: activation.pluginId,
      command: activation.commandLabel,
    }, 'committing')
    beginSearch(true)
    publish(true)
  }

  function executeSelection(): void {
    if (model.view !== 'launcher' || model.executePending) return
    invalidateFavoriteInteraction()
    let resultId: string | undefined
    if (model.launcherMode === 'applications') {
      const selected = model.results[model.selectedIndex]
      if (!selected) return
      if (selected.activation.kind === 'completion') {
        applyEdit(selected.activation.completionText)
        return
      }
      if (selected.activation.kind === 'pluginCompletion') {
        applyPluginCompletion(selected)
        return
      }
      if (selected.activation.kind === 'windowActivation') {
        applyWindowActivation(selected)
        return
      }
      if (selected.activation.kind === 'mainResultActivation') {
        applyMainResultActivation(selected)
        return
      }
      if (selected.activation.kind === 'panelActivation') {
        openPanel(selected)
        return
      }
      if (selected.activation.kind === 'openFind') {
        submitFind(selected.activation.query)
        return
      }
      if (selected.activation.kind === 'openQuicklinks') {
        openQuicklinks()
        return
      }
      if (selected.hasDefaultAction === false) return
      resultId = selected.resultId
    } else {
      resultId = model.file?.results[model.file.selectedIndex]?.resultId
    }
    if (!model.requestId || resultId === undefined) return
    model.shownNotice = undefined
    model.status = ''
    model.executePending = true
    const captured = { token: ++token, epoch: model.viewEpoch, invocationId: model.invocationId }
    executeToken = captured.token
    const requestId = model.requestId
    publish(true)
    let pending: Promise<ExecuteOutcome>
    try {
      pending = client.executeResult({ requestId, resultId })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (outcome) => {
        if (outcome.status === 'activationRefusedLaunchRequested') activationNoticePending = true
        if (destroyed || captured.token !== executeToken || captured.epoch !== model.viewEpoch || captured.invocationId !== model.invocationId) return
        model.executePending = false
        publish(true)
      },
      (error: unknown) => {
        if (destroyed || captured.token !== executeToken || captured.epoch !== model.viewEpoch || captured.invocationId !== model.invocationId) return
        model.executePending = false
        model.status = errorText(error)
        publish(true)
      },
    )
  }

  function activateResult(index: number): void {
    if (
      sequenceExhausted ||
      model.view !== 'launcher' ||
      model.launcherMode !== 'applications' ||
      !Number.isInteger(index) ||
      index < 0 ||
      index >= model.results.length
    ) return
    invalidateFavoriteInteraction()
    model.selectedIndex = index
    publish(true)
    executeSelection()
  }

  function openPluginContextMenu(index: number): void {
    if (
      destroyed ||
      sequenceExhausted ||
      model.view !== 'launcher' ||
      model.launcherMode !== 'applications' ||
      !Number.isInteger(index) ||
      index < 0 ||
      index >= model.results.length
    ) return
    const result = model.results[index]
    const resultFavorite = result?.favorite
    if (!resultFavorite || !model.invocationId) return
    if (
      favoriteInteraction &&
      ownsFavoriteInteraction(favoriteInteraction) &&
      favoriteInteraction.resultKey === result.key &&
      sameFavoriteTarget(favoriteInteraction.target, resultFavorite.target)
    ) {
      favoriteMenuConsumed = false
      publish(false)
      return
    }
    invalidateFavoriteInteraction()
    model.selectedIndex = index
    favoriteInteraction = {
      token: favoriteInteractionToken,
      epoch: model.viewEpoch,
      invocationId: model.invocationId,
      control: model.queryControl,
      querySequence: model.querySequence,
      value: model.queryControlValue,
      resultKey: result.key,
      target: resultFavorite.target,
    }
    publish(true)
  }

  function closePluginContextMenu(): void {
    if (favoriteMenuConsumed) {
      favoriteMenuConsumed = false
      return
    }
    invalidateFavoriteInteraction()
  }

  function setPluginFavorite(index: number, favorite: boolean): void {
    if (model.favoriteMutationPending || !Number.isInteger(index)) return
    const result = model.results[index]
    const resultFavorite = result?.favorite
    const interaction = favoriteInteraction
    if (
      !interaction ||
      !ownsFavoriteInteraction(interaction) ||
      result?.key !== interaction.resultKey ||
      resultFavorite === undefined ||
      !sameFavoriteTarget(resultFavorite.target, interaction.target) ||
      resultFavorite.favorite === favorite
    ) return
    const owner: FavoriteMutationOwner = { ...interaction, favorite }
    favoriteMutation = owner
    favoriteMenuConsumed = true
    model.favoriteMutationPending = true
    model.status = ''
    publish(true)
    let pending: Promise<void>
    try {
      pending = owner.target.kind === 'publicPlugin'
        ? client.setPublicPluginFavorite({ pluginId: owner.target.pluginId, favorite })
        : client.setBuiltinFeatureFavorite({ feature: owner.target.feature, favorite })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      () => finishFavoriteMutation(owner, false),
      () => finishFavoriteMutation(owner, true),
    )
  }

  function finishFavoriteMutation(owner: FavoriteMutationOwner, failed: boolean): void {
    if (
      favoriteMutation?.token !== owner.token ||
      !sameFavoriteTarget(favoriteMutation.target, owner.target)
    ) return
    favoriteMutation = undefined
    model.favoriteMutationPending = false
    if (!ownsFavoriteInteraction(owner)) {
      publish(true)
      return
    }
    if (failed) {
      model.status = FALLBACK_ERROR
      publish(true)
      return
    }
    applyEdit(owner.value)
  }

  async function requestHide(): Promise<void> {
    if (destroyed || model.hidePending) return
    model.shownNotice = undefined
    model.status = ''
    model.hidePending = true
    completionOrigin = undefined
    invalidateFavoriteInteraction()
    leaveFileMode()
    discardQuicklinksUi()
    const captured = { token: ++token, epoch: model.viewEpoch }
    hideToken = captured.token
    publish(true)
    try {
      await client.hideLauncher()
      if (destroyed || captured.token !== hideToken || captured.epoch !== model.viewEpoch) return
      model.hidePending = false
      discardPanelUi()
      resetHiddenNonLauncherView()
      publish(true)
    } catch (error) {
      if (destroyed || captured.token !== hideToken || captured.epoch !== model.viewEpoch) return
      model.hidePending = false
      model.status = errorText(error)
      publish(true)
    }
  }

  function routePanelHostKey(input: PluginPanelHostKeyPhysicalInput): boolean {
    const panel = model.panel
    if (destroyed || input.isComposing || !panel || panel.closePending || panelHostKeyEpoch !== panel.sessionEpoch) {
      return false
    }
    let declaration: PanelHostKeyDeclaration | undefined
    let key: PluginPanelHostKey | undefined
    if (!input.ctrlKey && !input.metaKey && !input.shiftKey && !input.altKey) {
      if (input.key === 'ArrowDown') declaration = key = 'ArrowDown'
      else if (input.key === 'ArrowUp') declaration = key = 'ArrowUp'
      else if (input.key === 'Tab') declaration = key = 'Tab'
      else if (input.key === 'Enter') declaration = key = 'Enter'
    } else if (
      input.key === 'Tab' &&
      input.shiftKey &&
      !input.ctrlKey &&
      !input.metaKey &&
      !input.altKey
    ) {
      declaration = 'Shift+Tab'
      key = 'Tab'
    } else if (
      input.key.toLowerCase() === 'n' && !input.shiftKey && !input.altKey &&
      (input.platform === 'windows'
        ? input.ctrlKey && !input.metaKey
        : input.metaKey && !input.ctrlKey)
    ) {
      declaration = 'Primary+N'
      key = 'n'
    }
    if (!declaration || !key || !panel.hostKeys.includes(declaration)) return false

    const maxU64 = (1n << 64n) - 1n
    if (nextPanelHostKeyClientSequence > maxU64) {
      void requestHide()
      return true
    }
    const owner = {
      sessionEpoch: panel.sessionEpoch,
      clientSequence: String(nextPanelHostKeyClientSequence) as U64Decimal,
      declaration,
      key,
      ctrlKey: input.ctrlKey,
      metaKey: input.metaKey,
      shiftKey: input.shiftKey,
      altKey: input.altKey,
    }
    nextPanelHostKeyClientSequence += 1n
    panelHostKeyEnqueueTail = panelHostKeyEnqueueTail.then(async () => {
      if (destroyed || model.panel?.sessionEpoch !== owner.sessionEpoch || panelHostKeyEpoch !== owner.sessionEpoch) return
      try {
        const result = await client.enqueuePluginPanelHostKey(owner)
        if (result.outcome === 'protocolViolation') await requestHide()
      } catch {
        await requestHide()
      }
    })
    return true
  }

  function keyDown(key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean): void {
    if (destroyed || isComposing) return
    if (key === 'Escape') {
      if (model.view === 'settings') navigate('launcher')
      else if (model.launcherMode === 'quicklinks') closeQuicklinks()
      else void requestHide()
      return
    }
    if (key === 'Enter') {
      if (sequenceExhausted) return
      if (model.view === 'launcher' && model.launcherMode === 'panel') {
        const panel = model.panel
        if (panel) submitPanel(panel.suffix.value)
        return
      }
      if (model.view === 'launcher' && model.launcherMode === 'quicklinks') return
      if (
        model.view === 'launcher' &&
        model.launcherMode === 'applications' &&
        model.selectedIndex >= 0 &&
        model.selectedIndex < model.results.length
      ) {
        executeSelection()
        return
      }
      if (model.view === 'launcher' && model.launcherMode === 'applications' && commitArmedPluginCompletion()) {
        return
      }
      const fileQuery = model.launcherMode === 'applications' ? fileCommand(model.query) : null
      if (model.view === 'launcher' && fileQuery !== null && model.queryControlValue === model.query) {
        submitFind(fileQuery)
        return
      }
      if (
        model.launcherMode === 'applications' && model.view === 'launcher' && model.searchPending &&
        !model.executePending && !model.results.length && model.query !== '' &&
        !model.query.startsWith('/') && model.queryControlValue === model.query && model.invocationId
      ) {
        pendingDefaultActivation = {
          epoch: model.viewEpoch,
          invocationId: model.invocationId,
          sequence: model.querySequence,
          query: model.query,
        }
        return
      }
      if (
        model.launcherMode === 'applications' &&
        model.view === 'launcher' &&
        !model.executePending &&
        !model.results.length &&
        model.query !== '' &&
        model.queryControlValue === model.query &&
        (!model.searchPending || model.query.startsWith('/'))
      ) {
        model.shownNotice = undefined
        cancelSlashSearch()
        if (model.query.startsWith('/')) {
          model.querySequence += 1
          beginSearch(true)
        } else applyEdit(model.query)
        publish(true)
        return
      }
      executeSelection()
      return
    }
    if (sequenceExhausted) return
    if (model.launcherMode === 'panel') return
    if (model.launcherMode === 'quicklinks') return
    if (model.launcherMode === 'files') {
      const file = model.file
      if (!file?.results.length) return
      model.shownNotice = undefined
      const offset = key === 'ArrowDown' ? 1 : -1
      const selectedIndex = (file.selectedIndex + offset + file.results.length) % file.results.length
      if (selectedIndex === file.selectedIndex) return
      invalidateFavoriteInteraction()
      file.selectedIndex = selectedIndex
      publish(true)
      return
    }
    if (!model.results.length) return
    model.shownNotice = undefined
    const offset = key === 'ArrowDown' ? 1 : -1
    const selectedIndex = (model.selectedIndex + offset + model.results.length) % model.results.length
    if (selectedIndex === model.selectedIndex) return
    invalidateFavoriteInteraction()
    model.selectedIndex = selectedIndex
    publish(true)
  }

  function failInitialization(): void {
    if (destroyed || model.status === FALLBACK_ERROR) return
    model.status = FALLBACK_ERROR
    publish(true)
  }

  async function start(): Promise<void> {
    if (started || destroyed) return
    started = true
    if (!await preparePanelHostInputFocusListener()) {
      failInitialization()
      return
    }
    let registered: (() => void) | undefined
    let hiddenRegistered: (() => void) | undefined
    let panelErrorsRegistered: (() => void) | undefined
    let panelResetsRegistered: (() => void) | undefined
    try {
      registered = await client.listenShown(shown)
      hiddenRegistered = await client.listenHidden(hidden)
      panelErrorsRegistered = await client.listenPluginPanelError(handlePanelError)
      panelResetsRegistered = await client.listenPluginPanelReset(handlePanelReset)
    } catch {
      registered?.()
      hiddenRegistered?.()
      panelErrorsRegistered?.()
      panelResetsRegistered?.()
      unlistenPanelFocusHostInput?.()
      unlistenPanelFocusHostInput = undefined
      failInitialization()
      return
    }
    if (destroyed) {
      registered()
      hiddenRegistered()
      panelErrorsRegistered()
      panelResetsRegistered()
      return
    }
    unlisten = registered
    unlistenHidden = hiddenRegistered
    unlistenPanelError = panelErrorsRegistered
    unlistenPanelReset = panelResetsRegistered
    await messageCenter.start()
    if (destroyed) return
    const operation = startSettingsOperation('load', {
      scope: 'startup',
      previewGeneration: previewPreferenceDurableGeneration,
      themeGeneration: themeDurableGeneration,
    })
    if (!operation) return
    try {
      const settings = await client.loadSettings()
      if (!ownsSettingsOperation(operation)) return
      if (operation.owner.scope !== 'startup') return
      applyLoadedPreferences(
        settings,
        operation.owner.previewGeneration,
        operation.owner.themeGeneration,
      )
      if (model.view !== 'settings') {
        replaceSettingsView(settings)
      }
      releaseSettingsOperation(operation)
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      releaseSettingsOperation(operation)
      if (model.view !== 'settings') model.status = errorText(error)
      publish(true)
    }
    void drainSettingsLoad()
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    cancelSlashSearch()
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    completionOrigin = undefined
    favoriteMutation = undefined
    model.favoriteMutationPending = false
    invalidateFavoriteInteraction()
    settingsOperation = undefined
    pendingSettingsLoadEpoch = undefined
    pluginListOwner = undefined
    pluginMutationOwners.clear()
    unsubscribeMessages?.()
    unsubscribeMessages = undefined
    messageCenter.destroy()
    unlisten?.()
    unlisten = undefined
    unlistenHidden?.()
    unlistenHidden = undefined
    unlistenPanelError?.()
    unlistenPanelError = undefined
    unlistenPanelReset?.()
    unlistenPanelReset = undefined
    unlistenPanelFocusHostInput?.()
    unlistenPanelFocusHostInput = undefined
    pendingPanelFocusRequest = undefined
    listeners.clear()
  }

  function setFileCategory(category: FileCategory): void {
    const file = model.file
    if (model.launcherMode !== 'files' || !file || file.category === category) return
    file.category = category
    file.results = []
    file.selectedIndex = -1
    file.total = '0'
    model.requestId = undefined
    model.status = ''
    searchToken = ++token
    model.searchPending = false
    if (model.query.length === 0 || !nextFileSequence()) {
      publish(true)
      return
    }
    beginFileSearch()
  }

  function cycleFileCategory(direction: 'next' | 'previous'): void {
    const file = model.file
    if (model.launcherMode !== 'files' || !file) return
    const index = FILE_CATEGORY_ORDER.indexOf(file.category)
    const offset = direction === 'next' ? 1 : -1
    const nextIndex = (index + offset + FILE_CATEGORY_ORDER.length) % FILE_CATEGORY_ORDER.length
    setFileCategory(FILE_CATEGORY_ORDER[nextIndex]!)
  }

  function setFileSort(_sort: FileSort): void {
    if (model.file) model.file.sort = 'modifiedDesc'
  }

  function setFilePreviewEnabled(enabled: boolean): void {
    const file = model.file
    if (
      model.launcherMode !== 'files' ||
      !file ||
      previewPreferencePending !== undefined ||
      file.previewEnabled === enabled
    ) {
      return
    }
    const owner = { token: ++previewPreferenceToken, enabled }
    previewPreferencePending = owner
    file.previewEnabled = enabled
    file.preferencePending = true
    model.status = ''
    publish(true)
    let pending: Promise<void>
    try {
      pending = legacyFindClient.setPreviewPreference({ preference: { enabled } }).then(() => undefined)
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      () => {
        if (previewPreferencePending?.token !== owner.token) return
        previewPreferencePending = undefined
        lastLoadedFilePreviewEnabled = enabled
        previewPreferenceDurableGeneration += 1
        if (destroyed) return
        const current = model.file
        if (!current) return
        const changed = current.previewEnabled !== enabled || current.preferencePending
        current.previewEnabled = enabled
        current.durablePreviewEnabled = enabled
        current.preferencePending = false
        publish(changed)
      },
      () => {
        if (previewPreferencePending?.token !== owner.token) return
        previewPreferencePending = undefined
        if (destroyed) return
        const current = model.file
        if (!current) return
        current.durablePreviewEnabled = lastLoadedFilePreviewEnabled
        current.previewEnabled = lastLoadedFilePreviewEnabled
        current.preferencePending = false
        model.status = FILE_PREVIEW_ERROR
        publish(true)
      },
    )
  }

  async function clearMessages(): Promise<void> {
    await messageCenter.clear()
  }

  return {
    client,
    getSnapshot,
    subscribe,
    start,
    preparePanelHostInputFocusListener,
    failInitialization,
    shown,
    text,
    retireControl,
    keyDown,
    routePanelHostKey,
    navigate,
    selectSettingsTab,
    requestHide,
    closeMainResultCommand,
    closePanel,
    closeQuicklinks,
    newQuicklink,
    selectQuicklink,
    completeQuicklink,
    setQuicklinkDraft,
    chooseQuicklinkIcon,
    saveQuicklink,
    deleteQuicklink,
    setPanelBounds,
    settlePanelHostInputFocus,
    activateResult,
    openPluginContextMenu,
    closePluginContextMenu,
    setPluginFavorite,
    setAutostart,
    setThemePreference,
    setWebSearchEngine,
    setHotkeyCanonical,
    setHotkeyRecordingPhase,
    saveHotkeyCanonical,
    clearMessages,
    setFileCategory,
    cycleFileCategory,
    setFileSort,
    setFilePreviewEnabled,
    resetSettings,
    reloadSettings,
    activatePlugins,
    deactivatePlugins,
    reloadPlugins,
    installPlugin,
    reloadPlugin,
    deletePlugin,
    destroy,
  }
}
