# Remove Application Aliases Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove application aliases from the settings UI, frontend state, IPC contracts, Rust persistence, application model, and search ranking.

**Architecture:** Delete the feature along its existing data path instead of hiding it at the view. Keep the current settings and search architecture, but reduce each contract to fields that are still used; Serde's default unknown-field handling reads legacy settings without alias-specific migration code.

**Tech Stack:** React 19, TypeScript 7, Vitest, Tauri 2, Rust, Serde

## Global Constraints

- Do not add dependencies, migrations, compatibility structs, or replacement alias features.
- Preserve hotkey, Research ID, autostart, file preview, use counts, application launching, plugins, and `/math` behavior.
- Old `settings.json` files containing `aliases` must load; the field must disappear on the next settings write.
- Keep `.app-mark` styles used by result icon fallback; delete only selectors owned by the settings application list.
- Do not stage or delete the untracked `.superpowers/` directory.

---

### Task 1: Remove Alias UI and Frontend State

**Files:**
- Modify: `src/launcher.test.tsx`
- Modify: `src/protocol.ts`
- Modify: `src/launcher-core.ts`
- Modify: `src/launcher-view.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: existing `SettingsView`, `UserSettingsUpdate`, `SettingsSnapshot`, and `LauncherCore`.
- Produces: settings contracts containing only hotkey, autostart, file preview, optional Research ID, and operation state.

- [ ] **Step 1: Write the failing settings-view regression test**

Add a view test that supplies a legacy payload and proves application data cannot reach the settings UI:

```tsx
it('does not render application aliases in settings', async () => {
  installMatchMedia(false)
  const fake = fakeClient()
  vi.mocked(fake.client.loadSettings).mockResolvedValueOnce({
    hotkey: 'Alt+Space',
    autostart: false,
    filePreviewEnabled: true,
    applications: [{ appId: 'legacy', displayName: 'LiveCaptions', aliases: ['caption'] }],
  } as SettingsView)
  const core = createLauncherCore(fake.client)
  await core.start()
  const mounted = await mountLauncherView(core)
  await act(async () => fake.emit(shown('settings-no-aliases', 'settings')))

  expect(mounted.host.textContent).not.toContain('LiveCaptions')
  expect(mounted.host.textContent).not.toContain('添加别名')
  expect(mounted.host.textContent).not.toContain('别名 1')
  await mounted.unmount()
})
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
npx vitest run src/launcher.test.tsx -t "does not render application aliases in settings"
```

Expected: FAIL because the current settings view renders `LiveCaptions` and alias controls.

- [ ] **Step 3: Delete the frontend alias contracts and state**

Replace the settings portions of `src/protocol.ts` with:

```ts
export interface SettingsView {
  hotkey: string
  autostart: boolean
  filePreviewEnabled: boolean
  researchId?: string
}

export interface UserSettingsUpdate {
  hotkey: string
  autostart: boolean
  researchId?: string | null
}

export interface TextControlView {
  key: ControlKey
  value: string
}

export interface SettingsSnapshot {
  hotkey: TextControlView
  researchId: TextControlView
  autostart: boolean
  readOnly: boolean
  operation?: 'load' | 'save' | 'rescan' | 'export' | 'clear'
  clearConfirmation: boolean
  needsReload: boolean
}
```

Delete `AppAliasTarget` and `ApplicationAliasView`, and rename the remaining generic `AliasControlView` to `TextControlView`. In `src/launcher-core.ts`, delete `PrivateApplication`, `PrivateSettings.applications`, `appIds`, `LauncherCore.addAlias`, `LauncherCore.removeAlias`, both method implementations, and their returned method entries.

Use only the two live text controls:

```ts
function settingsControls(settings: PrivateSettings): TextControl[] {
  return [settings.hotkey, settings.researchId]
}

function replaceSettings(view: SettingsView, previewGeneration: number): void {
  if (model.settings) {
    for (const control of settingsControls(model.settings)) retireControl(control.key)
  }
  if (previewGeneration === previewPreferenceDurableGeneration) {
    lastLoadedFilePreviewEnabled = view.filePreviewEnabled
  }
  model.settings = {
    hotkey: newTextControl(view.hotkey),
    researchId: newTextControl(view.researchId ?? ''),
    autostart: view.autostart,
  }
  model.settingsNeedsReload = false
  model.settingsLoadError = undefined
  model.clearConfirmation = false
}

function findTextControl(control: ControlKey): TextControl | undefined {
  if (!model.settings) return undefined
  if (model.settings.hotkey.key === control) return model.settings.hotkey
  if (model.settings.researchId.key === control) return model.settings.researchId
  return undefined
}

