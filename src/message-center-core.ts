import {
  compareU64Decimal,
  parseMessageCenterSnapshot,
  parseMessageHostCommandError,
  parseMessageHostStateChanged,
  parseMessageSummary,
  type LauncherClient,
  type MessageCenterSnapshot,
  type MessageCenterStateSnapshot,
  type MessageView,
  type U64Decimal,
} from './protocol'

type MessageCenterClient = Pick<
  LauncherClient,
  | 'listenMessageStateChanged'
  | 'getMessageSummary'
  | 'openMessageCenter'
  | 'readMessageCenter'
  | 'clearMessages'
>

export interface MessageCenterCore {
  readonly getSnapshot: () => MessageCenterStateSnapshot
  readonly subscribe: (listener: () => void) => () => void
  readonly start: () => Promise<void>
  readonly enter: () => Promise<void>
  readonly leave: () => void
  readonly clear: () => Promise<void>
  readonly destroy: () => void
}

interface Model {
  status: MessageCenterStateSnapshot['status']
  unreadCount?: number
  summaryRevision?: U64Decimal
  snapshotRevision?: U64Decimal
  messages: MessageView[]
  clearPending: boolean
  operationError: boolean
}

function project(model: Model): MessageCenterStateSnapshot {
  return Object.freeze({
    status: model.status,
    ...(model.unreadCount === undefined ? {} : { unreadCount: model.unreadCount }),
    ...(model.summaryRevision === undefined ? {} : { summaryRevision: model.summaryRevision }),
    ...(model.snapshotRevision === undefined ? {} : { snapshotRevision: model.snapshotRevision }),
    messages: Object.freeze(model.messages.map((message) => Object.freeze({ ...message }))),
    clearPending: model.clearPending,
    operationError: model.operationError,
  })
}

