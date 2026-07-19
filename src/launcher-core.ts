import {
  parseLauncherShown,
  type ClassifiedTextRecord,
  type CommandErrorCode,
  type ControlKey,
  type ExecuteOutcome,
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

interface Model {
  view: 'launcher' | 'settings'
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
  clearConfirmation: boolean
}

interface CompositionOwner {
  control: ControlKey
  viewEpoch: number
  invocationId?: string
  generation: number
}

interface FinalizationOwner extends CompositionOwner {
  value?: string
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

type SettingsOperationKind = 'load' | 'save' | 'rescan' | 'export' | 'clear'

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
}

const NOTICE_TEXT = {
  settingsFailed: '快捷键或开机启动设置可能未完全应用，请重启 UiPilot 后检查设置。',
  validationFailed: '本地验证数据操作失败。',
} as const

const REFUSED_NOTICE = 'Windows 拒绝了前台切换，已发送启动请求'
const FALLBACK_ERROR = '操作不可用，请重试。'
const ERROR_CODES = new Set(Object.keys(ERROR_TEXT))

function errorText(value: unknown): string {
  if (typeof value !== 'object' || value === null || !Object.prototype.hasOwnProperty.call(value, 'code')) return FALLBACK_ERROR
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' && ERROR_CODES.has(code) ? ERROR_TEXT[code as CommandErrorCode] : FALLBACK_ERROR
}

function projectSnapshot(model: Model): LauncherSnapshot {
  const results = Object.freeze(
    model.results.map(({ key, title, subtitle }) => Object.freeze(subtitle === undefined ? { key, title } : { key, title, subtitle })),
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
  })
}

export function createLauncherCore(client: LauncherClient): LauncherCore {
  const model: Model = {
    view: 'launcher',
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
  let unlisten: (() => void) | undefined
  let token = 0
  let searchToken = 0
  let executeToken = 0
  let hideToken = 0
  let resultKey = 1
  let controlKey = 2
  let activationNoticePending = false
  let compositionGeneration = 0
  let composition: CompositionOwner | undefined
  let suppression: FinalizationOwner | undefined
  let tombstone: FinalizationOwner | undefined
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

  function replaceSettings(view: SettingsView): void {
    if (model.settings) {
      for (const control of settingsControls(model.settings)) retireControl(control.key)
    }
    appIds.clear()
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
      applyEdit(value)
      return
    }
    const field = findTextControl(control)
    if (!field || model.settingsNeedsReload || settingsOperation) return
    const changed = field.value !== value || field.draft !== value
    field.value = value
    field.draft = value
    publish(changed)
  }

  function settingsEditable(): boolean {
    return model.settings !== undefined && !model.settingsNeedsReload && settingsOperation === undefined
  }

  function clearResults(): void {
    model.requestId = undefined
    model.results = []
    model.selectedIndex = -1
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
      model.results = response.items.map((item: ResultItem) => ({
        key: resultKey++,
        resultId: item.resultId,
        title: item.title,
        ...(item.subtitle === undefined ? {} : { subtitle: item.subtitle }),
      }))
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
    tombstone = composition ? { ...composition } : undefined
    composition = undefined
    suppression = undefined
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
      composition = undefined
      suppression = undefined
      tombstone = undefined
      commitControl(record.control, record.value)
      return
    }
    if (record.kind === 'compositionStart') {
      const visibleMutation =
        getControlDraft(record.control) !== record.value ||
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
      }
      suppression = undefined
      tombstone = undefined
      setControlDraft(record.control, record.value)
      if (queryControl) {
        searchToken = ++token
        model.searchPending = false
        model.status = ''
        clearResults()
      }
      publish(visibleMutation)
      return
    }
    if (record.kind === 'compositionUpdate' || record.kind === 'compositionInput') {
      if (ownsComposition(composition, record.control)) {
        publish(setControlDraft(record.control, record.value))
        return
      }
      if (
        record.kind === 'compositionInput' &&
        suppression?.control === record.control &&
        suppression.generation === compositionGeneration &&
        suppression.value === record.value
      ) {
        suppression = undefined
        return
      }
      if (record.kind === 'compositionInput' && tombstone?.control === record.control && tombstone.value === record.value) {
        tombstone = undefined
        publish(restoreControl(record.control))
      }
      return
    }
    if (ownsComposition(composition, record.control)) {
      const owner = composition
      composition = undefined
      suppression = { ...owner, value: record.value }
      commitControl(record.control, record.value)
      return
    }
    if (tombstone?.control === record.control) {
      tombstone.value = record.value
      publish(restoreControl(record.control))
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
    if (composition?.control === control) composition = undefined
    if (suppression?.control === control) suppression = undefined
    if (tombstone?.control === control) tombstone = undefined
  }

  function setAutostart(checked: boolean): void {
    if (!settingsEditable() || model.settings!.autostart === checked) return
    model.settings!.autostart = checked
    publish(true)
  }

  function addAlias(application: ControlKey): void {
    if (!settingsEditable()) return
    const target = model.settings!.applications.find((candidate) => candidate.key === application)
    if (!target) return
    target.aliases.push(newTextControl(''))
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
    try {
      const view = await client.loadSettings()
      if (!ownsSettingsOperation(operation)) return
      if (!ownsSettingsView(operation)) {
        releaseSettingsOperation(operation)
        publish(true)
        return
      }
      replaceSettings(view)
      releaseSettingsOperation(operation)
      model.status = ''
      publish(true)
    } catch (error) {
      if (!ownsSettingsOperation(operation)) return
      const current = ownsSettingsView(operation)
      releaseSettingsOperation(operation)
      if (current) model.status = errorText(error)
      publish(true)
    }
  }

  async function reloadSettings(): Promise<void> {
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
    model.status = ''
    publish(true)
  }

  function cancelClearValidation(): void {
    if (!model.clearConfirmation || settingsOperation) return
    model.clearConfirmation = false
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
    const selected = model.results[model.selectedIndex]
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
      executeSelection()
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
    try {
      const settings = await client.loadSettings()
      if (!destroyed) {
        replaceSettings(settings)
        publish(true)
      }
    } catch (error) {
      if (!destroyed) {
        model.status = errorText(error)
        publish(true)
      }
    }
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    searchToken = ++token
    executeToken = ++token
    hideToken = ++token
    settingsOperation = undefined
    unlisten?.()
    unlisten = undefined
    listeners.clear()
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
