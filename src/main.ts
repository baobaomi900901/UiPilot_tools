import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import { createFindCore } from './find-core'
import { FindView } from './find-view'
import { createLauncherCore } from './launcher-core'
import { HOTKEY_RECORDING_CURRENT_DOM_EVENT, LauncherView } from './launcher-view'
import { createPluginWindowCore } from './plugin-window-core'
import { PluginWindowView } from './plugin-window-view'
import {
  parseFileSearchResponse,
  parseMessageCenterSnapshot,
  parseMessageSummary,
  parsePluginInventorySnapshot,
  parsePluginPanelCommandResult,
  parsePluginPanelHostKeyEnqueueResult,
  parsePublicPluginInventory,
  parsePublicPluginPrepareSummary,
  parsePublicPluginWindowIdentity,
  parseFindPreviewPreferenceResult,
  parseFindReadyOutcome,
  type FindClient,
  type FindPreviewPreferenceResult,
  type FindReadyOutcome,
  parsePluginMutationOutcome,
  type FileSearchResponse,
  type ExecuteOutcome,
  type HotkeySettingsView,
  type LauncherClient,
  type PluginWindowClient,
  type SearchResponse,
  type SettingsView,
} from './protocol'

export const client: LauncherClient = {
  listenShown: (handler) => listen('launcher://shown', (event) => handler(event.payload)),
  listenMessageStateChanged: (handler) =>
    listen('message-center://state-changed', (event) => handler(event.payload)),
  listenPluginPanelError: (handler) =>
    listen('uipilot-plugin-panel-error', (event) => handler(event.payload)),
  listenPluginPanelReset: (handler) =>
    listen('uipilot-plugin-panel-reset', (event) => handler(event.payload)),
  listenPluginPanelFocusHostInput: (handler) =>
    listen('uipilot-plugin-panel-focus-host-input', (event) => handler(event.payload)),
  getMessageSummary: async () => {
    const value = await invoke<unknown>('get_message_summary')
    const summary = parseMessageSummary(value)
    if (!summary) throw { code: 'MessageOperationFailed', storeStatus: 'ready' }
    return summary
  },
  openMessageCenter: async () => {
    const value = await invoke<unknown>('open_message_center')
    const snapshot = parseMessageCenterSnapshot(value)
    if (!snapshot) throw { code: 'MessageOperationFailed', storeStatus: 'ready' }
    return snapshot
  },
  readMessageCenter: async () => {
    const value = await invoke<unknown>('read_message_center')
    const snapshot = parseMessageCenterSnapshot(value)
    if (!snapshot) throw { code: 'MessageOperationFailed', storeStatus: 'ready' }
    return snapshot
  },
  clearMessages: async () => {
    const value = await invoke<unknown>('clear_messages')
    const snapshot = parseMessageCenterSnapshot(value)
    if (!snapshot) throw { code: 'MessageOperationFailed', storeStatus: 'ready' }
    return snapshot
  },
  searchApps: (input) => invoke<SearchResponse | null>('search_apps', { ...input }),
  openFind: (input) => invoke('open_find_window', { input }),
  executeResult: (input) => invoke<ExecuteOutcome>('execute_result', input),
  openPluginPanel: async (input) => {
    const value = await invoke<unknown>('open_plugin_panel', { input })
    const result = parsePluginPanelCommandResult(value)
    if (!result) throw { code: 'windowFailed', message: 'panel open failed' }
    return result
  },
  submitPluginPanel: async (input) => {
    const value = await invoke<unknown>('submit_plugin_panel', { input })
    const result = parsePluginPanelCommandResult(value)
    if (!result) throw { code: 'windowFailed', message: 'panel submit failed' }
    return result
  },
  enqueuePluginPanelHostKey: async (input) => {
    const value = await invoke<unknown>('plugin_panel_host_key_enqueue', { input })
    const result = parsePluginPanelHostKeyEnqueueResult(value)
    if (!result) throw { code: 'windowFailed', message: 'panel Host-key enqueue failed' }
    return result
  },
  closePluginPanel: async (input) => { await invoke('close_plugin_panel', { input }) },
  acknowledgePluginPanelFocusHostInput: async (input) => {
    await invoke('plugin_panel_focus_host_input_ack', { ...input })
  },
  commitPluginWindowTransfer: (input) => invoke<void>('commit_plugin_window_transfer', input),
  listPublicPlugins: async () => {
    const value = await invoke<unknown>('list_public_plugins')
    const inventory = parsePublicPluginInventory(value)
    if (!inventory) throw { code: 'pluginListFailed', message: 'public plugin list failed' }
    return inventory
  },
  selectPublicPluginArchive: async () => {
    const selected = await open({ multiple: false, directory: false, filters: [{ name: 'UiPilot Plugin', extensions: ['uipilot-plugin'] }] })
    return typeof selected === 'string' ? selected : null
  },
  selectPublicPluginDirectory: () => invoke<string | null>('select_public_plugin_directory'),
  preparePublicPlugin: async (input) => {
    const value = await invoke<unknown>('prepare_public_plugin_install', input)
    const prepared = parsePublicPluginPrepareSummary(value)
    if (!prepared) throw { code: 'pluginInstallFailed', message: 'public plugin prepare failed' }
    return prepared
  },
  commitPublicPlugin: async (input) => { await invoke('commit_public_plugin_install', input) },
  cancelPublicPlugin: async (input) => { await invoke('cancel_public_plugin_install', input) },
  setPublicPluginEnabled: async (input) => { await invoke('set_plugin_enabled', input) },
  setPublicPluginNetworkAccess: async (input) => { await invoke('set_public_plugin_network_access', input) },
  setPublicPluginFavorite: async (input) => { await invoke('set_plugin_favorite', input) },
  setPublicPluginEffectiveName: async (input) => { await invoke('set_plugin_effective_name', input) },
  savePublicPluginSettings: async (input) => { await invoke('save_plugin_settings', input) },
  uninstallPublicPlugin: async (input) => { await invoke('uninstall_plugin', input) },  listPlugins: async () => {
    const value = await invoke<unknown>('list_plugins')
    const snapshot = parsePluginInventorySnapshot(value)
    if (!snapshot) throw { code: 'pluginListFailed', message: 'plugin list failed' }
    return snapshot
  },
  installPlugin: async (input) => {
    const value = await invoke<unknown>(
      'install_plugin',
      Object.freeze({ pluginId: input.pluginId }),
    )
    const outcome = parsePluginMutationOutcome(value)
    if (!outcome) throw { code: 'pluginInstallFailed', message: 'plugin install failed' }
    return outcome
  },
  reloadPlugin: async (input) => {
    const value = await invoke<unknown>(
      'reload_plugin',
      Object.freeze({ pluginId: input.pluginId }),
    )
    const outcome = parsePluginMutationOutcome(value)
    if (!outcome) throw { code: 'pluginReloadFailed', message: 'plugin reload failed' }
    return outcome
  },
  deletePlugin: async (input) => {
    const value = await invoke<unknown>('delete_plugin', Object.freeze({ pluginId: input.pluginId }))
    const outcome = parsePluginMutationOutcome(value)
    if (!outcome) throw { code: 'pluginDeleteFailed', message: 'plugin delete failed' }
    return outcome
  },
  loadSettings: () => invoke<SettingsView>('load_settings'),
  saveSettings: (input) => invoke<void>('save_settings', input),
  saveHotkey: (input) => invoke<HotkeySettingsView>('save_hotkey', input),
  setThemePreference: (input) =>
    invoke<void>(
      'set_theme_preference',
      Object.freeze({ preference: Object.freeze({ theme: input.preference.theme }) }),
    ),
  setWebSearchEngine: (input) =>
    invoke<void>(
      'set_web_search_engine',
      Object.freeze({ preference: Object.freeze({ engine: input.preference.engine }) }),
    ),
  hideLauncher: () => invoke<void>('hide_launcher'),
}

