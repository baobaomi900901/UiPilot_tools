import {
  compareDecimalRevision,
  parseFileSearchResponse,
  parseLauncherShown,
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
  type LauncherSnapshot,
  type PluginInventoryView,
  type PluginListStatus,
  type PluginMutationKind,
  type ResultItem,
  type SettingsLoadStatus,
  type SettingsView,
  type ThemePreference,
  type UserSettingsUpdate,
  type ViewResult,
} from './protocol'

export interface LauncherCore {
  readonly client: LauncherClient
  readonly getSnapshot: () => LauncherSnapshot
  readonly subscribe: (listener: () => void) => () => void
  readonly start: () => Promise<void>
  readonly failInitialization: () => void
  readonly shown: (payload: unknown) => void
  readonly text: (record: ClassifiedTextRecord) => void
  readonly retireControl: (control: ControlKey) => void
  readonly keyDown: (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean) => void
  readonly requestHide: () => Promise<void>
  readonly activateResult: (index: number) => void
  readonly setAutostart: (checked: boolean) => void
  readonly setThemePreference: (theme: ThemePreference) => void
  readonly setHotkeyCanonical: (value: string) => void
  readonly saveHotkeyCanonical: (value: string) => Promise<void>
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
}

interface PrivateFindResult extends ViewResult {
  kind: 'find'
  query: string
}

type PrivateResult = PrivateApplicationResult | PrivateFindResult

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

interface Model {
  view: 'launcher' | 'settings'
  launcherMode: 'applications' | 'files'
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
  shownNotice?: string
  status: string
  settings?: PrivateSettings
  settingsOperation?: SettingsOperationKind
  settingsUncertain: boolean
  settingsLoadStatus: SettingsLoadStatus
  settingsLoadError?: string
  file?: PrivateFileState
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

type SettingsOperationKind = 'load' | 'save' | 'hotkey' | 'theme'

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
  clipboardWriteFailed: '无法复制到剪贴板。',
  pluginPermissionDenied: '插件无权写入剪贴板。',
  pluginListFailed: '无法加载插件清单。',
  pluginInstallFailed: '无法安装插件。',
  pluginReloadFailed: '无法重新加载插件。',
  pluginDeleteFailed: '无法删除插件。',
}

const NOTICE_TEXT = {
  settingsFailed: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
} as const

const REFUSED_NOTICE = 'Windows 拒绝了前台切换，已发送启动请求'
const FALLBACK_ERROR = '操作不可用，请重试。'
const FILE_PREVIEW_ERROR = '无法保存文件预览设置。'
const THEME_PREFERENCE_ERROR = '无法保存风格设置。'
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

function safeApplicationIcon(value: unknown): string | undefined {
  if (typeof value !== 'string' || value.length > MAX_ICON_LENGTH || !value.startsWith(ICON_PREFIX)) return undefined
  const payload = value.slice(ICON_PREFIX.length)
  return payload.length > 0 && BASE64.test(payload) ? value : undefined
}

function errorText(value: unknown): string {
  if (typeof value !== 'object' || value === null || !Object.prototype.hasOwnProperty.call(value, 'code')) return FALLBACK_ERROR
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' && ERROR_CODES.has(code) ? ERROR_TEXT[code as CommandErrorCode] : FALLBACK_ERROR
}

