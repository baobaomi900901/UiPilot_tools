import {
  parseLauncherShown,
  type ClassifiedTextRecord,
  type CommandErrorCode,
  type ControlKey,
  type ExecuteOutcome,
  type LauncherClient,
  type LauncherSnapshot,
  type ResultItem,
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
  let activationNoticePending = false
  let compositionGeneration = 0
  let composition: CompositionOwner | undefined
  let suppression: FinalizationOwner | undefined
  let tombstone: FinalizationOwner | undefined

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
    if (destroyed || record.control !== model.queryControl) return
    if (record.kind === 'ordinaryInput') {
      composition = undefined
      suppression = undefined
      tombstone = undefined
      applyEdit(record.value)
      return
    }
    if (record.kind === 'compositionStart') {
      const visibleMutation =
        model.queryControlValue !== record.value ||
        model.searchPending ||
        model.requestId !== undefined ||
        model.results.length > 0 ||
        model.selectedIndex !== -1 ||
        model.status !== ''
      compositionGeneration += 1
      composition = {
        control: record.control,
        viewEpoch: model.viewEpoch,
        invocationId: model.invocationId,
        generation: compositionGeneration,
      }
      suppression = undefined
      tombstone = undefined
      searchToken = ++token
      model.searchPending = false
      model.queryControlValue = record.value
      model.status = ''
      clearResults()
      publish(visibleMutation)
      return
    }
    if (record.kind === 'compositionUpdate' || record.kind === 'compositionInput') {
      if (ownsComposition(composition, record.control)) {
        const changed = model.queryControlValue !== record.value
        model.queryControlValue = record.value
        publish(changed)
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
        const changed = model.queryControlValue !== model.query
        model.queryControlValue = model.query
        publish(changed)
      }
      return
    }
    if (ownsComposition(composition, record.control)) {
      const owner = composition
      composition = undefined
      suppression = { ...owner, value: record.value }
      applyEdit(record.value)
      return
    }
    if (tombstone?.control === record.control) {
      tombstone.value = record.value
      const changed = model.queryControlValue !== model.query
      model.queryControlValue = model.query
      publish(changed)
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
      await client.loadSettings()
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
    destroy,
  }
}
