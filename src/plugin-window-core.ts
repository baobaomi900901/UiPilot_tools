import type { PluginWindowClient } from './protocol'

export interface PluginWindowSnapshot {
  pinned: boolean
  pending: boolean
  error?: string
}

export interface PluginWindowCore {
  getSnapshot(): PluginWindowSnapshot
  subscribe(listener: () => void): () => void
  togglePinned(): Promise<void>
  close(): Promise<void>
  destroy(): void
}

export function createPluginWindowCore(client: PluginWindowClient): PluginWindowCore {
  let snapshot: PluginWindowSnapshot = { pinned: false, pending: false }
  let destroyed = false
  const listeners = new Set<() => void>()
  const publish = (next: PluginWindowSnapshot) => {
    if (destroyed) return
    snapshot = Object.freeze(next)
    for (const listener of listeners) listener()
  }
  const errorText = () => '插件窗口操作失败。'

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      if (destroyed) return () => {}
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    async togglePinned() {
      if (destroyed || snapshot.pending) return
      const expected = !snapshot.pinned
      publish({ ...snapshot, pending: true, error: undefined })
      try {
        const result = await client.setPinned({ pinned: expected })
        publish({ pinned: result.pinned, pending: false })
      } catch {
        publish({ ...snapshot, pending: false, error: errorText() })
      }
    },
    async close() {
      if (destroyed || snapshot.pending) return
      publish({ ...snapshot, pending: true, error: undefined })
      try {
        await client.close()
        publish({ pinned: false, pending: false })
      } catch {
        publish({ ...snapshot, pending: false, error: errorText() })
      }
    },
    destroy() {
      destroyed = true
      listeners.clear()
    },
  }
}