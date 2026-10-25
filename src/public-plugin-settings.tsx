import { Button, Input, Select, Switch } from 'antd'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { LauncherClient, PublicPluginInventoryItem, PublicSettingDefinition } from './protocol'

type Value = string | number | boolean
type Definition = Exclude<PublicSettingDefinition, { type: 'secret' }>
function validate(definition: Definition, raw: string | boolean): { value?: Value; error?: string } {
  if (definition.type === 'boolean') return { value: raw === true }
  if (typeof raw !== 'string') return { error: '设置值无效' }
  if (definition.type === 'text') return /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(raw) ? { error: '文本包含不支持的控制字符' } : { value: raw }
  if (definition.type === 'select') return definition.options.some(option => option.value === raw) ? { value: raw } : { error: '请选择声明的选项' }
  if (!raw.trim() || !Number.isFinite(Number(raw))) return { error: '请输入有限数字' }
  const value = Number(raw)
  if (definition.min !== undefined && value < definition.min) return { error: `不能小于 ${definition.min}` }
  if (definition.max !== undefined && value > definition.max) return { error: `不能大于 ${definition.max}` }
  return { value }
}

// The enclosing detail owns the accepted inventory and admission across management operations.
export function PublicPluginSettings({ client, plugin, revision, reload, detailBusy, acquire, release }: {
  client: LauncherClient; plugin: PublicPluginInventoryItem; revision: string
  reload: (beforePublish?: (revision: string) => void) => Promise<void>
  detailBusy: boolean; acquire: () => boolean; release: () => void
}) {
  const fields = plugin.settings.flatMap(setting => setting.definition.type === 'secret' ? [] : [{ definition: setting.definition as Definition, value: setting.value ?? setting.definition.default }])
  const signature = JSON.stringify(fields)
  const previous = useRef(signature)
  const acceptedRevision = useRef<string | null>(null)
  const draftsRef = useRef(new Map<string, string | boolean>())
  const [drafts, setDrafts] = useState(draftsRef.current)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  const live = useRef(true)
  const sequence = useRef(0)
  const [review, setReview] = useState(false)
  const [unread, setUnread] = useState(false)
  const [feedback, setFeedback] = useState('')
  const updateDrafts = (next: Map<string, string | boolean>) => { draftsRef.current = next; setDrafts(next) }
  useEffect(() => { live.current = true; return () => { live.current = false; sequence.current++ } }, [])
  useLayoutEffect(() => {
    if (previous.current !== signature) {
      if (acceptedRevision.current !== revision) {
        sequence.current++
        if (draftsRef.current.size) { setReview(true); setFeedback('设置状态已变化，请核对草稿并取消修改后重新编辑。') }
      }
      previous.current = signature
    }
    acceptedRevision.current = null
  }, [signature, revision])
  useLayoutEffect(() => { if (!busy) pending.current = false })
  const changed = fields.flatMap(({ definition, value }) => {
    if (!drafts.has(definition.key)) return []
    const parsed = validate(definition, drafts.get(definition.key)!)
    return parsed.error || parsed.value !== value ? [{ definition, ...parsed }] : []
  })
  const invalid = changed.some(field => field.error)
  const readonly = busy || detailBusy || !plugin.enabled || !!plugin.fault || unread || review
  const read = async (owns: () => boolean, outcome: 'success' | 'failure' | 'read') => {
    try {
      await reload(nextRevision => { if (owns()) acceptedRevision.current = nextRevision })
      if (live.current) setUnread(false)
      if (!owns()) return
      if (outcome === 'success') { updateDrafts(new Map()); setReview(false); setFeedback('已保存') }
      else { setReview(true); setFeedback(outcome === 'failure' ? '保存未确认，落盘值可能已改变。已重新读取；请核对草稿并取消修改后重新编辑。' : '已重新读取；请核对草稿并取消修改后重新编辑。') }
    } catch {
      // External ordinary changes retire feedback ownership, never the need to reconcile a write.
      if (live.current) { setUnread(true); setFeedback(outcome === 'success' ? '已保存，无法刷新当前状态。请重新读取普通设置后核对。' : '保存未确认，落盘值可能已改变；无法刷新当前状态。请重新读取普通设置，勿重复提交。') }
    } finally {
      if (live.current) { setBusy(false); release() }
    }
  }
  const save = async () => {
    if (pending.current || readonly || !changed.length || invalid || !acquire()) return
    pending.current = true; setBusy(true); setFeedback('正在保存…')
    const token = ++sequence.current
    const owns = () => live.current && sequence.current === token
    let outcome: 'success' | 'failure' = 'success'
    try { await client.savePublicPluginSettings({ input: { pluginId: plugin.pluginId, settings: Object.fromEntries(changed.map(field => [field.definition.key, field.value!])), secrets: {} } }) }
    catch { outcome = 'failure' }
    // An unmounted detail must never call even a newly supplied reload continuation.
    if (live.current) await read(owns, outcome)
  }
  const reread = async () => {
    if (pending.current || detailBusy || !acquire()) return
    pending.current = true; setBusy(true)
    const token = ++sequence.current
    await read(() => live.current && sequence.current === token, 'read')
  }
  if (!fields.length) return null
  return <section className="public-ordinary-settings" aria-label="普通设置" aria-busy={busy}>
    <h3>普通设置</h3>
    {!plugin.enabled || plugin.fault ? <p className="public-settings-help">{plugin.fault ? '插件运行故障，请恢复并启用后保存设置。' : '插件已停用，请先启用后保存设置。'}</p> : null}
    <form noValidate onSubmit={event => { event.preventDefault(); void save() }}>
      {fields.map(({ definition, value }, index) => {
        const raw = drafts.has(definition.key) ? drafts.get(definition.key)! : value
        const error = drafts.has(definition.key) ? validate(definition, drafts.get(definition.key)!).error : undefined
        const id = `public-setting-${index}`
        const edit = (next: string | boolean) => { const updated = new Map(draftsRef.current); updated.set(definition.key, next); updateDrafts(updated); setFeedback('') }
        return <div className="public-ordinary-setting" key={definition.key}>
          <label htmlFor={id}>{definition.label} <code>{definition.key}</code></label>
          {value === undefined && !drafts.has(definition.key) ? <span id={`${id}-unset`} className="public-settings-help">未设置</span> : null}
          {definition.type === 'boolean' ? <Switch id={id} aria-label={definition.label} checked={raw === true} disabled={readonly} onChange={edit} />
            : definition.type === 'select' ? <Select id={id} aria-label={definition.label} aria-invalid={!!error} aria-describedby={error ? `${id}-error` : undefined} disabled={readonly}
              value={raw === undefined ? undefined : String(raw)} placeholder="请选择（未设置）" options={definition.options.map(option => ({ ...option }))} onChange={edit} />
            : definition.type === 'text' ? <Input.TextArea id={id} aria-label={definition.label} aria-invalid={!!error} aria-describedby={error ? `${id}-error` : undefined} disabled={readonly}
              value={raw === undefined ? '' : String(raw)} placeholder="输入文本" rows={2} onChange={event => edit(event.target.value)} />
            : <Input id={id} aria-label={definition.label} aria-invalid={!!error} aria-describedby={error ? `${id}-error` : undefined} disabled={readonly} value={raw === undefined ? '' : String(raw)}
              placeholder="输入数字" inputMode="decimal" step={definition.step}
              onChange={event => edit(event.target.value)} />}
          {definition.type === 'number' ? <span className="public-settings-help">{definition.min !== undefined ? `最小 ${definition.min} ` : ''}{definition.max !== undefined ? `最大 ${definition.max} ` : ''}{definition.step !== undefined ? `步长提示 ${definition.step}` : ''}</span> : null}
          {error ? <span id={`${id}-error`} role="alert" className="plugin-row-error">{error}</span> : null}
        </div>
      })}
      <div className="public-settings-actions">
        <Button htmlType="submit" type="primary" aria-label="保存普通设置" disabled={readonly || !changed.length || invalid}>保存设置</Button>
        <Button aria-label="取消普通设置修改" disabled={busy || detailBusy || unread || (!drafts.size && !review)} onClick={() => { updateDrafts(new Map()); setReview(false); setFeedback('已取消修改') }}>取消修改</Button>
        {unread ? <Button aria-label="重新读取普通设置" disabled={busy || detailBusy} onClick={() => void reread()}>重新读取</Button> : null}
      </div>
      <div className="public-settings-feedback" role="status" aria-live="polite">{feedback}</div>
    </form>
  </section>
}
