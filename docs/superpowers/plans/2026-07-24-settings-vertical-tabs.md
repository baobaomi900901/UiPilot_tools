# Settings Vertical Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the stacked settings page with accessible left-side `通用` and `插件` Tabs, prevent the title from receiving focus, and keep all current settings and plugin behavior intact.

**Architecture:** Keep settings and plugin data ownership in `LauncherCore` unchanged. `LauncherView` owns only an epoch-scoped Tab key, renders Ant Design 6 `Tabs` with `tabPlacement="start"`, and focuses the selected Tab once per settings entry. CSS gives the Tabs a fixed left navigation and one independently scrolling right panel.

**Tech Stack:** React 19, TypeScript 7, Ant Design 6 `Tabs`, Vitest/JSDOM, CSS, existing Rust/Tauri regression suite.

## Global Constraints

- Work only in `D:\code\UiPilot_tools\.worktrees\settings-vertical-tabs` on `codex/settings-vertical-tabs`.
- The only production files in scope are `src/launcher-view.tsx` and `src/styles.css`.
- Do not modify `LauncherCore`, protocol DTOs, Rust, Tauri commands, permissions, or persisted settings.
- Use Ant Design 6 `tabPlacement="start"`; do not use deprecated `tabPosition`.
- The only Tabs are `通用` and `插件`, in that order.
- Every new settings `viewEpoch` defaults to `通用`; Tab selection is never persisted.
- The title remains an `<h1>` but has no `tabIndex`, ref, or focus call.
- Entering settings focuses the selected `通用` Tab; routine state updates must not steal focus.
- Entering settings still starts the existing settings load and plugin list load. Switching Tabs starts no request.
- The left navigation is fixed at `112px`; only the right panel scrolls.
- The right scrollbar must retain the existing 6px main-surface style in light, dark, and forced-colors modes.
- Do not add responsive collapse, cards, a third Tab, a new dependency, or a backend API.

---

## File Structure

- Modify `src/launcher-view.tsx`: add Tabs import, epoch-scoped local selection, selected-Tab focus, and route the existing general/plugin JSX into separate Tab panels.
- Modify `src/launcher.test.tsx`: add view helpers and cover focus, keyboard/click switching, epoch reset, request isolation, hidden mutation completion, error isolation, and existing plugin rendering under the new Tab.
- Modify `src/styles.css`: replace `.settings-form` scrolling with `.settings-tabs` plus `.settings-tab-panel`, fix the left nav width, and migrate theme/forced-color scrollbar rules.
- Keep `docs/superpowers/specs/2026-07-24-settings-vertical-tabs-design.md` as the source of truth; no additional production module is needed.

---

### Task 1: Accessible Tab State, Focus, and Content Routing

**Files:**
- Modify: `src/launcher.test.tsx:200-290, 2025-2320`
- Modify: `src/launcher-view.tsx:1-24, 195-250, 505-645`

**Interfaces:**
- Consumes: `LauncherSnapshot.view`, `LauncherSnapshot.viewEpoch`, existing `settings` and `plugins` projections, and Ant Design `Tabs`.
- Produces: local `SettingsTabKey`, epoch-scoped `SettingsTabSelection`, `.settings-tabs`, `.settings-tab-panel`, and the exact `general`/`plugins` Tab keys used by CSS and tests.

- [ ] **Step 1: Add exact test helpers for the two settings Tabs**

Add these helpers below `mountLauncherView` in `src/launcher.test.tsx`:

```tsx
function settingsTab(host: HTMLElement, label: '通用' | '插件'): HTMLElement {
  const tab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find(
    (candidate) => candidate.textContent?.trim() === label,
  )
  if (!tab) throw new Error(`settings tab missing: ${label}`)
  return tab
}

async function activateSettingsTab(host: HTMLElement, label: '通用' | '插件'): Promise<HTMLElement> {
  const tab = settingsTab(host, label)
  await act(async () => tab.click())
  await vi.waitFor(() => expect(tab.getAttribute('aria-selected')).toBe('true'))
  return tab
}
```