function settingsUpdate(): UserSettingsUpdate {
  const settings = model.settings!
  return {
    hotkey: settings.hotkey.value,
    autostart: settings.autostart,
    ...(settings.researchId.value === '' ? {} : { researchId: settings.researchId.value }),
  }
}
```

Delete the `application-list` block from `src/launcher-view.tsx`. Delete `.application-list`, `.application-row`, `.application-heading`, `.alias-list`, and `.alias-row` rules and their media-query references from `src/styles.css`; retain `.app-mark` and `.result-icon .app-mark`.

- [ ] **Step 4: Update frontend fixtures and alias-only tests**

Reduce shared settings fixtures to:

```ts
const emptySettings: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
}

const settingsFixture: SettingsView = {
  hotkey: 'Alt+Space',
  autostart: false,
  filePreviewEnabled: true,
}
```

Delete tests whose only subject is alias control composition, add/remove ownership, duplicate application labels, or private app-ID mapping. Change the save assertion to:

```ts
expect(client.saveSettings).toHaveBeenCalledWith({
  settings: {
    hotkey: 'Ctrl+Space',
    autostart: true,
    researchId: 'research_1',
  },
})
```

Change adapter fixtures such as `const update = { hotkey: 'Alt+Space', autostart: false, aliases: {} }` to omit `aliases`. Keep composition tests for the query, hotkey, and Research ID controls.

- [ ] **Step 5: Verify GREEN and commit**

Run:

```powershell
npx vitest run src/launcher.test.tsx
npm run build
```

Expected: all frontend tests PASS and TypeScript compilation succeeds.

Commit:

```powershell
git add src/launcher.test.tsx src/protocol.ts src/launcher-core.ts src/launcher-view.tsx src/styles.css
git commit -m "remove application aliases from settings UI"
```

---

### Task 2: Remove Aliases from Settings Persistence and IPC

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lifecycle.rs`

**Interfaces:**
- Consumes: alias-free TypeScript wire shape from Task 1.
- Produces: alias-free `Settings`, `SettingsUpdate`, `SettingsView`, and `UserSettingsUpdate`; settings save no longer consumes `AppCache`.

- [ ] **Step 1: Write the failing legacy-settings regression test**

Add this test in `src-tauri/src/settings.rs`:

```rust
#[test]
fn legacy_aliases_are_dropped_on_next_write() {
    let dir = TestDir::new("drop-legacy-aliases");
    fs::write(
        dir.current(),
        format!(
            r#"{{"hotkey":"Alt+Space","autostart":false,"filePreviewEnabled":true,"researchId":"study_01","aliases":{{"{APP_A}":["legacy"]}},"useCounts":{{}}}}"#
        ),
    )
    .unwrap();

    let store = SettingsStore::load(dir.path()).unwrap();
    store.set_file_preview_enabled(false).unwrap();

    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.current()).unwrap()).unwrap();
    assert_eq!(persisted["researchId"], "study_01");
    assert_eq!(persisted["filePreviewEnabled"], false);
    assert!(persisted.get("aliases").is_none());
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml settings::tests::legacy_aliases_are_dropped_on_next_write -- --exact
```

Expected: FAIL because current serialization retains `aliases`.

- [ ] **Step 3: Delete alias persistence and validation**

Reduce the stored and update models to:

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    #[serde(default = "default_file_preview_enabled")]
    pub(crate) file_preview_enabled: bool,
    pub(crate) research_id: Option<String>,
    #[serde(default)]
    pub(crate) use_counts: BTreeMap<String, u64>,
}

pub(crate) struct SettingsUpdate {
    pub(crate) hotkey: String,
    pub(crate) autostart: bool,
    pub(crate) research_id: Option<String>,
}
```

Make validation independent of the application cache:

```rust
fn validate_user_settings_update(update: &SettingsUpdate) -> Result<(), SettingsError> {
    if update
        .research_id
        .as_deref()
        .is_some_and(|value| !valid_research_id(value))
    {
        return Err(SettingsError::InvalidUpdate);
    }
    Ok(())
}

impl SettingsStore {
    pub(crate) fn validate_user_settings(
        update: &SettingsUpdate,
    ) -> Result<(), SettingsError> {
        validate_user_settings_update(update)
    }

    pub(crate) fn decorate_applications(&self, applications: &mut [Application]) {
        let state = self.state.lock().expect("settings lock poisoned");
        for application in applications {
            application.use_count = state
                .value
                .use_counts
                .get(&application.app_id)
                .copied()
                .unwrap_or_default();
        }
    }

