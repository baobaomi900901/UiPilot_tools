import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import { createLauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import type {
  ExecuteOutcome,
  ExportOutcome,
  LauncherClient,
  SearchResponse,
  SettingsView,
} from './protocol'

export const client: LauncherClient = {
  listenShown: (handler) => listen('launcher://shown', (event) => handler(event.payload)),
  searchApps: (input) => invoke<SearchResponse | null>('search_apps', input),
  executeResult: (input) => invoke<ExecuteOutcome>('execute_result', input),
  loadSettings: () => invoke<SettingsView>('load_settings'),
  saveSettings: (input) => invoke<void>('save_settings', input),
  rescanApps: () => invoke<void>('rescan_apps'),
  exportValidationData: () => invoke<ExportOutcome>('export_validation_data'),
  clearValidationData: () => invoke<void>('clear_validation_data'),
  hideLauncher: () => invoke<void>('hide_launcher'),
}

const host = document.querySelector<HTMLElement>('#app')
if (!host) throw new Error('Missing application root')

const core = createLauncherCore(client)
const root = createRoot(host)
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
  onReady('failed')
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
