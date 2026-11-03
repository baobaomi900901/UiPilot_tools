import test from 'node:test'
import assert from 'node:assert/strict'
import { createPanelModel } from '../package/dist/panel-model.js'
function harness() {
  let now = 0
  let sequence = 0
  const timers = new Map()
  const calls = []
  const runTimers = () => {
   for (const [id, timer] of [...timers]) {
     if (timer.at <= now) {
       timers.delete(id)
       timer.fn()
     }
   }
  }
  const model = createPanelModel({
   request: input => new Promise((resolve, reject) => calls.push({ input, resolve, reject })),
   clock: {
     now: () => now,
     setTimeout(fn, delay) {
       const id = ++sequence
       timers.set(id, { fn, at: now + delay })
       return id
     },
     clearTimeout: id => timers.delete(id),
   },
  })
  return {
   model,
   calls,
   async tick(ms = 0) {
     now += ms
     runTimers()
     await Promise.resolve()
     await Promise.resolve()
     runTimers()
     await Promise.resolve()
     await Promise.resolve()
   },
   ready(text = 'hello', sessionEpoch = '1') {
     model.installUpdate({ text, sessionEpoch, requestId: 'r' + sessionEpoch })
     model.markReady({ sessionEpoch, requestId: 'r' + sessionEpoch })
   },
  }
}
const success = (text = '译文', detectedSourceLanguage = 'en') => ({
  ok: true, text, detectedSourceLanguage, targetLanguage: 'zh-CHS',
})
test('host readiness is separate from local revision and edited text keeps debounce',async () => {
  const h=harness();
  h.model.installUpdate({text:'A',sessionEpoch:'1',requestId:'r1'});
  h.model.editText('B');
  h.model.markReady({sessionEpoch:'1',requestId:'wrong'});
  await h.tick(499);
  assert.equal(h.calls.length,0);
  h.model.markReady({sessionEpoch:'1',requestId:'r1'});
  await h.tick();
  assert.equal(h.calls.length,0);
  await h.tick(1);
  assert.equal(h.calls[0].input.payload.text,'B')
})
test('initial text waits for ready and uses business response directly',async () => {
  const h=harness();
  h.ready();
  await h.tick();
  assert.equal(h.calls.length,1);
  h.calls[0].resolve(success());
  await h.tick();
  assert.equal(h.model.getState().translation,'译文');
  assert.equal(h.model.getState().detectedSourceLanguage,'en')
})
test('single request drops A and B and dispatches only latest C',async () => {
  const h=harness();
  h.ready('A');
  await h.tick();
  h.model.editText('B');
  h.model.editText('C');
  await h.tick(500);
  assert.equal(h.calls.length,1);
  h.calls[0].resolve(success('old'));
  await h.tick();
  assert.equal(h.model.getState().translation,'');
  assert.equal(h.calls.length,2);
  assert.equal(h.calls[1].input.payload.text,'C')
})
test('replaced session cannot receive old result or dispatch before matching ready',async () => {
  const h=harness();
  h.ready('A');
  await h.tick();
  h.model.installUpdate({text:'new',sessionEpoch:'2',requestId:'r2'});
  h.calls[0].resolve(success('old'));
  await h.tick();
  assert.equal(h.calls.length,1);
  assert.equal(h.model.getState().translation,'');
  h.model.markReady({sessionEpoch:'2',requestId:'r2'});
  await h.tick();
  assert.equal(h.calls.length,2)
})
test('composition suppresses requests and composition end debounces',async () => {
  const h=harness();
  h.ready('');
  h.model.setComposition(true);
  h.model.editText('hello');
  await h.tick(1000);
  assert.equal(h.calls.length,0);
  h.model.submit();
  await h.tick();
  assert.equal(h.calls.length,0);
  h.model.setComposition(false);
  await h.tick(499);
  assert.equal(h.calls.length,0);
  await h.tick(1);
  assert.equal(h.calls.length,1)
})
test('empty avoids requests; same language still requests authorization',async () => {
  const h=harness();
  h.ready('  ');
  await h.tick();
  assert.equal(h.calls.length,0);
  h.model.setLanguages('en','en');
  h.model.editText('hello');
  h.model.submit();
  await h.tick();
  assert.equal(h.calls.length,1);
  assert.equal(h.calls[0].input.payload.sourceLanguage,'en')
})
test('UTF8 limit includes multibyte text and preserves exact input',async () => {
  const h=harness();
  h.ready('你'.repeat(2730)+'ab');
  await h.tick();
  assert.equal(h.calls.length,1);
  h.calls[0].resolve(success());
  await h.tick();
  h.model.editText('你'.repeat(2731));
  h.model.submit();
  await h.tick();
  assert.equal(h.calls.length,1);
  assert.equal(h.model.getState().error.code,'TEXT_TOO_LONG')
})
test('auto never guesses language; swap needs current detected result',async () => {
  const h=harness();
  h.ready('中文');
  await h.tick();
  assert.equal(h.calls[0].input.payload.sourceLanguage,'auto');
  assert.equal(h.model.canSwap(),false);
  h.calls[0].resolve(success('hello',null));
  await h.tick();
  assert.equal(h.model.canSwap(),false);
  h.model.submit();
  await h.tick();
  h.calls[1].resolve(success('hello','en'));
  await h.tick();
  assert.equal(h.model.canSwap(),true);
  assert.equal(h.model.swap(),true);
  assert.equal(h.model.getState().text,'hello');
  assert.equal(h.model.getState().sourceLanguage,'zh-CHS');
  assert.equal(h.model.getState().targetLanguage,'en');
  assert.equal(h.model.canSwap(),false)
})
test('edits clear results; failure does not retry; dispose invalidates callbacks',async () => {
  const h=harness();
  h.ready();
  await h.tick();
  h.calls[0].resolve(success());
  await h.tick();
  h.model.editText('new');
  assert.equal(h.model.getState().translation,'');
  assert.equal(h.model.getState().detectedSourceLanguage,null);
  await h.tick(500);
  h.calls[1].reject(new Error('PRIVATE'));
  await h.tick(1000);
  assert.equal(h.calls.length,2);
  assert.doesNotMatch(JSON.stringify(h.model.getState()),/PRIVATE/);
  h.model.submit();
  await h.tick();
  h.model.dispose();
  h.calls[2].resolve(success());
  await h.tick();
  assert.equal(h.model.getState().translation,'')})

