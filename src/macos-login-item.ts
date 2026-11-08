export type LoginItemStatus = 'notRegistered' | 'enabled' | 'requiresApproval' | 'notFound' | 'unavailable'
export type LoginItemSnapshot = Readonly<{ status: LoginItemStatus }>
export type LoginItemIssue = 'invalidSignature' | 'permissionDenied' | 'serviceUnavailable' | 'notFound' | 'unknown'
export type LoginItemMutation = Readonly<LoginItemSnapshot & { outcome: 'applied' | 'unchanged' | 'requiresApproval' | 'unconfirmed'; issue?: LoginItemIssue }>
export interface MacLoginItemClient {
  read(): Promise<LoginItemSnapshot>
  set(enabled: boolean): Promise<LoginItemMutation>
  openSettings(): Promise<void>
}
const states = ['notRegistered', 'enabled', 'requiresApproval', 'notFound', 'unavailable']
function record(value: unknown, keys: readonly string[]): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const actual = Object.keys(value)
  return actual.length === keys.length && keys.every(key => Object.prototype.hasOwnProperty.call(value, key))
    ? value as Record<string, unknown> : null
}
export function parseLoginItemSnapshot(value: unknown): LoginItemSnapshot | null {
  const data = record(value, ['status'])
  return data && states.includes(data.status as string) ? Object.freeze({ status: data.status as LoginItemStatus }) : null
}
export function parseLoginItemMutation(value: unknown): LoginItemMutation | null {
  const data = record(value, ['status', 'outcome']) ?? record(value, ['status', 'outcome', 'issue'])
  if (!data || !states.includes(data.status as string) || !['applied', 'unchanged', 'requiresApproval', 'unconfirmed'].includes(data.outcome as string)) return null
  if ('issue' in data && (data.outcome !== 'unconfirmed' || !['invalidSignature', 'permissionDenied', 'serviceUnavailable', 'notFound', 'unknown'].includes(data.issue as string))) return null
  return Object.freeze({ ...data }) as LoginItemMutation
}
const unavailable = () => ({ code: 'loginItemUnavailable' as const })
const busy = () => ({ code: 'loginItemBusy' as const })

// Keep this client alive across settings views. A replacement view waits for an
// earlier user mutation to settle before reading fresh system truth.
export function createMacLoginItemClient(invoke: (command: string, input?: Record<string, unknown>) => Promise<unknown>): MacLoginItemClient {
  let pending: Promise<void> | undefined
  return {
    async read() {
      while (pending) await pending
      try {
        const result = parseLoginItemSnapshot(await invoke('get_macos_login_item'))
        if (!result) throw unavailable()
        return result
      } catch { throw unavailable() }
    },
    set(enabled) {
      if (pending) return Promise.reject(busy())
      if (typeof enabled !== 'boolean') return Promise.reject(unavailable())
      let release!: () => void
      const owner = new Promise<void>(resolve => { release = resolve })
      pending = owner // Occupy before even a synchronously throwing transport.
      return Promise.resolve().then(async () => {
        try {
          const result = parseLoginItemMutation(await invoke('set_macos_login_item', { input: { enabled } }))
          if (!result) throw unavailable()
          return result
        } catch { throw unavailable() }
        finally {
          if (pending === owner) pending = undefined
          release()
        }
      })
    },
    async openSettings() {
      if (pending) throw busy()
      try { await invoke('open_macos_login_item_settings') }
      catch { throw unavailable() }
    },
  }
}
