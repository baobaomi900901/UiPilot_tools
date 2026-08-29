import { createRoot } from 'react-dom/client'

import { createLauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import { parseU64Decimal, type LauncherClient, type ResultItem, type SearchResponse, type SettingsView } from './protocol'

const settings: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
  theme: 'dark',
  webSearchEngine: 'bing',
}
const revisionOne = parseU64Decimal('1')!
const previewParameters = new URLSearchParams(window.location.search)
const panelPreview = previewParameters.get('mode') === 'panel'
const requestedPanelCommand = previewParameters.get('command')
const panelPreviewCommand = requestedPanelCommand && /^[a-z][a-z0-9-]{0,31}$/u.test(requestedPanelCommand)
  ? requestedPanelCommand
  : 'demo-panel'
const panelPreviewPluginId = 'com.uipilot.demo-panel'

if (panelPreview) document.title = `UiPilot /${panelPreviewCommand} 外壳预览`

const appIconAsset = new URL('../src-tauri/icons/icon.png', import.meta.url)
let appIconDataUrl: Promise<string | null> | undefined

function loadAppIcon(): Promise<string | null> {
  appIconDataUrl ??= fetch(appIconAsset).then(async (response) => {
    if (!response.ok) return null
    const blob = await response.blob()
    return new Promise<string | null>((resolve) => {
      const reader = new FileReader()
      reader.addEventListener('load', () => resolve(typeof reader.result === 'string' ? reader.result : null))
      reader.addEventListener('error', () => resolve(null))
      reader.readAsDataURL(blob)
    })
  }, () => null)
  return appIconDataUrl
}

async function searchResponse(query: string, querySequence: number): Promise<SearchResponse> {
  if (panelPreview) {
    return {
      requestId: `browser-preview-${querySequence}`,
      items: [{
        resultId: `preview-${panelPreviewCommand}`,
        title: `/${panelPreviewCommand}`,
        subtitle: 'Panel 外壳样式预览',
        activation: {
          kind: 'panelActivation',
          pluginId: panelPreviewPluginId,
          initialArgument: '',
          favorite: false,
        },
        hasDefaultAction: false,
      }],
    }
  }
  const icon = await loadAppIcon()
  const items: ResultItem[] = [
    {
      resultId: 'preview-uipilot',
      title: 'UiPilot',
      subtitle: String.raw`D:\code\UiPilot_tools\src-tauri\target\debug\uipilot.exe`,
      activation: { kind: 'executeResult' },
      hasDefaultAction: true,
      ...(icon ? { icon } : {}),
    },
    {
      resultId: 'preview-explorer',
      title: '文件资源管理器',
      subtitle: String.raw`C:\Windows\explorer.exe`,
      activation: { kind: 'executeResult' },
      hasDefaultAction: true,
    },
    {
      resultId: 'preview-notepad',
      title: '记事本',
      subtitle: String.raw`C:\Windows\System32\notepad.exe`,
      activation: { kind: 'executeResult' },
      hasDefaultAction: true,
    },
  ]
  const normalized = query.trim().toLocaleLowerCase()
  return {
    requestId: `browser-preview-${querySequence}`,
    items: normalized
      ? items.filter((item) => `${item.title} ${item.subtitle ?? ''}`.toLocaleLowerCase().includes(normalized))
      : items,
  }
}

const noMessages = { revision: '0', unreadCount: 0, messages: [] }

