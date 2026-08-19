import type { PluginWindowClient } from './protocol'
import { safePublicPluginIconUrl } from './plugin-icon-url'

export interface PluginWindowSnapshot {
  name: string
  iconUrl?: string
  pinned: boolean
  pending: boolean
  error?: string
}

export interface PluginWindowCore {
  getSnapshot(): PluginWindowSnapshot
  subscribe(listener: () => void): () => void
  start(): Promise<void>
  togglePinned(): Promise<void>
  close(): Promise<void>
  destroy(): void
}

export function createPluginWindowCore(client: PluginWindowClient): PluginWindowCore {
  let snapshot: PluginWindowSnapshot = { name: '插件', pinned: false, pending: false }
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
    async start() {
      if (destroyed) return
      try {
        const identity = await client.getIdentity()
        if (destroyed) return
        const iconUrl = safePublicPluginIconUrl(identity.iconUrl)
        publish({
          ...snapshot,
          name: identity.name,
          ...(iconUrl === undefined ? { iconUrl: undefined } : { iconUrl }),
        })
      } catch {
        // Identity failure must not disable host-owned window controls.
      }
    },
    async togglePinned() {
      if (destroyed || snapshot.pending) return
      const expected = !snapshot.pinned
      publish({ ...snapshot, pending: true, error: undefined })
      try {
        const result = await client.setPinned({ pinned: expected })
        publish({ ...snapshot, pinned: result.pinned, pending: false, error: undefined })
      } catch {
        publish({ ...snapshot, pending: false, error: errorText() })
      }
    },
    async close() {
      if (destroyed || snapshot.pending) return
      publish({ ...snapshot, pending: true, error: undefined })
      try {
        await client.close()
        publish({ ...snapshot, pinned: false, pending: false, error: undefined })
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
