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

  it('loads only the current selected result thumbnail', async () => {
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
    const core = createFindCore(fake.client)
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
