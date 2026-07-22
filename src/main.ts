import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import { createLauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import {
  parseFileSearchResponse,
  type FileSearchResponse,
  type ExecuteOutcome,
  type HotkeySettingsView,
  type LauncherClient,
  type SearchResponse,
  type SettingsView,
} from './protocol'

export const client: LauncherClient = {
  listenShown: (handler) => listen('launcher://shown', (event) => handler(event.payload)),
  listenFileIndexChanged: (handler) => listen<unknown>('file-index://changed', (event) => handler(event.payload)),
  searchApps: (input) => invoke<SearchResponse | null>('search_apps', input),
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
  loadSettings: () => invoke<SettingsView>('load_settings'),
  saveSettings: (input) => invoke<void>('save_settings', input),
  saveHotkey: (input) => invoke<HotkeySettingsView>('save_hotkey', input),
  setFilePreviewPreference: (input) =>
    invoke<void>(
      'set_file_preview_preference',
      Object.freeze({ preference: Object.freeze({ enabled: input.preference.enabled }) }),
    ),
  hideLauncher: () => invoke<void>('hide_launcher'),
}

const host = document.querySelector<HTMLElement>('#app')
if (!host) throw new Error('Missing application root')

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
