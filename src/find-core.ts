import {
  compareDecimalRevision,
  parseFileSearchResponse,
  parseFindForwardPayload,
  parseFindPreviewPreferenceResult,
  parseFindReadyOutcome,
  parseFindThemeChanged,
  type CommandErrorCode,
  type FileCategory,
  type FileIndexStatus,
  type FileResultItem,
  type FileResultView,
  type FileSort,
  type FindClient,
  type FindForwardPayload,
  type ThemePreference,
  type U64Decimal,
} from './protocol'

export const FIND_CATEGORY_ORDER: readonly FileCategory[] = [
  'all', 'folder', 'excel', 'word', 'ppt', 'pdf', 'image', 'video', 'audio', 'archive',
]

export interface FindSnapshot {
  ready: boolean
  theme: ThemePreference
  invocationId?: string
  query: string
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  previewPending: boolean
  pinned: boolean
  pinPending: boolean
  total: string
  indexStatus: FileIndexStatus
  results: readonly FileResultView[]
  selectedIndex: number
  searchPending: boolean
  executePending: boolean
  hidePending: boolean
  status: string
}

export interface FindCore {
  getSnapshot(): FindSnapshot
  subscribe(listener: () => void): () => void
  start(): Promise<void>
  acceptForward(payload: unknown): void
  acceptTheme(payload: unknown): void
  setQuery(query: string): void
  setCategory(category: FileCategory): void
  cycleCategory(direction: 'next' | 'previous'): void
  setPreviewEnabled(enabled: boolean): void
  setPinned(pinned: boolean): void
  select(index: number): void
  keyDown(key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean): void
  requestHide(force: boolean): Promise<void>
  destroy(): void
}

interface PrivateResult {
  resultId: string
  view: FileResultView
}

interface Model {
  ready: boolean
  theme: ThemePreference
  invocationId?: string
  query: string
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  previewPending: boolean
  pinned: boolean
  pinPending: boolean
  total: string
  indexStatus: FileIndexStatus
  results: PrivateResult[]
  selectedIndex: number
  requestId?: string
  searchPending: boolean
  executePending: boolean
  hidePending: boolean
  status: string
}

const ERROR_TEXT: Partial<Record<CommandErrorCode, string>> = {
  staleRequest: '搜索结果已过期，请重新搜索。',
  unknownResult: '搜索结果已过期，请重新搜索。',
  invalidFileQuery: '查询无效。',
  fileSearchWorkerFailed: '搜索暂不可用。',
  searchUnavailable: '搜索暂不可用。',
  fileNotFound: '文件已不存在。',
  fileOpenFailed: '无法在资源管理器中打开。',
  windowFailed: '窗口操作失败。',
}
const FALLBACK_ERROR = '操作不可用，请重试。'
const PREVIEW_ERROR = '无法保存文件预览设置。'
const READY_ERROR = '文件搜索暂不可用。'

function errorText(value: unknown): string {
  if (typeof value !== 'object' || value === null) return FALLBACK_ERROR
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' ? ERROR_TEXT[code as CommandErrorCode] ?? FALLBACK_ERROR : FALLBACK_ERROR
}

function statusText(status: FileIndexStatus, hasResults: boolean): string {
  if (status === 'building') return '正在索引。'
  if (status === 'partial') return '部分位置无法访问。'
  if (status === 'rebuilding') return '索引正在重建。'
  if (status === 'unavailable') return '搜索暂不可用。'
  return hasResults ? '' : '未找到文件'
}

