// @vitest-environment jsdom
import { existsSync, readFileSync } from 'node:fs'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createFindCore, type FindCore } from './find-core'
import { FindView } from './find-view'
import type { ExecuteOutcome, FileSearchResponse, FindClient } from './protocol'

const stylesSource = readFileSync('src/styles.css', 'utf8')
const findViewSource = readFileSync('src/find-view.tsx', 'utf8')
const findPreviewSource = readFileSync('src/find-browser-preview.tsx', 'utf8')
const notesStylesSource = readFileSync('examples/public-plugins/com.uipilot.notes/package/dist/panel.css', 'utf8')
const clipboardStylesSource = readFileSync('examples/public-plugins/com.uipilot.clipboard-history/package/dist/panel.css', 'utf8')

function response(): FileSearchResponse {
  return {
    requestId: 'request-1',
    indexRevision: '1',
    total: '1',
    status: 'ready',
    items: [{
      resultId: 'secret-result-id',
      name: 'Quarterly.pdf',
      kind: 'file',
      sizeBytes: '42',
      modifiedUtc: '2026-08-11T01:02:03Z',
      fullPath: String.raw`C:\Private\Quarterly.pdf`,
    }],
  }
}

function fakeClient() {
  let forward: ((payload: unknown) => void) | undefined
  const loadThumbnail = vi.fn(async (_input: { requestId: string; resultId: string }): Promise<unknown> => null)
  const client = {
    listenForward: vi.fn(async (handler) => { forward = handler; return vi.fn() }),
    listenThemeChanged: vi.fn(async () => vi.fn()),
    prepareInitialization: vi.fn(async () => ({
      status: 'prepared',
      initialization: {
        initializationToken: 'init-1',
        themeRevision: '1',
        theme: 'system',
        filePreviewRevision: '1',
        filePreviewEnabled: true,
        pinned: false,
      },
    })),
    commitReady: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    getReadyStatus: vi.fn(async ({ initializationToken }) => ({ status: 'ready', initializationToken })),
    searchFiles: vi.fn(async () => response()),
    loadThumbnail,
    executeResult: vi.fn(async () => ({ status: 'fileRevealRequested' }) satisfies ExecuteOutcome),
    setPinned: vi.fn(async ({ pinned }) => ({ pinned })),
    setPreviewPreference: vi.fn(async ({ preference }) => ({
      filePreviewRevision: '2',
      filePreviewEnabled: preference.enabled,
    })),
    hide: vi.fn(async () => undefined),
  } as unknown as FindClient
  return {
    client,
    loadThumbnail,
    emitForward(payload = { invocationId: 'inv-1', forwardSequence: '1', query: 'quarterly' }) {
      if (!forward) throw new Error('forward listener unavailable')
      forward(payload)
    },
  }
}

let mountedRoot: Root | undefined

async function mount(core: FindCore) {
  const host = document.createElement('div')
  document.body.append(host)
  mountedRoot = createRoot(host)
  await act(async () => mountedRoot!.render(<FindView core={core} />))
  return host
}

afterEach(async () => {
  if (mountedRoot) await act(async () => mountedRoot!.unmount())
  mountedRoot = undefined
  document.body.replaceChildren()
})

Object.defineProperty(window, 'matchMedia', {
  configurable: true,
  value: vi.fn(() => ({
    matches: false,
    media: '(prefers-color-scheme: dark)',
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  })),
})

