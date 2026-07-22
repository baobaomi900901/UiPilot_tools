import {
  parseFileIndexChanged,
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
  type LauncherClient,
  type LauncherSnapshot,
  type ResultItem,
  type SettingsView,
  type UserSettingsUpdate,
  type ViewResult,
} from './protocol'

export interface LauncherCore {
  readonly getSnapshot: () => LauncherSnapshot
  readonly subscribe: (listener: () => void) => () => void
  readonly start: () => Promise<void>
  readonly failInitialization: () => void
  readonly shown: (payload: unknown) => void
  readonly text: (record: ClassifiedTextRecord) => void
  readonly retireControl: (control: ControlKey) => void
  readonly keyDown: (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean) => void
  readonly requestHide: () => Promise<void>
  readonly setAutostart: (checked: boolean) => void
  readonly setHotkeyCanonical: (value: string) => void
  readonly saveHotkeyCanonical: (value: string) => Promise<void>
  readonly setFileCategory: (category: FileCategory) => void
  readonly setFileSort: (sort: FileSort) => void
  readonly setFilePreviewEnabled: (enabled: boolean) => void
  readonly addAlias: (application: ControlKey) => void
  readonly removeAlias: (application: ControlKey, alias: ControlKey) => void
  readonly saveSettings: () => Promise<void>
  readonly reloadSettings: () => Promise<void>
  readonly rescanApps: () => Promise<void>
  readonly exportValidation: () => Promise<void>
  readonly beginClearValidation: () => void
  readonly cancelClearValidation: () => void
  readonly confirmClearValidation: () => Promise<void>
  readonly destroy: () => void
}

interface PrivateResult extends ViewResult {
  resultId: string
}

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
  settingsNeedsReload: boolean
  settingsLoadError?: string
  clearConfirmation: boolean
  file?: PrivateFileState
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

interface PrivateApplication {
  key: ControlKey
  displayName: string
  aliases: TextControl[]
}

interface PrivateSettings {
  hotkey: TextControl
  researchId: TextControl
  autostart: boolean
  applications: PrivateApplication[]
}

interface FileSearchOwner {
  token: number
  epoch: number
  invocationId: string
  sequence: number
  query: string
  category: FileCategory
  sort: FileSort
  requiredRevision: bigint
}

interface PreviewPreferenceOwner {
  token: number
  enabled: boolean
}

type SettingsOperationKind = 'load' | 'save' | 'hotkey' | 'rescan' | 'export' | 'clear'

interface SettingsOperation {
  token: number
  kind: SettingsOperationKind
  viewEpoch: number
  view: 'launcher' | 'settings'
}

const ERROR_TEXT: Record<CommandErrorCode, string> = {
  invalidCaller: '操作不可用，请重试。',
  staleRequest: '搜索结果已过期，请重新搜索。',
  unknownResult: '搜索结果已过期，请重新搜索。',
  applicationEntryUnavailable: '应用入口不可用，请重新扫描。',
  settingsFailed: '设置未能确认完成；若快捷键或开机启动行为异常，请重启 UiPilot 后检查设置。',
  validationFailed: '验证数据操作失败。',
  windowFailed: '窗口操作失败。',
  scanFailed: '重新扫描失败。',
  scanWorkerFailed: '重新扫描失败。',
  mainThreadDispatchFailed: '导出失败。',
  exportFailed: '导出失败。',
  exportWorkerFailed: '导出失败。',
  invalidFileQuery: '查询无效。',
  fileSearchWorkerFailed: '搜索暂不可用。',
  searchUnavailable: '搜索暂不可用。',
  fileNotFound: '文件已不存在。',
  fileOpenFailed: '无法在资源管理器中打开。',
  clipboardWriteFailed: '无法复制到剪贴板。',
  pluginPermissionDenied: '插件无权写入剪贴板。',
}

const NOTICE_TEXT = {
  settingsFailed: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
  validationFailed: '本地验证数据操作失败。',
} as const

