import { Button, Input, Tooltip } from 'antd'
import { Star } from 'lucide-react'
import { createPortal } from 'react-dom'
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { HotkeyRecorderInput } from './hotkey-recorder-input'
import type { HotkeyRecordingPhase } from './launcher-core'
import type { PublicPluginInventoryItem } from './protocol'

// Each command owns its drafts and continuations; package settings remain singular.
export function StartupCommandEditor({ command, revision, reload, single, onRecordingPhase, operations, headerActionsTarget, submitRef, onBlockedChange, disabled = false }: {
  command: PublicPluginInventoryItem['commands'][number]
  revision: string
  reload: (beforePublish?: (revision: string) => void) => Promise<void>
  single: boolean
  onRecordingPhase?: (phase: HotkeyRecordingPhase) => void | Promise<void>
  operations: { name: (value: string | null) => Promise<void>; hotkey: (value: string | null) => Promise<void>; favorite: (value: boolean) => Promise<void> }
  disabled?: boolean
  headerActionsTarget?: HTMLElement | null
  submitRef?: RefObject<(() => Promise<boolean>) | null>
  onBlockedChange?: (blocked: boolean) => void
}) {
  const [draft, setDraft] = useState(`/${command.effectiveName}`)
  const [busy, setBusy] = useState(false)
  const [unreconciled, setUnreconciled] = useState(false)
  const acceptedRevision = useRef<string | null>(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const touched = useRef(false)
  const row = useRef<HTMLDivElement>(null)
  const alive = useRef(true)
  const currentRevision = useRef(revision)
  currentRevision.current = revision
  const sequence = useRef(0)
  const pending = useRef(false)
  const focusReturn = useRef<{ origin: HTMLElement; moved: boolean } | null>(null)
  const previousCommand = useRef(command)
  useEffect(() => {
    alive.current = true
    const onFocus = (event: FocusEvent) => {
      if (focusReturn.current && event.target !== document.body && event.target !== focusReturn.current.origin) focusReturn.current.moved = true
    }
    document.addEventListener('focusin', onFocus)
    return () => { alive.current = false; sequence.current++; document.removeEventListener('focusin', onFocus) }
  }, [])
  useLayoutEffect(() => {
    // Revision ownership governs UI publication, not the outstanding mutation gate.
    // The reload owner marks this row's accepted refresh before publishing inventory.
    if (acceptedRevision.current !== revision) {
      if (pending.current) focusReturn.current = null
      sequence.current++
      setError('')
      setNotice('')
    }
    acceptedRevision.current = null
    setUnreconciled(false)
    const previous = previousCommand.current
    if (!touched.current && (previous.effectiveName !== command.effectiveName || previous.nameOverride !== command.nameOverride)) {
      setDraft(`/${command.effectiveName}`)
      touched.current = false
    }
    previousCommand.current = command
  }, [revision, command.effectiveName, command.nameOverride])
  useLayoutEffect(() => {
    // Release admission only after the accepted props/disabled state reach the DOM.
    if (!busy) pending.current = false
  })
  useLayoutEffect(() => {
    const focus = focusReturn.current
    if (busy || !focus) return
    focusReturn.current = null
    if (focus.moved) return
    const active = document.activeElement
    if (active !== document.body && active !== focus.origin) return
    const recovery = unreconciled ? row.current?.querySelector<HTMLButtonElement>('button[data-command-refresh]') : null
    const target = focus.origin.isConnected && !focus.origin.matches(':disabled') ? focus.origin
      : recovery ?? row.current?.querySelector<HTMLButtonElement>('.settings-hotkey-recorder button') ?? row.current?.querySelector<HTMLInputElement>('input')
    if (target?.isConnected && !target.matches(':disabled')) target.focus()
  }, [busy, unreconciled])
  const reconcile = async (owns: () => boolean) => {
    try {
      await reload((nextRevision) => {
        if (owns()) acceptedRevision.current = nextRevision
      })
      if (alive.current) setUnreconciled(false)
      return true
    } catch {
      if (alive.current) {
        // The write may already be durable. Retry only the read, never the mutation.
        setUnreconciled(true)
        setError('无法刷新当前状态，请刷新命令状态后继续。')
        setNotice('')
      }
      return false
    } finally {
      if (alive.current) setBusy(false)
    }
  }
  const mutate = async (operation: () => Promise<void>, success: () => void, failure: string, origin?: HTMLElement) => {
    if (pending.current || unreconciled || disabled) return false
    pending.current = true
    const token = ++sequence.current
    const ownerRevision = revision
    const owns = () => alive.current && sequence.current === token && currentRevision.current === ownerRevision
    if (origin) focusReturn.current = { origin, moved: false }
    setBusy(true); setError(''); setNotice('')
    let succeeded = false
    try {
      await operation()
      succeeded = true
      if (owns()) success()
    } catch {
      if (owns()) setError(failure)
    }
    // A sibling revision can retire captured feedback, but cannot erase reconciliation.
    if (alive.current) return (await reconcile(owns)) && succeeded
    return false
  }
  const retryRefresh = (origin: HTMLElement) => {
    if (pending.current || disabled) return
    pending.current = true
    const token = ++sequence.current
    const ownerRevision = revision
    focusReturn.current = { origin, moved: false }
    setBusy(true); setError('')
    void reconcile(() => alive.current && sequence.current === token && currentRevision.current === ownerRevision)
  }
  const save = async (value: string | null, origin?: HTMLElement): Promise<boolean> => {
    if (pending.current || unreconciled || disabled) return false
    const normalized = value === null ? null : value.trim().replace(/^\/+/, '')
    if (normalized !== null && !normalized) { setError('启动指令不能为空。'); return false }
    if (normalized === command.effectiveName || (normalized === null && command.nameOverride === null)) {
      setDraft(`/${command.effectiveName}`); touched.current = false; setError('')
      return true
    }
    return mutate(
      () => operations.name(normalized),
      () => { setDraft(`/${normalized ?? command.defaultName}`); touched.current = false; setNotice('已保存') },
      '无法保存启动键，请检查指令格式后重试。', origin,
    )
  }
  useLayoutEffect(() => {
    if (submitRef) submitRef.current = () => touched.current ? save(draft) : Promise.resolve(!pending.current && !unreconciled)
    return () => { if (submitRef) submitRef.current = null }
  })
  useLayoutEffect(() => { onBlockedChange?.(busy || unreconciled) }, [busy, unreconciled, onBlockedChange])
  const saveHotkey = (hotkey: string | null, origin?: HTMLElement) => mutate(
    () => operations.hotkey(hotkey),
    () => setNotice(hotkey === null ? '已清除快捷键' : '已保存快捷键'),
    '无法保存快捷键，请检查快捷键冲突或系统占用后重试。', origin,
  )
  const suffix = single ? '' : ` ${command.commandId}`
  const favoriteAction = (
    <Tooltip title={command.favorite ? '取消常用' : '设为常用'}><Button
          data-command-favorite onMouseDown={(event) => event.preventDefault()}
          aria-label={`${command.favorite ? '取消常用' : '设为常用'}${suffix}`} aria-pressed={command.favorite} disabled={busy || unreconciled || disabled}
          icon={<Star aria-hidden size={16} strokeWidth={1.8} fill={command.favorite ? 'currentColor' : 'none'} />}
          onClick={(event) => void mutate(
            () => operations.favorite(!command.favorite),
            () => setNotice(command.favorite ? '已取消常用' : '已设为常用'), '无法更改常用状态，请重试。', event.currentTarget,
          )} /></Tooltip>
  )
  return (
    <div className="public-command-row" data-command-id={command.commandId} aria-busy={busy} ref={row}>
      {headerActionsTarget ? createPortal(favoriteAction, headerActionsTarget) : <div className="public-detail-local-favorite">{favoriteAction}</div>}
      <form className="public-detail-name-control" onSubmit={(event) => { event.preventDefault(); save(draft, document.activeElement instanceof HTMLElement ? document.activeElement : undefined) }}>
        <Input aria-label={`启动键${suffix}`} placeholder="输入 /启动指令" value={draft} readOnly={busy || unreconciled || disabled}
          onChange={(event) => { touched.current = true; setDraft(event.target.value) }}
          onBlur={(event) => {
            // Tab/click into a row action gives that action sole ownership (especially restore).
            if (!touched.current || (event.relatedTarget instanceof Element && (event.relatedTarget.closest('[data-command-reset]') || event.relatedTarget.closest('[data-command-selector]') || event.relatedTarget.closest('[data-command-favorite]')))) return
            void save(event.currentTarget.value)
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return
            if (event.key === 'Enter') { event.preventDefault(); void save(draft, event.currentTarget) }
            if (event.key === 'Escape' && (touched.current || error)) {
              event.preventDefault(); event.stopPropagation(); setDraft(`/${command.effectiveName}`); touched.current = false; setError('')
            }
          }} />
        <Tooltip title="恢复默认指令"><Button aria-label={`恢复默认启动键${suffix}`} disabled={busy || unreconciled || disabled}
          data-command-reset onMouseDown={(event) => event.preventDefault()} onClick={(event) => void save(null, event.currentTarget)}>初始化</Button></Tooltip>
      </form>
      <div className="public-command-hotkey">
        <HotkeyRecorderInput key={revision} value={command.hotkey ?? ''} label={`命令快捷键${suffix}`} allowDoubleTap={false}
          disabled={busy || unreconciled || disabled} onRecordingPhase={onRecordingPhase}
          onCommit={async (chord) => { await saveHotkey(chord, row.current?.querySelector<HTMLButtonElement>('.settings-hotkey-recorder button') ?? undefined) }} />
        <Button aria-label={`清除命令快捷键${suffix}`} disabled={busy || unreconciled || disabled || !command.hotkey} onClick={(event) => void saveHotkey(null, event.currentTarget)}>清除</Button>

      </div>
      <div className="public-command-feedback" aria-live="polite">
        {error ? <span role="alert" className="plugin-row-error">{error}</span> : busy ? '保存中…' : notice || (command.hotkeyStatus !== 'unset' ? ({ active: '快捷键已启用', suspended: '快捷键已暂停', unavailable: '快捷键不可用' })[command.hotkeyStatus] : '')}
        {unreconciled ? <Button size="small" data-command-refresh aria-label={`刷新命令状态${suffix}`} disabled={busy}
          onClick={(event) => retryRefresh(event.currentTarget)}>刷新状态</Button> : null}
      </div>
    </div>
  )
}

