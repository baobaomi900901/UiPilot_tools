import { MacPluginManagement, type MacPluginManagementHandle, type MacPluginManagementStatus } from './macos-plugin-management'
import { MacHotkeyStatusNotice } from './macos-hotkey-status'
import { bindCompositionKeys, isCompositionKey } from './ime-keyboard'
import { MacLoginItemControl } from './macos-login-item-control'
import { settingsTabAvailable } from './host-capabilities'
import { BuiltinFeatureDetail } from './builtin-feature-panel'
import {
  App,
  Badge,
  Button,
  Checkbox,
  ConfigProvider,
  Dropdown,
  Form,
  Input,
  Popconfirm,
  Select,
  Spin,
  Switch,
  Tabs,
  Tooltip,
  type InputProps,
  type InputRef,
} from 'antd'
import { ArrowLeft, ArrowRightLeft, Calculator, Calendar, Clock, FolderSearch, ImageIcon, Link2, PanelsTopLeft, Pin, PinOff, Plus, Ruler, Save, ScanLine, Search, Settings, Star, Trash2, X } from 'lucide-react'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react'
import ReactMarkdown from 'react-markdown'
import { AppVersion } from './app-version'
import { createContextActionsMenu, type ContextActionsMenu, type ContextAction } from './context-actions-ui.js'
import { OverlayScrollbarsComponent } from 'overlayscrollbars-react'

import { sameFavoriteTarget, type HotkeyRecordingPhase, type LauncherCore } from './launcher-core'
import { CredentialSettings, type CredentialSettingsHandle, type CredentialSettingsStatus } from './credential-settings'
import { MessageCenterPanel } from './message-center-panel'
import { bindNativeTextInput } from './native-input'
import { PluginIcon } from './plugin-icon'
import { PublicPluginDetail, PublicPluginPanel, type PublicPluginRemovalOutcome } from './public-plugin-panel'
import { PluginPermissionsPanel } from './plugin-permissions-panel'
import type {
  ControlKey,
  FileCategory,
  FileResultKind,
  PluginPanelBounds,
  PublicPluginInventoryItem,
  BuiltinFeatureView,
  QuicklinksSnapshot,
  ResultFavoriteTarget,
  ResultIconKind,
  SettingsTabKey,
  ThemePreference,
  WebSearchEngine,
} from './protocol'
import { resolveUiColorScheme, uiThemeConfig } from './ui-theme'
import { formatHotkeyDisplay } from './hotkey-recorder'

export interface LauncherViewProps {
  core: LauncherCore
  onReady: (result: 'ready' | 'failed') => void
  setSettingsPinned?: (active: boolean, pinned: boolean, invocationId?: string) => Promise<void>
}

interface BoundInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  core: LauncherCore
  control: ControlKey
  value: string
  onBound?: (input: HTMLInputElement) => void
  onUnbound?: (input: HTMLInputElement) => void
  onBindingFailed?: () => void
}

export { HOTKEY_RECORDING_CURRENT_DOM_EVENT } from './hotkey-recorder-input'
import { HotkeyRecorderInput } from './hotkey-recorder-input'
import { isWindowPinShortcut } from './window-pin-shortcut'

const QUICKLINKS_STATUS_DURATION_MS = 2000
const QUICKLINK_EDITOR_FOCUSABLE_SELECTOR = 'input:not(:disabled), textarea:not(:disabled), button:not(:disabled)'

type QuicklinksUiStatus = {
  message: string
  tone: 'success' | 'error'
}

function BoundInput({ core, control, value, onBound, onUnbound, onBindingFailed, ...props }: BoundInputProps) {
  const ref = useRef<InputRef>(null)
  useLayoutEffect(() => {
    const input = ref.current?.input
    if (!input) {
      onBindingFailed?.()
      return () => core.retireControl(control)
    }
    let unbind: (() => void) | undefined
    let unbindKeys: (() => void) | undefined
    let cleaned = false
    const cleanup = () => {
      if (cleaned) return
      cleaned = true
      try {
        onUnbound?.(input)
      } finally {
        try {
          unbindKeys?.()
          unbind?.()
        } finally {
          core.retireControl(control)
        }
      }
    }
    try {
      unbindKeys = bindCompositionKeys(input)
      unbind = bindNativeTextInput(input, control, core.text)
      onBound?.(input)
      return cleanup
    } catch {
      try {
        cleanup()
      } catch { /* failure reporting below remains authoritative */ }
      onBindingFailed?.()
      return () => undefined
    }
  }, [control, core, onBindingFailed, onBound, onUnbound])
  return <Input {...props} ref={ref} value={value} onChange={() => {}} />
}

function CommandTag({
  commandLabel,
  className = '',
  disabled = false,
  exitLabel,
  exitTitle,
  onClose,
}: {
  commandLabel: string
  className?: string
  disabled?: boolean
  exitLabel: string
  exitTitle: string
  onClose: () => void
}) {
  return (
    <div
      className={`panel-command-tag${className ? ` ${className}` : ''}`}
      role="group"
      aria-label={`command ${commandLabel}`}
    >
      <span>/{commandLabel}</span>
      <Tooltip title={exitTitle}>
        <Button
          aria-label={exitLabel}
          disabled={disabled}
          icon={<X aria-hidden size={14} strokeWidth={2} />}
          tabIndex={-1}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onClose}
          size="small"
          type="text"
        />
      </Tooltip>
    </div>
  )
}

function composing(event: ReactKeyboardEvent): boolean {
  return isCompositionKey(event.nativeEvent)
}

function preventBrowserFind(event: ReactKeyboardEvent<HTMLElement>): void {
  if (
    event.key.toLowerCase() === 'f' &&
    event.ctrlKey !== event.metaKey &&
    !event.shiftKey &&
    !event.altKey
  ) event.preventDefault()
}

function BuiltInResultIcon({ kind }: { kind: ResultIconKind }) {
  if (kind === 'calculator') {
    return (
      <span className="built-in-result-icon built-in-result-icon-calculator" data-result-icon-kind={kind}>
        <Calculator aria-hidden size={27} strokeWidth={1.8} />
      </span>
    )
  }
  if (kind === 'currency') {
    return (
      <span className="built-in-result-icon built-in-result-icon-currency" data-result-icon-kind={kind}>
        <ArrowRightLeft aria-hidden size={26} strokeWidth={1.8} />
      </span>
    )
  }
  if (kind === 'date') {
    return (
      <span className="built-in-result-icon built-in-result-icon-date" data-result-icon-kind={kind}>
        <Calendar aria-hidden size={26} strokeWidth={1.8} />
      </span>
    )
  }
  if (kind === 'timeZone') {
    return (
      <span className="built-in-result-icon built-in-result-icon-time-zone" data-result-icon-kind={kind}>
        <Clock aria-hidden size={26} strokeWidth={1.8} />
      </span>
    )
  }
  if (kind === 'unit') {
    return (
      <span className="built-in-result-icon built-in-result-icon-unit" data-result-icon-kind={kind}>
        <Ruler aria-hidden size={26} strokeWidth={1.8} />
      </span>
    )
  }
  if (kind === 'find' || kind === 'quicklinks' || kind === 'screenshot') {
    const Icon = kind === 'screenshot' ? ScanLine : kind === 'find' ? FolderSearch : Link2
    return (
      <span className={`built-in-result-icon built-in-result-icon-${kind === 'screenshot' ? 'find' : kind}`} data-result-icon-kind={kind}>
        <Icon aria-hidden size={26} strokeWidth={1.8} />
      </span>
    )
  }
  return (
    <span
      className="built-in-result-icon built-in-result-icon-web-search"
      data-result-icon-kind={kind}
    >
      <PanelsTopLeft aria-hidden size={25} strokeWidth={1.8} />
      <Search aria-hidden className="built-in-result-icon-badge" size={12} strokeWidth={2} />
    </span>
  )
}


const pluginMarkdownElements = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'p', 'ul', 'ol', 'li', 'em', 'strong', 'code', 'pre']

const themeOptions = [
  { value: 'system', label: '跟随系统' },
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
] satisfies { value: ThemePreference; label: string }[]

const webSearchEngineOptions = [
  { value: 'bing', label: 'Bing' },
  { value: 'baidu', label: '百度' },
  { value: 'google', label: 'Google' },
] satisfies { value: WebSearchEngine; label: string }[]

const settingsScrollbarOptions = {
  overflow: { x: 'hidden', y: 'scroll' },
  scrollbars: { theme: 'os-theme-uipilot', visibility: 'auto', autoHide: 'never' },
} as const

const fileCategoryOptions = [
  { value: 'all', label: '全部' },
  { value: 'folder', label: '文件夹' },
  { value: 'excel', label: 'Excel' },
  { value: 'word', label: 'Word' },
  { value: 'ppt', label: 'PPT' },
  { value: 'pdf', label: 'PDF' },
  { value: 'image', label: '图片' },
  { value: 'video', label: '视频' },
  { value: 'audio', label: '音频' },
  { value: 'archive', label: '压缩包' },
] satisfies readonly { value: FileCategory; label: string }[]
function settingsTabKey(target: EventTarget): SettingsTabKey | null {
  if (!(target instanceof HTMLElement) || target.getAttribute('role') !== 'tab') return null
  const controlledPanel = target.getAttribute('aria-controls')
  if (controlledPanel?.endsWith('-panel-general')) return 'general'
  if (controlledPanel?.endsWith('-panel-messages')) return 'messages'
  if (controlledPanel?.endsWith('-panel-plugins')) return 'plugins'
  if (controlledPanel?.endsWith('-panel-permissions')) return 'permissions'
  if (controlledPanel?.endsWith('-panel-credentials')) return 'credentials'
  return null
}

function fileSize(kind: FileResultKind, sizeBytes: string | null): string {
  if (kind === 'folder' || sizeBytes === null) return '--'
  return `${sizeBytes} B`
}

function fileModified(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}

function comparePluginVersion(left: string, right: string): number {
  const leftParts = left.split('.').map(Number)
  const rightParts = right.split('.').map(Number)
  for (let index = 0; index < 3; index += 1) {
    const difference = leftParts[index]! - rightParts[index]!
    if (difference !== 0) return difference
  }
  return 0
}

function scrollFileResultIntoView(container: HTMLElement | null, selected: HTMLElement | undefined): void {
  if (!container || !selected) return
  const selectedTop = selected.offsetTop
  const selectedBottom = selectedTop + selected.offsetHeight
  const visibleTop = container.scrollTop
  const visibleBottom = visibleTop + container.clientHeight
  if (selectedTop < visibleTop) {
    container.scrollTop = selectedTop
  } else if (selectedBottom > visibleBottom) {
    container.scrollTop = selectedBottom - container.clientHeight
  }
}

