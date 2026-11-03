import { Button, Modal, Switch } from 'antd'
import { useEffect, useRef, useState } from 'react'
import { HOST_CREDENTIALS, HOST_CREDENTIAL_CONSENT, SELECTION_CONSENT, isHostCredentialPermission, useHostCredentialStatus } from './host-credential-permissions'
import type { HostCredentialPermission } from './host-credential-permissions'
import { parseU64Decimal } from './protocol'
import type { LauncherClient, PublicPluginInventoryItem } from './protocol'
import './host-credential-access.css'

export function HostCredentialAccess({ client, plugin, revision, reload, disabled, acquire, release, selectedText = false }: {
  client: LauncherClient; plugin: PublicPluginInventoryItem; revision: string; reload: () => Promise<void>
  selectedText?: boolean
  disabled: boolean; acquire: () => boolean; release: () => void
}) {
  const status = useHostCredentialStatus(client, true, plugin)
  const [consent, setConsent] = useState<{ permission: HostCredentialPermission | 'selection.read'; revision: string; origin: HTMLElement } | null>(null)
  const current = useRef({ pluginId: plugin.pluginId, revision })
  current.current = { pluginId: plugin.pluginId, revision }
  const live = useRef(true)
  const pending = useRef(false)
  const [busy, setBusy] = useState(false)
  const [uncertain, setUncertain] = useState(false)
  const [error, setError] = useState('')
  const focus = useRef<{ origin: HTMLElement; dialog: Element | null; moved: boolean; closed: boolean } | null>(null)
  const [closed, setClosed] = useState(0)
  useEffect(() => {
    live.current = true
    const onFocus = (event: FocusEvent) => {
      const item = focus.current; const target = event.target
      if (item && target instanceof HTMLElement && target !== document.body && target !== item.origin && !item.dialog?.contains(target)) item.moved = true
    }
    document.addEventListener('focusin', onFocus)
    return () => { live.current = false; document.removeEventListener('focusin', onFocus) }
  }, [])
  useEffect(() => {
    const item = focus.current
    if (!item?.closed || busy || disabled) return
    focus.current = null
    const active = document.activeElement
    if (!item.moved && item.origin.isConnected && !item.origin.matches(':disabled') && (active === document.body || active === item.origin || item.dialog?.contains(active))) item.origin.focus()
  }, [closed, busy, disabled])
  const close = () => {
    if (!consent) return
    focus.current = { origin: consent.origin, dialog: document.querySelector('[role="dialog"]'), moved: false, closed: false }
    setConsent(null)
  }
  const mutate = async (permission: HostCredentialPermission | 'selection.read', granted: boolean, expected: string) => {
    if (pending.current) return
    const expectedInventoryRevision = parseU64Decimal(expected)
    if (expectedInventoryRevision === null || current.current.revision !== expected) { setError('插件清单已改变，请刷新后重新确认。'); release(); return }
    pending.current = true; setBusy(true); setError('')
    try { await client.setPublicPluginPermissionAccess({ pluginId: plugin.pluginId, permission, granted, expectedInventoryRevision }) }
    catch { if (live.current) setError('无法更改授权，请核对刷新后的状态并重新确认，不会自动重试。') }
    if (!live.current) return
    try { await reload(); if (live.current) setUncertain(false) }
    catch { if (live.current) { setError('无法刷新授权状态，请刷新后核对。'); setUncertain(true) } }
    finally { pending.current = false; if (live.current) { setBusy(false); release() } }
  }
  return <div className="host-credential-access" onKeyDownCapture={(event) => {
    if (event.key === 'Escape' && consent) { event.preventDefault(); event.stopPropagation(); close(); release() }
  }}>
    {plugin.permissions.filter((item) => selectedText ? item.permission === 'selection.read' : isHostCredentialPermission(item.permission)).map((item) => {
      const permission = item.permission as HostCredentialPermission | 'selection.read'
      return <div className="host-credential-row" key={permission}>
        <span className="host-credential-label">{(permission === 'selection.read' ? '读取选中文字' : HOST_CREDENTIALS[permission].label)}<small>{(permission === 'selection.read' ? '仅在主动触发划词翻译时读取' : status(permission))}</small></span>
        <span className="host-credential-grant">{item.granted ? '已授权' : '未授权'}</span>
        <Switch size="small" aria-label={(permission === 'selection.read' ? '读取选中文字' : HOST_CREDENTIALS[permission].label)} checked={item.granted} disabled={disabled || busy || uncertain || !item.supported || consent !== null} onChange={(granted, event) => {
          if (pending.current || !acquire()) return
          if (granted) setConsent({ permission, revision, origin: event.currentTarget })
          else { focus.current = { origin: event.currentTarget, dialog: null, moved: false, closed: true }; void mutate(permission, false, revision) }
        }} />
      </div>
    })}
    {error ? <div role="alert" className="plugin-row-error">{error}</div> : null}
    {uncertain ? <Button size="small" disabled={busy} onClick={() => { if (!acquire()) return; setBusy(true); void reload().then(() => { if (live.current) { setUncertain(false); setError('') } }).catch(() => {}).finally(() => { if (live.current) { setBusy(false); release() } }) }}>刷新授权状态</Button> : null}
    <Modal className="public-plugin-confirmation" destroyOnHidden open={consent !== null} title={consent ? `开启${(consent.permission === 'selection.read' ? '读取选中文字' : HOST_CREDENTIALS[consent.permission].label)}` : '开启服务授权'}
      focusable={{ focusTriggerAfterClose: false }} afterClose={() => { if (focus.current) focus.current.closed = true; setClosed((value) => value + 1) }}
      onCancel={() => { close(); release() }} cancelText="取消" okText="确认开启" okButtonProps={{ 'aria-label': consent ? `确认开启${(consent.permission === 'selection.read' ? '读取选中文字' : HOST_CREDENTIALS[consent.permission].label)}` : '确认开启服务授权' }}
      onOk={() => { if (!consent) return; const snapshot = consent; close(); void mutate(snapshot.permission, true, snapshot.revision) }}>
      <p>{plugin.name}：{consent ? (consent.permission === 'selection.read' ? '读取选中文字' : HOST_CREDENTIALS[consent.permission].label) : ''}。{consent?.permission === 'selection.read' ? SELECTION_CONSENT : HOST_CREDENTIAL_CONSENT}</p>
    </Modal>
  </div>
}