- [ ] **Step 2: Replace the old title-focus assertion with a failing Tab/focus contract**

Rename the existing `renders settings controls and closes only through the core hide owner` test to `renders exactly two settings tabs, focuses general, and keeps the title unfocusable`. Keep its setup and close-button ownership assertions, then replace its title/focus assertions with:

```tsx
const heading = mounted.host.querySelector<HTMLElement>('.settings-header h1')!
expect(heading.textContent).toBe('设置')
expect(heading.hasAttribute('tabindex')).toBe(false)

const tabs = [...mounted.host.querySelectorAll<HTMLElement>('[role="tab"]')]
expect(tabs.map((tab) => tab.textContent?.trim())).toEqual(['通用', '插件'])
expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true')
expect(settingsTab(mounted.host, '插件').getAttribute('aria-selected')).toBe('false')
expect(document.activeElement).toBe(settingsTab(mounted.host, '通用'))
expect(document.activeElement).not.toBe(heading)
expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()
expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()
```

The existing close button assertions remain unchanged after this block.

- [ ] **Step 3: Add failing click, keyboard, request-isolation, and epoch-reset tests**

Add this test in `describe('React view and accessibility')`:

```tsx
it('switches settings panels without loading and resets to general for a new view epoch', async () => {
  installMatchMedia(false)
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
  vi.mocked(fake.client.listPlugins).mockResolvedValue([])
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('settings-tabs-first', 'settings')))
  await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))

  const settingsLoads = vi.mocked(fake.client.loadSettings).mock.calls.length
  const pluginLoads = vi.mocked(fake.client.listPlugins).mock.calls.length
  const pluginTab = await activateSettingsTab(mounted.host, '插件')
  expect(mounted.host.querySelector('.plugin-inventory')).toBeTruthy()
  expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeNull()
  expect(fake.client.loadSettings).toHaveBeenCalledTimes(settingsLoads)
  expect(fake.client.listPlugins).toHaveBeenCalledTimes(pluginLoads)

  await act(async () => {
    pluginTab.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true }),
    )
  })
  await vi.waitFor(() => expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true'))
  expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()
  expect(fake.client.loadSettings).toHaveBeenCalledTimes(settingsLoads)
  expect(fake.client.listPlugins).toHaveBeenCalledTimes(pluginLoads)

  await activateSettingsTab(mounted.host, '插件')
  await act(async () => fake.emit(shown('settings-tabs-launcher', 'launcher')))
  await act(async () => fake.emit(shown('settings-tabs-second', 'settings')))
  await vi.waitFor(() => expect(document.activeElement).toBe(settingsTab(mounted.host, '通用')))
  expect(settingsTab(mounted.host, '通用').getAttribute('aria-selected')).toBe('true')
  expect(mounted.host.querySelector('input[name^="settings-hotkey-"]')).toBeTruthy()
  expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()

  await mounted.unmount()
  core.destroy()
})
```

- [ ] **Step 4: Add failing tests for independent errors and hidden plugin mutation completion**

Add both tests in the same React view describe block:

```tsx
it('keeps general and plugin loading failures inside their own tab panels', async () => {
  installMatchMedia(false)
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings)
    .mockResolvedValueOnce(settingsFixture)
    .mockRejectedValueOnce({ code: 'settingsFailed', message: 'private settings error' })
  vi.mocked(fake.client.listPlugins).mockResolvedValueOnce([
    { id: 'internal.math', version: '1.0.0', trigger: '/math', description: null },
  ])
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('settings-tab-error', 'settings')))
  await vi.waitFor(() => expect(core.getSnapshot().settings?.loadStatus).toBe('error'))
  expect(mounted.host.textContent).toContain('重试')
  expect(mounted.host.querySelector('.plugin-item')).toBeNull()

  await activateSettingsTab(mounted.host, '插件')
  expect(mounted.host.querySelector('.plugin-item h3')?.textContent).toBe('internal.math')
  expect(mounted.host.textContent).not.toContain('private settings error')

  await mounted.unmount()
  core.destroy()
})

it('keeps a plugin list failure out of the general settings panel', async () => {
  installMatchMedia(false)
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
  vi.mocked(fake.client.listPlugins).mockRejectedValueOnce({
    code: 'pluginListFailed',
    message: 'private plugin error',
  })
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('plugin-tab-error', 'settings')))
  await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('error'))

  const hotkey = mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')
  expect(hotkey).toBeTruthy()
  expect(hotkey?.disabled).toBe(false)
  expect(mounted.host.textContent).not.toContain('无法加载插件清单。')

  await activateSettingsTab(mounted.host, '插件')
  expect(mounted.host.querySelector('[role="alert"]')?.textContent).toBe('无法加载插件清单。')
  expect(mounted.host.textContent).not.toContain('private plugin error')

  await activateSettingsTab(mounted.host, '通用')
  expect(mounted.host.querySelector<HTMLInputElement>('input[name^="settings-hotkey-"]')?.disabled).toBe(false)

  await mounted.unmount()
  core.destroy()
})

it('keeps a plugin reload running while its tab is hidden', async () => {
  installMatchMedia(false)
  const fake = fakeClient()
  const plugin = { id: 'internal.math', version: '1.0.0', trigger: '/math', description: null }
  const reload = deferred<PluginView>()
  vi.mocked(fake.client.loadSettings).mockResolvedValue(settingsFixture)
  vi.mocked(fake.client.listPlugins).mockResolvedValueOnce([plugin])
  vi.mocked(fake.client.reloadPlugin).mockReturnValueOnce(reload.promise)
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('settings-hidden-plugin', 'settings')))
  await vi.waitFor(() => expect(core.getSnapshot().plugins?.status).toBe('ready'))
  await activateSettingsTab(mounted.host, '插件')

  const reloadButton = [...mounted.host.querySelectorAll<HTMLButtonElement>('button')].find(
    (button) => button.textContent?.trim() === '重新加载',
  )!
  await act(async () => reloadButton.click())
  await activateSettingsTab(mounted.host, '通用')
  reload.resolve({ ...plugin, version: '2.0.0' })
  await vi.waitFor(() => expect(core.getSnapshot().plugins?.items[0]?.version).toBe('2.0.0'))
  expect(mounted.host.querySelector('.plugin-inventory')).toBeNull()

  await activateSettingsTab(mounted.host, '插件')
  expect(mounted.host.querySelector('.plugin-title-line span')?.textContent).toBe('2.0.0')

  await mounted.unmount()
  core.destroy()
})
```

- [ ] **Step 5: Update existing plugin rendering and source-boundary tests before implementation**

In `renders plugin metadata and safe markdown without links images or raw HTML`, insert this before waiting for `.plugin-item`:

```tsx
await activateSettingsTab(mounted.host, '插件')
```

In `keeps the React/AntD source boundary exact`, add `Tabs` to the required Ant Design symbols:

```tsx
for (const required of [
  'ConfigProvider',
  'App',
  'Input',
  'Form',
  'Checkbox',
  'Button',
  'Popconfirm',
  'Select',
  'Spin',
  'Tabs',
  'theme',
]) {
  expect(launcherViewSource).toContain(required)
}
```

- [ ] **Step 6: Run the focused tests and verify the RED state**

Run:

```powershell
npm.cmd test -- -t "settings tabs|settings panels|plugin reload running|exactly two settings tabs"
```

Expected: FAIL because the current view has no `[role="tab"]`, the heading still has `tabIndex={-1}` and receives focus, and both settings sections are stacked.