const client: LauncherClient = {
  listenShown: async (handler) => {
    queueMicrotask(() => handler({ invocationId: 'browser-preview', target: 'launcher', notice: null }))
    return () => undefined
  },
  listenMessageStateChanged: async () => () => undefined,
  listenPluginPanelError: async () => () => undefined,
  listenPluginPanelReset: async () => () => undefined,
  listenPluginPanelFocusHostInput: async () => () => undefined,
  getMessageSummary: async () => ({ revision: '0', unreadCount: 0 }),
  openMessageCenter: async () => noMessages,
  readMessageCenter: async () => noMessages,
  clearMessages: async () => noMessages,
  searchApps: ({ query, querySequence }) => searchResponse(query, querySequence),
  openFind: async () => ({ status: 'forwarded' }),
  executeResult: async () => ({ status: 'launchRequested' }),
  openPluginPanel: async () => ({
    sessionEpoch: revisionOne,
    pluginId: panelPreviewPluginId,
    commandLabel: panelPreviewCommand,
    hostKeys: [],
  }),
  submitPluginPanel: async ({ sessionEpoch }) => ({
    sessionEpoch,
    pluginId: panelPreviewPluginId,
    commandLabel: panelPreviewCommand,
    hostKeys: [],
  }),
  enqueuePluginPanelHostKey: async () => ({ outcome: 'enqueued', routeSequence: revisionOne }),
  closePluginPanel: async () => undefined,
  setPluginPanelBounds: async () => undefined,
  acknowledgePluginPanelFocusHostInput: async () => undefined,
  commitPluginWindowTransfer: async () => undefined,
  listPublicPlugins: async () => ({ revision: '0', items: [] }),
  selectPublicPluginArchive: async () => null,
  selectPublicPluginDirectory: async () => null,
  preparePublicPlugin: async () => { throw new Error('Preview does not install plugins') },
  commitPublicPlugin: async () => undefined,
  cancelPublicPlugin: async () => undefined,
  setPublicPluginEnabled: async () => undefined,
  setPublicPluginNetworkAccess: async () => undefined,
  setPublicPluginFavorite: async () => undefined,
  setPublicPluginEffectiveName: async () => undefined,
  savePublicPluginSettings: async () => undefined,
  uninstallPublicPlugin: async () => undefined,
  listPlugins: async () => ({ revision: '0', items: [] }),
  installPlugin: async () => ({ revision: '1' }),
  reloadPlugin: async () => ({ revision: '1' }),
  deletePlugin: async () => ({ revision: '1' }),
  loadSettings: async () => settings,
  saveSettings: async () => undefined,
  saveHotkey: async ({ hotkey }) => ({ hotkey: hotkey.hotkey }),
  setThemePreference: async () => undefined,
  setWebSearchEngine: async () => undefined,
  hideLauncher: async () => undefined,
}

const host = document.querySelector<HTMLElement>('#app')
if (!host) throw new Error('Missing preview root')

function mountPanelContentPreview(hostElement: HTMLElement): () => void {
  if (!panelPreview || panelPreviewCommand !== 'notes') return () => undefined

  const frame = document.createElement('iframe')
  frame.title = 'Notes 插件内容预览'
  frame.src = '/examples/public-plugins/com.uipilot.notes/preview.html?theme=dark'
  frame.style.display = 'block'
  frame.style.width = '100%'
  frame.style.height = '100%'
  frame.style.border = '0'
  frame.style.background = '#202020'
  frame.allow = 'clipboard-write'

  const attach = () => {
    const region = hostElement.querySelector<HTMLElement>('.panel-host-region')
    if (!region || frame.parentElement === region) return
    region.replaceChildren(frame)
  }
  const observer = new MutationObserver(attach)
  observer.observe(hostElement, { childList: true, subtree: true })
  attach()

  return () => {
    observer.disconnect()
    frame.remove()
  }
}

const core = createLauncherCore(client)
const root = createRoot(host)
let started = false
let panelActivationQueued = false
const stopPanelPreview = panelPreview
  ? core.subscribe(() => {
      if (panelActivationQueued) return
      const index = core.getSnapshot().results.findIndex(
        (result) => result.panelActivation?.pluginId === panelPreviewPluginId,
      )
      if (index < 0) return
      panelActivationQueued = true
      queueMicrotask(() => core.activateResult(index))
    })
  : () => undefined

root.render(<LauncherView
  core={core}
  onReady={(result) => {
    if (result === 'failed') {
      core.destroy()
      return
    }
    if (started) return
    started = true
    void core.start()
  }}
/>)
const stopPanelContentPreview = mountPanelContentPreview(host)

window.addEventListener('pagehide', () => {
  stopPanelContentPreview()
  stopPanelPreview()
  core.destroy()
  root.unmount()
}, { once: true })
