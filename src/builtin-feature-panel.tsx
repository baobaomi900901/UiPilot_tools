import { Button, Switch } from 'antd'
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
  const publish = useCallback((next: readonly BuiltinFeatureView[]) => {
    setItems(order.flatMap((feature) => next.filter((item) => item.feature === feature)))
  }, [])
  const reload = useCallback(async () => {
    if (pending.current) return
    const request = ++epoch.current
    try {
      const next = await client.listBuiltinFeatures()
      if (request === epoch.current) { publish(next); setError('') }
    } catch {
      if (request === epoch.current) setError('无法加载内置功能，请刷新重试。')
    }
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
    return () => { active = false; epoch.current++; unlisten?.() }
  }, [client, reload])
  const toggle = async (feature: ManagedBuiltinFeature, enabled: boolean) => {
    if (pending.current) return
    pending.current = true
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
    <div className="public-plugin-icon-cell builtin-feature-icon"><Icon aria-hidden size={24} strokeWidth={1.8} /></div>
    <div className="plugin-item-main public-plugin-main">
      <div className="public-plugin-summary-copy">
        <div className="plugin-title-line"><h3>{item.name}</h3><span className="builtin-feature-label">内置</span><span>v{item.version}</span><code>{item.command}</code></div>
        <p className="plugin-description">{item.description}</p>
      </div>
      <div className="public-plugin-row-links"><Button type="link" size="small" disabled={busy} aria-label={`查看 ${item.name} 详情`} onClick={onDetails}>详情</Button></div>
    </div>
    <div className="plugin-actions public-plugin-actions"><Switch aria-label={`启用 ${item.name}`} checked={item.enabled} disabled={busy} onChange={onToggle} /></div>
  </article>
}
export function BuiltinFeatureDetail({ client, feature }: { client: LauncherClient; feature: BuiltinFeatureView }) {
  const state = useBuiltinFeatures(client)
  const current = state.items.find((item) => item.feature === feature.feature) ?? feature
  return <section className="public-plugin-detail" aria-labelledby="public-plugin-detail-title" aria-busy={state.busy}>
    <dl className="public-plugin-detail-list">
      <dt>类型</dt><dd>内置</dd>
      <dt>启动键</dt><dd><code>{current.command}</code></dd>
      <dt>版本号</dt><dd>{current.version}</dd>
      <dt>功能说明</dt><dd>{current.description}</dd>
      <dt>使用示例</dt><dd><ul className="public-plugin-detail-items">{current.examples.map((example) => <li key={example}><code>{example}</code></li>)}</ul></dd>
      <dt>启用状态</dt><dd><Switch aria-label={`启用 ${current.name}`} checked={current.enabled} disabled={state.busy} onChange={(enabled) => void state.toggle(current.feature, enabled)} /> <span>{current.enabled ? '已启用' : '已停用'}</span></dd>
    </dl>
    {state.error ? <div className="plugin-row-error" role="alert">{state.error} <Button size="small" disabled={state.busy} onClick={() => void state.reload()}>刷新</Button></div> : null}
  </section>
}
