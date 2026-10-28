// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {describe,it,expect,vi} from 'vitest'
import {ScreenshotView} from './screenshot-view'
import type {ScreenshotCore, ScreenshotViewSnapshot} from './screenshot-core'
const state={sessionId:'1',generation:'1',windowInstance:'2',revision:'1',phase:'preview' as const,mode:'region' as const,width:100,height:50,error:null,theme:'light' as const}
describe('screenshot view',()=>{
 it.each(['light','dark'] as const)('exposes actions, dimensions and scoped keyboard in %s',async theme=>{
  const snapshot:ScreenshotViewSnapshot={state:{...state,theme},imageUrl:'blob:image',loading:false,pending:false,error:null,notice:null}
  const core:ScreenshotCore={getSnapshot:()=>snapshot,subscribe:()=>()=>{},start:vi.fn(async()=>{}),copy:vi.fn(async()=>{}),save:vi.fn(async()=>{}),retake:vi.fn(async()=>{}),close:vi.fn(async()=>{}),destroy:vi.fn()}
  const host=document.createElement('div');document.body.append(host);const root=createRoot(host)
  await act(async()=>root.render(<ScreenshotView core={core}/>))
  expect(host.textContent).toContain('100 × 50');expect(host.querySelector('img')?.getAttribute('src')).toBe('blob:image')
  const buttons=[...host.querySelectorAll('button')];expect(buttons.map(b=>b.getAttribute('aria-label'))).toEqual(['关闭截图','重新截图','复制截图','保存截图','原始尺寸预览'])
  expect(host.querySelector('[data-color-scheme]')?.getAttribute('data-color-scheme')).toBe(theme)
  await act(async()=>host.querySelector('main')?.dispatchEvent(new KeyboardEvent('keydown',{key:'c',ctrlKey:true,bubbles:true})))
  expect(core.copy).toHaveBeenCalledTimes(1)
  await act(async()=>host.querySelector('main')?.dispatchEvent(new KeyboardEvent('keydown',{key:'c',ctrlKey:true,repeat:true,bubbles:true})))
  expect(core.copy).toHaveBeenCalledTimes(1)
  await act(async()=>host.querySelector('main')?.dispatchEvent(new KeyboardEvent('keydown',{key:'s',ctrlKey:true,isComposing:true,bubbles:true})))
  expect(core.save).not.toHaveBeenCalled()
  await act(async()=>window.dispatchEvent(new KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true})))
  expect(core.save).not.toHaveBeenCalled()
  await act(async()=>host.querySelector('main')?.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})))
  expect(core.close).toHaveBeenCalledTimes(1)
  snapshot.pending=true
  await act(async()=>root.render(<ScreenshotView core={{...core}}/>))
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="复制截图"]')?.disabled).toBe(true)
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="保存截图"]')?.disabled).toBe(true)
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="重新截图"]')?.disabled).toBe(true)
  expect(host.querySelector<HTMLButtonElement>('button[aria-label="关闭截图"]')?.disabled).toBe(false)
  await act(async()=>host.querySelector('main')?.dispatchEvent(new KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true})))
  expect(core.save).not.toHaveBeenCalled()
  await act(async()=>root.unmount());host.remove()
 })
})
