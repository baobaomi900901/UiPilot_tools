# /find Everything Category Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Add a left category sidebar to /find and make each category perform a safe, complete Everything Query2 search for the current non-empty keyword.

**Architecture:** Keep the existing literal keyword encoder, ResultRegistry, authenticated Everything actions, query sequence, and modified-descending Query2 contract. Add a typed backend category predicate, pass the validated category through the command, and make the frontend sidebar and Tab shortcuts initiate ordinary category-owned searches with the existing stale-response guards.

**Tech Stack:** Rust, Tauri commands, Everything 1.4 Query2 IPC, TypeScript, React, Ant Design, Vitest.

## Global Constraints

- Category IDs are exactly all, folder, excel, word, ppt, pdf, image, video, audio, and archive, in that order.
- Empty keywords never issue an Everything request.
- User text remains literal and is encoded as nowildcards:#x<HEX>: entities.
- Category predicates are server-owned static strings; WebView category text is never concatenated into a query.
- Every search remains one Query2 request with offset 0, max 200, request flags 0x155, and date-modified descending sort.
- Existing opaque request/result IDs, authenticated actions, stale-response checks, preview behavior, Enter execution, and unavailable mapping remain authoritative.
- Do not add pagination, sorting UI, installer/runtime management, permissions, or background refresh.

---

### Task 1: Freeze Category Query Mapping

**Files:**
- Modify: src-tauri/src/file_search/everything.rs:55-117
- Test: src-tauri/src/file_search/everything.rs unit tests near the literal-query tests

**Interfaces:**
- Consumes: FileCategory from src-tauri/src/file_search/mod.rs.
- Produces: category_predicate(FileCategory) -> &'static str and a category-aware literal query builder for Task 2.

- [ ] Step 1: Write failing mapping tests.

Assert the exact predicates:

~~~rust
assert_eq!(category_predicate(FileCategory::All), "");
assert_eq!(category_predicate(FileCategory::Folder), "folder:");
assert_eq!(category_predicate(FileCategory::Excel), "file: ext:xls;xlsx;xlsm;xlsb;csv");
assert_eq!(category_predicate(FileCategory::Word), "file: ext:doc;docx;docm;rtf");
assert_eq!(category_predicate(FileCategory::Ppt), "file: ext:ppt;pptx;pptm");
assert_eq!(category_predicate(FileCategory::Pdf), "file: ext:pdf");
assert_eq!(category_predicate(FileCategory::Image), "file: ext:bmp;gif;heic;jpeg;jpg;png;svg;tif;tiff;webp");
assert_eq!(category_predicate(FileCategory::Video), "file: ext:avi;m4v;mkv;mov;mp4;webm;wmv");
assert_eq!(category_predicate(FileCategory::Audio), "file: ext:aac;flac;m4a;mp3;ogg;wav;wma");
assert_eq!(category_predicate(FileCategory::Archive), "file: ext:7z;bz2;gz;rar;tar;tgz;zip");
~~~

Also assert that a keyword containing |!*?<>" is still entity-encoded and that the category predicate is appended only from the enum.

- [ ] Step 2: Run the focused Rust tests and verify failure.

Run:

~~~text
cargo test -p uipilot file_search::everything::tests::category -- --nocapture
~~~

Expected: failure because the category mapping/query builder is not implemented.

- [ ] Step 3: Implement the static mapping and builder.

Implement an exhaustive mapping and a builder equivalent to:

~~~rust
fn category_search_query(query: &str, category: FileCategory) -> Vec<u16> {
    let mut search = literal_search_query(query);
    let predicate = category_predicate(category);
    if !predicate.is_empty() {
        search.extend(' '.encode_utf16());
        search.extend(predicate.encode_utf16());
    }
    search
}
~~~

Keep all existing EverythingQuerySpec fields and authentication logic unchanged.

- [ ] Step 4: Run the focused tests and verify success.

Run the same focused command. Expected: all category mapping and encoding tests pass.

- [ ] Step 5: Commit.

