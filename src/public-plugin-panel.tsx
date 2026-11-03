import { TranslationSelectionHotkey } from './translation-selection-hotkey'
import { BuiltinFeatureRow, useBuiltinFeatures } from './builtin-feature-panel'
import { StartupCommandEditor } from './plugin-startup-editor'
import type { HotkeyRecordingPhase } from './launcher-core'
import { Button, Checkbox, Input, Modal, Spin, Switch, Tooltip } from 'antd'
import { FolderOpen, RefreshCw, Upload } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

import { formatPublicPluginDiagnostics, parsePublicPluginDiagnostics, parseU64Decimal } from './protocol'
import { HOST_CREDENTIALS, isHostCredentialPermission, installationPermissionGrants } from './host-credential-permissions'
import { HostCredentialAccess } from './host-credential-access'
import { PluginIcon } from './plugin-icon'
import { PublicPluginSettings } from './public-plugin-settings'
import type {
  LauncherClient,
  BuiltinFeatureView,
  PublicPermission,
  PublicPluginInventory,
  PluginNetworkCredential,
  PublicSettingView,
  PublicPluginInventoryItem,
  PublicPluginPrepareSummary,
  PublicPluginDiagnosticReport,
} from './protocol'

export interface PublicPluginRemovalOutcome {
  pluginId: string
  pluginName: string
  dataCleanupPending: boolean
}

function removalMessage(outcome: PublicPluginRemovalOutcome): string {
  return `${outcome.pluginName}：${outcome.dataCleanupPending ? '插件已删除，但部分插件数据尚未清理。请核对残留数据，不会自动重试删除。' : '插件已删除。'}`
}

interface PublicPluginPanelProps {
  client: LauncherClient
  removalOutcome?: PublicPluginRemovalOutcome | null
  onOpenBuiltinDetails?: (feature: BuiltinFeatureView) => void
  onOpenDetails?: (plugin: PublicPluginInventoryItem, revision: string, hotkeyRevision: string) => void
}

const CREDENTIAL_NOTICE = '此凭据会发送给该服务；服务响应可能包含其回显。'
const PLUGIN_FILTER_DEBOUNCE_MS = 150
const CLIPBOARD_HISTORY_NOTICE = '授权后，UiPilot 运行、插件启用且权限有效期间，会在本机记录该插件可用的剪贴板历史摘要；不会自动识别密码或敏感来源。'

function permissionLabel(permission: PublicPermission): string {
  if (isHostCredentialPermission(permission)) return HOST_CREDENTIALS[permission].label
  switch (permission) {
    case 'selection.read':
      return '读取选中文字（仅在主动触发划词翻译时）'
    case 'files.openText':
      return '选择并读取本地 UTF-8 文本文件（最多 8 KiB）'
    case 'url.open':
      return '在默认浏览器打开 HTTP(S) 链接'
    case 'clipboard.paste':
      return '粘贴到原应用（写入剪贴板并向唤起 UiPilot 前的应用发送一次粘贴）'
    case 'clipboard.write':
      return '写入剪贴板'
    case 'clipboard.read':
      return '读取剪贴板（保留，暂不支持）'
    case 'clipboard.history.read':
      return '剪贴板历史读取'
    case 'clipboard.history.copy':
      return '剪贴板历史复制'
    case 'clipboard.history.paste':
      return '剪贴板历史粘贴'
    case 'network.https':
      return '网络访问'
    case 'notifications.publish':
      return '发送通知'
    case 'timer.control':
      return '计时器控制'
    case 'ui.window':
      return '独立窗口'
    case 'ui.panel':
      return '启动器面板'
    default:
      return permission
  }
}

function permissionSummary(permission: PublicPermission): string {
  const label = permissionLabel(permission)
  return label === permission ? permission : `${label} · ${permission}`
}

function hasClipboardHistoryRead(permissions: readonly PublicPermission[]) {
  return permissions.includes('clipboard.history.read')
}

