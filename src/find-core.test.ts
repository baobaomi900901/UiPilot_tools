import { describe, expect, it, vi } from 'vitest'

import { createFindCore } from './find-core'
import type {
  ExecuteOutcome,
  FileSearchResponse,
  FindClient,
  FindInitializationPrepared,
  FindReadyOutcome,
} from './protocol'

function initialization(overrides: Partial<FindInitializationPrepared> = {}): FindInitializationPrepared {
  return {
    initializationToken: 'init-1',
    themeRevision: '1' as FindInitializationPrepared['themeRevision'],
    theme: 'system',
    filePreviewRevision: '1' as FindInitializationPrepared['filePreviewRevision'],
    filePreviewEnabled: true,
    pinned: false,
    ...overrides,
  }
}

function ready(prepared = initialization()): FindReadyOutcome {
  return { status: 'prepared', initialization: prepared }
}

function fileResponse(name = 'Report.txt', requestId = 'request-1'): FileSearchResponse {
  return {
    requestId,
    indexRevision: '1',
    total: '1',
    status: 'ready',
    items: [{
      resultId: `result-${name}`,
      name,
      kind: 'file',
      sizeBytes: '42',
      modifiedUtc: '2026-08-11T01:02:03Z',
      fullPath: `C:\\Private\\${name}`,
    }],
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function fakeClient() {
  const order: string[] = []
  let forward: ((payload: unknown) => void) | undefined
  let theme: ((payload: unknown) => void) | undefined
  const unlistenForward = vi.fn()
  const unlistenTheme = vi.fn()
  const loadThumbnail = vi.fn(async (_input: { requestId: string; resultId: string }): Promise<unknown> => null)
  const client = {
    listenForward: vi.fn(async (handler) => { order.push('forward-listener'); forward = handler; return unlistenForward }),
    listenThemeChanged: vi.fn(async (handler) => { order.push('theme-listener'); theme = handler; return unlistenTheme }),
    prepareInitialization: vi.fn(async () => { order.push('prepare'); return ready() }),
    commitReady: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    getReadyStatus: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    searchFiles: vi.fn(async () => null),
    loadThumbnail,
    executeResult: vi.fn(async () => ({ status: 'fileRevealRequested' }) satisfies ExecuteOutcome),
    setPinned: vi.fn(async ({ pinned }) => ({ pinned })),
    setPreviewPreference: vi.fn(async ({ preference }) => ({
      filePreviewRevision: '2',
      filePreviewEnabled: preference.enabled,
    })),
    hide: vi.fn(async () => undefined),
  } as unknown as FindClient
  return {
    client,
    loadThumbnail,
    order,
    emitForward(payload: unknown) { forward?.(payload) },
    emitTheme(payload: unknown) { theme?.(payload) },
    unlistenForward,
    unlistenTheme,
  }
}

describe('find readiness', () => {
  it('registers both listeners before readiness preparation', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    await core.start()
    expect(fake.order).toEqual(['forward-listener', 'theme-listener', 'prepare'])
    expect(core.getSnapshot().ready).toBe(true)
  })

  it('cleans a partial listener registration without contacting readiness', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.listenThemeChanged).mockRejectedValueOnce(new Error('listen failed'))
    const core = createFindCore(fake.client)
    await core.start()
    expect(fake.unlistenForward).toHaveBeenCalledOnce()
    expect(fake.client.prepareInitialization).not.toHaveBeenCalled()
    expect(core.getSnapshot().ready).toBe(false)
  })

  it('releases a theme listener registered after teardown and ignores a late readiness reply', async () => {
    const fake = fakeClient()
    const listen = deferred<() => void>()
    vi.mocked(fake.client.listenThemeChanged).mockReturnValueOnce(listen.promise)
    const core = createFindCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(fake.client.listenThemeChanged).toHaveBeenCalledOnce())
    core.destroy()
    listen.resolve(fake.unlistenTheme)
    await start
    expect(fake.unlistenTheme).toHaveBeenCalledOnce()
    expect(fake.client.prepareInitialization).not.toHaveBeenCalled()

    const readyLater = deferred<FindReadyOutcome>()
    vi.mocked(fake.client.commitReady).mockReturnValueOnce(readyLater.promise)
    const next = createFindCore(fake.client)
    const starting = next.start()
    await vi.waitFor(() => expect(fake.client.commitReady).toHaveBeenCalledOnce())
    fake.emitForward({ invocationId: 'old', forwardSequence: '1', query: 'report' })
    next.destroy()
    readyLater.resolve({ status: 'ready', initializationToken: 'init-1' })
    await starting
    expect(next.getSnapshot().ready).toBe(false)
    expect(fake.client.searchFiles).not.toHaveBeenCalled()
  })

  it('recovers a lost commit response through status without replacing listeners', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.commitReady).mockRejectedValueOnce(new Error('response lost'))
    const core = createFindCore(fake.client)
    await core.start()
    expect(fake.client.getReadyStatus).toHaveBeenCalledWith({ initializationToken: 'init-1' })
    expect(fake.client.listenForward).toHaveBeenCalledOnce()
    expect(core.getSnapshot().ready).toBe(true)
  })

  it('yields to a timer turn before retrying a failed preparation', async () => {
    vi.useFakeTimers()
    const fake = fakeClient()
    const first = deferred<unknown>()
    const retry = deferred<unknown>()
    vi.mocked(fake.client.prepareInitialization)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(retry.promise)
    const core = createFindCore(fake.client)
    const starting = core.start()
    try {
      await vi.waitFor(() => expect(fake.client.prepareInitialization).toHaveBeenCalledOnce())
      first.reject(new Error('prepare failed'))
      await first.promise.catch(() => undefined)
      await Promise.resolve()
      await Promise.resolve()
      expect(fake.client.prepareInitialization).toHaveBeenCalledOnce()
      await vi.advanceTimersByTimeAsync(0)
      expect(fake.client.prepareInitialization).toHaveBeenCalledTimes(2)
      retry.resolve(ready())
      await starting
      expect(core.getSnapshot().ready).toBe(true)
    } finally {
      core.destroy()
      retry.resolve(ready())
      vi.useRealTimers()
    }
  })

  it('searches a forward received before readiness as soon as ready is confirmed', async () => {
    const fake = fakeClient()
    const preparation = deferred<unknown>()
    vi.mocked(fake.client.prepareInitialization).mockReturnValueOnce(preparation.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse())
    const core = createFindCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.listenThemeChanged).toHaveBeenCalledOnce())

    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' })
    expect(core.getSnapshot()).toMatchObject({ ready: false, query: 'windows' })
    expect(fake.client.searchFiles).not.toHaveBeenCalled()

    preparation.resolve(ready())
    await starting

    expect(core.getSnapshot().ready).toBe(true)
    expect(fake.client.searchFiles).toHaveBeenCalledWith({
      query: 'windows', category: 'all', sort: 'modifiedDesc', invocationId: 'inv-1', querySequence: 1,
    })
  })

  it('reconciles theme event and initialization by independent revision', async () => {
    const fake = fakeClient()
    const preparation = deferred<unknown>()
    vi.mocked(fake.client.prepareInitialization).mockReturnValueOnce(preparation.promise)
    const core = createFindCore(fake.client)
    const starting = core.start()
    await vi.waitFor(() => expect(fake.client.listenThemeChanged).toHaveBeenCalledOnce())
    fake.emitTheme({ themeRevision: '4', theme: 'dark' })
    preparation.resolve(ready(initialization({ themeRevision: '3' as FindInitializationPrepared['themeRevision'], theme: 'light' })))
    await starting
    expect(core.getSnapshot().theme).toBe('dark')
  })
})