~~~text
git add src-tauri/src/file_search/everything.rs
git commit -m "feat: add Everything file category predicates"
~~~

### Task 2: Thread Validated Category Through Rust Search

**Files:**
- Modify: src-tauri/src/commands.rs:434-535
- Modify: src-tauri/src/file_search/everything.rs:29-117
- Test: src-tauri/src/commands.rs tests near prepare_file_query and search_files_with

**Interfaces:**
- Consumes: category_search_query and category_predicate from Task 1.
- Produces: PreparedFileQuery with query, category, invocation_id, and query_sequence; EverythingSearchState::search(&self, query, category).

- [ ] Step 1: Add failing command validation and propagation tests.

Verify every declared category is accepted, not-a-category is rejected, and the worker receives the typed category:

~~~rust
let prepared = prepare_file_query(
    "report".into(),
    "pdf".into(),
    "modifiedDesc".into(),
    "inv".into(),
    1,
).unwrap();
assert_eq!(prepared.category, FileCategory::Pdf);
assert!(prepare_file_query(
    "report".into(),
    "not-a-category".into(),
    "modifiedDesc".into(),
    "inv".into(),
    1,
).is_err());
~~~

Extend the existing production-path test so the closure asserts query == report and category == FileCategory::Pdf.

- [ ] Step 2: Run focused command tests and verify failure.

~~~text
cargo test -p uipilot commands::tests::prepare_file_query -- --nocapture
~~~

Expected: failure because category is still rejected or absent from the prepared query.

- [ ] Step 3: Implement typed validation and propagation.

Parse the category with the existing protocol conversion pattern, reject unknown values before registry admission, and pass it through search_files, search_files_with, and EverythingSearchState::search. Keep require_main_window(&window)? as the first command statement and keep sort validation.

- [ ] Step 4: Run focused command and adapter tests.

~~~text
cargo test -p uipilot commands::tests::prepare_file_query -- --nocapture
cargo test -p uipilot file_search::everything -- --nocapture
~~~

Expected: all focused tests pass.

- [ ] Step 5: Commit.

~~~text
git add src-tauri/src/commands.rs src-tauri/src/file_search/everything.rs
git commit -m "feat: pass file category through Everything search"
~~~

### Task 3: Add Category Selection and Tab Cycling to Core State

**Files:**
- Modify: src/launcher-core.ts:295-310, 517-646, 1390-1400
- Test: src/launcher.test.tsx core/file-search tests

**Interfaces:**
- Consumes: existing FileCategory, FileSearchOwner, and beginFileSearch client path.
- Produces: functional setFileCategory(category) and /find search-input Tab cycling.

- [ ] Step 1: Add failing core tests.

Cover category selection clearing old results, non-empty category search, empty-keyword no-call, Shift+Tab wrap, and older response rejection.

~~~ts
core.setFileCategory('pdf')
expect(client.searchFiles).toHaveBeenLastCalledWith(
  expect.objectContaining({ category: 'pdf' }),
)
expect(core.getSnapshot().file?.results).toEqual([])
~~~

- [ ] Step 2: Run the focused frontend tests and verify failure.

~~~text
npm.cmd test -- --dir src launcher
~~~

Expected: failure because setFileCategory currently forces all and Tab is not category-aware.

- [ ] Step 3: Implement category transitions.

Define the fixed category order once. Make setFileCategory validate and no-op identical values, clear file results/request state, advance the file query sequence, and call beginFileSearch only for a non-empty query. Extend the input key path to route Tab and Shift+Tab to the category transition while preserving the IME guard.

- [ ] Step 4: Run focused frontend tests.

Run the same command. Expected: selection, empty-query, wrap, and stale-response tests pass.

- [ ] Step 5: Commit.

~~~text
git add src/launcher-core.ts src/launcher.test.tsx
git commit -m "feat: add file category selection and tab cycling"
~~~

### Task 4: Render the Category Sidebar

**Files:**
- Modify: src/launcher-view.tsx:295-487
- Test: src/launcher.test.tsx view/render tests

