import { createRoot } from 'react-dom/client'

import { createLauncherCore } from './launcher-core'
import { LauncherView } from './launcher-view'
import {
  parseU64Decimal,
  type LauncherClient,
  type PluginInventorySnapshot,
  type PublicPluginInventory,
  type PublicPluginInventoryItem,
  type QuicklinkView,
  type ResultItem,
  type SearchResponse,
  type SettingsView,
} from './protocol'

const previewParameters = new URLSearchParams(window.location.search)
const previewTheme = previewParameters.get('theme') === 'light' ? 'light' : 'dark'
const settings: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
  theme: previewTheme,
  webSearchEngine: 'bing',
}
const revisionOne = parseU64Decimal('1')!
const previewQuicklinks: QuicklinkView[] = [
  {
    id: 'preview-jd',
    name: '京东搜索',
    command: 'jd',
    template: 'https://search.jd.com/Search?keyword={Query}',
    createdAt: '2026-08-31T00:00:00Z',
    updatedAt: '2026-08-31T00:00:00Z',
  },
  {
    id: 'preview-github',
    name: 'GitHub 搜索',
    command: 'gh',
    template: 'https://github.com/search?q={Query}',
    createdAt: '2026-08-31T00:00:00Z',
    updatedAt: '2026-08-31T00:00:00Z',
  },
]
let previewPublicPlugins: PublicPluginInventory = {
  revision: '1',
  items: [
    {
      pluginId: 'com.uipilot.notes',
      name: 'Notes',
      description: '项目笔记、代码片段和常用链接管理。',
      version: '0.1.0',
      source: 'localPackage',
      defaultName: 'notes',
      effectiveName: 'notes',
      enabled: true,
      fault: null,
      generation: 1,
      iconUrl: null,
      network: null,
      permissions: [
        { permission: 'ui.panel', supported: true, granted: true },
        { permission: 'files.userSelected', supported: true, granted: true },
      ],
      settings: [
        { definition: { type: 'boolean', key: 'showRecent', label: '显示最近笔记', default: true }, value: true },
      ],
    },
    {
      pluginId: 'com.uipilot.translate',
      name: 'Translate',
      description: '选中文本翻译、方向切换和复制结果。',
      version: '0.3.0',
      source: 'localPackage',
      defaultName: 'translate',
      effectiveName: 'translate',
      enabled: false,
      fault: null,
      generation: 2,
      iconUrl: null,
      network: { httpsHosts: ['api.example.com', 'translate.example.com'], credentials: [] },
      permissions: [
        { permission: 'clipboard.read', supported: true, granted: true },
        { permission: 'clipboard.write', supported: true, granted: true },
        { permission: 'network.https', supported: true, granted: false },
      ],
      settings: [
        {
          definition: {
            type: 'select',
            key: 'direction',
            label: '默认翻译方向',
            options: [
              { value: 'auto-zh', label: '自动 → 中文' },
              { value: 'zh-en', label: '中文 → 英文' },
            ],
            default: 'auto-zh',
          },
          value: 'auto-zh',
        },
        { definition: { type: 'secret', key: 'apiKey', label: 'API Key' }, secretConfigured: false },
      ],
    },
  ],
}
// Synthetic metadata only: preview saves retain configuration flags, never entered values.
const credentialPreviewPlugin: PublicPluginInventoryItem = {
  pluginId: 'com.uipilot.credential-demo', name: 'Credential Demo',
  description: '合成凭据设置与网络授权预览，不连接真实服务。', version: '0.1.0', source: 'localPackage',
  defaultName: 'credential-demo', effectiveName: 'credential-demo', enabled: true,
  fault: null, generation: 1, iconUrl: null,
  network: { httpsHosts: ['api.example.com'], credentials: [
    { id: 'service-token', settingKey: 'api-token', host: 'api.example.com', type: 'bearer' },
  ] },
  permissions: [{ permission: 'network.https', supported: true, granted: false }],
  settings: [
    { definition: { type: 'secret', key: 'api-token', label: 'API Token' }, secretConfigured: false },
    { definition: { type: 'secret', key: 'backup-token', label: 'Backup Token' }, secretConfigured: true },
  ],
}
previewPublicPlugins = { ...previewPublicPlugins, items: [...previewPublicPlugins.items, credentialPreviewPlugin] }
let previewScenarioPending = true
function updatePreviewPlugin(pluginId: string, update: (plugin: PublicPluginInventoryItem) => PublicPluginInventoryItem) {
  previewPublicPlugins = {
    revision: String(BigInt(previewPublicPlugins.revision) + 1n),
    items: previewPublicPlugins.items.map((plugin) => plugin.pluginId === pluginId ? update(plugin) : plugin),
  }
}

