import { validText } from './providers.js'
// A physical request retains its slot until settlement, even after ownership changes.
export function createPanelModel({ request }) {
 let disposed = false, active = null, pending = null, scheduled = false
 let state = { sessionEpoch: null, requestId: null, ready: false, imageRevision: null, revision: 0, image: null, text: '', status: 'empty', error: null, providerId: 'baidu' }
 const listeners = new Set()
 const publish = () => { for (const listener of listeners) listener({ ...state }) }
 const current = job => !disposed && job.sessionEpoch === state.sessionEpoch && job.revision === state.revision
 function deferPump() {
  if (scheduled || disposed) return
  scheduled = true
  queueMicrotask(() => { scheduled = false; pump() })
 }
 function pump() {
  if (disposed || active || !pending || !state.ready) return
  const job = pending; pending = null
  if (!current(job)) return
  active = job
  let response
  try { response = request({ action: 'recognize', imageId: job.imageId, payload: { providerId: job.providerId } }) }
  catch (error) { response = Promise.reject(error) }
  Promise.resolve(response).then(result => {
   if (!current(job)) return
   if (result?.ok === true && validText(result.text)) state = { ...state, text: result.text, status: 'success', error: null }
   else state = { ...state, text: '', status: 'error', error: { message: result?.ok === false && typeof result.message === 'string' ? result.message : '识别失败，请重试' } }
   publish()
  }).catch(() => {
   if (current(job)) { state = { ...state, text: '', status: 'error', error: { message: '识别失败，请检查授权后重试' } }; publish() }
  }).finally(() => { active = null; deferPump() })
 }
 function invalidate(image = state.image) {
  state = { ...state, image, revision: state.revision + 1, text: '', error: null, status: image ? 'loading' : 'empty' }
  pending = image ? { sessionEpoch: state.sessionEpoch, revision: state.revision, imageId: image.id, providerId: state.providerId } : null
  publish(); deferPump()
 }
 return {
  getState: () => ({ ...state }),
  subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener) },
  installUpdate(update) {
   if (disposed) return
   if (state.sessionEpoch !== update.sessionEpoch) {
    pending = null
    state = { ...state, image: null, text: '', status: 'empty', error: null, imageRevision: null, revision: state.revision + 1 }
   }
   state = { ...state, sessionEpoch: update.sessionEpoch, requestId: update.requestId, ready: false }; publish()
  },
  markReady(event) { if (!disposed && event.sessionEpoch === state.sessionEpoch && event.requestId === state.requestId) { state = { ...state, ready: true }; publish(); deferPump() } },
  imageChanged(event) {
   if (disposed || event.sessionEpoch !== state.sessionEpoch || (state.imageRevision !== null && BigInt(event.revision) <= BigInt(state.imageRevision))) return
   state = { ...state, imageRevision: event.revision }; invalidate(event.image)
  },
  editText(text) { if (!disposed && state.status === 'success') { state = { ...state, text, revision: state.revision + 1 }; publish() } },
  clear() { if (!disposed) invalidate(null) },
  retry() { if (!disposed && state.image && state.ready && state.status !== 'loading') invalidate() },
  setProvider(providerId) { if (providerId !== 'baidu' || state.providerId === providerId) return },
  canCopy: () => !disposed && state.ready && state.status === 'success' && validText(state.text),
  dispose() { if (disposed) return; disposed = true; pending = null; state = { ...state, image: null, text: '', ready: false, revision: state.revision + 1 }; publish(); listeners.clear() },
 }
}
