import {
  compareDecimalRevision,
  parseFileSearchResponse,
  parseFindForwardPayload,
  parseFindDocumentsAccess,
  parseFindPreviewPreferenceResult,
  parseFindReadyOutcome,
  parseFindThumbnailDataUrl,
  parseFindThemeChanged,
  type CommandErrorCode,
  type FileCategory,
  type FileIndexStatus,
  type FileResultItem,
  type FileResultView,
  type FileSort,
  type FindClient,
  type FindDocumentsAccess,
  type FindDocumentsOperation,
  type MacFindScope,
  type FindForwardPayload,
  type ThemePreference,
  type U64Decimal,
} from './protocol'

export const FIND_CATEGORY_ORDER: readonly FileCategory[] = [
  'all', 'folder', 'excel', 'word', 'ppt', 'pdf', 'image', 'video', 'audio', 'archive',
]

interface DocumentsControl {
  access?: FindDocumentsAccess
  pending?: FindDocumentsOperation
  notice?: string
  storageError?: boolean
}

export interface FindSnapshot {
  documents?: Readonly<DocumentsControl>
  ready: boolean
  theme: ThemePreference
  invocationId?: string
  query: string
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  previewPending: boolean
  thumbnailDataUrl?: string
  thumbnailPending: boolean
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
  readonly macosScope?: MacFindScope
  updateDocuments(operation: FindDocumentsOperation, remember?: boolean): Promise<void>
  openDocument(): void
  getSnapshot(): FindSnapshot
  subscribe(listener: () => void): () => void
  start(): Promise<void>
  acceptForward(payload: unknown): void
  acceptTheme(payload: unknown): void
  setQuery(query: string): void
  submitSearch(): void
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
  documents?: DocumentsControl
  ready: boolean
  theme: ThemePreference
  invocationId?: string
  query: string
  category: FileCategory
  sort: FileSort
  previewEnabled: boolean
  previewPending: boolean
  thumbnailDataUrl?: string
  thumbnailPending: boolean
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
  fileSearchStarting: '文件搜索正在准备',
  searchUnavailable: '文件搜索服务不可用',
  fileNotFound: '文件已不存在。',
  fileOpenFailed: '无法在资源管理器中打开。',
  windowFailed: '窗口操作失败。',
}
const FALLBACK_ERROR = '操作不可用，请重试。'
const PREVIEW_ERROR = '无法保存文件预览设置。'
const READY_ERROR = '文件搜索暂不可用。'
const PREVIEW_IMAGE_EXTENSION = /\.(?:bmp|gif|heic|jpe?g|png|svg|tiff?|webp)$/iu

function errorText(value: unknown, macos = false): string {
  if (typeof value !== 'object' || value === null) return FALLBACK_ERROR
  const code = (value as { code?: unknown }).code
  if (macos && code === 'fileOpenFailed') return '无法打开此项目，请重新搜索。'
  if (macos && code === 'fileActionUnconfirmed') return '已向 macOS 提交请求，请检查结果后再重新搜索。'
  return typeof code === 'string' ? ERROR_TEXT[code as CommandErrorCode] ?? FALLBACK_ERROR : FALLBACK_ERROR
}

function statusText(status: FileIndexStatus, hasResults: boolean, macos: boolean): string {
  if (macos && status === 'partial') return '仅显示部分结果。'
  if (macos && !hasResults && status === 'ready') return '当前范围内没有匹配的 Spotlight 结果。'
  if (status === 'building') return '正在索引。'
  if (status === 'partial') return '部分位置无法访问。'
  if (status === 'rebuilding') return '索引正在重建。'
  if (status === 'unavailable') return '搜索暂不可用。'
  return hasResults ? '' : '未找到文件'
}

