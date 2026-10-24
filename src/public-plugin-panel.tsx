import { Button, Input, Modal, Popconfirm, Spin, Switch, Tooltip } from 'antd'
import { FolderOpen, RefreshCw, Undo2, Upload } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { parseU64Decimal } from './protocol'
import { PluginIcon } from './plugin-icon'
import type {
  LauncherClient,
  PublicPermission,
  PublicPluginInventory,
  PluginNetworkCredential,
  PublicSettingView,
  PublicPluginInventoryItem,
  PublicPluginPrepareSummary,
} from './protocol'

interface PublicPluginPanelProps {
  client: LauncherClient
  onOpenDetails?: (plugin: PublicPluginInventoryItem, revision: string) => void
}

const CLEANUP_PENDING_MESSAGE = '插件已卸载，数据清理将在下次启动时重试'
const CREDENTIAL_NOTICE = '此凭据会发送给该服务；服务响应可能包含其回显。'
const PLUGIN_FILTER_DEBOUNCE_MS = 150
const CLIPBOARD_HISTORY_NOTICE = '授权后，UiPilot 运行、插件启用且权限有效期间，会在本机记录该插件可用的剪贴板历史摘要；不会自动识别密码或敏感来源。'

function permissionLabel(permission: PublicPermission): string {
  switch (permission) {
    case 'clipboard.write':
      return '写入剪贴板'
    case 'clipboard.read':
      return '读取剪贴板（保留，暂不支持）'
    case 'clipboard.history.read':
      return '剪贴板历史读取'
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
  return `${permissionLabel(permission)} · ${permission}`
}

function hasClipboardHistoryRead(permissions: readonly PublicPermission[]) {
  return permissions.includes('clipboard.history.read')
}

function isCleanupPending(error: unknown): boolean {
  return typeof error === 'object'
    && error !== null
    && 'code' in error
    && error.code === 'dataCleanupPending'
}

function PublicPluginRow({
  client,
  plugin,
  reload,
  onCleanupPending,
  onDetails,
}: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  reload: () => Promise<void>
  onCleanupPending: () => Promise<void>
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
    } catch (caught) {
      if (isCleanupPending(caught)) await onCleanupPending()
      else setError('操作不可用，请重试。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <article className="plugin-item public-plugin-item">
      <div className="public-plugin-icon-cell">
        <PluginIcon iconUrl={plugin.iconUrl} size={36} />
      </div>
      <div className="plugin-item-main public-plugin-main">
        <div className="public-plugin-summary-copy">
          <div className="plugin-title-line">
            <h3>{plugin.name}</h3>
            <span>·</span>
            <span>v{plugin.version}</span>
            <span className="public-plugin-command">{plugin.commands.map((command) => <code key={command.commandId}>/{command.effectiveName}</code>)}</span>
            {plugin.fault ? <span className="plugin-fault-label">运行故障</span> : null}
          </div>
          {plugin.description ? <p className="plugin-description">{plugin.description}</p> : null}
        </div>
        <div className="public-plugin-row-links">
          <Button type="link" size="small" aria-label="查看插件详情" onClick={onDetails}>
            详情
          </Button>
          <Popconfirm
            title="卸载插件"
            description={(
              <div className="public-uninstall-options">
                <Button
                  danger
                  disabled={busy}
                  onClick={() => void mutate(() => client.uninstallPublicPlugin({
                    pluginId: plugin.pluginId,
                    retainData: false,
                  }))}
                >
                  全部卸载
                </Button>
                <Button
                  disabled={busy}
                  onClick={() => void mutate(() => client.uninstallPublicPlugin({
                    pluginId: plugin.pluginId,
                    retainData: true,
                  }))}
                >
                  保留数据卸载
                </Button>
              </div>
            )}
            okButtonProps={{ style: { display: 'none' } }}
            cancelText="取消"
          >
            <Button type="link" size="small" danger aria-label="卸载插件" disabled={busy}>
              删除
            </Button>
          </Popconfirm>
        </div>
        {error ? <div className="plugin-row-error" role="alert">{error}</div> : null}
      </div>
      <div className="plugin-actions public-plugin-actions">
        <Switch
          checked={plugin.enabled}
          disabled={busy}
          onChange={(enabled) => void mutate(() => client.setPublicPluginEnabled({
            pluginId: plugin.pluginId,
            enabled,
          }))}
        />
      </div>
    </article>
  )
}

function permissionText({ permission, supported, granted }: PublicPluginInventoryItem['permissions'][number]) {
  return `${permissionSummary(permission)} · ${supported ? (granted ? '已授权' : '未授权') : '不支持'}`
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

export function PublicPluginDetail({ client, plugin, inventoryRevision, reload }: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  inventoryRevision: string
  reload: () => Promise<void>
}) {
  const [busy, setBusy] = useState(false)
  const operationPending = useRef(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [names, setNames] = useState<Record<string, string>>(() => Object.fromEntries(plugin.commands.map((command) => [command.commandId, command.effectiveName])))
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
  }

  const cancelClear = () => {
    if (clearOrigin.current) rememberFocus(clearOrigin.current, null, 'clear')
    setClearKey(null)
  }
  const networkPermission = plugin.permissions.find(({ permission }) => permission === 'network.https')
  const secretSettings = plugin.settings.filter(({ definition }) => definition.type === 'secret')

  const mutate = async (operation: () => Promise<void>, success: () => void, failure: string) => {
    if (operationPending.current) return
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

  const saveName = async (commandId: string, nameOverride: string | null) => {
    if (busy) return
    const command = plugin.commands.find((command) => command.commandId === commandId)
    if (!command || nameOverride === command.nameOverride) return
    const nextName = nameOverride ?? command.defaultName
    setBusy(true)
    setError('')
    try {
      await client.setPublicPluginEffectiveName({ pluginId: plugin.pluginId, commandId, nameOverride })
      setNames((current) => ({ ...current, [commandId]: nextName }))
      await reload()
    } catch {
      setError('无法保存启动键，请重试。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="public-plugin-detail" aria-labelledby="public-plugin-detail-title" aria-busy={busy}>
      <dl className="public-plugin-detail-list">
        <dt className="public-detail-name-term">启动键</dt>
        <dd className="public-detail-name-value">
          {plugin.commands.map((command) => <div className="public-detail-name-control" key={command.commandId}>
            <Input
              aria-label={plugin.commands.length === 1 ? "启动键" : `启动键 ${command.commandId}`}
              value={Object.prototype.hasOwnProperty.call(names, command.commandId) ? names[command.commandId] : command.effectiveName}
              disabled={busy}
              onChange={(event) => setNames((current) => ({ ...current, [command.commandId]: event.target.value }))}
              onBlur={(event) => void saveName(command.commandId, event.currentTarget.value)}
            />
            <Tooltip title="恢复默认">
              <Button
                aria-label={plugin.commands.length === 1 ? "恢复默认启动键" : `恢复默认启动键 ${command.commandId}`}
                icon={<Undo2 aria-hidden size={16} strokeWidth={1.8} />}
                disabled={busy}
                onClick={() => void saveName(command.commandId, null)}
              />
            </Tooltip>
          </div>)}
        </dd>
        <dt>版本号</dt>
        <dd>{plugin.version}</dd>
        <dt>插件说明</dt>
        <dd>{plugin.description || '暂无插件说明'}</dd>
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
        <dt>网络 Host</dt>
        <dd>
          {plugin.network?.httpsHosts.length ? (
            <ul className="public-plugin-detail-items">
              {plugin.network.httpsHosts.map((host) => <li key={host}><code>{host}</code></li>)}
            </ul>
          ) : '无'}
        </dd>
        {networkPermission && plugin.network ? (
          <>
            <dt>网络访问</dt>
            <dd>
              <div className="public-network-access-header">
                <Switch aria-label="网络访问" checked={networkPermission.granted} disabled={busy || !networkPermission.supported}
                  onChange={(granted, event) => granted ? setNetworkConsent({ plugin, revision: inventoryRevision, origin: event.currentTarget }) : setNetworkAccess(false, event.currentTarget)} />
                <span>{networkPermission.supported ? (networkPermission.granted ? '已授权' : '未授权') : '不支持'}</span>
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
                        placeholder="输入新凭据" autoComplete="new-password" readOnly={busy}
                        value={draft}
                        onChange={(event) => setSecretDrafts((drafts) => ({ ...drafts, [definition.key]: event.target.value }))} />
                      <div className="public-secret-actions">
                        <Button htmlType="submit" type="primary" aria-label={`保存 ${definition.label}`} disabled={!draft.trim()} aria-disabled={busy}>保存</Button>
                        <Button danger aria-label={`清除 ${definition.label}`} disabled={busy} onClick={(event) => { clearOrigin.current = event.currentTarget; setClearKey(definition.key) }}>清除</Button>
                      </div>
                    </form>
                  )
                })}
              </div>
            </dd>
          </>
        ) : null}
        <dt>插件所在目录</dt>
        <dd>暂未提供插件目录</dd>
      </dl>
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

export function PublicPluginPanel({ client, onOpenDetails }: PublicPluginPanelProps) {
  const epoch = useRef(0)
  const [inventory, setInventory] = useState<PublicPluginInventory | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [prepared, setPrepared] = useState<PublicPluginPrepareSummary | null>(null)
  const [busy, setBusy] = useState(false)
  const [pluginFilterDraft, setPluginFilterDraft] = useState('')
  const [pluginFilter, setPluginFilter] = useState('')
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
    } finally {
      if (owner === epoch.current) setLoading(false)
    }
  }, [client])

  const handleCleanupPending = useCallback(async () => {
    setNotice(CLEANUP_PENDING_MESSAGE)
    await reload()
  }, [reload])

  useEffect(() => {
    void reload()
    return () => { epoch.current += 1 }
  }, [reload])

  useEffect(() => {
    const timer = setTimeout(() => {
      setPluginFilter(pluginFilterDraft.trim().toLocaleLowerCase())
    }, PLUGIN_FILTER_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [pluginFilterDraft])

  const prepare = async (kind: 'archive' | 'developmentDirectory', origin: HTMLElement) => {
    if (busy) return
    preparedOrigin.current = origin
    setBusy(true)
    setError('')
    try {
      const path = kind === 'archive' ? await client.selectPublicPluginArchive() : await client.selectPublicPluginDirectory()
      if (!path) {
        restorePreparedOrigin.current = true
        return
      }
      setPrepared(await client.preparePublicPlugin({ source: { kind, path } }))
    } catch {
      setError('无法准备插件安装。')
      restorePreparedOrigin.current = true
    } finally {
      setBusy(false)
    }
  }

  const cancelPrepared = async () => {
    if (!prepared || busy) return
    setBusy(true)
    try { await client.cancelPublicPlugin({ token: prepared.token }) } finally {
      restorePreparedOrigin.current = true
      setPrepared(null)
      setBusy(false)
    }
  }

  const commitPrepared = async () => {
    if (!prepared || busy) return
    setBusy(true)
    setError('')
    try {
      await client.commitPublicPlugin({ input: { token: prepared.token, permissionGrants: prepared.permissions as PublicPermission[] } })
      restorePreparedOrigin.current = true
      setPrepared(null)
      await reload()
    } catch {
      setError('无法安装公开插件。')
    } finally {
      setBusy(false)
    }
  }
  const filteredPlugins = pluginFilter && inventory
    ? inventory.items.filter((plugin) => plugin.name.toLocaleLowerCase().includes(pluginFilter))
    : inventory?.items ?? []

  return (
    <section className="public-plugin-inventory" aria-labelledby="public-plugin-title">
      <div className="plugin-inventory-header">
        <h2 id="public-plugin-title">公开插件</h2>
        <div className="public-install-actions">
          <Tooltip title="选择插件包"><Button aria-label="选择插件包" icon={<Upload aria-hidden size={16} strokeWidth={1.8} />} disabled={busy} onClick={(event) => void prepare('archive', event.currentTarget)} /></Tooltip>
          <Tooltip title="选择开发目录"><Button aria-label="选择开发目录" icon={<FolderOpen aria-hidden size={16} strokeWidth={1.8} />} disabled={busy} onClick={(event) => void prepare('developmentDirectory', event.currentTarget)} /></Tooltip>
          <Tooltip title="刷新">
            <Button
              aria-label="刷新"
              icon={<RefreshCw aria-hidden size={16} strokeWidth={1.8} />}
              disabled={busy || loading}
              onClick={() => void reload()}
            />
          </Tooltip>
        </div>
      </div>
      <div className="public-plugin-filter">
        <Input
          aria-label="筛选插件名称"
          placeholder="筛选插件名称"
          allowClear
          value={pluginFilterDraft}
          disabled={loading && !inventory}
          onChange={(event) => setPluginFilterDraft(event.target.value)}
        />
      </div>
      {prepared ? (
        <div className="public-prepare" role="status">
          <PluginIcon iconUrl={prepared.iconUrl} size={32} />
          <strong>{prepared.name}</strong><span>{prepared.version}</span>
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
          <Button type="primary" loading={busy} onClick={() => void commitPrepared()}>{prepared.isUpdate ? '确认更新' : '确认安装'}</Button>
          <Button aria-label="取消安装" disabled={busy} onClick={() => void cancelPrepared()}>取消</Button>
        </div>
      ) : null}
      {error ? <div className="plugin-list-state plugin-list-error" role="alert">{error}</div> : null}
      {notice ? <div className="plugin-list-state" role="status">{notice}</div> : null}
      {loading && !inventory ? <div className="plugin-list-state"><Spin size="small" /></div> : null}
      {inventory?.items.length === 0 ? <div className="plugin-list-state">未安装公开插件</div> : null}
      {inventory && inventory.items.length > 0 && filteredPlugins.length === 0 ? <div className="plugin-list-state">未找到匹配插件</div> : null}
      {filteredPlugins.map((plugin) => (
        <PublicPluginRow
          key={`${plugin.pluginId}:${plugin.generation}`}
          client={client}
          plugin={plugin}
          reload={reload}
          onCleanupPending={handleCleanupPending}
          onDetails={() => onOpenDetails?.(plugin, inventory!.revision)}
        />
      ))}
    </section>
  )
}
