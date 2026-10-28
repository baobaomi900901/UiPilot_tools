import {
  parseScreenshotSnapshot,
  type ScreenshotClient,
  type ScreenshotOwner,
  type ScreenshotSnapshot,
} from './screenshot-protocol'

export interface ScreenshotViewSnapshot {
  state: ScreenshotSnapshot | null
  imageUrl: string | null
  loading: boolean
  pending: boolean
  error: string | null
  notice: string | null
}

export interface ScreenshotCore {
  getSnapshot(): ScreenshotViewSnapshot
  subscribe(listener: () => void): () => void
  start(): Promise<void>
  copy(): Promise<void>
  save(): Promise<void>
  retake(): Promise<void>
  close(): Promise<void>
  destroy(): void
}

const errorMessages: Record<string, string> = {
  clipboardBusy: '剪贴板正忙，请重试',
  clipboardWriteFailed: '复制失败，请重试',
  saveFailed: '保存失败，请重试',
  resourceLimit: '截图资源超限，请重新截图',
  layoutChanged: '显示器布局已变化，请重新截图',
  expired: '截图会话已失效',
  unauthorized: '无法访问此截图',
  encodingFailed: '图片处理失败，请重新截图',
}

function errorText(error: unknown) {
  const code = error && typeof error === 'object' && 'code' in error ? String(error.code) : ''
  return errorMessages[code] ?? '截图操作失败，请重试'
}

// Shared across core remounts in one WebView; the host rejects replayed IDs.
let nextTransferId = 0n

export function createScreenshotCore(
  client: ScreenshotClient,
  windowInstance: string,
  urls = {
    create: (blob: Blob) => URL.createObjectURL(blob),
    revoke: (url: string) => URL.revokeObjectURL(url),
  },
): ScreenshotCore {
  let snapshot: ScreenshotViewSnapshot = {
    state: null, imageUrl: null, loading: true, pending: false, error: null, notice: null,
  }
  let destroyed = false
  let started = false
  let unlisten: (() => void) | undefined
  let request = 0
  let imageKey: string | null = null
  let readingKey: string | null = null
  let operation = 0
  let closePending = false
  const listeners = new Set<() => void>()

  const publish = (patch: Partial<ScreenshotViewSnapshot>) => {
    if (destroyed) return
    snapshot = Object.freeze({ ...snapshot, ...patch })
    listeners.forEach(handler => handler())
  }
  const owner = (state: ScreenshotSnapshot): ScreenshotOwner => ({
    sessionId: state.sessionId,
    generation: state.generation,
    windowInstance: state.windowInstance,
  })
  const key = (state: ScreenshotSnapshot) =>
    `${state.sessionId}:${state.generation}:${state.windowInstance}`

  const accept = (value: unknown) => {
    const state = parseScreenshotSnapshot(value)
    if (destroyed || !state || state.windowInstance !== windowInstance) return
    if (snapshot.state && BigInt(state.revision) <= BigInt(snapshot.state.revision)) return
    const previous = snapshot.state
    if (previous && key(previous) !== key(state)) {
      request++
      readingKey = null
    }
    publish({ state, error: state.error ? errorText({ code: state.error }) : null, notice: null })
    if (!['preview', 'copying', 'saving'].includes(state.phase)) {
      // Retakes keep the old preview until a new result or cancellation arrives.
      publish({ loading: !snapshot.imageUrl })
      return
    }
    const expected = key(state)
    if (imageKey === expected || readingKey === expected) return
    const ticket = ++request
    const transferId = String(++nextTransferId)
    const transferOwner = owner(state)
    readingKey = expected
    publish({ loading: true })
    void client.readImage(transferOwner, transferId).then(bytes => {
      // Do not allocate a Blob URL for an expired asynchronous response.
      if (destroyed || ticket !== request || !snapshot.state || key(snapshot.state) !== expected) return
      const url = urls.create(new Blob([bytes], { type: 'image/png' }))
      if (snapshot.imageUrl) urls.revoke(snapshot.imageUrl)
      imageKey = expected
      readingKey = null
      publish({ imageUrl: url, loading: false })
    }).catch(error => {
      if (ticket !== request) return
      readingKey = null
      if (snapshot.imageUrl) urls.revoke(snapshot.imageUrl)
      imageKey = null
      publish({ imageUrl: null, loading: false, error: errorText(error) })
    }).finally(() => {
      // The response has settled and is now owned by a Blob or discarded. Old
      // generations (including destroy) must still release their exact transfer.
      return client.releaseImage(transferOwner, transferId).catch(() => undefined)
    })
  }

  const busy = () => snapshot.pending || !!snapshot.state && snapshot.state.phase !== 'preview'
  const run = async (action: 'copy' | 'save' | 'retake') => {
    const state = snapshot.state
    if (destroyed || !state || busy() || (action !== 'retake' && (!snapshot.imageUrl || imageKey !== key(state)))) return
    const ticket = ++operation
    const expected = key(state)
    publish({ pending: true, error: null, notice: null })
    try {
      const result = await client[action](owner(state))
      if (!destroyed && ticket === operation && snapshot.state && key(snapshot.state) === expected) {
        publish({ notice: result?.status === 'copied' ? '已复制' : result?.status === 'saved' ? '已保存' : null })
      }
    } catch (error) {
      if (ticket === operation && snapshot.state && key(snapshot.state) === expected) {
        publish({ error: errorText(error) })
      }
    } finally {
      if (ticket === operation) publish({ pending: false })
    }
  }

  return {
    getSnapshot: () => snapshot,
    subscribe(handler) {
      listeners.add(handler)
      return () => { listeners.delete(handler) }
    },
    async start() {
      if (started || destroyed) return
      started = true
      try {
        const off = await client.listenState(accept)
        if (destroyed) { off(); return }
        unlisten = off
        accept(await client.getState())
      } catch (error) {
        publish({ loading: false, error: errorText(error) })
      }
    },
    copy: () => run('copy'),
    save: () => run('save'),
    retake: () => run('retake'),
    async close() {
      if (destroyed || !snapshot.state || closePending) return
      closePending = true
      try {
        // The host defers destruction until an active output operation completes.
        await client.close(owner(snapshot.state))
      } catch (error) {
        publish({ error: errorText(error) })
      } finally {
        closePending = false
      }
    },
    destroy() {
      if (destroyed) return
      destroyed = true
      request++
      operation++
      unlisten?.()
      if (snapshot.imageUrl) urls.revoke(snapshot.imageUrl)
      listeners.clear()
    },
  }
}
