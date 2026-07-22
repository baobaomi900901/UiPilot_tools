import {
  App,
  Alert,
  Button,
  Checkbox,
  ConfigProvider,
  Form,
  Input,
  Spin,
  Switch,
  Tooltip,
  theme,
  type InputProps,
  type InputRef,
} from 'antd'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react'

import type { LauncherCore } from './launcher-core'
import { bindNativeTextInput } from './native-input'
import type { ControlKey, FileCategory, FileResultKind } from './protocol'
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
  onBound?: () => void
  onBindingFailed?: () => void
}

function BoundInput({ core, control, value, onBound, onBindingFailed, ...props }: BoundInputProps) {
  const ref = useRef<InputRef>(null)
  useLayoutEffect(() => {
    const input = ref.current?.input
    if (!input) {
      onBindingFailed?.()
      return () => core.retireControl(control)
    }
    try {
      const unbind = bindNativeTextInput(input, control, core.text)
      onBound?.()
      return () => {
        unbind()
        core.retireControl(control)
      }
    } catch {
      onBindingFailed?.()
      return () => core.retireControl(control)
    }
  }, [control, core, onBindingFailed, onBound])
  return <Input {...props} ref={ref} value={value} onChange={() => {}} />
}

function composing(event: ReactKeyboardEvent): boolean {
  return event.nativeEvent.isComposing
}

const fileCategories: readonly { value: FileCategory; label: string }[] = [
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
]

function fileSize(kind: FileResultKind, sizeBytes: string | null): string {
  if (kind === 'folder' || sizeBytes === null) return '--'
  return `${sizeBytes} B`
}