function projectSnapshot(model: Model): LauncherSnapshot {
  const results = Object.freeze(
    model.results.map(({ key, title, subtitle, icon, detail, hasDefaultAction }) =>
      Object.freeze({
        key,
        title,
        ...(subtitle === undefined ? {} : { subtitle }),
        ...(icon === undefined ? {} : { icon }),
        ...(detail === undefined ? {} : { detail }),
        ...(hasDefaultAction === undefined ? {} : { hasDefaultAction }),
      }),
    ),
  )
  const settings = model.settings
    ? Object.freeze({
        hotkey: Object.freeze({ key: model.settings.hotkey.key, value: model.settings.hotkey.draft }),
        autostart: model.settings.autostart,
        theme: model.theme,
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
  const plugins = Object.freeze({
    status: model.plugins.status,
    items: Object.freeze(
      model.plugins.items.map((plugin) =>
        Object.freeze({ ...plugin }),
      ),
    ),
    ...(model.plugins.error === undefined ? {} : { error: model.plugins.error }),
  })
  return Object.freeze({
    view: model.view,
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
    ...(model.shownNotice === undefined ? {} : { shownNotice: model.shownNotice }),
    status:
      model.view === 'settings' && model.settingsUncertain
        ? NOTICE_TEXT.settingsFailed
        : model.view === 'settings' && model.settingsLoadError
          ? model.settingsLoadError
          : model.status,
    ...(settings === undefined ? {} : { settings }),
    ...(model.view === 'settings' ? { settingsLoadStatus: model.settingsLoadStatus, plugins } : {}),
    ...(file === undefined ? {} : { file }),
  })
}

export function createLauncherCore(client: LauncherClient, maximumQuerySequence = Number.MAX_SAFE_INTEGER): LauncherCore {
  const model: Model = {
    view: 'launcher',
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
  let previewPreferenceToken = 0
  let previewPreferencePending: PreviewPreferenceOwner | undefined
  let previewPreferenceDurableGeneration = 0
  let lastLoadedFilePreviewEnabled = true
  let themeDurableGeneration = 0
  let durableTheme: ThemePreference = 'system'
  let token = 0
  let searchToken = 0
  let slashSearchTimer: ReturnType<typeof setTimeout> | undefined
  let executeToken = 0
  let hideToken = 0
  let resultKey = 1
  let controlKey = 2
  let activationNoticePending = false
  let compositionGeneration = 0
  let composition: CompositionOwner | undefined
  let settingsOperation: SettingsOperation | undefined
  let pendingSettingsLoadEpoch: number | undefined
  let pluginListOwner: PluginListOwner | undefined
  const pluginMutationOwners = new Map<string, PluginMutationOwner>()
  const pluginMutationErrors = new Map<string, string>()
  let highestPluginRevision = '0'
  let pluginInventoryActive = false
  const legacyFindClient = client as unknown as Pick<FindClient, 'searchFiles' | 'setPreviewPreference'>
  let findSubmissionToken = 0


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
  }

  function replaceSettingsView(view: SettingsView): void {
    if (model.settings) {
      for (const control of settingsControls(model.settings)) retireControl(control.key)
    }
    model.settings = {
      hotkey: newTextControl(view.hotkey),
      autostart: view.autostart,
    }
  }

  function findTextControl(control: ControlKey): TextControl | undefined {
    if (!model.settings) return undefined
    if (model.settings.hotkey.key === control) return model.settings.hotkey
    return undefined
  }

  function getControlDraft(control: ControlKey): string | undefined {
    if (control === model.queryControl) return model.queryControlValue
    return findTextControl(control)?.draft
  }

  function setControlDraft(control: ControlKey, value: string): boolean {
    if (control === model.queryControl) {
      const changed = model.queryControlValue !== value
      model.queryControlValue = value
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
    const field = findTextControl(control)
    return field ? setControlDraft(control, field.value) : false
  }

  function commitControl(control: ControlKey, value: string): void {
    if (control === model.queryControl) {
      const visibleChanged = setControlDraft(control, value)
      if (model.query === value) {
        publish(visibleChanged)
        return
      }
      applyEdit(value)
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

  function fileCommand(value: string): string | null {
    if (value === '/find') return ''
    return value.startsWith('/find ') ? value.slice(6) : null
  }

  function localFindResult(query: string): PrivateFindResult {
    return {
      kind: 'find',
      key: resultKey++,
      query,
      title: '/find',
      subtitle: `搜索文件：${query}`,
    }
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
    let pending
    try {
      pending = client.openFind({ query, invocationId, querySequence: owner.querySequence })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (outcome) => {
        if (destroyed || outcome.status !== 'forwarded' || owner.token !== findSubmissionToken ||
            owner.epoch !== model.viewEpoch || owner.control !== model.queryControl ||
            owner.value !== model.queryControlValue || owner.invocationId !== model.invocationId ||
            owner.querySequence !== model.querySequence) return
        searchToken = ++token
        model.searchPending = false
        model.query = ''
        model.queryControlValue = ''
        model.status = ''
        clearResults()
        publish(true)
      },
      () => {
        if (destroyed || owner.token !== findSubmissionToken || owner.epoch !== model.viewEpoch ||
            owner.control !== model.queryControl || owner.value !== model.queryControlValue ||
            owner.invocationId !== model.invocationId || owner.querySequence !== model.querySequence) return
        model.status = '文件搜索窗口暂不可用。'
        publish(true)
      },
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

  function beginSearch(submit = false): void {
    const invocationId = model.invocationId
    if (!invocationId || model.query === '' || fileCommand(model.query) !== null) return
    if (!model.query.startsWith('/')) {
      model.results = [localFindResult(model.query)]
      model.selectedIndex = 0
    }
    const captured = {
      token: ++token,
      epoch: model.viewEpoch,
      invocationId,
      sequence: model.querySequence,
      query: model.query,
    }
    searchToken = captured.token
    model.searchPending = true
    let pending: Promise<import('./protocol').SearchResponse | null>
    try {
      pending = client.searchApps({
        query: captured.query,
        invocationId,
        querySequence: captured.sequence,
        ...(captured.query.startsWith('/') ? { submit } : {}),
      })
    } catch (error) {
      pending = Promise.reject(error)
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
  function ownsSearch(captured: { token: number; epoch: number; invocationId: string; sequence: number; query: string }): boolean {
    return (
      !destroyed &&
      captured.token === searchToken &&
      captured.epoch === model.viewEpoch &&
      captured.invocationId === model.invocationId &&
      captured.sequence === model.querySequence &&
      captured.query === model.query &&
      captured.query === model.queryControlValue
    )
  }

  function finishSearch(
    captured: { token: number; epoch: number; invocationId: string; sequence: number; query: string },
    response: import('./protocol').SearchResponse | null,
  ): void {
    if (!ownsSearch(captured)) return
    model.searchPending = false
    if (response !== null) {
      model.requestId = response.requestId
      const findResult = model.results.find((item): item is PrivateFindResult => item.kind === 'find')
      const applications: PrivateApplicationResult[] = response.items.map((item: ResultItem) => {
        const icon = safeApplicationIcon(item.icon)
        return {
          kind: 'application',
          key: resultKey++,
          resultId: item.resultId,
          title: item.title,
          ...(item.subtitle === undefined ? {} : { subtitle: item.subtitle }),
          ...(icon === undefined ? {} : { icon }),
          ...(item.detail === undefined ? {} : { detail: item.detail }),
          ...(item.hasDefaultAction === undefined ? {} : { hasDefaultAction: item.hasDefaultAction }),
        }
      })
      model.results = findResult ? [findResult, ...applications] : applications
      model.selectedIndex = model.results.length ? 0 : -1
      model.status = model.results.length ? '' : '未找到应用'
    }
    publish(true)
  }

  function failSearch(
    captured: { token: number; epoch: number; invocationId: string; sequence: number; query: string },
    error: unknown,
  ): void {
    if (!ownsSearch(captured)) return
    model.searchPending = false
    model.status = errorText(error)
    publish(true)
  }

  function applyEdit(value: string): void {
    if (model.launcherMode === 'files') {
      applyFileEdit(value)
      return
    }
    model.shownNotice = undefined
    model.query = value
    model.queryControlValue = value
    model.querySequence += 1
    searchToken = ++token
    model.searchPending = false
    model.status = ''
    clearResults()
    if (value !== '') scheduleSearch()
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

  function shown(payload: unknown): void {
    if (destroyed) return
    const event = parseLauncherShown(payload)
    if (!event) return
    if (event.notice === 'settingsFailed') model.settingsUncertain = true
    if (composition) restoreControl(composition.control)
    composition = undefined
    leaveFileMode()
    model.viewEpoch += 1
    model.invocationId = event.invocationId
    model.view = event.target
    pluginInventoryActive = false
    model.queryControlValue = model.query
    model.querySequence = 0
    cancelSlashSearch()
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    clearResults()
    model.shownNotice = event.notice === null ? undefined : NOTICE_TEXT[event.notice]
    if (event.target === 'launcher') pendingSettingsLoadEpoch = undefined
    else queueSettingsLoad()
    if (event.target === 'launcher' && event.notice === null && activationNoticePending) {
      activationNoticePending = false
      model.shownNotice = REFUSED_NOTICE
    }
    if (event.target === 'launcher' && model.query !== '') {
      model.querySequence = 1
      scheduleSearch()
    }
    publish(true)
    if (event.target === 'settings') {
      void drainSettingsLoad()
    }
  }

  function text(record: ClassifiedTextRecord): void {
    if (destroyed) return
    const queryControl = record.control === model.queryControl
    if (!queryControl && !findTextControl(record.control)) return
    if (!queryControl && !settingsEditable()) return
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
        (queryControl &&
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
      if (queryControl) {
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
    model.theme = 'system'
    model.shownNotice = undefined
    publish(true)
    await persistSettings(operation, {
      hotkey: 'Shift+Space',
      autostart: false,
      theme: 'system',
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

  function executeSelection(): void {
    if (model.view !== 'launcher' || model.executePending) return
    let resultId: string | undefined
    if (model.launcherMode === 'applications') {
      const selected = model.results[model.selectedIndex]
      if (!selected) return
      if (selected.kind === 'find') {
        submitFind(selected.query)
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
      model.view !== 'launcher' ||
      model.launcherMode !== 'applications' ||
      !Number.isInteger(index) ||
      index < 0 ||
      index >= model.results.length
    ) return
    model.selectedIndex = index
    publish(true)
    executeSelection()
  }
  async function requestHide(): Promise<void> {
    if (destroyed || model.hidePending) return
    model.shownNotice = undefined
    model.status = ''
    model.hidePending = true
    leaveFileMode()
    const captured = { token: ++token, epoch: model.viewEpoch }
    hideToken = captured.token
    publish(true)
    try {
      await client.hideLauncher()
      if (destroyed || captured.token !== hideToken || captured.epoch !== model.viewEpoch) return
      model.hidePending = false
      publish(true)
    } catch (error) {
      if (destroyed || captured.token !== hideToken || captured.epoch !== model.viewEpoch) return
      model.hidePending = false
      model.status = errorText(error)
      publish(true)
    }
  }

  function keyDown(key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean): void {
    if (destroyed || isComposing) return
    if (key === 'Escape') {
      void requestHide()
      return
    }
    if (key === 'Enter') {
      const fileQuery = model.launcherMode === 'applications' ? fileCommand(model.query) : null
      if (model.view === 'launcher' && fileQuery !== null && model.queryControlValue === model.query) {
        submitFind(fileQuery)
        return
      }
      if (
        model.launcherMode === 'applications' &&
        model.view === 'launcher' &&
        !model.searchPending &&
        !model.executePending &&
        !model.results.length &&
        model.query !== '' &&
        model.queryControlValue === model.query
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
    if (model.launcherMode === 'files') {
      const file = model.file
      if (!file?.results.length) return
      model.shownNotice = undefined
      const offset = key === 'ArrowDown' ? 1 : -1
      const selectedIndex = (file.selectedIndex + offset + file.results.length) % file.results.length
      if (selectedIndex === file.selectedIndex) return
      file.selectedIndex = selectedIndex
      publish(true)
      return
    }
    if (!model.results.length) return
    model.shownNotice = undefined
    const offset = key === 'ArrowDown' ? 1 : -1
    model.selectedIndex = (model.selectedIndex + offset + model.results.length) % model.results.length
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
    let registered: (() => void) | undefined
    try {
      registered = await client.listenShown(shown)
    } catch {
      failInitialization()
      return
    }
    if (destroyed) {
      registered()
      return
    }
    unlisten = registered
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
    settingsOperation = undefined
    pendingSettingsLoadEpoch = undefined
    pluginListOwner = undefined
    pluginMutationOwners.clear()
    unlisten?.()
    unlisten = undefined
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

  return {
    client,
    getSnapshot,
    subscribe,
    start,
    failInitialization,
    shown,
    text,
    retireControl,
    keyDown,
    requestHide,
    activateResult,
    setAutostart,
    setThemePreference,
    setHotkeyCanonical,
    saveHotkeyCanonical,
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
