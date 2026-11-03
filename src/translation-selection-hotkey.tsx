import { Button } from 'antd'
import { useEffect, useRef, useState } from 'react'
import { HotkeyRecorderInput } from './hotkey-recorder-input'
import type { HotkeyRecordingPhase } from './launcher-core'
import type { LauncherClient, TranslationSelectionSettings } from './protocol'

export function TranslationSelectionHotkey({ client, disabled, onRecordingPhase }: { client: LauncherClient; disabled: boolean; onRecordingPhase?: (phase: HotkeyRecordingPhase) => void | Promise<void> }) {
  const [settings, setSettings] = useState<TranslationSelectionSettings | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const live = useRef(false)
  const pending = useRef(false)
  const read = async () => {
    if (!client.getTranslationSelectionSettings) throw new Error('unavailable')
    const value = await client.getTranslationSelectionSettings()
    if (live.current) setSettings(value)
  }
  useEffect(() => { live.current = true; void read().catch(() => { if (live.current) setError('无法读取划词翻译快捷键，请刷新状态。') }); return () => { live.current = false } }, [client])
  const save = async (hotkey: string | null) => {
    if (pending.current || disabled || !client.setTranslationSelectionHotkey) return
    pending.current = true; setBusy(true); setError('')
    try { await client.setTranslationSelectionHotkey({ hotkey }) }
    catch { if (live.current) setError('无法保存划词翻译快捷键，请检查冲突并核对当前状态。') }
    try { await read() }
    catch { if (live.current) { setSettings(null); setError('无法刷新划词翻译快捷键，请刷新状态后核对。') } }
    finally { pending.current = false; if (live.current) setBusy(false) }
  }
  return <div className="public-command-row" aria-busy={busy}>
    <div className="public-command-hotkey"><HotkeyRecorderInput label="划词翻译快捷键" value={settings?.hotkey ?? ''} allowDoubleTap={false} disabled={disabled || busy || !settings} onRecordingPhase={onRecordingPhase} onCommit={save} />
      <Button aria-label="清除划词翻译快捷键" disabled={disabled || busy || !settings?.hotkey} onClick={() => void save(null)}>清除</Button></div>
    <div className="public-command-feedback" aria-live="polite">{error ? <span role="alert" className="plugin-row-error">{error}</span> : busy ? '保存中…' : settings ? ({ active: '划词快捷键已启用', unset: '未设置划词快捷键', suspended: '划词快捷键已暂停，请检查插件和读取选中文字授权', unavailable: '划词快捷键不可用，请检查系统占用' })[settings.status] : '正在读取…'}
      {error ? <Button size="small" disabled={busy || disabled} onClick={() => { setBusy(true); void read().then(() => { if (live.current) setError('') }).catch(() => {}).finally(() => { if (live.current) setBusy(false) }) }}>刷新状态</Button> : null}</div>
  </div>
}