- [ ] **Step 7: Add the exact local Tab type, state, and focus projection**

In `src/launcher-view.tsx`, add `Tabs` to the Ant Design import and define these types below `themeOptions`:

```tsx
type SettingsTabKey = 'general' | 'plugins'

interface SettingsTabSelection {
  viewEpoch: number
  key: SettingsTabKey
}
```

Inside `LauncherView`, replace `headingRef` with the Tabs ref and epoch-scoped selection:

```tsx
const settingsTabsRef = useRef<HTMLDivElement>(null)
const [settingsTabSelection, setSettingsTabSelection] = useState<SettingsTabSelection>({
  viewEpoch: -1,
  key: 'general',
})
const activeSettingsTab: SettingsTabKey =
  settingsTabSelection.viewEpoch === snapshot.viewEpoch ? settingsTabSelection.key : 'general'
```

Replace the settings branch of the existing invocation focus effect with:

```tsx
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
```

Do not include `activeSettingsTab`, settings data, or plugin data in this effect's dependency list; routine publishes must not refocus the Tab.

- [ ] **Step 8: Route the existing JSX into two exact Tab panels**

Immediately before `settingsView`, replace the existing inline ordinary-settings branch with this exact `generalSettingsPanel` definition:

```tsx
const generalSettingsPanel = (
  <div className="settings-tab-panel settings-general-panel">
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
        <div className="settings-actions">
          <Popconfirm
            title="恢复初始化设置？"
            description="快捷键将恢复为 Shift+Space，关闭开机启动，并将风格恢复为跟随系统。"
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
)
```

Create `pluginSettingsPanel` by moving the current plugin subtree into this exact wrapper without changing any state condition or command callback:

```tsx
const pluginSettingsPanel = (
  <div className="settings-tab-panel settings-plugin-panel">
    <section className="plugin-inventory" aria-labelledby="plugin-inventory-title">
      <div className="plugin-inventory-header">
        <h2 id="plugin-inventory-title">插件</h2>
        {plugins?.status === 'error' ? (
          <Button size="small" onClick={() => void core.reloadPlugins()}>
            重试
          </Button>
        ) : null}
      </div>
      {plugins?.status === 'loading' || plugins?.status === 'idle' ? (
        <div className="plugin-list-state"><Spin size="small" /></div>
      ) : plugins?.status === 'error' ? (
        <div className="plugin-list-state plugin-list-error" role="alert">{plugins.error}</div>
      ) : plugins?.items.length === 0 ? (
        <div className="plugin-list-state">未安装插件</div>
      ) : (
        <div className="plugin-list">
          {plugins?.items.map((plugin) => (
            <article className="plugin-item" key={plugin.id}>
              <div className="plugin-item-main">
                <div className="plugin-title-line">
                  <h3>{plugin.id}</h3>
                  <span>{plugin.version}</span>
                  <code>{plugin.trigger}</code>
                </div>
                <div className="plugin-description">
                  <div className="plugin-description-label">说明</div>
                  {plugin.description ? (
                    <ReactMarkdown allowedElements={pluginMarkdownElements} unwrapDisallowed>
                      {plugin.description}
                    </ReactMarkdown>
                  ) : (
                    <p>暂无说明</p>
                  )}
                </div>
                {plugin.error ? <div className="plugin-row-error" role="alert">{plugin.error}</div> : null}
              </div>
              <div className="plugin-actions">
                <Button
                  size="small"
                  loading={plugin.operation === 'reload'}
                  disabled={plugin.operation !== undefined}
                  onClick={() => void core.reloadPlugin(plugin.id)}
                >
                  重新加载
                </Button>
                <Popconfirm
                  title="删除此插件？"
                  description="插件包将从插件目录移除。"
                  okText="删除"
                  cancelText="取消"
                  onConfirm={() => void core.deletePlugin(plugin.id)}
                  disabled={plugin.operation !== undefined}
                >
                  <Button
                    size="small"
                    danger
                    loading={plugin.operation === 'delete'}
                    disabled={plugin.operation !== undefined}
                  >
                    删除
                  </Button>
                </Popconfirm>
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  </div>
)
```