const previewLegacyPlugins: PluginInventorySnapshot = {
  revision: '1',
  items: [
    {
      key: 'preview-installed-notes',
      id: 'com.uipilot.notes',
      displayName: 'Notes',
      installed: { state: 'valid', activeVersion: '0.1.0', versions: ['0.1.0'], trigger: '/notes' },
      development: { state: 'absent' },
      description: {
        state: 'available',
        source: 'installed',
        markdown: '项目笔记、代码片段和常用链接管理。',
      },
    },
    {
      key: 'preview-development-translate',
      id: 'com.uipilot.translate',
      displayName: 'Translate',
      installed: { state: 'absent' },
      development: { state: 'valid', version: '0.3.0', trigger: '/translate' },
      description: {
        state: 'available',
        source: 'development',
        markdown: '选中文本翻译、方向切换和复制结果。',
      },
    },
  ],
}
const panelPreview = previewParameters.get('mode') === 'panel'
const settingsPreview = previewParameters.get('mode') === 'settings'
const quicklinksPreview = previewParameters.get('mode') === 'quicklinks'
const requestedPanelCommand = previewParameters.get('command')
const panelPreviewCommand = requestedPanelCommand && /^[a-z][a-z0-9-]{0,31}$/u.test(requestedPanelCommand)
  ? requestedPanelCommand
  : 'demo-panel'
const panelPreviewPluginId = 'com.uipilot.demo-panel'
const panelPreviewInputPlaceholder = panelPreviewCommand === 'notes' ? '搜索目录' : undefined

