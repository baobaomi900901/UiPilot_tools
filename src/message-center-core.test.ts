import { describe, expect, it, vi } from 'vitest'

import { createMessageCenterCore } from './message-center-core'
import {
  compareU64Decimal,
  parseMessageCenterSnapshot,
  parseMessageHostCommandError,
  parseMessageHostStateChanged,
  parseMessageSummary,
  parseMessageView,
  parseU64Decimal,
  type LauncherClient,
  type MessageCenterSnapshot,
  type MessageSummary,
} from './protocol'

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve
    reject = nextReject
  })
  return { promise, resolve, reject }
}

function message(id = '1') {
  return {
    id,
    pluginId: 'com.uipilot.demo-win',
    pluginNameSnapshot: 'Demo Window',
    pluginIconUrl: null,
    createdAt: '2026-08-19T01:02:03.000Z',
    content: `message-${id}`,
    readAt: null,
  }
}

function summary(revision: string, unreadCount: number): MessageSummary {
  return { revision: revision as MessageSummary['revision'], unreadCount }
}

function snapshot(revision: string, unreadCount: number, ids: string[] = ['1']): MessageCenterSnapshot {
  return {
    ...summary(revision, unreadCount),
    messages: ids.map((id) => message(id) as MessageCenterSnapshot['messages'][number]),
  }
}

function fakeClient() {
  let stateHandler: ((payload: unknown) => void) | undefined
  const order: string[] = []
  const unlisten = vi.fn()
  const client = {
    listenMessageStateChanged: vi.fn(async (handler: (payload: unknown) => void) => {
      order.push('listen')
      stateHandler = handler
      return unlisten
    }),
    getMessageSummary: vi.fn(async () => {
      order.push('summary')
      return summary('0', 0)
    }),
    openMessageCenter: vi.fn(async () => snapshot('0', 0, [])),
    readMessageCenter: vi.fn(async () => snapshot('0', 0, [])),
    clearMessages: vi.fn(async () => snapshot('0', 0, [])),
  } as unknown as Pick<
    LauncherClient,
    | 'listenMessageStateChanged'
    | 'getMessageSummary'
    | 'openMessageCenter'
    | 'readMessageCenter'
    | 'clearMessages'
  >
  return {
    client,
    order,
    unlisten,
    emit(payload: unknown) {
      if (!stateHandler) throw new Error('message state listener is not installed')
      stateHandler(payload)
    },
  }
}

describe('message center protocol', () => {
  it('compares canonical u64 decimals without JavaScript number conversion', () => {
    expect(compareU64Decimal('9', '10')).toBe(-1)
    expect(compareU64Decimal('10', '9')).toBe(1)
    expect(compareU64Decimal('99', '100')).toBe(-1)
    expect(compareU64Decimal('9007199254740992', '9007199254740993')).toBe(-1)
    expect(compareU64Decimal('18446744073709551615', '18446744073709551615')).toBe(0)
    expect(parseU64Decimal('18446744073709551615')).toBe('18446744073709551615')

    for (const invalid of ['', '00', '01', '-1', '18446744073709551616', 9, null]) {
      expect(parseU64Decimal(invalid)).toBeNull()
    }
    expect(() => compareU64Decimal('01', '1')).toThrow(TypeError)
    expect(() => compareU64Decimal('18446744073709551616', '1')).toThrow(TypeError)
  })

  it('strictly parses summaries, messages, snapshots, events, and command errors', () => {
    const view = message()
    const center = { revision: '7', unreadCount: 1, messages: [view] }
    expect(parseMessageSummary({ revision: '7', unreadCount: 1 })).toEqual({ revision: '7', unreadCount: 1 })
    expect(parseMessageView(view)).toEqual(view)
    expect(parseMessageCenterSnapshot(center)).toEqual(center)
    expect(parseMessageHostStateChanged({ status: 'ready', revision: '7', unreadCount: 1 })).toEqual({
      status: 'ready', revision: '7', unreadCount: 1,
    })
    expect(parseMessageHostStateChanged({ status: 'unavailable', error: 'MessageStoreUnavailable' })).toEqual({
      status: 'unavailable', error: 'MessageStoreUnavailable',
    })
    expect(parseMessageHostCommandError({ code: 'MessageOperationFailed', storeStatus: 'ready' })).toEqual({
      code: 'MessageOperationFailed', storeStatus: 'ready',
    })
    expect(parseMessageHostCommandError({ code: 'MessageStoreUnavailable', storeStatus: 'unavailable' })).toEqual({
      code: 'MessageStoreUnavailable', storeStatus: 'unavailable',
    })

    for (const invalid of [
      { revision: '01', unreadCount: 1 },
      { revision: '7', unreadCount: 101 },
      { revision: '7', unreadCount: 1, extra: true },
      { ...view, createdAt: 'not-an-instant' },
      { ...view, id: '18446744073709551616' },
      { ...view, pluginIconUrl: '' },
      { ...center, messages: [view, view] },
      { status: 'ready', revision: '7', unreadCount: 1, extra: true },
      { status: 'unavailable', error: 'other' },
      { code: 'MessageOperationFailed', storeStatus: 'unavailable' },
    ]) {
      expect(
        parseMessageSummary(invalid) ??
          parseMessageView(invalid) ??
          parseMessageCenterSnapshot(invalid) ??
          parseMessageHostStateChanged(invalid) ??
          parseMessageHostCommandError(invalid),
      ).toBeNull()
    }
  })
})

