import { App, Button, ConfigProvider, Input, Spin, Tooltip } from 'antd'
import { File, FileSearch, Folder, ImageOff, Pin, X } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent as ReactKeyboardEvent } from 'react'

import { FIND_CATEGORY_ORDER, type FindCore } from './find-core'
import type { FileCategory, FileResultKind } from './protocol'
import { resolveUiColorScheme, uiThemeConfig } from './ui-theme'
import { createContextActionsMenu, type ContextActionsMenu } from './context-actions-ui'
import { isWindowPinShortcut } from './window-pin-shortcut'

const CATEGORY_LABELS: Record<FileCategory, string> = {
  all: '全部', folder: '文件夹', excel: 'Excel', word: 'Word', ppt: 'PPT',
  pdf: 'PDF', image: '图片', video: '视频', audio: '音频', archive: '压缩包',
}

const FILE_SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const

function fileType(kind: FileResultKind, name: string): string {
  if (kind === 'folder') return '文件夹'
  const dot = name.lastIndexOf('.')
  if (dot <= 0 || dot === name.length - 1) return '文件'
  const extension = name.slice(dot + 1)
  return extension.length <= 12 ? `${extension.toUpperCase()} 文件` : '文件'
}

function fileSize(kind: FileResultKind, sizeBytes: string | null): string {
  if (kind === 'folder') return '--'
  if (sizeBytes === null || !/^\d+$/.test(sizeBytes)) return '未知'

  const bytes = BigInt(sizeBytes)
  let unitIndex = 0
  let divisor = 1n
  while (bytes >= divisor * 1024n && unitIndex < FILE_SIZE_UNITS.length - 1) {
    divisor *= 1024n
    unitIndex += 1
  }
  if (unitIndex === 0) return `${bytes} ${FILE_SIZE_UNITS[unitIndex]}`

  const hundredths = (bytes * 100n + divisor / 2n) / divisor
  const whole = hundredths / 100n
  const fraction = (hundredths % 100n).toString().padStart(2, '0').replace(/0+$/, '')
  return `${whole}${fraction ? `.${fraction}` : ''} ${FILE_SIZE_UNITS[unitIndex]}`
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
  const [failedThumbnail, setFailedThumbnail] = useState<string>()
  const inputRef = useRef<HTMLInputElement | null>(null)
  const menuRef = useRef<ContextActionsMenu | null>(null)
  const menuOrigin = useRef<{ element: Element | null; invocationId: string } | null>(null)
  const restoreMenuFocus = useRef(false)
  const [actionsOpen, setActionsOpen] = useState(false)
  const optionRefs = useRef(new Map<number, HTMLElement>())
  const disabled = !snapshot.ready || snapshot.executePending
  const selected = snapshot.selectedIndex >= 0 ? snapshot.results[snapshot.selectedIndex] : undefined
  const thumbnailKey = selected && snapshot.thumbnailDataUrl
    ? `${selected.key}\u0000${snapshot.thumbnailDataUrl}`
    : undefined
  const showThumbnail = thumbnailKey !== undefined && failedThumbnail !== thumbnailKey
  const scheme = resolveUiColorScheme(snapshot.theme, systemDark)

  useLayoutEffect(() => {
    const previous = document.documentElement.dataset.colorScheme
    document.documentElement.dataset.colorScheme = scheme
    return () => {
      if (previous === undefined) delete document.documentElement.dataset.colorScheme
      else document.documentElement.dataset.colorScheme = previous
    }
  }, [scheme])

  const cycleCategory = useCallback((direction: 'next' | 'previous') => {
    const state = core.getSnapshot()
    if (!state.ready || !state.invocationId || state.executePending || state.hidePending) return
    core.cycleCategory(direction)
    const index = FIND_CATEGORY_ORDER.indexOf(core.getSnapshot().category)
    inputRef.current?.closest('.find-surface')
      ?.querySelectorAll<HTMLButtonElement>('.find-category')[index]?.focus({ preventScroll: true })
  }, [core])

  const openActions = useCallback(() => {
    const menu = menuRef.current
    if (!menu) return
    if (menu.isOpen) { menu.close(); return }
    const owner = core.getSnapshot()
    if (!owner.ready || !owner.invocationId || owner.hidePending) return
    const origin = document.activeElement
    menuOrigin.current = { element: origin, invocationId: owner.invocationId }
    const fromInput = origin === inputRef.current
    const item = owner.results[owner.selectedIndex]
    const current = () => {
      const now = core.getSnapshot()
      return now.ready && !now.hidePending && now.invocationId === owner.invocationId &&
        now.query === owner.query && now.category === owner.category &&
        now.results[now.selectedIndex]?.key === item?.key
    }
    const busy = owner.executePending ? '正在打开文件位置' : undefined
    menu.open({
      context: fromInput ? [{
        id: 'find-search', title: '搜索', shortcut: 'Enter', icon: 'search',
        disabledReason: busy ?? (owner.searchPending ? '正在搜索' : !owner.query ? '请输入搜索内容' : undefined),
        run() { if (current()) { inputRef.current?.focus(); core.submitSearch() } },
      }] : [{
        id: 'find-reveal', title: '打开所在的文件夹', shortcut: 'Enter', icon: 'arrow',
        disabledReason: busy ?? (owner.searchPending ? '正在搜索' : !item ? '没有选中的文件' : undefined),
        run() { if (current() && item) core.keyDown('Enter', false) },
      }],
      global: [
        { id: 'find-category', title: '下一分类', shortcut: 'Tab', icon: 'arrow-right-to-line',
          disabledReason: busy,
          run() { if (current()) cycleCategory('next') } },
        { id: 'find-previous-category', title: '上一分类', shortcut: 'Shift+Tab', icon: 'arrow-left-to-line',
          disabledReason: busy,
          run() { if (current()) cycleCategory('previous') } },
        { id: 'find-pin', title: owner.pinned ? '取消固定窗口' : '固定窗口', shortcut: 'Ctrl+T', icon: 'pin',
          disabledReason: busy ?? (owner.pinPending ? '正在更新固定状态' : undefined),
          run() { if (current()) core.setPinned(!owner.pinned) } },
        { id: 'find-preview', title: owner.previewEnabled ? '关闭预览' : '打开预览', icon: 'eye',
          disabledReason: busy ?? (owner.previewPending ? '正在更新预览状态' : undefined),
          run() { if (current()) core.setPreviewEnabled(!owner.previewEnabled) } },
        { id: 'find-input', title: '返回搜索框', shortcut: 'Ctrl+F', icon: 'search',
          run() { if (current()) inputRef.current?.focus() } },
      ],
    }, { origin, bottom: 42, onClose: () => setActionsOpen(false) })
    setActionsOpen(menu.isOpen)
  }, [core, cycleCategory])

  useEffect(() => {
    const menu = createContextActionsMenu(document)
    menuRef.current = menu
    const stamp = () => {
      const state = core.getSnapshot()
      return JSON.stringify([state.ready, state.invocationId, state.query, state.category,
        state.results[state.selectedIndex]?.key, state.searchPending, state.executePending,
        state.hidePending, state.pinned, state.pinPending, state.previewEnabled, state.previewPending])
    }
    let previous = stamp()
    const unsubscribe = core.subscribe(() => {
      const next = stamp()
      if (next !== previous && menu.isOpen) {
        restoreMenuFocus.current = true
        menu.close(false)
      }
      previous = next
    })
    return () => { unsubscribe(); menu.close(false); menuRef.current = null }
  }, [core])

  useLayoutEffect(() => {
    if (!restoreMenuFocus.current) return
    const origin = menuOrigin.current
    if (!origin || !snapshot.ready || snapshot.hidePending || origin.invocationId !== snapshot.invocationId) {
      restoreMenuFocus.current = false
      return
    }
    if (snapshot.executePending) return
    restoreMenuFocus.current = false
    const target = origin.element
    if (target instanceof HTMLElement && target.isConnected && !target.matches(':disabled')) {
      target.focus({ preventScroll: true })
    } else inputRef.current?.focus({ preventScroll: true })
  }, [snapshot])

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
      const menuShortcut = event.key.toLowerCase() === 'k' && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey
      if (menuShortcut && (event.repeat || event.isComposing)) {
        event.preventDefault()
        event.stopPropagation()
        return
      }
      // Shared menu must receive arrows/Escape before the background Find list.
      if (menuRef.current?.isOpen) return
      if (event.defaultPrevented) return
      if (event.key === 'Tab' && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
        event.preventDefault()
        event.stopPropagation()
        if (!event.repeat) cycleCategory(event.shiftKey ? 'previous' : 'next')
        return
      }
      if (menuShortcut) {
        if (event.isComposing) return
        event.preventDefault()
        event.stopPropagation()
        if (!event.repeat) openActions()
        return
      }
      if (isWindowPinShortcut(event)) {
        event.preventDefault()
        event.stopPropagation()
        if (!event.repeat) core.setPinned(!core.getSnapshot().pinned)
        return
      }
      if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'f') {
        event.preventDefault()
        event.stopPropagation()
        inputRef.current?.focus()
        return
      }
      if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
      if (event.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return
      event.preventDefault()
      event.stopPropagation()
      core.keyDown(event.key, false)
      optionRefs.current.get(core.getSnapshot().selectedIndex)?.focus({ preventScroll: true })
    }
    window.addEventListener('keydown', onWindowKeyDown, true)
    return () => window.removeEventListener('keydown', onWindowKeyDown, true)
  }, [core, openActions, cycleCategory])

  useLayoutEffect(() => {
    const option = snapshot.selectedIndex >= 0 ? optionRefs.current.get(snapshot.selectedIndex) : undefined
    if (typeof option?.scrollIntoView === 'function') option.scrollIntoView({ block: 'nearest' })
  }, [snapshot.selectedIndex])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (!['Enter', 'Escape'].includes(event.key) || event.nativeEvent.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return
    event.preventDefault()
    if (event.repeat) return
    if (event.key === 'Enter') core.submitSearch()
    else core.keyDown('Escape', false)
  }

  return (
    <ConfigProvider theme={uiThemeConfig(scheme)}>
      <App>
        <main
          className={snapshot.previewEnabled ? 'find-surface' : 'find-surface is-preview-collapsed'}
          data-color-scheme={scheme}
        >
          <div className="find-region find-header-region">
            <header className="find-header">
            <span className="find-drag-handle" aria-hidden="true" />
            <Input
              ref={(node) => { inputRef.current = node?.input ?? null }}
              value={snapshot.query}
              placeholder="搜索文件"
              aria-label="搜索文件"
              autoComplete="off"
              spellCheck={false}
              tabIndex={-1}
              disabled={disabled || !snapshot.invocationId}
              role="combobox"
              aria-autocomplete="list"
              aria-controls="find-results"
              aria-expanded={snapshot.results.length > 0}
              aria-activedescendant={snapshot.selectedIndex >= 0 ? `find-result-${snapshot.selectedIndex}` : undefined}
              onChange={(event) => core.setQuery(event.currentTarget.value)}
              onKeyDown={onKeyDown}
            />
            <Tooltip title={`${snapshot.pinned ? '取消固定' : '固定窗口'} (Ctrl+T)`}>
              <Button
                className={snapshot.pinned ? 'find-icon-button is-selected' : 'find-icon-button'}
                type="text"
                icon={(
                  <Pin
                    aria-hidden
                    fill={snapshot.pinned ? 'currentColor' : 'none'}
                    size={17}
                    strokeWidth={1.8}
                  />
                )}
                aria-label={snapshot.pinned ? '取消固定' : '固定窗口'}
                aria-pressed={snapshot.pinned}
                aria-keyshortcuts="Control+t"
                tabIndex={-1}
                disabled={disabled || snapshot.pinPending || !snapshot.invocationId}
                onClick={() => core.setPinned(!snapshot.pinned)}
              />
            </Tooltip>
            <Tooltip title="关闭">
              <Button
                className="find-icon-button"
                type="text"
                icon={<X aria-hidden size={17} strokeWidth={1.8} />}
                aria-label="关闭"
                tabIndex={-1}
                disabled={disabled || snapshot.hidePending || !snapshot.invocationId}
                onClick={() => void core.requestHide(true)}
              />
            </Tooltip>
            </header>
          </div>

          <div className="find-region find-categories-region">
            <nav
              className="find-categories file-categories"
              aria-label="文件类型"
            >
              {FIND_CATEGORY_ORDER.map((category) => (
                <button
                  key={category}
                  type="button"
                  className={snapshot.category === category ? 'find-category file-category is-selected' : 'find-category file-category'}
                  aria-pressed={snapshot.category === category}
                  disabled={disabled || !snapshot.invocationId}
                  onClick={() => core.setCategory(category)}
                  onKeyDown={(event) => {
                    if (event.nativeEvent.isComposing || event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return
                    if (event.key === 'Enter' || event.key === 'Escape') {
                      event.preventDefault()
                      event.stopPropagation()
                      if (!event.repeat) core.keyDown(event.key, false)
                      return
                    }
                  }}
                >
                  {CATEGORY_LABELS[category]}
                </button>
              ))}
            </nav>
          </div>

          <div className="find-region find-results-region">
            <Spin spinning={snapshot.searchPending} size="small" wrapperClassName="find-results-spin">
              <div id="find-results" className="result-list find-results" role="listbox" aria-label="文件结果">
                {snapshot.results.map((item, index) => (
                  <div
                    key={item.key}
                    id={`find-result-${index}`}
                    role="option"
                    tabIndex={-1}
                    onKeyDown={(event) => {
                      if (event.nativeEvent.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return
                      if (event.key !== 'Enter' && event.key !== 'Escape') return
                      event.preventDefault()
                      event.stopPropagation()
                      if (!event.repeat) core.keyDown(event.key, false)
                    }}
                    aria-selected={snapshot.selectedIndex === index}
                    className={snapshot.selectedIndex === index ? 'result-row file-result-row is-selected' : 'result-row file-result-row'}
                    ref={(element) => {
                      if (element) optionRefs.current.set(index, element)
                      else optionRefs.current.delete(index)
                    }}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={(event) => {
                      core.select(index)
                      if (core.getSnapshot().selectedIndex === index) event.currentTarget.focus({ preventScroll: true })
                    }}
                    onDoubleClick={() => {
                      core.select(index)
                      core.keyDown('Enter', false)
                    }}
                  >
                    <span className={`result-icon file-kind-mark is-${item.kind}`} aria-hidden="true">
                      {item.kind === 'folder'
                        ? <Folder size={20} strokeWidth={1.8} />
                        : <File size={20} strokeWidth={1.8} />}
                    </span>
                    <span className="result-copy">
                      <Tooltip title={item.name}><span className="result-title">{item.name}</span></Tooltip>
                      <span className="result-subtitle">{item.fullPath}</span>
                    </span>
                  </div>
                ))}
              </div>
            </Spin>
          </div>

          <div className="find-region find-preview-region">
            <aside
              className="find-preview file-preview"
              aria-label="文件预览"
              aria-hidden={!snapshot.previewEnabled}
            >
              {selected ? <>
                <Tooltip title={selected.name}><h2>{selected.name}</h2></Tooltip>
                <div className="find-preview-media">
                  {snapshot.thumbnailPending ? <Spin size="small" /> : null}
                  {!snapshot.thumbnailPending && showThumbnail ? (
                    <img
                      className="find-preview-thumbnail"
                      src={snapshot.thumbnailDataUrl}
                      alt={`${selected.name} 缩略图`}
                      draggable={false}
                      onError={() => setFailedThumbnail(thumbnailKey)}
                    />
                  ) : null}
                  {!snapshot.thumbnailPending && !showThumbnail ? (
                    <div className="find-preview-placeholder">
                      <ImageOff aria-hidden size={28} strokeWidth={1.6} />
                      <span>无预览图片</span>
                    </div>
                  ) : null}
                </div>
                <dl>
                  <dt>类型</dt><dd>{fileType(selected.kind, selected.name)}</dd>
                  <dt>大小</dt><dd>{fileSize(selected.kind, selected.sizeBytes)}</dd>
                  <dt>修改时间</dt><dd>{modified(selected.modifiedUtc)}</dd>
                </dl>
                </> : (
                  <div className="find-preview-empty">
                    <span className="find-preview-empty-icon" aria-hidden="true">
                      <FileSearch size={24} strokeWidth={1.6} />
                    </span>
                    <p>未选择文件</p>
                  </div>
                )}
            </aside>
          </div>

          <div className="find-region find-footer-region">
            <footer className="find-footer file-toolbar">
              <span>{snapshot.indexStatus === 'building' ? `正在索引，已有 ${snapshot.total} 条结果` : `共 ${snapshot.total} 条结果`}</span>
              <div className="status-region" role="status" aria-live="polite" aria-atomic="true">{snapshot.status}</div>
              <button type="button" className="launcher-operations-trigger" tabIndex={-1} aria-label="操作 Ctrl+K"
                aria-haspopup="dialog" aria-expanded={actionsOpen} aria-keyshortcuts="Control+k"
                disabled={!snapshot.ready || !snapshot.invocationId || snapshot.hidePending}
                onMouseDown={(event) => event.preventDefault()} onClick={openActions}>
                操作 <kbd>Ctrl+K</kbd>
              </button>
            </footer>
          </div>
        </main>
      </App>
    </ConfigProvider>
  )
}
