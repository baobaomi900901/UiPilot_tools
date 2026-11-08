import { useEffect, useState } from 'react'
import { formatHotkeyDisplay } from './hotkey-recorder'
export interface MacHotkeyStatus {
  status: 'active' | 'fallback' | 'unavailable'
  configuredHotkey: string
  effectiveHotkey: string | null
}
export function parseMacHotkeyStatus(value: unknown): MacHotkeyStatus | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const row = value as Record<string, unknown>
  if (Object.keys(row).length !== 3 || typeof row.configuredHotkey !== 'string' || row.configuredHotkey.length > 128 ||
      !['active', 'fallback', 'unavailable'].includes(String(row.status))) return null
  if (row.status === 'unavailable' ? row.effectiveHotkey !== null : typeof row.effectiveHotkey !== 'string' || !row.effectiveHotkey || row.effectiveHotkey.length > 128) return null
  return row as unknown as MacHotkeyStatus
}
export type ReadMacHotkeyStatus = () => Promise<unknown>
export function MacHotkeyStatusNotice({ read, revision }: { read?: ReadMacHotkeyStatus; revision: number }) {
  const [message, setMessage] = useState('正在确认快捷键状态…')
  useEffect(() => {
    let current = 0; let live = true
    const refresh = async () => {
      const owner = ++current
      setMessage('正在确认快捷键状态…')
      let result: MacHotkeyStatus | null = null
      try { result = parseMacHotkeyStatus(await read?.()) } catch { /* Fixed status only. */ }
      if (!live || current !== owner) return
      setMessage(!result ? '无法读取快捷键状态，请重新打开设置或刷新。' :
        result.status === 'unavailable' ? (result.configuredHotkey === 'DoubleMeta' ? '双击 ⌘ 不可用，请允许输入监控后重新录制；若仍不可用，请完整退出并重新启动 UiPilot 后再录制。' : '快捷键不可用，请重新录制。') :
        result.status === 'fallback' ? `双击 ⌘ 不可用，暂用 ${formatHotkeyDisplay(result.effectiveHotkey!, 'macos').replace('Space', '空格')}。请允许输入监控后重新录制；若仍不可用，请完整退出并重新启动 UiPilot 后再录制。` :
        `当前生效：${formatHotkeyDisplay(result.effectiveHotkey!, 'macos').replace('Space', '空格')}`)
    }
    void refresh(); window.addEventListener('focus', refresh)
    return () => { live = false; ++current; window.removeEventListener('focus', refresh) }
  }, [read, revision])
  return <span role="status" aria-live="polite">{message}</span>
}
