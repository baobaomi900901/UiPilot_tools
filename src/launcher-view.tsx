import {
  App,
  Alert,
  Button,
  Checkbox,
  ConfigProvider,
  Form,
  Input,
  Spin,
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
import type { ControlKey } from './protocol'

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

export function LauncherView({ core, onReady }: LauncherViewProps): React.JSX.Element {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  const [scheme] = useState(() => window.matchMedia('(prefers-color-scheme: dark)'))
  const [dark, setDark] = useState(scheme.matches)
  const queryRef = useRef<HTMLInputElement | null>(null)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const optionRefs = useRef(new Map<number, HTMLElement>())
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

  const status =
    snapshot.shownNotice ||
    snapshot.status ||
    (snapshot.results.length
      ? `${snapshot.results.length} 个结果。${snapshot.results[snapshot.selectedIndex]?.title ?? ''}${
          snapshot.results[snapshot.selectedIndex]?.subtitle ? `，${snapshot.results[snapshot.selectedIndex]!.subtitle}` : ''
        }`
      : '')

  const queryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
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
        onBound={() => {
          queryRef.current = document.getElementById(`launcher-query-${snapshot.queryControl}`) as HTMLInputElement | null
          reportReady()
        }}
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
                <span className="app-mark" aria-hidden="true" />
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
          {snapshot.status ? null : <Spin size="small" />}
          <Button onClick={() => void core.reloadSettings()}>重新加载设置</Button>
        </div>
      ) : (
        <Form component="div" layout="vertical" className="settings-form">
          <Form.Item label="快捷键" htmlFor={`settings-hotkey-${settings.hotkey.key}`}>
            <BoundInput
              core={core}
              control={settings.hotkey.key}
              value={settings.hotkey.value}
              id={`settings-hotkey-${settings.hotkey.key}`}
              name={`settings-hotkey-${settings.hotkey.key}`}
              disabled={locked}
              onKeyDown={settingsKeyDown}
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
          <div className="application-list">
            {settings.applications.map((application) => (
              <section key={application.key} className="application-row">
                <div className="application-heading">
                  <span className="app-mark" aria-hidden="true" />
                  <span>{application.displayName}</span>
                  <Button disabled={locked} onClick={() => core.addAlias(application.key)}>
                    添加别名
                  </Button>
                </div>
                <div className="alias-list">
                  {application.aliases.map((alias, index) => {
                    const id = `settings-alias-${alias.key}`
                    return (
                      <Form.Item key={alias.key} label={`别名 ${index + 1}`} htmlFor={id}>
                        <div className="alias-row">
                          <BoundInput
                            core={core}
                            control={alias.key}
                            value={alias.value}
                            id={id}
                            name={id}
                            disabled={locked}
                            onKeyDown={settingsKeyDown}
                          />
                          <Button disabled={locked} onClick={() => core.removeAlias(application.key, alias.key)}>
                            删除
                          </Button>
                        </div>
                      </Form.Item>
                    )
                  })}
                </div>
              </section>
            ))}
          </div>
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
          {snapshot.view === 'launcher' ? launcher : settingsView}
          <div className="status-region" role="status" aria-live="polite" aria-atomic="true">
            {status}
          </div>
        </main>
      </App>
    </ConfigProvider>
  )
}
