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
  it('names the search input and executes the selected result from category focus once', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
    expect(host.querySelector('[role="combobox"]')?.getAttribute('aria-label')).toBe('搜索文件')
    const category = host.querySelector<HTMLButtonElement>('.find-category')!
    category.focus()
    const key = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    await act(async () => category.dispatchEvent(key))
    expect(key.defaultPrevented).toBe(true)
    expect(fake.client.executeResult).toHaveBeenCalledOnce()
    const repeated = new KeyboardEvent('keydown', { key: 'Enter', repeat: true, bubbles: true, cancelable: true })
    await act(async () => category.dispatchEvent(repeated))
    expect(repeated.defaultPrevented).toBe(true)
    expect(fake.client.executeResult).toHaveBeenCalledOnce()
  })

  it('handles category Escape with the same pinned behavior as the search input', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    const category = host.querySelector<HTMLButtonElement>('.find-category')!
    category.focus()
    await act(async () => category.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).toHaveBeenCalledWith({ invocationId: 'inv-1', force: false })
    vi.mocked(fake.client.hide).mockClear()
    await act(async () => core.setPinned(true))
    await vi.waitFor(() => expect(core.getSnapshot().pinned).toBe(true))
    await act(async () => category.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(fake.client.hide).not.toHaveBeenCalled()
  })

  it('preserves composing and modified category keys and the space button action', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    const category = host.querySelector<HTMLButtonElement>('.find-category')!
    for (const init of [
      { key: 'Enter', isComposing: true }, { key: 'Escape', isComposing: true },
      { key: 'Enter', ctrlKey: true }, { key: 'Enter', altKey: true },
      { key: 'Enter', metaKey: true }, { key: 'Escape', shiftKey: true }, { key: ' ' },
    ]) {
      const event = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true })
      await act(async () => category.dispatchEvent(event))
      expect(event.defaultPrevented).toBe(false)
    }
    expect(fake.client.executeResult).not.toHaveBeenCalled()
    expect(fake.client.hide).not.toHaveBeenCalled()
  })

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

  it('toggles pin with Ctrl+T from search and categories without repeating or changing the query', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!
    const press = (target: Element, extra: KeyboardEventInit = {}) => {
      const event = new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true, cancelable: true, ...extra })
      target.dispatchEvent(event)
      return event
    }
    await act(async () => { press(input) })
    expect(fake.client.setPinned).not.toHaveBeenCalled()
    await act(async () => fake.emitForward())
    await act(async () => { expect(press(input).defaultPrevented).toBe(true) })
    expect(fake.client.setPinned).toHaveBeenLastCalledWith({ invocationId: 'inv-1', pinned: true })
    expect(host.querySelector('button[aria-label="取消固定"]')?.getAttribute('aria-keyshortcuts')).toBe('Control+t')
    await act(async () => {
      press(input, { repeat: true })
      press(input, { shiftKey: true })
      press(input, { altKey: true })
      press(input, { metaKey: true })
      press(input, { isComposing: true })
      press(input, { ctrlKey: false })
    })
    expect(fake.client.setPinned).toHaveBeenCalledTimes(1)
    await act(async () => { press(host.querySelector('.find-category')!, { key: 'T' }) })
    expect(fake.client.setPinned).toHaveBeenLastCalledWith({ invocationId: 'inv-1', pinned: false })
    expect(host.querySelector('button[aria-label="固定窗口"]')?.getAttribute('aria-pressed')).toBe('false')
    expect(core.getSnapshot().query).toBe('quarterly')
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
      /\.find-surface\s*\{[^}]*grid-template-rows:\s*60px 48px minmax\(0,\s*1fr\) 36px;/s,
    )
    expect(stylesSource).toMatch(/\.find-footer\s*\{[^}]*height:\s*22px;/s)
  })

  it('collapses the preview from its menu action without a footer switch', async () => {
    const fake = fakeClient()
    const core = createFindCore(fake.client)
    const host = await mount(core)
    await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
    await act(async () => fake.emitForward())
    const surface = host.querySelector<HTMLElement>('.find-surface')!
    const preview = host.querySelector<HTMLElement>('[aria-label="文件预览"]')!
    expect(surface.classList.contains('is-preview-collapsed')).toBe(false)
    expect(preview.getAttribute('aria-hidden')).toBe('false')

    expect(host.querySelector('[role="switch"]')).toBeNull()
    await pressFind(host.querySelector('[role="combobox"]')!, 'k', { ctrlKey: true })
    await act(async () => menuAction('关闭预览').click())
    await vi.waitFor(() => expect(core.getSnapshot().previewEnabled).toBe(false))

    expect(surface.classList.contains('is-preview-collapsed')).toBe(true)
    expect(preview.getAttribute('aria-hidden')).toBe('true')
    expect(preview.childElementCount).toBeGreaterThan(0)
    expect(stylesSource).toMatch(
      /\.find-surface\.is-preview-collapsed\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\) 0px;/s,
    )
    expect(stylesSource).toMatch(/\.find-preview-region\s*\{[^}]*transition:[^}]*opacity 160ms ease/s)
    expect(stylesSource).toMatch(
      /\.find-surface\.is-preview-collapsed \.find-preview-region\s*\{[^}]*transition:[^}]*visibility 0s linear 180ms;/s,
    )
  })

  it('keeps preview contents at a stable width while the drawer wrapper collapses', () => {
    expect(stylesSource).toMatch(/\.find-surface\s*\{[^}]*overflow:\s*hidden;/s)
    expect(stylesSource).toMatch(
      /\.find-preview-region\s*\{[^}]*position:\s*relative;[^}]*overflow:\s*hidden;/s,
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

  it('separates the category row and preview pane with consistent dividers', () => {
    expect(stylesSource).toMatch(
      /\.find-categories-region\s*\{[^}]*border-top:\s*1px solid var\(--uipilot-ui-border\);/s,
    )
    expect(stylesSource).toMatch(/\.find-surface \.result-list\s*\{[^}]*border:\s*0;/s)
    expect(stylesSource).toMatch(
      /\.find-surface \.file-preview\s*\{[^}]*border:\s*0;[^}]*border-left:\s*1px solid var\(--uipilot-ui-border\);[^}]*border-radius:\s*0;/s,
    )
  })

  it('uses shared semantic selection tokens for the selected search result', () => {
    expect(stylesSource).toContain('--find-selection-surface: var(--uipilot-ui-selection);')
    expect(stylesSource).toContain('--find-selection-border: var(--uipilot-ui-selection-border);')
    expect(stylesSource).toMatch(
      /\.find-surface \.result-row\.is-selected\s*\{[^}]*background:\s*var\(--find-selection-surface\);[^}]*box-shadow:\s*inset 0 0 0 1px var\(--find-selection-border\);/s,
    )
    expect(stylesSource).not.toContain('--find-selection-accent')
  })

  it('keeps category selection distinct without a focus outline', () => {
    expect(stylesSource).toMatch(
      /\.find-surface\s*\{[^}]*--find-category-selection-surface:\s*color-mix\(in srgb, var\(--uipilot-ui-foreground\) 7%, var\(--uipilot-ui-background\)\);/s,
    )
    expect(stylesSource).toMatch(/\.find-surface \.file-category:focus-visible\s*\{[^}]*outline:\s*none;/s)
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
    const categories = [...host.querySelectorAll<HTMLButtonElement>('.find-category')]

    expect(input.tabIndex).toBe(-1)
    expect(pin.tabIndex).toBe(-1)
    expect(close.tabIndex).toBe(-1)
    expect(categories.every((button) => button.tabIndex === 0)).toBe(true)
    expect(
      [...host.querySelectorAll<HTMLElement>('*')]
        .filter((element) => element.tabIndex >= 0 && !('disabled' in element && element.disabled)),
    ).toEqual(categories)

    input.focus()
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    await act(async () => input.dispatchEvent(tab))
    expect(tab.defaultPrevented).toBe(true)
    expect(core.getSnapshot().category).toBe('folder')
    expect(document.activeElement).toBe(categories[1])
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

async function readyFind() {
  const fake = fakeClient()
  const core = createFindCore(fake.client)
  const host = await mount(core)
  await vi.waitFor(() => expect(core.getSnapshot().ready).toBe(true))
  await act(async () => fake.emitForward())
  await vi.waitFor(() => expect(core.getSnapshot().results).toHaveLength(1))
  return { fake, core, host, input: host.querySelector<HTMLInputElement>('[role="combobox"]')! }
}
async function pressFind(target: Element, key: string, extra: KeyboardEventInit = {}) {
  await act(async () => target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })))
}
function menuAction(title: string) {
  return [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')].find(button => button.querySelector('.uipilot-context-action-label')?.textContent === title)!
}

it('Find actions distinguish input search from file reveal and restore row focus', async () => {
  const { fake, core, host, input } = await readyFind()
  const before = vi.mocked(fake.client.searchFiles).mock.calls.length
  await pressFind(input, 'Enter')
  await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(before + 1))
  expect(fake.client.executeResult).not.toHaveBeenCalled()
  await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
  input.focus()
  await pressFind(input, 'k', { ctrlKey: true })
  expect(menuAction('搜索')).toBeDefined()
  expect(menuAction('固定窗口')).toBeDefined()
  expect(menuAction('关闭预览')).toBeDefined()
  expect(menuAction('返回搜索框')).toBeDefined()
  await act(async () => menuAction('搜索').click())
  await vi.waitFor(() => expect(fake.client.searchFiles).toHaveBeenCalledTimes(before + 2))
  expect(fake.client.executeResult).not.toHaveBeenCalled()
  await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
  await pressFind(input, 'ArrowDown')
  const row = host.querySelector<HTMLElement>('[role="option"]')!
  expect(document.activeElement).toBe(row)
  await pressFind(row, 'k', { ctrlKey: true })
  expect(menuAction('搜索')).toBeUndefined()
  expect(menuAction('打开所在的文件夹')).toBeDefined()
  await pressFind(document.activeElement!, 'Escape')
  expect(document.activeElement).toBe(row)
  expect(fake.client.hide).not.toHaveBeenCalled()
  await pressFind(row, 'Enter')
  expect(fake.client.executeResult).toHaveBeenCalledExactlyOnceWith({ requestId: 'request-1', resultId: 'secret-result-id' })
})