describe('find forwarding and query ownership', () => {
  it.each([
    ['fileSearchStarting', '文件搜索正在准备'],
    ['searchUnavailable', '文件搜索服务不可用'],
  ])('maps %s to fixed text without exposing backend diagnostics', async (code, expected) => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockRejectedValueOnce({ code, message: 'private runtime diagnostic' })
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: `inv-${code}`, forwardSequence: '1', query: 'report' })

    await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
    expect(core.getSnapshot().status).toBe(expected)
    expect(core.getSnapshot().status).not.toContain('diagnostic')
    core.destroy()
  })

  it('ignores malformed, duplicate, stale, and overflow forwards', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-2', forwardSequence: '2', query: '' })
    for (const payload of [
      { invocationId: 'inv-bad', forwardSequence: '02', query: 'bad' },
      { invocationId: 'inv-duplicate', forwardSequence: '2', query: 'duplicate' },
      { invocationId: 'inv-stale', forwardSequence: '1', query: 'stale' },
      { invocationId: 'inv-overflow', forwardSequence: '18446744073709551616', query: 'overflow' },
    ]) fake.emitForward(payload)
    expect(core.getSnapshot()).toMatchObject({ invocationId: 'inv-2', query: '' })
    expect(fake.client.searchFiles).not.toHaveBeenCalled()
  })

  it('preserves category, preview, and pin while a newer forward resets query sequence', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse())
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'first' })
    await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledOnce())
    core.setCategory('pdf')
    core.setPinned(true)
    await vi.waitFor(() => expect(core.getSnapshot().pinned).toBe(true))
    fake.emitForward({ invocationId: 'inv-2', forwardSequence: '2', query: 'second' })
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({
      invocationId: 'inv-2', query: 'second', category: 'pdf', querySequence: 1,
    }))
    expect(core.getSnapshot()).toMatchObject({ category: 'pdf', previewEnabled: true, pinned: true })
  })

  it('fails the invocation closed before emitting an unsafe query sequence', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client, 1)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'first' })
    core.setQuery('second')
    expect(fake.client.searchFiles).toHaveBeenCalledTimes(1)
    expect(core.getSnapshot().invocationId).toBeUndefined()
  })

  it('keeps a late stale search response from replacing the current result', async () => {
    const fake = fakeClient()
    const stale = deferred<FileSearchResponse | null>()
    const current = deferred<FileSearchResponse | null>()
    vi.mocked(fake.client.searchFiles).mockReturnValueOnce(stale.promise).mockReturnValueOnce(current.promise)
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'old' })
    core.setQuery('new')
    current.resolve(fileResponse('Current.txt', 'request-current'))
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('Current.txt'))
    stale.resolve(fileResponse('Stale.txt', 'request-stale'))
    await stale.promise
    expect(core.getSnapshot().results[0]?.name).toBe('Current.txt')
  })

  it.each([false, true])('loads only the current selected result thumbnail (Mac=%s)', async (mac) => {
    const fake = fakeClient()
    const first = deferred<unknown>()
    const second = deferred<unknown>()
    vi.mocked(fake.loadThumbnail)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
    const response = fileResponse('First.png')
    response.total = '2'
    response.items.push({
      ...response.items[0]!,
      resultId: 'result-Second.png',
      name: 'Second.png',
      fullPath: String.raw`C:\Private\Second.png`,
    })
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(response)
    const core = createFindCore({ ...fake.client, ...(mac ? { macos: { scope: 'public-preview' as const, openDocument: vi.fn(), cancelQuery: vi.fn() } } : {}) })
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'png' })
    await vi.waitFor(() => expect(fake.loadThumbnail).toHaveBeenCalledWith({
      requestId: 'request-1', resultId: 'result-First.png',
    }))

    core.select(1)
    expect(fake.loadThumbnail).toHaveBeenLastCalledWith({
      requestId: 'request-1', resultId: 'result-Second.png',
    })
    first.resolve('data:image/png;base64,RklSU1Q=')
    await first.promise
    await Promise.resolve()
    expect(core.getSnapshot()).not.toHaveProperty('thumbnailDataUrl')

    second.resolve('data:image/png;base64,U0VDT05E')
    await second.promise
    await vi.waitFor(() => expect(core.getSnapshot()).toMatchObject({
      thumbnailPending: false,
      thumbnailDataUrl: 'data:image/png;base64,U0VDT05E',
    }))
  })

  it('clears an image thumbnail when keyboard selection moves to a folder', async () => {
    const fake = fakeClient()
    vi.mocked(fake.loadThumbnail).mockResolvedValueOnce('data:image/png;base64,SU1BR0U=')
    const response = fileResponse('Hong-Kong.png')
    response.total = '2'
    response.items.push({
      ...response.items[0]!,
      resultId: 'result-folder',
      name: 'Hong-Kong',
      kind: 'folder',
      sizeBytes: null,
      fullPath: String.raw`C:\Private\Hong-Kong`,
    })
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(response)
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'Hong-Kong' })
    await vi.waitFor(() => expect(core.getSnapshot().thumbnailDataUrl).toBe('data:image/png;base64,SU1BR0U='))

    core.keyDown('ArrowDown', false)

    expect(core.getSnapshot()).toMatchObject({ selectedIndex: 1, thumbnailPending: false })
    expect(core.getSnapshot()).not.toHaveProperty('thumbnailDataUrl')
    expect(fake.loadThumbnail).toHaveBeenCalledTimes(1)
  })

  it.each(['preview', 'query', 'hide', 'destroy'] as const)('discards late Mac thumbnails after %s', async (change) => {
    const fake = fakeClient()
    const pending = deferred<unknown>()
    fake.loadThumbnail.mockReturnValueOnce(pending.promise)
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(fileResponse('图片🐱.jpg'))
    const core = createFindCore({ ...fake.client, macos: {
      scope: 'public-preview', openDocument: vi.fn(), cancelQuery: vi.fn(async () => {}),
    } })
    await core.start()
    fake.emitForward({ invocationId: 'mac', forwardSequence: '1', query: '图片' })
    await vi.waitFor(() => expect(fake.loadThumbnail).toHaveBeenCalledOnce())
    if (change === 'preview') core.setPreviewEnabled(false)
    if (change === 'query') core.setQuery('')
    if (change === 'hide') await core.requestHide(true)
    if (change === 'destroy') core.destroy()
    pending.resolve('data:image/png;base64,UE5H')
    await pending.promise
    await Promise.resolve()
    expect(core.getSnapshot().thumbnailDataUrl).toBeUndefined()
    core.destroy()
  })
})