    pub(crate) fn update_user_settings(
        &self,
        update: SettingsUpdate,
    ) -> Result<(), SettingsError> {
        validate_user_settings_update(&update)?;
        let mut state = self.state.lock().expect("settings lock poisoned");
        let mut candidate = state.value.clone();
        candidate.hotkey = update.hotkey;
        candidate.autostart = update.autostart;
        candidate.research_id = update.research_id;
        self.persist(&mut state, candidate)
    }
}

fn valid_settings(settings: &Settings) -> bool {
    !matches!(
        settings.research_id.as_deref(),
        Some(value) if !valid_research_id(value)
    ) && settings
        .use_counts
        .keys()
        .all(|app_id| valid_app_id(app_id))
}
```

Remove alias validation, merging, and alias-only tests. Keep `valid_app_id` for `use_counts` validation and `UnknownApplication` for use-count updates.

- [ ] **Step 4: Delete alias IPC and cache plumbing**

In `src-tauri/src/commands.rs`, delete `AppAliasTarget`, the production `BTreeMap` import, `SettingsView.applications`, and `UserSettingsUpdate.aliases`. Replace settings loading with:

```rust
fn load_settings_core(settings: &SettingsStore) -> SettingsView {
    let settings = settings.snapshot();
    SettingsView {
        hotkey: settings.hotkey,
        autostart: settings.autostart,
        file_preview_enabled: settings.file_preview_enabled,
        research_id: settings.research_id,
    }
}
```

Delete `SaveSettingsCache`. Use these signatures:

```rust
fn prepare_settings_save(
    settings: UserSettingsUpdate,
) -> Result<(HotkeyKind, SettingsUpdate), CommandError>

async fn save_settings_with<R, E, W>(
    settings: UserSettingsUpdate,
    reserve: R,
    worker: W,
) -> Result<(), CommandError>

#[cfg(test)]
fn save_settings_core(
    settings: UserSettingsUpdate,
    store: &SettingsStore,
) -> Result<(), CommandError>
```

Remove `AppCache` from the production `load_settings` and `save_settings` command parameters where it was used only for aliases. In `src-tauri/src/lifecycle.rs`, remove the cache parameter from `save_settings_transaction` and call `settings.update_user_settings(update)`.

Rewrite settings command tests around this wire contract:

```rust
assert_eq!(
    serde_json::to_value(load_settings_core(&store)).unwrap(),
    serde_json::json!({
        "hotkey": "Alt+Space",
        "autostart": false,
        "filePreviewEnabled": true
    })
);
```

Delete tests for forged/unknown alias IDs and alias preservation. Retain Research ID, hotkey transaction, caller guard, no-write preflight, use-count, and file-preview tests with alias-free fixtures.

- [ ] **Step 5: Verify GREEN and commit**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml settings::tests::legacy_aliases_are_dropped_on_next_write -- --exact
cargo test --manifest-path src-tauri\Cargo.toml --lib commands::tests
cargo test --manifest-path src-tauri\Cargo.toml --lib settings::tests
```

Expected: all selected Rust tests PASS.

Commit:

```powershell
git add src-tauri/src/settings.rs src-tauri/src/commands.rs src-tauri/src/lifecycle.rs
git commit -m "remove application aliases from settings contracts"
```

---

### Task 3: Remove Aliases from the Application Model and Ranking

