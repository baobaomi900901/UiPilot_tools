import { isLanguage, sanitizePreferences } from './languages.js'

/** Pure scheduler: request resolves business data directly, never the Runtime envelope. */
export function createPanelModel({ request, preferences, clock = { now: () => Date.now(), setTimeout: (callback, delay) => globalThis.setTimeout(callback, delay), clearTimeout: id => globalThis.clearTimeout(id) } }) {
 let state = { ...sanitizePreferences(preferences), text: '', translation: '', detectedSourceLanguage: null, status: 'idle', error: null, speaking: null, sessionEpoch: null, revision: 0 }
 let identity = null, ready = false, disposed = false, composing = false, pending = null, active = null, timer = null, generation = 0
 let compositionWork = false
 const listeners = new Set()
 const getState = () => ({ ...state, error: state.error && { ...state.error } })
 const emit = () => { for (const listener of listeners) listener(getState()) }
 const clearTimer = () => { if (timer !== null) clock.clearTimeout(timer); timer = null }
 function invalidate() {
  state = { ...state, revision: state.revision + 1, translation: '', detectedSourceLanguage: null, status: 'idle', error: null, speaking: null }
  pending = null
  clearTimer()
 }
 function schedule(delay) {
  clearTimer()
  pending = null
  if (disposed || !state.text.trim()) { emit(); return }
  if (composing) { compositionWork = true; emit(); return }
  if (new TextEncoder().encode(state.text).length > 8192) {
   state.status = 'error'; state.error = { code: 'TEXT_TOO_LONG', message: '原文不能超过 8192 字节' }; emit(); return
  }
  pending = { generation, revision: state.revision, due: clock.now() + delay, payload: { providerId: state.providerId, text: state.text, sourceLanguage: state.sourceLanguage, targetLanguage: state.targetLanguage } }
  emit()
  // Always defer: neither Host onUpdate nor markReady invokes request synchronously.
  timer = clock.setTimeout(pump, delay)
 }
 function pump() {
  clearTimer()
  if (disposed || composing || !ready || active || !pending) return
  if (pending.due > clock.now()) { timer = clock.setTimeout(pump, pending.due - clock.now()); return }
  const job = pending; pending = null; active = job
  state.status = 'loading'; emit()
  let response
  try { response = request({ action: 'translate', payload: { ...job.payload } }) } catch { response = Promise.reject(new Error('request failed')) }
  Promise.resolve(response).then(result => {
   if (disposed || job.generation !== generation || job.revision !== state.revision) return
   if (result?.ok === true && typeof result.text === 'string' && result.targetLanguage === state.targetLanguage) {
    state.translation = result.text
    state.detectedSourceLanguage = isLanguage(result.detectedSourceLanguage) ? result.detectedSourceLanguage : null
    state.status = 'success'; state.error = null
   } else {
    state.status = 'error'
    state.error = result?.ok === false && typeof result.code === 'string' && /^[A-Z][A-Z0-9_]{0,63}$/.test(result.code) && typeof result.message === 'string' && result.message.length > 0 && result.message.length <= 256
     ? { code: result.code, message: result.message }
     : { code: 'TRANSLATION_FAILED', message: '翻译失败，请重试' }
   }
  }, () => {
   if (!disposed && job.generation === generation && job.revision === state.revision) {
    state.status = 'error'; state.error = { code: 'TRANSLATION_FAILED', message: '翻译失败，请重试' }
   }
  }).finally(() => {
   if (active === job) active = null
   if (!disposed) { emit(); if (pending) timer = clock.setTimeout(pump, Math.max(0, pending.due - clock.now())) }
  })
 }
 const getPreferences = () => sanitizePreferences(state)
 const canSwap = () => state.status === 'success' && Boolean(state.translation) && isLanguage(state.sourceLanguage === 'auto' ? state.detectedSourceLanguage : state.sourceLanguage)
 return {
  getState, getPreferences, canSwap,
  subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener) },
  installUpdate(update) {
   if (disposed) return
   generation++; ready = false; composing = false; compositionWork = false
   identity = { sessionEpoch: update.sessionEpoch, requestId: update.requestId }
   invalidate()
   state = { ...state, ...(update.preferences === undefined ? {} : sanitizePreferences(update.preferences)), sessionEpoch: update.sessionEpoch, text: typeof update.text === 'string' ? update.text : '' }
   schedule(0)
  },
  markReady(notice) {
   if (disposed || !identity || notice?.sessionEpoch !== identity.sessionEpoch || notice?.requestId !== identity.requestId) return
   ready = true
   if (pending) { clearTimer(); timer = clock.setTimeout(pump, Math.max(0, pending.due - clock.now())) }
  },
  editText(text) { if (disposed) return; invalidate(); state.text = String(text); schedule(500) },
  setComposition(value) {
   if (disposed || composing === Boolean(value)) return
   composing = Boolean(value)
   if (composing) {
    // Preserve only work that was actually waiting before composition began.
    compositionWork = pending !== null
    clearTimer()
    pending = null
    return
   }
   const resume = compositionWork
   compositionWork = false
   if (resume) schedule(500)
  },
  submit() { if (!disposed && !composing) { invalidate(); schedule(0) } },
  setLanguages(sourceLanguage, targetLanguage) {
   if (disposed) return
   const next = sanitizePreferences({ ...state, sourceLanguage, targetLanguage })
   if (next.sourceLanguage === state.sourceLanguage && next.targetLanguage === state.targetLanguage) return
   invalidate(); Object.assign(state, next); schedule(0)
  },
  setProvider(providerId) {
   if (disposed) return
   const next = sanitizePreferences({ ...state, providerId })
   if (next.providerId === state.providerId) return
   invalidate(); state.providerId = next.providerId; schedule(0)
  },
  swap() {
   if (disposed || !canSwap()) return false
   const source = state.sourceLanguage === 'auto' ? state.detectedSourceLanguage : state.sourceLanguage
   const text = state.translation, target = state.targetLanguage
   invalidate(); Object.assign(state, { text, sourceLanguage: target, targetLanguage: source }); schedule(0); return true
  },
  dispose() { disposed = true; generation++; clearTimer(); pending = null; listeners.clear(); state.speaking = null },
 }
}