if (panelPreview) document.title = `UiPilot /${panelPreviewCommand} 外壳预览`
else if (settingsPreview) document.title = 'UiPilot 设置预览'
else if (quicklinksPreview) document.title = 'UiPilot /quicklinks 预览'

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
        favorite: {
          target: { kind: 'publicPlugin', pluginId: panelPreviewPluginId },
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
  if (normalized === '/quicklinks') {
    return {
      requestId: `browser-preview-${querySequence}`,
      items: [{
        resultId: 'preview-quicklinks',
        title: '/quicklinks',
        subtitle: '管理快速链接',
        iconKind: 'quicklinks',
        activation: { kind: 'openQuicklinks' },
        hasDefaultAction: false,
      }],
    }
  }
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
    queueMicrotask(() => handler({
      invocationId: 'browser-preview',
      target: settingsPreview ? 'settings' : 'launcher',
      notice: null,
    }))
    return () => undefined
  },
  listenHidden: async () => () => undefined,
  listenMessageStateChanged: async () => () => undefined,
  listenPluginPanelError: async () => () => undefined,
  listenPluginPanelReset: async () => () => undefined,
  listenPluginPanelFocusHostInput: async () => () => undefined,
  getMessageSummary: async () => ({ revision: '0', unreadCount: 0 }),
  openMessageCenter: async () => noMessages,
  readMessageCenter: async () => noMessages,
  clearMessages: async () => noMessages,
  searchApps: ({ query, querySequence }) => searchResponse(query, querySequence),
  listQuicklinks: async () => ({ items: previewQuicklinks }),
  saveQuicklink: async ({ input }) => ({
    id: input.id ?? `preview-${input.command || 'quicklink'}`,
    name: input.name,
    command: input.command,
    template: input.template,
    createdAt: '2026-08-31T00:00:00Z',
    updatedAt: '2026-08-31T00:00:00Z',
  }),
  deleteQuicklink: async () => undefined,
  chooseQuicklinkIcon: async () => null,
  openFind: async () => ({ status: 'forwarded' }),
  executeResult: async () => ({ status: 'launchRequested' }),
  openPluginPanel: async () => ({
    sessionEpoch: revisionOne,
    pluginId: panelPreviewPluginId,
    commandLabel: panelPreviewCommand,
    ...(panelPreviewInputPlaceholder === undefined ? {} : { inputPlaceholder: panelPreviewInputPlaceholder }),
    hostKeys: [],
  }),
  submitPluginPanel: async ({ sessionEpoch }) => ({
    sessionEpoch,
    pluginId: panelPreviewPluginId,
    commandLabel: panelPreviewCommand,
    ...(panelPreviewInputPlaceholder === undefined ? {} : { inputPlaceholder: panelPreviewInputPlaceholder }),
    hostKeys: [],
  }),
  enqueuePluginPanelHostKey: async () => ({ outcome: 'enqueued', routeSequence: revisionOne }),
  closePluginPanel: async () => undefined,
  setPluginPanelBounds: async () => undefined,
  acknowledgePluginPanelFocusHostInput: async () => undefined,
  commitPluginWindowTransfer: async () => undefined,
  listPublicPlugins: async () => previewPublicPlugins,
  selectPublicPluginArchive: async () => null,
  selectPublicPluginDirectory: async () => 'synthetic-credential-preview',
  preparePublicPlugin: async () => ({
    token: 'synthetic-credential-preview', pluginId: credentialPreviewPlugin.pluginId,
    name: 'Credential Demo', version: '0.2.0', permissions: ['network.https'], isUpdate: true,
    sourceVerified: false, iconUrl: null,
    network: { httpsHosts: ['api.example.com'], addedHttpsHosts: [],
      credentials: [{ id: 'service-token', settingKey: 'candidate-token', host: 'api.example.com', type: 'bearer' }],
      addedCredentialIds: ['service-token'], requiresNetworkConsent: true },
  }),
  commitPublicPlugin: async () => {
    updatePreviewPlugin(credentialPreviewPlugin.pluginId, (plugin) => ({
      ...plugin, version: '0.2.0', network: { httpsHosts: ['api.example.com'], credentials: [
        { id: 'service-token', settingKey: 'candidate-token', host: 'api.example.com', type: 'bearer' },
      ] }, permissions: [{ permission: 'network.https', supported: true, granted: true }],
      settings: [{ definition: { type: 'secret', key: 'candidate-token', label: 'Candidate Token' }, secretConfigured: false }],
    }))
  },
  cancelPublicPlugin: async () => undefined,
  setPublicPluginEnabled: async ({ pluginId, enabled }) => { updatePreviewPlugin(pluginId, (plugin) => ({ ...plugin, enabled })) },
  setPublicPluginNetworkAccess: async ({ pluginId, granted, expectedInventoryRevision }) => {
    if (granted && previewScenarioPending && previewParameters.get('credentialScenario') === 'stale') {
      previewScenarioPending = false
      updatePreviewPlugin(pluginId, (plugin) => ({ ...plugin, description: '合成并发更新：请重新核对绑定后授权。' }))
    }
    if (granted && expectedInventoryRevision !== previewPublicPlugins.revision) throw new Error('Synthetic stale inventory')
    updatePreviewPlugin(pluginId, (plugin) => ({ ...plugin,
      permissions: plugin.permissions.map((permission) => permission.permission === 'network.https' ? { ...permission, granted } : permission),
    }))
  },
  setPublicPluginFavorite: async () => undefined,
  setBuiltinFeatureFavorite: async () => undefined,
  setPublicPluginEffectiveName: async ({ pluginId, nameOverride }) => { updatePreviewPlugin(pluginId, (plugin) => ({ ...plugin, effectiveName: nameOverride ?? plugin.defaultName })) },
  savePublicPluginSettings: async ({ input }) => {
    await new Promise<void>((resolve) => window.setTimeout(resolve, 500))
    updatePreviewPlugin(input.pluginId, (plugin) => ({ ...plugin, settings: plugin.settings.map((setting) =>
      Object.prototype.hasOwnProperty.call(input.secrets, setting.definition.key)
        ? { ...setting, secretConfigured: input.secrets[setting.definition.key] !== null } : setting),
    }))
    if (previewScenarioPending && previewParameters.get('credentialScenario') === 'save-error') {
      previewScenarioPending = false
      throw new Error('Synthetic partial save failure')
    }
  },
  uninstallPublicPlugin: async () => undefined,
  listPlugins: async () => previewLegacyPlugins,
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
  if (!panelPreview) return () => undefined
  const contentPreview = panelPreviewCommand === 'notes'
    ? { title: 'Notes 插件内容预览', path: 'com.uipilot.notes' }
    : panelPreviewCommand === 'clipboard-history'
      ? { title: '剪贴板历史插件内容预览', path: 'com.uipilot.clipboard-history' }
      : null
  if (!contentPreview) return () => undefined

  const frame = document.createElement('iframe')
  frame.title = contentPreview.title
  frame.src = `/examples/public-plugins/${contentPreview.path}/preview.html?theme=${previewTheme}`
  frame.style.display = 'block'
  frame.style.width = '100%'
  frame.style.height = '100%'
  frame.style.border = '0'
  frame.style.background = previewTheme === 'dark' ? '#07080a' : '#f7f7f8'
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
    void startPreview()
  }}
/>)
const stopPanelContentPreview = mountPanelContentPreview(host)

async function startPreview(): Promise<void> {
  await core.start()
  if (!quicklinksPreview || core.getSnapshot().view !== 'launcher') return
  await core.text({
    kind: 'ordinaryInput',
    control: core.getSnapshot().queryControl,
    value: '/quicklinks',
    inputType: 'insertText',
  })
  await core.keyDown('Enter', false)
}

window.addEventListener('pagehide', () => {
  stopPanelContentPreview()
  stopPanelPreview()
  core.destroy()
  root.unmount()
}, { once: true })