test('latest queued edit still waits its remaining debounce after active completion',async () => {
  const h=harness();
  h.ready('A');
  await h.tick();
  h.model.editText('B');
  await h.tick(100);
  h.calls[0].resolve(success('old'));
  await h.tick();
  assert.equal(h.calls.length,1);
  await h.tick(399);
  assert.equal(h.calls.length,1);
  await h.tick(1);
  assert.equal(h.calls[1].input.payload.text,'B')
})
test('preferences initialize atomically and state snapshots do not mutate model',async () => {
  const h=harness();
  h.model.installUpdate({sessionEpoch:'1',requestId:'r1',text:'hello',preferences:{sourceLanguage:'en',targetLanguage:'en',providerId:'youdao',text:'private'}});
  h.model.markReady({sessionEpoch:'1',requestId:'r1'});
  await h.tick();
  assert.equal(h.calls[0].input.payload.targetLanguage,'en');
  assert.deepEqual(h.model.getPreferences(),{sourceLanguage:'en',targetLanguage:'en',providerId:'youdao'});
  const snapshot=h.model.getState();
  snapshot.text='bad';
  assert.equal(h.model.getState().text,'hello')})
test('same epoch update with new request supersedes prior ready identity',async () => {
  const h=harness();
  h.model.installUpdate({text:'old',sessionEpoch:'18446744073709551615',requestId:'first'});
  h.model.installUpdate({text:'new',sessionEpoch:'18446744073709551615',requestId:'second'});
  h.model.markReady({sessionEpoch:'18446744073709551615',requestId:'first'});
  await h.tick();
  assert.equal(h.calls.length,0);
  h.model.markReady({sessionEpoch:'18446744073709551615',requestId:'second'});
  await h.tick();
  assert.equal(h.calls[0].input.payload.text,'new')})



test('preserves bounded structured Runtime failure while bridge exceptions stay generic', async () => {
  const h = harness()
  h.ready()
  await h.tick()
  h.calls[0].resolve({ok:false,code:'PERMISSION_DENIED',message:'请在设置中授权翻译服务'})
  await h.tick()
  assert.deepEqual(h.model.getState().error,{code:'PERMISSION_DENIED',message:'请在设置中授权翻译服务'})
})
