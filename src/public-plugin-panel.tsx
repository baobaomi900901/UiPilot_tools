import { Button, Input, Popconfirm, Spin, Switch, Tooltip } from 'antd'
import { FolderOpen, RefreshCw, Undo2, Upload } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { PluginIcon } from './plugin-icon'
import type {
  LauncherClient,
  PublicPermission,
  PublicPluginInventory,
  PublicPluginInventoryItem,
  PublicPluginPrepareSummary,
} from './protocol'

interface PublicPluginPanelProps {
  client: LauncherClient
  onOpenDetails?: (plugin: PublicPluginInventoryItem) => void
}

const CLEANUP_PENDING_MESSAGE = '插件已卸载，数据清理将在下次启动时重试'
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
            <span className="public-plugin-command"><code>/{plugin.effectiveName}</code></span>
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

export function PublicPluginDetail({
  client,
  plugin,
  reload = async () => undefined,
}: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  reload?: () => Promise<void>
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [name, setName] = useState(plugin.effectiveName)

  const saveName = async (nameOverride: string | null) => {
    if (busy) return
    const nextName = nameOverride ?? plugin.defaultName
    if (nameOverride !== null && nameOverride === plugin.effectiveName) return
    setBusy(true)
    setError('')
    try {
      await client.setPublicPluginEffectiveName({ pluginId: plugin.pluginId, nameOverride })
      setName(nextName)
      await reload()
    } catch {
      setError('无法保存启动键，请重试。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="public-plugin-detail" aria-labelledby="public-plugin-detail-title">
      <dl className="public-plugin-detail-list">
        <dt className="public-detail-name-term">启动键</dt>
        <dd className="public-detail-name-value">
          <div className="public-detail-name-control">
            <Input
              aria-label="启动键"
              value={name}
              disabled={busy}
              onChange={(event) => setName(event.target.value)}
              onBlur={(event) => void saveName(event.currentTarget.value)}
            />
            <Tooltip title="恢复默认">
              <Button
                aria-label="恢复默认启动键"
                icon={<Undo2 aria-hidden size={16} strokeWidth={1.8} />}
                disabled={busy}
                onClick={() => void saveName(null)}
              />
            </Tooltip>
          </div>
          {error ? <div className="plugin-row-error" role="alert">{error}</div> : null}
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
        <dt>插件所在目录</dt>
        <dd>暂未提供插件目录</dd>
      </dl>
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
          onDetails={() => onOpenDetails?.(plugin)}
        />
      ))}
    </section>
  )
}