function fileModified(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
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
  const [dark, setDark] = useState(scheme.matches)
  const queryRef = useRef<HTMLInputElement | null>(null)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const optionRefs = useRef(new Map<number, HTMLElement>())
  const fileOptionRefs = useRef(new Map<number, HTMLElement>())
  const ready = useRef(false)

  useEffect(() => {
    const update = (event: MediaQueryListEvent) => setDark(event.matches)
    scheme.addEventListener('change', update)
    return () => scheme.removeEventListener('change', update)
  }, [scheme])

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
  const reportQueryBound = useCallback(() => {
    queryRef.current = document.getElementById(`launcher-query-${snapshot.queryControl}`) as HTMLInputElement | null
    if (snapshot.view === 'launcher' && snapshot.invocationId) queryRef.current?.focus()
    reportReady()
  }, [reportReady, snapshot.invocationId, snapshot.queryControl, snapshot.view])

  useLayoutEffect(() => {
    if (!snapshot.invocationId) return
    if (snapshot.view === 'launcher') {
      queryRef.current?.focus()
      queryRef.current?.select()
    } else {
      headingRef.current?.focus()
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

  const queryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.altKey && (event.key.toLowerCase() === 's' || event.key.toLowerCase() === 'p')) {
      if (composing(event) || !file) return
      event.preventDefault()
      if (event.key.toLowerCase() === 's') {
        core.setFileSort(file.sort === 'modifiedDesc' ? 'modifiedAsc' : 'modifiedDesc')
      } else {
        core.setFilePreviewEnabled(!file.previewEnabled)
      }
      return
    }
    if (event.key === 'Tab' && file && !composing(event)) {
      event.preventDefault()
      const current = fileCategories.findIndex((category) => category.value === file.category)
      const offset = event.shiftKey ? -1 : 1
      const nextIndex = (current + offset + fileCategories.length) % fileCategories.length
      core.setFileCategory(fileCategories[nextIndex]!.value)
      return
    }
    if (!['ArrowUp', 'ArrowDown', 'Enter', 'Escape'].includes(event.key)) return
    const isComposing = composing(event)
    if (event.key === 'Escape' && !isComposing) event.preventDefault()
    core.keyDown(event.key as 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Escape', isComposing)
  }
  const settingsKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
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
        onKeyDown={queryKeyDown}
        onBound={reportQueryBound}
        onBindingFailed={reportFailed}
      />
      <Spin spinning={snapshot.searchPending} size="small">
        <div id="launcher-results" className="result-list" role="listbox" aria-label="搜索结果">
          {snapshot.results.map((item, index) => (
              <div
                key={item.key}
                id={`launcher-result-${item.key}`}
                role="option"
                aria-selected={snapshot.selectedIndex === index}
                className={snapshot.selectedIndex === index ? 'result-row is-selected' : 'result-row'}
                ref={(element) => {
                  if (element) optionRefs.current.set(item.key, element)
                  else optionRefs.current.delete(item.key)
                }}
              >
                <span className="result-icon" aria-hidden="true">
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
                </span>
                <span className="result-copy">
                  <span className="result-title">{item.title}</span>
                  {item.subtitle ? <span className="result-subtitle">{item.subtitle}</span> : null}
                </span>
              </div>
            ))}
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
      <div className="file-category-strip" role="tablist" aria-label="文件类型">
        {fileCategories.map((category, index) => (
          <Button
            key={category.value}
            role="tab"
            tabIndex={file.category === category.value ? 0 : -1}
            aria-selected={file.category === category.value}
            type={file.category === category.value ? 'primary' : 'default'}
            onClick={() => core.setFileCategory(category.value)}
            onKeyDown={(event) => {
              if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return
              event.preventDefault()
              const offset = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 1
              const nextIndex =
                event.key === 'Home'
                  ? 0
                  : event.key === 'End'
                    ? fileCategories.length - 1
                    : (index + offset + fileCategories.length) % fileCategories.length
              core.setFileCategory(fileCategories[nextIndex]!.value)
            }}
          >
            {category.label}
          </Button>
        ))}
      </div>
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
        <span>共 {file.total} 条结果</span>
        <Button onClick={() => core.setFileSort(file.sort === 'modifiedDesc' ? 'modifiedAsc' : 'modifiedDesc')}>
          {file.sort === 'modifiedDesc' ? '修改时间 ↓' : '修改时间 ↑'}
        </Button>
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

  const settings = snapshot.settings
  const busy = settings?.operation !== undefined
  const locked = busy || settings?.readOnly === true
  const settingsView = (
    <section className="settings-view" aria-label="设置">
      <header className="settings-header">
        <h1 ref={headingRef} tabIndex={-1}>
          设置
        </h1>
        <Button aria-label="关闭" disabled={snapshot.hidePending} onClick={() => void core.requestHide()}>
          关闭
        </Button>
      </header>
      {!settings ? (
        <div className="settings-loading">
          {snapshot.status ? <Button onClick={() => void core.reloadSettings()}>重新加载设置</Button> : <Spin size="small" />}
        </div>
      ) : (
        <Form component="div" layout="vertical" className="settings-form">
          <Form.Item label="快捷键" htmlFor={`settings-hotkey-${settings.hotkey.key}`}>
            <HotkeyRecorderInput
              core={core}
              value={settings.hotkey.value}
              id={`settings-hotkey-${settings.hotkey.key}`}
              name={`settings-hotkey-${settings.hotkey.key}`}
              disabled={locked}
            />
          </Form.Item>
          <Form.Item label="Research ID" htmlFor={`settings-research-${settings.researchId.key}`}>
            <BoundInput
              core={core}
              control={settings.researchId.key}
              value={settings.researchId.value}
              id={`settings-research-${settings.researchId.key}`}
              name={`settings-research-${settings.researchId.key}`}
              maxLength={64}
              pattern="[A-Za-z0-9_-]{1,64}"
              disabled={locked}
              onKeyDown={settingsKeyDown}
            />
          </Form.Item>
          <Checkbox checked={settings.autostart} disabled={locked} onChange={(event) => core.setAutostart(event.target.checked)}>
            开机启动
          </Checkbox>
          {settings.clearConfirmation ? (
            <Alert
              role="group"
              type="warning"
              showIcon={false}
              message="确认清除本地验证数据？"
              action={
                <span className="confirmation-actions">
                  <Button disabled={busy} onClick={() => void core.confirmClearValidation()}>
                    确认清除
                  </Button>
                  <Button disabled={busy} onClick={() => core.cancelClearValidation()}>
                    取消
                  </Button>
                </span>
              }
            />
          ) : null}
          <div className="settings-actions">
            <Button type="primary" disabled={locked} loading={settings.operation === 'save'} onClick={() => void core.saveSettings()}>
              保存
            </Button>
            <Button disabled={busy} loading={settings.operation === 'load'} onClick={() => void core.reloadSettings()}>
              重新加载设置
            </Button>
            <Button disabled={locked} loading={settings.operation === 'rescan'} onClick={() => void core.rescanApps()}>
              重新扫描
            </Button>
            <Button disabled={busy} loading={settings.operation === 'export'} onClick={() => void core.exportValidation()}>
              导出验证数据
            </Button>
            <Button disabled={busy} loading={settings.operation === 'clear'} onClick={() => core.beginClearValidation()}>
              清除验证数据
            </Button>
          </div>
        </Form>
      )}
    </section>
  )

  return (
    <ConfigProvider theme={{ algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm, token: { motion: false } }}>
      <App>
        <main className="launcher-surface" data-color-scheme={dark ? 'dark' : 'light'}>
          {snapshot.view === 'launcher' ? filePanel ?? launcher : settingsView}
          <div className="status-region" role="status" aria-live="polite" aria-atomic="true">
            {status}
          </div>
        </main>
      </App>
    </ConfigProvider>
  )
}