describe('find execution and preferences', () => {
  it('shows the pinned state immediately while persistence is pending', async () => {
    const fake = fakeClient()
    const pending = deferred<{ pinned: boolean }>()
    vi.mocked(fake.client.setPinned).mockReturnValueOnce(pending.promise)
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' })

    core.setPinned(true)

    expect(core.getSnapshot()).toMatchObject({ pinned: true, pinPending: true })
    pending.resolve({ pinned: true })
    await vi.waitFor(() => expect(core.getSnapshot().pinPending).toBe(false))
  })

  it('does not issue a second hide after authenticated execution', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.searchFiles).mockResolvedValue(fileResponse())
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'report' })
    await vi.waitFor(() => expect(core.getSnapshot().selectedIndex).toBe(0))
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().executePending).toBe(false))
    expect(fake.client.executeResult).toHaveBeenCalledOnce()
    expect(fake.client.hide).not.toHaveBeenCalled()
  })

  it('rolls preview back after a current failure and ignores a late completion', async () => {
    const fake = fakeClient()
    const pending = deferred<unknown>()
    vi.mocked(fake.client.setPreviewPreference).mockReturnValueOnce(pending.promise)
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' })
    core.setPreviewEnabled(false)
    pending.reject(new Error('persist failed'))
    await vi.waitFor(() => expect(core.getSnapshot().previewPending).toBe(false))
    expect(core.getSnapshot().previewEnabled).toBe(true)
  })

  it('keeps pinned Escape inert and forces close through the explicit path', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    await core.start()
    fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' })
    core.setPinned(true)
    await vi.waitFor(() => expect(core.getSnapshot().pinned).toBe(true))
    core.keyDown('Escape', false)
    expect(fake.client.hide).not.toHaveBeenCalled()
    await core.requestHide(true)
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: true })
    expect(core.getSnapshot()).toMatchObject({ pinned: false, pinPending: false })
  })
})

