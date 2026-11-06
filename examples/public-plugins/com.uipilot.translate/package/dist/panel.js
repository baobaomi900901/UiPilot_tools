import { createPanelCapabilities } from './panel-capabilities.js'
import { createPanelModel } from './panel-model.js'
import { filterLanguages, LANGUAGES, PROVIDERS, PREFERENCES_STORAGE_KEY, sanitizePreferences } from './languages.js'

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
  volume: [['path', { d: 'M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z' }], ['path', { d: 'M16 9a5 5 0 0 1 0 6' }], ['path', { d: 'M19.364 18.364a9 9 0 0 0 0-12.728' }]],
  swap: [['path', { d: 'm16 3 4 4-4 4' }], ['path', { d: 'M20 7H4' }], ['path', { d: 'm8 21-4-4 4-4' }], ['path', { d: 'M4 17h16' }]],
  chevron: [['path', { d: 'm6 9 6 6 6-6' }]],
  close: [['path', { d: 'M18 6 6 18' }], ['path', { d: 'm6 6 12 12' }]],
}

export function mountPanel(document, host, { createCapabilities = createPanelCapabilities } = {}) {
  const view = document.defaultView
  const node = id => document.getElementById(id)
  const source = node('source-text'), output = node('translation'), dialog = node('choice-dialog')
  const search = node('choice-search'), options = node('choice-options')
  const cleanup = []
  let disposed = false, session = null, modal = null, activeIndex = 0, choices = []
  let pendingSave = null, saving = false
  const label = id => id === 'auto' ? '自动检测' : LANGUAGES.find(item => item.id === id)?.label ?? '未识别'
  const listen = (target, name, fn, settings) => {
    target.addEventListener(name, fn, settings)
    cleanup.push(() => target.removeEventListener(name, fn, settings))
  }
  const current = owner => !disposed && session === owner
  function notify(owner, message, tone = 'error') {
    if (!current(owner)) return
    try { Promise.resolve(owner.facades.setStatus?.({ message, tone })).catch(() => {}) } catch { /* Session can expire between guard and call. */ }
  }
  const model = createPanelModel({ request: input => {
    const owner = session
    if (!current(owner) || !owner.ready || typeof owner.facades.request !== 'function') return Promise.reject(new Error('Unavailable'))
    return owner.facades.request(input)
  } })

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
    if (source.value !== state.text) source.value = state.text
    output.textContent = state.translation
    node('source-language').firstElementChild.textContent = label(state.sourceLanguage)
    node('target-language').firstElementChild.textContent = label(state.targetLanguage)
    node('provider').firstElementChild.textContent = PROVIDERS.find(item => item.id === state.providerId).label
    const detected = node('detected-language')
    detected.hidden = state.sourceLanguage !== 'auto'
    detected.textContent = state.detectedSourceLanguage ? `识别为 ${label(state.detectedSourceLanguage)}` : state.status === 'success' ? '未识别' : '等待识别'
    node('swap').disabled = !model.canSwap()
    node('swap').title = model.canSwap() ? '交换语言与文本' : '需要有效译文和明确的源语言'
    const status = node('translation-status')
    status.textContent = state.status === 'loading' ? '正在翻译…' : state.status === 'error' ? state.error.message : '译文将显示在这里'
    status.hidden = state.status === 'success'
    status.dataset.error = String(state.status === 'error')
    output.parentElement.setAttribute('aria-busy', String(state.status === 'loading'))
    for (const side of ['source', 'translation']) {
      for (const kind of ['copy', 'speak']) {
        const button = node(`${side}-${kind}`)
        const playing = kind === 'speak' && session?.capabilities.isPlaying?.(side)
        const title = `${playing ? '停止' : ''}${kind === 'copy' ? '复制' : '朗读'}${side === 'source' ? '原文' : '译文'}`
        const reason = !session?.ready ? '面板尚未就绪' : side === 'translation' && state.status !== 'success' ? '需要本次有效译文' : !(side === 'source' ? state.text : state.translation) ? '没有可用文本' : session.capabilities.availability(kind, side, state)
        button.disabled = Boolean(reason)
        if (kind === 'speak') button.setAttribute('aria-pressed', String(Boolean(playing)))
        button.title = reason ? `${title}：${reason}` : title
        button.setAttribute('aria-label', button.title)
      }
    }
  }

  // Single writer, latest unsent preferences only. Each job retains its captured facade.
  function pumpSave() {
    if (disposed || saving || !pendingSave || !pendingSave.owner.ready) return
    const job = pendingSave
    pendingSave = null
    if (!current(job.owner)) return
    saving = true
    Promise.resolve().then(() => {
      if (!current(job.owner)) return
      return job.owner.facades.storage.set(PREFERENCES_STORAGE_KEY, job.preferences)
    }).catch(() => notify(job.owner, '偏好保存失败；请重新选择后再保存')).finally(() => {
      saving = false
      pumpSave()
    })
  }
  function preferenceChanged(change) {
    const before = JSON.stringify(model.getPreferences())
    session?.capabilities.invalidate()
    change()
    if (!session || before === JSON.stringify(model.getPreferences())) return
    pendingSave = { owner: session, preferences: model.getPreferences() }
    pumpSave()
  }
  function closeChoices(restore = true) {
    if (!modal) return
    const opener = modal.opener
    opener.setAttribute('aria-expanded', 'false')
    modal = null
    if (typeof dialog.close === 'function') dialog.close()
    else dialog.removeAttribute('open')
    if (restore && opener.isConnected) (opener.hidden ? node('source-language') : opener).focus()
  }
  function renderChoices() {
    if (!modal) return
    const state = model.getState()
    choices = modal.kind === 'provider' ? PROVIDERS.filter(item => `${item.label} ${item.id}`.toLowerCase().includes(search.value.trim().toLowerCase())) : filterLanguages(search.value, modal.kind === 'source')
    activeIndex = Math.min(Math.max(activeIndex, 0), choices.length - 1)
    options.replaceChildren()
    const selected = modal.kind === 'provider' ? state.providerId : modal.kind === 'source' ? state.sourceLanguage : state.targetLanguage
    choices.forEach((choice, index) => {
      const button = document.createElement('button')
      button.type = 'button'
      button.className = 'choice-option'
      button.id = `choice-${index}`
      button.tabIndex = -1
      button.setAttribute('role', 'option')
      button.setAttribute('aria-selected', String(choice.id === selected))
      button.dataset.active = String(index === activeIndex)
      button.textContent = choice.label + (modal.kind === 'source' && choice.autoDetect === false ? ' · 需手动选择' : '')
      if (choice.id === selected) {
        const marker = document.createElement('span')
        marker.setAttribute('aria-hidden', 'true')
        marker.textContent = '✓'
        button.append(marker)
      }
      button.addEventListener('click', () => selectChoice(index))
      button.addEventListener('pointermove', () => {
        if (activeIndex === index) return
        activeIndex = index
        for (const option of options.children) option.dataset.active = String(option === button)
        search.setAttribute('aria-activedescendant', button.id)
      })
      options.append(button)
    })
    if (choices.length) search.setAttribute('aria-activedescendant', `choice-${activeIndex}`)
    else search.removeAttribute('aria-activedescendant')
    node('choice-empty').hidden = choices.length > 0
    options.children[activeIndex]?.scrollIntoView?.({ block: 'nearest' })
  }
  function openChoices(kind, opener) {
    closeChoices(false)
    modal = { kind, opener }
    node('choice-title').textContent = kind === 'provider' ? '翻译源' : kind === 'source' ? '源语言' : '目标语言'
    search.placeholder = kind === 'provider' ? '搜索翻译源' : '搜索语言名称或代码'
    search.setAttribute('aria-label', search.placeholder)
    options.setAttribute('aria-label', node('choice-title').textContent)
    search.value = ''
    activeIndex = 0
    opener.setAttribute('aria-expanded', 'true')
    renderChoices()
    if (typeof dialog.showModal === 'function') dialog.showModal()
    else dialog.setAttribute('open', '')
    search.focus()
  }
  function selectChoice(index) {
    const choice = choices[index]
    if (!choice || !modal) return
    const kind = modal.kind
    preferenceChanged(() => {
      const state = model.getState()
      if (kind === 'provider') model.setProvider(choice.id)
      else model.setLanguages(kind === 'source' ? choice.id : state.sourceLanguage, kind === 'target' ? choice.id : state.targetLanguage)
    })
    closeChoices()
  }

  listen(source, 'input', () => {
    // Install the live value before effects can render; reassigning the old value
    // would discard this edit and reset the browser's selection/IME state.
    model.editText(source.value)
    session?.capabilities.invalidate()
  })
  listen(source, 'compositionstart', () => model.setComposition(true))
  listen(source, 'compositionend', () => model.setComposition(false))
  for (const [id, kind] of [['source-language', 'source'], ['detected-language', 'source'], ['target-language', 'target'], ['provider', 'provider']]) listen(node(id), 'click', () => openChoices(kind, node(id)))
  listen(node('swap'), 'click', () => preferenceChanged(() => model.swap()))
  listen(node('choice-close'), 'click', () => closeChoices())
  listen(dialog, 'cancel', event => { event.preventDefault(); closeChoices() })
  listen(search, 'input', () => { activeIndex = 0; renderChoices() })
  for (const side of ['source', 'translation']) for (const kind of ['copy', 'speak']) listen(node(`${side}-${kind}`), 'click', () => {
    if (!node(`${side}-${kind}`).disabled) session?.capabilities.run(kind, side, model.getState())
  })
  function handleKey(event) {
    const primary = event.ctrlKey || event.metaKey
    if (event.key === 'F5' || event.key === 'BrowserRefresh' || (primary && event.key.toLowerCase() === 'r')) {
      event.preventDefault()
      return
    }
    if (modal && primary && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      search.focus()
      return
    }
    if (event.isComposing || event.keyCode === 229) return
    if (modal) {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation?.(); closeChoices(); return }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        activeIndex = choices.length ? (activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length : 0
        renderChoices()
        return
      }
      if (event.key === 'Enter' && event.target !== node('choice-close')) { event.preventDefault(); selectChoice(activeIndex); return }
      if (event.key === 'Tab') {
        event.preventDefault()
        ;(document.activeElement === search ? node('choice-close') : search).focus()
      }
      return
    }
    if (event.key === 'Enter' && event.ctrlKey && event.target === source) {
      event.preventDefault()
      session?.capabilities.invalidate()
      model.submit()
    }
  }
  listen(document, 'keydown', handleKey)
  cleanup.push(model.subscribe(render))
  // onReady must be installed before the first onUpdate; timers never establish readiness.
  if (typeof host.onReady === 'function') cleanup.push(host.onReady(event => {
    if (!session || event.sessionEpoch !== session.sessionEpoch || event.requestId !== session.requestId) return
    session.ready = true
    session.capabilities.start?.()
    model.markReady(event)
    pumpSave()
    render()
  }))
  if (host.share?.onSource) cleanup.push(host.share.onSource(() => {
    const state = model.getState()
    return !disposed && !modal && session?.ready && state.status === 'success' && state.translation.trim()
      ? { type: 'text/plain', text: state.translation } : null
  }))
  if (host.share?.onReceive) cleanup.push(host.share.onReceive(event => {
    if (disposed || modal || !session?.ready || session.received || event?.version !== 1 || event.receiverId !== 'translate' || event.sessionEpoch !== session.sessionEpoch || event.payload?.type !== 'text/plain' || typeof event.payload.text !== 'string' || !event.payload.text.trim() || new TextEncoder().encode(event.payload.text).length > 8192) return { accepted: false }
    session.received = true
    session.capabilities.invalidate()
    // Store synchronously. Business work is scheduled by the existing model timer.
    model.editText(event.payload.text)
    model.submit()
    return { accepted: true }
  }))
  cleanup.push(host.onUpdate(update => {
    if (disposed) return
    const previous = session
    const sameSession = previous?.sessionEpoch === update.sessionEpoch
    document.documentElement.dataset.theme = update.theme === 'light' ? 'light' : 'dark'
    if (sameSession && previous.received && previous.requestId === update.requestId && update.input === '' && !update.data?.text) return
    const preferences = sameSession ? model.getPreferences() : sanitizePreferences(update.data?.preferences)
    const queued = sameSession ? pendingSave?.preferences : null
    previous?.capabilities.dispose()
    closeChoices(false)
    const owner = {
      sessionEpoch: update.sessionEpoch, requestId: update.requestId, ready: false,
      facades: { request: host.request, storage: host.storage, setStatus: host.setStatus, clipboard: host.clipboard, speech: host.speech },
    }
    session = owner
    owner.capabilities = createCapabilities({
      facades: owner.facades,
      isCurrent: revision => current(owner) && (revision === undefined || revision === model.getState().revision),
      notify: (message, tone) => notify(owner, message, tone),
      render: () => render(),
      getState: model.getState,
    })
    pendingSave = queued ? { owner, preferences: queued } : null
    document.documentElement.dataset.theme = update.theme === 'light' ? 'light' : 'dark'
    model.installUpdate({ sessionEpoch: update.sessionEpoch, requestId: update.requestId, text: typeof update.data?.text === 'string' ? update.data.text : update.input, preferences })
    if (typeof host.onReady !== 'function') notify(owner, '翻译面板需要 UiPilot 0.19.0 或更高版本')
  }))
  if (typeof host.onActions === 'function') cleanup.push(host.onActions(() => {
    if (disposed || modal || !session?.ready) return null
    const owner = session, state = model.getState(), revision = state.revision
    const guarded = fn => () => { if (current(owner) && owner.ready && !modal && model.getState().revision === revision) fn() }
    const context = [{ id: 'translate', title: '翻译原文', icon: 'arrow', shortcut: 'Ctrl+Enter', ...(state.text.trim() ? {} : { disabledReason: '请输入原文' }), run: guarded(() => { owner.capabilities.invalidate(); model.submit() }) }]
    for (const side of ['source', 'translation']) for (const kind of ['copy', 'speak']) {
      const button = node(`${side}-${kind}`)
      context.push({ id: `${side}-${kind}`, title: button.getAttribute('aria-label'), ...(kind === 'copy' ? { icon: 'copy' } : {}), ...(button.disabled ? { disabledReason: button.title } : {}), run: guarded(() => { if (!button.disabled) owner.capabilities.run(kind, side, model.getState()) }) })
    }
    return { context }
  }))
  function dispose() {
    if (disposed) return
    disposed = true
    pendingSave = null
    session?.capabilities.dispose()
    model.dispose()
    for (const remove of cleanup.splice(0)) if (typeof remove === 'function') remove()
  }
  listen(view, 'pagehide', dispose)
  render()
  return { dispose }
}

if (typeof window !== 'undefined' && window.uipilotPluginPanel) mountPanel(window.document, window.uipilotPluginPanel)