export function LauncherView({ core, onReady, setSettingsPinned }: LauncherViewProps): React.JSX.Element {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  const [hotkeyStatusRevision, setHotkeyStatusRevision] = useState(0)
  const [scheme] = useState(() => window.matchMedia('(prefers-color-scheme: dark)'))
  const [systemDark, setSystemDark] = useState(scheme.matches)
  const [publicPluginRemovalOutcome, setPublicPluginRemovalOutcome] = useState<PublicPluginRemovalOutcome | null>(null)
  const [detailHeaderActions, setDetailHeaderActions] = useState<HTMLDivElement | null>(null)
  const [builtinFeatureDetail, setBuiltinFeatureDetail] = useState<BuiltinFeatureView | null>(null)
  const [publicPluginDetail, setPublicPluginDetail] = useState<{ plugin: PublicPluginInventoryItem; revision: string; hotkeyRevision: string; session: number } | null>(null)
  const detailSession = useRef(0)
  const detailReload = useRef(0)
  const detailReloadPending = useRef<{
    session: number
    request: number
    promise: Promise<void>
    resolve: (next?: PromiseLike<void>) => void
    reject: (reason: unknown) => void
    beforePublish: Array<(revision: string) => void>
  } | null>(null)
  const selectedPublicPlugin = publicPluginDetail?.plugin ?? null
  const [quicklinksStatus, setQuicklinksStatus] = useState<QuicklinksUiStatus | null>(null)
  const colorScheme = resolveUiColorScheme(snapshot.theme, systemDark)
  const queryRef = useRef<HTMLInputElement | null>(null)
  const mainResultInputRef = useRef<HTMLInputElement | null>(null)
  const panelInputRef = useRef<HTMLInputElement | null>(null)
  const operationsMenu = useRef<ContextActionsMenu | null>(null)
  const [operationsOpen, setOperationsOpen] = useState(false)
  const [operationsError, setOperationsError] = useState('')
  const panelActionSource = useRef<'hostInput' | 'content'>('hostInput')
  const panelMenuTicket = useRef<{ ticket: string; sessionEpoch: string; suffix: string } | null>(null)
  useEffect(() => {
    const blur = () => { panelActionSource.current = 'content'; operationsMenu.current?.close(false) }
    window.addEventListener('blur', blur)
    let alive = true
    let unlisten: (() => void) | undefined
    if (snapshot.panel) void core.client.listenPluginPanelActionsSubmit?.((payload) => {
      const record = payload as { sessionEpoch?: unknown; ticket?: unknown } | null
      const pending = panelMenuTicket.current
      const current = core.getSnapshot()
      if (!alive || !pending || !record || record.sessionEpoch !== pending.sessionEpoch || record.ticket !== pending.ticket ||
          current.view !== 'launcher' || current.panel?.sessionEpoch !== pending.sessionEpoch || current.panel.suffix !== pending.suffix || current.panel.closePending) return
      panelMenuTicket.current = null
      core.keyDown('Enter', false)
    }).then((dispose) => { if (alive) unlisten = dispose; else dispose() }).catch(() => { if (alive) setOperationsError('操作面板暂不可用') })
    return () => { alive = false; unlisten?.(); window.removeEventListener('blur', blur); operationsMenu.current?.close(false) }
  }, [core, snapshot.panel?.sessionEpoch])
  useEffect(() => {
    operationsMenu.current?.close(false)
    panelMenuTicket.current = null
    setOperationsError('')
  }, [snapshot.invocationId, snapshot.viewEpoch, snapshot.querySequence, snapshot.results[snapshot.selectedIndex]?.key, snapshot.panel?.sessionEpoch, snapshot.panel?.suffix])
  const quicklinksInputRef = useRef<HTMLInputElement | null>(null)
  const quicklinkItemRefs = useRef(new Map<string, HTMLButtonElement>())
  const quicklinksEditorRef = useRef<HTMLElement | null>(null)
  const quicklinkCreateDialogRef = useRef<HTMLDialogElement | null>(null)
  const quicklinkFocusAfterSaveId = useRef<string | undefined>(undefined)
  const quicklinksStatusTimer = useRef<number | undefined>(undefined)
  const panelHostRef = useRef<HTMLDivElement | null>(null)
  const publicPluginDetailRef = useRef<HTMLElement | null>(null)
  const settingsTabsRef = useRef<HTMLDivElement>(null)
  const macPluginManagementRef = useRef<MacPluginManagementHandle | null>(null)
  const [macPluginStatus, setMacPluginStatus] = useState<MacPluginManagementStatus>({ message: '', tone: 'default' })
  const credentialSettingsRef = useRef<CredentialSettingsHandle | null>(null)
  const [credentialStatus, setCredentialStatus] = useState<CredentialSettingsStatus>({ message: '', tone: 'default', detail: false })
  const [settingsPinned, setSettingsPin] = useState(false)
  const [appliedPin, setAppliedPin] = useState(false)
  const [pinAttempt, setPinAttempt] = useState(0)
  const [pinPending, setPinPending] = useState(false)
  const [pinError, setPinError] = useState(false)
  const pinQueue = useRef(Promise.resolve())
  const pinSessionActive = useRef(false)
  const pinInvocation = useRef<string | undefined>(undefined)
  const sendSettingsPin = (active: boolean, pinned: boolean, invocationId?: string) =>
    core.capabilities.platform === 'macos'
      ? setSettingsPinned?.(active, pinned, invocationId)
      : setSettingsPinned?.(active, pinned)
  const hotkeyRecordingActive = useRef(false)
  const handleHotkeyRecordingPhase = useCallback((phase: HotkeyRecordingPhase) => {
    hotkeyRecordingActive.current = phase === 'recording'
    return core.setHotkeyRecordingPhase(phase)
  }, [core])
  useEffect(() => {
    if (snapshot.view !== 'settings' || !['windows', 'macos'].includes(core.capabilities.platform)) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (operationsMenu.current?.isOpen || hotkeyRecordingActive.current || !isWindowPinShortcut(event)) return
      event.preventDefault()
      event.stopPropagation()
      if (!event.repeat && !snapshot.hidePending && !pinPending) {
        if (pinError) setPinAttempt((attempt) => attempt + 1)
        else setSettingsPin((pinned) => !pinned)
      }
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [snapshot.view, snapshot.hidePending, pinPending, pinError])
  useEffect(() => { setSettingsPin(false) }, [snapshot.view, snapshot.viewEpoch])
  useEffect(() => {
    let current = true
    const active = snapshot.view === 'settings'
    if (!active && !pinSessionActive.current) return
    const invocationId = active ? snapshot.invocationId : pinInvocation.current
    pinInvocation.current = invocationId
    pinSessionActive.current = active
    setPinPending(true)
    setPinError(false)
    pinQueue.current = pinQueue.current
      .then(() => sendSettingsPin(active, active && settingsPinned, invocationId))
      .then(() => { if (current) { setAppliedPin(active && settingsPinned); setPinPending(false) } })
      .catch(() => { if (current) { setPinPending(false); setPinError(true) } })
    return () => { current = false }
  }, [snapshot.view, snapshot.viewEpoch, settingsPinned, pinAttempt])
  useEffect(() => () => {
    if (!pinSessionActive.current) return
    const invocationId = pinInvocation.current
    pinQueue.current = pinQueue.current.then(() => sendSettingsPin(false, false, invocationId)).catch(() => {})
  }, [])
  const activatedPluginEpoch = useRef<number | undefined>(undefined)
  const restoreSettingsTabFocus = useRef(false)
  const activeSettingsTab = snapshot.settingsTab
  const optionRefs = useRef(new Map<number, HTMLElement>())
  const actionsButtonRef = useRef<HTMLButtonElement | null>(null)
  const rerunButtonRef = useRef<HTMLButtonElement | null>(null)
  const actionFocusTarget = useRef<{
    invocationId: string | undefined
    viewEpoch: number
    querySequence: number
    value: string
    resultKey: number
    origin: HTMLElement | null
    moved: boolean
  } | undefined>(undefined)
  const [favoriteMenuVersion, setFavoriteMenuVersion] = useState(0)
  const favoriteFocusTarget = useRef<{
    invocationId: string | undefined
    viewEpoch: number
    queryControlValue: string
    target: ResultFavoriteTarget
    origin: HTMLElement | null
    moved: boolean
    menuOpen: boolean
  } | undefined>(undefined)
  const fileOptionRefs = useRef(new Map<number, HTMLElement>())
  const ready = useRef(false)

  useEffect(() => {
    const update = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    scheme.addEventListener('change', update)
    return () => scheme.removeEventListener('change', update)
  }, [scheme])

  useLayoutEffect(() => {
    document.documentElement.dataset.colorScheme = colorScheme
    return () => {
      delete document.documentElement.dataset.colorScheme
    }
  }, [colorScheme])

  useEffect(() => () => {
    detailSession.current++
    detailReload.current++
    detailReloadPending.current?.beforePublish.splice(0)
    detailReloadPending.current?.resolve()
    detailReloadPending.current = null
  }, [])

  useEffect(() => {
    if (snapshot.view !== 'settings') {
      detailSession.current++
      detailReload.current++
      detailReloadPending.current?.beforePublish.splice(0)
      detailReloadPending.current?.resolve()
      detailReloadPending.current = null
      setPublicPluginDetail(null)
      setBuiltinFeatureDetail(null)
    }
  }, [snapshot.view])

  const selectedPublicPluginId = selectedPublicPlugin?.pluginId
  useLayoutEffect(() => {
    if (snapshot.view !== 'settings' || (!selectedPublicPluginId && !builtinFeatureDetail)) return
    publicPluginDetailRef.current?.focus()
  }, [selectedPublicPluginId, builtinFeatureDetail, snapshot.view])

  useLayoutEffect(() => {
    if (!restoreSettingsTabFocus.current) return
    if (snapshot.view !== 'settings' || selectedPublicPlugin || builtinFeatureDetail) return
    restoreSettingsTabFocus.current = false
    settingsTabsRef.current
      ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
      ?.focus()
  }, [activeSettingsTab, selectedPublicPlugin, builtinFeatureDetail, snapshot.view])

  const reportReady = useCallback(() => {
    if (ready.current) return
    ready.current = true
    onReady('ready')
  }, [onReady])
  const reportFailed = useCallback(() => {
    if (ready.current) return
    ready.current = true
    onReady('failed')
  }, [onReady])
  const reportQueryBound = useCallback((input: HTMLInputElement) => {
    queryRef.current = input
    if (snapshot.view === 'launcher' && snapshot.invocationId) queryRef.current?.focus()
    reportReady()
  }, [reportReady, snapshot.invocationId, snapshot.queryControl, snapshot.view])

  const mainResultCommand = snapshot.mainResultCommand
  const reportMainResultBound = useCallback((input: HTMLInputElement) => {
    mainResultInputRef.current = input
    input.focus()
    const caret = input.value.length
    input.setSelectionRange(caret, caret)
  }, [mainResultCommand?.commandId, mainResultCommand?.commandLabel, mainResultCommand?.suffixControl])
  const reportMainResultUnbound = useCallback((input: HTMLInputElement) => {
    if (mainResultInputRef.current === input) mainResultInputRef.current = null
  }, [])

  const panel = snapshot.panel
  const quicklinks = snapshot.quicklinks
  const quicklinksCreateOpen = quicklinks?.createDraft !== undefined
  const panelActive = Boolean(panel || quicklinks)
  const clearQuicklinksStatus = useCallback(() => {
    if (quicklinksStatusTimer.current !== undefined) {
      window.clearTimeout(quicklinksStatusTimer.current)
      quicklinksStatusTimer.current = undefined
    }
    setQuicklinksStatus(null)
  }, [])
  const showQuicklinksStatus = useCallback((message: string, tone: QuicklinksUiStatus['tone'] = 'success') => {
    if (quicklinksStatusTimer.current !== undefined) {
      window.clearTimeout(quicklinksStatusTimer.current)
      quicklinksStatusTimer.current = undefined
    }
    setQuicklinksStatus({ message, tone })
    quicklinksStatusTimer.current = window.setTimeout(() => {
      quicklinksStatusTimer.current = undefined
      setQuicklinksStatus(null)
    }, QUICKLINKS_STATUS_DURATION_MS)
  }, [])
  const reportPanelBound = useCallback((input: HTMLInputElement) => {
    panelInputRef.current = input
    input.focus()
    const caret = input.value.length
    input.setSelectionRange(caret, caret)
  }, [panel?.sessionEpoch, panel?.suffixControl])
  const reportPanelUnbound = useCallback((input: HTMLInputElement) => {
    if (panelInputRef.current === input) panelInputRef.current = null
  }, [])
  const reportQuicklinksBound = useCallback((input: HTMLInputElement) => {
    quicklinksInputRef.current = input
    queryRef.current = input
    input.focus()
    const caret = input.value.length
    input.setSelectionRange(caret, caret)
    reportReady()
  }, [reportReady, snapshot.queryControl])
  const reportQuicklinksUnbound = useCallback((input: HTMLInputElement) => {
    if (quicklinksInputRef.current === input) quicklinksInputRef.current = null
    if (queryRef.current === input) queryRef.current = null
  }, [])
  const focusQuicklinksInput = useCallback((): boolean => {
    const input = quicklinksInputRef.current
    if (!input?.isConnected || input.disabled) return false
    input.focus()
    const caret = input.value.length
    input.setSelectionRange(caret, caret)
    return document.activeElement === input
  }, [])
  const focusQuicklinkItem = useCallback((id?: string): boolean => {
    const snapshot = core.getSnapshot()
    const targetId = id ?? snapshot.quicklinks?.selectedId ?? snapshot.quicklinks?.items[0]?.id
    if (!targetId) return false
    const target = quicklinkItemRefs.current.get(targetId)
    if (!target?.isConnected || target.disabled) return false
    target.focus()
    return document.activeElement === target
  }, [core])
  const focusQuicklinksEditor = useCallback((): boolean => {
    const target = quicklinksEditorRef.current
      ?.querySelector<HTMLElement>(QUICKLINK_EDITOR_FOCUSABLE_SELECTOR)
    if (!target) return false
    target.focus()
    return document.activeElement === target
  }, [])
  const focusQuicklinksEditorByTab = useCallback((
    origin: HTMLElement | null,
    direction: 'previous' | 'next',
  ): boolean => {
    const editor = quicklinksEditorRef.current
    if (!editor) return false
    const controls = Array.from(
      editor.querySelectorAll<HTMLElement>(QUICKLINK_EDITOR_FOCUSABLE_SELECTOR),
    ).filter((element) => element.tabIndex >= 0)
    if (controls.length === 0) return false
    const originControl = origin?.closest<HTMLElement>(QUICKLINK_EDITOR_FOCUSABLE_SELECTOR) ?? null
    const originIndex = originControl ? controls.indexOf(originControl) : -1
    const targetIndex = originIndex < 0
      ? direction === 'next' ? 0 : controls.length - 1
      : (originIndex + (direction === 'next' ? 1 : -1) + controls.length) % controls.length
    const target = controls[targetIndex]
    if (!target) return false
    target.focus()
    return document.activeElement === target
  }, [])
  const saveQuicklinkWithStatus = useCallback(async (focusAfterSaveId?: string) => {
    if (focusAfterSaveId) quicklinkFocusAfterSaveId.current = focusAfterSaveId
    await core.saveQuicklink()
    const latestQuicklinks = core.getSnapshot().quicklinks
    if (!latestQuicklinks) return
    if (latestQuicklinks.error) {
      showQuicklinksStatus('保存失败', 'error')
      return
    }
    showQuicklinksStatus('已保存')
  }, [core, showQuicklinksStatus])

  useLayoutEffect(() => {
    if (snapshot.view !== 'launcher' || !quicklinks || quicklinks.operation) return
    const focusAfterSaveId = quicklinkFocusAfterSaveId.current
    if (focusAfterSaveId) {
      quicklinkFocusAfterSaveId.current = undefined
      if (focusQuicklinkItem(focusAfterSaveId)) return
    }
    focusQuicklinksInput()
  }, [focusQuicklinkItem, focusQuicklinksInput, quicklinks?.operation, snapshot.queryControl, snapshot.view])

  useLayoutEffect(() => {
    const dialog = quicklinkCreateDialogRef.current
    if (!dialog) return
    if (quicklinksCreateOpen) {
      if (!dialog.open) {
        const showAsTopLayer = (dialog as HTMLDialogElement & Record<string, unknown>)['show' + '\u004dodal']
        if (typeof showAsTopLayer === 'function') showAsTopLayer.call(dialog)
        else dialog.setAttribute('open', '')
      }
      return
    }
    if (!dialog.open) return
    if (typeof dialog.close === 'function') dialog.close()
    else dialog.removeAttribute('open')
  }, [quicklinksCreateOpen])

  useEffect(() => {
    return () => {
      if (quicklinksStatusTimer.current !== undefined) {
        window.clearTimeout(quicklinksStatusTimer.current)
        quicklinksStatusTimer.current = undefined
      }
    }
  }, [])

  useEffect(() => {
    if (quicklinks !== undefined) return
    clearQuicklinksStatus()
  }, [clearQuicklinksStatus, quicklinks])

  useLayoutEffect(() => {
    if (!panel?.focusRequestId || panel.closePending) return
    const input = panelInputRef.current
    let focused = false
    if (input?.isConnected) {
      input.focus()
      focused = document.activeElement === input
    }
    core.settlePanelHostInputFocus({
      sessionEpoch: panel.sessionEpoch,
      focusRequestId: panel.focusRequestId,
      focused,
    })
  }, [core, panel?.closePending, panel?.focusRequestId, panel?.sessionEpoch, panel?.suffixControl])

  useLayoutEffect(() => {
    if (snapshot.view !== 'launcher' || !panel || panel.closePending) return
    const host = panelHostRef.current
    if (!host) return
    let disposed = false
    let frame: number | undefined
    let lastBounds: PluginPanelBounds | undefined

    const measure = () => {
      frame = undefined
      if (disposed || !host.isConnected) return
      const rect = host.getBoundingClientRect()
      // DOM coordinates include browser zoom; native child bounds use physical pixels.
      const pixelRatio = window.devicePixelRatio
      if (!Number.isFinite(pixelRatio) || pixelRatio <= 0) return
      const bounds: PluginPanelBounds = {
        x: rect.left * pixelRatio,
        y: rect.top * pixelRatio,
        width: rect.width * pixelRatio,
        height: rect.height * pixelRatio,
      }
      if (
        !Number.isFinite(bounds.x) || !Number.isFinite(bounds.y) ||
        !Number.isFinite(bounds.width) || !Number.isFinite(bounds.height) ||
        bounds.width <= 0 || bounds.height <= 0
      ) return
      if (
        lastBounds?.x === bounds.x && lastBounds.y === bounds.y &&
        lastBounds.width === bounds.width && lastBounds.height === bounds.height
      ) return
      lastBounds = bounds
      core.setPanelBounds({ sessionEpoch: panel.sessionEpoch, bounds })
    }
    const schedule = () => {
      if (disposed || frame !== undefined) return
      frame = window.requestAnimationFrame(measure)
    }
    const observer = new ResizeObserver(schedule)
    observer.observe(host)
    window.addEventListener('resize', schedule)
    schedule()
    return () => {
      disposed = true
      observer.disconnect()
      window.removeEventListener('resize', schedule)
      if (frame !== undefined) window.cancelAnimationFrame(frame)
    }
  }, [core, panel?.closePending, panel?.sessionEpoch, snapshot.view])

  useLayoutEffect(() => {
    if (!snapshot.invocationId) return
    if (snapshot.view === 'launcher') {
      const input = mainResultInputRef.current ?? queryRef.current
      input?.focus()
      input?.select()
    } else {
      settingsTabsRef.current
        ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
        ?.focus()
    }
  }, [snapshot.invocationId, snapshot.view, snapshot.viewEpoch])

  useLayoutEffect(() => {
    const selected = snapshot.results[snapshot.selectedIndex]
    if (snapshot.view !== 'launcher' || !selected) return
    const option = optionRefs.current.get(selected.key)
    option?.scrollIntoView({ block: 'nearest' })
    const section = option?.closest('.result-section')
    if (section && section.querySelector('[role="option"]') === option) {
      section.querySelector<HTMLElement>('.result-section-title')?.scrollIntoView({ block: 'nearest' })
    }
  }, [snapshot.results, snapshot.selectedIndex, snapshot.view])

  useEffect(() => {
    const onFocus = (event: FocusEvent) => {
      const actionTarget = actionFocusTarget.current
      if (actionTarget && event.target instanceof HTMLElement && event.target !== document.body &&
          event.target !== actionTarget.origin && !event.target.closest('[role="menu"]')) actionTarget.moved = true
      const pending = favoriteFocusTarget.current
      const element = event.target
      if (!pending || !(element instanceof HTMLElement) || element === document.body || element === pending.origin || element.closest('[role="menu"]')) return
      pending.moved = true
    }
    document.addEventListener('focusin', onFocus)
    return () => document.removeEventListener('focusin', onFocus)
  }, [])

  useLayoutEffect(() => {
    const target = actionFocusTarget.current
    if (!target) return
    if (target.moved || snapshot.view !== 'launcher' || target.invocationId !== snapshot.invocationId ||
        target.viewEpoch !== snapshot.viewEpoch || target.querySequence !== snapshot.querySequence ||
        target.value !== snapshot.queryControlValue || !snapshot.results.some((row) => row.key === target.resultKey)) {
      actionFocusTarget.current = undefined
      return
    }
    if (snapshot.resultActionMenu) return
    const frame = window.requestAnimationFrame(() => {
      if (actionFocusTarget.current !== target || target.moved) return
      actionFocusTarget.current = undefined
      const origin = target.origin?.isConnected && !target.origin.hasAttribute('disabled') ? target.origin : null
      const fallback = mainResultInputRef.current ?? queryRef.current
      if (origin && origin !== document.body) origin.focus()
      else fallback?.focus()
    })
    return () => window.cancelAnimationFrame(frame)
  }, [snapshot.resultActionMenu, snapshot.invocationId, snapshot.viewEpoch, snapshot.querySequence, snapshot.queryControlValue, snapshot.results, snapshot.view])

  useLayoutEffect(() => {
    const target = favoriteFocusTarget.current
    if (!target) return
    if (
      target.moved || snapshot.view !== 'launcher' ||
      snapshot.invocationId !== target.invocationId ||
      snapshot.viewEpoch !== target.viewEpoch ||
      snapshot.queryControlValue !== target.queryControlValue
    ) {
      favoriteFocusTarget.current = undefined
      return
    }
    if (target.menuOpen || snapshot.favoriteMutationPending || snapshot.searchPending) return
    const result = snapshot.results.find((item) => item.favorite && sameFavoriteTarget(item.favorite.target, target.target)) ?? snapshot.results[0]
    const frame = window.requestAnimationFrame(() => {
      if (favoriteFocusTarget.current !== target || target.moved) return
      const option = result ? optionRefs.current.get(result.key) : queryRef.current
      if (!option?.isConnected) return
      favoriteFocusTarget.current = undefined
      option.focus()
    })
    return () => window.cancelAnimationFrame(frame)
  }, [
    favoriteMenuVersion,
    snapshot.favoriteMutationPending,
    snapshot.invocationId,
    snapshot.queryControlValue,
    snapshot.results,
    snapshot.searchPending,
    snapshot.view,
    snapshot.viewEpoch,
  ])

  const file = snapshot.file
  const activeFileIndex =
    file?.selected === undefined ? -1 : file.results.findIndex((item) => item.fullPath === file.selected?.fullPath)

  useLayoutEffect(() => {
    if (snapshot.view === 'launcher' && file && activeFileIndex >= 0) {
      const selected = fileOptionRefs.current.get(activeFileIndex)
      scrollFileResultIntoView(document.getElementById('file-results'), selected)
    }
  }, [activeFileIndex, file, snapshot.view])

  const statusResult = snapshot.results[snapshot.selectedIndex]
  const launcherResultsActive = snapshot.view === 'launcher' && !file && !panelActive
  const resultOperation = launcherResultsActive && statusResult && !snapshot.resultActionRerun
    ? statusResult.actions
      ? statusResult.defaultActionTitle ?? (statusResult.actions.length ? '选择动作' : '')
      : statusResult.hasDefaultAction === false ? '' : snapshot.mainResultCommand ? '复制' : '执行'
    : ''
  const macPluginFeedback = snapshot.view === 'settings' && activeSettingsTab === 'plugins' && core.capabilities.pluginManagement && !core.capabilities.plugins ? macPluginStatus : undefined
  const status =
    operationsError ||
    macPluginFeedback?.message ||
    (snapshot.view === 'settings' && activeSettingsTab === 'credentials'
      ? credentialStatus.message : '') ||
    snapshot.shownNotice ||
    snapshot.status ||
    (panel ? panel.feedback?.status.message : '') ||
    (snapshot.view === 'launcher' && !panelActive && snapshot.results.length
      ? `${snapshot.results.length} 个结果`
      : '')
  const messageBadgeCount = snapshot.messageCenter.status === 'unavailable'
    ? '!'
    : snapshot.messageCenter.status === 'ready'
      ? snapshot.messageCenter.unreadCount ?? 0
      : 0
  const captureActionFocus = (index = snapshot.selectedIndex) => {
    const row = snapshot.results[index]
    if (!row) return
    actionFocusTarget.current = {
      invocationId: snapshot.invocationId, viewEpoch: snapshot.viewEpoch, querySequence: snapshot.querySequence,
      value: snapshot.queryControlValue, resultKey: row.key,
      origin: document.activeElement instanceof HTMLElement ? document.activeElement : null, moved: false,
    }
  }
  const openActions = () => { captureActionFocus(); core.openResultActionMenu() }
  const openOperations = () => {
    if (panel) {
      const ticket = window.crypto.randomUUID()
      panelMenuTicket.current = { ticket, sessionEpoch: panel.sessionEpoch, suffix: panel.suffix }
      if (!core.client.openPluginPanelActions) { setOperationsError('操作面板暂不可用'); return }
      void core.client.openPluginPanelActions({ sessionEpoch: panel.sessionEpoch, source: panelActionSource.current, ticket })
        .catch(() => setOperationsError('操作面板暂不可用'))
      return
    }
    const menu = operationsMenu.current ??= createContextActionsMenu(document, () => setOperationsError('操作未完成，请重试'))
    if (menu.isOpen) { menu.close(); return }
    core.closeResultActionMenu()
    const context: ContextAction[] = []
    const global: ContextAction[] = []
    const owner = { invocationId: snapshot.invocationId, viewEpoch: snapshot.viewEpoch, querySequence: snapshot.querySequence, resultKey: statusResult?.key }
    const owned = (run: () => void | Promise<void>) => () => {
      const now = core.getSnapshot()
      if (now.invocationId === owner.invocationId && now.viewEpoch === owner.viewEpoch && now.querySequence === owner.querySequence && now.results[now.selectedIndex]?.key === owner.resultKey) return run()
    }
    if (launcherResultsActive && statusResult) {
      if (statusResult.actions) statusResult.actions.forEach((action, index) => context.push({ id: `result-${index}`, title: action.title,
        shortcut: action.actionId === statusResult.defaultActionId ? 'Enter' : undefined,
        icon: /复制/.test(action.title) ? 'copy' : 'arrow', disabledReason: snapshot.resultActionsDisabled ? '结果暂不可用，请重新运行' : undefined,
        run: owned(() => core.executeResultAction(snapshot.selectedIndex, action.actionId)) }))
      else if (statusResult.hasDefaultAction !== false) context.push({ id: 'result-default', title: resultOperation || '执行', shortcut: 'Enter', icon: snapshot.mainResultCommand ? 'copy' : 'arrow', run: owned(() => core.activateResult(snapshot.selectedIndex)) })
      if (snapshot.resultActionRerun) context.push({ id: 'rerun', title: '重新运行', icon: 'arrow', run: owned(core.rerunResultCommand) })
    }
    if (quicklinks) {
      global.push({ id: 'new-link', title: '新建快捷链接', shortcut: 'Ctrl+N', icon: 'plus', run: core.newQuicklink })
      if (quicklinksDraftActive) context.push({ id: 'save-link', title: '保存快捷链接', icon: 'save', run: () => core.saveQuicklink() })
    }
    if (file) context.push({ id: 'open-file', title: '打开选中项', shortcut: 'Enter', icon: 'arrow', disabledReason: file.selected ? undefined : '没有选中项', run: () => core.keyDown('Enter', false) })
    global.push({ id: 'settings', title: snapshot.view === 'settings' ? '返回启动器' : '打开设置', icon: 'settings', run: () => { const navigate = () => core.navigate(snapshot.view === 'settings' ? 'launcher' : 'settings'); if (snapshot.view === 'settings' && activeSettingsTab === 'plugins' && macPluginManagementRef.current) macPluginManagementRef.current.requestLeave(navigate); else navigate() } },
      { id: 'hide', title: '隐藏窗口', shortcut: 'Esc', icon: 'close', run: () => { if (snapshot.view === 'settings' && activeSettingsTab === 'plugins' && macPluginManagementRef.current) macPluginManagementRef.current.requestLeave(core.requestHide); else core.requestHide() } })
    setOperationsOpen(true)
    menu.open({ context, global }, { bottom: 44, onClose: () => setOperationsOpen(false) })
  }
  const settingsControl = (
    <Tooltip title="设置">
      <span className="launcher-settings-control">
        <Badge className={`launcher-settings-badge${snapshot.messageCenter.status === 'unavailable' ? ' is-unavailable' : ''}`}
          count={messageBadgeCount} offset={[-2, 2]} overflowCount={99} size="small">
          <Button
            aria-label="打开设置" className="launcher-settings-button" disabled={!snapshot.invocationId}
            icon={<Settings aria-hidden size={16} strokeWidth={1.8} />}
            onMouseDown={(event) => event.preventDefault()} onClick={() => core.navigate('settings')} size="small" type="text" />
        </Badge>
      </span>
    </Tooltip>
  )
  const indexedLauncherResults = snapshot.results.map((item, index) => ({ item, index }))
  const hasLauncherQuery = snapshot.queryControlValue !== ''
  const isApplicationResult = ({ item }: (typeof indexedLauncherResults)[number]) =>
    item.favorite === undefined && item.iconKind === undefined && item.pluginIconUrl === undefined
  const launcherResultSections = snapshot.mainResultCommand
    ? undefined
    : [
        ...(hasLauncherQuery
          ? [{
              key: 'applications',
              title: '应用',
              results: indexedLauncherResults.filter(isApplicationResult),
            }]
          : []),
        {
          key: 'favorites',
          title: '常用',
          results: indexedLauncherResults.filter(({ item }) => item.favorite?.favorite === true),
        },
        {
          key: 'all',
          title: '所有功能',
          results: indexedLauncherResults.filter((result) =>
            result.item.favorite?.favorite !== true && (!hasLauncherQuery || !isApplicationResult(result))),
        },
      ].filter(({ results }) => results.length > 0)

  const queryKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (
      event.key === 'Tab' &&
      snapshot.file &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey
    ) {
      if (composing(event)) return
      event.preventDefault()
      core.cycleFileCategory(event.shiftKey ? 'previous' : 'next')
      return
    }
    if (event.altKey && event.key.toLowerCase() === 'p') {
      if (composing(event) || !file) return
      event.preventDefault()
      core.setFilePreviewEnabled(!file.previewEnabled)
      return
    }
    if (!['ArrowUp', 'ArrowDown', 'Enter', 'Escape'].includes(event.key)) return
    const isComposing = composing(event)
    if (!isComposing) event.preventDefault()
    if (event.key === 'Enter' && !isComposing && statusResult?.actions && !statusResult.hasDefaultAction) captureActionFocus()
    core.keyDown(event.key as 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing)
  }
  const launcherTabKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (hotkeyRecordingActive.current || event.target instanceof Element && event.target.closest('dialog, [role="dialog"]')) return
    if (event.key.toLowerCase() === 'k' && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && !composing(event)) {
      event.preventDefault()
      event.stopPropagation()
      if (!event.repeat) openOperations()
      return
    }
    if (!launcherResultsActive) return
    if (
      event.key !== 'Tab' ||
      event.ctrlKey ||
      event.altKey ||
      event.metaKey ||
      composing(event)
    ) return

    const input = mainResultInputRef.current ?? queryRef.current
    if (!input || input.disabled) return
    const settingsButton = input.closest('.launcher-surface')?.querySelector<HTMLElement>('.launcher-settings-button') ?? null
    const controls = [input, actionsButtonRef.current, rerunButtonRef.current, settingsButton]
      .filter((element): element is HTMLElement => element !== null && !element.hasAttribute('disabled') && element.getAttribute('aria-disabled') !== 'true')
    event.preventDefault()
    event.stopPropagation()
    const origin = actionFocusTarget.current?.origin
    const active = controls.includes(document.activeElement as HTMLElement) ? document.activeElement : origin
    const index = controls.indexOf(active as HTMLElement)
    actionFocusTarget.current = undefined
    core.closeResultActionMenu()
    controls[(index + (event.shiftKey ? -1 : 1) + controls.length) % controls.length]?.focus()
  }
  const mainResultKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Backspace' && !composing(event)) {
      const input = event.currentTarget
      if (input.selectionStart === 0 && input.selectionEnd === 0) {
        event.preventDefault()
        core.closeMainResultCommand()
      }
      return
    }
    queryKeyDown(event)
  }
  const panelKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.defaultPrevented) return
    const isComposing = composing(event)
    if (event.key === 'Backspace' && !isComposing) {
      const input = event.currentTarget
      if (input.selectionStart === 0 && input.selectionEnd === 0) {
        event.preventDefault()
        void core.closePanel()
      }
      return
    }
    if (core.routePanelHostKey({
      repeat: event.repeat,
      key: event.key,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      shiftKey: event.shiftKey,
      altKey: event.altKey,
      isComposing,
      platform: navigator.platform.toLowerCase().includes('mac') ? 'macos' : 'windows',
    })) {
      event.preventDefault()
      return
    }
    if (event.key !== 'Enter' && event.key !== 'Escape') return
    if (!isComposing) event.preventDefault()
    core.keyDown(event.key, isComposing)
  }
  const quicklinksKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    const isComposing = composing(event)
    const target = event.target
    const targetElement = target instanceof HTMLElement ? target : null
    const fromFilterInput =
      target instanceof HTMLInputElement &&
      target.getAttribute('aria-label') === '搜索快速链接目录'
    const fromQuicklinkItem = targetElement?.closest<HTMLButtonElement>('.quicklink-item') ?? null
    const fromQuicklinksEditor = targetElement?.closest('.quicklinks-editor') !== null
    const primaryModifier = event.ctrlKey || event.metaKey
    if (
      event.key === 'Tab' &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !isComposing
    ) {
      event.preventDefault()
      event.stopPropagation()
      if (fromQuicklinksEditor) {
        focusQuicklinksEditorByTab(targetElement, event.shiftKey ? 'previous' : 'next')
      }
      return
    }
    if (
      primaryModifier &&
      !event.altKey &&
      !event.shiftKey &&
      !isComposing &&
      event.key.toLowerCase() === 'f'
    ) {
      event.preventDefault()
      event.stopPropagation()
      focusQuicklinksInput()
      return
    }
    if (
      primaryModifier &&
      !event.altKey &&
      !event.shiftKey &&
      !isComposing &&
      event.key.toLowerCase() === 's' &&
      fromQuicklinksEditor
    ) {
      event.preventDefault()
      event.stopPropagation()
      if (quicklinksCanSave && quicklinks?.selectedId) {
        void saveQuicklinkWithStatus(quicklinks.selectedId)
      }
      return
    }
    if (
      event.key.toLowerCase() === 'n' &&
      primaryModifier &&
      !event.altKey &&
      !event.shiftKey &&
      !isComposing
    ) {
      event.preventDefault()
      event.stopPropagation()
      core.newQuicklink()
      return
    }
    if (
      event.key === 'ArrowDown' &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.shiftKey &&
      !isComposing &&
      fromFilterInput
    ) {
      event.preventDefault()
      event.stopPropagation()
      focusQuicklinkSelectionFromInput()
      return
    }
    if (
      (event.key === 'ArrowDown' || event.key === 'ArrowUp') &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.shiftKey &&
      !isComposing &&
      fromQuicklinkItem
    ) {
      if (moveQuicklinkSelection(event.key === 'ArrowDown' ? 'next' : 'previous', fromQuicklinkItem.dataset.quicklinkId)) {
        event.preventDefault()
        event.stopPropagation()
        return
      }
    }
    if (
      event.key === 'ArrowRight' &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.shiftKey &&
      !isComposing &&
      fromQuicklinkItem
    ) {
      event.preventDefault()
      event.stopPropagation()
      if (fromQuicklinkItem.dataset.quicklinkId) core.selectQuicklink(fromQuicklinkItem.dataset.quicklinkId)
      focusQuicklinksEditor()
      return
    }
    if (event.key === 'Backspace' && !isComposing) {
      if (event.currentTarget instanceof HTMLInputElement) {
        const input = event.currentTarget
        if (input.selectionStart === 0 && input.selectionEnd === 0) {
          event.preventDefault()
          event.stopPropagation()
          core.closeQuicklinks()
        }
      }
      return
    }
    if (event.key !== 'Escape' || isComposing) return
    event.preventDefault()
    event.stopPropagation()
    core.keyDown('Escape', false)
  }
  const focusQuicklinkSelectionFromInput = (): boolean => {
    if (!quicklinks || quicklinks.operation !== undefined || quicklinks.items.length === 0) return false
    const selectedVisible = quicklinks.selectedId !== undefined &&
      quicklinks.items.some((item) => item.id === quicklinks.selectedId)
    const targetId = selectedVisible ? quicklinks.selectedId : quicklinks.items[0]?.id
    if (!targetId) return false
    if (!selectedVisible) core.selectQuicklink(targetId)
    return focusQuicklinkItem(targetId)
  }
  const moveQuicklinkSelection = (
    direction: 'previous' | 'next',
    sourceId?: string,
  ): boolean => {
    if (!quicklinks || quicklinks.operation !== undefined || quicklinks.items.length === 0) return false
    const sourceIndex = sourceId === undefined
      ? -1
      : quicklinks.items.findIndex((item) => item.id === sourceId)
    const selectedIndex = quicklinks.selectedId === undefined
      ? -1
      : quicklinks.items.findIndex((item) => item.id === quicklinks.selectedId)
    const currentIndex = sourceIndex >= 0 ? sourceIndex : selectedIndex
    const targetIndex = currentIndex >= 0
      ? Math.max(0, Math.min(quicklinks.items.length - 1, currentIndex + (direction === 'next' ? 1 : -1)))
      : direction === 'next'
        ? 0
        : quicklinks.items.length - 1
    const target = quicklinks.items[targetIndex]
    if (!target) return false
    if (target.id !== quicklinks.selectedId) core.selectQuicklink(target.id)
    focusQuicklinkItem(target.id)
    return true
  }
  const settingsKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    const isComposing = composing(event)
    const primaryModifier = event.ctrlKey || event.metaKey
    if (
      activeSettingsTab === 'plugins' &&
      primaryModifier &&
      !event.altKey &&
      !event.shiftKey &&
      !isComposing &&
      event.key.toLowerCase() === 'f'
    ) {
      const input = settingsTabsRef.current
        ?.querySelector<HTMLInputElement>('input[aria-label="搜索插件名称或启动命令"]')
      if (!input?.isConnected || input.disabled) return
      event.preventDefault()
      event.stopPropagation()
      input.focus()
      const caret = input.value.length
      input.setSelectionRange(caret, caret)
      return
    }
    if (event.key === 'Escape') {
      if (!isComposing) event.preventDefault()
      if (!isComposing && activeSettingsTab === 'plugins' && macPluginManagementRef.current?.back()) return
      if (!isComposing && activeSettingsTab === 'credentials' && credentialSettingsRef.current?.back()) return
      core.keyDown('Escape', isComposing)
      return
    }
    if (
      isComposing ||
      event.ctrlKey ||
      event.altKey ||
      event.metaKey ||
      event.shiftKey ||
      (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft')
    ) {
      return
    }
    const target = event.target
    if (!(target instanceof HTMLElement)) return
    const selectedTab = settingsTabsRef.current
      ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
    if (!selectedTab) return
    const panelId = selectedTab.getAttribute('aria-controls')
    const panel = panelId
      ? [...(settingsTabsRef.current?.querySelectorAll<HTMLElement>('[role="tabpanel"]') ?? [])]
        .find((candidate) => candidate.id === panelId) ?? null
      : null
    if (!panel) return

    if (event.key === 'ArrowRight') {
      if (target !== selectedTab) return
      const firstControl = panel.querySelector<HTMLElement>([
        'button:not([disabled])',
        'input:not([disabled]):not([type="hidden"])',
        'select:not([disabled])',
        'textarea:not([disabled])',
        'a[href]',
        '[tabindex]:not([tabindex="-1"])',
      ].join(','))
      if (!firstControl || firstControl.getAttribute('aria-disabled') === 'true') return
      event.preventDefault()
      firstControl.focus()
      return
    }

    if (!panel.contains(target)) return
    event.preventDefault()
    selectedTab.focus()
  }

  const renderLauncherResult = (item: (typeof snapshot.results)[number], index: number) => {
    const resultFavorite = item.favorite
    const hasActions = Boolean(item.actions?.length)
    const row = (
      <div
        key={item.key}
        id={`launcher-result-${item.key}`}
        role="option"
        aria-selected={snapshot.selectedIndex === index}
        className={snapshot.selectedIndex === index ? 'result-row is-selected' : 'result-row'}
        tabIndex={-1}
        onClick={() => {
          if (hasActions && !item.hasDefaultAction) captureActionFocus(index)
          core.activateResult(index)
        }}
        onFocus={() => core.selectResult(index)}
        onContextMenu={resultFavorite || hasActions ? (event) => event.preventDefault() : undefined}
        ref={(element) => {
          if (element) optionRefs.current.set(item.key, element)
          else optionRefs.current.delete(item.key)
        }}
      >
        <span className="result-icon" aria-hidden="true">
          {item.iconKind ? (
            <BuiltInResultIcon kind={item.iconKind} />
          ) : item.pluginIconUrl ? (
            <PluginIcon iconUrl={item.pluginIconUrl} size={28} />
          ) : (
            <>
              <span className="app-mark" hidden={item.icon !== undefined} />
              {item.icon ? (
                <img
                  className="result-icon-image"
                  src={item.icon}
                  alt=""
                  aria-hidden="true"
                  draggable={false}
                  onError={(event) => {
                    event.currentTarget.hidden = true
                    const fallback = event.currentTarget.previousElementSibling
                    if (fallback instanceof HTMLElement) fallback.hidden = false
                  }}
                />
              ) : null}
            </>
          )}
        </span>
        <span className="result-copy">
          <span className="result-title-line">
            <span className="result-title">{item.title}</span>
            {resultFavorite?.favorite ? (
              <Star
                aria-label="常用"
                className="result-favorite-star"
                fill="currentColor"
                size={14}
                strokeWidth={1.8}
              />
            ) : null}
          </span>
          {item.subtitle ? <span className="result-subtitle" title={item.subtitle}>{item.subtitle}</span> : null}
          {item.detail ? <span className="result-detail">{item.detail}</span> : null}
        </span>
      </div>
    )
    if (!resultFavorite && !hasActions) return row
    return (
      <Dropdown
        key={item.key}
        trigger={['contextMenu']}
        open={hasActions ? snapshot.resultActionMenu?.resultKey === item.key : undefined}
        autoFocus={hasActions}
        menu={{
          className: hasActions ? 'launcher-result-action-menu' : undefined,
          'aria-label': hasActions ? '结果动作' : '结果菜单',
          items: [
            ...(item.actions ?? []).map((action, actionIndex) => ({ key: `action-${actionIndex}`, label: action.title, disabled: snapshot.resultActionsDisabled })),
            ...(hasActions && resultFavorite ? [{ key: 'separator', type: 'divider' as const }] : []),
            ...(resultFavorite ? [{
              key: 'favorite',
              label: resultFavorite.favorite ? '取消常用' : '设为常用',
              disabled: snapshot.favoriteMutationPending,
            }] : []),
          ],
          onKeyDownCapture: (event) => {
            if (composing(event)) return
            if (hasActions && ['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter'].includes(event.key)) {
              event.preventDefault()
              event.stopPropagation()
              const items = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])')]
              if (!items.length) return
              const current = items.indexOf(document.activeElement as HTMLElement)
              if (event.key === 'Enter') { if (current >= 0) items[current]?.click(); return }
              const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
                : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
              items[next]?.focus()
              return
            }
            if (event.key === 'Escape' && hasActions) {
              event.preventDefault()
              event.stopPropagation()
              core.closeResultActionMenu()
            } else if (event.key === 'Tab' && hasActions) {
              event.stopPropagation()
              launcherTabKeyDown(event)
            }
          },
          onClick: ({ key, domEvent }) => {
            domEvent.stopPropagation()
            if (key.startsWith('action-')) {
              const action = item.actions?.[Number(key.slice('action-'.length))]
              if (action) core.executeResultAction(index, action.actionId)
              return
            }
            if (!resultFavorite) return
            favoriteFocusTarget.current = {
              invocationId: snapshot.invocationId,
              viewEpoch: snapshot.viewEpoch,
              queryControlValue: snapshot.queryControlValue,
              target: resultFavorite.target,
              origin: optionRefs.current.get(item.key) ?? null,
              moved: false,
              menuOpen: false,
            }
            core.setPluginFavorite(index, !resultFavorite.favorite)
          },
        }}
        onOpenChange={(open) => {
          if (hasActions) {
            if (open) {
              captureActionFocus(index)
              core.openResultActionMenu(index)
            } else core.closeResultActionMenu()
            return
          }
          if (!resultFavorite) return
          if (open) {
            favoriteFocusTarget.current = {
              invocationId: snapshot.invocationId, viewEpoch: snapshot.viewEpoch,
              queryControlValue: snapshot.queryControlValue, target: resultFavorite.target,
              origin: optionRefs.current.get(item.key) ?? null, moved: false, menuOpen: true,
            }
            core.openPluginContextMenu(index)
          } else {
            const pending = favoriteFocusTarget.current
            if (pending && sameFavoriteTarget(pending.target, resultFavorite.target)) pending.menuOpen = false
            setFavoriteMenuVersion((version) => version + 1)
            core.closePluginContextMenu()
          }
        }}
      >
        {row}
      </Dropdown>
    )
  }

  const launcher = (
    <section className="launcher-view" aria-label="应用启动器">
      <div className="launcher-query-region">
        {mainResultCommand ? (
          <div className="panel-input-row panel-input-shell main-result-input-shell">
            <CommandTag
              className="main-result-command-tag"
              commandLabel={mainResultCommand.commandLabel}
              exitLabel={`退出 ${mainResultCommand.commandLabel} 命令`}
              exitTitle="退出命令"
              onClose={core.closeMainResultCommand}
            />
            <label
              className="visually-hidden"
              htmlFor={`launcher-main-result-suffix-${mainResultCommand.suffixControl}`}
            >
              {mainResultCommand.commandLabel} argument
            </label>
            <BoundInput
              className="panel-suffix-input"
              core={core}
              control={mainResultCommand.suffixControl}
              value={mainResultCommand.suffix}
              id={`launcher-main-result-suffix-${mainResultCommand.suffixControl}`}
              name={`launcher-main-result-suffix-${mainResultCommand.suffixControl}`}
              aria-label={`${mainResultCommand.commandLabel} argument`}
              aria-autocomplete="list"
              aria-controls="launcher-results"
              aria-expanded={snapshot.results.length > 0}
              aria-activedescendant={
                snapshot.selectedIndex >= 0 ? `launcher-result-${snapshot.results[snapshot.selectedIndex]?.key}` : undefined
              }
              autoComplete="off"
              disabled={!snapshot.invocationId || snapshot.view !== 'launcher'}
              placeholder="请输入参数"
              role="combobox"
              spellCheck={false}
              onKeyDown={mainResultKeyDown}
              onBound={reportMainResultBound}
              onUnbound={reportMainResultUnbound}
              onBindingFailed={reportFailed}
            />
            {snapshot.executePending && snapshot.results.length === 0 ? (
              <span role="status" aria-label={`正在打开 ${mainResultCommand.commandLabel} 命令`}>
                <Spin size="small" />
              </span>
            ) : null}
            {settingsControl}
          </div>
        ) : (
          <>
            <label className="visually-hidden" htmlFor={`launcher-query-${snapshot.queryControl}`}>
              搜索应用
            </label>
            <BoundInput
              core={core}
              control={snapshot.queryControl}
              value={snapshot.queryControlValue}
              id={`launcher-query-${snapshot.queryControl}`}
              name={`launcher-query-${snapshot.queryControl}`}
              placeholder="搜索应用"
              autoComplete="off"
              spellCheck={false}
              disabled={!snapshot.invocationId || snapshot.view !== 'launcher'}
              role="combobox"
              aria-autocomplete="list"
              aria-controls="launcher-results"
              aria-expanded={snapshot.results.length > 0}
              aria-activedescendant={
                snapshot.selectedIndex >= 0 ? `launcher-result-${snapshot.results[snapshot.selectedIndex]?.key}` : undefined
              }
              suffix={settingsControl}
              onKeyDown={queryKeyDown}
              onBound={reportQueryBound}
              onBindingFailed={reportFailed}
            />
          </>
        )}
      </div>

      <div className="launcher-results-region">
        <Spin spinning={snapshot.searchPending} size="small">
        <div className="launcher-result-surface">
          {snapshot.commandHint ? <div className="command-hint">{snapshot.commandHint}</div> : null}
          <div
            id="launcher-results"
            className="result-list"
            role="listbox"
            aria-label="搜索结果"
            onKeyDown={queryKeyDown}
          >
            {launcherResultSections
              ? launcherResultSections.map((section) => (
                  <div
                    key={section.key}
                    className="result-section"
                    role="group"
                    aria-labelledby={`launcher-${section.key}-title`}
                  >
                    <div
                      id={`launcher-${section.key}-title`}
                      className="result-section-title"
                      role="presentation"
                    >
                      {section.title}
                    </div>
                    {section.results.map(({ item, index }) => renderLauncherResult(item, index))}
                  </div>
                ))
              : indexedLauncherResults.map(({ item, index }) => renderLauncherResult(item, index))}
          </div>
        </div>
        </Spin>
      </div>
    </section>
  )

  const chooseFileResult = (index: number) => {
    if (!file || activeFileIndex === index) return
    const direction = index > activeFileIndex ? 'ArrowDown' : 'ArrowUp'
    const steps = Math.abs(index - activeFileIndex)
    for (let step = 0; step < steps; step += 1) core.keyDown(direction, false)
  }

  const filePanel = file ? (
    <section className="file-workspace" aria-label="文件搜索">
      <label className="visually-hidden" htmlFor={`launcher-query-${snapshot.queryControl}`}>
        搜索文件
      </label>
      <BoundInput
        core={core}
        control={snapshot.queryControl}
        value={snapshot.queryControlValue}
        id={`launcher-query-${snapshot.queryControl}`}
        name={`launcher-query-${snapshot.queryControl}`}
        placeholder="搜索文件"
        autoComplete="off"
        spellCheck={false}
        disabled={!snapshot.invocationId || snapshot.view !== 'launcher'}
        role="combobox"
        aria-autocomplete="list"
        aria-controls="file-results"
        aria-expanded={file.results.length > 0}
        aria-activedescendant={activeFileIndex >= 0 ? `file-result-option-${activeFileIndex}` : undefined}
        onKeyDown={queryKeyDown}
        onBound={reportQueryBound}
        onBindingFailed={reportFailed}
      />
      <nav className="file-categories" aria-label="文件分类">
        {fileCategoryOptions.map(({ value, label }) => (
          <button
            key={value}
            type="button"
            className={file.category === value ? 'file-category is-selected' : 'file-category'}
            aria-current={file.category === value ? 'page' : undefined}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              core.setFileCategory(value)
              queryRef.current?.focus()
            }}
          >
            {label}
          </button>
        ))}
      </nav>
      <Spin spinning={snapshot.searchPending} size="small">
        <div id="file-results" className="result-list file-result-list" role="listbox" aria-label="文件结果">
          {file.results.map((item, index) => (
            <div
              key={item.key}
              id={`file-result-option-${index}`}
              role="option"
              tabIndex={-1}
              aria-selected={activeFileIndex === index}
              className={activeFileIndex === index ? 'result-row file-result-row is-selected' : 'result-row file-result-row'}
              ref={(element) => {
                if (element) fileOptionRefs.current.set(index, element)
                else fileOptionRefs.current.delete(index)
              }}
              onMouseDown={(event) => {
                event.preventDefault()
                chooseFileResult(index)
                queryRef.current?.focus()
              }}
              onDoubleClick={() => {
                chooseFileResult(index)
                core.keyDown('Enter', false)
                queryRef.current?.focus()
              }}
            >
              <span className="result-icon file-kind-mark" aria-hidden="true">
                {item.kind === 'folder' ? '□' : '◇'}
              </span>
              <span className="result-copy">
                <Tooltip title={item.name}>
                  <span className="result-title">{item.name}</span>
                </Tooltip>
                <span className="result-subtitle">{item.fullPath}</span>
              </span>
            </div>
          ))}
        </div>
      </Spin>
      <aside className="file-preview" aria-label="文件预览">
        {file.previewEnabled && file.selected ? (
          <>
            <Tooltip title={file.selected.name}>
              <h2>{file.selected.name}</h2>
            </Tooltip>
            <dl>
              <dt>类型</dt>
              <dd>{file.selected.kind === 'folder' ? '文件夹' : '文件'}</dd>
              <dt>大小</dt>
              <dd>{fileSize(file.selected.kind, file.selected.sizeBytes)}</dd>
              <dt>修改时间</dt>
              <dd>{fileModified(file.selected.modifiedUtc)}</dd>
              <dt>完整路径</dt>
              <dd>{file.selected.fullPath}</dd>
            </dl>
          </>
        ) : (
          <p>预览已关闭</p>
        )}
      </aside>
      <footer className="file-toolbar">
        <span>{file.indexStatus === 'building' ? `正在索引，已有 ${file.total} 条结果` : `共 ${file.total} 条结果`}</span>
        <Switch
          aria-label="文件预览"
          checked={file.previewEnabled}
          loading={file.preferencePending}
          disabled={file.preferencePending}
          onChange={(checked) => core.setFilePreviewEnabled(checked)}
        />
        <Tooltip title="设置暂不可用">
          <Button aria-label="设置暂不可用" disabled className="file-settings-placeholder">
            ⚙
          </Button>
        </Tooltip>
      </footer>
    </section>
  ) : null
  const panelLauncher = panel ? (
    <section className="panel-launcher" aria-label={`${panel.commandLabel} 面板`}>
      <div className="panel-input-region">
        <div className="panel-input-row panel-input-shell">
          <CommandTag
            commandLabel={panel.commandLabel}
            disabled={panel.closePending}
            exitLabel={`退出 ${panel.commandLabel} 面板`}
            exitTitle="退出面板"
            onClose={() => void core.closePanel()}
          />
          <label className="visually-hidden" htmlFor={`launcher-panel-suffix-${panel.suffixControl}`}>
            {panel.commandLabel} argument
          </label>
          <BoundInput
            className="panel-suffix-input"
            onFocus={() => { panelActionSource.current = 'hostInput' }}
            core={core}
            control={panel.suffixControl}
            value={panel.suffix}
            id={`launcher-panel-suffix-${panel.suffixControl}`}
            name={`launcher-panel-suffix-${panel.suffixControl}`}
            aria-label={`${panel.commandLabel} argument`}
            placeholder={panel.inputPlaceholder}
            autoComplete="off"
            spellCheck={false}
            disabled={!snapshot.invocationId || panel.closePending}
            onKeyDown={panelKeyDown}
            onBound={reportPanelBound}
            onUnbound={reportPanelUnbound}
            onBindingFailed={reportFailed}
          />
        </div>
      </div>
      <div className="panel-content-divider" aria-hidden="true" />
      <div ref={panelHostRef} className="panel-host-region" aria-label="插件面板内容" />
    </section>
  ) : null

  const quicklinksBusy = quicklinks?.operation !== undefined
  const quicklinksDraft = quicklinks?.draft
  const quicklinksCreateDraft = quicklinks?.createDraft
  const quicklinksDraftActive = quicklinks?.draftActive === true
  const quicklinksCanSave = Boolean(
    quicklinksDraftActive &&
      quicklinksDraft?.name.trim() &&
      quicklinksDraft.command.trim() &&
      quicklinksDraft.template.trim() &&
      !quicklinksBusy,
  )
  const quicklinksCreateCanSave = Boolean(
    quicklinksCreateDraft?.name.trim() &&
      quicklinksCreateDraft.command.trim() &&
      quicklinksCreateDraft.template.trim() &&
      !quicklinksBusy,
  )
  const quicklinksPanel = quicklinks ? (
    <>
      <section className="quicklinks-workspace" aria-label="快速链接" onKeyDown={quicklinksKeyDown}>
        <div className="panel-input-region">
          <div className="panel-input-row panel-input-shell quicklinks-input-shell">
            <CommandTag
              commandLabel="quicklinks"
              disabled={quicklinksBusy}
              exitLabel="退出 quicklinks 面板"
              exitTitle="退出面板"
              onClose={core.closeQuicklinks}
            />
            <label className="visually-hidden" htmlFor={`quicklinks-filter-${snapshot.queryControl}`}>
              搜索快速链接目录
            </label>
            <BoundInput
              className="panel-suffix-input quicklinks-filter-input"
              core={core}
              control={snapshot.queryControl}
              value={snapshot.queryControlValue}
              id={`quicklinks-filter-${snapshot.queryControl}`}
              name={`quicklinks-filter-${snapshot.queryControl}`}
              aria-label="搜索快速链接目录"
              placeholder="搜索快速链接"
              autoComplete="off"
              spellCheck={false}
              tabIndex={-1}
              disabled={!snapshot.invocationId || quicklinksBusy}
              onKeyDown={quicklinksKeyDown}
              onBound={reportQuicklinksBound}
              onUnbound={reportQuicklinksUnbound}
              onBindingFailed={reportFailed}
            />
          </div>
        </div>
        <div className="quicklinks-body">
          <aside className="quicklinks-directory" aria-label="快速链接目录">
            <div className="quicklinks-directory-header">
              <span>快速链接</span>
              <Button
                aria-label="新增快速链接"
                disabled={quicklinksBusy}
                icon={<Plus aria-hidden size={16} />}
                onClick={core.newQuicklink}
                size="small"
                tabIndex={-1}
                type="primary"
              />
            </div>
            <Spin spinning={quicklinks.status === 'loading'} size="small">
              <OverlayScrollbarsComponent className="quicklinks-list-scroll" options={settingsScrollbarOptions}>
                <div className="quicklinks-list" role="list">
                  {quicklinks.items.length ? quicklinks.items.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      data-quicklink-id={item.id}
                      className={quicklinks.selectedId === item.id ? 'quicklink-item is-selected' : 'quicklink-item'}
                      aria-current={quicklinks.selectedId === item.id ? 'true' : undefined}
                      tabIndex={-1}
                      onClick={() => core.selectQuicklink(item.id)}
                      ref={(element) => {
                        if (element) quicklinkItemRefs.current.set(item.id, element)
                        else quicklinkItemRefs.current.delete(item.id)
                      }}
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter' || composing(event)) return
                        event.preventDefault()
                        event.stopPropagation()
                        core.completeQuicklink(item.id)
                      }}
                    >
                      <span className="quicklink-item-icon" aria-hidden="true">
                        {item.iconDataUrl ? (
                          <img src={item.iconDataUrl} alt="" draggable={false} />
                        ) : (
                          <ImageIcon size={18} strokeWidth={1.8} />
                        )}
                      </span>
                      <span className="quicklink-item-copy">
                        <span className="quicklink-item-name">{item.name || '未命名目录'}</span>
                        <code>/{item.command}</code>
                      </span>
                    </button>
                  )) : (
                    <div className="quicklinks-empty">暂无快速链接，点击右上角新增。</div>
                  )}
                </div>
              </OverlayScrollbarsComponent>
            </Spin>
          </aside>
          <section
            className={quicklinksDraftActive ? 'quicklinks-editor-pane' : 'quicklinks-editor-pane is-empty'}
            aria-label="快速链接表单"
          >
            {quicklinksDraftActive ? (
              <>
                <header className="quicklinks-editor-header">
                  <div className="quicklinks-editor-heading">
                    <strong>{quicklinksDraft?.name || '未命名目录'}</strong>
                    <code>/{quicklinksDraft?.command}</code>
                  </div>
                  {quicklinksStatus ? (
                    <span
                      className="quicklinks-editor-status"
                      data-tone={quicklinksStatus.tone}
                      role="status"
                      aria-live="polite"
                    >
                      {quicklinksStatus.message}
                    </span>
                  ) : null}
                </header>
                <OverlayScrollbarsComponent className="quicklinks-editor-scroll" options={settingsScrollbarOptions}>
                  <section ref={quicklinksEditorRef} className="quicklinks-editor">
                <Form component="div" layout="vertical" className="quicklinks-form">
                  <Form.Item label="目录名称" className="quicklinks-form-row">
                    <Input
                      aria-label="目录名称"
                      value={quicklinksDraft?.name ?? ''}
                      disabled={quicklinksBusy}
                      onChange={(event) => core.setQuicklinkDraft('name', event.target.value)}
                    />
                  </Form.Item>
                  <Form.Item label="启动键" className="quicklinks-form-row">
                    <div className="quicklink-command-field">
                      <span aria-hidden="true">/</span>
                      <Input
                        aria-label="启动键"
                        value={quicklinksDraft?.command ?? ''}
                        disabled={quicklinksBusy}
                        placeholder="jd"
                        onChange={(event) => core.setQuicklinkDraft('command', event.target.value)}
                      />
                    </div>
                  </Form.Item>
                  <Form.Item label="图标" className="quicklinks-form-row quicklinks-form-row-icon">
                    <div className="quicklink-icon-picker">
                      <button
                        type="button"
                        className="quicklink-icon-preview"
                        disabled={quicklinksBusy}
                        onClick={() => void core.chooseQuicklinkIcon()}
                      >
                        {quicklinksDraft?.iconDataUrl ? (
                          <img src={quicklinksDraft.iconDataUrl} alt="" draggable={false} />
                        ) : (
                          <span>
                            <ImageIcon size={22} strokeWidth={1.8} />
                            选择 128×128 PNG
                          </span>
                        )}
                      </button>
                      <Button
                        icon={<ImageIcon aria-hidden size={16} />}
                        loading={quicklinks.operation === 'icon'}
                        disabled={quicklinksBusy && quicklinks.operation !== 'icon'}
                        onClick={() => void core.chooseQuicklinkIcon()}
                      >
                        选择图标
                      </Button>
                    </div>
                  </Form.Item>
                  <Form.Item label="链接" className="quicklinks-form-row">
                    <Input
                      aria-label="链接"
                      value={quicklinksDraft?.template ?? ''}
                      disabled={quicklinksBusy}
                      placeholder="https://search.jd.com/Search?keyword={Query}"
                      onChange={(event) => core.setQuicklinkDraft('template', event.target.value)}
                    />
                  </Form.Item>
                  {quicklinks.error ? <div className="quicklinks-error">{quicklinks.error}</div> : null}
                  <div className="quicklinks-actions">
                    <Button
                      type="primary"
                      icon={<Save aria-hidden size={16} />}
                      loading={quicklinks.operation === 'save'}
                      disabled={!quicklinksCanSave}
                      onClick={() => void saveQuicklinkWithStatus()}
                    >
                      保存
                    </Button>
                    <Popconfirm
                      title="删除快速链接？"
                      description="删除后该启动键将不再可用。"
                      okText="删除"
                      cancelText="取消"
                      disabled={!quicklinksDraft?.id || quicklinksBusy}
                      onConfirm={() => void core.deleteQuicklink()}
                    >
                      <Button
                        danger
                        icon={<Trash2 aria-hidden size={16} />}
                        loading={quicklinks.operation === 'delete'}
                        disabled={!quicklinksDraft?.id || quicklinksBusy}
                      >
                        删除
                      </Button>
                    </Popconfirm>
                  </div>
                </Form>
                  </section>
                </OverlayScrollbarsComponent>
              </>
              ) : (
                <section ref={quicklinksEditorRef} className="quicklinks-editor-empty">
                  <span className="quicklinks-editor-empty-icon" aria-hidden="true">
                    <Link2 size={24} strokeWidth={1.7} />
                  </span>
                  <h2>请选择或新增快速链接</h2>
                  <p>从左侧目录选择一条快速链接进行编辑，或点击右上角新增。</p>
                </section>
              )}
          </section>
        </div>
      </section>
      {quicklinksCreateOpen ? (
        <dialog
          ref={quicklinkCreateDialogRef}
          className="dialog quicklink-create-dialog"
          role="dialog"
          aria-labelledby="quicklink-create-title"
          onCancel={(event) => {
            event.preventDefault()
            if (!quicklinksBusy) core.closeNewQuicklink()
          }}
        >
          <form
            method="dialog"
            className="dialog-body quicklinks-create-form"
            onSubmit={(event) => {
              event.preventDefault()
              if (quicklinksCreateCanSave) void core.saveNewQuicklink()
            }}
          >
            <h3 id="quicklink-create-title" className="dialog-title">新建快速链接</h3>
            <Form component="div" layout="vertical" className="quicklinks-form">
              <Form.Item label="目录名称">
                <Input
                  aria-label="目录名称"
                  autoFocus
                  value={quicklinksCreateDraft?.name ?? ''}
                  disabled={quicklinksBusy}
                  onChange={(event) => core.setNewQuicklinkDraft('name', event.target.value)}
                />
              </Form.Item>
              <Form.Item label="启动键">
                <div className="quicklink-command-field">
                  <span aria-hidden="true">/</span>
                  <Input
                    aria-label="启动键"
                    value={quicklinksCreateDraft?.command ?? ''}
                    disabled={quicklinksBusy}
                    placeholder="jd"
                    onChange={(event) => core.setNewQuicklinkDraft('command', event.target.value)}
                  />
                </div>
              </Form.Item>
              <Form.Item label="图标">
                <div className="quicklink-icon-picker">
                  <button
                    type="button"
                    className="quicklink-icon-preview"
                    disabled={quicklinksBusy}
                    onClick={() => void core.chooseNewQuicklinkIcon()}
                  >
                    {quicklinksCreateDraft?.iconDataUrl ? (
                      <img src={quicklinksCreateDraft.iconDataUrl} alt="" draggable={false} />
                    ) : (
                      <span>
                        <ImageIcon size={22} strokeWidth={1.8} />
                        选择 128×128 PNG
                      </span>
                    )}
                  </button>
                  <Button
                    icon={<ImageIcon aria-hidden size={16} />}
                    loading={quicklinks.operation === 'icon'}
                    disabled={quicklinksBusy && quicklinks.operation !== 'icon'}
                    onClick={() => void core.chooseNewQuicklinkIcon()}
                  >
                    选择图标
                  </Button>
                </div>
              </Form.Item>
              <Form.Item label="链接">
                <Input
                  aria-label="链接"
                  value={quicklinksCreateDraft?.template ?? ''}
                  disabled={quicklinksBusy}
                  placeholder="https://search.jd.com/Search?keyword={Query}"
                  onChange={(event) => core.setNewQuicklinkDraft('template', event.target.value)}
                />
              </Form.Item>
              {quicklinks.error ? <div className="quicklinks-error">{quicklinks.error}</div> : null}
            </Form>
            <div className="dialog-actions">
              <Button
                type="primary"
                htmlType="submit"
                icon={<Save aria-hidden size={16} />}
                loading={quicklinks.operation === 'save'}
                disabled={!quicklinksCreateCanSave}
              >
                保存
              </Button>
              <Button disabled={quicklinksBusy} onClick={core.closeNewQuicklink}>
                取消
              </Button>
            </div>
          </form>
        </dialog>
      ) : null}
    </>
  ) : null

  const settings = snapshot.settings
  const plugins = snapshot.plugins
  const showLegacyPluginInventory =
    plugins?.status === 'error' || (plugins?.items.length ?? 0) > 0
  const busy = settings?.operation !== undefined
  const locked = busy || settings?.readOnly === true
  const generalSettingsPanel = (
    <OverlayScrollbarsComponent className="settings-tab-panel settings-general-panel" options={settingsScrollbarOptions}>
      <div className="settings-scroll-content">
        {!settings ? (
          <div className="settings-loading">
            {snapshot.settingsLoadStatus === 'error' ? (
              <Button onClick={() => void core.reloadSettings()}>重试</Button>
            ) : (
              <Spin size="small" />
            )}
          </div>
        ) : (
          <Form component="div" layout="vertical" className="settings-basic-form">
            <Form.Item label="快捷键" htmlFor={`settings-hotkey-${settings.hotkey.key}`}
              help={core.capabilities.platform === 'macos' && core.capabilities.doubleTap
                ? '双击同一侧 Command 可唤起 UiPilot。仅启用此手势时需要「输入监控」权限，普通组合键可直接使用。' : undefined}>
              <HotkeyRecorderInput
                allowSameValue={core.capabilities.platform === 'macos'}
                allowDoubleTap={core.capabilities.doubleTap}
                platform={core.capabilities.platform}
                onCommit={async (value) => { try { await core.saveHotkeyCanonical(value) } finally { setHotkeyStatusRevision(value => value + 1) } }}
                onRecordingPhase={handleHotkeyRecordingPhase}
                value={settings.hotkey.value}
                id={`settings-hotkey-${settings.hotkey.key}`}
                name={`settings-hotkey-${settings.hotkey.key}`}
                disabled={locked}
              />
            </Form.Item>
            {core.capabilities.platform === 'macos' && <MacHotkeyStatusNotice read={core.client.macosHotkeyStatus} revision={hotkeyStatusRevision} />}
            {core.capabilities.screenshot && <>
              <Form.Item label="截图快捷键" htmlFor="settings-screenshot-hotkey" help={settings.screenshotHotkeyStatus === 'unavailable' ? '快捷键不可用，请重新录制或清除后重试。' : settings.screenshotHotkeyStatus === 'active' ? '已启用' : '未设置'}>
                <HotkeyRecorderInput
                  label="截图快捷键"
                  id="settings-screenshot-hotkey"
                  name="settings-screenshot-hotkey"
                  allowDoubleTap={false}
                  allowSameValue={settings.screenshotHotkeyStatus === 'unavailable'}
                  value={settings.screenshotHotkey ?? ''}
                  disabled={locked}
                  onCommit={core.saveScreenshotHotkey}
                  onRecordingPhase={handleHotkeyRecordingPhase}
                />
                <Button aria-label="清除截图快捷键" disabled={locked || settings.screenshotHotkey === null} onClick={() => void core.saveScreenshotHotkey(null)}>清除</Button>
              </Form.Item>
              <Form.Item label="截图默认位置" htmlFor="settings-screenshot-save-directory">
                <Input
                  id="settings-screenshot-save-directory"
                  aria-label="截图默认位置"
                  readOnly
                  value={settings.screenshotSaveDirectoryPath ?? ''}
                  title={settings.screenshotSaveDirectoryPath ?? undefined}
                  placeholder="保存目录不可用"
                />
                <div className="settings-screenshot-directory-actions">
                  <Button disabled={locked} loading={settings.operation === 'screenshotDirectory'} onClick={() => void core.selectScreenshotSaveDirectory()}>选择文件夹</Button>
                  <Button disabled={locked || settings.screenshotSaveDirectory === null} onClick={() => void core.restoreScreenshotSaveDirectory()}>恢复桌面</Button>
                </div>
              </Form.Item>
            </>}
            {core.capabilities.loginItems && <MacLoginItemControl
              key={`${snapshot.invocationId}:${snapshot.viewEpoch}`}
              client={core.client.macosLoginItem} disabled={locked} />}
            {core.capabilities.autostart && <Checkbox
              checked={settings.autostart}
              disabled={locked}
              onChange={(event) => core.setAutostart(event.target.checked)}
            >
              开机启动
            </Checkbox>}
            <Form.Item label="风格">
              <Select
                aria-label="风格"
                value={settings.theme}
                disabled={locked}
                options={themeOptions}
                onChange={(value: ThemePreference) => core.setThemePreference(value)}
              />
            </Form.Item>
            {core.capabilities.webSearch && <Form.Item label="搜索引擎">
              <Select
                aria-label="搜索引擎"
                value={settings.webSearchEngine}
                disabled={locked}
                options={webSearchEngineOptions}
                onChange={(value: WebSearchEngine) => core.setWebSearchEngine(value)}
              />
            </Form.Item>}
            <div className="settings-actions">
              {core.capabilities.platform === 'windows' && <Popconfirm
                title="恢复初始化设置？"
                description="快捷键将恢复为 Shift+Space，关闭开机启动，将风格恢复为跟随系统，并将搜索引擎恢复为 Bing。"
                okText="恢复"
                cancelText="取消"
                onConfirm={() => void core.resetSettings()}
                disabled={locked}
              >
                <Button danger disabled={locked} loading={settings.operation === 'save'}>
                  恢复初始化
                </Button>
              </Popconfirm>}
              {settings.loadStatus === 'error' ? (
                <Button
                  disabled={busy}
                  loading={settings.operation === 'load'}
                  onClick={() => void core.reloadSettings()}
                >
                  重试
                </Button>
              ) : null}
            </div>
          </Form>
        )}
      </div>
    </OverlayScrollbarsComponent>
  )
  const selectSettingsTab = (key: SettingsTabKey) => {
    if (activeSettingsTab === 'plugins' && key !== 'plugins' && macPluginManagementRef.current) {
      macPluginManagementRef.current.requestLeave(() => applySettingsTab(key)); return
    }
    if (activeSettingsTab === 'credentials' && key !== 'credentials' && credentialSettingsRef.current) {
      credentialSettingsRef.current.requestLeave(() => applySettingsTab(key))
      return
    }
    applySettingsTab(key)
  }
  const applySettingsTab = (key: SettingsTabKey) => {
    core.selectSettingsTab(key)
    if (key === 'plugins' && core.capabilities.plugins) {
      if (activatedPluginEpoch.current === snapshot.viewEpoch) return
      activatedPluginEpoch.current = snapshot.viewEpoch
      void core.activatePlugins()
    } else {
      activatedPluginEpoch.current = undefined
      core.deactivatePlugins()
    }
  }
  const closePublicPluginDetail = () => {
    detailSession.current++
    detailReload.current++
    detailReloadPending.current?.beforePublish.splice(0)
    detailReloadPending.current?.resolve()
    detailReloadPending.current = null
    restoreSettingsTabFocus.current = true
    setPublicPluginDetail(null)
    setBuiltinFeatureDetail(null)
  }
  const pluginSettingsPanel = (
    <OverlayScrollbarsComponent className="settings-tab-panel settings-plugin-panel" options={settingsScrollbarOptions}>
      <div className="settings-scroll-content">
        {core.capabilities.pluginManagement && !core.capabilities.plugins ? <MacPluginManagement ref={macPluginManagementRef} client={core.client.macPluginManagement} onStatusChange={setMacPluginStatus} /> : <PublicPluginPanel client={core.client} removalOutcome={publicPluginRemovalOutcome} onOpenBuiltinDetails={setBuiltinFeatureDetail} onOpenDetails={(plugin, revision, hotkeyRevision) => { setPublicPluginRemovalOutcome(null); setPublicPluginDetail({ plugin, revision, hotkeyRevision, session: ++detailSession.current }) }} />}
        {showLegacyPluginInventory ? (
        <section className="plugin-inventory" aria-labelledby="plugin-inventory-title">
        <div className="plugin-inventory-header">
          <h2 id="plugin-inventory-title">插件</h2>
          <Button
            size="small"
            disabled={plugins?.status === 'loading'}
            onClick={() => void core.reloadPlugins()}
          >
            {plugins?.status === 'error' ? '重试' : '刷新'}
          </Button>
        </div>
        {plugins?.status === 'loading' || plugins?.status === 'idle' ? (
          <div className="plugin-list-state"><Spin size="small" /></div>
        ) : plugins?.status === 'error' ? (
          <div className="plugin-list-state plugin-list-error" role="alert">{plugins.error}</div>
        ) : plugins?.items.length === 0 ? (
          <div className="plugin-list-state">未发现插件</div>
        ) : (
          <div className="plugin-list">
            {plugins?.items.map((plugin) => {
              const installed = plugin.installed.state === 'valid' ? plugin.installed : undefined
              const development = plugin.development.state === 'valid' ? plugin.development : undefined
              const canInstall =
                plugin.id !== null &&
                development !== undefined &&
                (plugin.installed.state === 'absent' ||
                  (installed !== undefined && comparePluginVersion(development.version, installed.activeVersion) > 0))
              const installLabel = installed ? '更新' : '安装'
              const version = installed?.activeVersion ?? development?.version
              const trigger = installed?.trigger ?? development?.trigger
              const fallbackVersions = installed?.versions
                .filter((candidateVersion) => candidateVersion !== installed.activeVersion)
                .sort(comparePluginVersion)
              const fallbackVersion = fallbackVersions?.[fallbackVersions.length - 1]
              const deleteDescription = installed
                ? fallbackVersion
                  ? `将删除 ${installed.activeVersion}，并自动启用 ${fallbackVersion}。`
                  : `将删除最后一个版本 ${installed.activeVersion}。`
                : ''
              const stateLabel =
                plugin.installed.state === 'invalid'
                  ? '安装状态故障'
                  : plugin.development.state === 'invalid'
                    ? '开发包不可用'
                    : installed
                      ? canInstall
                        ? '可更新'
                        : '已安装'
                      : '未安装'
              return (
              <article className="plugin-item" key={plugin.key}>
                <div className="plugin-item-main">
                  <div className="plugin-title-line">
                    <h3>{plugin.displayName}</h3>
                    <span>{stateLabel}</span>
                    {version ? <span>{version}</span> : null}
                    {trigger ? <code>{trigger}</code> : null}
                  </div>
                  {installed ? (
                    <div className="plugin-version-list">
                      已安装版本：{installed.versions.join('、')}
                    </div>
                  ) : null}
                  {development ? (
                    <div className="plugin-version-list">
                      开发版本：{development.version}
                    </div>
                  ) : null}
                  <div className="plugin-description">
                    <div className="plugin-description-label">说明</div>
                    {plugin.description.state === 'available' ? (
                      <ReactMarkdown allowedElements={pluginMarkdownElements} unwrapDisallowed>
                        {plugin.description.markdown}
                      </ReactMarkdown>
                    ) : (
                      <p>暂无说明</p>
                    )}
                  </div>
                  {plugin.error ? <div className="plugin-row-error" role="alert">{plugin.error}</div> : null}
                </div>
                <div className="plugin-actions">
                  {canInstall ? <Button
                    size="small"
                    loading={plugin.operation === 'install'}
                    disabled={plugin.operation !== undefined}
                    onClick={() => void core.installPlugin(plugin.id!)}
                  >
                    {installLabel}
                  </Button> : null}
                  {installed && plugin.id ? <Button
                    size="small"
                    loading={plugin.operation === 'reload'}
                    disabled={plugin.operation !== undefined}
                    onClick={() => void core.reloadPlugin(plugin.id!)}
                  >
                    重新加载
                  </Button> : null}
                  {installed && plugin.id ? <Popconfirm
                    title="删除此插件？"
                    description={deleteDescription}
                    okText="删除"
                    cancelText="取消"
                    onConfirm={() => void core.deletePlugin(plugin.id!)}
                    disabled={plugin.operation !== undefined}
                  >
                    <Button size="small" danger loading={plugin.operation === 'delete'} disabled={plugin.operation !== undefined}>
                      删除
                    </Button>
                  </Popconfirm> : null}
                </div>
              </article>
              )
            })}
          </div>
        )}
        </section>
        ) : null}
      </div>
    </OverlayScrollbarsComponent>
  )
  const messageSettingsPanel = (
    <MessageCenterPanel
      state={snapshot.messageCenter}
      onClear={() => void core.clearMessages()}
    />
  )
  const publicPluginDetailView = selectedPublicPlugin || builtinFeatureDetail ? (
    <section
      ref={publicPluginDetailRef}
      className="settings-view public-plugin-detail-view"
      aria-label={`${selectedPublicPlugin?.name ?? builtinFeatureDetail!.name} ${builtinFeatureDetail ? '内置功能' : '插件'}详情`}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || composing(event) || !event.currentTarget.contains(event.target as Node)) return
        event.preventDefault()
        closePublicPluginDetail()
      }}
    >
      <div className="settings-header-region">
        <header className="settings-header">
          <div className="settings-title-group">
            <Tooltip title="返回插件列表">
              <Button
                aria-label="返回插件列表"
                icon={<ArrowLeft aria-hidden size={17} strokeWidth={1.8} />}
                onClick={closePublicPluginDetail}
                size="small"
                type="text"
              />
            </Tooltip>
            <h1 id="public-plugin-detail-title" className="public-plugin-detail-title" title={selectedPublicPlugin?.name ?? builtinFeatureDetail!.name}>{selectedPublicPlugin?.name ?? builtinFeatureDetail!.name}</h1>
          </div>
          <div className="public-detail-header-actions" ref={setDetailHeaderActions} />
        </header>
      </div>
      <OverlayScrollbarsComponent className="settings-detail-panel" options={settingsScrollbarOptions}>
        <div className="settings-scroll-content">
          {builtinFeatureDetail ? <BuiltinFeatureDetail key={builtinFeatureDetail.feature} client={core.client} feature={builtinFeatureDetail} headerActionsTarget={detailHeaderActions} onRecordingPhase={handleHotkeyRecordingPhase} /> : selectedPublicPlugin ? <PublicPluginDetail
            client={core.client}
            key={`${selectedPublicPlugin.pluginId}:${publicPluginDetail!.session}`}
            plugin={selectedPublicPlugin}
            headerActionsTarget={detailHeaderActions}
            onRemoved={(outcome) => { setPublicPluginRemovalOutcome(outcome); closePublicPluginDetail() }}
            inventoryRevision={publicPluginDetail!.revision}
            hotkeyRevision={publicPluginDetail!.hotkeyRevision} onRecordingPhase={handleHotkeyRecordingPhase}
            reload={async (beforePublish) => {
              const session = publicPluginDetail!.session
              if (detailSession.current !== session) return
              const previous = detailReloadPending.current
              let resolve!: (next?: PromiseLike<void>) => void
              let reject!: (reason: unknown) => void
              const promise = new Promise<void>((accepted, failed) => { resolve = accepted; reject = failed })
              const entry = {
                session, request: ++detailReload.current, promise, resolve, reject,
                beforePublish: previous?.session === session ? previous.beforePublish.splice(0) : [],
              }
              if (beforePublish) entry.beforePublish.push(beforePublish)
              detailReloadPending.current = entry
              // Older waiters follow the newest request immediately, even if their read hangs.
              if (previous?.session === session) previous.resolve(entry.promise)
              void (async () => {
                try {
                  const inventory = await core.client.listPublicPlugins()
                  if (detailSession.current !== session || detailReload.current !== entry.request) return
                  // Notify waiting rows before React can publish the accepted revision.
                  for (const accept of entry.beforePublish.splice(0)) accept(`${inventory.revision}:${inventory.hotkeyRevision}`)
                  setPublicPluginDetail((current) => {
                    if (current?.session !== session || current.plugin.pluginId !== selectedPublicPlugin.pluginId) return current
                    const plugin = inventory.items.find(({ pluginId }) => pluginId === current.plugin.pluginId)
                    return plugin ? { plugin, revision: inventory.revision, hotkeyRevision: inventory.hotkeyRevision, session } : null
                  })
                  entry.resolve()
                } catch (error) {
                  if (detailSession.current !== session || detailReload.current !== entry.request) return
                  entry.beforePublish.length = 0
                  entry.reject(error)
                }
              })()
              await entry.promise
            }}
          /> : null}
        </div>
      </OverlayScrollbarsComponent>
    </section>
  ) : null
  const settingsView = (
    <section className="settings-view" aria-label="设置" onKeyDown={settingsKeyDown}>
      <div className="settings-header-region">
        <header className="settings-header">
          <div className="settings-title-group">
            <Tooltip title="返回主界面">
              <Button
                aria-label="返回主界面"
                disabled={snapshot.hidePending}
                icon={<ArrowLeft aria-hidden size={17} strokeWidth={1.8} />}
                onClick={() => {
                  if (activeSettingsTab === 'plugins' && macPluginManagementRef.current) macPluginManagementRef.current.requestLeave(() => core.navigate('launcher'))
                  else if (activeSettingsTab === 'credentials' && credentialSettingsRef.current) credentialSettingsRef.current.requestLeave(() => core.navigate('launcher'))
                  else core.navigate('launcher')
                }}
                size="small"
                type="text"
              />
            </Tooltip>
            <h1>设置</h1>
          </div>
          {['windows', 'macos'].includes(core.capabilities.platform) && <Tooltip title={`${pinError ? '置顶设置失败，点击重试' : appliedPin ? '取消置顶' : '置顶设置窗口'} (Ctrl+T)`}>
            <Button
              aria-label={pinError ? '重试置顶设置' : appliedPin ? '取消置顶' : '置顶设置窗口'}
              aria-pressed={appliedPin}
              aria-keyshortcuts="Control+t"
              disabled={snapshot.hidePending || pinPending}
              icon={appliedPin ? <Pin size={17} aria-hidden /> : <PinOff size={17} aria-hidden />}
              type={appliedPin ? 'primary' : 'text'}
              size="small"
              onClick={() => pinError ? setPinAttempt((attempt) => attempt + 1) : setSettingsPin((pinned) => !pinned)}
            />
          </Tooltip>}
        </header>
      </div>
      <div
        ref={settingsTabsRef}
        className="settings-tabs"
        onFocusCapture={(event) => {
          const key = settingsTabKey(event.target)
          if (!key || key === activeSettingsTab) return
          selectSettingsTab(key)
        }}
      >
        <Tabs
          activeKey={activeSettingsTab}
          destroyOnHidden
          items={[
            { key: 'general', label: '通用', children: generalSettingsPanel },
            {
              key: 'messages',
              label: (
                <Badge
                  className={`settings-message-tab-badge${snapshot.messageCenter.status === 'unavailable' ? ' is-unavailable' : ''}`}
                  count={messageBadgeCount}
                  offset={[-1, 1]}
                  overflowCount={99}
                  size="small"
                >
                  <span>消息</span>
                </Badge>
              ),
              children: messageSettingsPanel,
            },
            { key: 'plugins', label: '插件', children: pluginSettingsPanel },
            {
              key: 'permissions', label: '权限', children: (
                <OverlayScrollbarsComponent className="settings-tab-panel" options={settingsScrollbarOptions}>
                  <div className="settings-scroll-content">
                    <PluginPermissionsPanel client={core.client} active={snapshot.view === 'settings' && activeSettingsTab === 'permissions' && !publicPluginDetail && !builtinFeatureDetail} />
                  </div>
                </OverlayScrollbarsComponent>
              ),
            },
            {
              key: 'credentials',
              label: '密钥配置',
              children: (
                <OverlayScrollbarsComponent className="settings-tab-panel" options={settingsScrollbarOptions}>
                  <div className="settings-scroll-content">
                    <CredentialSettings ref={credentialSettingsRef} client={core.client.credentials} onStatusChange={setCredentialStatus} />
                  </div>
                </OverlayScrollbarsComponent>
              ),
            },
          ].filter(tab => settingsTabAvailable(core.capabilities, tab.key))}
          tabPlacement="start"
          onChange={(key) => {
            if (key !== 'general' && key !== 'messages' && key !== 'plugins' && key !== 'permissions' && key !== 'credentials') return
            selectSettingsTab(key)
          }}
        />
      </div>
    </section>
  )

  return (
    <ConfigProvider theme={uiThemeConfig(colorScheme)}>
      <App>
        <main
          className={panelActive ? 'launcher-surface is-panel-active' : 'launcher-surface'}
          data-color-scheme={colorScheme}
          onKeyDownCapture={(event) => { preventBrowserFind(event); launcherTabKeyDown(event) }}
        >
          {core.capabilities.platform === 'macos' && <div className="launcher-drag-region" data-tauri-drag-region title="拖动窗口" />}
          <div className="launcher-region launcher-section-region">
            {snapshot.view === 'launcher' ? (quicklinksPanel ?? panelLauncher ?? filePanel ?? launcher) : (publicPluginDetailView ?? settingsView)}
          </div>
            <div className={`launcher-region launcher-status-region${snapshot.view === 'settings' ? ' settings-status-region' : ''}`}>
              <div className={`status-region${macPluginFeedback?.message && !operationsError
                ? macPluginFeedback.tone === 'error' ? ' is-action-error' : macPluginFeedback.tone === 'success' ? ' is-action-success' : ''
                : snapshot.view === 'settings' && activeSettingsTab === 'credentials' && credentialStatus.message
                ? credentialStatus.tone === 'error' ? ' is-action-error' : credentialStatus.tone === 'success' ? ' is-action-success' : ''
                : snapshot.view === 'launcher' && !snapshot.shownNotice && !snapshot.status && panel?.feedback?.status.message
                ? panel.feedback.status.tone === 'error' ? ' is-action-error' : panel.feedback.status.tone === 'success' ? ' is-action-success' : ''
                : launcherResultsActive && snapshot.resultActionRerun
                ? snapshot.status === '已复制' ? ' is-action-success' : ' is-action-error'
                : launcherResultsActive && snapshot.executePending && statusResult?.actions ? ' is-action-pending' : ''}`}
                id="launcher-status-message" data-tauri-drag-region={core.capabilities.platform === 'macos' ? true : undefined} role="status" aria-live="polite" aria-atomic="true" title={status || undefined}>
                {status}
              </div>
              <div className="launcher-status-actions">
              {snapshot.view === 'settings' ? <AppVersion /> : null}
              <button ref={actionsButtonRef} className="launcher-operations-trigger" type="button" aria-label="操作 Ctrl+K"
                aria-haspopup="dialog" aria-expanded={panel ? undefined : operationsOpen} onPointerDown={(event) => event.preventDefault()} onClick={openOperations}>
                操作 <kbd>Ctrl+K</kbd>
              </button>
              </div>
            </div>
        </main>
      </App>
    </ConfigProvider>
  )
}