Replace `settingsView` with this exact shell:

```tsx
const settingsView = (
  <section className="settings-view" aria-label="设置">
    <header className="settings-header">
      <h1>设置</h1>
      <Button aria-label="关闭" disabled={snapshot.hidePending} onClick={() => void core.requestHide()}>
        关闭
      </Button>
    </header>
    <div ref={settingsTabsRef} className="settings-tabs">
      <Tabs
        activeKey={activeSettingsTab}
        destroyOnHidden
        items={[
          { key: 'general', label: '通用', children: generalSettingsPanel },
          { key: 'plugins', label: '插件', children: pluginSettingsPanel },
        ]}
        tabPlacement="start"
        onChange={(key) => {
          if (key !== 'general' && key !== 'plugins') return
          setSettingsTabSelection({ viewEpoch: snapshot.viewEpoch, key })
        }}
      />
    </div>
  </section>
)
```

- [ ] **Step 9: Run focused and complete frontend tests and verify GREEN**

Run:

```powershell
npm.cmd test -- -t "settings tabs|settings panels|plugin reload running|exactly two settings tabs"
npm.cmd test
```

Expected: focused tests PASS, then all 3 files and at least 140 tests PASS with zero failures. If the Ant Design vertical keyboard handler uses `ArrowDown` rather than `ArrowUp` in the installed version, inspect the rendered handler behavior and use the direction that selects the preceding `通用` Tab from `插件`; do not replace keyboard coverage with a direct state call.

- [ ] **Step 10: Commit the accessible Tabs behavior**

```powershell
git add src/launcher-view.tsx src/launcher.test.tsx
git commit -m "feat: split settings into vertical tabs"
```

---

### Task 2: Fixed Left Navigation and Right-Panel Scrollbar

**Files:**
- Modify: `src/launcher.test.tsx:1770-1870`
- Modify: `src/styles.css:25-55, 300-590`

**Interfaces:**
- Consumes: `.settings-tabs`, `.settings-tab-panel`, `.settings-general-panel`, `.settings-plugin-panel` from Task 1 and Ant Design's `.ant-tabs-*` DOM classes.
- Produces: fixed 112px left nav, full-height right content holder, independent panel scrolling, and theme-compatible scrollbar variables.

- [ ] **Step 1: Replace old `.settings-form` CSS-source expectations with failing Tabs layout assertions**

Update the existing drag-region and scrollbar source tests so they require `.settings-tabs` and `.settings-tab-panel` instead of `.settings-form`. Add these exact expectations:

```tsx
expect(stylesSource).toMatch(
  /button,[\s\S]*\.settings-tabs,[\s\S]*\.settings-tab-panel\s*\{[^}]*app-region:\s*no-drag;/,
)
expect(stylesSource).toMatch(
  /\.settings-tabs\s*\{[^}]*min-width:\s*0;[^}]*min-height:\s*0;[^}]*height:\s*100%;/s,
)
expect(stylesSource).toMatch(
  /\.settings-tabs \.ant-tabs-nav\s*\{[^}]*flex:\s*0 0 112px;[^}]*width:\s*112px;/s,
)
expect(stylesSource).toMatch(
  /\.settings-tabs \.ant-tabs-content-holder\s*\{[^}]*min-width:\s*0;[^}]*min-height:\s*0;/s,
)
expect(stylesSource).toMatch(
  /\.settings-tab-panel\s*\{[^}]*height:\s*100%;[^}]*overflow-y:\s*auto;/s,
)
expect(stylesSource).toMatch(
  /\.result-list,\s*\.settings-tab-panel\s*\{[^}]*--result-scrollbar-thumb:\s*rgba\(64, 64, 64, 0\.48\);/s,
)
expect(stylesSource).toMatch(
  /\.result-list::-webkit-scrollbar,\s*\.settings-tab-panel::-webkit-scrollbar\s*\{[^}]*width:\s*6px;/s,
)
expect(stylesSource).toMatch(
  /\.launcher-surface\[data-color-scheme="dark"\][\s\S]*\.settings-tab-panel\s*\{[^}]*--result-scrollbar-thumb:\s*rgba\(217, 217, 217, 0\.55\);/s,
)
expect(stylesSource).toMatch(
  /@media \(forced-colors: active\)[\s\S]*\.settings-tab-panel::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*ButtonText;/s,
)
```

