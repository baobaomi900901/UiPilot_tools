import { CloseOutlined, PushpinFilled, PushpinOutlined } from '@ant-design/icons'
import { App, Button, ConfigProvider, Input, Spin, Switch, Tooltip, theme } from 'antd'
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent as ReactKeyboardEvent } from 'react'

import { FIND_CATEGORY_ORDER, type FindCore } from './find-core'
import type { FileCategory, FileResultKind, ThemePreference } from './protocol'

const CATEGORY_LABELS: Record<FileCategory, string> = {
  all: '全部', folder: '文件夹', excel: 'Excel', word: 'Word', ppt: 'PPT',
  pdf: 'PDF', image: '图片', video: '视频', audio: '音频', archive: '压缩包',
}

function colorScheme(preference: ThemePreference, systemDark: boolean): 'light' | 'dark' {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference
}

function fileSize(kind: FileResultKind, sizeBytes: string | null): string {
  return kind === 'folder' ? '文件夹' : sizeBytes ?? '未知'
}

function modified(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}

export interface FindViewProps {
  core: FindCore
}

export function FindView({ core }: FindViewProps): React.JSX.Element {
  const snapshot = useSyncExternalStore(core.subscribe, core.getSnapshot, core.getSnapshot)
  const [media] = useState(() => window.matchMedia('(prefers-color-scheme: dark)'))
  const [systemDark, setSystemDark] = useState(media.matches)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const optionRefs = useRef(new Map<number, HTMLElement>())
  const disabled = !snapshot.ready || snapshot.executePending
  const selected = snapshot.selectedIndex >= 0 ? snapshot.results[snapshot.selectedIndex] : undefined

  useEffect(() => {
    const changed = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    media.addEventListener('change', changed)
    void core.start()
    return () => media.removeEventListener('change', changed)
  }, [core, media])

  useEffect(() => {
    if (snapshot.ready) inputRef.current?.focus()
  }, [snapshot.ready, snapshot.invocationId])

  useEffect(() => {
    const onWindowKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
      if (event.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return
      event.preventDefault()
      event.stopPropagation()
      core.keyDown(event.key, false)
    }
    window.addEventListener('keydown', onWindowKeyDown, true)
    return () => window.removeEventListener('keydown', onWindowKeyDown, true)
  }, [core])

  useLayoutEffect(() => {
    const option = snapshot.selectedIndex >= 0 ? optionRefs.current.get(snapshot.selectedIndex) : undefined
    if (typeof option?.scrollIntoView === 'function') option.scrollIntoView({ block: 'nearest' })
  }, [snapshot.selectedIndex])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Tab' && !event.ctrlKey && !event.altKey && !event.metaKey) {
      if (event.nativeEvent.isComposing) return
      event.preventDefault()
      core.cycleCategory(event.shiftKey ? 'previous' : 'next')
      return
    }
    if (!['Enter', 'Escape'].includes(event.key)) return
    if (event.key === 'Escape' && !event.nativeEvent.isComposing) event.preventDefault()
    core.keyDown(event.key as 'Enter' | 'Escape', event.nativeEvent.isComposing)
  }

  return (
    <ConfigProvider theme={{ algorithm: colorScheme(snapshot.theme, systemDark) === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm, token: { motion: false } }}>
      <App>
        <main className="find-surface" data-color-scheme={colorScheme(snapshot.theme, systemDark)}>
          <header className="find-header">
            <span className="find-drag-handle" aria-hidden="true" />
            <Input
              ref={(node) => { inputRef.current = node?.input ?? null }}
              value={snapshot.query}
              placeholder="搜索文件"
              autoComplete="off"
              spellCheck={false}
              disabled={disabled || !snapshot.invocationId}
              role="combobox"
              aria-autocomplete="list"
              aria-controls="find-results"
              aria-expanded={snapshot.results.length > 0}
              aria-activedescendant={snapshot.selectedIndex >= 0 ? `find-result-${snapshot.selectedIndex}` : undefined}
              onChange={(event) => core.setQuery(event.currentTarget.value)}
              onKeyDown={onKeyDown}
            />
            <Tooltip title={snapshot.pinned ? '取消固定' : '固定窗口'}>
              <Button
                className={snapshot.pinned ? 'find-icon-button is-selected' : 'find-icon-button'}
                type="text"
                icon={snapshot.pinned ? <PushpinFilled /> : <PushpinOutlined />}
                aria-label={snapshot.pinned ? '取消固定' : '固定窗口'}
                aria-pressed={snapshot.pinned}
                disabled={disabled || snapshot.pinPending || !snapshot.invocationId}
                onClick={() => core.setPinned(!snapshot.pinned)}
              />
            </Tooltip>
            <Tooltip title="关闭">
              <Button
                className="find-icon-button"
                type="text"
                icon={<CloseOutlined />}
                aria-label="关闭"
                disabled={disabled || snapshot.hidePending || !snapshot.invocationId}
                onClick={() => void core.requestHide(true)}
              />
            </Tooltip>
          </header>

          <nav
            className="find-categories file-categories"
            aria-label="文件类型"
            onMouseDown={(event) => {
              event.preventDefault()
              inputRef.current?.focus()
            }}
          >
            {FIND_CATEGORY_ORDER.map((category) => (
              <button
                key={category}
                type="button"
                className={snapshot.category === category ? 'find-category file-category is-selected' : 'find-category file-category'}
                aria-pressed={snapshot.category === category}
                tabIndex={-1}
                disabled={disabled || !snapshot.invocationId}
                onClick={() => core.setCategory(category)}
              >
                {CATEGORY_LABELS[category]}
              </button>
            ))}
          </nav>

          <Spin spinning={snapshot.searchPending} size="small" wrapperClassName="find-results-spin">
            <div id="find-results" className="result-list find-results" role="listbox" aria-label="文件结果">
              {snapshot.results.map((item, index) => (
                <div
                  key={item.key}
                  id={`find-result-${index}`}
                  role="option"
                  aria-selected={snapshot.selectedIndex === index}
                  className={snapshot.selectedIndex === index ? 'result-row file-result-row is-selected' : 'result-row file-result-row'}
                  ref={(element) => {
                    if (element) optionRefs.current.set(index, element)
                    else optionRefs.current.delete(index)
                  }}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => core.select(index)}
                  onDoubleClick={() => {
                    core.select(index)
                    core.keyDown('Enter', false)
                  }}
                >
                  <span className="result-icon file-kind-mark" aria-hidden="true">{item.kind === 'folder' ? '□' : '◇'}</span>
                  <span className="result-copy">
                    <Tooltip title={item.name}><span className="result-title">{item.name}</span></Tooltip>
                    <span className="result-subtitle">{item.fullPath}</span>
                  </span>
                </div>
              ))}
            </div>
          </Spin>

          <aside className="find-preview file-preview" aria-label="文件预览">
            {snapshot.previewEnabled && selected ? (
              <>
                <Tooltip title={selected.name}><h2>{selected.name}</h2></Tooltip>
                <dl>
                  <dt>类型</dt><dd>{selected.kind === 'folder' ? '文件夹' : '文件'}</dd>
                  <dt>大小</dt><dd>{fileSize(selected.kind, selected.sizeBytes)}</dd>
                  <dt>修改时间</dt><dd>{modified(selected.modifiedUtc)}</dd>
                  <dt>完整路径</dt><dd>{selected.fullPath}</dd>
                </dl>
              </>
            ) : <p>{snapshot.previewEnabled ? '请选择文件' : '预览已关闭'}</p>}
          </aside>

          <footer className="find-footer file-toolbar">
            <span>{snapshot.indexStatus === 'building' ? `正在索引，已有 ${snapshot.total} 条结果` : `共 ${snapshot.total} 条结果`}</span>
            <Switch
              aria-label="文件预览"
              checked={snapshot.previewEnabled}
              loading={snapshot.previewPending}
              disabled={disabled || snapshot.previewPending || !snapshot.invocationId}
              onChange={(checked) => core.setPreviewEnabled(checked)}
            />
          </footer>
          <div className="status-region" role="status" aria-live="polite" aria-atomic="true">{snapshot.status}</div>
        </main>
      </App>
    </ConfigProvider>
  )
}
