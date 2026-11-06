import assert from 'node:assert/strict'
import test from 'node:test'
const flush = () => new Promise(resolve => setImmediate(resolve))
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done }); return { promise, resolve } }
async function setup(t) {
 const { createPanelModel } = await import('../package/dist/panel-model.js'); const jobs = []
 const model = createPanelModel({ request(input) { const job = deferred(); jobs.push({ ...job, input }); return job.promise } }); t.after(model.dispose)
 const update = (requestId = 'r1', sessionEpoch = '1') => model.installUpdate({ requestId, sessionEpoch })
 const ready = (requestId = 'r1', sessionEpoch = '1') => model.markReady({ requestId, sessionEpoch })
 const changed = (id, revision = '1', sessionEpoch = '1') => model.imageChanged({ sessionEpoch, revision, image: id ? { id, previewDataUrl: 'data:image/png;base64,AAAA', width: 100, height: 100 } : null })
 update(); return { model, jobs, update, ready, changed }
}
test('ACK cannot request; matching Ready starts exactly once; same source no request', async t => {
 const a = await setup(t); await flush(); assert.equal(a.jobs.length, 0); a.changed('A'); await flush(); assert.equal(a.jobs.length, 0)
 a.ready('stale'); await flush(); assert.equal(a.jobs.length, 0); a.ready(); await flush(); assert.equal(a.jobs.length, 1)
 a.ready(); a.changed('A'); a.model.setProvider('baidu'); await flush(); assert.equal(a.jobs.length, 1)
})
test('one physical active request plus latest pending A B C: only C displays', async t => {
 const a = await setup(t); a.ready(); a.changed('A'); await flush(); a.changed('B', '2'); a.changed('C', '3'); await flush()
 assert.equal(a.jobs.length, 1); assert.equal(a.model.getState().text, '')
 a.jobs[0].resolve({ ok: true, text: 'STALE', lines: [] }); await flush(); await flush()
 assert.equal(a.jobs.length, 2); assert.equal(a.jobs[1].input.imageId, 'C'); assert.equal(a.model.getState().text, '')
 a.jobs[1].resolve({ ok: true, text: 'CURRENT', lines: [] }); await flush(); assert.equal(a.model.getState().text, 'CURRENT')
})
test('same-session update preserves edited text/image and cannot repeat OCR', async t => {
 const a = await setup(t); a.ready(); a.changed('A'); await flush(); a.jobs[0].resolve({ ok: true, text: 'original', lines: [] }); await flush()
 a.model.editText('edited\n全文'); a.update('theme'); a.ready('theme'); await flush()
 assert.equal(a.model.getState().text, 'edited\n全文'); assert.equal(a.model.getState().image.id, 'A'); assert.equal(a.jobs.length, 1)
})
test('clear and replacement revoke pending/results while preserving physical single flight', async t => {
 const a = await setup(t); a.ready(); a.changed('A'); await flush(); a.changed('B', '2'); a.model.clear()
 assert.equal(a.model.getState().text, ''); assert.equal(a.model.getState().image, null)
 a.update('new', '2'); a.ready('new', '2'); a.changed('N', '1', '2'); await flush(); assert.equal(a.jobs.length, 1)
 a.jobs[0].resolve({ ok: false, message: 'OLD ERROR' }); await flush(); await flush(); assert.equal(a.jobs.length, 2); assert.equal(a.jobs[1].input.imageId, 'N'); assert.equal(a.model.getState().error, null)
})
test('failure disables copy, explicit retry, length bounds, disposal ignores response', async t => {
 const a = await setup(t); a.ready(); a.changed('A'); await flush(); a.jobs[0].resolve({ ok: false, code: 'empty', message: '未识别到文字' }); await flush()
 assert.equal(a.model.canCopy(), false); await flush(); assert.equal(a.jobs.length, 1); a.model.retry(); await flush(); assert.equal(a.jobs.length, 2)
 a.jobs[1].resolve({ ok: true, text: 'x'.repeat(16000), lines: [] }); await flush(); assert.equal(a.model.canCopy(), true)
 a.model.editText('汉'.repeat(6000)); assert.equal(a.model.canCopy(), false)
 a.changed('B', '2'); await flush(); a.model.dispose(); a.jobs[2].resolve({ ok: true, text: 'LATE', lines: [] }); await flush(); assert.equal(a.model.getState().text, '')
})
test('same-session update between scheduled pump and request cannot lose the current image', async t => {
 const a = await setup(t); a.ready(); a.changed('A'); queueMicrotask(() => a.update('theme')); await flush()
 assert.notEqual(a.model.getState().status, 'error'); a.ready('theme'); await flush(); assert.equal(a.jobs.length, 1)
})
