import { Camera, Copy, Download, RotateCcw, X } from 'lucide-react'
import { useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent } from 'react'
import type {ScreenshotCore} from './screenshot-core'
import type {UiColorScheme} from './ui-theme'
import './ui-theme.css'
import './screenshot.css'
export function ScreenshotView({core}:{core:ScreenshotCore}) {
 const snapshot=useSyncExternalStore(core.subscribe,core.getSnapshot,core.getSnapshot)
 const root=useRef<HTMLElement>(null)
 const [actualSize,setActualSize]=useState(false)
 const scheme:UiColorScheme=snapshot.state?.theme ?? 'light'
 const busy=snapshot.pending || !snapshot.state || snapshot.state.phase!=='preview'
 const disabled=busy || !snapshot.imageUrl || snapshot.loading
 useEffect(()=>{root.current?.focus()},[])
 const keyDown=(event:KeyboardEvent)=>{
  if(event.nativeEvent.isComposing || event.keyCode===229 || event.altKey || event.metaKey)return
  const key=event.key.toLowerCase()
  if(event.key==='Escape' && !event.ctrlKey){event.preventDefault();if(!event.repeat)void core.close();return}
  if(!event.ctrlKey || event.shiftKey || (key!=='c' && key!=='s'))return
  event.preventDefault()
  if(event.repeat || disabled)return
  if(key==='c')void core.copy();else void core.save()
 }
 const phase=snapshot.state?.phase
 const status=snapshot.error ?? snapshot.notice ?? (phase==='copying'?'正在复制…':phase==='saving'?'正在保存…':snapshot.loading?'正在加载截图…':phase==='preparing'?'正在准备截图…':phase==='selecting'?'请选择截图区域':'')
 return <main ref={root} tabIndex={-1} className="screenshot-shell" data-color-scheme={scheme} onKeyDown={keyDown} aria-label="截图预览" aria-busy={busy || snapshot.loading}>
  <header className="screenshot-titlebar" data-tauri-drag-region>
   <Camera size={18} aria-hidden/><strong data-tauri-drag-region>截图</strong><span className="screenshot-spacer" data-tauri-drag-region/>
   <button className="screenshot-close" aria-label="关闭截图" title="关闭截图 (Esc)" onClick={()=>void core.close()}><X size={16} aria-hidden/></button>
  </header>
  <div className="screenshot-toolbar" role="toolbar" aria-label="截图操作">
   <span className="screenshot-dimensions">{snapshot.state?.width ? `${snapshot.state.width} × ${snapshot.state.height} 像素`:'截图预览'}</span>
   <span className="screenshot-spacer"/>
   <button disabled={busy} aria-label="重新截图" onClick={()=>{setActualSize(false);void core.retake()}}><RotateCcw size={16} aria-hidden/>重新截图</button>
   <button disabled={disabled} aria-label="复制截图" aria-keyshortcuts="Control+c" title="复制截图 (Ctrl+C)" onClick={()=>void core.copy()}><Copy size={16} aria-hidden/>复制</button>
   <button className="screenshot-primary" disabled={disabled} aria-label="保存截图" aria-keyshortcuts="Control+s" title="保存截图 (Ctrl+S)" onClick={()=>void core.save()}><Download size={16} aria-hidden/>保存</button>
  </div>
  <div className={`screenshot-canvas${actualSize?' screenshot-actual-size':''}`}>
   {snapshot.imageUrl ? <img src={snapshot.imageUrl} alt="截图预览" width={snapshot.state?.width ?? undefined} height={snapshot.state?.height ?? undefined} onDoubleClick={()=>setActualSize(v=>!v)} title="双击切换原始尺寸预览"/>:<div className="screenshot-empty"><Camera size={28} aria-hidden/><span>{snapshot.error?'截图加载失败':'正在加载截图…'}</span></div>}
  </div>
  <footer className="screenshot-status"><span role={snapshot.error?'alert':'status'} aria-live="polite" className={snapshot.error?'screenshot-error':''}>{status}</span><span className="screenshot-spacer"/>{snapshot.imageUrl ? <button className="screenshot-scale" aria-label="原始尺寸预览" aria-pressed={actualSize} onClick={()=>setActualSize(v=>!v)}>{actualSize?'原始尺寸':'适应窗口'}</button>:null}</footer>
 </main>
}