const REFUSED_NOTICE = 'Windows 拒绝了前台切换，已发送启动请求'
const FALLBACK_ERROR = '操作不可用，请重试。'
const FILE_PREVIEW_ERROR = '无法保存文件预览设置。'
const ERROR_CODES = new Set(Object.keys(ERROR_TEXT))
const ICON_PREFIX = 'data:image/png;base64,'
const MAX_ICON_LENGTH = 65_536
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
    model.results.map(({ key, title, subtitle, icon }) =>
      Object.freeze({
        key,
        title,
        ...(subtitle === undefined ? {} : { subtitle }),
        ...(icon === undefined ? {} : { icon }),
      }),
    ),
  )
  const settings = model.settings
    ? Object.freeze({
        hotkey: Object.freeze({ key: model.settings.hotkey.key, value: model.settings.hotkey.draft }),
        researchId: Object.freeze({ key: model.settings.researchId.key, value: model.settings.researchId.draft }),
        autostart: model.settings.autostart,
        applications: Object.freeze(
          model.settings.applications.map((application) =>
            Object.freeze({
              key: application.key,
              displayName: application.displayName,
              aliases: Object.freeze(application.aliases.map((alias) => Object.freeze({ key: alias.key, value: alias.draft }))),
            }),
          ),
        ),
        readOnly: model.settingsNeedsReload,
        ...(model.settingsOperation === undefined ? {} : { operation: model.settingsOperation }),
        clearConfirmation: model.clearConfirmation,
        needsReload: model.settingsNeedsReload,
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
  return Object.freeze({
    view: model.view,
    viewEpoch: model.viewEpoch,
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
    status: model.status,
    ...(settings === undefined ? {} : { settings }),
    ...(file === undefined ? {} : { file }),
  })
}

export function createLauncherCore(client: LauncherClient, maximumQuerySequence = Number.MAX_SAFE_INTEGER): LauncherCore {
  const model: Model = {
    view: 'launcher',
    launcherMode: 'applications',
    viewEpoch: 0,
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
    settingsNeedsReload: false,
    clearConfirmation: false,
  }
  const listeners = new Set<() => void>()
  let snapshot = projectSnapshot(model)
  let destroyed = false
  let started = false
  let startupSettingsPending = false
  let unlisten: (() => void) | undefined
  let fileUnlisten: (() => void) | undefined
  let fileListenerRegistration: Promise<boolean> | undefined
  let fileListenerToken = 0
  let fileRefreshTimer: ReturnType<typeof setTimeout> | undefined
  let fileRefreshMaxTimer: ReturnType<typeof setTimeout> | undefined
  let fileRefreshRequired = 0n
  let previewPreferenceToken = 0
  let previewPreferencePending: PreviewPreferenceOwner | undefined
  let previewPreferenceDurableGeneration = 0
  let lastLoadedFilePreviewEnabled = true
  let token = 0
  let searchToken = 0
  let executeToken = 0
  let hideToken = 0
  let resultKey = 1
  let controlKey = 2
  let activationNoticePending = false
  let compositionGeneration = 0
  let composition: CompositionOwner | undefined
  let settingsOperation: SettingsOperation | undefined
  const appIds = new Map<ControlKey, string>()

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
    return [settings.hotkey, settings.researchId, ...settings.applications.flatMap((application) => application.aliases)]
  }

  function replaceSettings(view: SettingsView, previewGeneration: number): void {
    if (model.settings) {
      for (const control of settingsControls(model.settings)) retireControl(control.key)
    }
    appIds.clear()
    if (previewGeneration === previewPreferenceDurableGeneration) {
      lastLoadedFilePreviewEnabled = view.filePreviewEnabled
    }
    const totals = new Map<string, number>()
    for (const application of view.applications) totals.set(application.displayName, (totals.get(application.displayName) ?? 0) + 1)
    const seen = new Map<string, number>()
    const applications = view.applications.map((application) => {
      const key = controlKey++
      appIds.set(key, application.appId)
      const ordinal = (seen.get(application.displayName) ?? 0) + 1
      seen.set(application.displayName, ordinal)
      return {
        key,
        displayName: totals.get(application.displayName) === 1 ? application.displayName : `${application.displayName} (${ordinal})`,
        aliases: (application.aliases.length ? application.aliases : ['']).map(newTextControl),
      }
    })
    model.settings = {
      hotkey: newTextControl(view.hotkey),
      researchId: newTextControl(view.researchId ?? ''),
      autostart: view.autostart,
      applications,
    }
    model.settingsNeedsReload = false
    model.settingsLoadError = undefined
    model.clearConfirmation = false
  }

  function findTextControl(control: ControlKey): TextControl | undefined {
    if (!model.settings) return undefined
    if (model.settings.hotkey.key === control) return model.settings.hotkey
    if (model.settings.researchId.key === control) return model.settings.researchId
    for (const application of model.settings.applications) {
      const alias = application.aliases.find((candidate) => candidate.key === control)
      if (alias) return alias
    }
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
    if (!field || model.settingsNeedsReload || settingsOperation) return
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
    return model.settings !== undefined && !model.settingsNeedsReload && settingsOperation === undefined
  }

  function clearResults(): void {
    model.requestId = undefined
    model.results = []
    model.selectedIndex = -1
  }

  function clearFileRefreshTimers(): void {
    if (fileRefreshTimer !== undefined) clearTimeout(fileRefreshTimer)
    if (fileRefreshMaxTimer !== undefined) clearTimeout(fileRefreshMaxTimer)
    fileRefreshTimer = undefined
    fileRefreshMaxTimer = undefined
    fileRefreshRequired = 0n
  }

  function leaveFileMode(): void {
    if (model.launcherMode !== 'files') return
    clearFileRefreshTimers()
    searchToken = ++token
    model.searchPending = false
    model.launcherMode = 'applications'
    model.file = undefined
    model.query = ''
    model.queryControlValue = ''
    if (!fileUnlisten && fileListenerRegistration) {
      fileListenerToken += 1
      fileListenerRegistration = undefined
    }
  }

  function fileCommand(value: string): string | null {
    if (value === '/find') return ''
    return value.startsWith('/find ') ? value.slice(6) : null
  }

  function fileStatusText(status: FileIndexStatus): string {
    if (status === 'building') return '索引正在建立或校准。'
    if (status === 'partial') return '部分位置无法访问。'
    if (status === 'rebuilding') return '索引正在重建。'
    if (status === 'unavailable') return '搜索暂不可用。'
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

  async function ensureFileListener(): Promise<boolean> {
    if (fileUnlisten) return true
    if (fileListenerRegistration) return fileListenerRegistration
    const owner = ++fileListenerToken
    let registration: Promise<boolean>
    try {
      registration = client.listenFileIndexChanged(fileIndexChanged).then(
        (release) => {
          if (destroyed || owner !== fileListenerToken) {
            release()
            return false
          }
          fileUnlisten = release
          return true
        },
        () => false,
      )
    } catch {
      return false
    }
    fileListenerRegistration = registration
    const result = await registration
    if (fileListenerRegistration === registration) fileListenerRegistration = undefined
    return result
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

  function beginFileSearch(requiredRevision: bigint): void {
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
      requiredRevision,
    }
    searchToken = owner.token
    model.searchPending = true
    publish(true)
    let pending: Promise<FileSearchResponse | null>
    try {
      pending = client.searchFiles({
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
    if (revision < owner.requiredRevision || revision < file.latestSeenRevision) {
      model.searchPending = false
      publish(true)
      return
    }
    if (revision >= fileRefreshRequired) clearFileRefreshTimers()
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
    model.status = fileStatusText(response.status)
    publish(true)
  }

  function failFileSearch(owner: FileSearchOwner, error: unknown): void {
    if (!ownsFileSearch(owner)) return
    model.searchPending = false
    model.status = errorText(error)
    publish(true)
  }

  async function enterFileMode(query: string): Promise<void> {
    const epoch = model.viewEpoch
    const invocationId = model.invocationId
    if (!invocationId) return
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
    const listening = await ensureFileListener()
    if (
      !listening ||
      destroyed ||
      epoch !== model.viewEpoch ||
      invocationId !== model.invocationId ||
      model.launcherMode !== 'files'
    ) {
      if (!listening && model.launcherMode === 'files' && epoch === model.viewEpoch) {
        model.status = '搜索暂不可用。'
        publish(true)
      }
      return
    }
    if (!nextFileSequence()) return
    beginFileSearch(0n)
  }

  function applyFileEdit(value: string): void {
    const file = model.file
    if (!file) return
    clearFileRefreshTimers()
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
    if (!fileUnlisten) {
      publish(true)
      return
    }
    if (!nextFileSequence()) return
    beginFileSearch(file.latestSeenRevision)
  }

  function runFileRefresh(): void {
    const file = model.file
    const required = fileRefreshRequired
    clearFileRefreshTimers()
    if (!file || required === 0n || !nextFileSequence()) return
    beginFileSearch(required)
  }

  function scheduleFileRefresh(required: bigint): void {
    fileRefreshRequired = required > fileRefreshRequired ? required : fileRefreshRequired
    if (fileRefreshTimer !== undefined) clearTimeout(fileRefreshTimer)
    fileRefreshTimer = setTimeout(runFileRefresh, 250)
    fileRefreshMaxTimer ??= setTimeout(runFileRefresh, 1_000)
  }

  function fileIndexChanged(payload: unknown): void {
    const event = parseFileIndexChanged(payload)
    const file = model.file
    if (!event || !file || model.launcherMode !== 'files') return
    const revision = BigInt(event.revision)
    if (revision <= file.latestSeenRevision) return
    const statusChanged = file.indexStatus !== event.status
    file.latestSeenRevision = revision
    file.indexStatus = event.status
    if (statusChanged) {
      model.status = fileStatusText(event.status)
      publish(true)
    }
    scheduleFileRefresh(revision)
  }

  function beginSearch(): void {
    const invocationId = model.invocationId
    if (!invocationId || model.query === '') return
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
      pending = client.searchApps({ query: captured.query, invocationId, querySequence: captured.sequence })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (response) => finishSearch(captured, response),
      (error: unknown) => failSearch(captured, error),
    )
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
      model.results = response.items.map((item: ResultItem) => {
        const icon = safeApplicationIcon(item.icon)
        return {
          key: resultKey++,
          resultId: item.resultId,
          title: item.title,
          ...(item.subtitle === undefined ? {} : { subtitle: item.subtitle }),
          ...(icon === undefined ? {} : { icon }),
        }
      })
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
    if (value !== '') beginSearch()
    publish(true)
  }

  function shown(payload: unknown): void {
    if (destroyed) return
    const event = parseLauncherShown(payload)
    if (!event) return
    if (composition) restoreControl(composition.control)
    composition = undefined
    leaveFileMode()
    model.viewEpoch += 1
    model.invocationId = event.invocationId
    model.view = event.target
    model.queryControlValue = model.query
    model.querySequence = 0
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    clearResults()
    model.shownNotice = event.notice === null ? undefined : NOTICE_TEXT[event.notice]
    if (event.target === 'settings' && event.notice === null && model.settingsLoadError) model.status = model.settingsLoadError
    if (event.target === 'launcher' && event.notice === null && activationNoticePending) {
      activationNoticePending = false
      model.shownNotice = REFUSED_NOTICE
    }
    if (event.target === 'launcher' && model.query !== '') {
      model.querySequence = 1
      beginSearch()
    }
    publish(true)
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
    model.settings!.autostart = checked
    model.shownNotice = undefined
    publish(true)
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

  function addAlias(application: ControlKey): void {
    if (!settingsEditable()) return
    const target = model.settings!.applications.find((candidate) => candidate.key === application)
    if (!target) return
    target.aliases.push(newTextControl(''))
    model.shownNotice = undefined
    publish(true)
  }

  function removeAlias(application: ControlKey, alias: ControlKey): void {
    if (!settingsEditable()) return
    const target = model.settings!.applications.find((candidate) => candidate.key === application)
    const index = target?.aliases.findIndex((candidate) => candidate.key === alias) ?? -1
    if (!target || index < 0) return
    retireControl(alias)
    target.aliases.splice(index, 1)
    if (!target.aliases.length) target.aliases.push(newTextControl(''))
    model.shownNotice = undefined
    publish(true)
  }

  function startSettingsOperation(kind: SettingsOperationKind): SettingsOperation | undefined {
    if (destroyed || settingsOperation || (kind !== 'load' && !model.settings)) return undefined
    const operation = { token: ++token, kind, viewEpoch: model.viewEpoch, view: model.view }
    settingsOperation = operation
    model.settingsOperation = kind
    model.clearConfirmation = false
    model.shownNotice = undefined
    model.status = ''
    publish(true)
    return operation
  }

  function ownsSettingsOperation(operation: SettingsOperation): boolean {
    return !destroyed && settingsOperation?.token === operation.token
  }

  function ownsSettingsView(operation: SettingsOperation): boolean {
    return ownsSettingsOperation(operation) && operation.viewEpoch === model.viewEpoch && operation.view === model.view
  }

  function releaseSettingsOperation(operation: SettingsOperation): void {
    if (settingsOperation?.token !== operation.token) return
    settingsOperation = undefined
    model.settingsOperation = undefined
  }

  function settingsUpdate(): UserSettingsUpdate {
    const settings = model.settings!
    const aliases: Record<string, string[]> = {}
    for (const application of settings.applications) {
      const appId = appIds.get(application.key)
      if (!appId) continue
      aliases[appId] = application.aliases.map((alias) => alias.value).filter((value) => value !== '')
    }
    return {
      hotkey: settings.hotkey.value,
      autostart: settings.autostart,
      ...(settings.researchId.value === '' ? {} : { researchId: settings.researchId.value }),
      aliases,
    }
  }

  async function finishSettingsLoad(operation: SettingsOperation): Promise<void> {
    model.settingsLoadError = undefined
    const previewGeneration = previewPreferenceDurableGeneration
    try {
      const view = await client.loadSettings()
      if (!ownsSettingsOperation(operation)) return
      if (!ownsSettingsView(operation)) {
        releaseSettingsOperation(operation)
        publish(true)
        return
      }
      replaceSettings(view, previewGeneration)
      releaseSettingsOperation(operation)
      model.status = ''
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) {
        model.settingsLoadError = errorText(error)
        model.status = model.settingsLoadError
      }
      publish(true)
    }
  }

  async function reloadSettings(): Promise<void> {
    if (startupSettingsPending) return
    const operation = startSettingsOperation('load')
    if (!operation) return
    await finishSettingsLoad(operation)
  }

  async function reloadAfterMutation(operation: SettingsOperation): Promise<void> {
    if (!ownsSettingsOperation(operation)) return
    if (!ownsSettingsView(operation)) {
      model.settingsNeedsReload = true
      releaseSettingsOperation(operation)
      publish(true)
      return
    }
    model.settingsNeedsReload = true
    publish(true)
    await finishSettingsLoad(operation)
  }

  async function saveSettings(): Promise<void> {
    if (!settingsEditable()) return
    const update = settingsUpdate()
    const operation = startSettingsOperation('save')
    if (!operation) return
    try {
      await client.saveSettings({ settings: update })
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      model.settingsNeedsReload = true
      releaseSettingsOperation(operation)
      if (current) model.status = errorText(error)
      publish(true)
      return
    }
    await reloadAfterMutation(operation)
  }

  async function saveHotkeyCanonical(value: string): Promise<void> {
    if (!settingsEditable() || !model.settings) return
    const settings = model.settings
    const previous = settings.hotkey.value
    const operation = startSettingsOperation('hotkey')
    if (!operation) return
    setControlDraft(settings.hotkey.key, value)
    settings.hotkey.value = value
    publish(true)
    try {
      const result = await client.saveHotkey({ hotkey: { hotkey: value } })
      if (!ownsSettingsOperation(operation)) return
      if (!ownsSettingsView(operation)) {
        model.settingsNeedsReload = true
        releaseSettingsOperation(operation)
        publish(true)
        return
      }
      setControlDraft(settings.hotkey.key, result.hotkey)
      settings.hotkey.value = result.hotkey
      releaseSettingsOperation(operation)
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) {
        model.settingsNeedsReload = true
        setControlDraft(settings.hotkey.key, previous)
        settings.hotkey.value = previous
        model.status = errorText(error)
      } else {
        model.settingsNeedsReload = true
      }
      publish(true)
    }
  }

  async function rescanApps(): Promise<void> {
    if (!settingsEditable()) return
    const operation = startSettingsOperation('rescan')
    if (!operation) return
    try {
      await client.rescanApps()
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = errorText(error)
      else model.settingsNeedsReload = true
      publish(true)
      return
    }
    await reloadAfterMutation(operation)
  }

  async function exportValidation(): Promise<void> {
    const operation = startSettingsOperation('export')
    if (!operation) return
    try {
      const outcome = await client.exportValidationData()
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = outcome.status === 'exported' ? '验证数据已导出。' : ''
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = errorText(error)
      publish(true)
    }
  }

  function beginClearValidation(): void {
    if (!model.settings || settingsOperation || model.clearConfirmation) return
    model.clearConfirmation = true
    model.shownNotice = undefined
    model.status = ''
    publish(true)
  }

  function cancelClearValidation(): void {
    if (!model.clearConfirmation || settingsOperation) return
    model.clearConfirmation = false
    model.shownNotice = undefined
    publish(true)
  }

  async function confirmClearValidation(): Promise<void> {
    if (!model.clearConfirmation) return
    const operation = startSettingsOperation('clear')
    if (!operation) return
    try {
      await client.clearValidationData()
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = '验证数据已清除。'
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = errorText(error)
      publish(true)
    }
  }

  function executeSelection(): void {
    if (model.view !== 'launcher' || model.executePending || !model.requestId) return
    const selected =
      model.launcherMode === 'files' ? model.file?.results[model.file.selectedIndex] : model.results[model.selectedIndex]
    if (!selected) return
    model.shownNotice = undefined
    model.status = ''
    model.executePending = true
    const captured = { token: ++token, epoch: model.viewEpoch, invocationId: model.invocationId }
    executeToken = captured.token
    const requestId = model.requestId
    publish(true)
    let pending: Promise<ExecuteOutcome>
    try {
      pending = client.executeResult({ requestId, resultId: selected.resultId })
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
        void enterFileMode(fileQuery)
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
        applyEdit(model.query)
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
    startupSettingsPending = true
    const previewGeneration = previewPreferenceDurableGeneration
    try {
      const settings = await client.loadSettings()
      if (!destroyed) {
        replaceSettings(settings, previewGeneration)
        publish(true)
      }
    } catch (error) {
      if (!destroyed) {
        model.settingsLoadError = errorText(error)
        model.status = model.settingsLoadError
        publish(true)
      }
    } finally {
      startupSettingsPending = false
    }
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    clearFileRefreshTimers()
    settingsOperation = undefined
    unlisten?.()
    unlisten = undefined
    fileListenerToken += 1
    fileUnlisten?.()
    fileUnlisten = undefined
    fileListenerRegistration = undefined
    listeners.clear()
  }

  function setFileCategory(category: FileCategory): void {
    const file = model.file
    if (model.launcherMode !== 'files' || !file || file.category === category) return
    clearFileRefreshTimers()
    file.category = category
    file.results = []
    file.selectedIndex = -1
    file.total = '0'
    searchToken = ++token
    model.searchPending = false
    if (!fileUnlisten) {
      publish(true)
      return
    }
    if (!nextFileSequence()) return
    beginFileSearch(file.latestSeenRevision)
  }

  function setFileSort(sort: FileSort): void {
    const file = model.file
    if (model.launcherMode !== 'files' || !file || file.sort === sort) return
    clearFileRefreshTimers()
    file.sort = sort
    file.results = []
    file.selectedIndex = -1
    file.total = '0'
    searchToken = ++token
    model.searchPending = false
    if (!fileUnlisten) {
      publish(true)
      return
    }
    if (!nextFileSequence()) return
    beginFileSearch(file.latestSeenRevision)
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
      pending = client.setFilePreviewPreference({ preference: { enabled } })
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
    getSnapshot,
    subscribe,
    start,
    failInitialization,
    shown,
    text,
    retireControl,
    keyDown,
    requestHide,
    setAutostart,
    setHotkeyCanonical,
    saveHotkeyCanonical,
    setFileCategory,
    setFileSort,
    setFilePreviewEnabled,
    addAlias,
    removeAlias,
    saveSettings,
    reloadSettings,
    rescanApps,
    exportValidation,
    beginClearValidation,
    cancelClearValidation,
    confirmClearValidation,
    destroy,
  }
}
