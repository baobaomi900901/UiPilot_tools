import { Button, Input, Modal, Spin, Switch } from 'antd'
import type { InputRef } from 'antd'
import { RefreshCw, Search, ShieldCheck } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { PluginIcon } from './plugin-icon'
import { compareDecimalRevision, parseU64Decimal } from './protocol'
import type { LauncherClient, PublicPermission, PublicPluginInventory, PublicPluginInventoryItem } from './protocol'
import './plugin-permissions-panel.css'
import { HOST_CREDENTIALS, HOST_CREDENTIAL_CONSENT, SELECTION_CONSENT, SPEECH_CONSENT, isHostCredentialPermission, useHostCredentialStatus } from './host-credential-permissions'

const GROUPS = ['选中文字', '朗读', '网络', '剪贴板', '文件访问', '界面', '通知', '计时器', '后台运行', '主程序密钥'] as const
const ENGLISH_LABELS: Record<PublicPermission, string> = {
  ...Object.fromEntries(Object.entries(HOST_CREDENTIALS).map(([key, value]) => [key, value.english])) as Record<keyof typeof HOST_CREDENTIALS, string>,
  'speech.speak': 'Local Speech',
  'selection.read': 'Selected Text',
  'network.https': 'HTTPS Network',
  'url.open': 'Open URLs',
  'clipboard.write': 'Write Clipboard',
  'clipboard.paste': 'Paste to App',
  'clipboard.read': 'Read Clipboard',
  'clipboard.history.read': 'Read Clipboard History',
  'clipboard.history.paste': 'Paste Clipboard History',
  'clipboard.history.copy': 'Copy Clipboard History',
  'files.openText': 'Read Text Files',
  'files.userSelected': 'Selected Files',
  'files.index.readAll': 'Read File Index',
  'ui.panel': 'Launcher Panel',
  'ui.window': 'Plugin Window',
  'notifications.publish': 'Notifications',
  'timer.control': 'Timer Control',
  'background.schedule': 'Background Tasks',
}
const PERMISSIONS: Record<PublicPermission, { group: typeof GROUPS[number]; label: string; description: string }> = {
  ...Object.fromEntries(Object.entries(HOST_CREDENTIALS).map(([key, value]) => [key, { group: '主程序密钥', label: value.label, description: '使用主程序配置的官方服务，可能消耗 API 额度' }])) as Record<keyof typeof HOST_CREDENTIALS, { group: '主程序密钥'; label: string; description: string }>,
  'speech.speak': { group: '朗读', label: '本地朗读', description: SPEECH_CONSENT },
  'selection.read': { group: '选中文字', label: '读取选中文字', description: SELECTION_CONSENT },
  'network.https': { group: '网络', label: '网络访问', description: '通过 UiPilot Host 访问申报的 HTTPS 主机' },
  'url.open': { group: '网络', label: '打开网页', description: '在默认浏览器打开 HTTP(S) 链接' },
  'clipboard.write': { group: '剪贴板', label: '写入剪贴板', description: '将文本写入系统剪贴板' },
  'clipboard.paste': { group: '剪贴板', label: '粘贴到原应用', description: '写入剪贴板并向原应用发送一次粘贴' },
  'clipboard.read': { group: '剪贴板', label: '读取剪贴板', description: '读取系统剪贴板内容' },
  'clipboard.history.read': { group: '剪贴板', label: '剪贴板历史读取', description: '启用且授权期间在本机记录可用的历史摘要；不会自动识别敏感来源' },
  'clipboard.history.copy': { group: '剪贴板', label: '剪贴板历史复制', description: '将选定的完整历史记录写入剪贴板' },
  'clipboard.history.paste': { group: '剪贴板', label: '剪贴板历史粘贴', description: '将选定的剪贴板历史粘贴到原应用' },
  'files.openText': { group: '文件访问', label: '读取文本文件', description: '选择并读取本地 UTF-8 文本文件（最多 8 KiB）' },
  'files.userSelected': { group: '文件访问', label: '访问所选文件', description: '访问用户选择的文件' },
  'files.index.readAll': { group: '文件访问', label: '读取文件索引', description: '读取文件索引中的内容' },
  'ui.panel': { group: '界面', label: '启动器面板', description: '关闭后会关闭受影响的插件面板' },
  'ui.window': { group: '界面', label: '独立窗口', description: '关闭后会关闭受影响的插件窗口' },
  'notifications.publish': { group: '通知', label: '发送通知', description: '关闭后会清理该插件的活动通知资源' },
  'timer.control': { group: '计时器', label: '计时器控制', description: '关闭后会清理该插件的活动计时器' },
  'background.schedule': { group: '后台运行', label: '后台调度', description: '在后台安排任务' },
}