export function createFindCore(client: FindClient, maximumQuerySequence = Number.MAX_SAFE_INTEGER): FindCore {
  const model: Model = {
    ready: false,
    theme: 'system',
    query: '',
    category: 'all',
    sort: 'modifiedDesc',
    previewEnabled: true,
    previewPending: false,
    pinned: false,
    pinPending: false,
    total: '0',
    indexStatus: 'building',
    results: [],
    selectedIndex: -1,
    searchPending: false,
    executePending: false,
    hidePending: false,
    status: '',
  }
  const listeners = new Set<() => void>()
  let snapshot = project()
  let destroyed = false
  let started = false
  let unlistenForward: (() => void) | undefined
  let unlistenTheme: (() => void) | undefined
  let lastForward: U64Decimal | undefined
  let themeRevision: U64Decimal | undefined
  let previewRevision: U64Decimal | undefined
  let durablePreview = true
  let querySequence = 0
  let operationToken = 0
  let searchOwner = 0
  let previewOwner = 0
  let pinOwner = 0
  let executeOwner = 0
  let hideOwner = 0

  function project(): FindSnapshot {
    return Object.freeze({
      ready: model.ready,
      theme: model.theme,
      ...(model.invocationId ? { invocationId: model.invocationId } : {}),
      query: model.query,
      category: model.category,
      sort: model.sort,
      previewEnabled: model.previewEnabled,
      previewPending: model.previewPending,
      pinned: model.pinned,
      pinPending: model.pinPending,
      total: model.total,
      indexStatus: model.indexStatus,
      results: Object.freeze(model.results.map(({ view }) => Object.freeze({ ...view }))),
      selectedIndex: model.selectedIndex,
      searchPending: model.searchPending,
      executePending: model.executePending,
      hidePending: model.hidePending,
      status: model.status,
    })
  }

  function publish(): void {
    snapshot = project()
    for (const listener of [...listeners]) listener()
  }

  function clearResults(): void {
    model.requestId = undefined
    model.results = []
    model.selectedIndex = -1
    model.total = '0'
  }

  function invocationClosed(): void {
    model.invocationId = undefined
    model.searchPending = false
    clearResults()
    model.status = '查询次数已达上限，请重新打开文件搜索。'
    publish()
  }

  function nextSequence(): number | null {
    if (querySequence >= maximumQuerySequence) {
      searchOwner = ++operationToken
      invocationClosed()
      return null
    }
    querySequence += 1
    return querySequence
  }

  function beginSearch(): void {
    const invocationId = model.invocationId
    if (!model.ready || !invocationId || !model.query || model.executePending) return
    const sequence = nextSequence()
    if (sequence === null) return
    const owner = ++operationToken
    searchOwner = owner
    const query = model.query
    const category = model.category
    const sort = model.sort
    model.searchPending = true
    model.status = ''
    clearResults()
    publish()
    let pending
    try {
      pending = client.searchFiles({ query, category, sort, invocationId, querySequence: sequence })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (raw) => {
        if (destroyed || owner !== searchOwner || invocationId !== model.invocationId ||
            query !== model.query || category !== model.category || sequence !== querySequence) return
        model.searchPending = false
        const response = raw === null ? null : parseFileSearchResponse(raw)
        if (!response) {
          publish()
          return
        }
        model.requestId = response.requestId
        model.total = response.total
        model.indexStatus = response.status
        model.results = response.items.map((item: FileResultItem) => ({
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
        model.selectedIndex = model.results.length ? 0 : -1
        model.status = statusText(response.status, model.results.length > 0)
        publish()
      },
      (error: unknown) => {
        if (destroyed || owner !== searchOwner || invocationId !== model.invocationId ||
            query !== model.query || category !== model.category || sequence !== querySequence) return
        model.searchPending = false
        model.indexStatus = 'unavailable'
        model.status = errorText(error)
        publish()
      },
    )
  }

  function acceptForward(value: unknown): void {
    if (destroyed) return
    const payload = parseFindForwardPayload(value)
    if (!payload || (lastForward && compareDecimalRevision(payload.forwardSequence, lastForward) <= 0)) return
    lastForward = payload.forwardSequence
    model.invocationId = payload.invocationId
    model.query = payload.query
    querySequence = 0
    searchOwner = ++operationToken
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    clearResults()
    publish()
    if (payload.query) beginSearch()
  }

  function acceptTheme(value: unknown): void {
    if (destroyed) return
    const event = parseFindThemeChanged(value)
    if (!event || (themeRevision && compareDecimalRevision(event.themeRevision, themeRevision) <= 0)) return
    themeRevision = event.themeRevision
    model.theme = event.theme
    publish()
  }

  function reconcileInitialization(initialization: import('./protocol').FindInitializationPrepared): void {
    if (!themeRevision || compareDecimalRevision(initialization.themeRevision, themeRevision) > 0) {
      themeRevision = initialization.themeRevision
      model.theme = initialization.theme
    }
    if (!previewRevision || compareDecimalRevision(initialization.filePreviewRevision, previewRevision) > 0) {
      previewRevision = initialization.filePreviewRevision
      durablePreview = initialization.filePreviewEnabled
      if (!model.previewPending) model.previewEnabled = initialization.filePreviewEnabled
    }
    model.pinned = initialization.pinned
  }

  async function start(): Promise<void> {
    if (started || destroyed) return
    started = true
    try {
      unlistenForward = await client.listenForward(acceptForward)
      if (destroyed) {
        unlistenForward()
        unlistenForward = undefined
        return
      }
      unlistenTheme = await client.listenThemeChanged(acceptTheme)
    } catch {
      unlistenTheme?.()
      unlistenForward?.()
      unlistenTheme = undefined
      unlistenForward = undefined
      model.status = READY_ERROR
      publish()
      return
    }
    while (!destroyed && !model.ready) {
      let prepared
      try {
        prepared = parseFindReadyOutcome(await client.prepareInitialization())
      } catch {
        prepared = null
      }
      if (!prepared || prepared.status !== 'prepared') {
        await new Promise<void>((resolve) => setTimeout(resolve, 0))
        continue
      }
      reconcileInitialization(prepared.initialization)
      publish()
      const initializationToken = prepared.initialization.initializationToken
      let ready = false
      try {
        const committed = parseFindReadyOutcome(await client.commitReady({ initializationToken }))
        ready = committed?.status === 'ready' && committed.initializationToken === initializationToken
        if (committed?.status === 'superseded') continue
      } catch {
        // The commit response may be lost after the backend committed it.
      }
      if (!ready) {
        try {
          const status = parseFindReadyOutcome(await client.getReadyStatus({ initializationToken }))
          ready = status?.status === 'ready' && status.initializationToken === initializationToken
          if (status?.status === 'superseded') continue
        } catch {
          // Retry the same commit before preparing a replacement.
        }
      }
      if (!ready) {
        try {
          const retried = parseFindReadyOutcome(await client.commitReady({ initializationToken }))
          ready = retried?.status === 'ready' && retried.initializationToken === initializationToken
        } catch {
          ready = false
        }
      }
      if (ready) {
        model.ready = true
        model.status = ''
        publish()
        if (model.query) beginSearch()
        return
      }
      await new Promise<void>((resolve) => setTimeout(resolve, 0))
    }
  }

  function setQuery(query: string): void {
    if (!model.ready || !model.invocationId || model.executePending || query === model.query) return
    model.query = query
    searchOwner = ++operationToken
    model.searchPending = false
    model.status = ''
    clearResults()
    publish()
    if (query) beginSearch()
  }

  function setCategory(category: FileCategory): void {
    if (!model.ready || !model.invocationId || model.executePending || category === model.category) return
    model.category = category
    searchOwner = ++operationToken
    model.searchPending = false
    model.status = ''
    clearResults()
    publish()
    if (model.query) beginSearch()
  }

  function cycleCategory(direction: 'next' | 'previous'): void {
    const index = FIND_CATEGORY_ORDER.indexOf(model.category)
    const offset = direction === 'next' ? 1 : -1
    setCategory(FIND_CATEGORY_ORDER[(index + offset + FIND_CATEGORY_ORDER.length) % FIND_CATEGORY_ORDER.length]!)
  }

  function setPreviewEnabled(enabled: boolean): void {
    if (!model.ready || !model.invocationId || model.executePending || model.previewPending || enabled === model.previewEnabled) return
    const owner = ++operationToken
    previewOwner = owner
    model.previewEnabled = enabled
    model.previewPending = true
    model.status = ''
    publish()
    let pending
    try {
      pending = client.setPreviewPreference({ preference: { enabled } })
    } catch (error) {
      pending = Promise.reject(error)
    }
    void pending.then(
      (raw) => {
        if (destroyed || owner !== previewOwner) return
        const result = parseFindPreviewPreferenceResult(raw)
        model.previewPending = false
        if (!result || (previewRevision && compareDecimalRevision(result.filePreviewRevision, previewRevision) < 0)) {
          model.previewEnabled = durablePreview
          model.status = PREVIEW_ERROR
          publish()
          return
        }
        previewRevision = result.filePreviewRevision
        durablePreview = result.filePreviewEnabled
        model.previewEnabled = result.filePreviewEnabled
        publish()
      },
      () => {
        if (destroyed || owner !== previewOwner) return
        model.previewPending = false
        model.previewEnabled = durablePreview
        model.status = PREVIEW_ERROR
        publish()
      },
    )
  }

  function setPinned(pinned: boolean): void {
    const invocationId = model.invocationId
    if (!model.ready || !invocationId || model.executePending || model.pinPending || pinned === model.pinned) return
    const owner = ++operationToken
    const previousPinned = model.pinned
    pinOwner = owner
    model.pinned = pinned
    model.pinPending = true
    model.status = ''
    publish()
    void client.setPinned({ invocationId, pinned }).then(
      (result) => {
        if (destroyed || owner !== pinOwner || invocationId !== model.invocationId) return
        model.pinPending = false
        model.pinned = result.pinned
        publish()
      },
      (error: unknown) => {
        if (destroyed || owner !== pinOwner || invocationId !== model.invocationId) return
        model.pinPending = false
        model.pinned = previousPinned
        model.status = errorText(error)
        publish()
      },
    )
  }

  function select(index: number): void {
    if (!model.ready || model.executePending || index < 0 || index >= model.results.length || index === model.selectedIndex) return
    model.selectedIndex = index
    publish()
  }

  function executeSelection(): void {
    const invocationId = model.invocationId
    const selected = model.results[model.selectedIndex]
    const requestId = model.requestId
    if (!model.ready || !invocationId || !selected || !requestId || model.executePending) return
    const owner = ++operationToken
    executeOwner = owner
    model.executePending = true
    model.status = ''
    publish()
    void client.executeResult({ requestId, resultId: selected.resultId }).then(
      () => {
        if (destroyed || owner !== executeOwner || invocationId !== model.invocationId) return
        model.executePending = false
        publish()
      },
      (error: unknown) => {
        if (destroyed || owner !== executeOwner || invocationId !== model.invocationId) return
        model.executePending = false
        model.status = errorText(error)
        publish()
      },
    )
  }

  async function requestHide(force: boolean): Promise<void> {
    const invocationId = model.invocationId
    if (!model.ready || !invocationId || model.executePending || model.hidePending || (model.pinned && !force)) return
    const owner = ++operationToken
    hideOwner = owner
    model.hidePending = true
    model.status = ''
    publish()
    try {
      await client.hide({ invocationId, force })
      if (destroyed || owner !== hideOwner || invocationId !== model.invocationId) return
      model.hidePending = false
      if (force) {
        pinOwner = ++operationToken
        model.pinPending = false
        model.pinned = false
      }
      publish()
    } catch (error) {
      if (destroyed || owner !== hideOwner || invocationId !== model.invocationId) return
      model.hidePending = false
      model.status = errorText(error)
      publish()
    }
  }

  function keyDown(key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing: boolean): void {
    if (!model.ready || model.executePending || isComposing) return
    if (key === 'Escape') {
      if (!model.pinned) void requestHide(false)
      return
    }
    if (key === 'Enter') {
      executeSelection()
      return
    }
    if (!model.results.length) return
    const offset = key === 'ArrowDown' ? 1 : -1
    model.selectedIndex = (model.selectedIndex + offset + model.results.length) % model.results.length
    publish()
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    searchOwner = ++operationToken
    previewOwner = ++operationToken
    pinOwner = ++operationToken
    executeOwner = ++operationToken
    hideOwner = ++operationToken
    unlistenTheme?.()
    unlistenForward?.()
    unlistenTheme = undefined
    unlistenForward = undefined
    listeners.clear()
  }

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    start,
    acceptForward,
    acceptTheme,
    setQuery,
    setCategory,
    cycleCategory,
    setPreviewEnabled,
    setPinned,
    select,
    keyDown,
    requestHide,
    destroy,
  }
}
