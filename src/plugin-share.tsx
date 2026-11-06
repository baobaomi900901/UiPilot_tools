import { useEffect, useRef, useState } from 'react'
import type { PluginShareView } from './protocol'

export function ShareSurface({ share, select, close }: { share: PluginShareView; select: (plugin: string, receiver: string) => void; close: () => void }) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(-1)
  const root = useRef<HTMLDivElement>(null)
  const search = useRef<HTMLInputElement>(null)
  const rows = share.receivers.filter(r => `${r.pluginName} ${r.title}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
  useEffect(() => { setQuery(''); setActive(-1) }, [share.transferId])
  useEffect(() => { if (share.phase === 'choosing') search.current?.focus(); else root.current?.focus() }, [share.phase, share.transferId])
  const choose = (index: number) => { const row = rows[index]; if (row && !row.disabledReason) select(row.pluginId, row.receiverId) }
  return <div className="plugin-share-surface" role="dialog" aria-modal="true" aria-label={share.phase === 'choosing' ? '分享文字' : '接收分享'} ref={root} tabIndex={-1}
    onKeyDown={event => {
      event.stopPropagation()
      if (event.isDefaultPrevented() || event.nativeEvent.isComposing || event.keyCode === 229) return
      if (event.key === 'Escape' || (event.ctrlKey && event.key.toLowerCase() === 'k')) { event.preventDefault(); if (!event.repeat) close(); return }
      if (event.key === 'Tab') {
        const items = [...root.current!.querySelectorAll<HTMLElement>('input,button:not(:disabled)')]
        const index = items.indexOf(document.activeElement as HTMLElement)
        event.preventDefault(); items[(index + (event.shiftKey ? -1 : 1) + items.length) % items.length]?.focus(); return
      }
      if (share.phase !== 'choosing') return
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setActive(rows.length ? active < 0 ? (event.key === 'ArrowDown' ? 0 : rows.length - 1) : (active + (event.key === 'ArrowDown' ? 1 : -1) + rows.length) % rows.length : -1) }
      if (event.key === 'Enter' && event.target === search.current) { event.preventDefault(); if (!event.repeat) choose(active) }
    }}>
    {share.phase === 'choosing' ? <>
      <div className="plugin-share-toolbar"><input ref={search} aria-label="搜索分享接收者" placeholder="搜索接收者" value={query} onChange={event => { setQuery(event.target.value); setActive(-1) }} /><button type="button" onClick={close}>取消</button></div>
      <div className="plugin-share-receivers" role="listbox" aria-label="分享接收者" aria-activedescendant={active < 0 ? undefined : `share-receiver-${active}`}>
        {rows.map((row, index) => <button type="button" role="option" id={`share-receiver-${index}`} aria-selected={active === index} key={`${row.pluginId}/${row.receiverId}`} disabled={!!row.disabledReason} title={row.disabledReason ?? undefined}
          onMouseMove={() => setActive(index)} onFocus={() => setActive(index)} onClick={() => choose(index)}>
          <span>{row.title}<small>{row.pluginName}</small></span><small>{row.disabledReason ?? `${row.maxBytes} 字节`}</small>
        </button>)}
        {!rows.length && <p role="status">没有兼容的接收者</p>}
      </div>
    </> : <div className="plugin-share-message"><p role={share.phase === 'error' ? 'alert' : 'status'}>{share.phase === 'error' ? share.error : '正在打开接收者…'}</p><button type="button" onClick={close}>关闭</button></div>}
  </div>
}