it('Find menu owns arrow keys, toggles globals and invalidates old file actions', async () => {
  const { fake, core, host, input } = await readyFind()
  const row = host.querySelector<HTMLElement>('[role="option"]')!
  await act(async () => row.click())
  await pressFind(row, 'k', { ctrlKey: true })
  const old = menuAction('打开所在的文件夹')
  const keyDown = vi.spyOn(core, 'keyDown')
  await pressFind(document.activeElement!, 'ArrowDown')
  expect(keyDown).not.toHaveBeenCalled()
  await act(async () => core.setQuery('new search'))
  expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  expect(document.activeElement).toBe(input)
  await act(async () => old.click())
  expect(fake.client.executeResult).not.toHaveBeenCalled()
  await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
  input.focus()
  await pressFind(input, 'k', { ctrlKey: true })
  await act(async () => menuAction('固定窗口').click())
  await vi.waitFor(() => expect(core.getSnapshot().pinPending).toBe(false))
  expect(core.getSnapshot().pinned).toBe(true)
  await pressFind(input, 'k', { ctrlKey: true })
  expect(menuAction('取消固定窗口')).toBeDefined()
  await act(async () => menuAction('关闭预览').click())
  await vi.waitFor(() => expect(core.getSnapshot().previewPending).toBe(false))
  expect(core.getSnapshot().previewEnabled).toBe(false)
  await pressFind(input, 'k', { ctrlKey: true })
  expect(menuAction('打开预览')).toBeDefined()
  await act(async () => menuAction('返回搜索框').click())
  expect(document.activeElement).toBe(input)
})