describe('Mac Spotlight execution', () => {
  it('applies the new window pin snapshot when a reply from a previous invocation is late', async () => {
    const fake = fakeClient()
    const old = deferred<{ pinned: boolean }>()
    vi.mocked(fake.client.setPinned).mockReturnValueOnce(old.promise)
    const core = createFindCore({ ...fake.client, macos: { scope: 'public-preview', openDocument: vi.fn(), cancelQuery: vi.fn() } })
    await core.start()
    fake.emitForward({ invocationId: '1', forwardSequence: '1', query: '', pinned: false })
    core.setPinned(true)
    expect(core.getSnapshot().pinPending).toBe(true)
    fake.emitForward({ invocationId: '2', forwardSequence: '2', query: '', pinned: false })
    old.resolve({ pinned: true })
    await old.promise
    expect(core.getSnapshot()).toMatchObject({ invocationId: '2', pinned: false, pinPending: false })
    core.keyDown('Escape', false)
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: '2', force: false })
    core.destroy()
  })
  it('uses only host-marked documents, retires unconfirmed tickets, and separates empty and partial results', async () => {
    const fake = fakeClient()
    const openDocument = vi.fn(async (): Promise<ExecuteOutcome> => ({status: 'fileOpenRequested'}))
    const core = createFindCore({ ...fake.client, macos: { scope: 'public-preview', openDocument, cancelQuery: vi.fn(async () => {}) } })
    await core.start()
    const entry = fileResponse()
    entry.items.push({ ...entry.items[0], resultId: 'eligible', fullPath: '/approved/other.pdf', canOpenDocument: true })
    entry.total = '2'
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(entry)
    fake.emitForward({ invocationId: '1', forwardSequence: '1', query: 'Report' })
    await vi.waitFor(() => expect(core.getSnapshot().results.length).toBe(2))
    core.openDocument()
    expect(openDocument).not.toHaveBeenCalled()
    vi.mocked(fake.client.executeResult).mockRejectedValueOnce({ code: 'fileActionUnconfirmed', message: 'private' })
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().status).toContain('已向 macOS 提交请求'))
    core.select(1)
    core.openDocument()
    core.keyDown('Enter', false)
    expect(fake.client.executeResult).toHaveBeenCalledOnce()
    expect(openDocument).not.toHaveBeenCalled()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({ ...entry, status: 'partial' })
    core.submitSearch()
    await vi.waitFor(() => expect(core.getSnapshot().status).toBe('仅显示部分结果。'))
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({ ...entry, items: [], total: '0' })
    core.setQuery('无匹配')
    await vi.waitFor(() => expect(core.getSnapshot().status).toContain('当前范围内没有匹配'))
    core.destroy()
  })

  it('clearing a query cancels its sequence; late results or cancellation errors cannot replace new results', async () => {
    const fake = fakeClient()
    const old = deferred<FileSearchResponse | null>()
    const cancellation = deferred<void>()
    const cancelQuery = vi.fn(() => cancellation.promise)
    const core = createFindCore({ ...fake.client, macos: { scope: 'public-preview', cancelQuery, openDocument: vi.fn() } })
    vi.mocked(fake.client.searchFiles).mockReturnValueOnce(old.promise).mockResolvedValueOnce(fileResponse('New.txt'))
    await core.start()
    fake.emitForward({ invocationId: 'mac', forwardSequence: '1', query: 'old' })
    core.setQuery('')
    core.setQuery('New')
    expect(cancelQuery).toHaveBeenCalledExactlyOnceWith({ invocationId: 'mac', querySequence: 2 })
    old.resolve(fileResponse('Old.txt'))
    cancellation.reject({ code: 'staleRequest' })
    await vi.waitFor(() => expect(core.getSnapshot().results.map(r => r.name)).toEqual(['New.txt']))
    expect(core.getSnapshot().status).toBe('')
    core.destroy()
  })
})