**Interfaces:**
- Consumes: snapshot.file.category, the category order constant, and core.setFileCategory from Task 3.
- Produces: accessible category navigation with active state and input Tab behavior.

- [ ] Step 1: Add failing view tests.

Assert that the ten labels render in order, the selected category has the selected accessibility state, clicking a category calls core.setFileCategory, and the input Tab handler prevents default focus traversal while Shift+Tab reverses.

- [ ] Step 2: Run focused view tests and verify failure.

Run the existing launcher render test filter. Expected: failure because no category navigation is rendered.

- [ ] Step 3: Implement the view.

Render a nav/list of buttons inside the file workspace. Keep labels visible, use stable IDs, set selected accessibility attributes, and return focus to the query input after mouse selection. Route only file-mode search-input Tab events to the core category action; preserve composition handling and existing Enter/Escape behavior.

- [ ] Step 4: Run focused view tests.

Expected: all category rendering and keyboard tests pass.

- [ ] Step 5: Commit.

~~~text
git add src/launcher-view.tsx src/launcher.test.tsx
git commit -m "feat: render file category sidebar"
~~~

### Task 5: Apply Responsive Sidebar Styling

**Files:**
- Modify: src/styles.css:74-160, 571-585
- Test: src/launcher.test.tsx markup/class assertions if required

**Interfaces:**
- Consumes: file workspace class names from Task 4.
- Produces: desktop three-column layout and narrow horizontal category row.

- [ ] Step 1: Add layout assertions or a visual smoke check.

Verify the workspace has category, results, preview, and toolbar areas in the generated markup/classes; verify the narrow breakpoint exposes a scrollable category row and keeps preview below results.

- [ ] Step 2: Implement desktop and narrow rules.

Add grid areas for query, categories, results, preview, and toolbar. Use a stable category column width, a scrollable result list, and a plain navigation panel. At the existing narrow breakpoint, collapse to one column and make categories horizontally scrollable.

- [ ] Step 3: Run frontend tests and build.

~~~text
npm.cmd test -- --dir src
npm.cmd run build
~~~

Expected: all root frontend tests pass and the production build completes.

- [ ] Step 4: Commit.

~~~text
git add src/styles.css
git commit -m "feat: style responsive file category sidebar"
~~~

### Task 6: Cross-Check Documentation and Full Verification

**Files:**
- Add: docs/superpowers/specs/2026-08-02-find-everything-category-sidebar-design.md
- Add: docs/superpowers/plans/2026-08-02-find-everything-category-sidebar.md
- Modify: docs/superpowers/specs/2026-07-30-find-everything-mvp-integration-design.md only if a cross-reference is needed to distinguish its historical fixed-category scope

**Interfaces:**
- Consumes: completed Tasks 1-5.
- Produces: reviewed design/implementation record and verified feature branch.

- [ ] Step 1: Cross-check documentation against implementation.

Verify every category ID, extension list, Tab behavior, empty-query rule, and explicit non-goal matches the code. Keep the earlier MVP wording clearly historical if it remains unchanged.

- [ ] Step 2: Run complete focused verification.

~~~text
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo check
npm.cmd test -- --dir src
npm.cmd run build
~~~

Expected: all existing and new tests pass, Clippy has no warnings, and the frontend build succeeds.

- [ ] Step 3: Run manual Everything acceptance.

With Everything 1.4 running, test all ten categories using a keyword that matches known folders, documents, images, media, and archives; test Tab/Shift+Tab, empty keyword, Chinese/special characters, Enter file/folder execution, and Everything exit/unavailable behavior.

- [ ] Step 4: Review the diff and commit the verification record.

~~~text
git diff --check
git status --short
git add docs/superpowers/specs/2026-08-02-find-everything-category-sidebar-design.md docs/superpowers/plans/2026-08-02-find-everything-category-sidebar.md
git commit -m "docs: specify Everything category sidebar"
~~~

The final branch must contain only the category-sidebar implementation and its design/plan documentation. Existing unrelated worktree changes remain untouched.
