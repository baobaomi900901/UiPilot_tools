import {
  DeleteOutlined,
  FolderOpenOutlined,
  SaveOutlined,
  UndoOutlined,
  UploadOutlined,
} from '@ant-design/icons'
import { Button, Checkbox, Form, Input, InputNumber, Popconfirm, Select, Spin, Switch, Tooltip } from 'antd'
import { useCallback, useEffect, useRef, useState } from 'react'

import type {
  LauncherClient,
  PublicPermission,
  PublicPluginInventory,
  PublicPluginInventoryItem,
  PublicPluginPrepareSummary,
  PublicSettingView,
} from './protocol'

interface PublicPluginPanelProps {
  client: LauncherClient
}

type PlainSettingValue = string | number | boolean

function initialSetting(setting: PublicSettingView): PlainSettingValue {
  if (setting.value !== undefined) return setting.value
  if (setting.definition.type !== 'secret' && setting.definition.default !== undefined) return setting.definition.default
  if (setting.definition.type === 'boolean') return false
  if (setting.definition.type === 'number') return 0
  if (setting.definition.type === 'select') return setting.definition.options[0]?.value ?? ''
  return ''
}

function PublicPluginRow({
  client,
  plugin,
  reload,
}: {
  client: LauncherClient
  plugin: PublicPluginInventoryItem
  reload: () => Promise<void>
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [name, setName] = useState(plugin.effectiveName)
  const [settings, setSettings] = useState<Record<string, PlainSettingValue>>(() =>
    Object.fromEntries(plugin.settings.filter(({ definition }) => definition.type !== 'secret').map((setting) => [setting.definition.key, initialSetting(setting)])),
  )
  const [secrets, setSecrets] = useState<Record<string, string>>({})
  const [clearSecrets, setClearSecrets] = useState<Record<string, boolean>>({})

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

  const secretUpdates = (): Record<string, string | null> => {
    const updates: Record<string, string | null> = {}
    for (const { definition } of plugin.settings) {
      if (definition.type !== 'secret') continue
      if (clearSecrets[definition.key]) updates[definition.key] = null
      else if (secrets[definition.key]) updates[definition.key] = secrets[definition.key]
    }
    return updates
  }
  const settingControl = ({ definition, secretConfigured }: PublicSettingView) => {
    if (definition.type === 'secret') {
      return (
        <div className="public-secret-control" key={definition.key}>
          <Input.Password
            aria-label={definition.label}
            value={secrets[definition.key] ?? ''}
            placeholder={secretConfigured ? '已配置' : ''}
            disabled={busy || clearSecrets[definition.key]}
            onChange={(event) => setSecrets((current) => ({ ...current, [definition.key]: event.target.value }))}
          />
          <Checkbox
            checked={clearSecrets[definition.key] ?? false}
            disabled={busy || !secretConfigured}
            onChange={(event) => setClearSecrets((current) => ({ ...current, [definition.key]: event.target.checked }))}
          >
            清除
          </Checkbox>
        </div>
      )
    }
    const value = settings[definition.key] ?? initialSetting({ definition })
    if (definition.type === 'boolean') {
      return <Switch aria-label={definition.label} checked={Boolean(value)} disabled={busy} onChange={(checked) => setSettings((current) => ({ ...current, [definition.key]: checked }))} />
    }
    if (definition.type === 'number') {
      return (
        <InputNumber
          aria-label={definition.label}
          value={Number(value)}
          min={definition.min}
          max={definition.max}
          step={definition.step}
          disabled={busy}
          onChange={(next) => next !== null && setSettings((current) => ({ ...current, [definition.key]: next }))}
        />
      )
    }
    if (definition.type === 'select') {
      return (
        <Select
          aria-label={definition.label}
          value={String(value)}
          options={[...definition.options]}
          disabled={busy}
          onChange={(next) => setSettings((current) => ({ ...current, [definition.key]: next }))}
        />
      )
    }
    return <Input aria-label={definition.label} value={String(value)} disabled={busy} onChange={(event) => setSettings((current) => ({ ...current, [definition.key]: event.target.value }))} />
  }

  return (
    <article className="plugin-item public-plugin-item">
      <div className="plugin-item-main">
        <div className="plugin-title-line">
          <h3>{plugin.name}</h3>
          <code>/{plugin.effectiveName}</code>
          <span>{plugin.version}</span>
          <span>{plugin.enabled ? '已启用' : '已禁用'}</span>
          {plugin.fault ? <span>运行故障</span> : null}
        </div>
        <div className="plugin-version-list">{plugin.pluginId}</div>
        {plugin.description ? <p className="plugin-description">{plugin.description}</p> : null}
        <div className="public-permissions">
          {plugin.permissions.map((permission) => (
            <span key={permission.permission}>{permission.permission} · {permission.supported ? (permission.granted ? '已授权' : '未授权') : '不支持'}</span>
          ))}
        </div>
        <Form layout="vertical" size="small" className="public-plugin-form">
          <Form.Item label="启动名称">
            <div className="public-name-control">
              <Input value={name} disabled={busy} onChange={(event) => setName(event.target.value)} />
              <Tooltip title="保存名称"><Button icon={<SaveOutlined />} disabled={busy} onClick={() => void mutate(() => client.setPublicPluginEffectiveName({ pluginId: plugin.pluginId, nameOverride: name }))} /></Tooltip>
              <Tooltip title="恢复默认"><Button icon={<UndoOutlined />} disabled={busy} onClick={() => void mutate(() => client.setPublicPluginEffectiveName({ pluginId: plugin.pluginId, nameOverride: null }))} /></Tooltip>
            </div>
          </Form.Item>
          {plugin.settings.map((setting) => (
            <Form.Item key={setting.definition.key} label={setting.definition.label}>{settingControl(setting)}</Form.Item>
          ))}
          {plugin.settings.length ? (
            <Button
              icon={<SaveOutlined />}
              loading={busy}
              onClick={() => void mutate(() => client.savePublicPluginSettings({
                input: {
                  pluginId: plugin.pluginId,
                  settings,
                  secrets: secretUpdates(),
                },
              }))}
            >
              保存设置
            </Button>
          ) : null}
        </Form>
        {error ? <div className="plugin-row-error" role="alert">{error}</div> : null}
      </div>
      <div className="plugin-actions public-plugin-actions">
        <Switch checked={plugin.enabled} disabled={busy} onChange={(enabled) => void mutate(() => client.setPublicPluginEnabled({ pluginId: plugin.pluginId, enabled }))} />
        <Popconfirm title="卸载并删除数据？" okText="删除" cancelText="取消" onConfirm={() => void mutate(() => client.uninstallPublicPlugin({ pluginId: plugin.pluginId, retainData: false }))}>
          <Tooltip title="卸载并删除数据"><Button danger icon={<DeleteOutlined />} disabled={busy} /></Tooltip>
        </Popconfirm>
        <Popconfirm title="卸载但保留数据？" okText="保留" cancelText="取消" onConfirm={() => void mutate(() => client.uninstallPublicPlugin({ pluginId: plugin.pluginId, retainData: true }))}>
          <Button disabled={busy}>保留数据卸载</Button>
        </Popconfirm>
      </div>
    </article>
  )
}

export function PublicPluginPanel({ client }: PublicPluginPanelProps) {
  const epoch = useRef(0)
  const [inventory, setInventory] = useState<PublicPluginInventory | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [prepared, setPrepared] = useState<PublicPluginPrepareSummary | null>(null)
  const [busy, setBusy] = useState(false)

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

  useEffect(() => {
    void reload()
    return () => { epoch.current += 1 }
  }, [reload])

  const prepare = async (kind: 'archive' | 'developmentDirectory') => {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      const path = kind === 'archive' ? await client.selectPublicPluginArchive() : await client.selectPublicPluginDirectory()
      if (!path) return
      setPrepared(await client.preparePublicPlugin({ source: { kind, path } }))
    } catch {
      setError('无法准备插件安装。')
    } finally {
      setBusy(false)
    }
  }

  const cancelPrepared = async () => {
    if (!prepared || busy) return
    setBusy(true)
    try { await client.cancelPublicPlugin({ token: prepared.token }) } finally {
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
      setPrepared(null)
      await reload()
    } catch {
      setError('无法安装公开插件。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="public-plugin-inventory" aria-labelledby="public-plugin-title">
      <div className="plugin-inventory-header">
        <h2 id="public-plugin-title">公开插件</h2>
        <div className="public-install-actions">
          <Tooltip title="选择插件包"><Button icon={<UploadOutlined />} disabled={busy} onClick={() => void prepare('archive')} /></Tooltip>
          <Tooltip title="选择开发目录"><Button icon={<FolderOpenOutlined />} disabled={busy} onClick={() => void prepare('developmentDirectory')} /></Tooltip>
          <Button disabled={busy || loading} onClick={() => void reload()}>刷新</Button>
        </div>
      </div>
      {prepared ? (
        <div className="public-prepare" role="status">
          <strong>{prepared.name}</strong><span>{prepared.version}</span>
          <span>{prepared.permissions.join('、') || '无额外权限'}</span>
          <Button type="primary" loading={busy} onClick={() => void commitPrepared()}>确认安装</Button>
          <Button disabled={busy} onClick={() => void cancelPrepared()}>取消</Button>
        </div>
      ) : null}
      {error ? <div className="plugin-list-state plugin-list-error" role="alert">{error}</div> : null}
      {loading && !inventory ? <div className="plugin-list-state"><Spin size="small" /></div> : null}
      {inventory?.items.length === 0 ? <div className="plugin-list-state">未安装公开插件</div> : null}
      {inventory?.items.map((plugin) => <PublicPluginRow key={`${plugin.pluginId}:${plugin.generation}`} client={client} plugin={plugin} reload={reload} />)}
    </section>
  )
}
