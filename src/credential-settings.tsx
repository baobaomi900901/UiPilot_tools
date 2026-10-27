import { Button, Dropdown, Form, Input, Popconfirm, Switch } from 'antd'
import { KeyRound, Plus, Trash2 } from 'lucide-react'
import type { Dispatch, SetStateAction } from 'react'
import { useState } from 'react'

type Category = 'translation' | 'ocr'
type Provider = 'youdao' | 'baidu'
type Entry = { enabled: boolean; values: Record<string, string> }
type Group = { selected: Provider | null; entries: Partial<Record<Provider, Entry>> }
export type CredentialDraft = { category: Category; translation: Group; ocr: Group }

const providers: Provider[] = ['youdao', 'baidu']
const providerNames = { youdao: '有道', baidu: '百度' }
const categories: { key: Category; label: string }[] = [
  { key: 'translation', label: '翻译' },
  { key: 'ocr', label: 'OCR' },
]
const appId = { key: 'appId', label: 'App ID', secret: false }
const appKey = { key: 'appKey', label: 'App Key', secret: true }
const fields = {
  translation: { youdao: [appId, appKey], baidu: [appId, { ...appKey, label: '密钥' }] },
  ocr: {
    youdao: [appId, appKey],
    baidu: [appId, { key: 'apiKey', label: 'API Key', secret: true }, { key: 'secretKey', label: 'Secret Key', secret: true }],
  },
}

export function createCredentialDraft(): CredentialDraft {
  const group = (): Group => ({
    selected: 'youdao',
    entries: { youdao: { enabled: false, values: {} }, baidu: { enabled: false, values: {} } },
  })
  return { category: 'translation', translation: group(), ocr: group() }
}