describe('FindView', () => {
  it('keeps controls disabled until readiness is confirmed', async () => {
    const fake = fakeClient()
    let resolveForward!: (unlisten: () => void) => void
    vi.mocked(fake.client.listenForward).mockReturnValueOnce(new Promise((resolve) => { resolveForward = resolve }))
    const core = createFindCore(fake.client)
    const host = await mount(core)
    expect(host.querySelector<HTMLInputElement>('[role="combobox"]')?.disabled).toBe(true)
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')?.disabled).toBe(true)
    resolveForward(vi.fn())
  })

  it('renders accessible fixed icon controls and forced close', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' }))
    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!
    expect(pin.getAttribute('aria-pressed')).toBe('false')
    expect(pin.classList.contains('find-icon-button')).toBe(true)
    expect(pin.querySelector('.lucide-pin')).not.toBeNull()
    await act(async () => pin.click())
    await vi.waitFor(() => expect(host.querySelector('button[aria-label="取消固定"]')).toBeTruthy())
    const close = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!
    expect(close.querySelector('.lucide-x')).not.toBeNull()
    await act(async () => close.click())
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: true })
  })

  it('uses the shared UiPilot theme and semantic surface tokens', () => {
    expect(findViewSource).toContain("from './ui-theme'")
    expect(findViewSource).toContain('uiThemeConfig(scheme)')
    expect(findViewSource).not.toContain('@ant-design/icons')
    expect(stylesSource).toMatch(
      /\.find-surface\s*\{[^}]*color:\s*var\(--uipilot-ui-foreground\);[^}]*background:\s*var\(--uipilot-ui-background\);/s,
    )
    expect(stylesSource).toMatch(
      /\.find-icon-button\.is-selected\s*\{[^}]*color:\s*var\(--uipilot-ui-primary\);[^}]*background:\s*var\(--uipilot-ui-accent\);/s,
    )
  })

  it('provides a browser-only find preview outside the production entry', () => {
    expect(existsSync('dev/find-preview.html')).toBe(true)
    const previewHtml = readFileSync('dev/find-preview.html', 'utf8')
    expect(previewHtml).toContain('/src/find-browser-preview.tsx')
    expect(findPreviewSource).toContain("get('theme') === 'light'")
    expect(readFileSync('index.html', 'utf8')).not.toContain('find-browser-preview')
  })

  it('reuses the launcher file workspace visual language', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(host.querySelectorAll('#find-results [role="option"]')).toHaveLength(1))

    expect(host.querySelector('.find-categories')?.classList.contains('file-categories')).toBe(true)
    expect([...host.querySelectorAll('.find-category')].every((item) => item.classList.contains('file-category'))).toBe(true)
    expect(host.querySelector('.find-preview')?.classList.contains('file-preview')).toBe(true)
    expect(host.querySelector('.find-footer')?.classList.contains('file-toolbar')).toBe(true)
    const kindMark = host.querySelector('#find-results [aria-hidden="true"]')
    expect(kindMark?.classList.contains('result-icon')).toBe(true)
    expect(kindMark?.classList.contains('file-kind-mark')).toBe(true)
  })

  it('keeps result count, status, and preview switch in one compact footer row', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())

    const footer = host.querySelector<HTMLElement>('.find-footer')!
    expect(footer.querySelector('.status-region')).not.toBeNull()
    expect(host.querySelector('.find-surface > .status-region')).toBeNull()
    expect(stylesSource).toMatch(
      /\.find-surface\s*\{[^}]*grid-template-rows:\s*48px minmax\(0,\s*1fr\) 44px;/s,
    )
    expect(stylesSource).toMatch(/\.find-footer\s*\{[^}]*height:\s*28px;/s)
  })

  it('collapses the preview as a right-side drawer when its switch is off', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    const surface = host.querySelector<HTMLElement>('.find-surface')!
    const preview = host.querySelector<HTMLElement>('[aria-label="文件预览"]')!
    expect(surface.classList.contains('is-preview-collapsed')).toBe(false)
    expect(preview.getAttribute('aria-hidden')).toBe('false')

    await act(async () => host.querySelector<HTMLElement>('[role="switch"][aria-label="文件预览"]')!.click())
    await vi.waitFor(() => expect(core.getSnapshot().previewEnabled).toBe(false))

    expect(surface.classList.contains('is-preview-collapsed')).toBe(true)
    expect(preview.getAttribute('aria-hidden')).toBe('true')
    expect(preview.childElementCount).toBeGreaterThan(0)
    expect(stylesSource).toMatch(
      /\.find-surface\.is-preview-collapsed\s*\{[^}]*grid-template-columns:\s*112px minmax\(280px,\s*1fr\) 0px;/s,
    )
    expect(stylesSource).toMatch(/\.find-preview-region\s*\{[^}]*transition:[^}]*opacity 160ms ease/s)
    expect(stylesSource).toMatch(
      /\.find-surface\.is-preview-collapsed \.find-preview-region\s*\{[^}]*transition:[^}]*visibility 0s linear 180ms;/s,
    )
  })

  it('keeps preview contents at a stable width while the drawer wrapper collapses', () => {
    expect(stylesSource).toMatch(/\.find-surface\s*\{[^}]*overflow:\s*hidden;/s)
    expect(stylesSource).toMatch(
      /\.find-preview-region\s*\{[^}]*position:\s*relative;[^}]*overflow:\s*visible;/s,
    )
    expect(stylesSource).toMatch(
      /\.find-preview-region > \.find-preview\s*\{[^}]*position:\s*absolute;[^}]*top:\s*0;[^}]*left:\s*0;[^}]*width:\s*200px;[^}]*min-width:\s*200px;/s,
    )
  })

  it('expands results while the fixed preview slides with its collapsing grid column', () => {
    expect(stylesSource).toMatch(
      /\.find-surface\s*\{[^}]*transition:\s*grid-template-columns 180ms ease;/s,
    )
  })

  it('separates the three panes with consistent dividers instead of a preview outline', () => {
    expect(stylesSource).toMatch(
      /\.find-categories-region\s*\{[^}]*border-right:\s*1px solid var\(--uipilot-ui-border\);/s,
    )
    expect(stylesSource).toMatch(/\.find-surface \.result-list\s*\{[^}]*border:\s*0;/s)
    expect(stylesSource).toMatch(
      /\.find-surface \.file-preview\s*\{[^}]*border:\s*0;[^}]*border-left:\s*1px solid var\(--uipilot-ui-border\);[^}]*border-radius:\s*0;/s,
    )
  })

  it('matches the notes directory treatment for the selected search result', () => {
    for (const declaration of [
      'selection-surface: #e9e9ec;',
      'selection-border: rgba(23, 23, 25, 0.2);',
      'selection-surface: #1b1c1e;',
      'selection-border: rgba(255, 255, 255, 0.17);',
    ]) {
      expect(notesStylesSource).toContain(`--note-${declaration}`)
      expect(stylesSource).toContain(`--find-${declaration}`)
    }
    expect(stylesSource).toMatch(
      /\.find-surface \.result-row\.is-selected\s*\{[^}]*background:\s*var\(--find-selection-surface\);[^}]*box-shadow:\s*inset 0 0 0 1px var\(--find-selection-border\);/s,
    )
    expect(stylesSource).not.toContain('--find-selection-accent')
  })

  it('matches the clipboard-history category selected state', () => {
    expect(clipboardStylesSource).toMatch(
      /--panel-card:\s*color-mix\(in srgb, var\(--uipilot-color-text, #171719\) 7%, var\(--panel-canvas\)\);/,
    )
    expect(stylesSource).toMatch(
      /\.find-surface\s*\{[^}]*--find-category-selection-surface:\s*color-mix\(in srgb, var\(--uipilot-ui-foreground\) 7%, var\(--uipilot-ui-background\)\);/s,
    )
    expect(stylesSource).toMatch(
      /\.find-surface \.file-category\.is-selected\s*\{[^}]*color:\s*var\(--uipilot-ui-foreground\);[^}]*background:\s*var\(--find-category-selection-surface\);[^}]*font-weight:\s*500;/s,
    )
  })

  it('applies the built-in surface hierarchy to the find workspace', () => {
    expect(stylesSource).toMatch(
      /\.find-header > \.ant-input\s*\{[^}]*background:\s*var\(--uipilot-ui-surface-elevated\);[^}]*border-color:\s*var\(--uipilot-ui-hairline-strong\);[^}]*border-radius:\s*var\(--uipilot-ui-control-radius\);/s,
    )
    expect(stylesSource).toMatch(
      /\.find-categories-region\s*\{[^}]*background:\s*var\(--uipilot-ui-surface\);/s,
    )
    expect(stylesSource).toMatch(
      /\.find-surface \.file-category\.is-selected\s*\{[^}]*color:\s*var\(--uipilot-ui-foreground\);[^}]*background:\s*var\(--find-category-selection-surface\);[^}]*font-weight:\s*500;/s,
    )
    expect(stylesSource).toMatch(/\.find-results\s*\{[^}]*padding:\s*0 8px;/s)
    expect(stylesSource).toMatch(
      /\.find-surface \.result-row\s*\{[^}]*min-height:\s*53px;[^}]*margin-block:\s*1px;[^}]*padding:\s*7px 10px;[^}]*border-radius:\s*var\(--uipilot-ui-radius-sm\);/s,
    )
    expect(stylesSource).toMatch(
      /\.find-preview-media\s*\{[^}]*aspect-ratio:\s*4 \/ 3;/s,
    )
    expect(stylesSource).toMatch(
      /\.find-footer-region\s*\{[^}]*background:\s*var\(--uipilot-ui-surface\);/s,
    )
  })

  it('renders a compact file empty state before a result is selected', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)

    const empty = host.querySelector<HTMLElement>('.find-preview-empty')
    expect(empty).not.toBeNull()
    expect(empty?.querySelector('.lucide-file-search')).not.toBeNull()
    expect(empty?.textContent).toBe('未选择文件')
  })

  it('uses the shared success green for the enabled preview switch', () => {
    expect(stylesSource).toMatch(
      /\.find-surface \.ant-switch\.ant-switch-checked,[\s\S]*?\.find-surface \.ant-switch\.ant-switch-checked:not\(\.ant-switch-disabled\):hover\s*\{[^}]*background:\s*var\(--uipilot-ui-icon-green\);/,
    )
  })

  it('removes padding from the main find surface', () => {
    expect(stylesSource).toMatch(/\.find-surface\s*\{[^}]*padding:\s*0;/s)
  })

  it('removes the row gap from the main find surface', () => {
    expect(stylesSource).toMatch(/\.find-surface\s*\{[^}]*row-gap:\s*0;/s)
  })

  it('adds eight pixels of padding to the header region', () => {
    expect(stylesSource).toMatch(
      /\.find-region\.find-header-region\s*\{[^}]*padding:\s*8px;/s,
    )
  })

  it('wraps each grid region in its own div', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    const surface = host.querySelector<HTMLElement>('.find-surface')!
    const regions = Array.from(surface.children) as HTMLElement[]

    expect(regions.map((region) => region.tagName)).toEqual(['DIV', 'DIV', 'DIV', 'DIV', 'DIV'])
    expect(regions.map((region) => region.className)).toEqual([
      'find-region find-header-region',
      'find-region find-categories-region',
      'find-region find-results-region',
      'find-region find-preview-region',
      'find-region find-footer-region',
    ])
    expect(regions[0]?.firstElementChild?.matches('header.find-header')).toBe(true)
    expect(regions[1]?.firstElementChild?.matches('nav.find-categories')).toBe(true)
    expect(regions[2]?.firstElementChild?.classList.contains('find-results-spin')).toBe(true)
    expect(regions[3]?.firstElementChild?.matches('aside.find-preview')).toBe(true)
    expect(regions[4]?.firstElementChild?.matches('footer.find-footer')).toBe(true)

    for (const [className, area] of [
      ['find-header-region', 'header'],
      ['find-categories-region', 'categories'],
      ['find-results-region', 'results'],
      ['find-preview-region', 'preview'],
      ['find-footer-region', 'footer'],
    ] as const) {
      expect(stylesSource).toMatch(new RegExp(`\\.${className}\\s*\\{[^}]*grid-area:\\s*${area};`, 's'))
    }
  })

  it('uses distinct familiar icons for files and folders', async () => {
    const fake = fakeClient()
    const initial = response()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({
      ...initial,
      total: '2',
      items: [
        initial.items[0]!,
        {
          ...initial.items[0]!,
          resultId: 'folder-result-id',
          name: 'Quarterly',
          kind: 'folder',
          sizeBytes: null,
          fullPath: String.raw`C:\Private\Quarterly`,
        },
      ],
    })
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())

    const marks = await vi.waitFor(() => {
      const elements = host.querySelectorAll<HTMLElement>('.file-kind-mark')
      expect(elements).toHaveLength(2)
      return elements
    })
    expect(marks[0]?.classList.contains('is-file')).toBe(true)
    expect(marks[0]?.querySelector('.lucide-file')).not.toBeNull()
    expect(marks[1]?.classList.contains('is-folder')).toBe(true)
    expect(marks[1]?.querySelector('.lucide-folder')).not.toBeNull()
  })

  it('provides a native drag handle while keeping category interactions clickable', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)

    expect(host.querySelector('.find-drag-handle')).toBeTruthy()
    const normalizedStyles = stylesSource.split(String.fromCharCode(13) + String.fromCharCode(10)).join(String.fromCharCode(10))
    expect(normalizedStyles).toContain(['.find-surface,', '.find-header,', '.find-drag-handle {', '  app-region: drag;'].join(String.fromCharCode(10)))
    expect(normalizedStyles).toContain(['.find-categories {', '  app-region: no-drag;', '}'].join(String.fromCharCode(10)))
  })

  it('keeps pinned Escape inert and sends ordinary hide when unpinned', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: '' }))
    const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: false })
    vi.mocked(fake.client.hide).mockClear()
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!.click())
    await vi.waitFor(() => expect(core.getSnapshot().pinned).toBe(true))
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).not.toHaveBeenCalled()
  })

  it('prevents the browser find shortcut and focuses the query input', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const close = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!
    close.focus()
    expect(document.activeElement).toBe(close)

    const focusSearch = new KeyboardEvent('keydown', {
      key: 'f',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    })
    await act(async () => window.dispatchEvent(focusSearch))

    expect(focusSearch.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(input)
    expect(input.value).toBe('windows')
  })

  it('keeps only file categories in the Tab order', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!
    const close = host.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!
    const previewSwitch = host.querySelector<HTMLElement>('[role="switch"][aria-label="文件预览"]')!
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]

    expect(input.tabIndex).toBe(-1)
    expect(pin.tabIndex).toBe(-1)
    expect(close.tabIndex).toBe(-1)
    expect(previewSwitch.tabIndex).toBe(-1)
    expect(categories.every((button) => button.tabIndex === 0)).toBe(true)
    expect(
      [...host.querySelectorAll<HTMLElement>('*')]
        .filter((element) => element.tabIndex >= 0 && !('disabled' in element && element.disabled)),
    ).toEqual(categories)

    input.focus()
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    await act(async () => input.dispatchEvent(tab))
    expect(tab.defaultPrevented).toBe(false)
    expect(core.getSnapshot().category).toBe('all')
  })

  it('activates the next file category on forward Tab', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]
    const first = categories[0]!
    const second = categories[1]!
    first.focus()

    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    await act(async () => first.dispatchEvent(tab))

    expect(tab.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(second)
    expect(core.getSnapshot().category).toBe('folder')
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'folder' }))
  })

  it('activates the previous file category on Shift+Tab', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]
    const first = categories[0]!
    const second = categories[1]!
    await act(async () => second.click())
    second.focus()

    const shiftTab = new KeyboardEvent('keydown', {
      key: 'Tab', shiftKey: true, bubbles: true, cancelable: true,
    })
    await act(async () => second.dispatchEvent(shiftTab))

    expect(shiftTab.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(first)
    expect(core.getSnapshot().category).toBe('all')
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'all' }))
  })

  it('wraps Shift+Tab from the first file category to the last', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]
    const first = categories[0]!
    const last = categories[categories.length - 1]!
    first.focus()

    const shiftTab = new KeyboardEvent('keydown', {
      key: 'Tab', shiftKey: true, bubbles: true, cancelable: true,
    })
    await act(async () => first.dispatchEvent(shiftTab))

    expect(shiftTab.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(last)
    expect(core.getSnapshot().category).toBe('archive')
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'archive' }))
  })

  it('wraps forward Tab from the last file category to the first', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'windows' }))
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]
    const first = categories[0]!
    const last = categories[categories.length - 1]!
    await act(async () => last.click())
    last.focus()

    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    await act(async () => last.dispatchEvent(tab))

    expect(tab.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(first)
    expect(core.getSnapshot().category).toBe('all')
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'all' }))
  })

  it('navigates file results with arrow keys from any focused control', async () => {
    const fake = fakeClient()
    const first = response()
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({
      ...first,
      total: '2',
      items: [
        ...first.items,
        {
          resultId: 'second-result-id',
          name: 'Roadmap.docx',
          kind: 'file',
          sizeBytes: '84',
          modifiedUtc: '2026-08-12T01:02:03Z',
          fullPath: String.raw`C:\Private\Roadmap.docx`,
        },
      ],
    })
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(2))

    const pin = host.querySelector<HTMLButtonElement>('button[aria-label="固定窗口"]')!
    pin.focus()
    expect(document.activeElement).toBe(pin)
    expect(core.getSnapshot().selectedIndex).toBe(0)

    await act(async () => pin.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'ArrowDown', bubbles: true, cancelable: true,
    })))
    expect(core.getSnapshot().selectedIndex).toBe(1)

    await act(async () => pin.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'ArrowUp', bubbles: true, cancelable: true,
    })))
    expect(core.getSnapshot().selectedIndex).toBe(0)
  })

  it('keeps result identity private and shows a path-free placeholder for non-images', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(host.querySelectorAll('#find-results [role="option"]')).toHaveLength(1))
    expect(host.textContent).toContain('Quarterly.pdf')
    const preview = host.querySelector<HTMLElement>('[aria-label="文件预览"]')!
    expect(preview.textContent).toContain('无预览图片')
    expect(preview.textContent).not.toContain('完整路径')
    expect(preview.textContent).not.toContain(String.raw`C:\Private\Quarterly.pdf`)
    expect(preview.querySelector('.lucide-image-off')).not.toBeNull()
    expect(fake.loadThumbnail).not.toHaveBeenCalled()
    expect(host.innerHTML).not.toContain('secret-result-id')
    await act(async () => host.querySelector<HTMLButtonElement>('.find-category:nth-of-type(6)')!.click())
    expect(fake.client.searchFiles).toHaveBeenLastCalledWith(expect.objectContaining({ category: 'pdf' }))
  })

  it('shows the selected file extension as its type', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())

    const preview = await vi.waitFor(() => {
      const element = host.querySelector<HTMLElement>('[aria-label="文件预览"]')
      expect(element?.querySelectorAll('dd')).toHaveLength(3)
      return element!
    })
    expect(preview.querySelectorAll('dd')[0]?.textContent).toBe('PDF 文件')
  })

  it('formats selected file sizes with binary units', async () => {
    const fake = fakeClient()
    const initial = response()
    const sizes = [
      ['bytes.bin', '42', '42 B'],
      ['kilobytes.bin', '1536', '1.5 KB'],
      ['megabytes.bin', '1572864', '1.5 MB'],
      ['gigabytes.bin', '1610612736', '1.5 GB'],
    ] as const
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce({
      ...initial,
      total: String(sizes.length),
      items: sizes.map(([name, sizeBytes], index) => ({
        ...initial.items[0]!,
        resultId: `result-${index}`,
        name,
        sizeBytes,
        fullPath: ['C:', 'Private', name].join('\\'),
      })),
    })
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(sizes.length))

    const preview = host.querySelector<HTMLElement>('[aria-label="文件预览"]')!
    for (const [index, expected] of sizes.map(([, , value], index) => [index, value] as const)) {
      await act(async () => core.select(index))
      expect(preview.querySelectorAll('dd')[1]?.textContent).toBe(expected)
    }
  })

  it('shows a neutral size value for folders', async () => {
    const fake = fakeClient()
    const folder = response()
    folder.items[0] = {
      ...folder.items[0]!,
      name: 'Quarterly',
      kind: 'folder',
      sizeBytes: null,
      fullPath: String.raw`C:\Private\Quarterly`,
    }
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(folder)
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())

    const preview = await vi.waitFor(() => {
      const element = host.querySelector<HTMLElement>('[aria-label="文件预览"]')
      expect(element?.querySelectorAll('dd')).toHaveLength(3)
      return element!
    })
    expect(preview.querySelectorAll('dd')[1]?.textContent).toBe('--')
  })

  it('renders the selected image thumbnail from the opaque result command', async () => {
    const fake = fakeClient()
    const image = response()
    image.items[0] = {
      ...image.items[0]!,
      name: 'Quarterly.png',
      fullPath: String.raw`C:\Private\Quarterly.png`,
    }
    vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(image)
    vi.mocked(fake.loadThumbnail).mockResolvedValueOnce('data:image/png;base64,UE5H')
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward({ invocationId: 'inv-1', forwardSequence: '1', query: 'png' }))

    const thumbnail = await vi.waitFor(() => {
      const element = host.querySelector<HTMLImageElement>('.find-preview-thumbnail')
      expect(element).toBeInstanceOf(HTMLImageElement)
      return element!
    })
    expect(fake.loadThumbnail).toHaveBeenCalledWith({ requestId: 'request-1', resultId: 'secret-result-id' })
    expect(thumbnail.getAttribute('src')).toBe('data:image/png;base64,UE5H')
    expect(thumbnail.alt).toBe('Quarterly.png 缩略图')
  })
})
