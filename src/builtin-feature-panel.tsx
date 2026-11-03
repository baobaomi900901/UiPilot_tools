import { Button, Switch } from 'antd'
import { StartupCommandEditor } from './plugin-startup-editor'
import type { HotkeyRecordingPhase } from './launcher-core'
import { Camera, Globe, Link } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { BuiltinFeatureView, LauncherClient, ManagedBuiltinFeature } from './protocol'

const order: readonly ManagedBuiltinFeature[] = ['screenshot', 'quicklinks', 'webSearch']
export function useBuiltinFeatures(client: LauncherClient) {
  const [items, setItems] = useState<readonly BuiltinFeatureView[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const epoch = useRef(0)
  const pending = useRef(false)
  const activeRead = useRef<{
    request: number
    promise: Promise<readonly BuiltinFeatureView[] | undefined>
    resolve: (value: readonly BuiltinFeatureView[] | undefined | PromiseLike<readonly BuiltinFeatureView[] | undefined>) => void
    reject: (reason: unknown) => void
    beforePublish: ((items: readonly BuiltinFeatureView[]) => void)[]
  } | null>(null)
  const publish = useCallback((next: readonly BuiltinFeatureView[]) => {
    setItems(order.flatMap((feature) => next.filter((item) => item.feature === feature)))
  }, [])
  const reload = useCallback((throwOnFailure = false, beforePublish?: (items: readonly BuiltinFeatureView[]) => void) => {
    if (pending.current) return Promise.resolve(undefined)
    const previous = activeRead.current
    let resolve!: (value: readonly BuiltinFeatureView[] | undefined | PromiseLike<readonly BuiltinFeatureView[] | undefined>) => void
    let reject!: (reason: unknown) => void
    const promise = new Promise<readonly BuiltinFeatureView[] | undefined>((accept, fail) => { resolve = accept; reject = fail })
    const entry = { request: ++epoch.current, promise, resolve, reject, beforePublish: previous?.beforePublish.splice(0) ?? [] }
    if (beforePublish) entry.beforePublish.push(beforePublish)
    activeRead.current = entry
    // A mutation's waiter follows the newest authoritative read even if its old read hangs.
    previous?.resolve(promise)
    void (async () => {
      try {
        const next = await client.listBuiltinFeatures()
        if (entry.request !== epoch.current) return
        for (const accept of entry.beforePublish.splice(0)) accept(next)
        publish(next); setError(''); activeRead.current = null; entry.resolve(next)
      } catch {
        if (entry.request !== epoch.current) return
        entry.beforePublish.length = 0; activeRead.current = null
        setError('无法加载内置功能，请刷新重试。')
        entry.reject(new Error('builtin read unavailable'))
      }
    })()
    return throwOnFailure ? promise : promise.catch(() => undefined)
  }, [client, publish])
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | undefined
    void (async () => {
      try {
        unlisten = await client.listenBuiltinFeaturesChanged?.(() => { void reload() })
      } catch {
        // The initial read still provides the current state if event delivery is unavailable.
      }
      if (!active) { unlisten?.(); return }
      void reload()
    })()
    return () => { active = false; epoch.current++; activeRead.current?.resolve(undefined); activeRead.current = null; unlisten?.() }
  }, [client, reload])
  const toggle = async (feature: ManagedBuiltinFeature, enabled: boolean) => {
    if (pending.current) return
    pending.current = true
    activeRead.current?.resolve(undefined); activeRead.current = null
    const request = ++epoch.current
    setBusy(true); setError('')
    try {
      const next = await client.setBuiltinFeatureEnabled({ feature, enabled })
      if (request === epoch.current) publish(next)
    } catch {
      if (request !== epoch.current) return
      try {
        const next = await client.listBuiltinFeatures()
        if (request === epoch.current) { publish(next); setError('无法更改启用状态，已读取当前状态。') }
      } catch {
        if (request === epoch.current) setError('无法更改启用状态，无法读取当前状态，请刷新后核对。')
      }
    } finally {
      pending.current = false
      if (request === epoch.current) setBusy(false)
    }
  }
  return { items, busy, error, reload, toggle }
}
export function BuiltinFeatureRow({ item, busy, onToggle, onDetails }: {
  item: BuiltinFeatureView; busy: boolean; onToggle: (enabled: boolean) => void; onDetails: () => void
}) {
  const Icon = item.feature === 'screenshot' ? Camera : item.feature === 'quicklinks' ? Link : Globe
  return <article className="plugin-item public-plugin-item" data-builtin-feature={item.feature}>
    <div className="public-plugin-icon-cell builtin-feature-icon"><Icon aria-hidden size={20} strokeWidth={1.8} /></div>
    <div className="plugin-title-line public-plugin-name"><h3 title={item.name}>{item.name}</h3></div>
    <div className="public-plugin-command" title={item.command}><code>{item.command}</code></div>
    <div className="plugin-actions public-plugin-actions"><Switch size="small" aria-label={`启用 ${item.name}`} checked={item.enabled} disabled={busy} onChange={onToggle} /></div>
    <Button className="public-plugin-detail-link" type="link" size="small" disabled={busy} aria-label={`查看 ${item.name} 详情`} onClick={onDetails}>详情</Button>
  </article>
}
export function BuiltinFeatureDetail({ client, feature, headerActionsTarget, onRecordingPhase }: {
  client: LauncherClient; feature: BuiltinFeatureView; headerActionsTarget?: HTMLElement | null
  onRecordingPhase?: (phase: HotkeyRecordingPhase) => void | Promise<void>
}) {
  const state = useBuiltinFeatures(client)
  const current = state.items.find((item) => item.feature === feature.feature) ?? feature
  const [commandBlocked, setCommandBlocked] = useState(false)
  const command = {
    commandId: current.feature, effectiveName: current.command.replace(/^\//, ''), defaultName: current.defaultCommand.replace(/^\//, ''),
    nameOverride: current.command === current.defaultCommand ? null : current.command.replace(/^\//, ''),
    favorite: current.favorite, hotkey: current.hotkey, hotkeyStatus: current.hotkeyStatus,
  }
  const revisionFor = (item: BuiltinFeatureView) => JSON.stringify([item.command, item.favorite, item.hotkey, item.hotkeyStatus, item.enabled])
  return <section className="public-plugin-detail" aria-labelledby="public-plugin-detail-title" aria-busy={state.busy || commandBlocked}>
    <fieldset className="public-detail-fields" disabled={state.busy}>
    <dl className="public-plugin-detail-list">
      <dt className="public-detail-name-term">启动</dt><dd className="public-detail-name-value">
        <StartupCommandEditor command={command} revision={revisionFor(current)} single headerActionsTarget={headerActionsTarget}
          onBlockedChange={setCommandBlocked} onRecordingPhase={onRecordingPhase} disabled={state.busy}
          reload={async (accept) => {
            await state.reload(true, (next) => {
              const item = next.find((item) => item.feature === current.feature)
              if (item) accept?.(revisionFor(item))
            })
          }}
          operations={{
            name: (nameOverride) => client.setBuiltinFeatureName({ feature: current.feature, nameOverride }),
            hotkey: (hotkey) => client.setBuiltinFeatureHotkey({ feature: current.feature, hotkey }),
            favorite: (favorite) => client.setBuiltinFeatureFavorite({ feature: current.feature, favorite }),
          }} />
      </dd>
      <dt>版本号</dt><dd>{current.version}</dd>
      <dt>插件说明</dt><dd className="public-detail-description">{current.description}</dd>
      <dt>权限列表</dt><dd>宿主内置功能，由 UiPilot 管理系统能力。</dd>
      <dt>插件所在目录</dt><dd className="public-detail-directory">{current.directory ?? '宿主内置，无独立插件目录'}</dd>
      <dt>使用示例</dt><dd><ul className="public-plugin-detail-items">{current.examples.map((example) => <li key={example}><code>{example}</code></li>)}</ul></dd>
      <dt>启用状态</dt><dd><Switch aria-label={`启用 ${current.name}`} checked={current.enabled} disabled={state.busy || commandBlocked} onChange={(enabled) => void state.toggle(current.feature, enabled)} /> <span>{current.enabled ? '已启用' : '已停用'}</span></dd>
    </dl>
    </fieldset>
    {state.error ? <div className="plugin-row-error" role="alert">{state.error} <Button size="small" disabled={state.busy || commandBlocked} onClick={() => void state.reload()}>刷新</Button></div> : null}
  </section>
}