describe('message center state', () => {
  it('installs the event listener before requesting the initial summary', async () => {
    const fake = fakeClient()
    const core = createMessageCenterCore(fake.client)

    await core.start()

    expect(fake.order).toEqual(['listen', 'summary'])
    expect(core.getSnapshot()).toMatchObject({ status: 'ready', unreadCount: 0, summaryRevision: '0' })
    fake.emit({ status: 'ready', revision: '0', unreadCount: 100 })
    expect(core.getSnapshot()).toMatchObject({ unreadCount: 0, summaryRevision: '0' })
  })

  it('accepts a full snapshot at the same revision after the event arrives first', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.getMessageSummary).mockResolvedValueOnce(summary('9', 1))
    const opened = deferred<MessageCenterSnapshot>()
    vi.mocked(fake.client.openMessageCenter).mockReturnValueOnce(opened.promise)
    const core = createMessageCenterCore(fake.client)
    await core.start()

    const entering = core.enter()
    fake.emit({ status: 'ready', revision: '10', unreadCount: 2 })
    opened.resolve(snapshot('10', 0, ['10', '9']))
    await entering

    expect(core.getSnapshot()).toMatchObject({
      status: 'ready', summaryRevision: '10', snapshotRevision: '10', unreadCount: 0,
    })
    expect(core.getSnapshot().messages.map(({ id }) => id)).toEqual(['10', '9'])
  })

  it('rereads without marking when a higher event makes an arriving snapshot stale', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.getMessageSummary).mockResolvedValueOnce(summary('9', 1))
    const opened = deferred<MessageCenterSnapshot>()
    vi.mocked(fake.client.openMessageCenter).mockReturnValueOnce(opened.promise)
    vi.mocked(fake.client.readMessageCenter).mockResolvedValueOnce(snapshot('10', 2, ['10', '9']))
    const core = createMessageCenterCore(fake.client)
    await core.start()

    const entering = core.enter()
    fake.emit({ status: 'ready', revision: '10', unreadCount: 2 })
    opened.resolve(snapshot('9', 0, ['9']))
    await entering
    await vi.waitFor(() => expect(fake.client.readMessageCenter).toHaveBeenCalledOnce())
    await vi.waitFor(() => expect(core.getSnapshot().snapshotRevision).toBe('10'))

    expect(fake.client.openMessageCenter).toHaveBeenCalledOnce()
    expect(core.getSnapshot().messages.map(({ id }) => id)).toEqual(['10', '9'])
  })

  it('keeps unavailable absorbing across delayed ready events, summaries, snapshots, and ready errors', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.getMessageSummary).mockResolvedValueOnce(summary('9', 1))
    vi.mocked(fake.client.openMessageCenter).mockResolvedValueOnce(snapshot('9', 1, ['9']))
    const read = deferred<MessageCenterSnapshot>()
    const clear = deferred<MessageCenterSnapshot>()
    vi.mocked(fake.client.readMessageCenter).mockReturnValueOnce(read.promise)
    vi.mocked(fake.client.clearMessages).mockReturnValueOnce(clear.promise)
    const core = createMessageCenterCore(fake.client)
    await core.start()
    await core.enter()

    fake.emit({ status: 'ready', revision: '10', unreadCount: 2 })
    const clearing = core.clear()
    fake.emit({ status: 'unavailable', error: 'MessageStoreUnavailable' })
    fake.emit({ status: 'ready', revision: '11', unreadCount: 3 })
    read.resolve(snapshot('10', 2, ['10', '9']))
    clear.reject({ code: 'MessageOperationFailed', storeStatus: 'ready' })
    await clearing
    await Promise.resolve()

    expect(core.getSnapshot().status).toBe('unavailable')
    expect(core.getSnapshot().unreadCount).toBeUndefined()
    expect(core.getSnapshot().summaryRevision).toBeUndefined()
    expect(core.getSnapshot().snapshotRevision).toBeUndefined()
    expect(core.getSnapshot().operationError).toBe(false)
  })

  it('discards a delayed startup summary after unavailable is observed', async () => {
    const fake = fakeClient()
    const initial = deferred<MessageSummary>()
    vi.mocked(fake.client.getMessageSummary).mockReturnValueOnce(initial.promise)
    const core = createMessageCenterCore(fake.client)
    const start = core.start()
    await vi.waitFor(() => expect(fake.client.getMessageSummary).toHaveBeenCalledOnce())

    fake.emit({ status: 'unavailable', error: 'MessageStoreUnavailable' })
    initial.resolve(summary('18446744073709551615', 100))
    await start

    expect(core.getSnapshot()).toMatchObject({ status: 'unavailable', messages: [] })
  })

  it('starts unknown after a WebView reload and queries the still-unavailable native host', async () => {
    const fake = fakeClient()
    vi.mocked(fake.client.getMessageSummary).mockRejectedValueOnce({
      code: 'MessageStoreUnavailable', storeStatus: 'unavailable',
    })
    const core = createMessageCenterCore(fake.client)

    expect(core.getSnapshot()).toMatchObject({ status: 'unknown', messages: [] })
    await core.start()

    expect(fake.order).toEqual(['listen'])
    expect(fake.client.getMessageSummary).toHaveBeenCalledOnce()
    expect(core.getSnapshot()).toMatchObject({ status: 'unavailable', messages: [] })
  })
})
