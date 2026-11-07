import { createMacLoginItemClient } from './macos-login-item'
import { parsePublicPluginDiagnostics } from './protocol'
import { createCredentialClient } from './credential-client'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import { createFindCore } from './find-core'
import { FindView } from './find-view'
import { parseHostCapabilities, type HostCapabilities } from './host-capabilities'
import { createLauncherCore } from './launcher-core'
import { HOTKEY_RECORDING_CURRENT_DOM_EVENT, LauncherView } from './launcher-view'
import { createPluginWindowCore } from './plugin-window-core'
import { PluginWindowView } from './plugin-window-view'
import {
  parseFileSearchResponse,
  parseCommandEntryResolution,
  parseMessageCenterSnapshot,
  parseMessageSummary,
  parsePluginInventorySnapshot,
  parsePluginPanelCommandResult,
  parsePluginPanelHostKeyEnqueueResult,
  parsePublicPluginInventory,
  parseBuiltinFeatures,
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
  type QuicklinkIconCandidate,
  type QuicklinkListResponse,
  type QuicklinkView,
  type SearchResponse,
  type SettingsView,
  type ScreenshotSaveDirectoryView,
} from './protocol'

export const client: LauncherClient = {
  macosLoginItem: createMacLoginItemClient(invoke),
  listenShown: (handler) => listen('launcher://shown', (event) => handler(event.payload)),
  listenHidden: (handler) => listen('launcher://hidden', () => handler()),
  listenBuiltinFeaturesChanged: (handler) => listen('builtin-features://changed', () => handler()),
  listenPublicPluginPermissionsChanged: (handler) => listen('public-plugin-permissions-changed', () => handler()),
  listenMessageStateChanged: (handler) =>
    listen('message-center://state-changed', (event) => handler(event.payload)),
  listenPluginPanelError: (handler) =>
    listen('uipilot-plugin-panel-error', (event) => handler(event.payload)),
  listenPluginPanelStatus: (handler) =>
    listen('uipilot-plugin-panel-status', (event) => handler(event.payload)),
  listenPluginPanelActionsSubmit: (handler) => listen('uipilot-plugin-panel-actions-submit', (event) => handler(event.payload)),
  openPluginPanelActions: (input) => invoke('open_plugin_panel_actions', input),
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
  listQuicklinks: () => invoke<QuicklinkListResponse>('list_quicklinks'),
  saveQuicklink: (input) => invoke<QuicklinkView>('save_quicklink', input),
  deleteQuicklink: (input) => invoke('delete_quicklink', input),
  chooseQuicklinkIcon: () => invoke<QuicklinkIconCandidate | null>('choose_quicklink_icon'),
  openFind: (input) => invoke('open_find_window', { input }),
  startScreenshot: (input) => invoke('start_screenshot', input),
  listenScreenshotError: (handler) => listen('screenshot://error', (event) => handler(event.payload)),
  executeResult: (input) => invoke<ExecuteOutcome>('execute_result', input),
  executeResultAction: (request) => invoke<unknown>('execute_result_action', { request }),
  retireResultActions: (request) => invoke<void>('retire_result_actions', { request }),
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
  setPluginPanelBounds: async (input) => { await invoke('set_plugin_panel_bounds', { input }) },
  closePluginPanel: async (input) => { await invoke('close_plugin_panel', { input }) },
  acknowledgePluginPanelFocusHostInput: async (input) => {
    await invoke('plugin_panel_focus_host_input_ack', { ...input })
  },
  commitPluginWindowTransfer: (input) => invoke<void>('commit_plugin_window_transfer', input),
  setPublicPluginCommandHotkey: (input) => invoke('set_public_plugin_command_hotkey', { input }),
  cancelTranslationSelection: () => invoke('cancel_translation_selection'),
  getTranslationSelectionSettings: () => invoke('get_translation_selection_settings'),
  setTranslationSelectionHotkey: (input) => invoke('set_translation_selection_hotkey', input),
  resolvePublicPluginCommandEntry: async (input) => {
    const resolved = parseCommandEntryResolution(await invoke('resolve_public_plugin_command_entry', input))
    if (!resolved) throw { code: 'pluginQueryFailed' }
    return resolved
  },
  listBuiltinFeatures: async () => {
    const items = parseBuiltinFeatures(await invoke<unknown>('list_builtin_features'))
    if (!items) throw { code: 'builtinFeatureListFailed', message: 'builtin feature list failed' }
    return items
  },
  setBuiltinFeatureName: (input) => invoke('set_builtin_feature_name', input),
  setBuiltinFeatureHotkey: (input) => invoke('set_builtin_feature_hotkey', input),
  setBuiltinFeatureEnabled: async (input) => {
    const items = parseBuiltinFeatures(await invoke<unknown>('set_builtin_feature_enabled', input))
    if (!items) throw { code: 'builtinFeatureListFailed', message: 'builtin feature list failed' }
    return items
  },
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
  getPublicPluginDiagnostics: async (input) => {
    const report = parsePublicPluginDiagnostics(await invoke<unknown>('get_public_plugin_diagnostics', input))
    if (!report) throw { code: 'pluginUnavailable' }
    return report
  },
  copyPublicPluginDiagnostics: async (input) => {
    const report = parsePublicPluginDiagnostics(input.report)
    if (!report) throw { code: 'invalidPackage' }
    await invoke('copy_public_plugin_diagnostics', { report })
  },
  commitPublicPlugin: async (input) => { await invoke('commit_public_plugin_install', input) },
  cancelPublicPlugin: async (input) => { await invoke('cancel_public_plugin_install', input) },
  setPublicPluginEnabled: async (input) => { await invoke('set_plugin_enabled', input) },
  setPublicPluginNetworkAccess: async (input) => { await invoke('set_public_plugin_network_access', input) },
  setPublicPluginPermissionAccess: async (input) => { await invoke('set_public_plugin_permission_access', input) },
  setPublicPluginFavorite: async (input) => { await invoke('set_plugin_favorite', input) },
  setBuiltinFeatureFavorite: async (input) => { await invoke('set_builtin_feature_favorite', { input }) },
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
  selectScreenshotSaveDirectory: () => invoke<ScreenshotSaveDirectoryView | null>('select_screenshot_save_directory'),
  setScreenshotSaveDirectory: (input) => invoke<ScreenshotSaveDirectoryView>('set_screenshot_save_directory', { input: { directory: input.input.directory } }),
  setScreenshotHotkey: (input) => invoke('set_screenshot_hotkey', input),
  setHotkeyRecording: (input) => invoke('set_hotkey_recording', input),
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
  hideLauncher: (invocationId) => invoke<void>('hide_launcher', ...(invocationId === undefined ? [] : [{ invocationId }])),
  validateYoudaoTranslation: (input) => invoke('validate_youdao_translation', { input }),
  credentials: createCredentialClient(invoke),
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
const findForwardEvent = 'find://forwarded'
const findThemeEvent = 'find://theme-changed'
export const findClient: FindClient = {
  listenForward: (handler) => listen(findForwardEvent, (event) => handler(event.payload)),
  listenThemeChanged: (handler) => listen(findThemeEvent, (event) => handler(event.payload)),
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
  loadThumbnail: (input) => invoke<unknown>('load_find_thumbnail', input),
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

function mountMain(host: HTMLElement, capabilities: HostCapabilities) {
  const core = createLauncherCore(client, undefined, capabilities)
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
    root.render(createElement(LauncherView, {
      core, onReady,
      setSettingsPinned: capabilities.platform === 'windows'
        ? (active: boolean, pinned: boolean) => invoke<void>('set_settings_pinned', { active, pinned })
        : undefined,
    }))
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
    if (tornDown) return
    try {
      const recordingUnlisten = await listen('hotkey-recording://current', (event) => {
        let detail = event.payload
        if (capabilities.platform === 'macos') {
          const value = detail as { invocationId?: unknown; hotkey?: unknown } | null
          if (!value || typeof value.hotkey !== 'string' || value.invocationId !== core.getSnapshot().invocationId) return
          detail = value.hotkey
        }
        window.dispatchEvent(new CustomEvent(HOTKEY_RECORDING_CURRENT_DOM_EVENT, { detail }))
      })
      let entryUnlisten: (() => void) | undefined
      try { if (capabilities.commandHotkeys) entryUnlisten = await listen('command-hotkey://entry', (event) => core.commandHotkeyEntry(event.payload)) }
      catch (error) { recordingUnlisten(); throw error }
      const unlisten = () => { recordingUnlisten(); entryUnlisten?.() }
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
}

function bootstrapMain(host: HTMLElement): void {
  const lifetime = new AbortController()
  window.addEventListener('pagehide', () => lifetime.abort(), { once: true, signal: lifetime.signal })
  const load = async () => {
    try {
      const capabilities = parseHostCapabilities(await invoke<unknown>('get_host_capabilities'))
      if (!capabilities) throw new Error('Unsupported capability snapshot')
      if (lifetime.signal.aborted) return
      host.replaceChildren()
      mountMain(host, capabilities)
      lifetime.abort()
    } catch {
      if (lifetime.signal.aborted) return
      const status = document.createElement('div')
      status.setAttribute('role', 'status')
      status.textContent = '无法读取启动器功能。'
      const retry = document.createElement('button')
      retry.type = 'button'
      retry.textContent = '重试'
      retry.addEventListener('click', () => { retry.disabled = true; void load() }, { once: true, signal: lifetime.signal })
      host.replaceChildren(status, retry)
    }
  }
  void load()
}

function mountFind(host: HTMLElement): void {
  const scope = (window as Window & { __UIPILOT_MACOS_FIND__?: unknown }).__UIPILOT_MACOS_FIND__
  if (scope !== undefined && scope !== 'public-preview' && scope !== 'indexed-local-qa') {
    host.textContent = '文件搜索范围不可用。'
    return
  }
  const core = createFindCore(scope ? {
    ...findClient,
    listenForward: (handler) => listen(findForwardEvent, event => handler(event.payload), { target: 'find' }),
    listenThemeChanged: (handler) => listen(findThemeEvent, event => handler(event.payload), { target: 'find' }),
    macos: {
      scope,
      ...(scope === 'public-preview' ? { documents: {
        get: (input) => invoke<unknown>('get_find_documents_access', input),
        grant: (input) => invoke<unknown>('grant_find_documents_access', input),
        revoke: (input) => invoke<unknown>('revoke_find_documents_access', input),
      } } : {}),
      cancelQuery: (input) => invoke<void>('cancel_find_query', input),
      openDocument: (input) => invoke<ExecuteOutcome>('open_find_document', input),
    },
  } : findClient)
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
}

const label = currentWindowLabel()
if (label === 'main') {
  bootstrapMain(host)
} else if (label === 'screenshot') {
  void (async () => {
    const [{createScreenshotClient}, {createScreenshotCore}, {ScreenshotView}] = await Promise.all([
      import('./screenshot-client'), import('./screenshot-core'), import('./screenshot-view'),
    ])
    const instance = (window as Window & {__UIPILOT_SCREENSHOT_INSTANCE__?: unknown}).__UIPILOT_SCREENSHOT_INSTANCE__
    const core = createScreenshotCore(createScreenshotClient(instance), String(instance))
    const root = createRoot(host, {onUncaughtError: () => core.destroy()})
    const teardown = () => {
      window.removeEventListener('pagehide', teardown)
      core.destroy()
      root.unmount()
    }
    window.addEventListener('pagehide', teardown)
    root.render(createElement(ScreenshotView, {core}))
    await core.start()
  })().catch(() => {host.textContent = '截图预览暂不可用。'})
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
  mountFind(host)
} else {
  throw new Error(`Unknown Tauri window label: ${label}`)
}