function NetworkScope({ plugin }: { plugin: PublicPluginInventoryItem }) {
  return <div className="permission-network-scope">
    <span>HTTPS Host</span>
    <ul>{plugin.network?.httpsHosts.map((host) => <li key={host}><code>{host}</code></li>)}</ul>
    {plugin.network?.credentials.length ? <>
      <span>凭据绑定</span>
      <ul>{plugin.network.credentials.map((credential) => {
        const setting = plugin.settings.find(({ definition }) => definition.type === 'secret' && definition.key === credential.settingKey)
        return <li key={credential.id}><code>{credential.id}</code> · {setting?.definition.label ? `${setting.definition.label} · ` : ''}<code>{credential.settingKey}</code> → <code>{credential.host}</code> · Bearer</li>
      })}</ul>
      <p>此凭据会发送给该服务；服务响应可能包含其回显。</p>
    </> : <p>无凭据绑定</p>}
  </div>
}

type Consent = { permission: PublicPermission; plugin: PublicPluginInventoryItem; revision: string; origin: HTMLElement }
type FocusReturn = { origin: HTMLElement; pluginId: string; dialog: Element | null; closed: boolean; moved: boolean }

export function PluginPermissionsPanel({ client, active }: { client: LauncherClient; active: boolean }) {
  const [inventory, setInventory] = useState<PublicPluginInventory | null>(null)
  const credentialStatus = useHostCredentialStatus(client, active, inventory)
  const current = useRef<PublicPluginInventory | null>(null)
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [rowErrors, setRowErrors] = useState<Record<string, string>>({})
  const [pendingIds, setPendingIds] = useState<ReadonlySet<string>>(new Set())
  const pending = useRef(new Set<string>())
  // An id enters this set only after its write settles. A read started later may unlock it.
  const awaitingRead = useRef(new Set<string>())
  const epoch = useRef(0)
  const readSequence = useRef(0)
  const live = useRef(false)
  const input = useRef<InputRef>(null)
  const [consent, setConsent] = useState<Consent | null>(null)
  const consentRef = useRef<Consent | null>(null)
  const focusReturn = useRef<FocusReturn | null>(null)
  const [closedDialog, setClosedDialog] = useState(0)
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set())

  const reload = useCallback(async () => {
    const owner = epoch.current; const request = ++readSequence.current
    const settledWrites = [...awaitingRead.current]
    if (!live.current) return
    setLoading(true)
    try {
      const next = await client.listPublicPlugins()
      if (!live.current || owner !== epoch.current || request !== readSequence.current) return
      if (current.current && compareDecimalRevision(next.revision, current.current.revision) < 0) throw new Error('stale inventory')
      current.current = next; setInventory(next); setLoadError('')
      for (const id of settledWrites) { awaitingRead.current.delete(id); pending.current.delete(id) }
      setPendingIds(new Set(pending.current))
    } catch {
      if (live.current && owner === epoch.current && request === readSequence.current) setLoadError('加载权限失败。无法刷新当前状态，请刷新后核对。')
    } finally {
      if (live.current && owner === epoch.current && request === readSequence.current) setLoading(false)
    }
  }, [client])

  useEffect(() => {
    if (!active) return
    live.current = true; epoch.current += 1
    current.current = null; setInventory(null); setLoadError(''); setRowErrors({}); setLoading(true)
    pending.current.clear(); awaitingRead.current.clear(); setPendingIds(new Set())
    void reload()
    let disposed = false; let off: (() => void) | undefined
    void client.listenPublicPluginPermissionsChanged?.(() => { if (!disposed) void reload() }).then((stop) => {
      if (disposed) stop(); else off = stop
    }).catch(() => { if (!disposed) setLoadError('权限变更监听不可用，请刷新后核对当前状态。') })
    return () => {
      disposed = true; live.current = false; epoch.current += 1; readSequence.current += 1; off?.()
      consentRef.current = null; setConsent(null); focusReturn.current = null
    }
  }, [active, client, reload])

  useEffect(() => {
    if (!active) return
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === 'f' && !consentRef.current && !event.isComposing) {
        event.preventDefault(); event.stopPropagation(); input.current?.focus()
      }
    }
    const onFocus = (event: FocusEvent) => {
      const returning = focusReturn.current; const target = event.target
      if (returning && target instanceof HTMLElement && target !== document.body && target !== returning.origin && !returning.dialog?.contains(target)) returning.moved = true
    }
    document.addEventListener('keydown', onKey); document.addEventListener('focusin', onFocus)
    return () => { document.removeEventListener('keydown', onKey); document.removeEventListener('focusin', onFocus) }
  }, [active])

  useEffect(() => {
    const returning = focusReturn.current
    if (!active || !returning?.closed || pending.current.has(returning.pluginId)) return
    focusReturn.current = null
    const focused = document.activeElement
    if (returning.moved || (focused && focused !== document.body && focused !== returning.origin && !returning.dialog?.contains(focused))) return
    if (returning.origin.isConnected && !returning.origin.matches(':disabled')) returning.origin.focus()
  }, [active, pendingIds, closedDialog])

  const closeConsent = () => {
    const confirmation = consentRef.current
    if (!confirmation) return
    focusReturn.current = { origin: confirmation.origin, pluginId: confirmation.plugin.pluginId, dialog: document.activeElement?.closest('[role="dialog"]') ?? document.querySelector('[role="dialog"]'), closed: false, moved: false }
    consentRef.current = null; setConsent(null)
  }
  const mutate = async (plugin: PublicPluginInventoryItem, permission: PublicPermission, granted: boolean, revision: string) => {
    if (!live.current || pending.current.has(plugin.pluginId)) return
    const expectedInventoryRevision = parseU64Decimal(revision)
    if (expectedInventoryRevision === null) { setRowErrors((errors) => ({ ...errors, [plugin.pluginId]: '清单版本无效，请刷新后重试。' })); return }
    const owner = epoch.current
    pending.current.add(plugin.pluginId); setPendingIds(new Set(pending.current)); setRowErrors((errors) => ({ ...errors, [plugin.pluginId]: '' }))
    try {
      await client.setPublicPluginPermissionAccess({ pluginId: plugin.pluginId, permission, granted, expectedInventoryRevision })
    } catch (failure) {
      if (live.current && owner === epoch.current) {
        const stale = typeof failure === 'object' && failure !== null && 'code' in failure && failure.code === 'staleInventory'
        setRowErrors((errors) => ({ ...errors, [plugin.pluginId]: stale ? '插件清单已改变，请刷新后重新确认。' : '无法更改权限。请核对刷新后的状态，不会自动重试。' }))
      }
    }
    if (!live.current || owner !== epoch.current) return
    awaitingRead.current.add(plugin.pluginId)
    await reload()
  }
  const toggle = (plugin: PublicPluginInventoryItem, permission: PublicPermission, granted: boolean, origin: HTMLElement) => {
    if (!current.current || loadError || pending.current.has(plugin.pluginId) || consentRef.current) return
    if ((permission === 'network.https' || permission === 'selection.read' || permission === 'speech.speak' || isHostCredentialPermission(permission)) && granted) {
      const next = { permission, plugin, revision: current.current.revision, origin }; consentRef.current = next; setConsent(next)
    } else {
      focusReturn.current = { origin, pluginId: plugin.pluginId, dialog: null, closed: true, moved: false }
      void mutate(plugin, permission, granted, current.current.revision)
    }
  }
  const confirmNetwork = () => {
    const confirmation = consentRef.current
    if (!confirmation) return
    closeConsent()
    if (loadError || current.current?.revision !== confirmation.revision) {
      setRowErrors((errors) => ({ ...errors, [confirmation.plugin.pluginId]: '插件清单或网络范围已改变，请刷新后重新确认。' })); return
    }
    void mutate(confirmation.plugin, confirmation.permission, true, confirmation.revision)
  }

  if (!active) return null
  const normalized = query.trim().toLocaleLowerCase()
  const rows = inventory?.items.flatMap((plugin) => plugin.permissions.map((permission) => ({ plugin, ...permission, ...PERMISSIONS[permission.permission] }))) ?? []
  const filtered = rows.filter((row) => [row.plugin.name, row.plugin.pluginId, ...row.plugin.commands.flatMap((command) => [`/${command.effectiveName}`, command.defaultName, command.commandId]), row.permission, row.label, ENGLISH_LABELS[row.permission], row.group].some((value) => value.toLocaleLowerCase().includes(normalized)))
  const sections = GROUPS.flatMap((group) => [...new Set(rows.filter((row) => row.group === group).map((row) => row.permission))])
  const empty = !inventory?.items.length ? '尚未安装公开插件' : !rows.length ? '已安装插件没有申报权限' : '没有匹配的权限'
  return <section className="plugin-permissions-panel" aria-labelledby="plugin-permissions-title" aria-busy={loading} onKeyDownCapture={(event) => {
    // Modal portals remain in this React tree. Own Escape before the settings return handler.
    if (event.key === 'Escape' && consent !== null) {
      event.preventDefault(); event.stopPropagation(); closeConsent()
    }
  }}>
    <header className="plugin-inventory-header permissions-heading"><h2 id="plugin-permissions-title">权限</h2><Button type="text" aria-label="刷新权限" title="刷新权限" disabled={loading} icon={<RefreshCw size={15} />} onClick={() => void reload()} /></header>
    <div className="public-plugin-filter permissions-search"><Input ref={input} aria-label="搜索插件或权限" placeholder="搜索插件、启动命令或权限" prefix={<Search size={15} />} value={query} onChange={(event) => setQuery(event.target.value)} /></div>
    {loadError ? <div className="permissions-error" role="alert">{loadError}</div> : null}
    {loading && !inventory ? <div className="permissions-empty" role="status"><Spin size="small" /><span>正在读取权限…</span></div> : !filtered.length ? <div className="permissions-empty"><ShieldCheck size={28} /><strong>{loadError && !inventory ? '无法读取插件权限' : empty}</strong><span>{normalized ? '尝试其他插件名称或权限。' : '可通过插件页面安装或查看公开插件。'}</span></div> : sections.map((capability) => {
      const grouped = filtered.filter((row) => row.permission === capability)
      const { label, description } = PERMISSIONS[capability]
      return grouped.length ? <section className="permission-group" key={capability} aria-labelledby={`permission-${capability}`}>
        <header className="permission-group-heading"><h3 id={`permission-${capability}`} title={capability}>{label} <span lang="en">/ {ENGLISH_LABELS[capability]}</span></h3><p>{description}{isHostCredentialPermission(capability) ? ` · ${credentialStatus(capability)}` : ''}</p></header>
        <ul>{grouped.map(({ plugin, permission, supported, granted }) => {
        const key = `${plugin.pluginId}:${permission}`; const busy = pendingIds.has(plugin.pluginId)
        const status = `${isHostCredentialPermission(permission) ? credentialStatus(permission) + ' · ' : ''}${!plugin.enabled ? '插件已停用，更改授权不会启用插件。' : ''}${busy ? '正在核对…' : !supported ? '暂不支持' : granted ? '已开启' : '已关闭'}`
        return <li className="permission-row" key={key} aria-busy={busy}>
          <div className="permission-row-main"><PluginIcon iconUrl={plugin.iconUrl} size={28} />
            {permission === 'network.https' && plugin.network ? <button type="button" className="permission-plugin-name permission-scope-toggle" title={`${plugin.name} · 点击查看网络范围`} aria-label={`查看 ${plugin.name} 的网络范围`} aria-expanded={expanded.has(plugin.pluginId)} onClick={() => setExpanded((previous) => { const next = new Set(previous); if (next.has(plugin.pluginId)) next.delete(plugin.pluginId); else next.add(plugin.pluginId); return next })}>{plugin.name}</button> : <span className="permission-plugin-name" title={plugin.name}>{plugin.name}</span>}
            <Switch size="small" title={status} aria-description={status} aria-label={`${plugin.name}：${label}`} checked={granted} disabled={!supported || busy || !!loadError || consent?.plugin.pluginId === plugin.pluginId || (permission === 'network.https' && !plugin.network)} onChange={(value, event) => toggle(plugin, permission, value, event.currentTarget)} />
          </div>
          {permission === 'network.https' && plugin.network && expanded.has(plugin.pluginId) ? <NetworkScope plugin={plugin} /> : null}
          {rowErrors[plugin.pluginId] ? <div className="permissions-error" role="alert">{rowErrors[plugin.pluginId]}</div> : null}
        </li>
      })}</ul></section> : null
    })}
    <p className="permissions-footer">此页管理公开插件申报的权限；内置功能与旧版兼容插件不在此管理。</p>
    <Modal className="public-plugin-confirmation permission-network-confirmation" destroyOnHidden title={consent ? `开启${PERMISSIONS[consent.permission].label}` : '开启权限'} open={consent !== null} onCancel={closeConsent} focusable={{ focusTriggerAfterClose: false }} afterClose={() => { if (focusReturn.current) focusReturn.current.closed = true; setClosedDialog((version) => version + 1) }} onOk={confirmNetwork} okText="确认开启" cancelText="取消" okButtonProps={{ 'aria-label': consent ? `确认开启${PERMISSIONS[consent.permission].label}` : '确认开启权限' }}>
      {consent?.permission === 'speech.speak' ? <p>{consent.plugin.name}：{SPEECH_CONSENT}</p> : consent?.permission === 'selection.read' ? <p>{consent.plugin.name}：{SELECTION_CONSENT}</p> : consent && isHostCredentialPermission(consent.permission) ? <p>{consent.plugin.name}：{HOST_CREDENTIALS[consent.permission].label}。{HOST_CREDENTIAL_CONSENT}</p> : <><p>确认允许 UiPilot Host 访问以下 HTTPS 主机和完整凭据绑定：</p>{consent ? <NetworkScope plugin={consent.plugin} /> : null}</>}
    </Modal>
  </section>
}
