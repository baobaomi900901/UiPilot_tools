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
import { ArrowLeft, Calculator, FolderSearch, PanelsTopLeft, Search, Settings, Star, X } from 'lucide-react'
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
import { OverlayScrollbarsComponent } from 'overlayscrollbars-react'

import type { LauncherCore } from './launcher-core'
import { MessageCenterPanel } from './message-center-panel'
import { bindNativeTextInput } from './native-input'
import { PluginIcon } from './plugin-icon'
import { PublicPluginPanel } from './public-plugin-panel'
import type {
  ControlKey,
  FileCategory,
  FileResultKind,
  ResultIconKind,
  SettingsTabKey,
  ThemePreference,
  WebSearchEngine,
} from './protocol'
import { resolveUiColorScheme, uiThemeConfig } from './ui-theme'
import {
  formatHotkeyDisplay,
  reduceHotkeyRecorder,
  type RecorderState,
} from './hotkey-recorder'

export interface LauncherViewProps {
  core: LauncherCore
  onReady: (result: 'ready' | 'failed') => void
}

interface BoundInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  core: LauncherCore
  control: ControlKey
  value: string
  onBound?: (input: HTMLInputElement) => void
  onUnbound?: (input: HTMLInputElement) => void
  onBindingFailed?: () => void
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
    let cleaned = false
    const cleanup = () => {
      if (cleaned) return
      cleaned = true
      try {
        onUnbound?.(input)
      } finally {
        try {
          unbind?.()
        } finally {
          core.retireControl(control)
        }
      }
    }
    try {
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

function composing(event: ReactKeyboardEvent): boolean {
  return event.nativeEvent.isComposing
}

function BuiltInResultIcon({ kind }: { kind: ResultIconKind }) {
  if (kind === 'calculator') {
    return (
      <span className="built-in-result-icon built-in-result-icon-calculator" data-result-icon-kind={kind}>
        <Calculator aria-hidden size={27} strokeWidth={1.8} />
      </span>
    )
  }
  const isFind = kind === 'find'
  return (
    <span
      className={`built-in-result-icon ${isFind ? 'built-in-result-icon-find' : 'built-in-result-icon-web-search'}`}
      data-result-icon-kind={kind}
    >
      {isFind ? (
        <FolderSearch aria-hidden size={26} strokeWidth={1.8} />
      ) : (
        <>
          <PanelsTopLeft aria-hidden size={25} strokeWidth={1.8} />
          <Search aria-hidden className="built-in-result-icon-badge" size={12} strokeWidth={2} />
        </>
      )}
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

interface HotkeyRecorderInputProps {
  core: LauncherCore
  value: string
  disabled?: boolean
  id?: string
  name?: string
}

function HotkeyRecorderInput({ core, value, disabled, id, name }: HotkeyRecorderInputProps): React.JSX.Element {
  const [recorderState, setRecorderState] = useState<RecorderState>(() => ({ status: 'idle', baseline: value }))

  useEffect(() => {
    setRecorderState((current) =>
      current.status === 'recording' ? current : { status: 'idle', baseline: value, pendingTap: undefined },
    )
  }, [value])

  const display =
    recorderState.status === 'recording' ? '按下快捷键…' : formatHotkeyDisplay(value)

  const startRecording = useCallback(() => {
    if (disabled) return
    setRecorderState((current) => reduceHotkeyRecorder(current, { type: 'start', baseline: value }).state)
  }, [disabled, value])

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    event.preventDefault()
    if (event.key === 'Escape') {
      setRecorderState((current) => reduceHotkeyRecorder(current, { type: 'cancel' }).state)
      return
    }
    setRecorderState((current) => {
      const result = reduceHotkeyRecorder(current, {
        type: 'keydown',
        key: event.key,
        code: event.code,
        ctrl: event.ctrlKey,
        alt: event.altKey,
        shift: event.shiftKey,
        meta: event.metaKey,
        repeat: event.repeat,
        nowMs: Date.now(),
      })
      if (result.commit) void core.saveHotkeyCanonical(result.commit)
      return result.state
    })
  }

  const handleBlur = () => {
    setRecorderState((current) => reduceHotkeyRecorder(current, { type: 'blur' }).state)
  }

  return (
    <Input
      readOnly
      value={display}
      id={id}
      name={name}
      disabled={disabled}
      onFocus={startRecording}
      onClick={startRecording}
      onKeyDown={handleKeyDown}
      onBlur={handleBlur}
    />
  )
}

export function LauncherView({ core, onReady }: LauncherViewProps): React.JSX.Element {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  const [scheme] = useState(() => window.matchMedia('(prefers-color-scheme: dark)'))
  const [systemDark, setSystemDark] = useState(scheme.matches)
  const colorScheme = resolveUiColorScheme(snapshot.theme, systemDark)
  const queryRef = useRef<HTMLInputElement | null>(null)
  const panelInputRef = useRef<HTMLInputElement | null>(null)
  const settingsTabsRef = useRef<HTMLDivElement>(null)
  const activatedPluginEpoch = useRef<number | undefined>(undefined)
  const activeSettingsTab = snapshot.settingsTab
  const optionRefs = useRef(new Map<number, HTMLElement>())
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

  const panel = snapshot.panel
  const reportPanelBound = useCallback((input: HTMLInputElement) => {
    panelInputRef.current = input
    input.focus()
    const caret = input.value.length
    input.setSelectionRange(caret, caret)
  }, [panel?.sessionEpoch, panel?.suffixControl])
  const reportPanelUnbound = useCallback((input: HTMLInputElement) => {
    if (panelInputRef.current === input) panelInputRef.current = null
  }, [])

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
    if (!snapshot.invocationId) return
    if (snapshot.view === 'launcher') {
      queryRef.current?.focus()
      queryRef.current?.select()
    } else {
      settingsTabsRef.current
        ?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
        ?.focus()
    }
  }, [snapshot.invocationId, snapshot.view, snapshot.viewEpoch])

  useLayoutEffect(() => {
    const selected = snapshot.results[snapshot.selectedIndex]
    if (snapshot.view === 'launcher' && selected) optionRefs.current.get(selected.key)?.scrollIntoView({ block: 'nearest' })
  }, [snapshot.results, snapshot.selectedIndex, snapshot.view])

  const file = snapshot.file
  const activeFileIndex =
    file?.selected === undefined ? -1 : file.results.findIndex((item) => item.fullPath === file.selected?.fullPath)

  useLayoutEffect(() => {
    if (snapshot.view === 'launcher' && file && activeFileIndex >= 0) {
      const selected = fileOptionRefs.current.get(activeFileIndex)
      scrollFileResultIntoView(document.getElementById('file-results'), selected)
    }
  }, [activeFileIndex, file, snapshot.view])

  const status =
    snapshot.shownNotice ||
    snapshot.status ||
    (snapshot.results.length
      ? `${snapshot.results.length} 个结果。${snapshot.results[snapshot.selectedIndex]?.title ?? ''}${
          snapshot.results[snapshot.selectedIndex]?.subtitle ? `，${snapshot.results[snapshot.selectedIndex]!.subtitle}` : ''
        }`
      : '')
  const messageBadgeCount = snapshot.messageCenter.status === 'unavailable'
    ? '!'
    : snapshot.messageCenter.status === 'ready'
      ? snapshot.messageCenter.unreadCount ?? 0
      : 0

  const queryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
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
    if (event.key === 'Escape' && !isComposing) event.preventDefault()
    core.keyDown(event.key as 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing)
  }
  const panelKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
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
  const settingsKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape') return
    const isComposing = composing(event)
    if (!isComposing) event.preventDefault()
    core.keyDown('Escape', isComposing)
  }

  const launcher = (
    <section className="launcher-view" aria-label="应用启动器">
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
        suffix={(
          <Tooltip title="设置">
            <span className="launcher-settings-control">
              <Badge
                className={`launcher-settings-badge${snapshot.messageCenter.status === 'unavailable' ? ' is-unavailable' : ''}`}
                count={messageBadgeCount}
                offset={[-2, 2]}
                overflowCount={99}
                size="small"
              >
                <Button
                  aria-label="打开设置"
                  className="launcher-settings-button"
                  disabled={!snapshot.invocationId}
                  icon={<Settings aria-hidden size={16} strokeWidth={1.8} />}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => core.navigate('settings')}
                  size="small"
                  type="text"
                />
              </Badge>
            </span>
          </Tooltip>
        )}
        onKeyDown={queryKeyDown}
        onBound={reportQueryBound}
        onBindingFailed={reportFailed}
      />

      <Spin spinning={snapshot.searchPending} size="small">
        <div className="launcher-result-surface">
          {snapshot.commandHint ? <div className="command-hint">{snapshot.commandHint}</div> : null}
          <div id="launcher-results" className="result-list" role="listbox" aria-label="搜索结果">
            {snapshot.results.map((item, index) => {
              const pluginActivation = item.pluginCompletion ?? item.panelActivation
              const row = (
                <div
                key={item.key}
                id={`launcher-result-${item.key}`}
                role="option"
                aria-selected={snapshot.selectedIndex === index}
                className={snapshot.selectedIndex === index ? 'result-row is-selected' : 'result-row'}
                onClick={() => core.activateResult(index)}
                onContextMenu={pluginActivation ? (event) => event.preventDefault() : undefined}
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
                    {pluginActivation?.favorite ? (
                      <Star
                        aria-label="常用"
                        className="result-favorite-star"
                        fill="currentColor"
                        size={14}
                        strokeWidth={1.8}
                      />
                    ) : null}
                  </span>
                  {item.subtitle ? <span className="result-subtitle">{item.subtitle}</span> : null}
                  {item.detail ? <span className="result-detail">{item.detail}</span> : null}
                </span>
              </div>
              )
              if (!pluginActivation) return row
              return (
                <Dropdown
                  key={item.key}
                  trigger={['contextMenu']}
                  menu={{
                    items: [{
                      key: 'favorite',
                      label: pluginActivation.favorite ? '取消常用' : '设为常用',
                      disabled: snapshot.favoriteMutationPending,
                    }],
                    onClick: () => core.setPluginFavorite(index, !pluginActivation.favorite),
                  }}
                  onOpenChange={(open) => {
                    if (open) core.openPluginContextMenu(index)
                    else core.closePluginContextMenu()
                  }}
                >
                  {row}
                </Dropdown>
              )
            })}
          </div>
        </div>
      </Spin>
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
      <div className="panel-input-row panel-input-shell">
        <div className="panel-command-tag" role="group" aria-label={`command ${panel.commandLabel}`}>
          <span>/{panel.commandLabel}</span>
          <Tooltip title="退出面板">
            <Button
              aria-label={`退出 ${panel.commandLabel} 面板`}
              disabled={panel.closePending}
              icon={<X aria-hidden size={14} strokeWidth={2} />}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => void core.closePanel()}
              size="small"
              type="text"
            />
          </Tooltip>
        </div>
        <label className="visually-hidden" htmlFor={`launcher-panel-suffix-${panel.suffixControl}`}>
          {panel.commandLabel} argument
        </label>
        <BoundInput
          className="panel-suffix-input"
          core={core}
          control={panel.suffixControl}
          value={panel.suffix}
          id={`launcher-panel-suffix-${panel.suffixControl}`}
          name={`launcher-panel-suffix-${panel.suffixControl}`}
          aria-label={`${panel.commandLabel} argument`}
          autoComplete="off"
          spellCheck={false}
          disabled={!snapshot.invocationId || panel.closePending}
          onKeyDown={panelKeyDown}
          onBound={reportPanelBound}
          onUnbound={reportPanelUnbound}
          onBindingFailed={reportFailed}
        />
      </div>
      <div className="panel-host-region" aria-label="插件面板内容" />
    </section>
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
            <Form.Item label="快捷键" htmlFor={`settings-hotkey-${settings.hotkey.key}`}>
              <HotkeyRecorderInput
                core={core}
                value={settings.hotkey.value}
                id={`settings-hotkey-${settings.hotkey.key}`}
                name={`settings-hotkey-${settings.hotkey.key}`}
                disabled={locked}
              />
            </Form.Item>
            <Checkbox
              checked={settings.autostart}
              disabled={locked}
              onChange={(event) => core.setAutostart(event.target.checked)}
            >
              开机启动
            </Checkbox>
            <Form.Item label="风格">
              <Select
                aria-label="风格"
                value={settings.theme}
                disabled={locked}
                options={themeOptions}
                onChange={(value: ThemePreference) => core.setThemePreference(value)}
              />
            </Form.Item>
            <Form.Item label="搜索引擎">
              <Select
                aria-label="搜索引擎"
                value={settings.webSearchEngine}
                disabled={locked}
                options={webSearchEngineOptions}
                onChange={(value: WebSearchEngine) => core.setWebSearchEngine(value)}
              />
            </Form.Item>
            <div className="settings-actions">
              <Popconfirm
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
              </Popconfirm>
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
    core.selectSettingsTab(key)
    if (key === 'plugins') {
      if (activatedPluginEpoch.current === snapshot.viewEpoch) return
      activatedPluginEpoch.current = snapshot.viewEpoch
      void core.activatePlugins()
    } else {
      activatedPluginEpoch.current = undefined
      core.deactivatePlugins()
    }
  }
  const pluginSettingsPanel = (
    <OverlayScrollbarsComponent className="settings-tab-panel settings-plugin-panel" options={settingsScrollbarOptions}>
      <div className="settings-scroll-content">
        <PublicPluginPanel client={core.client} />
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
  const settingsView = (
    <section className="settings-view" aria-label="设置" onKeyDown={settingsKeyDown}>
      <header className="settings-header">
        <div className="settings-title-group">
          <Tooltip title="返回主界面">
            <Button
              aria-label="返回主界面"
              disabled={snapshot.hidePending}
              icon={<ArrowLeft aria-hidden size={17} strokeWidth={1.8} />}
              onClick={() => core.navigate('launcher')}
              size="small"
              type="text"
            />
          </Tooltip>
          <h1>设置</h1>
        </div>
      </header>
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
          ]}
          tabPlacement="start"
          onChange={(key) => {
            if (key !== 'general' && key !== 'messages' && key !== 'plugins') return
            selectSettingsTab(key)
          }}
        />
      </div>
    </section>
  )

  return (
    <ConfigProvider theme={uiThemeConfig(colorScheme)}>
      <App>
        <main className="launcher-surface" data-color-scheme={colorScheme}>
          {snapshot.view === 'launcher' ? (panelLauncher ?? filePanel ?? launcher) : settingsView}
          <div className="status-region" role="status" aria-live="polite" aria-atomic="true">
            {status}
          </div>
        </main>
      </App>
    </ConfigProvider>
  )
}
