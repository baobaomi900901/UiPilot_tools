import { createPanelModel } from './panel-model.js'
import { PROVIDERS, validText } from './providers.js'
/* Paths from installed lucide-react 1.21.0. License:
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

*/

const ICONS = {
 copy: [['rect', { width: '14', height: '14', x: '8', y: '8', rx: '2', ry: '2' }], ['path', { d: 'M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2' }]],
 chevron: [['path', { d: 'm6 9 6 6 6-6' }]],
 close: [['path', { d: 'M18 6 6 18' }], ['path', { d: 'm6 6 12 12' }]],
 image: [['rect', { width: '18', height: '18', x: '3', y: '3', rx: '2', ry: '2' }], ['circle', { cx: '9', cy: '9', r: '2' }], ['path', { d: 'm21 15-4-4a2 2 0 0 0-2.828 0L6 21' }]],
}
export function mountPanel(document, host) {
 const node = id => document.getElementById(id), view = document.defaultView
 const output = node('ocr-text'), dialog = node('source-dialog'), cleanup = []
 let session = null, disposed = false, modal = false, copying = null, clearing = null
 const listen = (target, name, handler) => { target.addEventListener(name, handler); cleanup.push(() => target.removeEventListener(name, handler)) }
 const current = owner => !disposed && owner && owner === session
 const model = createPanelModel({ request: input => session.facades.request(input) })
 function notify(owner, message, tone = 'error') {
  if (!current(owner) || !owner.ready) return
  try { Promise.resolve(owner.facades.setStatus({ message, tone })).catch(() => {}) } catch { /* Owner may expire after guard. */ }
 }
 function focusHost(owner) {
  if (!current(owner)) return
  try { Promise.resolve(owner.facades.focusHostInput()).catch(() => {}) } catch { /* Session expired. */ }
 }
 for (const target of document.querySelectorAll('[data-icon]')) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  for (const [key, value] of Object.entries({ viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' })) svg.setAttribute(key, value)
  for (const [tag, attributes] of ICONS[target.dataset.icon]) {
   const shape = document.createElementNS(svg.namespaceURI, tag)
   for (const [key, value] of Object.entries(attributes)) shape.setAttribute(key, value)
   svg.append(shape)
  }
  target.append(svg)
 }
 function render(state = model.getState()) {
  if (disposed) return
  if (output.value !== state.text) output.value = state.text
  output.readOnly = state.status !== 'success'
  const preview = node('image')
  if (state.image) {
   if (preview.getAttribute('src') !== state.image.previewDataUrl) preview.src = state.image.previewDataUrl
   // Do not enlarge a preview beyond its own <=512px natural dimensions.
  } else preview.removeAttribute('src')
  preview.hidden = !state.image; node('image-empty').hidden = Boolean(state.image)
  node('clear-image').disabled = !state.image || !session?.ready || Boolean(clearing)
  node('output-body').setAttribute('aria-busy', String(state.status === 'loading'))
  const status = node('ocr-status')
  status.textContent = state.status === 'loading' ? '正在识别…' : state.status === 'error' ? state.error.message : state.status === 'success' && state.text && !validText(state.text) ? '文字过长或格式无效，无法复制全文' : '等待图片'
  status.dataset.error = String(state.status === 'error' || (state.status === 'success' && state.text && !validText(state.text)))
  node('result-notice').hidden = state.status === 'success' && (!state.text || validText(state.text))
  node('retry').hidden = state.status !== 'error'; node('retry').disabled = !session?.ready
  node('copy').disabled = !model.canCopy() || Boolean(copying) || Boolean(clearing)
  node('copy').title = node('copy').disabled ? '复制文字：需要本次有效且不超过 16 KiB 的文字' : '复制文字'
 }
 function closeSource(restore = true) {
  if (!modal) return
  modal = false; node('provider').setAttribute('aria-expanded', 'false')
  if (typeof dialog.close === 'function') dialog.close(); else dialog.removeAttribute('open')
  if (restore) node('provider').focus()
 }
 function copy() {
  const owner = session, state = model.getState()
  if (!current(owner) || !owner.ready || !model.canCopy() || copying || clearing || !owner.facades.clipboard) return
  // Read the live editor at the explicit user action, never a cached provider response.
  const text = output.value
  if (!validText(text)) return
  const job = { owner, revision: state.revision }; copying = job; render()
  const valid = () => current(owner) && owner.ready && copying === job && model.getState().revision === job.revision
  Promise.resolve().then(() => { if (valid()) return owner.facades.clipboard.writeText({ text }) }).then(() => {
   if (valid()) notify(owner, '已复制', 'success')
  }).catch(() => { if (valid()) notify(owner, '复制失败，请重试') }).finally(() => { if (copying === job) { copying = null; render() } })
 }
 function clearImage() {
  const owner = session
  if (!current(owner) || !owner.ready || !model.getState().image || clearing) return
  const job = { owner }; clearing = job; copying = null; model.clear()
  Promise.resolve().then(() => { if (current(owner) && clearing === job) return owner.facades.clearImage() }).catch(() => {
   if (current(owner) && clearing === job) notify(owner, '清除图片失败，请重新粘贴图片')
  }).finally(() => { if (clearing === job) { clearing = null; render() } })
 }
 function focusOrder() { return ['clear-image', 'ocr-text', 'retry', 'copy', 'provider'].map(node).filter(element => !element.hidden && !element.disabled) }
 listen(output, 'input', () => { model.editText(output.value) })
 listen(node('copy'), 'click', copy)
 listen(node('clear-image'), 'click', clearImage)
 listen(node('retry'), 'click', () => model.retry())
 listen(node('provider'), 'click', () => {
  modal = true; node('provider').setAttribute('aria-expanded', 'true')
  if (typeof dialog.showModal === 'function') dialog.showModal(); else dialog.setAttribute('open', '')
  node('provider-option').focus()
 })
 listen(node('provider-option'), 'click', () => { model.setProvider(PROVIDERS[0].id); closeSource() })
 listen(node('source-close'), 'click', () => closeSource())
 listen(dialog, 'cancel', event => { event.preventDefault(); closeSource() })
 listen(document, 'keydown', event => {
  const primary = event.ctrlKey || event.metaKey
  if (event.key === 'F5' || event.key === 'BrowserRefresh' || (primary && event.key.toLowerCase() === 'r')) { event.preventDefault(); return }
  if (event.isComposing || event.keyCode === 229) return
  if (event.key === 'Enter' && event.repeat && event.target !== output) { event.preventDefault(); return }
  if (modal) {
   if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeSource(); return }
   if (event.key === 'Tab') { event.preventDefault(); (document.activeElement === node('source-close') ? node('provider-option') : node('source-close')).focus() }
   return
  }
  if (event.key === 'Tab' && !primary && !event.altKey) {
   event.preventDefault(); const order = focusOrder(), index = order.indexOf(document.activeElement), next = index + (event.shiftKey ? -1 : 1)
   if (next < 0 || next >= order.length) focusHost(session); else order[next].focus()
  }
 })
 cleanup.push(model.subscribe(render))
 // Install every receiver before onUpdate: Ready is authority, never a timeout.
 cleanup.push(host.onImageChanged(event => {
  if (!current(session) || event.sessionEpoch !== session.sessionEpoch) return
  copying = null; clearing = null; model.imageChanged(event)
 }))
 cleanup.push(host.onReady(event => {
  if (!current(session) || event.sessionEpoch !== session.sessionEpoch || event.requestId !== session.requestId) return
  session.ready = true; model.markReady(event)
 }))
 cleanup.push(host.onHostKey(event => {
  const owner = session
  if (!current(owner) || event.sessionEpoch !== owner.sessionEpoch || !owner.ready || modal) return
  if (event.key === 'Enter') { copy(); focusHost(owner) }
  else if (event.key === 'Tab') { const order = focusOrder(); (event.shiftKey ? order.at(-1) : order[0])?.focus() }
  // No awaited effect: Host may immediately accept another paste after this ACK.
 }))
 if (host.share?.onSource) cleanup.push(host.share.onSource(() => {
  if (disposed || modal || !session?.ready || !model.canCopy() || !validText(output.value)) return null
  return { type: 'text/plain', text: output.value }
 }))
 cleanup.push(host.onUpdate(update => {
  if (disposed) return
  const same = session?.sessionEpoch === update.sessionEpoch
  if (!same) { copying = null; clearing = null; closeSource(false); session = { sessionEpoch: update.sessionEpoch } }
  session.requestId = update.requestId; session.ready = false
  session.facades = { request: host.request, clipboard: host.clipboard, clearImage: host.clearImage, setStatus: host.setStatus, focusHostInput: host.focusHostInput }
  document.documentElement.dataset.theme = update.theme === 'light' ? 'light' : 'dark'
  model.installUpdate(update)
 }))
 if (typeof host.onActions === 'function') cleanup.push(host.onActions(() => {
  if (disposed || modal || !session?.ready) return null
  const owner = session, revision = model.getState().revision
  const guarded = run => () => { if (current(owner) && owner.ready && !modal && model.getState().revision === revision) run() }
  return { context: [
   { id: 'copy', title: '复制文字', icon: 'copy', ...(node('copy').disabled ? { disabledReason: node('copy').title } : {}), run: guarded(copy) },
   { id: 'clear-image', title: '清除图片', icon: 'close', ...(node('clear-image').disabled ? { disabledReason: '没有可清除的图片' } : {}), run: guarded(clearImage) },
  ] }
 }))
 function dispose() {
  if (disposed) return
  copying = null; clearing = null; closeSource(false); model.dispose(); disposed = true
  for (const remove of cleanup.splice(0)) if (typeof remove === 'function') remove()
 }
 listen(view, 'pagehide', dispose); render()
 return { dispose }
}
if (typeof window !== 'undefined' && window.uipilotPluginPanel) mountPanel(window.document, window.uipilotPluginPanel)