Remove only the obsolete expectations that require `.settings-form` to own drag and scrollbar behavior. Keep all `.result-list` expectations.

- [ ] **Step 2: Run the CSS contract test and verify RED**

Run:

```powershell
npm.cmd test -- -t "drag regions|slim result scrollbar|settings tabs layout"
```

Expected: FAIL because `.settings-tabs` and `.settings-tab-panel` do not yet have the fixed layout or scrollbar rules.

- [ ] **Step 3: Replace the settings container CSS with the exact full-height Tabs layout**

In the drag exclusion selector near the top of `src/styles.css`, replace `.settings-form` with both new classes:

```css
button,
input,
label,
.result-list,
.file-category-strip,
.file-preview,
.file-toolbar,
.settings-tabs,
.settings-tab-panel {
  app-region: no-drag;
}
```

Replace the old `.settings-form, .settings-loading` block with:

```css
.settings-tabs {
  min-width: 0;
  min-height: 0;
  height: 100%;
  padding-top: 8px;
  overflow: hidden;
}

.settings-tabs > .ant-tabs {
  min-width: 0;
  min-height: 0;
  height: 100%;
}

.settings-tabs .ant-tabs-nav {
  flex: 0 0 112px;
  width: 112px;
  margin: 0;
}

.settings-tabs .ant-tabs-tab {
  margin: 0;
  padding: 10px 12px;
}

.settings-tabs .ant-tabs-tab-btn {
  width: 100%;
  text-align: left;
}

.settings-tabs .ant-tabs-content-holder,
.settings-tabs .ant-tabs-content,
.settings-tabs .ant-tabs-tabpane {
  min-width: 0;
  min-height: 0;
  height: 100%;
}

.settings-tabs .ant-tabs-content-holder {
  border-inline-start: 1px solid #d9d9d9;
}

.settings-tab-panel {
  min-width: 0;
  min-height: 0;
  height: 100%;
  padding: 10px 12px 4px;
  overflow-y: auto;
}

.settings-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  min-width: 0;
  min-height: 100%;
}
```

Keep `.settings-actions` and `.settings-basic-form`. Simplify the plugin root because it no longer separates two stacked sections:

```css
.plugin-inventory {
  min-width: 0;
}
```

- [ ] **Step 4: Move the scrollbar selectors from `.settings-form` to `.settings-tab-panel`**

Replace each selector pair exactly:

```css
.result-list,
.settings-tab-panel {
  --result-scrollbar-thumb: rgba(64, 64, 64, 0.48);
}

.result-list::-webkit-scrollbar,
.settings-tab-panel::-webkit-scrollbar {
  width: 6px;
}

.result-list::-webkit-scrollbar-track,
.settings-tab-panel::-webkit-scrollbar-track {
  background: transparent;
}

.result-list::-webkit-scrollbar-thumb,
.settings-tab-panel::-webkit-scrollbar-thumb {
  background: var(--result-scrollbar-thumb);
  border-radius: 3px;
}
```

Update the dark rules to include the Tabs divider and panel scrollbar:

```css
.launcher-surface[data-color-scheme="dark"] .settings-tabs .ant-tabs-content-holder {
  border-color: #595959;
}

.launcher-surface[data-color-scheme="dark"] .result-list,
.launcher-surface[data-color-scheme="dark"] .settings-tab-panel {
  --result-scrollbar-thumb: rgba(217, 217, 217, 0.55);
}
```

Remove `.plugin-inventory` from the dark border selector because it no longer owns a border. Keep `.plugin-item` and all text-color rules.

Update forced colors:

```css
@media (forced-colors: active) {
  .launcher-surface,
  .result-list,
  .settings-header,
  .settings-tabs .ant-tabs-content-holder,
  .file-preview,
  .app-mark {
    border-color: CanvasText;
  }

  .result-list::-webkit-scrollbar-thumb,
  .settings-tab-panel::-webkit-scrollbar-thumb {
    background: ButtonText;
  }
}
```

Preserve the other declarations inside the forced-colors block.

- [ ] **Step 5: Run focused CSS tests, full frontend tests, and the production build**

Run:

```powershell
npm.cmd test -- -t "drag regions|slim result scrollbar|settings tabs layout"
npm.cmd test
npm.cmd run build
```

Expected: focused tests PASS; all frontend tests PASS; TypeScript and Vite production build exit 0. The existing Vite chunk-size warning is non-blocking.

- [ ] **Step 6: Commit the layout and scrollbar behavior**

```powershell
git add src/styles.css src/launcher.test.tsx
git commit -m "style: lay out settings tabs side by side"
```

---

### Task 3: Full Regression and Worktree Handoff

**Files:**
- Verify only; no production file should change.

**Interfaces:**
- Consumes: the two implementation commits from Tasks 1 and 2.
- Produces: clean verification evidence and manual test instructions for the isolated worktree.

- [ ] **Step 1: Run the complete automatic verification**

Run from the worktree root unless a command changes directory explicitly:

```powershell
npm.cmd test
npm.cmd run build
cargo test --quiet --manifest-path src-tauri/Cargo.toml -- --skip plugins::tests::delete::no_follow_handle_move_removes_original_path_and_preserves_identity
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --check
git diff --check main...HEAD
```

Expected:

- Vitest: 3 files, at least 144 tests, zero failures.
- Vite/TypeScript build: exit 0.
- Rust: 376 or more passed, zero failed, with only the existing ignored/filtered tests.
- `cargo check`, `cargo fmt --check`, and `git diff --check`: exit 0.

- [ ] **Step 2: Remove only build-generated line-ending noise**

Run:

```powershell
git diff -- src-tauri/Cargo.toml src-tauri/permissions/autogenerated
```

Expected: no textual diff. If `git status --short` still reports these files solely because the Tauri build rewrote line endings, restore only those generated paths:

```powershell
git restore src-tauri/Cargo.toml src-tauri/permissions/autogenerated
```

Do not restore `src/launcher-view.tsx`, `src/styles.css`, `src/launcher.test.tsx`, the spec, or this plan.

- [ ] **Step 3: Verify the final branch state**

Run:

```powershell
git status --short
git log --oneline main..HEAD
git diff --check main...HEAD
```

Expected: clean status; design/plan plus two implementation commits visible; no whitespace errors.

- [ ] **Step 4: Hand off manual testing without starting the GUI**

Provide this command:

```powershell
cd D:\code\UiPilot_tools\.worktrees\settings-vertical-tabs
npm run tauri dev
```

Ask the user to verify:

1. The title is visible but the focus indicator starts on `通用`.
2. The left side contains only `通用` and `插件`.
3. The right side switches content by click and vertical arrow keys.
4. Closing from `插件` and reopening returns to `通用`.
5. A long plugin list scrolls only on the right with the main scrollbar style.
6. Light, Dark, and system themes remain readable.
7. Settings instant apply, reset, plugin reload, and plugin delete still work.

Do not merge, push, remove the worktree, or delete the branch before the user reports manual acceptance.