export const pluginWindowClient: PluginWindowClient = {
  getIdentity: async () => {
    const value = await invoke<unknown>('get_public_plugin_window_identity')
    const identity = parsePublicPluginWindowIdentity(value)
    if (!identity) throw new Error('invalid public plugin window identity')
    return identity
  },
  setPinned: (input) => invoke('set_plugin_window_pinned', input),
  close: () => invoke<void>('close_plugin_window'),
}
export const findClient: FindClient = {
  listenForward: (handler) => listen('find://forwarded', (event) => handler(event.payload)),
  listenThemeChanged: (handler) => listen('find://theme-changed', (event) => handler(event.payload)),
  prepareInitialization: () => invoke<FindReadyOutcome>('prepare_find_initialization'),
  commitReady: (input) => invoke<FindReadyOutcome>('commit_find_ready', { input }),
  getReadyStatus: (input) => invoke<FindReadyOutcome>('get_find_ready_status', { input }),
  searchFiles: async (input) => {
    const payload = Object.freeze({
      query: input.query,
      category: input.category,
      sort: input.sort,
      invocationId: input.invocationId,
      querySequence: input.querySequence,
    })
    const response = await invoke<FileSearchResponse | null>('search_files', payload)
    return response === null ? null : parseFileSearchResponse(response)
  },
  executeResult: (input) => invoke<ExecuteOutcome>('execute_result', input),
  setPinned: (input) => invoke('set_find_pinned', { input }),
  setPreviewPreference: async (input) => {
    const value = await invoke<FindPreviewPreferenceResult>('set_find_preview_preference', {
      preference: Object.freeze({ enabled: input.preference.enabled }),
    })
    return parseFindPreviewPreferenceResult(value) ?? value
  },
  hide: (input) => invoke<void>('hide_find_window', { input }),
}

