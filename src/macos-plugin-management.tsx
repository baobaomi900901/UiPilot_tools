import { Button, Input, Modal, Select, Switch } from 'antd'
import { ArrowLeft, FolderOpen, RefreshCw, Upload } from 'lucide-react'
import { useEffect, useLayoutEffect, useImperativeHandle, useRef, useState, type KeyboardEvent, type Ref } from 'react'
import type { MacPluginManagementClient, MacPluginPreview, MacPluginSnapshot, OrdinarySetting, SettingValues } from './macos-plugin-management-client'
import './macos-plugin-management.css'

export interface MacPluginManagementHandle { requestLeave: (action: () => void) => void; back: () => boolean }
const errors: Record<string, string> = {
  pluginManagementStale: '插件配置已变化，请刷新后核对并重试。',
  pluginManagementCommitUncertain: '保存结果尚未确认，请刷新核对；不会自动重复提交。',
  pluginManagementInvalidPackage: '无法导入此插件，请检查插件包是否完整。',
  pluginManagementUnsupported: '此插件使用了当前 Mac 尚不支持的能力。',
  pluginManagementConsentRequired: '权限确认已失效，请重新选择插件。',
  pluginManagementCapacity: '插件或安装清单超过当前容量限制。',
  pluginManagementConflict: '启动命令与已安装插件冲突。',
  pluginManagementInvalidSetting: '设置值无效，请核对后重新保存。',
  pluginManagementStorage: '无法读取或保存插件配置，请刷新核对。',
  pluginManagementBusy: '另一项插件管理操作尚未结束。',
}
function message(error: unknown) {
  const code = typeof error === 'string' ? error : typeof error === 'object' && error && 'code' in error ? String(error.code) : ''
  return errors[code] ?? '插件管理暂不可用，请刷新后重试。'
}
function parseValue(d: OrdinarySetting, v: string | boolean): string | number | boolean {
  if (d.type === 'boolean' && typeof v === 'boolean') return v
  if (typeof v !== 'string') throw new Error()
  if (d.type === 'text' && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(v)) return v
  if (d.type === 'select' && d.options.some(o => o.value === v)) return v
  if (d.type === 'number' && v.trim()) {
    const n = Number(v)
    if (Number.isFinite(n) && (d.min === undefined || n >= d.min) && (d.max === undefined || n <= d.max)) return n
  }
  throw new Error()
}
export function MacPluginManagement({ client, ref }: { client?: MacPluginManagementClient; ref?: Ref<MacPluginManagementHandle> }) {
  const [snapshot, setSnapshot] = useState<MacPluginSnapshot | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [filter, setFilter] = useState('')
  const [drafts, setDrafts] = useState<Map<string, string | boolean>>(new Map())
  const [busy, setBusy] = useState(false); const pending = useRef(false)
  const [notice, setNotice] = useState(''); const [error, setError] = useState('')
  const [needsReview, setNeedsReview] = useState(false)
  const [prepared, setPrepared] = useState<MacPluginPreview | null>(null)
  const preparedRef = useRef<MacPluginPreview | null>(null)
  const [leave, setLeave] = useState<(() => void) | null>(null)
  const [removeOpen, setRemoveOpen] = useState(false)
  const live = useRef(false)
  const generation = useRef(0)
  const section = useRef<HTMLElement | null>(null)
  const heading = useRef<HTMLHeadingElement | null>(null)
  const previousSelection = useRef<string | null>(null)
  const owns = (epoch: number) => live.current && generation.current === epoch
  const plugin = snapshot?.plugins.find(p => p.pluginId === selected)
  useLayoutEffect(() => {
    if (selected) heading.current?.focus()
    else if (previousSelection.current) {
      const row = [...(section.current?.querySelectorAll<HTMLButtonElement>('[data-plugin-id]') ?? [])].find(item => item.dataset.pluginId === previousSelection.current)
      if (row) row.focus()
      else section.current?.querySelector<HTMLInputElement>('input[aria-label="搜索插件名称或启动命令"]')?.focus()
    }
    previousSelection.current = selected
  }, [selected])
  const begin = () => { if (pending.current || !client) return false; pending.current = true; setBusy(true); setError(''); setNotice(''); return true }
  const finish = (epoch: number) => { if (generation.current === epoch) { pending.current = false; if (live.current) setBusy(false) } }
  const publish = (value: MacPluginSnapshot) => { setSnapshot(value); setSelected(current => value.plugins.some(p => p.pluginId === current) ? current : null); setNeedsReview(false); setDrafts(new Map()) }
  const reload = async () => {
    if (!begin() || !client) return
    const epoch = generation.current
    try { const value = await client.list(); if (owns(epoch)) { publish(value); setNotice('已刷新') } }
    catch (e) { if (owns(epoch)) { setError(message(e)); setNeedsReview(true) } }
    finally { finish(epoch) }
  }
  useEffect(() => {
    generation.current++; live.current = true; pending.current = false; setSnapshot(null); setSelected(null); setDrafts(new Map()); setPrepared(null); setLeave(null); setRemoveOpen(false); setNeedsReview(false); void reload()
    return () => { generation.current++; live.current = false; const p = preparedRef.current; preparedRef.current = null; if (p) void client?.cancel({ token: p.token }).catch(() => {}) }
    // The client is a stable host-owned instance with a cross-mount operation queue.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client])
  const requestLeave = (action: () => void) => {
    if (pending.current) { setNotice('正在处理，请稍候。'); return }
    if (preparedRef.current) { setNotice('请先完成或取消安装确认。'); return }
    if (drafts.size) setLeave(() => action)
    else action()
  }
  useImperativeHandle(ref, () => ({ requestLeave, back: () => {
    if (leave || removeOpen || preparedRef.current || pending.current) return true
    if (selected) { requestLeave(() => { setDrafts(new Map()); setSelected(null) }); return true }
    return false
  } }))
  const mutate = async (operation: () => Promise<MacPluginSnapshot>, success: string) => {
    if (needsReview || !begin()) return
    const epoch = generation.current
    try { const value = await operation(); if (owns(epoch)) { publish(value); setNotice(success) } }
    catch (e) { if (owns(epoch)) { setError(message(e)); setNeedsReview(true) } }
    finally { finish(epoch) }
  }
  const choose = async (kind: 'archive' | 'directory') => {
    if (!snapshot || !client || needsReview || !begin()) return
    const epoch = generation.current
    try {
      const p = await client.prepare({ kind, expectedRevision: snapshot.revision })
      if (!owns(epoch)) { if (p) await client.cancel({ token: p.token }); return }
      preparedRef.current = p; setPrepared(p)
    } catch (e) { if (owns(epoch)) { setError(message(e)); setNeedsReview(true) } }
    finally { finish(epoch) }
  }
  const cancel = async () => {
    if (!prepared || !client || !begin()) return
    const epoch = generation.current
    const p = prepared; preparedRef.current = null; setPrepared(null)
    try { await client.cancel({ token: p.token }) } catch (e) { if (owns(epoch)) setError(message(e)) }
    finally { finish(epoch) }
  }
  const commit = async () => {
    if (!prepared || !client || !begin()) return
    const epoch = generation.current
    const p = prepared; preparedRef.current = null; setPrepared(null)
    try {
      const value = await client.commit({ token: p.token, expectedRevision: p.expectedRevision, approvedPermissions: p.requiredPermissions })
      if (owns(epoch)) { publish(value); setSelected(p.plugin.pluginId); setNotice('插件已安装，配置已保存；当前暂不能运行。') }
    } catch (e) { if (owns(epoch)) { setError(message(e)); setNeedsReview(true) } }
    finally { finish(epoch) }
  }
  let updates: SettingValues = {}; let invalid = false
  if (plugin) {
    try { updates = Object.fromEntries([...drafts].map(([key, value]) => { const d = plugin.settings.find(d => d.key === key); if (!d) throw new Error(); return [key, parseValue(d, value)] })) }
    catch { invalid = true }
  }
  const disabled = busy || needsReview || !snapshot || !client
  const list = snapshot?.plugins.filter(p => `${p.name} ${p.commands.map(c => c.name).join(' ')}`.toLocaleLowerCase().includes(filter.toLocaleLowerCase())) ?? []
  const installed = prepared ? snapshot?.plugins.find(p => p.pluginId === prepared.plugin.pluginId) : undefined
  const listKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (busy || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('button[data-plugin-id]:not(:disabled)')]
    const index = rows.findIndex(row => row === event.target)
    if (index < 0) return
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
    event.preventDefault(); event.stopPropagation(); rows[next]?.focus()
  }
  return <section ref={section} className="mac-plugin-management" aria-label="插件管理" aria-busy={busy}>
    <div className="mac-plugin-toolbar">
      {selected ? <Button aria-label="返回插件列表" icon={<ArrowLeft size={16} />} disabled={busy} onClick={() => requestLeave(() => { setDrafts(new Map()); setSelected(null) })} /> : <h2>插件</h2>}
      <Button aria-label="刷新插件列表" icon={<RefreshCw size={16} />} disabled={busy || !client} onClick={() => requestLeave(() => { setDrafts(new Map()); void reload() })}>刷新</Button>
      <Button icon={<FolderOpen size={16} />} disabled={disabled} onClick={() => requestLeave(() => void choose('directory'))}>导入开发目录</Button>
      <Button type="primary" icon={<Upload size={16} />} disabled={disabled} onClick={() => requestLeave(() => void choose('archive'))}>导入插件包</Button>
    </div>
    <p className="mac-plugin-runtime-note">可管理插件安装、设置和权限配置。当前 Mac 插件暂不能运行，启用及授权配置会保存。</p>
    <div className="mac-plugin-feedback" aria-live="polite">{busy ? <span role="status">正在处理…</span> : null}{notice ? <span role="status">{notice}</span> : null}{error ? <span role="alert">{error}</span> : null}{!client ? <span role="alert">插件管理暂不可用。</span> : null}</div>
    {!selected ? <>
      <Input aria-label="搜索插件名称或启动命令" placeholder="搜索插件名称或启动命令" value={filter} onChange={e => setFilter(e.target.value)} allowClear />
      <div className="mac-plugin-list" onKeyDown={listKeyDown}>
        {list.map(p => <button className="mac-plugin-row" key={p.pluginId} data-plugin-id={p.pluginId} disabled={busy} onClick={() => { setSelected(p.pluginId); setDrafts(new Map()); setNotice('') }}>
          <span><strong>{p.name}</strong><small>{p.commands.map(c => `/${c.name}`).join(' · ')}</small></span><span>{p.version}<small>{p.enabled ? '启用配置已保存' : '已停用'}</small></span>
        </button>)}
        {!list.length && snapshot ? <p className="mac-plugin-empty">{snapshot.plugins.length ? '没有匹配的插件。' : '尚未安装插件，请导入插件包或开发目录。'}</p> : null}
      </div>
    </> : plugin && snapshot && client ? <div className="mac-plugin-detail">
      <h2 ref={heading} tabIndex={-1}>{plugin.name}</h2><p>{plugin.description || '暂无插件说明'}</p><p className="mac-plugin-metadata">{plugin.pluginId} · {plugin.version}</p>
      <label className="mac-plugin-switch"><span>保存启用配置</span><Switch aria-label="保存启用配置" checked={plugin.enabled} disabled={disabled || drafts.size > 0} onChange={enabled => void mutate(() => client.enabled({ pluginId: plugin.pluginId, enabled, expectedRevision: snapshot.revision }), '启用配置已保存；当前暂不能运行。')} /></label>
      {plugin.permissions.length ? <section aria-label="插件权限"><h3>权限配置</h3>{plugin.permissions.map(p => <label key={p.permission} className="mac-plugin-switch"><span>允许写入剪贴板（运行可用后生效）</span><Switch aria-label="允许写入剪贴板" checked={p.granted} disabled={disabled || drafts.size > 0} onChange={granted => void mutate(() => client.permission({ pluginId: plugin.pluginId, permission: p.permission, granted, expectedRevision: snapshot.revision }), '权限配置已保存。')} /></label>)}</section> : null}
      {plugin.settings.length ? <form className="mac-plugin-settings" aria-label="普通设置" noValidate onSubmit={e => { e.preventDefault(); if (!invalid && drafts.size && !disabled) void mutate(() => client.settings({ pluginId: plugin.pluginId, expectedRevision: snapshot.revision, updates }), '设置已保存。') }}>
        <h3>普通设置</h3>
        {plugin.settings.map((d, index) => {
          const value = drafts.has(d.key) ? drafts.get(d.key)! : (Object.prototype.hasOwnProperty.call(plugin.values, d.key) ? plugin.values[d.key] : d.default)
          const id = `mac-plugin-setting-${index}`
          let bad = false; if (drafts.has(d.key)) try { parseValue(d, drafts.get(d.key)!) } catch { bad = true }
          const change = (value: string | boolean) => { setDrafts(current => new Map(current).set(d.key, value)); setNotice('') }
          return <div className="mac-plugin-field" key={d.key}><label htmlFor={id}>{d.label}</label>
            {d.type === 'boolean' ? <Switch id={id} aria-label={d.label} checked={value === true} disabled={disabled} onChange={change} />
              : d.type === 'select' ? <Select id={id} aria-label={d.label} aria-invalid={bad} value={value === undefined ? undefined : String(value)} placeholder="请选择" disabled={disabled} options={[...d.options]} onChange={change} />
              : d.type === 'text' ? <Input.TextArea id={id} aria-label={d.label} aria-invalid={bad} value={value === undefined ? '' : String(value)} placeholder="输入文本" rows={2} disabled={disabled} onChange={e => change(e.target.value)} />
              : <Input id={id} aria-label={d.label} aria-invalid={bad} value={value === undefined ? '' : String(value)} placeholder="输入数字" inputMode="decimal" disabled={disabled} onChange={e => change(e.target.value)} />}
            {bad ? <span role="alert">请检查此设置的类型、范围或选项。</span> : null}
          </div>
        })}
        <div className="mac-plugin-form-actions"><Button htmlType="submit" type="primary" disabled={disabled || !drafts.size || invalid}>保存设置</Button><Button disabled={busy || !drafts.size} onClick={() => { setDrafts(new Map()); setNotice('已取消修改') }}>取消修改</Button></div>
      </form> : null}
      <Button danger disabled={disabled} onClick={() => requestLeave(() => { setDrafts(new Map()); setRemoveOpen(true) })}>移除插件</Button>
    </div> : <p>插件已不在当前安装清单中，请返回列表。</p>}
    <Modal title="放弃未保存修改？" open={!!leave} onCancel={() => setLeave(null)} footer={<><Button onClick={() => setLeave(null)}>取消</Button><Button danger onClick={() => { const action = leave; setLeave(null); setDrafts(new Map()); action?.() }}>放弃修改</Button></>}><p>尚未保存的普通设置将被放弃。</p></Modal>
    <Modal title={installed ? '替换已安装插件' : '安装插件'} open={!!prepared} onCancel={() => void cancel()} closable={!busy} mask={{ closable: !busy }} keyboard={!busy} footer={<><Button disabled={busy} onClick={() => void cancel()}>取消</Button><Button type="primary" disabled={busy} onClick={() => void commit()}>{installed ? '确认替换' : '确认安装'}</Button></>}>
      {prepared ? <><p>{prepared.plugin.name}</p>{installed ? <p>当前版本：{installed.version}</p> : null}<p>目标版本：{prepared.plugin.version}</p><p>当前 Mac 暂不能运行此插件。</p>{prepared.requiredPermissions.length ? <p>本次确认将保存新增授权：写入剪贴板（运行可用后生效）。</p> : <p>本次安装没有新增权限。</p>}</> : null}
    </Modal>
    <Modal title="移除插件" open={removeOpen} onCancel={() => setRemoveOpen(false)} footer={<><Button onClick={() => setRemoveOpen(false)}>取消</Button><Button danger disabled={disabled} onClick={() => { setRemoveOpen(false); if (plugin && snapshot && client) void mutate(() => client.remove({ pluginId: plugin.pluginId, expectedRevision: snapshot.revision }), '安装和普通设置已移除，插件存储数据已保留。') }}>移除安装和设置</Button></>}><p>移除 {plugin?.name} 的安装和普通设置，保留插件存储数据。再次安装时需要重新确认权限。</p></Modal>
  </section>
}