export function createMessageCenterCore(client: MessageCenterClient): MessageCenterCore {
  const model: Model = {
    status: 'unknown',
    messages: [],
    clearPending: false,
    operationError: false,
  }
  const listeners = new Set<() => void>()
  let snapshot = project(model)
  let started = false
  let destroyed = false
  let listenerReady = false
  let open = false
  let unlisten: (() => void) | undefined
  let lastReadRevision: U64Decimal | undefined

  function publish(changed: boolean): void {
    if (!changed || destroyed) return
    snapshot = project(model)
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

  function isUnavailable(): boolean {
    return model.status === 'unavailable'
  }

  function becomeUnavailable(): void {
    if (model.status === 'unavailable') return
    model.status = 'unavailable'
    model.unreadCount = undefined
    model.summaryRevision = undefined
    model.snapshotRevision = undefined
    model.clearPending = false
    model.operationError = false
    publish(true)
  }

  function handleError(value: unknown): void {
    if (destroyed || model.status === 'unavailable') return
    const error = parseMessageHostCommandError(value)
    if (error?.storeStatus === 'unavailable') {
      becomeUnavailable()
      return
    }
    model.operationError = true
    publish(true)
  }

  function applySummary(value: unknown): boolean {
    if (destroyed || model.status === 'unavailable') return false
    const next = parseMessageSummary(value)
    if (!next) {
      handleError({ code: 'MessageOperationFailed', storeStatus: 'ready' })
      return false
    }
    if (
      model.summaryRevision !== undefined &&
      compareU64Decimal(next.revision, model.summaryRevision) < 0
    ) {
      return false
    }
    const changed =
      model.status !== 'ready' ||
      model.summaryRevision !== next.revision ||
      model.unreadCount !== next.unreadCount ||
      model.operationError
    model.status = 'ready'
    model.summaryRevision = next.revision
    model.unreadCount = next.unreadCount
    model.operationError = false
    publish(changed)
    return true
  }

  function snapshotIsStale(): boolean {
    return model.summaryRevision !== undefined &&
      (model.snapshotRevision === undefined ||
        compareU64Decimal(model.snapshotRevision, model.summaryRevision) < 0)
  }

  function requestReadIfStale(): void {
    if (
      destroyed ||
      !listenerReady ||
      !open ||
      model.status === 'unavailable' ||
      model.summaryRevision === undefined ||
      !snapshotIsStale()
    ) {
      return
    }
    const requestedRevision = model.summaryRevision
    if (
      lastReadRevision !== undefined &&
      compareU64Decimal(lastReadRevision, requestedRevision) >= 0
    ) {
      return
    }
    lastReadRevision = requestedRevision
    void readCurrent(requestedRevision)
  }

  function applySnapshot(value: unknown): boolean {
    if (destroyed || model.status === 'unavailable') return false
    const next = parseMessageCenterSnapshot(value)
    if (!next) {
      handleError({ code: 'MessageOperationFailed', storeStatus: 'ready' })
      return false
    }
    if (
      model.summaryRevision !== undefined &&
      compareU64Decimal(next.revision, model.summaryRevision) < 0
    ) {
      requestReadIfStale()
      return false
    }
    const sorted = [...next.messages].sort((left, right) => compareU64Decimal(right.id, left.id))
    model.status = 'ready'
    model.summaryRevision = next.revision
    model.snapshotRevision = next.revision
    model.unreadCount = next.unreadCount
    model.messages = sorted
    model.operationError = false
    publish(true)
    return true
  }

  async function readCurrent(requestedRevision: U64Decimal): Promise<void> {
    try {
      const value = await client.readMessageCenter()
      if (destroyed || isUnavailable()) return
      applySnapshot(value)
    } catch (error) {
      handleError(error)
    } finally {
      if (
        !destroyed &&
        model.status !== 'unavailable' &&
        model.summaryRevision !== undefined &&
        compareU64Decimal(model.summaryRevision, requestedRevision) > 0
      ) {
        requestReadIfStale()
      }
    }
  }

  function stateChanged(payload: unknown): void {
    if (destroyed || model.status === 'unavailable') return
    const event = parseMessageHostStateChanged(payload)
    if (!event) return
    if (event.status === 'unavailable') {
      becomeUnavailable()
      return
    }
    const previousRevision = model.summaryRevision
    if (
      previousRevision !== undefined &&
      compareU64Decimal(event.revision, previousRevision) <= 0
    ) {
      return
    }
    if (!applySummary({ revision: event.revision, unreadCount: event.unreadCount })) return
    if (
      previousRevision === undefined ||
      compareU64Decimal(event.revision, previousRevision) > 0
    ) {
      requestReadIfStale()
    }
  }

  async function loadSummary(): Promise<void> {
    try {
      applySummary(await client.getMessageSummary())
    } catch (error) {
      handleError(error)
    }
  }

  async function start(): Promise<void> {
    if (started || destroyed) return
    started = true
    let registered: (() => void) | undefined
    try {
      registered = await client.listenMessageStateChanged(stateChanged)
    } catch (error) {
      handleError(error)
      return
    }
    if (destroyed) {
      registered()
      return
    }
    unlisten = registered
    listenerReady = true
    const initial = loadSummary()
    const initialOpen = open ? openCurrent() : undefined
    await initial
    await initialOpen
  }

  async function openCurrent(): Promise<void> {
    try {
      const value = await client.openMessageCenter()
      if (destroyed || model.status === 'unavailable') return
      applySnapshot(value)
    } catch (error) {
      handleError(error)
    }
  }

  async function enter(): Promise<void> {
    if (destroyed || open) return
    open = true
    lastReadRevision = undefined
    if (!listenerReady || model.status === 'unavailable') return
    await openCurrent()
  }

  function leave(): void {
    open = false
    lastReadRevision = undefined
  }

  async function clear(): Promise<void> {
    if (
      destroyed ||
      model.status !== 'ready' ||
      model.messages.length === 0 ||
      model.clearPending
    ) {
      return
    }
    model.clearPending = true
    model.operationError = false
    publish(true)
    try {
      const value = await client.clearMessages()
      if (destroyed || isUnavailable()) return
      applySnapshot(value)
    } catch (error) {
      handleError(error)
    } finally {
      if (!destroyed && !isUnavailable() && model.clearPending) {
        model.clearPending = false
        publish(true)
      }
    }
  }

  function destroy(): void {
    if (destroyed) return
    destroyed = true
    unlisten?.()
    unlisten = undefined
    listeners.clear()
  }

  return { getSnapshot, subscribe, start, enter, leave, clear, destroy }
}