export function CredentialSettings({ draft, setDraft, validateYoudao }: {
  draft: CredentialDraft
  setDraft: Dispatch<SetStateAction<CredentialDraft>>
  validateYoudao?: (input: { appId: string; appKey: string }) => Promise<{ valid: boolean; message: string }>
}) {
  const category = draft.category
  const group = draft[category]
  const selected = group.selected
  const entry = selected ? group.entries[selected] : undefined
  const [validationStatus, setValidationStatus] = useState<'idle' | 'checking' | 'success' | 'error'>('idle')
  const available = providers.filter((provider) => !group.entries[provider])
  const name = (provider: Provider) => `${providerNames[provider]}${category === 'translation' ? '翻译' : ' OCR'}`
  const updateGroup = (update: (current: Group) => Group) => {
    setDraft((current) => ({ ...current, [category]: update(current[category]) }))
  }
  const updateEntry = (provider: Provider, update: (current: Entry) => Entry) => {
    updateGroup((current) => {
      const existing = current.entries[provider]
      return existing ? { ...current, entries: { ...current.entries, [provider]: update(existing) } } : current
    })
  }
  const add = (provider: Provider) => updateGroup((current) => current.entries[provider] ? current : {
    selected: provider,
    entries: { ...current.entries, [provider]: { enabled: false, values: {} } },
  })
  const remove = (provider: Provider) => updateGroup((current) => {
    const entries = { ...current.entries }
    delete entries[provider]
    return { entries, selected: current.selected === provider ? providers.find((key) => entries[key]) ?? null : current.selected }
  })

  const validate = async () => {
    if (!selected || !entry || category !== 'translation' || selected !== 'youdao' || !validateYoudao) return
    setValidationStatus('checking')
    try {
      const result = await validateYoudao({ appId: entry.values.appId ?? '', appKey: entry.values.appKey ?? '' })
      setValidationStatus(result.valid ? 'success' : 'error')
    } catch { setValidationStatus('error') }
  }
  return (
    <div className="credential-settings" onKeyDown={(event) => {
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') event.stopPropagation()
    }}>
      <div className="credential-category-tabs" role="tablist" aria-label="接口分类">
        {categories.map(({ key, label }, index) => (
          <button key={key} id={`credential-tab-${key}`} type="button" role="tab"
            aria-selected={category === key} aria-controls="credential-category-panel"
            tabIndex={category === key ? 0 : -1}
            onClick={() => setDraft((current) => ({ ...current, category: key }))}
            onKeyDown={(event) => {
              if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
              event.preventDefault()
              const next = event.key === 'Home' ? 0 : event.key === 'End' ? 1 : 1 - index
              setDraft((current) => ({ ...current, category: categories[next]!.key }))
              event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('button')[next]?.focus()
            }}
          >{label}</button>
        ))}
      </div>
      <div id="credential-category-panel" role="tabpanel" aria-labelledby={`credential-tab-${category}`} className="credential-layout">
        <aside className="credential-sidebar" aria-label="服务商">
          <div className="credential-sidebar-label">服务商</div>
          <div className="credential-provider-list">
            {providers.filter((provider) => group.entries[provider]).map((provider) => (
              <div key={provider} className={`credential-provider-row${selected === provider ? ' is-selected' : ''}`}>
                <button type="button" className="credential-provider-select" aria-pressed={selected === provider}
                  onClick={() => updateGroup((current) => ({ ...current, selected: provider }))}>
                  <span className={`credential-provider-mark is-${provider}`} aria-hidden>{provider === 'youdao' ? '有' : '百'}</span>
                  <span>{name(provider)}</span>
                </button>
                <Switch size="small" aria-label={`启用${name(provider)}`} checked={group.entries[provider]!.enabled}
                  onChange={(enabled) => updateEntry(provider, (current) => ({ ...current, enabled }))} />
                <Popconfirm title={`删除${name(provider)}？`} description="此项已填写的内容将被清除。"
                  okText="删除" cancelText="取消" onConfirm={() => remove(provider)}>
                  <Button type="text" size="small" className="credential-delete" aria-label={`删除${name(provider)}`}
                    icon={<Trash2 size={14} aria-hidden />} />
                </Popconfirm>
              </div>
            ))}
            {Object.keys(group.entries).length === 0 ? <p className="credential-hint">暂无服务商</p> : null}
          </div>
          <Dropdown trigger={['click']} menu={{ items: available.map((provider) => ({
            key: provider, label: providerNames[provider], onClick: () => add(provider),
          })) }} disabled={available.length === 0}>
            <Button className="credential-add" icon={<Plus size={15} aria-hidden />} disabled={available.length === 0}>添加服务商</Button>
          </Dropdown>
          {available.length === 0 ? <p className="credential-hint">已添加全部支持的服务商</p> : null}
        </aside>
        <section className="credential-detail" aria-label="服务商配置">
          {selected && entry ? <>
            <h2>{name(selected)}</h2>
            <Form component="div" layout="vertical" className="credential-form" key={`${category}-${selected}`}>
              {fields[category][selected].map((field) => {
                const id = `credential-${category}-${selected}-${field.key}`
                const props = { id, value: entry.values[field.key] ?? '', placeholder: `请输入 ${field.label}`,
                  autoComplete: 'off', spellCheck: false,
                  onChange: (event: React.ChangeEvent<HTMLInputElement>) => {
                    const value = event.target.value
                    updateEntry(selected, (current) => ({ ...current, values: { ...current.values, [field.key]: value } }))
                  },
                }
                return <Form.Item key={field.key} label={field.label} htmlFor={id}>
                  {field.secret ? <Input.Password {...props} /> : <Input {...props} />}
                </Form.Item>
              })}
              <div className="credential-validation">
                <span>{validationStatus === 'checking' ? '验证中…' : validationStatus === 'success' ? '验证成功' : validationStatus === 'error' ? '验证失败' : '尚未验证'}</span>
                <Button
                  loading={validationStatus === 'checking'}
                  disabled={Object.values(entry.values).some((value) => value.trim() === '')}
                  onClick={() => void validate()}
                >
                  验证
                </Button>
              </div>
              <p className="credential-hint">验证会发送一条最小翻译请求，不会记录密钥。</p>
            </Form>
          </> : <div className="credential-empty"><KeyRound size={28} aria-hidden /><h2>暂无配置</h2><p>添加服务商后填写对应的密钥。</p></div>}
        </section>
      </div>
    </div>
  )
}
