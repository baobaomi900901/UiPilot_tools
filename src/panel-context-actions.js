// Host-only adapter, injected alongside the shared menu into the Panel document.
export function installPanelContextActions({ document, window, getEpoch, invoke, createMenu, getShare }) {
  let provider = null
  let revision = 0
  const reportError = () => { void window.uipilotPluginPanel.setStatus({ message: '操作未完成，请重试', tone: 'error' }).catch(() => {}) }
  const menu = createMenu(document, reportError)
  function open({ sessionEpoch, source = 'content', ticket = '' }) {
    if (sessionEpoch !== getEpoch() || !['hostInput', 'content'].includes(source)) return
    if (menu.isOpen) { menu.close(); return }
    if (document.querySelector('dialog[open]')) return
    const captured = ++revision
    const origin = document.activeElement
    try {
      const supplied = provider ? provider(Object.freeze({ source })) : {}
      if (supplied === null) return
      if (!supplied || typeof supplied !== 'object' || typeof supplied.then === 'function') throw new Error()
      if (Object.keys(supplied).some(key => !['context', 'global'].includes(key))) throw new Error()
      for (const items of [supplied.context, supplied.global]) {
        if (items !== undefined && (!Array.isArray(items) || items.length > 12)) throw new Error()
        if (items?.some(action => typeof action?.run !== 'function' || action.id?.startsWith('host-'))) throw new Error()
      }
      const wrap = action => ({ ...action, run() {
        if (sessionEpoch !== getEpoch() || captured !== revision) return
        return action.run()
      } })
      const context = [...(supplied.context ?? [])].map(wrap)
      const global = [...(supplied.global ?? [])].map(wrap)
      const share = getShare?.()
      if (share) context.push(wrap(share))
      if (source === 'hostInput' && ticket) context.unshift({ id: 'host-submit', title: '搜索', shortcut: 'Enter', icon: 'search', run: async () => {
        if (sessionEpoch !== getEpoch() || captured !== revision) return
        await window.uipilotPluginPanel.focusHostInput()
        if (sessionEpoch === getEpoch() && captured === revision) await invoke('plugin_panel_actions_submit', { sessionEpoch, ticket })
      } })
      if (source === 'content' && !global.some(action => action.shortcut === 'Ctrl+F')) global.push({ id: 'host-input', title: '返回主输入框', shortcut: 'Ctrl+F', icon: 'search', run: () => window.uipilotPluginPanel.focusHostInput() })
      menu.open({ context, global }, {
        origin,
        restoreFocus: source === 'hostInput' ? () => { void window.uipilotPluginPanel.focusHostInput().catch(reportError) } : undefined,
      })
    } catch { reportError() }
  }
  Object.defineProperty(window, '__UIPILOT_PLUGIN_PANEL_ACTIONS__', { value: open, configurable: false })
  return {
    register(next) {
      if (provider || typeof next !== 'function') throw new TypeError('one action provider required')
      provider = next
      return () => { if (provider === next) { provider = null; revision++; menu.close(false) } }
    },
    reset() { revision++; menu.close(false) },
    handleKey(event) {
      if (menu.isOpen) { menu.handleKeydown(event); return true }
      if (event.key.toLowerCase() === 'k' && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && !event.isComposing && !event.repeat) {
        event.preventDefault(); event.stopImmediatePropagation()
        open({ sessionEpoch: getEpoch(), source: 'content' }); return true
      }
      return false
    },
  }
}
