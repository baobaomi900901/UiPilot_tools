import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import { createFindCore } from './find-core'
import { FindView } from './find-view'
import { createLauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import {
  parseFileSearchResponse,
  parsePluginInventorySnapshot,
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
  type SearchResponse,
  type SettingsView,
} from './protocol'

export const client: LauncherClient = {
  listenShown: (handler) => listen('launcher://shown', (event) => handler(event.payload)),
  searchApps: (input) => invoke<SearchResponse | null>('search_apps', input),
  openFind: (input) => invoke('open_find_window', { input }),
  executeResult: (input) => invoke<ExecuteOutcome>('execute_result', input),
  listPlugins: async () => {
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
  hideLauncher: () => invoke<void>('hide_launcher'),
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
const failMount = () => {
  if (mountFailed) return
  mountFailed = true
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
    return
  }
  await core.start()
})()
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