it('Find ignores composing and repeated Ctrl+K and disables empty-result reveal', async () => {
  const { core, host, input } = await readyFind()
  await pressFind(input, 'k', { ctrlKey: true, isComposing: true })
  await pressFind(input, 'k', { ctrlKey: true, repeat: true })
  expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  await pressFind(input, 'k', { ctrlKey: true })
  await pressFind(document.activeElement!, 'k', { ctrlKey: true, repeat: true })
  expect(document.querySelector('.uipilot-context-actions')).not.toBeNull()
  await pressFind(document.activeElement!, 'Escape')
  await act(async () => core.setQuery(''))
  host.querySelector<HTMLElement>('.find-category')!.focus()
  await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="操作 Ctrl+K"]')!.click())
  const action = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')].find(button => button.textContent?.includes('打开所在的文件夹'))!
  expect(action.getAttribute('aria-disabled')).toBe('true')
  await pressFind(document.activeElement!, 'Escape')
})

it('Find restores input after pending search invalidates its menu', async () => {
  const { fake, core, input } = await readyFind()
  let finish!: (value: FileSearchResponse) => void
  vi.mocked(fake.client.searchFiles).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  await act(async () => core.submitSearch())
  input.focus()
  await pressFind(input, 'k', { ctrlKey: true })
  await act(async () => finish(response()))
  expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  expect(document.activeElement).toBe(input)
})

