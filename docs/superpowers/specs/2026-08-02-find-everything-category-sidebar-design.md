# /find Everything Category Sidebar Design

## Status

Approved design for the category-filter milestone that extends the Everything MVP.

The preceding MVP specification deliberately fixed the category to all. This specification supersedes that limitation for the /find file workspace while retaining its literal query encoding, 200-result limit, modified-time descending sort, Rust-owned actions, and stale-response checks.

## Goal

Add a left category panel to /find so a non-empty filename query can be searched within a stable, user-visible category without locally filtering an incomplete result set.

## User Contract

The category list, in this order, is fixed:

| ID | Label | Matching rule |
| --- | --- | --- |
| all | 全部 | Files and folders; no extra predicate |
| folder | 文件夹 | folder: |
| excel | Excel | file: ext:xls;xlsx;xlsm;xlsb;csv |
| word | Word | file: ext:doc;docx;docm;rtf |
| ppt | PPT | file: ext:ppt;pptx;pptm |
| pdf | PDF | file: ext:pdf |
| image | 图片 | file: ext:bmp;gif;heic;jpeg;jpg;png;svg;tif;tiff;webp |
| video | 视频 | file: ext:avi;m4v;mkv;mov;mp4;webm;wmv |
| audio | 音频 | file: ext:aac;flac;m4a;mp3;ogg;wav;wma |
| archive | 压缩包 | file: ext:7z;bz2;gz;rar;tar;tgz;zip |

The extension sets are explicit and version-independent. They match the existing file classification rules; Everything macros such as image: are not used because their membership may vary by Everything version or configuration.

### Query behavior

- /find never sends an empty query. With an empty keyword, selecting a category updates the selected state only; no browsing search is started.
- A non-empty keyword is encoded exactly as in the Everything MVP: every Unicode scalar becomes #x<HEX>: and is prefixed with nowildcards:.
- The backend appends only a server-owned category predicate. The WebView sends the category ID, but the backend accepts only the ten IDs above and never concatenates WebView text into the predicate.
- Category and literal keyword are combined with AND semantics. all adds no predicate; every other category adds the fixed folder: or file: ext:... term.
- Each category change is a new search for the same keyword. It uses one Query2 request, offset 0, max 200, request flags 0x155, and date-modified descending sort.
- Results are authenticated and published using the existing ResultRegistry request/result identity rules. The result total remains the number of authenticated items actually published.

### Keyboard behavior

- While the /find search input is focused, Tab selects the next category and starts the category search when the keyword is non-empty.
- Shift+Tab selects the previous category.
- The category order wraps in both directions. Focus remains in the search input so the user can continue typing.
- Tab switching is handled only by the /find search input. Settings and other controls retain normal browser focus traversal.
- Tab is ignored while an IME composition is active. Mouse selection returns focus to the search input.
- The selected category is exposed with aria-current or an equivalent selected-state attribute; each change also updates the input/result accessible state.

## UI Layout

Desktop /find uses a three-column workspace below the query input:

    query:  [ full-width search input                          ]
    body:   [ categories ] [ results                         ] [ preview ]
    footer: [ status / total                         preview switch ]

The category panel is a plain vertical navigation region, not a nested card. It contains the ten text labels in the fixed order and a clear active state. The results list remains keyboard-first and the preview remains optional.

At narrow widths, the category panel becomes a horizontally scrollable row above the results list so the result and preview areas retain usable width. No category is hidden; the row scrolls to the active item.

## Backend Design

### Filter construction

Add a typed category-to-predicate mapping in src-tauri/src/file_search/everything.rs:

    pub(crate) fn category_predicate(category: FileCategory) -> &'static str

The function is exhaustive over the protocol enum and returns only static strings. Query assembly accepts the already encoded literal term and the typed predicate; it does not parse or re-encode user input.

The adapter entry point becomes conceptually:

    EverythingSearchState::search(&self, query: &str, category: FileCategory)

The existing deadline, request flags, sort, authentication, revision handling, and error behavior remain unchanged. A category search must not issue one request per extension and must not fetch unfiltered results for local filtering.

### Command validation