export function currentWindowLabel(): string {
  return getCurrentWindow().label
}

const host = document.querySelector<HTMLElement>('#app')
if (!host) throw new Error('Missing application root')

const label = currentWindowLabel()
if (label === 'main') {
const core = createLauncherCore(client)
const panelFocusListenerReady = core.preparePanelHostInputFocusListener()
let settleReady!: (result: 'ready' | 'failed') => void
const viewReady = new Promise<'ready' | 'failed'>((resolve) => {
  settleReady = resolve
})
let readySettled = false
const onReady = (result: 'ready' | 'failed') => {
  if (readySettled) return
  readySettled = true
  settleReady(result)
}
let mountFailed = false
let unlistenHotkeyRecordingCurrent: (() => void) | undefined
const failMount = () => {
  if (mountFailed) return
  mountFailed = true
  unlistenHotkeyRecordingCurrent?.()
  unlistenHotkeyRecordingCurrent = undefined
  onReady('failed')
  core.failInitialization()
  core.destroy()
  const status = document.createElement('div')
  status.className = 'status-region'
  status.setAttribute('role', 'status')
  status.setAttribute('aria-live', 'polite')
  status.setAttribute('aria-atomic', 'true')
  status.textContent = core.getSnapshot().status
  host.replaceChildren(status)
}
const root = createRoot(host, { onUncaughtError: failMount })

let tornDown = false
const teardown = () => {
  if (tornDown) return
  tornDown = true
  window.removeEventListener('pagehide', teardown)
  unlistenHotkeyRecordingCurrent?.()
  unlistenHotkeyRecordingCurrent = undefined
  core.destroy()
  root.unmount()
}
window.addEventListener('pagehide', teardown)

try {
  root.render(createElement(LauncherView, { core, onReady }))
} catch {
  failMount()
}

void (async () => {
  const result = await viewReady
  if (tornDown) return
  if (result === 'failed') {
    core.failInitialization()
    core.destroy()
    return
  }
  if (!await panelFocusListenerReady) {
    core.failInitialization()
    core.destroy()
    return
  }
  try {
    const unlisten = await listen('hotkey-recording://current', () => {
      window.dispatchEvent(new Event(HOTKEY_RECORDING_CURRENT_DOM_EVENT))
    })
    if (tornDown) {
      unlisten()
      return
    }
    unlistenHotkeyRecordingCurrent = unlisten
  } catch {
    core.failInitialization()
    core.destroy()
    return
  }
  await core.start()
})()
} else if (label.startsWith('plugin-shell-')) {
  const core = createPluginWindowCore(pluginWindowClient)
  const root = createRoot(host, { onUncaughtError: () => core.destroy() })
  const teardown = () => {
    window.removeEventListener('pagehide', teardown)
    core.destroy()
    root.unmount()
  }
  window.addEventListener('pagehide', teardown)
  root.render(createElement(PluginWindowView, { core }))
  void core.start()
} else if (label === 'find') {
  const core = createFindCore(findClient)
  const root = createRoot(host, {
    onUncaughtError: () => {
      core.destroy()
      host.textContent = '文件搜索暂不可用。'
    },
  })
  let tornDown = false
  const teardown = () => {
    if (tornDown) return
    tornDown = true
    window.removeEventListener('pagehide', teardown)
    core.destroy()
    root.unmount()
  }
  window.addEventListener('pagehide', teardown)
  root.render(createElement(FindView, { core }))
} else {
  throw new Error(`Unknown Tauri window label: ${label}`)
}