it('Find cannot focus a different file while reveal is pending', async () => {
  const { fake, core, host } = await readyFind()
  const two = response()
  two.items.push({ ...two.items[0]!, resultId: 'second', name: 'second.pdf', fullPath: 'C:\\second.pdf' })
  vi.mocked(fake.client.searchFiles).mockResolvedValueOnce(two)
  await act(async () => core.submitSearch())
  const rows = host.querySelectorAll<HTMLElement>('[role="option"]')
  await act(async () => rows[0]!.click())
  let finish!: (value: ExecuteOutcome) => void
  vi.mocked(fake.client.executeResult).mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
  await pressFind(rows[0]!, 'Enter')
  await act(async () => rows[1]!.click())
  expect(document.activeElement).toBe(rows[0])
  expect(core.getSnapshot().selectedIndex).toBe(0)
  await act(async () => finish({ status: 'fileRevealRequested' }))
})

it('Find category action and Tab share cycling without trapping result navigation', async () => {
  const { core, host, input } = await readyFind()
  input.focus()
  await pressFind(input, 'k', { ctrlKey: true })
  expect(menuAction('下一分类').querySelector('kbd')?.textContent).toBe('Tab')
  expect(menuAction('上一分类').querySelector('kbd')?.textContent).toBe('Shift+Tab')
  await pressFind(document.activeElement!, 'Tab')
  expect(core.getSnapshot().category).toBe('all')
  await act(async () => menuAction('下一分类').click())
  expect(core.getSnapshot().category).toBe('folder')
  expect(document.activeElement).toBe(host.querySelectorAll('.find-category')[1])
  await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
  await pressFind(document.activeElement!, 'ArrowDown')
  expect(document.activeElement?.getAttribute('role')).toBe('option')
  await pressFind(document.activeElement!, 'Tab')
  expect(core.getSnapshot().category).toBe('excel')
  await pressFind(document.activeElement!, 'Tab', { shiftKey: true })
  expect(core.getSnapshot().category).toBe('folder')
  await vi.waitFor(() => expect(core.getSnapshot().searchPending).toBe(false))
  await pressFind(document.activeElement!, 'k', { ctrlKey: true })
  await act(async () => menuAction('上一分类').click())
  expect(core.getSnapshot().category).toBe('all')
  expect(document.activeElement).toBe(host.querySelector('.find-category'))
})