export function createFindCore(client: FindClient, maximumQuerySequence = Number.MAX_SAFE_INTEGER): FindCore {
  const documents = client.macos?.scope === 'public-preview' ? client.macos.documents : undefined
  const model: Model = {
    documents: documents ? {} : undefined,
    ready: false,
    theme: 'system',
    query: '',
    category: 'all',
    sort: 'modifiedDesc',
    previewEnabled: true,
    previewPending: false,
    thumbnailPending: false,
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
  let thumbnailOwner = 0
  let pinOwner = 0
  let executeOwner = 0
  let hideOwner = 0
  let documentsOwner = 0
  let documentsReadFailed = false

  function project(): FindSnapshot {
    return Object.freeze({
      documents: model.documents && Object.freeze({ ...model.documents }),
      ready: model.ready,
      theme: model.theme,
      ...(model.invocationId ? { invocationId: model.invocationId } : {}),
      query: model.query,
      category: model.category,
      sort: model.sort,
      previewEnabled: model.previewEnabled,
      previewPending: model.previewPending,
      ...(model.thumbnailDataUrl ? { thumbnailDataUrl: model.thumbnailDataUrl } : {}),
      thumbnailPending: model.thumbnailPending,
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
    clearThumbnail()
    model.requestId = undefined
    model.results = []
    model.selectedIndex = -1
    model.total = '0'
  }

  function clearThumbnail(): void {
    thumbnailOwner = ++operationToken
    model.thumbnailDataUrl = undefined
    model.thumbnailPending = false
  }

  function beginThumbnail(): void {
    clearThumbnail()
    const invocationId = model.invocationId
    const requestId = model.requestId
    const selected = model.results[model.selectedIndex]
    if (
      client.macos || !model.previewEnabled || !model.ready || !invocationId || !requestId || !selected ||
      selected.view.kind !== 'file' || !PREVIEW_IMAGE_EXTENSION.test(selected.view.name)
    ) {
      publish()
      return
    }
    const owner = ++operationToken
    thumbnailOwner = owner
    const resultId = selected.resultId
    model.thumbnailPending = true
    publish()
    let pending
    try {
      pending = client.loadThumbnail({ requestId, resultId })
    } catch (error) {
      pending = Promise.reject(error)
    }
    const owns = () => !destroyed && owner === thumbnailOwner && invocationId === model.invocationId &&
      requestId === model.requestId && resultId === model.results[model.selectedIndex]?.resultId
    void pending.then(
      (raw) => {
        if (!owns()) return
        model.thumbnailPending = false
        model.thumbnailDataUrl = parseFindThumbnailDataUrl(raw) ?? undefined
        publish()
      },
      () => {
        if (!owns()) return
        model.thumbnailPending = false
        model.thumbnailDataUrl = undefined
        publish()
      },
    )
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
    if (!model.ready || !invocationId || !model.query || model.executePending || model.documents?.pending === 'revoke') return
    // On first use the host may restore a bookmark and retire the old scope.
    // Keep typing responsive and issue the latest query once restoration finishes.
    if (model.documents?.pending === 'get' && !model.documents.access) return
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
          if (raw === null && model.documents?.access?.status === 'granted') void updateDocuments('get')
          return
        }
        model.requestId = response.requestId
        model.total = response.total
        model.indexStatus = response.status
        model.results = response.items.map((item: FileResultItem) => ({
          resultId: item.resultId,
          view: {
            key: item.fullPath,
            ...(client.macos && item.canOpenDocument ? { canOpenDocument: true } : {}),
            name: item.name,
            kind: item.kind,
            sizeBytes: item.sizeBytes,
            modifiedUtc: item.modifiedUtc,
            fullPath: item.fullPath,
          },
        }))
        model.selectedIndex = model.results.length ? 0 : -1
        model.status = statusText(response.status, model.results.length > 0, Boolean(client.macos))
        publish()
        beginThumbnail()
      },
      (error: unknown) => {
        if (destroyed || owner !== searchOwner || invocationId !== model.invocationId ||
            query !== model.query || category !== model.category || sequence !== querySequence) return
        model.searchPending = false
        model.indexStatus = 'unavailable'
        model.status = errorText(error, Boolean(client.macos))
        publish()
        checkAccessError(error)
      },
    )
  }

  function refreshSearchScope(): void {
    searchOwner = ++operationToken
    model.searchPending = false
    clearResults()
    if (model.query && !model.executePending) beginSearch()
    else publish()
  }

  async function updateDocuments(operation: FindDocumentsOperation, remember = true): Promise<void> {
    const invocationId = model.invocationId
    const control = model.documents
    if (!documents || !control || !invocationId || !model.ready || destroyed ||
        control.pending || model.hidePending || model.executePending) return
    if (operation === 'grant' && (!control.access || control.access.status === 'granted')) return
    if (operation === 'revoke' && !control.storageError && (!control.access || control.access.status === 'notGranted')) return
    const owner = ++documentsOwner
    const current = () => !destroyed && model.invocationId === invocationId && owner === documentsOwner
    model.documents = { access: control.access, pending: operation }
    if (operation === 'revoke') {
      searchOwner = ++operationToken
      model.searchPending = false
      clearResults()
    }
    publish()

    const read = async (action: FindDocumentsOperation): Promise<FindDocumentsAccess> => {
      const reply = action === 'grant' ? await documents.grant({ invocationId, remember })
        : await documents[action]({ invocationId })
      const next = parseFindDocumentsAccess(reply)
      if (!next || (control.access && compareDecimalRevision(next.revision, control.access.revision) < 0)) {
        throw new Error('Invalid Documents access revision')
      }
      return next
    }

    let next: FindDocumentsAccess
    let notice: string | undefined
    const firstRead = operation === 'get' && !control.access && !documentsReadFailed
    let refresh = operation === 'revoke' || documentsReadFailed || firstRead
    try {
      next = await read(operation)
    } catch (error) {
      if (!current()) return
      if (operation === 'get') {
        documentsReadFailed = true
        model.documents = {
          storageError: (error as {code?: unknown} | null)?.code === 'documentsAccessStorageFailed',
          notice: '无法读取文稿授权，可重试。',
        }
        if (control.access?.status === 'granted') {
          searchOwner = ++operationToken
          clearResults()
          model.searchPending = false
        }
        publish()
        if (firstRead && model.query) beginSearch()
        return
      }
      // A lost mutation reply may follow a successful native change. Retire old
      // results and reconcile the grant before showing the scope again.
      searchOwner = ++operationToken
      clearResults()
      model.searchPending = false
      model.documents = { pending: 'get' }
      publish()
      const code = (error as { code?: unknown } | null)?.code
      if (code === 'documentsAccessStorageFailed') notice = operation === 'revoke'
        ? '未能确认已忘记文稿授权，请重试。' : '无法保存文稿授权，请重试。'
      else if (code === 'documentsRememberedAccessExists') notice = '已保留原文稿授权。如要改为仅本次搜索，请先忘记旧授权。'
      else notice = code === 'documentsAccessDenied'
        ? '无法授权文稿。请在系统选择框选择当前用户的“文稿”文件夹。' : '未能确认文稿授权，请检查后重试。'
      try {
        next = await read('get')
      } catch (readError) {
        if (!current()) return
        documentsReadFailed = true
        model.documents = {
          storageError: code === 'documentsAccessStorageFailed' ||
            (readError as {code?: unknown} | null)?.code === 'documentsAccessStorageFailed',
          notice,
        }
        publish()
        return
      }
      refresh = true
    }
    if (!current()) return
    const changed = control.access && (next.revision !== control.access.revision || next.status !== control.access.status)
    if (!notice && operation === 'grant') {
      if (!changed) notice = '已取消选择，搜索范围未更改。'
      else if (next.persistence === 'remembered') notice = '已记住文稿授权，下次启动会自动恢复。'
      else if (next.status === 'granted') notice = remember
        ? '文稿仅限本次搜索，下次启动需重新授权。' : '本次搜索已包含文稿。'
    }
    if (!notice && operation === 'revoke') notice = next.status === 'notGranted'
      ? '已停止搜索文稿并忘记授权。' : '文稿搜索范围未更改。'
    documentsReadFailed = false
    model.documents = { access: next, notice }
    if (refresh || changed) refreshSearchScope()
    else publish()
  }

  function checkAccessError(error: unknown): void {
    const code = (error as {code?: unknown} | null)?.code
    if (code === 'staleRequest' || code === 'fileOpenFailed') void updateDocuments('get')
  }

  function acceptForward(value: unknown): void {
    if (destroyed) return
    const payload = parseFindForwardPayload(value, Boolean(client.macos))
    if (!payload || (lastForward && compareDecimalRevision(payload.forwardSequence, lastForward) <= 0)) return
    lastForward = payload.forwardSequence
    documentsOwner++
    documentsReadFailed = false
    if (documents) model.documents = {}
    model.invocationId = payload.invocationId
    model.query = payload.query
    querySequence = 0
    if (payload.pinned !== undefined) {
      pinOwner = ++operationToken
      model.pinPending = false
      model.pinned = payload.pinned
    }
    searchOwner = ++operationToken
    model.searchPending = false
    model.executePending = false
    model.hidePending = false
    model.status = ''
    clearResults()
    publish()
    void updateDocuments('get')
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
      if (destroyed) {
        unlistenTheme()
        unlistenTheme = undefined
        return
      }
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
      if (destroyed) return
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
      if (destroyed) return
      if (!ready) {
        try {
          const status = parseFindReadyOutcome(await client.getReadyStatus({ initializationToken }))
          ready = status?.status === 'ready' && status.initializationToken === initializationToken
          if (status?.status === 'superseded') continue
        } catch {
          // Retry the same commit before preparing a replacement.
        }
      }
      if (destroyed) return
      if (!ready) {
        try {
          const retried = parseFindReadyOutcome(await client.commitReady({ initializationToken }))
          ready = retried?.status === 'ready' && retried.initializationToken === initializationToken
        } catch {
          ready = false
        }
      }
      if (destroyed) return
      if (ready) {
        model.ready = true
        model.status = ''
        publish()
        void updateDocuments('get')
        if (model.query) beginSearch()
        return
      }
      await new Promise<void>((resolve) => setTimeout(resolve, 0))
    }
  }

  function setQuery(query: string): void {
    if (!model.ready || !model.invocationId || model.executePending || query === model.query) return
    model.query = query
    if (model.documents?.notice) model.documents = { ...model.documents, notice: undefined }
    searchOwner = ++operationToken
    model.searchPending = false
    model.status = ''
    clearResults()
    publish()
    if (query) beginSearch()
    else if (client.macos) {
      const invocationId = model.invocationId
      const sequence = nextSequence()
      if (sequence !== null) void client.macos.cancelQuery({ invocationId, querySequence: sequence }).catch((error) => {
        if (destroyed || model.invocationId !== invocationId || querySequence !== sequence || model.query) return
        model.status = errorText(error, true)
        publish()
      })
    }
  }

  function submitSearch(): void {
    if (destroyed || !model.ready || !model.invocationId || model.executePending || model.searchPending) return
    if (model.query) beginSearch()
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
    if (!enabled) clearThumbnail()
    publish()
    if (enabled) beginThumbnail()
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
        model.status = errorText(error, Boolean(client.macos))
        publish()
      },
    )
  }

  function select(index: number): void {
    if (!model.ready || model.executePending || index < 0 || index >= model.results.length || index === model.selectedIndex) return
    model.selectedIndex = index
    beginThumbnail()
  }

  function retireMacResultSet(): void {
    model.requestId = undefined
    for (const row of model.results) delete row.view.canOpenDocument
  }

  function executeSelection(openDocument = false): void {
    const invocationId = model.invocationId
    const selected = model.results[model.selectedIndex]
    const requestId = model.requestId
    if (!model.ready || !invocationId || !selected || model.executePending ||
        model.documents?.pending === 'grant' || model.documents?.pending === 'revoke') return
    if (!requestId) {
      model.status = '请重新搜索后再执行操作。'
      publish()
      return
    }
    if (openDocument && (!client.macos || !selected.view.canOpenDocument)) return
    const owner = ++operationToken
    executeOwner = owner
    model.executePending = true
    model.status = ''
    publish()
    const input = { requestId, resultId: selected.resultId }
    const pending = openDocument && client.macos ? client.macos.openDocument(input) : client.executeResult(input)
    void pending.then(
      (outcome) => {
        if (destroyed || owner !== executeOwner || invocationId !== model.invocationId) return
        model.executePending = false
        if (client.macos) {
          retireMacResultSet()
          model.status = outcome.status === 'fileRevealRequested' ? '已请求在 Finder 中显示。' : '已提交打开请求。'
        }
        publish()
      },
      (error: unknown) => {
        if (destroyed || owner !== executeOwner || invocationId !== model.invocationId) return
        model.executePending = false
        if (client.macos && (error as {code?: string} | null)?.code === 'fileActionUnconfirmed') retireMacResultSet()
        model.status = errorText(error, Boolean(client.macos))
        publish()
        checkAccessError(error)
      },
    )
  }

  async function requestHide(force: boolean): Promise<void> {
    const invocationId = model.invocationId
    if (!model.ready || !invocationId || model.executePending || model.hidePending || model.documents?.pending === 'grant' || (model.pinned && !force)) return
    const owner = ++operationToken
    hideOwner = owner
    model.hidePending = true
    model.status = ''
    publish()
    try {
      await client.hide({ invocationId, force })
      if (destroyed || owner !== hideOwner || invocationId !== model.invocationId) return
      model.hidePending = false
      documentsOwner++
      if (documents) model.documents = {}
      if (force) {
        pinOwner = ++operationToken
        model.pinPending = false
        model.pinned = false
      }
      publish()
    } catch (error) {
      if (destroyed || owner !== hideOwner || invocationId !== model.invocationId) return
      model.hidePending = false
      model.status = errorText(error, Boolean(client.macos))
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
    beginThumbnail()
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    searchOwner = ++operationToken
    previewOwner = ++operationToken
    thumbnailOwner = ++operationToken
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
    macosScope: client.macos?.scope,
    updateDocuments,
    openDocument: () => executeSelection(true),
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    start,
    acceptForward,
    acceptTheme,
    setQuery,
    submitSearch,
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