prepare_file_query validates the category against the protocol enum and keeps sort == modifiedDesc. The WebView category ID is parsed once and is not reused as a query fragment. The prepared query carries the typed category into search_files_with; the worker closure receives both query and category, so tests can assert the exact category without depending on WebView behavior.

The command still calls require_main_window first.

### Race and publication rules

Category changes increment the existing file query sequence and search owner token. The frontend clears the visible result IDs immediately. A response is publishable only when invocation, view epoch, sequence, literal query, category, and sort still match the current owner. A late response from a previous category can never replace the newer category result.

The ResultRegistry continues to own opaque request/result IDs and authenticated actions. No category or query string is trusted as an execution credential.

## Frontend Design

### State

PrivateFileState.category remains the selected FileCategory and is projected in the public snapshot. setFileCategory(category) must:

1. Ignore invalid/no-op transitions.
2. Set the selected category.
3. Clear visible file results, selected item, request ID, total, and status.
4. Advance the file query sequence and invalidate the previous search owner.
5. Start a new search only when the current keyword is non-empty.

The existing owner check gains no new trust boundary; it already includes category and sequence, and tests must prove both are required.

### View

launcher-view.tsx renders the category navigation only in file mode. Each category is a real button with a stable ID, selected state, and click handler. The search input handles Tab and Shift+Tab before the general launcher key handler, while preserving IME behavior.

The view does not add a sort control or expose Everything syntax. Preview, Enter, double-click, unavailable state, and status text remain unchanged.

## Development Runtime

npm run dev invokes scripts/dev-with-everything.ps1 before Vite. The wrapper uses the reviewed src-tauri/resources/everything/Everything.exe resource, or the equivalent local third-party copy, and starts it with -startup when no Everything process is already running. It waits for the process startup window, then starts Vite on port 1420. On exit it stops only the process started by the wrapper.

The executable and its license/lock are bundle resources. A fresh checkout must run powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-everything.ps1 once when the ignored local resource is absent. This development bootstrap does not implement the production installer, Windows Service, owner policy, or runtime supervisor.

## Error Behavior

- Invalid category: invalidFileQuery; no search or registry mutation occurs.
- Everything unavailable, timeout, overload, protocol failure, or revision exhaustion: existing searchUnavailable mapping.
- Category search failure clears the current visible result set for the requested category and leaves the file workspace unavailable until the next valid edit/category selection, matching the existing explicit-search behavior.
- No error response contains a local path or raw Everything query.

## Testing

### Rust adapter and command

- Every category maps to the exact fixed predicate in the table.
- all produces only the literal term; folder and extension categories produce an AND query.
- Every extension group is case-insensitive through Everything extension matching and does not treat the user keyword as syntax.
- All ten categories are accepted; unknown, empty, and malformed category values are rejected before begin_query.
- The command passes category to the Everything worker and keeps offset, max results, flags, sort, and deadline unchanged.
- A category result is stale when a newer sequence or category publishes first; stale work does not replace the current result.

### Frontend

- The category panel renders in the fixed order and reports the active category.
- Clicking a category clears old IDs and starts a new query for a non-empty keyword.
- Empty keyword category changes do not call searchFiles.
- Tab and Shift+Tab wrap through all categories, keep input focus, and do nothing during IME composition.
- Rapid category changes cannot let an older response replace the newest response.
- Preview and Enter continue to use the current opaque IDs only.
- Narrow layout preserves access to every category through horizontal scrolling.

### Manual acceptance

After npm run dev has started Everything, use a non-empty keyword matching multiple types:

1. /find shows the left category panel.
2. Each category returns only its declared file/folder class.
3. Tab and Shift+Tab change categories without leaving the input.
4. Chinese, spaces, wildcard-looking characters, and operator-looking characters remain literal.
5. Opening files and folders still works from each category.
6. Exiting Everything shows the existing unavailable state.

## Explicit Non-Goals

- Empty-keyword browsing.
- Sort selection or a new sort protocol.
- Local filtering of the current result list.
- Per-extension multi-query merging.
- Pagination, background refresh, production installer/Service management, permissions, or multi-user policy.

## Official Reference

- Everything search functions and file:, folder:, and ext: syntax: https://www.voidtools.com/support/everything/searching/
