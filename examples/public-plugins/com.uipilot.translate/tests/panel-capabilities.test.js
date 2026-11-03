import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
const flush = async () => { for (let i=0;i<12;i++) await Promise.resolve() }
const deferred = () => { let resolve, reject; const promise = new Promise((a,b)=>{resolve=a;reject=b}); return {promise,resolve,reject} }
async function setup(options={}) {
  const url = new URL('../package/dist/panel-capabilities.js', import.meta.url)
  const exists = await readFile(url, 'utf8').catch(()=>null)
  assert.ok(exists, 'formal Panel effect adapter exists')
  const { createPanelCapabilities } = await import(url)
  let state={revision:1,text:'hello',translation:'你好\n第二行',sourceLanguage:'en',detectedSourceLanguage:null,targetLanguage:'zh-CHS',status:'success'}, current=true, listener
  const copies=[], plays=[], stops=[], notices=[]; let subscribed=0, unsubscribed=0
  const speech={ voices: async()=>options.voices ?? [{language:'en'},{language:'zh-CHS'}],
    onState(fn){listener=fn;subscribed++;return()=>{unsubscribed++}},
    play(input){plays.push(input);return options.play?.(input) ?? Promise.resolve({playbackId:String(plays.length),status:'playing'})},
    stop(input){stops.push(input);return options.stop?.(input) ?? Promise.resolve({playbackId:input.playbackId,status:'stopped'})},
  }
  const adapter=createPanelCapabilities({facades:{clipboard:{writeText(input){copies.push(input);return options.copy?.(input) ?? Promise.resolve()}},speech},getState:()=>state,isCurrent:revision=>current&&(revision===undefined||revision===state.revision),notify:(...value)=>notices.push(value),render(){}})
  assert.equal(subscribed,0,'factory does not use session facades during onUpdate')
  adapter.start();await flush()
  return {adapter,copies,plays,stops,notices,event:value=>listener(value),state:()=>state,edit(){adapter.invalidate();state={...state,revision:state.revision+1,text:'new',translation:'',status:'idle'}},replace(){current=false;adapter.dispose()},get unsubscribed(){return unsubscribed}}
}
test('copies complete multiline text; success is guarded against revision changes',async()=>{
  const pending=deferred(), a=await setup({copy:()=>pending.promise})
  const job=a.adapter.run('copy','translation',a.state());assert.deepEqual(a.copies,[{text:'你好\n第二行'}]);a.edit();pending.resolve();await job;assert.deepEqual(a.notices,[])
  await a.adapter.run('copy','source',a.state());assert.equal(a.notices[0][0],'已复制')
})
test('clipboard failures map permissions without success or raw errors',async()=>{
  const a=await setup({copy:()=>Promise.reject(Object.assign(new Error('secret'),{name:'PermissionDenied'}))});await a.adapter.run('copy','source',a.state());assert.match(a.notices[0][0],/权限/);assert.equal(JSON.stringify(a.notices).includes('secret'),false)
})
test('play toggles stop and source/target ownership ignores old terminal IDs',async()=>{
  const a=await setup();await a.adapter.run('speak','source',a.state());assert.equal(a.adapter.isPlaying('source'),true)
  await a.adapter.run('speak','translation',a.state());assert.deepEqual(a.stops,[{playbackId:'1'}]);assert.equal(a.adapter.isPlaying('translation'),true)
  a.event({playbackId:'1',status:'completed'});assert.equal(a.adapter.isPlaying('translation'),true)
  await a.adapter.run('speak','translation',a.state());assert.deepEqual(a.stops,[{playbackId:'1'},{playbackId:'2'}]);assert.equal(a.adapter.isPlaying('translation'),false)
})
test('late created obsolete playback is stopped before queued replacement starts',async()=>{
  const pending=deferred(),a=await setup({play:()=>a.plays.length===1?pending.promise:Promise.resolve({playbackId:'B',status:'playing'})})
  const first=a.adapter.run('speak','source',a.state());await flush();const second=a.adapter.run('speak','translation',a.state());await flush();assert.equal(a.plays.length,1)
  pending.resolve({playbackId:'A',status:'playing'});await Promise.all([first,second]);assert.deepEqual(a.stops,[{playbackId:'A'}]);assert.equal(a.adapter.isPlaying('translation'),true)
})
test('completion before play reply never restores playing',async()=>{
  const pending=deferred(),a=await setup({play:()=>pending.promise});const job=a.adapter.run('speak','source',a.state());await flush();a.event({playbackId:'A',status:'completed'});pending.resolve({playbackId:'A',status:'playing'});await job;assert.equal(a.adapter.isPlaying('source'),false)
})
for(const reason of ['edit','replace']) test(`${reason} while play pending stops exact late playback and never publishes`,async()=>{
  const pending=deferred(),a=await setup({play:()=>pending.promise});const job=a.adapter.run('speak','source',a.state());await flush();a[reason]();pending.resolve({playbackId:'late',status:'playing'});await job;assert.deepEqual(a.stops,[{playbackId:'late'}]);assert.equal(a.adapter.isPlaying('source'),false);assert.deepEqual(a.notices,[]);if(reason==='replace')assert.equal(a.unsubscribed,1)
})
test('unknown auto source and missing voice explain unavailability; explicit source works without translation',async()=>{
 const a=await setup({voices:[{language:'en'}]});assert.match(a.adapter.availability('speak','translation',a.state()),/声音/)
 const state={...a.state(),sourceLanguage:'auto',detectedSourceLanguage:null,status:'error'};assert.match(a.adapter.availability('speak','source',state),/语言/)
 assert.equal(a.adapter.availability('speak','source',{...state,sourceLanguage:'en'}),null)
 await a.adapter.run('speak','translation',a.state());assert.equal(a.plays.length,0)
})
test('no browser/private system bypasses in adapter',async()=>{const a=await setup();const text=await readFile(new URL('../package/dist/panel-capabilities.js',import.meta.url),'utf8');assert.doesNotMatch(text,/navigator\.clipboard|speechSynthesis|__TAURI|invoke\(/);a.adapter.dispose()})

test('stale failed stop cannot overwrite replacement completion status',async()=>{
 const pending=deferred(),a=await setup({stop:()=>pending.promise});await a.adapter.run('speak','source',a.state());const stopping=a.adapter.run('speak','source',a.state());await flush();await a.adapter.run('copy','source',a.state());a.adapter.invalidate();pending.reject(new Error('private'));await stopping;assert.deepEqual(a.notices,[['已复制','success']])
})
test('permission-denied play reports fixed feedback',async()=>{
 const a=await setup({play:()=>Promise.reject(Object.assign(new Error('secret'),{name:'PermissionDenied'}))});await a.adapter.run('speak','source',a.state());assert.match(a.notices[0][0],/朗读权限/);assert.equal(a.adapter.isPlaying('source'),false)
})
test('speech completion error maps missing voice without exposing private payload',async()=>{
 const a=await setup();await a.adapter.run('speak','source',a.state());a.event({playbackId:'1',status:'error',errorCode:'SpeechVoiceUnavailable'});assert.match(a.notices[0][0],/声音/);assert.equal(a.adapter.isPlaying('source'),false)
})
test('dispose after started playback stops only its owned ID and unsubscribes',async()=>{
 const a=await setup();await a.adapter.run('speak','source',a.state());a.adapter.dispose();await flush();assert.deepEqual(a.stops,[{playbackId:'1'}]);assert.equal(a.unsubscribed,1)
})