**Files:**
- Modify: `src-tauri/src/apps/mod.rs`
- Modify: `src-tauri/src/apps/rank.rs`
- Modify: `src-tauri/src/apps/discovery.rs`
- Modify: `src-tauri/src/apps/appsfolder.rs`
- Modify: `src-tauri/src/apps/cache.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: alias-free settings and IPC from Task 2.
- Produces: `Application` with no alias field and ranking based only on display name plus existing tie-breakers.

- [ ] **Step 1: Write the failing ranking regression test**

Add this test while the old helper still accepts aliases:

```rust
#[test]
fn application_search_only_matches_display_name() {
    let applications = vec![application("console", "Console", &["terminal"], 0)];

    assert!(rank(&applications, "terminal").is_empty());
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml apps::rank::tests::application_search_only_matches_display_name -- --exact
```

Expected: FAIL because the current ranker matches `terminal` through the alias.

- [ ] **Step 3: Delete aliases from `Application` and ranking**

Reduce the model to:

```rust
pub(crate) struct Application {
    pub(crate) app_id: String,
    pub(crate) display_name: String,
    pub(crate) target: ApplicationLaunchTarget,
    pub(crate) icon: Option<String>,
    pub(crate) use_count: u64,
}
```

Delete the `Match.alias` state and use display-name matching directly:

```rust
fn best_match(application: &Application, query: &str) -> Option<u8> {
    match_class(&application.display_name.to_lowercase(), query)
}

pub(crate) fn rank(applications: &[Application], query: &str) -> Vec<Application> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_lowercase();
    let mut matches: Vec<_> = applications
        .iter()
        .filter_map(|application| {
            best_match(application, &query).map(|class| (application, class))
        })
        .collect();
    matches.sort_by(|(left, left_class), (right, right_class)| {
        left_class
            .cmp(right_class)
            .then_with(|| right.use_count.cmp(&left.use_count))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| match (left.entry_kind(), right.entry_kind()) {
                (ApplicationEntryKind::PackagedApp, ApplicationEntryKind::DesktopShortcut) => Ordering::Less,
                (ApplicationEntryKind::DesktopShortcut, ApplicationEntryKind::PackagedApp) => Ordering::Greater,
                _ => Ordering::Equal,
            })
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
    matches
        .into_iter()
        .take(20)
        .map(|(application, _)| application.clone())
        .collect()
}
```

Remove the alias tie-breaker only; preserve every other ordering rule.

Change rank test helpers to:

```rust
fn application(id: &str, name: &str, use_count: u64) -> Application
```

Keep `application_search_only_matches_display_name`, now constructing `application("console", "Console", 0)` and asserting that `terminal` has no result. Rename duplicate-name tests to remove alias wording.

- [ ] **Step 4: Remove alias initializers from every constructor**

Delete every alias field from all `Application` literals in the listed files, including empty and non-empty test fixtures:

```rust
aliases: Vec::new(),
aliases: vec!["legacy alias".into()],
```

Update test helpers and expected values accordingly. Do not remove `use_count`, app IDs, icons, launch targets, or the unrelated hotkey test named `parses_double_tap_exact_and_rejects_aliases`.

- [ ] **Step 5: Verify GREEN and commit**

Run:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --lib apps::rank::tests
cargo test --manifest-path src-tauri\Cargo.toml --all-features
```

Expected: ranking tests and the full Rust suite PASS.

Commit:

```powershell
git add src-tauri/src/apps src-tauri/src/commands.rs src-tauri/src/settings.rs src-tauri/src/lib.rs
git commit -m "remove application alias search support"
```

---

### Task 4: Final Regression and Manual Acceptance

**Files:**
- Verify only; modify no files unless a failing check identifies an in-scope omission.

**Interfaces:**
- Consumes: completed Tasks 1-3.
- Produces: evidence that alias functionality is absent without regressing settings, application search, or plugins.

- [ ] **Step 1: Scan for remaining application-alias code**

Run:

```powershell
rg -n "AppAliasTarget|ApplicationAliasView|PrivateApplication|addAlias|removeAlias|application\.aliases|settings\.aliases|aliases:" src src-tauri/src
```

Expected: no application-alias implementation matches. The unrelated hotkey test text and validation-export security deny-list may still contain the English word `aliases`.

- [ ] **Step 2: Run complete automated verification**

Run:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri\Cargo.toml --all-features
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets --all-features -- -D warnings
powershell -ExecutionPolicy Bypass -File scripts\test-security-config.ps1
git diff --check
```

Expected: every command exits `0`; the build may retain the existing Vite chunk-size warning.

- [ ] **Step 3: Run manual acceptance from this worktree**

Ensure no other worktree dev server is running, then run:

```powershell
cd D:\code\UiPilot_tools\.worktrees\internal-plugin-mvp

$pluginRoot = "$env:APPDATA\com.uipilot.launcher\plugins"
Remove-Item -Recurse -Force "$pluginRoot\internal.math" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pluginRoot | Out-Null
Copy-Item -Recurse -Force .\examples\plugins\internal.math $pluginRoot

npm run tauri dev
```

Expected:

1. Settings opens without application names, alias fields, `添加别名`, or alias delete buttons.
2. Saving settings does not make the application list reappear.
3. Normal application-name search still works.
4. `/math 1+1` still displays `2`, and Enter copies `2`.

- [ ] **Step 4: Confirm branch state**

Run:

```powershell
git status --short --branch
git log -5 --oneline --decorate
```

Expected: no tracked changes; `.superpowers/` may remain untracked and must not be committed.