function PublicPluginRow({
  client,
  plugin,
  reload,
  onDetails,
}: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  reload: () => Promise<void>
  onDetails: () => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  const mutate = async (operation: () => Promise<void>) => {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await operation()
      await reload()
    } catch {
      setError('操作不可用，请重试。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <article className="plugin-item public-plugin-item">
      <div className="public-plugin-icon-cell">
        <PluginIcon iconUrl={plugin.iconUrl} size={28} />
      </div>
      <div className="plugin-title-line public-plugin-name">
        <h3 title={plugin.name}>{plugin.name}</h3>
        {plugin.fault ? <span className="plugin-fault-label">运行故障</span> : null}
      </div>
      <div className="public-plugin-command" title={plugin.commands.map((command) => `${command.commandId}: /${command.effectiveName}`).join(' · ')}>
        {plugin.commands.slice(0, 1).map((command) => <code key={command.commandId}>/{command.effectiveName}</code>)}
        {plugin.commands.length > 1 ? <span className="public-plugin-command-count">+{plugin.commands.length - 1}</span> : null}
      </div>
      <div className="plugin-actions public-plugin-actions">
        <Switch
          size="small"
          aria-label={`启用 ${plugin.name}`}
          checked={plugin.enabled}
          disabled={busy}
          onChange={(enabled) => void mutate(() => client.setPublicPluginEnabled({
            pluginId: plugin.pluginId,
            enabled,
          }))}
        />
      </div>
      <Button className="public-plugin-detail-link" type="link" size="small" aria-label="查看插件详情" onClick={onDetails}>详情</Button>
      {error ? <div className="plugin-row-error public-plugin-row-error" role="alert">{error}</div> : null}
    </article>
  )
}

function permissionText({ permission, supported, granted }: PublicPluginInventoryItem['permissions'][number]) {
  return `${permissionSummary(permission)} · ${supported ? (granted ? '已开启' : '已关闭') : '暂不支持'}`
}

function CredentialBindings({ credentials, settings = [], addedIds = [] }: {
  credentials: readonly PluginNetworkCredential[]
  settings?: readonly PublicSettingView[]
  addedIds?: readonly string[]
}) {
  if (!credentials.length) return null
  return (
    <div className="public-credential-bindings">
      <ul className="public-plugin-detail-items">
        {credentials.map((credential) => {
          const setting = settings.find(({ definition }) => definition.type === 'secret' && definition.key === credential.settingKey)
          return (
            <li key={credential.id}>
              <code>{credential.id}</code> · {setting ? `${setting.definition.label} · ` : ''}<code>{credential.settingKey}</code>
              {' → '}<code>{credential.host}</code> · Bearer
              {addedIds.includes(credential.id) ? <span className="public-network-added"> 新增或变更</span> : null}
            </li>
          )
        })}
      </ul>
      <p className="public-clipboard-history-notice">{CREDENTIAL_NOTICE}</p>
    </div>
  )
}

export function PublicPluginConfirmation({ prepared, busy, onCommit, onCancel }: { prepared: PublicPluginPrepareSummary; busy: boolean; onCommit: () => void; onCancel: () => void }) {
  return (
        <div className="public-prepare" role="status">
          <PluginIcon iconUrl={prepared.iconUrl} size={32} />
          <strong>{prepared.name}</strong><span>{prepared.previousVersion ? `${prepared.previousVersion} → ${prepared.version}` : prepared.version}</span>
          <span>{prepared.operation === 'developmentRefresh' ? '同版本开发目录快照' : prepared.isUpdate ? '替换插件包' : '安装插件'}</span>
          {prepared.permissions.some((permission) => permission.startsWith('credentials.')) ? <p>主程序密钥授权默认关闭；已有授权保持原状态。安装后请在插件详情逐项确认开启，可能消耗 API 额度。</p> : null}
          {prepared.isUpdate ? <p>插件包版本与保存的数据版本独立；替换或降级不会自动转换、恢复数据，请核对兼容性。</p> : null}
          <span>{prepared.permissions.filter((permission) => permission !== 'network.https').map(permissionSummary).join('、') || (prepared.network ? permissionSummary('network.https') : '无额外权限')}</span>
          {hasClipboardHistoryRead(prepared.permissions) ? (
            <p className="public-clipboard-history-notice">{CLIPBOARD_HISTORY_NOTICE}</p>
          ) : null}
          {prepared.network ? (
            <div className="public-network-consent">
              <strong>网络访问 · network.https{prepared.network.requiresNetworkConsent ? '（需要确认）' : ''}</strong>
              <ul className="public-network-host-list">
                {prepared.network.httpsHosts.map((host) => (
                  <li key={host}>
                    <code>{host}</code>
                    {prepared.network?.addedHttpsHosts.includes(host) ? <span className="public-network-added">新增</span> : null}
                  </li>
                ))}
              </ul>
              <CredentialBindings credentials={prepared.network.credentials} addedIds={prepared.network.addedCredentialIds} />
              <p>仅允许由 UiPilot Host 代理访问以上 HTTPS 主机，不会开放插件 WebView 的通用网络访问。</p>
            </div>
          ) : null}
          <Button type="primary" loading={busy} onClick={onCommit}>{prepared.operation === 'developmentRefresh' ? '确认重新载入' : prepared.isUpdate ? '确认更新' : '确认安装'}</Button>
          <Button aria-label="取消安装" disabled={busy} onClick={onCancel}>取消</Button>
        </div>
  )
}

export function publicPluginRecoveryError(error: unknown): string {
  const code = typeof error === 'object' && error !== null && 'code' in error ? error.code : null
  switch (code) {
    case 'refreshTargetMismatch': return '所选目录不属于当前已安装插件，请重新选择。'
    case 'refreshVersionMismatch': return '版本不同；请返回列表使用选择开发目录或选择插件包更新。'
    case 'staleInventory': return '插件列表已改变，请刷新后重新选择。'
    case 'runtimeNotReady': return '插件未能就绪；请修复开发目录后重新选择。'
    case 'invalidToken': case 'expiredToken': return '本次确认已失效，请重新选择并确认。'
    case 'permissionDenied': case 'unsupportedPermission': return '无法确认插件权限，请核对后重新选择。'
    case 'nameConflict': return '插件启动键冲突，请核对当前命令后重试。'
    case 'invalidPackage': case 'incompatibleApi': case 'incompatiblePlatform': return '插件包无效或与当前宿主不兼容，请检查后重新选择。'
    default: return '操作未完成，请刷新状态并查看诊断；状态未确认时请重启宿主后核对。'
  }
}

export function PublicPluginRecovery({ client, plugin, inventoryRevision, reload, detailBusy = false, onBusy = () => {}, acquire = () => true, onRemoved }: {
  client: LauncherClient; plugin: PublicPluginInventoryItem; inventoryRevision: string; reload: () => Promise<void>; detailBusy?: boolean; onBusy?: (busy: boolean) => void; acquire?: () => boolean; onRemoved?: (outcome: PublicPluginRemovalOutcome) => void
}) {
  const live = useRef(true)
  const request = useRef(0)
  const pending = useRef(false)
  const origin = useRef<HTMLElement | null>(null)
  const restore = useRef(false)
  const [busy, setBusy] = useState(false)
  const [prepared, setPrepared] = useState<PublicPluginPrepareSummary | null>(null)
  const [report, setReport] = useState<PublicPluginDiagnosticReport | null>(null)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [retainData, setRetainData] = useState(true)
  const removeOrigin = useRef<HTMLElement | null>(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  useEffect(() => { live.current = true; return () => { live.current = false; request.current += 1 } }, [])
  useEffect(() => {
    if (busy || prepared || !restore.current) return
    restore.current = false
    if (origin.current?.isConnected && (!document.activeElement || document.activeElement === document.body || document.activeElement === origin.current)) origin.current.focus()
  }, [busy, prepared])
  const begin = (ownDialog = false) => {
    if (pending.current || (detailBusy && !ownDialog) || !acquire()) return false
    pending.current = true; setBusy(true); onBusy(true); setError(''); setNotice(''); return true
  }
  const end = (confirming = false) => { pending.current = false; if (live.current) { setBusy(false); onBusy(confirming) } }
  const refresh = async (element: HTMLElement) => {
    if (prepared || !begin()) return
    origin.current = element
    let confirming = false
    try {
      const path = await client.selectPublicPluginDirectory()
      if (!live.current) return
      if (!path) { restore.current = true; return }
      const next = await client.preparePublicPlugin({ source: { kind: 'developmentDirectory', path }, refreshTarget: { pluginId: plugin.pluginId, expectedInventoryRevision: inventoryRevision } })
      if (!live.current) { void client.cancelPublicPlugin({ token: next.token }).catch(() => {}); return }
      confirming = true; setPrepared(next)
    } catch (failure) { if (live.current) { setError(publicPluginRecoveryError(failure)); restore.current = true } }
    finally { end(confirming) }
  }
  const cancel = async () => {
    if (!prepared || !begin()) return
    const token = prepared.token; setPrepared(null)
    try { await client.cancelPublicPlugin({ token }) } catch { if (live.current) setError('无法取消确认；请重新选择，旧确认不会自动执行。') }
    finally { restore.current = true; end() }
  }
  const mutate = async (operation: () => Promise<void>, success: string) => {
    if (!begin()) return
    request.current += 1; setReport(null)
    let committed = false
    try { await operation(); committed = true; if (live.current) setNotice(success) }
    catch (failure) { if (live.current) setError(publicPluginRecoveryError(failure)) }
    finally { if (live.current) setPrepared(null) }
    if (!live.current) return
    try { await reload() } catch { if (live.current) setError(committed ? '操作已完成。无法刷新当前状态，请返回列表刷新后核对。' : '操作未完成，无法刷新当前状态；请返回列表刷新并查看诊断。') }
    restore.current = true; end()
  }
  const commit = () => {
    if (!prepared) return
    const confirmation = prepared
    void mutate(() => client.commitPublicPlugin({ input: { token: confirmation.token, permissionGrants: installationPermissionGrants(confirmation.permissions) } }), '开发目录快照已重新载入；请重新运行命令查看新代码。')
  }
  const diagnose = async () => {
    if (!begin()) return
    const owner = ++request.current
    try {
      const next = parsePublicPluginDiagnostics(await client.getPublicPluginDiagnostics({ pluginId: plugin.pluginId }))
      if (!next || next.pluginId !== plugin.pluginId) throw new Error('invalid diagnostic')
      if (live.current && owner === request.current) setReport(next)
    } catch { if (live.current && owner === request.current) { setReport(null); setError('无法读取诊断，请刷新状态后重试。') } }
    finally { if (owner === request.current) end() }
  }
  const copy = async () => {
    if (!report || prepared || !begin()) return
    const snapshot = report
    try { await client.copyPublicPluginDiagnostics({ report: snapshot }); if (live.current) setNotice('已复制预览中的诊断。') }
    catch { if (live.current) setError('无法复制诊断，请重试。') }
    finally { end() }
  }
  const closeRemove = () => { if (pending.current) return; setRemoveOpen(false); restore.current = true; onBusy(false) }
  const remove = async () => {
    if (!removeOpen || !begin(true)) return
    let outcome: PublicPluginRemovalOutcome | null = null
    try {
      await client.uninstallPublicPlugin({ pluginId: plugin.pluginId, retainData })
      outcome = { pluginId: plugin.pluginId, pluginName: plugin.name, dataCleanupPending: false }
    } catch (failure) {
      const code = typeof failure === 'object' && failure !== null && 'code' in failure ? failure.code : null
      if (code === 'dataCleanupPending') outcome = { pluginId: plugin.pluginId, pluginName: plugin.name, dataCleanupPending: true }
      else if (live.current) setError('无法删除插件，请刷新状态后核对。')
    }
    if (!live.current) return
    if (outcome) {
      setRemoveOpen(false); setNotice(removalMessage(outcome))
      // Publish durable outcomes before a read removes this detail from the tree.
      onRemoved?.(outcome)
    }
    try { await reload() } catch { if (live.current) setError('无法刷新当前状态，请返回列表后核对。') }
    restore.current = true; end()
  }
  return <div className="public-plugin-recovery" aria-busy={busy} onKeyDown={(event) => {
    if (event.key === 'Escape' && prepared) { event.preventDefault(); event.stopPropagation(); void cancel() }
  }}>
    <div className="public-recovery-actions">
      <Button disabled={busy || detailBusy || !!prepared} onClick={(event) => void refresh(event.currentTarget)}>重新载入</Button>
      {!plugin.enabled ? <Button disabled={busy || detailBusy || !!prepared} onClick={(event) => { origin.current = event.currentTarget; void mutate(() => client.setPublicPluginEnabled({ pluginId: plugin.pluginId, enabled: true }), '插件已启用；请重新运行命令。') }}>{plugin.fault ? '重新启用' : '启用'}</Button> : null}
      <Button danger aria-label="删除插件" disabled={busy || detailBusy || !!prepared || removeOpen} onClick={(event) => {
        if (!acquire()) return
        removeOrigin.current = event.currentTarget; origin.current = event.currentTarget; setRetainData(true); setRemoveOpen(true); onBusy(true)
      }}>删除</Button>
      <Button disabled={busy || detailBusy || !!prepared || removeOpen} onClick={() => void diagnose()}>查看诊断</Button>
    </div>
    <p className="public-recovery-help">重新选择 package 目录；保存的数据不会被主动清除。替换后需重新运行命令；已停用的插件仍保持停用。</p>
    {prepared ? <PublicPluginConfirmation prepared={prepared} busy={busy} onCommit={commit} onCancel={() => void cancel()} /> : null}
    {report ? <div className="public-diagnostic-preview">
      <p>{report.state === null ? '状态未确认，请重启宿主后核对' : report.state.status === 'notInstalled' ? '当前未安装' : report.state.enabled ? '配置已启用；不代表 Runtime 健康' : '配置已停用'}。诊断包含插件标识和版本，请核对预览后复制。</p>
      <Input.TextArea aria-label="诊断 JSON 预览" readOnly value={formatPublicPluginDiagnostics(report)} rows={10} />
      <Button disabled={busy || detailBusy || !!prepared} onClick={() => void copy()}>复制此诊断</Button>
    </div> : null}
    <Modal className="public-plugin-confirmation" title="删除插件" open={removeOpen} onCancel={closeRemove} focusable={{ focusTriggerAfterClose: false }}
      okText="删除" cancelText="取消" confirmLoading={busy} onOk={() => void remove()}
      okButtonProps={{ danger: true, 'aria-label': '确认删除插件' }} cancelButtonProps={{ disabled: busy }}
      afterClose={() => {
        const target = removeOrigin.current
        if (!busy && target?.isConnected && (document.activeElement === document.body || document.activeElement === target)) target.focus()
      }}>
      <p>删除 {plugin.name}？</p>
      {error ? <p role="alert" className="plugin-row-error">{error}</p> : null}
      <Checkbox checked={retainData} disabled={busy} onChange={(event) => setRetainData(event.target.checked)}>保留插件数据</Checkbox>
    </Modal>
    <div className="public-detail-feedback" aria-live="polite">{busy ? <span role="status">正在处理…</span> : null}{notice ? <span role="status">{notice}</span> : null}{error ? <span role="alert" className="plugin-row-error">{error}</span> : null}</div>
  </div>
}

export function PublicPluginDetail({ client, plugin, inventoryRevision, hotkeyRevision = '0', onRecordingPhase, reload, headerActionsTarget, onRemoved }: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  inventoryRevision: string
  hotkeyRevision?: string
  headerActionsTarget?: HTMLElement | null
  onRemoved?: (outcome: PublicPluginRemovalOutcome) => void
  onRecordingPhase?: (phase: HotkeyRecordingPhase) => void
  reload: (beforePublish?: (revision: string) => void) => Promise<void>
}) {
  const [busy, setBusy] = useState(false)
  const [selectedCommandId, setSelectedCommandId] = useState(plugin.commands[0]?.commandId ?? '')
  const selectedCommand = plugin.commands.find((command) => command.commandId === selectedCommandId) ?? plugin.commands[0]
  const submitCommand = useRef<(() => Promise<boolean>) | null>(null)
  const switching = useRef(false)
  const [commandBlocked, setCommandBlocked] = useState(false)
  const changeCommand = async (commandId: string) => {
    if (switching.current || commandBlocked || commandId === selectedCommand?.commandId) return
    switching.current = true
    try { if (!submitCommand.current || await submitCommand.current()) setSelectedCommandId(commandId) }
    finally { switching.current = false }
  }
  const [recoveryBusy, setRecoveryBusy] = useState(false)
  const [ordinaryBusy, setOrdinaryBusy] = useState(false)
  const managementOwner = useRef<'ordinary' | 'detail' | 'recovery' | null>(null)
  const acquire = (owner: 'ordinary' | 'detail' | 'recovery') => {
    if (managementOwner.current !== null && managementOwner.current !== owner) return false
    managementOwner.current = owner
    return true
  }
  const release = (owner: 'ordinary' | 'detail' | 'recovery') => { if (managementOwner.current === owner) managementOwner.current = null }
  const operationPending = useRef(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [secretDrafts, setSecretDrafts] = useState<Record<string, string>>({})
  const [networkConsent, setNetworkConsent] = useState<{ plugin: PublicPluginInventoryItem; revision: string; origin: HTMLElement } | null>(null)
  const [clearKey, setClearKey] = useState<string | null>(null)
  const clearOrigin = useRef<HTMLElement | null>(null)
  const focusReturn = useRef<{
    origin: HTMLElement
    fallback: HTMLElement | null
    dialog: 'network' | 'clear' | null
    dialogElement: Element | null
    dialogClosed: boolean
    moved: boolean
  } | null>(null)
  const [closedDialog, setClosedDialog] = useState(0)
  const permissionReload = useRef(reload)
  useEffect(() => { permissionReload.current = reload }, [reload])
  useEffect(() => {
    if (!client.listenPublicPluginPermissionsChanged) return
    let live = true
    let unsubscribe: (() => void) | undefined
    void client.listenPublicPluginPermissionsChanged(() => {
      if (!live) return
      void permissionReload.current().catch(() => {
        if (live) setError('无法刷新权限状态，请重新打开详情。')
      })
    }).then((stop) => { if (live) unsubscribe = stop; else stop() }).catch(() => {
      if (live) setError('无法监听权限变化，请重新打开详情。')
    })
    return () => { live = false; unsubscribe?.() }
  }, [client, plugin.pluginId])

  const rememberFocus = (origin: HTMLElement, fallback: HTMLElement | null, dialog: 'network' | 'clear' | null) => {
    focusReturn.current = {
      origin, fallback, dialog,
      dialogElement: dialog ? document.activeElement?.closest('[role="dialog"]') ?? null : null,
      dialogClosed: dialog === null,
      moved: false,
    }
  }

  useEffect(() => {
    const onFocus = (event: FocusEvent) => {
      const pending = focusReturn.current
      const target = event.target
      if (!pending || !(target instanceof HTMLElement) || target === document.body
        || target === pending.origin || target === pending.fallback || pending.dialogElement?.contains(target)) return
      pending.moved = true
    }
    document.addEventListener('focusin', onFocus)
    return () => document.removeEventListener('focusin', onFocus)
  }, [])

  useEffect(() => {
    const pending = focusReturn.current
    if (busy || !pending?.dialogClosed) return
    focusReturn.current = null
    const active = document.activeElement
    if (pending.moved || (active && active !== document.body && active !== pending.origin
      && active !== pending.fallback && !pending.dialogElement?.contains(active))) return
    // The modal and disabled state must both settle before returning keyboard position.
    const target = pending.origin.isConnected && !pending.origin.matches(':disabled') ? pending.origin : pending.fallback
    if (target?.isConnected && !target.matches(':disabled')) target.focus()
  }, [busy, closedDialog])

  const afterDialogClose = (dialog: 'network' | 'clear') => {
    if (focusReturn.current?.dialog !== dialog) return
    focusReturn.current.dialogClosed = true
    setClosedDialog((version) => version + 1)
  }

  const cancelNetworkConsent = () => {
    if (networkConsent) rememberFocus(networkConsent.origin, null, 'network')
    setNetworkConsent(null)
    release('detail')
  }

  const cancelClear = () => {
    if (clearOrigin.current) rememberFocus(clearOrigin.current, null, 'clear')
    setClearKey(null)
    release('detail')
  }
  const networkPermission = plugin.permissions.find(({ permission }) => permission === 'network.https')
  const secretSettings = plugin.settings.filter(({ definition }) => definition.type === 'secret')

  const mutate = async (operation: () => Promise<void>, success: () => void, failure: string) => {
    if (operationPending.current || !acquire('detail')) return
    operationPending.current = true
    setBusy(true)
    setError('')
    setNotice('')
    let committed = false
    try {
      await operation()
      committed = true
      success()
    } catch {
      setError(failure)
    }
    try {
      await reload()
    } catch {
      setError(`${committed ? '操作已完成。' : failure} 无法刷新当前状态，请返回列表刷新后核对。`)
    } finally {
      operationPending.current = false
      release('detail')
      setBusy(false)
    }
  }

  const saveSecret = (key: string, value: string | null, form?: HTMLFormElement) => {
    if (operationPending.current || (value !== null && !value.trim())) return
    const input = (form ?? clearOrigin.current?.closest('form'))?.querySelector<HTMLInputElement>('input[type="password"]') ?? null
    const active = document.activeElement
    const origin = value === null ? clearOrigin.current
      : active instanceof HTMLElement && form?.contains(active) ? active : input
    if (origin) rememberFocus(origin, input, value === null ? 'clear' : null)
    setClearKey(null)
    void mutate(
      () => client.savePublicPluginSettings({ input: { pluginId: plugin.pluginId, settings: {}, secrets: { [key]: value } } }),
      () => {
        setSecretDrafts((drafts) => ({ ...drafts, [key]: '' }))
        setNotice(value === null ? '已清除' : '已保存')
      },
      '保存未完成，落盘值可能已改变；已提交的更改不会回滚。本次操作失败后网络可能关闭，后续恢复可能使用实际落盘值；旧请求不会恢复。',
    )
  }

  const setNetworkAccess = (granted: boolean, origin?: HTMLElement) => {
    if (operationPending.current) return
    const consent = networkConsent
    if (consent) rememberFocus(consent.origin, null, 'network')
    else if (origin) rememberFocus(origin, null, null)
    setNetworkConsent(null)
    if (granted && !consent) return
    const expectedInventoryRevision = consent ? parseU64Decimal(consent.revision) : null
    if (granted && !expectedInventoryRevision) {
      setError('无法确认当前插件状态，请返回列表刷新后重试。')
      return
    }
    void mutate(
      () => client.setPublicPluginNetworkAccess({
        pluginId: plugin.pluginId,
        granted,
        ...(granted ? { expectedInventoryRevision: expectedInventoryRevision! } : {}),
      }),
      () => setNotice(granted ? '网络访问已开启' : '网络访问已关闭'),
      '无法更改网络授权。已刷新当前状态；开启前请重新核对绑定并确认，不会自动重试。',
    )
  }

  return (
    <section className="public-plugin-detail" aria-labelledby="public-plugin-detail-title" aria-busy={busy}>
      <fieldset className="public-detail-fields" disabled={busy || recoveryBusy || ordinaryBusy || networkConsent !== null || clearKey !== null}>
      <dl className="public-plugin-detail-list">
        <dt className="public-detail-name-term">启动</dt>
        <dd className="public-detail-name-value">
          {plugin.commands.length > 1 ? <select className="public-command-selector" data-command-selector aria-label="选择启动命令"
            value={selectedCommand?.commandId ?? ''} disabled={commandBlocked} onChange={(event) => void changeCommand(event.target.value)}>
            {plugin.commands.map((command) => <option key={command.commandId} value={command.commandId}>{command.commandId} · /{command.effectiveName}</option>)}
          </select> : null}
          {selectedCommand ? <StartupCommandEditor key={selectedCommand.commandId} command={selectedCommand}
            revision={`${inventoryRevision}:${hotkeyRevision}`} onRecordingPhase={onRecordingPhase}
            reload={reload} single={plugin.commands.length === 1} headerActionsTarget={headerActionsTarget}
            submitRef={submitCommand} onBlockedChange={setCommandBlocked} disabled={busy || recoveryBusy || ordinaryBusy || networkConsent !== null || clearKey !== null}
            operations={{
              name: (nameOverride) => client.setPublicPluginEffectiveName({ pluginId: plugin.pluginId, commandId: selectedCommand.commandId, nameOverride }),
              hotkey: (hotkey) => client.setPublicPluginCommandHotkey({ pluginId: plugin.pluginId, commandId: selectedCommand.commandId, hotkey }),
              favorite: (favorite) => client.setPublicPluginFavorite({ pluginId: plugin.pluginId, commandId: selectedCommand.commandId, favorite }),
            }} /> : <span>无启动命令</span>}
        </dd>
        {plugin.pluginId === 'com.uipilot.translate' ? <><dt>划词翻译快捷键</dt><dd><TranslationSelectionHotkey key={inventoryRevision} client={client} disabled={busy || recoveryBusy || ordinaryBusy || networkConsent !== null || clearKey !== null} onRecordingPhase={onRecordingPhase} /></dd></> : null}
        <dt>版本号</dt>
        <dd>{plugin.version}</dd>
        <dt>插件说明</dt>
        <dd className="public-detail-description">{plugin.description || '暂无插件说明'}</dd>
        <dt>权限列表</dt>
        <dd>
          {plugin.permissions.length ? (
            <>
              <ul className="public-plugin-detail-items">
                {plugin.permissions.map((permission) => (
                  <li key={permission.permission}>{permissionText(permission)}</li>
                ))}
              </ul>
              {plugin.permissions.some(({ permission }) => permission === 'clipboard.history.read') ? (
                <p className="public-clipboard-history-notice">{CLIPBOARD_HISTORY_NOTICE}</p>
              ) : null}
            </>
          ) : '无额外权限'}
        </dd>
        {plugin.network ? <><dt>网络 Host</dt>
        <dd>
          {plugin.network?.httpsHosts.length ? (
            <ul className="public-plugin-detail-items">
              {plugin.network.httpsHosts.map((host) => <li key={host}><code>{host}</code></li>)}
            </ul>
          ) : '无'}
        </dd></> : null}
        {networkPermission && plugin.network ? (
          <>
            <dt>网络访问权限</dt>
            <dd>
              <div className="public-network-access-header">
                <Switch aria-label="网络访问" checked={networkPermission.granted} disabled={busy || commandBlocked || !networkPermission.supported}
                  onChange={(granted, event) => { if (!acquire('detail')) return; if (granted) setNetworkConsent({ plugin, revision: inventoryRevision, origin: event.currentTarget }); else setNetworkAccess(false, event.currentTarget) }} />
                <span>{networkPermission.supported ? (networkPermission.granted ? '已开启' : '已关闭') : '暂不支持'}</span>
              </div>
              <CredentialBindings credentials={plugin.network.credentials} settings={plugin.settings} />
            </dd>
          </>
        ) : null}
        {secretSettings.length ? (
          <>
            <dt>凭据设置</dt>
            <dd>
              <p className="public-secret-help">仅显示配置状态，不代表可用于 Bearer 请求。留空不会删除已存凭据。</p>
              <div className="public-secret-settings">
                {secretSettings.map(({ definition, secretConfigured }) => {
                  const statusId = `public-secret-${definition.key}-status`
                  const draft = Object.prototype.hasOwnProperty.call(secretDrafts, definition.key) ? secretDrafts[definition.key]! : ''
                  return (
                    <form key={definition.key} className="public-secret-setting" onSubmit={(event) => { event.preventDefault(); saveSecret(definition.key, draft, event.currentTarget) }}>
                      <label htmlFor={`public-secret-${definition.key}`}>{definition.label} <code>{definition.key}</code></label>
                      <span id={statusId} className="public-secret-status">{secretConfigured ? '已配置' : '未配置'}</span>
                      <Input id={`public-secret-${definition.key}`} type="password" aria-label={definition.label} aria-describedby={statusId}
                        placeholder="输入新凭据" autoComplete="new-password" readOnly={busy || commandBlocked}
                        value={draft}
                        onChange={(event) => setSecretDrafts((drafts) => ({ ...drafts, [definition.key]: event.target.value }))} />
                      <div className="public-secret-actions">
                        <Button htmlType="submit" type="primary" aria-label={`保存 ${definition.label}`} disabled={!draft.trim() || commandBlocked} aria-disabled={busy}>保存</Button>
                        <Button danger aria-label={`清除 ${definition.label}`} disabled={busy || commandBlocked} onClick={(event) => { if (!acquire('detail')) return; clearOrigin.current = event.currentTarget; setClearKey(definition.key) }}>清除</Button>
                      </div>
                    </form>
                  )
                })}
              </div>
            </dd>
          </>
        ) : null}
        {plugin.permissions.some(({ permission }) => permission === 'selection.read') ? <><dt>读取选中文字</dt><dd><HostCredentialAccess selectedText key={plugin.pluginId} client={client} plugin={plugin} revision={inventoryRevision} reload={reload} disabled={busy || recoveryBusy || ordinaryBusy || commandBlocked || networkConsent !== null || clearKey !== null} acquire={() => { if (!acquire('ordinary')) return false; setOrdinaryBusy(true); return true }} release={() => { release('ordinary'); setOrdinaryBusy(false) }} /></dd></> : null}
        {plugin.permissions.some(({ permission }) => permission.startsWith('credentials.')) ? <><dt>主程序密钥</dt><dd><HostCredentialAccess key={plugin.pluginId} client={client} plugin={plugin} revision={inventoryRevision} reload={reload} disabled={busy || recoveryBusy || ordinaryBusy || commandBlocked || networkConsent !== null || clearKey !== null} acquire={() => { if (!acquire('ordinary')) return false; setOrdinaryBusy(true); return true }} release={() => { release('ordinary'); setOrdinaryBusy(false) }} /></dd></> : null}
        <dt>插件所在目录</dt>
        <dd className="public-detail-directory">{plugin.installationDirectory ?? '暂未提供插件目录'}</dd>
      </dl>
      </fieldset>
      <PublicPluginSettings key={`settings:${plugin.pluginId}`} client={client} plugin={plugin} revision={inventoryRevision} reload={reload}
        detailBusy={busy || recoveryBusy || networkConsent !== null || clearKey !== null || commandBlocked}
        acquire={() => { if (!acquire('ordinary')) return false; setOrdinaryBusy(true); return true }}
        release={() => { release('ordinary'); setOrdinaryBusy(false) }} />
      <PublicPluginRecovery key={plugin.pluginId} onRemoved={onRemoved} client={client} plugin={plugin} inventoryRevision={inventoryRevision} reload={reload} detailBusy={busy || ordinaryBusy || networkConsent !== null || clearKey !== null || commandBlocked} acquire={() => acquire('recovery')} onBusy={(next) => { setRecoveryBusy(next); if (!next) release('recovery') }} />
      <div className="public-detail-feedback" aria-live="polite">
        {busy ? <span role="status">正在保存…</span> : null}
        {notice ? <span role="status">{notice}</span> : null}
        {error ? <div className="plugin-row-error" role="alert">{error}</div> : null}
      </div>
      <Modal className="public-plugin-confirmation" destroyOnHidden title="开启网络访问" open={networkConsent !== null} onCancel={cancelNetworkConsent}
        focusable={{ focusTriggerAfterClose: false }} afterClose={() => afterDialogClose('network')}
        onOk={() => setNetworkAccess(true)} okText="确认开启" cancelText="取消"
        okButtonProps={{ 'aria-label': '确认开启网络访问', disabled: busy }}>
        <p>确认允许 UiPilot Host 访问以下 HTTPS 主机和完整凭据绑定：</p>
        <ul className="public-plugin-detail-items">{networkConsent?.plugin.network?.httpsHosts.map((host) => <li key={host}><code>{host}</code></li>)}</ul>
        <CredentialBindings credentials={networkConsent?.plugin.network?.credentials ?? []} settings={networkConsent?.plugin.settings} />
      </Modal>
      <Modal className="public-plugin-confirmation" destroyOnHidden title="清除凭据" open={clearKey !== null} onCancel={cancelClear}
        focusable={{ focusTriggerAfterClose: false }} afterClose={() => afterDialogClose('clear')}
        onOk={() => { if (clearKey) saveSecret(clearKey, null) }} okText="清除" cancelText="取消"
        okButtonProps={{ danger: true, disabled: busy, 'aria-label': `确认清除 ${secretSettings.find(({ definition }) => definition.key === clearKey)?.definition.label ?? ''}` }}>
        <p>清除后，需要重新配置此凭据才能使用相关请求。</p>
      </Modal>
    </section>
  )
}

export function PublicPluginPanel({ client, onOpenDetails, onOpenBuiltinDetails, removalOutcome }: PublicPluginPanelProps) {
  const builtins = useBuiltinFeatures(client)
  const epoch = useRef(0)
  const [inventory, setInventory] = useState<PublicPluginInventory | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [prepared, setPrepared] = useState<PublicPluginPrepareSummary | null>(null)
  const [busy, setBusy] = useState(false)
  const [pluginFilterDraft, setPluginFilterDraft] = useState('')
  const [pluginFilter, setPluginFilter] = useState('')
  const mutationPending = useRef(false)
  const preparedOrigin = useRef<HTMLElement | null>(null)
  const restorePreparedOrigin = useRef(false)

  useEffect(() => {
    if (busy || prepared || !restorePreparedOrigin.current) return
    restorePreparedOrigin.current = false
    const origin = preparedOrigin.current
    if (origin?.isConnected) origin.focus()
  }, [busy, prepared])

  const reload = useCallback(async () => {
    const owner = ++epoch.current
    setLoading(true)
    setError('')
    try {
      const next = await client.listPublicPlugins()
      if (owner !== epoch.current) return
      setInventory(next)
    } catch {
      if (owner !== epoch.current) return
      setError('无法加载公开插件。')
      throw new Error('inventory unavailable')
    } finally {
      if (owner === epoch.current) setLoading(false)
    }
  }, [client])

  useEffect(() => {
    void reload().catch(() => {})
    return () => { epoch.current += 1 }
  }, [reload])

  useEffect(() => {
    const timer = setTimeout(() => {
      setPluginFilter(pluginFilterDraft.trim().toLocaleLowerCase())
    }, PLUGIN_FILTER_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [pluginFilterDraft])

  const prepare = async (kind: 'archive' | 'developmentDirectory', origin: HTMLElement) => {
    if (mutationPending.current || prepared) return
    mutationPending.current = true
    const owner = epoch.current
    preparedOrigin.current = origin
    setBusy(true)
    setError('')
    try {
      const path = kind === 'archive' ? await client.selectPublicPluginArchive() : await client.selectPublicPluginDirectory()
      if (!path) {
        restorePreparedOrigin.current = true
        return
      }
      const next = await client.preparePublicPlugin({ source: { kind, path } })
      if (owner !== epoch.current) { void client.cancelPublicPlugin({ token: next.token }).catch(() => {}); return }
      setPrepared(next)
    } catch (failure) {
      if (owner !== epoch.current) return
      setError(publicPluginRecoveryError(failure))
      restorePreparedOrigin.current = true
    } finally {
      mutationPending.current = false
      if (owner === epoch.current) setBusy(false)
    }
  }

  const cancelPrepared = async () => {
    if (!prepared || mutationPending.current) return
    mutationPending.current = true
    setBusy(true)
    try { await client.cancelPublicPlugin({ token: prepared.token }) } catch { setError('无法取消确认；请重新选择，旧确认不会自动执行。') } finally {
      mutationPending.current = false
      restorePreparedOrigin.current = true
      setPrepared(null)
      setBusy(false)
    }
  }

  const commitPrepared = async () => {
    if (!prepared || mutationPending.current) return
    mutationPending.current = true
    const confirmation = prepared
    setBusy(true); setError(''); setNotice('')
    let committed = false
    let operationError = ''
    try {
      await client.commitPublicPlugin({ input: { token: confirmation.token, permissionGrants: installationPermissionGrants(confirmation.permissions) } })
      committed = true; setNotice('插件包已安装。')
    } catch (failure) { operationError = publicPluginRecoveryError(failure) }
    finally { restorePreparedOrigin.current = true; setPrepared(null) }
    try {
      await reload()
      setError(operationError)
    } catch { setError(committed ? '操作已完成。无法刷新当前状态，请刷新列表后核对。' : '操作未完成，无法刷新当前状态，请刷新列表并查看诊断。') }
    mutationPending.current = false; setBusy(false)
  }
  const filteredPlugins = pluginFilter && inventory
    ? inventory.items.filter((plugin) => [plugin.name, ...plugin.commands.map((command) => `/${command.effectiveName}`)].join(' ').toLocaleLowerCase().includes(pluginFilter))
    : inventory?.items ?? []

  const filteredBuiltins = builtins.items.filter((item) => !pluginFilter || [item.name, item.command].join(' ').toLocaleLowerCase().includes(pluginFilter))

  return (
    <section className="public-plugin-inventory" aria-labelledby="public-plugin-title">
      <div className="plugin-inventory-header">
        <h2 id="public-plugin-title">插件</h2>
        <div className="public-install-actions">
          <Tooltip title="选择插件包"><Button aria-label="选择插件包" icon={<Upload aria-hidden size={16} strokeWidth={1.8} />} disabled={busy} onClick={(event) => void prepare('archive', event.currentTarget)}><span className="public-install-label">选择插件包</span></Button></Tooltip>
          <Tooltip title="选择开发目录"><Button aria-label="选择开发目录" icon={<FolderOpen aria-hidden size={16} strokeWidth={1.8} />} disabled={busy} onClick={(event) => void prepare('developmentDirectory', event.currentTarget)}><span className="public-install-label">选择开发目录</span></Button></Tooltip>
          <Tooltip title="刷新">
            <Button
              aria-label="刷新"
              icon={<RefreshCw aria-hidden size={16} strokeWidth={1.8} />}
              disabled={busy || loading || builtins.busy}
              onClick={() => { void reload().catch(() => {}); void builtins.reload() }}
            />
          </Tooltip>
        </div>
      </div>
      <div className="public-plugin-filter">
        <Input
          aria-label="搜索插件名称或启动命令"
          placeholder="搜索插件名称或启动命令"
          allowClear
          value={pluginFilterDraft}
          disabled={loading && !inventory}
          onChange={(event) => setPluginFilterDraft(event.target.value)}
        />
      </div>
      {prepared ? (
        <PublicPluginConfirmation prepared={prepared} busy={busy} onCommit={() => void commitPrepared()} onCancel={() => void cancelPrepared()} />
      ) : null}
      {error ? <div className="plugin-list-state plugin-list-error" role="alert">{error}</div> : null}
      {removalOutcome ? <div className="plugin-list-state" role="status">{removalMessage(removalOutcome)}</div> : null}
      {notice ? <div className="plugin-list-state" role="status">{notice}</div> : null}
      {loading && !inventory ? <div className="plugin-list-state"><Spin size="small" /></div> : null}
      {inventory?.items.length === 0 ? <div className="plugin-list-state">未安装公开插件</div> : null}
      {inventory && (inventory.items.length > 0 || builtins.items.length > 0) && filteredBuiltins.length === 0 && filteredPlugins.length === 0 ? <div className="plugin-list-state">未找到匹配插件</div> : null}
      {builtins.error ? <div className="plugin-list-state plugin-list-error" role="alert">{builtins.error}</div> : null}
      {filteredBuiltins.length > 0 || filteredPlugins.length > 0 ? (
        <div className="public-plugin-list">
          {filteredBuiltins.map((item) => <BuiltinFeatureRow key={item.feature} item={item} busy={builtins.busy} onToggle={(enabled) => void builtins.toggle(item.feature, enabled)} onDetails={() => onOpenBuiltinDetails?.(item)} />)}
          {filteredPlugins.map((plugin) => (
            <PublicPluginRow
              key={`${plugin.pluginId}:${plugin.generation}`}
              client={client}
              plugin={plugin}
              reload={reload}
              onDetails={() => onOpenDetails?.(plugin, inventory!.revision, inventory!.hotkeyRevision)}
            />
          ))}
        </div>
      ) : null}
    </section>
  )
}
