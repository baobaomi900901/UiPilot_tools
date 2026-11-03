// Shared Host UI: imported by the launcher and injected into isolated Panel WebViews.
export function createContextActionsMenu(document, onError = () => {}) {
  let current = null
  const paths = {
    'arrow-right-to-line': 'M4 12h12 M10 6l6 6-6 6 M20 4v16',
    'arrow-left-to-line': 'M20 12H8 M14 6l-6 6 6 6 M4 4v16',
    pin: 'M16 9V3H8v6l-3 3v3h14v-3Z M12 15v7',
    eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
    search: 'm21 21-5-5 M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',
    edit: 'm16 3 5 5-13 13H3v-5Z M14 5l5 5',
    copy: 'M9 9h12v12H9Z M5 15H3V3h12v2',
    save: 'M4 3h13l4 4v14H3V3Z M7 3v6h10V3 M7 21v-8h10v8',
    plus: 'M12 4v16 M4 12h16', trash: 'M3 6h18 M9 6V3h6v3 M5 6l1 15h12l1-15 M10 10v7 M14 10v7',
    arrow: 'M4 12h16 m-7-7 7 7-7 7', settings: 'M12 3v3 M12 18v3 M3 12h3 M18 12h3 M6 6l2 2 M16 16l2 2 M6 18l2-2 M16 8l2-2 M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0',
    'close': 'm5 5 14 14 M5 19 19 5',
  }
  const text = (value, limit, optional = false) => optional && value === undefined ||
    typeof value === 'string' && value.trim().length > 0 && [...value].length <= limit && !/[\u0000-\u001f\u007f]/.test(value)
  function normalize(groups) {
    if (!groups || typeof groups !== 'object' || Object.keys(groups).some(key => !['context', 'global'].includes(key))) throw new TypeError('Invalid actions')
    const ids = new Set()
    return ['context', 'global'].map(key => {
      const items = groups[key] ?? []
      if (!Array.isArray(items) || items.length > 16) throw new TypeError('Invalid actions')
      return items.map(item => {
        if (!item || typeof item.id !== 'string' || !/^[a-z][a-z0-9-]{0,63}$/.test(item.id) || ids.has(item.id) ||
            !text(item.title, 80) || !text(item.shortcut, 32, true) || !text(item.disabledReason, 160, true) ||
            item.icon !== undefined && !Object.hasOwn(paths, item.icon) || typeof item.run !== 'function') throw new TypeError('Invalid action')
        ids.add(item.id)
        return { ...item }
      })
    })
  }
  function mountScrollbar(root, viewport, content) {
    const win = document.defaultView
    const track = document.createElement('div')
    track.className = 'uipilot-context-actions-track'
    track.setAttribute('aria-hidden', 'true')
    const thumb = document.createElement('div')
    thumb.className = 'uipilot-context-actions-thumb'
    track.append(thumb); root.append(track)
    let drag = null
    const geometry = () => {
      const range = Math.max(0, viewport.scrollHeight - viewport.clientHeight)
      const height = track.clientHeight
      const size = Math.min(height, Math.max(24, height * viewport.clientHeight / Math.max(1, viewport.scrollHeight)))
      return { range, size, travel: Math.max(0, height - size) }
    }
    const update = () => {
      const overflow = viewport.scrollHeight > viewport.clientHeight + 1
      track.hidden = !overflow
      root.classList.toggle('has-overflow', overflow)
      const { range, size, travel } = geometry()
      thumb.style.height = `${size}px`
      thumb.style.transform = `translateY(${range ? Math.max(0, Math.min(range, viewport.scrollTop)) / range * travel : 0}px)`
    }
    const scrollTo = value => {
      viewport.scrollTop = Math.max(0, Math.min(geometry().range, value))
      update()
    }
    const start = event => {
      if (event.button !== 0 || track.hidden) return
      event.preventDefault()
      const { range, size, travel } = geometry()
      if (event.target !== thumb) scrollTo(travel ? (event.clientY - track.getBoundingClientRect().top - size / 2) / travel * range : 0)
      drag = { y: event.clientY, top: viewport.scrollTop, pointerId: event.pointerId }
      track.setPointerCapture?.(event.pointerId)
    }
    const move = event => {
      if (!drag || event.pointerId !== drag.pointerId) return
      event.preventDefault()
      const { range, travel } = geometry()
      scrollTo(drag.top + (travel ? (event.clientY - drag.y) / travel * range : 0))
    }
    const end = event => {
      if (!drag || event.pointerId !== drag.pointerId) return
      if (track.hasPointerCapture?.(drag.pointerId)) track.releasePointerCapture(drag.pointerId)
      drag = null
    }
    const wheel = event => {
      event.preventDefault()
      scrollTo(viewport.scrollTop + event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewport.clientHeight : 1))
    }
    viewport.addEventListener('scroll', update, { passive: true })
    track.addEventListener('pointerdown', start)
    track.addEventListener('wheel', wheel, { passive: false })
    document.addEventListener('pointermove', move)
    document.addEventListener('pointerup', end)
    document.addEventListener('pointercancel', end)
    win?.addEventListener('resize', update)
    const observer = win?.ResizeObserver ? new win.ResizeObserver(update) : null
    observer?.observe(viewport); observer?.observe(content)
    update()
    return () => {
      if (drag && track.hasPointerCapture?.(drag.pointerId)) track.releasePointerCapture(drag.pointerId)
      drag = null
      observer?.disconnect()
      win?.removeEventListener('resize', update)
      document.removeEventListener('pointermove', move)
      document.removeEventListener('pointerup', end)
      document.removeEventListener('pointercancel', end)
      viewport.removeEventListener('scroll', update)
      track.removeEventListener('pointerdown', start)
      track.removeEventListener('wheel', wheel)
    }
  }
  function close(restore = true, executing = false) {
    const own = current
    if (!own) return
    current = null
    own.disposeScrollbar?.()
    own.root.remove()
    document.removeEventListener('keydown', own.keydown, true)
    document.removeEventListener('pointerdown', own.outside, true)
    if (restore) {
      if (own.restoreFocus && !executing) own.restoreFocus()
      else if (own.origin?.isConnected) {
        own.origin.focus?.({ preventScroll: true })
        if (own.selection) own.origin.setSelectionRange?.(...own.selection)
      }
    }
    own.onClose?.()
  }
  function open(groups, options = {}) {
    const normalized = normalize(groups)
    close(false)
    if (!document.getElementById('uipilot-context-actions-style')) {
      const style = document.createElement('style')
      style.id = 'uipilot-context-actions-style'
      style.textContent = `
        .uipilot-context-actions { position:fixed; right:8px; z-index:2147483647; width:min(360px,calc(100vw - 16px)); max-height:min(480px,calc(100vh - var(--uipilot-actions-bottom,8px) - 8px)); display:flex; flex-direction:column; overflow:hidden; box-sizing:border-box; padding:6px; border:1px solid var(--uipilot-color-border,var(--uipilot-ui-border,#555)); border-radius:8px; background:var(--uipilot-color-surface,var(--uipilot-ui-surface,#222)); color:var(--uipilot-color-text,var(--uipilot-ui-foreground,#eee)); font:14px/1.4 system-ui,sans-serif; box-shadow:0 8px 24px #0003; }
        .uipilot-context-actions-viewport { min-height:0; overflow:auto; overscroll-behavior:contain; scrollbar-width:none; }
        .uipilot-context-actions-viewport::-webkit-scrollbar { display:none; width:0; height:0; }
        .uipilot-context-actions.has-overflow .uipilot-context-actions-content { padding-right:12px; }
        .uipilot-context-actions-track { position:absolute; top:10px; bottom:10px; right:6px; width:8px; touch-action:none; user-select:none; }
        .uipilot-context-actions-track[hidden] { display:none; }
        .uipilot-context-actions-thumb { position:absolute; top:0; left:1px; width:6px; min-height:0; border-radius:3px; background:var(--uipilot-ui-scrollbar,var(--uipilot-color-text-muted,#888)); opacity:.65; }
        .uipilot-context-actions-track:hover .uipilot-context-actions-thumb { opacity:1; }
        .uipilot-context-actions h3 { margin:4px 8px; font:12px/24px system-ui,sans-serif; color:var(--uipilot-color-text-muted,var(--uipilot-ui-muted-foreground,#aaa)); }
        .uipilot-context-action { all:unset; box-sizing:border-box; display:flex; width:100%; align-items:center; gap:10px; padding:8px; border-radius:6px; cursor:pointer; }
        .uipilot-context-action:focus-visible,.uipilot-context-action:hover { outline:none; background:var(--uipilot-color-selection,var(--uipilot-ui-selection,#5554)); }
        .uipilot-context-action[aria-disabled=true] { cursor:default; opacity:.5; }
        .uipilot-context-action svg { width:16px; height:16px; flex:none; }
        .uipilot-context-action-label { flex:1; min-width:0; overflow-wrap:anywhere; }
        .uipilot-context-action small { display:block; font-size:12px; opacity:.8; }
        .uipilot-context-action kbd { flex:none; font:12px system-ui,sans-serif; color:var(--uipilot-color-text-muted,var(--uipilot-ui-muted-foreground,#aaa)); }
      `
      document.head.append(style)
    }
    const root = document.createElement('div')
    root.className = 'uipilot-context-actions'
    root.setAttribute('role', 'dialog'); root.setAttribute('aria-label', '操作'); root.setAttribute('aria-modal', 'true')
    root.style.bottom = `${Math.max(8, Math.min(200, options.bottom ?? 8))}px`
    root.style.setProperty('--uipilot-actions-bottom', root.style.bottom)
    root.tabIndex = -1
    const viewport = document.createElement('div')
    viewport.className = 'uipilot-context-actions-viewport'
    const content = document.createElement('div')
    content.className = 'uipilot-context-actions-content'
    viewport.append(content); root.append(viewport)
    const origin = options.origin ?? document.activeElement
    const selection = typeof origin?.selectionStart === 'number' ? [origin.selectionStart, origin.selectionEnd, origin.selectionDirection] : null
    const own = { root, origin, selection, ...options }
    current = own
    const enabled = []
    normalized.forEach((items, index) => {
      if (!items.length) return
      const heading = document.createElement('h3'); heading.textContent = index ? '通用操作' : '当前对象'; content.append(heading)
      items.forEach(action => {
        const button = document.createElement('button'); button.type = 'button'; button.className = 'uipilot-context-action'
        button.setAttribute('aria-disabled', String(Boolean(action.disabledReason)))
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
        for (const [key, value] of Object.entries({ viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.6', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' })) svg.setAttribute(key, value)
        const path = document.createElementNS('http://www.w3.org/2000/svg', 'path'); path.setAttribute('d', paths[action.icon ?? 'arrow']); svg.append(path); button.append(svg)
        const label = document.createElement('span'); label.className = 'uipilot-context-action-label'; label.textContent = action.title
        if (action.disabledReason) { const reason = document.createElement('small'); reason.textContent = action.disabledReason; label.append(reason) }
        button.append(label)
        if (action.shortcut) { const shortcut = document.createElement('kbd'); shortcut.textContent = action.shortcut; button.append(shortcut) }
        button.addEventListener('click', () => {
          if (current !== own || action.disabledReason) return
          close(true, true)
          try { Promise.resolve(action.run()).catch(onError) } catch { onError() }
        })
        content.append(button)
        if (!action.disabledReason) enabled.push(button)
      })
    })
    own.keydown = event => {
      if (current !== own || event.isComposing) return
      event.stopImmediatePropagation()
      const key = event.key
      if (key === 'Escape' || key.toLowerCase() === 'k' && event.ctrlKey && !event.altKey && !event.metaKey) { event.preventDefault(); close(); return }
      if (['ArrowDown', 'ArrowUp', 'Home', 'End', 'Tab', 'Enter', ' '].includes(key)) {
        event.preventDefault()
        if (!enabled.length) return
        const index = enabled.indexOf(document.activeElement)
        if (key === 'Enter' || key === ' ') { if (index >= 0 && !event.repeat) enabled[index].click(); return }
        const next = key === 'Home' ? 0 : key === 'End' ? enabled.length - 1 : (index + (key === 'ArrowUp' || key === 'Tab' && event.shiftKey ? -1 : 1) + enabled.length) % enabled.length
        enabled[next].focus()
      } else if (event.ctrlKey || event.metaKey || event.altKey) event.preventDefault()
    }
    own.outside = event => { if (!root.contains(event.target)) close() }
    document.addEventListener('keydown', own.keydown, true)
    document.addEventListener('pointerdown', own.outside, true)
    document.body.append(root)
    own.disposeScrollbar = mountScrollbar(root, viewport, content)
    ;(enabled[0] ?? root).focus()
  }
  return { open, close, handleKeydown(event) { current?.keydown(event) }, get isOpen() { return current !== null } }
}
