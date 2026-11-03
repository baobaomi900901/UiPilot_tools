import { Button, Dropdown, Form, Input, Modal, Spin, Switch, Tooltip } from 'antd'
import type { InputRef } from 'antd'
import { ArrowLeft, BrainCircuit, KeyRound, Languages, Plus, RefreshCw, ScanText, Search, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react'
import type { Ref } from 'react'
import type { CredentialClient, CredentialEntry, CredentialInventory, CredentialService } from './credential-protocol'
import './credential-settings.css'

const groups = [
  { label: '翻译', english: 'Translation', services: ['youdaoTranslation', 'baiduTranslation'] },
  { label: '文字识别', english: 'OCR', services: ['baiduOcr'] },
  { label: '大模型', english: 'LLM', services: ['deepseek'] },
] as const
const services: Record<CredentialService, { name: string; fields: { key: string; label: string }[]; hint: string }> = {
  youdaoTranslation: { name: '有道翻译', fields: [{ key: 'appId', label: 'App ID' }, { key: 'appKey', label: 'App Key' }], hint: '通过一条最小翻译请求验证凭据。' },
  baiduTranslation: { name: '百度翻译', fields: [{ key: 'appId', label: 'App ID' }, { key: 'appKey', label: '密钥' }], hint: '通过一条最小翻译请求验证凭据。' },
  baiduOcr: { name: '百度 OCR', fields: [{ key: 'appId', label: 'App ID' }, { key: 'apiKey', label: 'API Key' }, { key: 'secretKey', label: 'Secret Key' }], hint: '验证 API Key / Secret Key 的令牌认证及 OCR 权限范围，不校验 App ID 与密钥的对应关系，也不验证识别额度。' },
  deepseek: { name: 'DeepSeek', fields: [{ key: 'apiKey', label: 'API Key' }], hint: '使用 DeepSeek 官方 API（https://api.deepseek.com）模型列表接口验证认证，不验证余额或生成额度。' },
}
const errors: Record<string, string> = {
  invalidInput: '请检查名称和必填凭据。', staleInventory: '配置已变化，已刷新列表。请核对后重新保存。',
  notConfigured: '请先验证并保存凭据，再启用此配置。', credentialUnavailable: '已保存的凭据不可用，请重新填写。',
  invalidCredentials: '凭据验证未通过，请检查后重试。', networkFailed: '无法连接验证服务，请检查网络后重试。',
  validationFailed: '验证服务未能完成检查，请稍后重试。', storageFailed: '保存失败，请稍后重试。', invalidCaller: '当前窗口无法管理密钥配置。',
}
function errorCode(error: unknown): string {
  return typeof error === 'object' && error !== null && 'code' in error && typeof error.code === 'string' ? error.code : ''
}
export interface CredentialSettingsStatus { message: string; tone: 'default' | 'success' | 'error'; detail: boolean }
export interface CredentialSettingsHandle { requestLeave: (action: () => void) => void; back: () => boolean }

export function CredentialSettings({ client, ref, onStatusChange }: {
  client?: CredentialClient
  ref?: Ref<CredentialSettingsHandle>
  onStatusChange?: (status: CredentialSettingsStatus) => void
}) {
  const [inventory, setInventory] = useState<CredentialInventory | null>(null)
  const current = useRef<CredentialInventory | null>(null)
  const [selected, setSelected] = useState<CredentialService | null>(null)
  const [name, setName] = useState('')
  const [originalName, setOriginalName] = useState('')
  const [values, setValues] = useState<Record<string, string>>({})
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [pending, setPending] = useState(false)
  const busy = useRef(false)
  const live = useRef(false)
  const epoch = useRef(0)
  const readSequence = useRef(0)
  const [notice, setNotice] = useState<{ message: string; tone: CredentialSettingsStatus['tone'] }>({ message: '', tone: 'default' })
  const [removing, setRemoving] = useState<CredentialEntry | null>(null)
  const [leaveAction, setLeaveAction] = useState<{ action: () => void; origin: HTMLElement | null } | null>(null)
  const leaveFocusReturn = useRef<HTMLElement | null>(null)
  const listOrigin = useRef<HTMLElement | null>(null)
  const search = useRef<InputRef>(null)
  const detailBack = useRef<HTMLButtonElement | null>(null)
  const item = inventory?.items.find((candidate) => candidate.service === selected)
  const dirty = name !== originalName || Object.values(values).some((value) => value !== '')
  const missing = (Object.keys(services) as CredentialService[]).filter((service) => !inventory?.items.some((item) => item.service === service))

  const publish = useCallback((next: CredentialInventory) => {
    if (current.current && BigInt(next.revision) < BigInt(current.current.revision)) throw { code: 'staleInventory' }
    current.current = next; setInventory(next)
  }, [])
  const reload = useCallback(async () => {
    if (!client) { setLoading(false); setLoadError('密钥配置服务不可用。'); return }
    const owner = epoch.current; const sequence = ++readSequence.current
    setLoading(true)
    try {
      const next = await client.list()
      if (!live.current || owner !== epoch.current || sequence !== readSequence.current) return
      publish(next); setLoadError('')
    } catch {
      if (live.current && owner === epoch.current && sequence === readSequence.current) setLoadError('加载密钥配置失败，请刷新后重试。')
    } finally {
      if (live.current && owner === epoch.current && sequence === readSequence.current) setLoading(false)
    }
  }, [client, publish])
  useEffect(() => {
    live.current = true; epoch.current++; current.current = null; setInventory(null); busy.current = false; setPending(false)
    void reload()
    return () => { live.current = false; epoch.current++; readSequence.current++ }
  }, [reload])
  useEffect(() => { onStatusChange?.({ message: loadError || notice.message, tone: loadError ? 'error' : notice.tone, detail: selected !== null }) }, [notice, loadError, selected, onStatusChange])
  useEffect(() => { if (selected) detailBack.current?.focus() }, [selected])
  useEffect(() => {
    const handleFind = (event: KeyboardEvent) => {
      if (!selected && !removing && !leaveAction && event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey && !event.isComposing && event.key.toLowerCase() === 'f') {
        event.preventDefault(); event.stopPropagation(); search.current?.focus()
      }
    }
    document.addEventListener('keydown', handleFind)
    return () => document.removeEventListener('keydown', handleFind)
  }, [selected, removing, leaveAction])

  const closeDetail = () => {
    setSelected(null); setName(''); setOriginalName(''); setValues({}); setNotice({ message: '', tone: 'default' })
    const service = selected
    requestAnimationFrame(() => {
      const origin = listOrigin.current
      if (origin?.isConnected) origin.focus()
      else document.querySelector<HTMLButtonElement>(`[data-credential-service="${service}"]`)?.focus()
    })
  }
  const requestLeave = (action: () => void) => {
    if (busy.current) { setNotice({ message: '操作正在进行，请等待完成后再离开。', tone: 'default' }); return }
    if (leaveAction || removing) return
    if (dirty) {
      const focused = document.activeElement
      const origin = focused instanceof HTMLElement && focused.closest('.credential-settings') ? focused : detailBack.current
      setLeaveAction({ action, origin }); return
    }
    action()
  }
  const back = () => {
    if (removing || leaveAction) return true
    if (selected) { requestLeave(closeDetail); return true }
    if (busy.current) { requestLeave(() => {}); return true }
    return false
  }
  useImperativeHandle(ref, () => ({ requestLeave, back }))

  const mutate = async (operation: () => Promise<CredentialInventory>, success: string, accepted?: (next: CredentialInventory) => void) => {
    if (busy.current || !client || !current.current || loadError) return
    busy.current = true; setPending(true); setNotice({ message: selected ? '正在验证并保存…' : '正在保存…', tone: 'default' })
    const owner = epoch.current; readSequence.current++
    try {
      const next = await operation()
      if (!live.current || owner !== epoch.current) return
      publish(next); setNotice({ message: success, tone: 'success' }); accepted?.(next)
    } catch (error) {
      if (!live.current || owner !== epoch.current) return
      const suppliedCode = errorCode(error)
      const code = Object.prototype.hasOwnProperty.call(errors, suppliedCode) ? suppliedCode : ''
      setNotice({ message: errors[code] ?? '操作未完成，请刷新配置后重试。', tone: 'error' })
      // Reconcile a failed/uncertain write before another mutation uses its revision.
      if (code === 'staleInventory' || code === 'storageFailed' || !errors[code]) await reload()
    } finally {
      if (live.current && owner === epoch.current) { busy.current = false; setPending(false) }
    }
  }
  const open = (entry: CredentialEntry) => {
    listOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    setSelected(entry.service); setName(entry.name); setOriginalName(entry.name); setValues({}); setNotice({ message: '', tone: 'default' })
  }
  const save = () => {
    if (!item || !client || !current.current) return
    const submittedName = name.trim()
    const submittedValues = Object.fromEntries(Object.entries(values).filter(([, value]) => value !== ''))
    void mutate(() => client.save({ service: item.service, name: submittedName, values: submittedValues, expectedRevision: current.current!.revision }), '验证成功，已保存', (next) => {
      const saved = next.items.find((entry) => entry.service === item.service)
      if (!saved) return
      setName(saved.name); setOriginalName(saved.name); setValues({})
    })
  }
  const canSave = !!item && !!name.trim() && [...name.trim()].length <= 80 && (item.configured || services[item.service].fields.every(({ key }) => values[key]?.trim()))
  const locked = pending || loading || !!loadError || !client
  const cancelLeave = () => { leaveFocusReturn.current = leaveAction?.origin ?? null; setLeaveAction(null) }
  const filtered = inventory?.items.filter((entry) => `${entry.name} ${services[entry.service].name}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) ?? []

  return <div className="credential-settings" onKeyDown={(event) => {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') event.stopPropagation()
    if (event.key === 'Escape' && (selected || removing || leaveAction || pending)) {
      event.preventDefault(); event.stopPropagation()
      if (leaveAction) cancelLeave()
      else if (removing) setRemoving(null)
      else back()
    }
  }}>
    {selected ? <>
      <header className="credential-detail-heading">
        <Tooltip title="返回密钥配置"><Button ref={detailBack} type="text" size="small" icon={<ArrowLeft size={17} aria-hidden />} aria-label="返回密钥配置" onClick={() => requestLeave(closeDetail)} /></Tooltip>
        <h2 title={item?.name ?? services[selected].name}>{item?.name ?? services[selected].name}</h2>
      </header>
      <Form component="div" layout="horizontal" className="credential-form">
        <Form.Item label="名称" htmlFor="credential-name"><Input id="credential-name" aria-label="名称" value={name} placeholder="请输入配置名称" disabled={locked || !item} onChange={(event) => { setName(event.target.value); setNotice({ message: '', tone: 'default' }) }} /></Form.Item>
        {services[selected].fields.map(({ key, label }) => <Form.Item key={key} label={label} htmlFor={`credential-${key}`}>
          <Input.Password id={`credential-${key}`} aria-label={label} value={values[key] ?? ''} autoComplete="off" spellCheck={false} disabled={locked || !item}
            placeholder={item?.configured ? '已保存，留空保留原值' : `请输入 ${label}`} onChange={(event) => { setValues((previous) => ({ ...previous, [key]: event.target.value })); setNotice({ message: '', tone: 'default' }) }} />
        </Form.Item>)}
        <p className="credential-hint">{services[selected].hint}</p>
        <p className="credential-hint">{item?.configured ? '已保存的凭据不会回填。留空保留原值，输入新值可替换。' : '验证通过后才保存到本机。'}此配置不会自动授予插件网络权限。</p>
        {!item && !loading ? <p className="credential-error" role="alert">此配置已被删除，请返回列表。</p> : null}
        <div className="credential-save"><Button type="primary" loading={pending} disabled={locked || !canSave} onClick={save}>验证并保存</Button></div>
      </Form>
    </> : <>
      <header className="credential-heading"><h2>密钥配置</h2><div className="credential-heading-actions">
        <Tooltip title="刷新密钥配置"><Button type="text" aria-label="刷新密钥配置" icon={<RefreshCw size={16} aria-hidden />} disabled={pending || loading || !client} onClick={() => void reload()} /></Tooltip>
        <Dropdown trigger={['click']} disabled={locked || !inventory || !missing.length} menu={{ items: missing.map((service) => ({ key: service, label: services[service].name, onClick: () => {
          if (!client || !current.current) return
          void mutate(() => client.add({ service, expectedRevision: current.current!.revision }), '已添加配置', (next) => { const entry = next.items.find((item) => item.service === service); if (entry) open(entry) })
        } })) }}><Button icon={<Plus size={15} aria-hidden />} disabled={locked || !inventory || !missing.length}>添加配置</Button></Dropdown>
      </div></header>
      <Input ref={search} className="credential-search" prefix={<Search size={15} aria-hidden />} aria-label="搜索配置名称或服务商" placeholder="搜索名称或服务商" value={query} onChange={(event) => setQuery(event.target.value)} />
      {loading && !inventory ? <div className="credential-loading"><Spin size="small" />正在加载密钥配置…</div> : groups.map((group) => {
        const entries = filtered.filter((entry) => (group.services as readonly string[]).includes(entry.service))
        return <section className="credential-group" key={group.english} aria-label={group.label}>
          <header className="credential-group-heading"><h3>{group.label}<span> / {group.english}</span></h3></header>
          <ul>{entries.map((entry) => <li className="credential-row" key={entry.service}>
            <span className={`credential-provider-icon is-${entry.service}`} aria-hidden>{entry.service === 'deepseek' ? <BrainCircuit size={17} /> : entry.service === 'baiduOcr' ? <ScanText size={17} /> : <Languages size={17} />}</span>
            <button className="credential-name" type="button" data-credential-service={entry.service} title={`${entry.name} · ${services[entry.service].name}${entry.configured ? '' : ' · 尚未配置'}`} disabled={locked} onClick={() => open(entry)}>{entry.name}</button>
            <Tooltip title={entry.configured ? '控制此配置是否可供使用' : '请先验证并保存凭据'}><Switch size="small" aria-label={`启用${entry.name}`} aria-description={entry.configured ? undefined : '请先验证并保存凭据'} disabled={locked || !entry.configured} checked={entry.enabled} onChange={(enabled) => {
              if (client && current.current) void mutate(() => client.setEnabled({ service: entry.service, enabled, expectedRevision: current.current!.revision }), enabled ? '已启用配置' : '已停用配置')
            }} /></Tooltip>
            <Tooltip title="删除配置"><Button type="text" size="small" className="credential-delete" aria-label={`删除${entry.name}`} icon={<Trash2 size={14} aria-hidden />} disabled={locked} onClick={() => setRemoving(entry)} /></Tooltip>
          </li>)}</ul>
          {!entries.length ? <p className="credential-group-empty">{query.trim() ? '没有匹配的配置' : '暂无配置'}</p> : null}
        </section>
      })}
      {inventory?.items.length === 0 && !query ? <p className="credential-empty"><KeyRound size={16} aria-hidden />添加配置后填写并验证凭据。</p> : null}
    </>}
    {loadError ? <div className="credential-error" role="alert">{loadError}{client ? <Button type="link" size="small" disabled={loading || pending} onClick={() => void reload()}>刷新配置</Button> : null}</div> : null}
    {notice.message ? <p className={`credential-notice is-${notice.tone}`} role={notice.tone === 'error' ? 'alert' : 'status'}>{notice.message}</p> : null}
    <Modal destroyOnHidden open={!!removing} title={`删除${removing?.name ?? '配置'}？`} okText="删除配置" cancelText="取消" okButtonProps={{ danger: true }} onCancel={() => setRemoving(null)} onOk={() => {
      const entry = removing; setRemoving(null)
      if (entry && client && current.current) void mutate(() => client.remove({ service: entry.service, expectedRevision: current.current!.revision }), '已删除配置')
    }}><p>将删除本机保存的此项配置及凭据。删除后可以重新添加。</p></Modal>
    <Modal destroyOnHidden focusTriggerAfterClose={false} open={!!leaveAction} title="放弃未保存的修改？" okText="放弃修改" cancelText="取消" okButtonProps={{ danger: true }} onCancel={cancelLeave} afterClose={() => {
      // The trigger may be a focus-activated settings tab; restoring it would reopen this guard.
      const origin = leaveFocusReturn.current; leaveFocusReturn.current = null
      if (live.current && origin?.isConnected) origin.focus()
    }} onOk={() => {
      const action = leaveAction?.action; leaveFocusReturn.current = null; setLeaveAction(null); setValues({}); setName(originalName); action?.()
    }}><p>本次修改尚未验证保存。放弃后，已保存的配置保持不变。</p></Modal>
  </div>
}