describe('Documents access in the Mac public preview', () => {
  function setup() {
    const fake = fakeClient()
    const docs = {
      get: vi.fn(async () => ({ status: 'notGranted', persistence: 'none', revision: '0' })),
      grant: vi.fn(async () => ({ status: 'granted', persistence: 'remembered', revision: '1' })),
      revoke: vi.fn(async () => ({ status: 'notGranted', persistence: 'none', revision: '2' })),
    }
    const core = createFindCore({ ...fake.client, macos: {
      scope: 'public-preview', documents: docs, cancelQuery: vi.fn(), openDocument: vi.fn(),
    } })
    return { ...fake, docs, core }
  }

  it('removes pending thumbnail authority immediately on Documents revocation', async () => {
    const { core, client, docs, loadThumbnail, emitForward } = setup()
    docs.get.mockResolvedValue({ status: 'granted', persistence: 'remembered', revision: '1' })
    const pending = deferred<unknown>()
    loadThumbnail.mockReturnValueOnce(pending.promise)
    vi.mocked(client.searchFiles).mockResolvedValueOnce(fileResponse('private.png'))
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'private' })
    await vi.waitFor(() => expect(loadThumbnail).toHaveBeenCalledOnce())
    const revoking = core.updateDocuments('revoke')
    pending.resolve('data:image/png;base64,UE5H')
    await revoking
    expect(core.getSnapshot().thumbnailDataUrl).toBeUndefined()
    core.destroy()
  })

  it('keeps the current search after chooser cancellation, refreshes on authorization, and removes old rows on revoke', async () => {
    const { core, client, docs, emitForward } = setup()
    vi.mocked(client.searchFiles).mockResolvedValueOnce(fileResponse('public.ttf'))
      .mockResolvedValueOnce(fileResponse('document.pdf')).mockResolvedValueOnce(fileResponse('public.ttf'))
    await core.start()
    expect(docs.get).not.toHaveBeenCalled()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'example' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('public.ttf'))
    expect(docs.get).toHaveBeenCalledExactlyOnceWith({ invocationId: 'find' })
    expect(docs.grant).not.toHaveBeenCalled()
    docs.grant.mockResolvedValueOnce({ status: 'notGranted', persistence: 'none', revision: '0' })
    await core.updateDocuments('grant')
    expect(client.searchFiles).toHaveBeenCalledOnce()
    expect(core.getSnapshot().documents?.notice).toContain('未更改')
    await core.updateDocuments('grant')
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('document.pdf'))
    const pending = deferred<{status: string; revision: string; persistence: string}>()
    docs.revoke.mockReturnValueOnce(pending.promise)
    const revoking = core.updateDocuments('revoke')
    expect(core.getSnapshot().results).toEqual([])
    core.submitSearch()
    await core.updateDocuments('revoke')
    expect(docs.revoke).toHaveBeenCalledExactlyOnceWith({ invocationId: 'find' })
    pending.resolve({ status: 'notGranted', persistence: 'none', revision: '2' })
    await revoking
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('public.ttf'))
    expect(client.searchFiles).toHaveBeenCalledTimes(3)
    core.destroy()
  })

  it('reconciles a rejected grant, shows a fixed message, and ignores a completion superseded by another invocation', async () => {
    const { core, client, docs, emitForward } = setup()
    await core.start()
    emitForward({ invocationId: 'a', forwardSequence: '1', query: 'font' })
    await vi.waitFor(() => expect(core.getSnapshot().documents?.access?.status).toBe('notGranted'))
    docs.grant.mockRejectedValueOnce({ code: 'documentsAccessDenied', message: '/private/path' })
    await core.updateDocuments('grant')
    expect(core.getSnapshot().documents).toMatchObject({ access: { status: 'notGranted' } })
    expect(core.getSnapshot().documents?.notice).toContain('系统选择框')
    expect(JSON.stringify(core.getSnapshot())).not.toContain('/private/path')
    const pending = deferred<{status: string; revision: string; persistence: string}>()
    docs.grant.mockReturnValueOnce(pending.promise)
    const old = core.updateDocuments('grant')
    emitForward({ invocationId: 'b', forwardSequence: '2', query: 'new' })
    pending.resolve({ status: 'granted', persistence: 'remembered', revision: '1' })
    await old
    await vi.waitFor(() => expect(core.getSnapshot().documents?.access?.status).toBe('notGranted'))
    expect(client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ invocationId: 'b', querySequence: 1 }))
    core.destroy()
  })

  it('restores results once when a previously granted scope is readable after a failed status check', async () => {
    const { core, client, docs, emitForward } = setup()
    const allowed = { status: 'granted', persistence: 'remembered', revision: '1' }
    docs.get.mockResolvedValueOnce(allowed).mockRejectedValueOnce(new Error('unavailable')).mockResolvedValueOnce(allowed)
    vi.mocked(client.searchFiles).mockResolvedValue(fileResponse('document.pdf'))
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'report' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('document.pdf'))
    expect(client.searchFiles).toHaveBeenCalledOnce()
    await core.updateDocuments('get')
    expect(core.getSnapshot().documents?.access).toBeUndefined()
    expect(core.getSnapshot().results).toEqual([])
    await core.updateDocuments('get')
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('document.pdf'))
    expect(client.searchFiles).toHaveBeenCalledTimes(2)
    core.destroy()
  })

  it('restores remembered access before the first search and uses the latest typed query', async () => {
    const { core, client, docs, emitForward } = setup()
    const restored = deferred<{ status: string; revision: string; persistence: string }>()
    docs.get.mockReturnValueOnce(restored.promise)
    vi.mocked(client.searchFiles).mockResolvedValue(fileResponse('插画.png'))
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: '插' })
    core.setQuery('插画')
    expect(client.searchFiles).not.toHaveBeenCalled()
    restored.resolve({ status: 'granted', persistence: 'remembered', revision: '1' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('插画.png'))
    expect(client.searchFiles).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({query: '插画'}))
    expect(docs.grant).not.toHaveBeenCalled()
    core.destroy()
  })

  it('reports session-only fallback after consent and preserves a deliberate session choice', async () => {
    for (const remember of [true, false]) {
      const { core, docs, emitForward } = setup()
      await core.start()
      emitForward({ invocationId: 'find', forwardSequence: '1', query: '' })
      await vi.waitFor(() => expect(core.getSnapshot().documents?.access?.status).toBe('notGranted'))
      docs.grant.mockResolvedValueOnce({ status: 'granted', persistence: 'session', revision: '1' })
      await core.updateDocuments('grant', remember)
      expect(docs.grant).toHaveBeenCalledExactlyOnceWith({ invocationId: 'find', remember })
      expect(core.getSnapshot().documents?.notice).toContain(remember ? '下次启动需重新授权' : '本次搜索已包含文稿')
      core.destroy()
    }
  })

  it('allows forgetting a record even when its storage cannot be read', async () => {
    const { core, docs, emitForward } = setup()
    docs.get.mockRejectedValueOnce({ code: 'documentsAccessStorageFailed' })
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: '' })
    await vi.waitFor(() => expect(core.getSnapshot().documents?.storageError).toBe(true))
    await core.updateDocuments('revoke')
    expect(docs.revoke).toHaveBeenCalledExactlyOnceWith({ invocationId: 'find' })
    expect(core.getSnapshot().documents?.notice).toBe('已停止搜索文稿并忘记授权。')
    core.destroy()
  })

  it('keeps forget retryable when the host cannot confirm a durable removal', async () => {
    const { core, docs, emitForward } = setup()
    docs.get.mockResolvedValueOnce({ status: 'granted', persistence: 'remembered', revision: '1' })
      .mockResolvedValueOnce({ status: 'needsReauthorization', persistence: 'remembered', revision: '2' })
    docs.revoke.mockRejectedValueOnce({ code: 'documentsAccessStorageFailed' })
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: '' })
    await vi.waitFor(() => expect(core.getSnapshot().documents?.access?.persistence).toBe('remembered'))
    await core.updateDocuments('revoke')
    expect(core.getSnapshot().documents?.notice).toContain('未能确认已忘记')
    await core.updateDocuments('revoke')
    expect(docs.revoke).toHaveBeenCalledTimes(2)
    expect(core.getSnapshot().documents?.access?.status).toBe('notGranted')
    core.destroy()
  })

  it('searches public files even when the initial Documents state cannot be read', async () => {
    const { core, client, docs, emitForward } = setup()
    docs.get.mockRejectedValueOnce(new Error('unavailable'))
    vi.mocked(client.searchFiles).mockResolvedValue(fileResponse('public.ttf'))
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'font' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('public.ttf'))
    expect(client.searchFiles).toHaveBeenCalledOnce()
    expect(docs.grant).not.toHaveBeenCalled()
    expect(core.getSnapshot().documents?.access).toBeUndefined()
    core.destroy()
  })

  it('rechecks a granted scope if the host retires the current search without a response', async () => {
    const { core, client, docs, emitForward } = setup()
    docs.get.mockResolvedValueOnce({status: 'granted', persistence: 'remembered', revision: '1'})
      .mockResolvedValueOnce({status: 'needsReauthorization', persistence: 'none', revision: '2'})
    vi.mocked(client.searchFiles).mockResolvedValueOnce(fileResponse('private.pdf'))
      .mockResolvedValueOnce(null).mockResolvedValueOnce(fileResponse('public.ttf'))
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'old' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('private.pdf'))
    core.setQuery('new')
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('public.ttf'))
    expect(core.getSnapshot().documents?.access?.status).toBe('needsReauthorization')
    expect(docs.get).toHaveBeenCalledTimes(2)
    core.destroy()
  })

  it('refreshes access after a stale result and clears documents that need reauthorization', async () => {
    const { core, client, docs, emitForward } = setup()
    docs.get.mockResolvedValueOnce({ status: 'granted', persistence: 'remembered', revision: '1' })
      .mockResolvedValueOnce({ status: 'needsReauthorization', persistence: 'none', revision: '2' })
    vi.mocked(client.searchFiles).mockResolvedValueOnce(fileResponse('private.pdf'))
      .mockResolvedValueOnce(fileResponse('public.ttf'))
    vi.mocked(client.executeResult).mockRejectedValueOnce({ code: 'staleRequest' })
    await core.start()
    emitForward({ invocationId: 'find', forwardSequence: '1', query: 'report' })
    await vi.waitFor(() => expect(core.getSnapshot().results[0]?.name).toBe('private.pdf'))
    core.keyDown('Enter', false)
    await vi.waitFor(() => expect(core.getSnapshot().documents?.access?.status).toBe('needsReauthorization'))
    expect(core.getSnapshot().results[0]?.name).toBe('public.ttf')
    expect(docs.grant).not.toHaveBeenCalled()
    core.destroy()
  })
})