it('exposes Mac preview scope, Finder actions and metadata without requesting a thumbnail', async () => {
  const fake = fakeClient()
  const entry = response()
  Object.assign(entry.items[0], { name: '样例.png', fullPath: '/approved/样例.png', canOpenDocument: true })
  vi.mocked(fake.client.searchFiles).mockResolvedValue(entry)
  const openDocument = vi.fn(async (): Promise<ExecuteOutcome> => ({ status: 'fileOpenRequested' }))
  const cancelQuery = vi.fn(async () => {})
  const core = createFindCore({ ...fake.client, macos: { scope: 'public-preview', openDocument, cancelQuery } })
  const host = await mount(core)
  await act(async () => fake.emitForward({ invocationId: 'mac-1', forwardSequence: '1', query: '样例' }))
  expect(host.textContent).toContain('本机公共字体与共享目录（仅限 Spotlight 已索引项目）')
  expect(host.textContent).toContain('PNG 文件')
  expect(fake.loadThumbnail).not.toHaveBeenCalled()
  expect(host.querySelector('.find-preview-media')).toBeNull()
  const option = host.querySelector<HTMLElement>('[role=option]')!
  option.focus()
  await pressFind(option, 'k', { metaKey: true, isComposing: true })
  expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  await pressFind(option, 'k', { metaKey: true })
  expect(document.querySelector('.uipilot-context-actions')?.textContent).toContain('在 Finder 中显示')
  await pressFind(document.activeElement!, 'k', { metaKey: true })
  expect(document.querySelector('.uipilot-context-actions')).toBeNull()
  await pressFind(option, 'k', { metaKey: true })
  const open = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')].find(b => b.textContent === '打开文件')!
  await act(async () => open.click())
  expect(openDocument).toHaveBeenCalledExactlyOnceWith({ requestId: 'request-1', resultId: 'secret-result-id' })
  expect(fake.client.executeResult).not.toHaveBeenCalled()
  await act(async () => core.setQuery(''))
  expect(cancelQuery).toHaveBeenCalledExactlyOnceWith({ invocationId: 'mac-1', querySequence: 2 })
  expect(core.getSnapshot().results).toEqual([])
})

it('opens and removes session Documents access from the header and keyboard Actions menu', async () => {
  const fake = fakeClient()
  const get = vi.fn(async () => ({ status: 'notGranted', revision: '0' }))
  const grant = vi.fn(async () => ({ status: 'granted', revision: '1' }))
  const revoke = vi.fn(async () => ({ status: 'notGranted', revision: '2' }))
  const core = createFindCore({ ...fake.client, macos: {
    scope: 'public-preview', documents: { get, grant, revoke }, openDocument: vi.fn(), cancelQuery: vi.fn(),
  } })
  const host = await mount(core)
  await act(async () => fake.emitForward())
  const header = host.querySelector<HTMLButtonElement>('.find-documents-button')!
  expect(header.textContent).toBe('允许本次搜索文稿')
  expect(grant).not.toHaveBeenCalled()
  await act(async () => header.click())
  expect(grant).toHaveBeenCalledExactlyOnceWith({ invocationId: 'inv-1' })
  expect(header.textContent).toBe('停止搜索文稿')
  expect(host.textContent).toContain('共享目录与本次允许的文稿')
  const input = host.querySelector<HTMLInputElement>('[role=combobox]')!
  input.focus()
  await pressFind(input, 'k', { metaKey: true })
  const stop = [...document.querySelectorAll<HTMLButtonElement>('.uipilot-context-action')]
    .find(button => button.textContent === '停止搜索文稿')!
  await pressFind(document.activeElement!, 'ArrowDown')
  expect(document.activeElement).toBe(stop)
  await pressFind(stop, 'Enter')
  expect(revoke).toHaveBeenCalledExactlyOnceWith({ invocationId: 'inv-1' })
  expect(header.textContent).toBe('允许本次搜索文稿')
  get.mockResolvedValueOnce({ status: 'needsReauthorization', revision: '3' })
  await act(async () => { window.dispatchEvent(new Event('focus')) })
  expect(host.textContent).toContain('文稿需重新授权')
  expect(grant).toHaveBeenCalledOnce()
})
