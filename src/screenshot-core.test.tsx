// @vitest-environment jsdom
import {describe,it,expect,vi} from 'vitest'
import {createScreenshotCore} from './screenshot-core'
import type {ScreenshotClient, ScreenshotSnapshot} from './screenshot-protocol'
export const frame = (revision='1',generation='1'): ScreenshotSnapshot => ({sessionId:'1',generation,windowInstance:'2',revision,phase:'preview',mode:'region',width:100,height:50,error:null,theme:'light'})
function harness() {
 let push: (v:unknown)=>void = ()=>{}
 const client: ScreenshotClient = {listenState:vi.fn(async h=>{push=h;return vi.fn()}),getState:vi.fn(async()=>frame()),readImage:vi.fn(async()=>new ArrayBuffer(2)),releaseImage:vi.fn(async()=>{}),copy:vi.fn(async()=>({status:'copied' as const})),save:vi.fn(async()=>({status:'cancelled' as const})),retake:vi.fn(async()=>{}),close:vi.fn(async()=>{})}
 const urls = {create:vi.fn(()=>`blob:${Math.random()}`),revoke:vi.fn()}
 return {client,urls,push:(v:unknown)=>push(v),core:createScreenshotCore(client,'2',urls)}
}
describe('screenshot ownership and outputs',()=>{
 it('acknowledges transfers after Blob creation and also after stale completion or read failure',async()=>{
  const h=harness();let resolveA!:(v:ArrayBuffer)=>void
  h.client.readImage=vi.fn().mockImplementationOnce(()=>new Promise(r=>resolveA=r)).mockResolvedValue(new ArrayBuffer(3))
  await h.core.start();const [ownerA,idA]=vi.mocked(h.client.readImage).mock.calls[0]
  expect(h.client.releaseImage).not.toHaveBeenCalled()
  h.push(frame('2','2'));await vi.waitFor(()=>expect(h.urls.create).toHaveBeenCalledTimes(1))
  expect(h.client.releaseImage).toHaveBeenCalledWith({sessionId:'1',generation:'2',windowInstance:'2'},expect.any(String))
  resolveA(new ArrayBuffer(2));await vi.waitFor(()=>expect(h.client.releaseImage).toHaveBeenCalledWith(ownerA,idA))
  expect(h.urls.create).toHaveBeenCalledTimes(1)
  h.client.readImage=vi.fn().mockRejectedValue({code:'resourceLimit'})
  h.push(frame('3','3'));await vi.waitFor(()=>expect(h.client.releaseImage).toHaveBeenCalledTimes(3))
 })
 it('never copies or saves B while displaying A after B preview loading fails',async()=>{
  const h=harness();await h.core.start();await Promise.resolve();const original=h.core.getSnapshot().imageUrl
  h.client.readImage=vi.fn().mockRejectedValue({code:'resourceLimit'})
  h.push(frame('2','2'));await vi.waitFor(()=>expect(h.core.getSnapshot().loading).toBe(false))
  expect(h.core.getSnapshot().imageUrl).toBeNull();expect(h.urls.revoke).toHaveBeenCalledWith(original)
  await h.core.copy();await h.core.save();expect(h.client.copy).not.toHaveBeenCalled();expect(h.client.save).not.toHaveBeenCalled()
 })
 it('acknowledges a late transfer even after the core is destroyed',async()=>{
  const h=harness();let resolve!:(v:ArrayBuffer)=>void
  h.client.readImage=vi.fn(()=>new Promise<ArrayBuffer>(r=>resolve=r));await h.core.start()
  const [owner,id]=vi.mocked(h.client.readImage).mock.calls[0]
  h.core.destroy();resolve(new ArrayBuffer(3))
  await vi.waitFor(()=>expect(h.client.releaseImage).toHaveBeenCalledWith(owner,id));expect(h.urls.create).not.toHaveBeenCalled()
 })
 it('subscribes before fetching and rejects older bootstrap snapshot',async()=>{
  const h=harness(); h.client.getState=vi.fn(async()=>{h.push(frame('3'));return frame('1')})
  await h.core.start(); expect(h.core.getSnapshot().state?.revision).toBe('3');expect(h.client.readImage).toHaveBeenCalledTimes(1)
 })
 it('drops asynchronous A after B and releases replaced and destroyed resources',async()=>{
  const h=harness();let resolveA!:(v:ArrayBuffer)=>void
  h.client.readImage=vi.fn().mockImplementationOnce(()=>new Promise(r=>resolveA=r)).mockResolvedValue(new ArrayBuffer(3))
  await h.core.start();h.push(frame('2','2'));await Promise.resolve();await Promise.resolve()
  const url=h.core.getSnapshot().imageUrl;expect(url).toBeTruthy()
  resolveA(new ArrayBuffer(2));await Promise.resolve();expect(h.core.getSnapshot().imageUrl).toBe(url);expect(h.urls.create).toHaveBeenCalledTimes(1)
  h.push(frame('3','3'));await Promise.resolve();await Promise.resolve();expect(h.urls.revoke).toHaveBeenCalledWith(url)
  h.core.destroy();expect(h.urls.revoke).toHaveBeenCalledTimes(2)
 })
 it('keeps one image for copy revisions and cancelled save, permits retry after failure',async()=>{
  const h=harness();await h.core.start();await Promise.resolve(); const url=h.core.getSnapshot().imageUrl
  await h.core.copy();h.push({...frame('2'),phase:'copying'});h.push(frame('3'));await h.core.save()
  expect(h.core.getSnapshot().imageUrl).toBe(url);expect(h.client.readImage).toHaveBeenCalledTimes(1)
  h.client.save=vi.fn().mockRejectedValueOnce({code:'saveFailed'}).mockResolvedValue({status:'saved'})
  await h.core.save();expect(h.core.getSnapshot().error).toContain('保存');await h.core.save();expect(h.core.getSnapshot().notice).toBe('已保存')
 })
 it('retains old preview during retake and restores cancelled retake without losing ownership',async()=>{
  const h=harness();await h.core.start();await Promise.resolve();const original=h.core.getSnapshot().imageUrl
  await h.core.retake();h.push({...frame('2','2'),phase:'selecting'});expect(h.core.getSnapshot().imageUrl).toBe(original)
  h.push(frame('3','2'));await Promise.resolve();await Promise.resolve();expect(h.core.getSnapshot().imageUrl).toBeTruthy();expect(h.urls.revoke).toHaveBeenCalledWith(original)
  await h.core.copy();expect(h.client.copy).toHaveBeenLastCalledWith({sessionId:'1',generation:'2',windowInstance:'2'})
 })
 it('releases late listener registration and ignores late binary data after destruction',async()=>{
  const h=harness();let resolve!:(v:ArrayBuffer)=>void;h.client.readImage=vi.fn(()=>new Promise<ArrayBuffer>(r=>resolve=r))
  await h.core.start();h.core.destroy();resolve(new ArrayBuffer(3));await Promise.resolve();expect(h.urls.create).not.toHaveBeenCalled()
  const late=harness();let registered!:(off:()=>void)=>void;const off=vi.fn();late.client.listenState=vi.fn(()=>new Promise<()=>void>(r=>registered=r))
  const start=late.core.start();late.core.destroy();registered(off);await start;expect(off).toHaveBeenCalledTimes(1);expect(late.client.getState).not.toHaveBeenCalled()
 })
 it('blocks repeated outputs and retake while pending but forwards close for backend deferral',async()=>{
  const h=harness();await h.core.start();let done!:()=>void;h.client.copy=vi.fn(()=>new Promise<{status:'copied'}>(r=>{done=()=>r({status:'copied' as const})}))
  const pending=h.core.copy();await h.core.copy();await h.core.save();await h.core.retake();await h.core.close()
  expect(h.client.copy).toHaveBeenCalledTimes(1);expect(h.client.save).not.toHaveBeenCalled();expect(h.client.retake).not.toHaveBeenCalled();expect(h.client.close).toHaveBeenCalledTimes(1);done();await pending
 })
})
