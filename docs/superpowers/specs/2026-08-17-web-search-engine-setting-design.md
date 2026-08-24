# Web Search Engine Setting Design

**Status:** Draft - awaiting written review

## Goal

Let users choose Bing, Baidu, or Google for the launcher browser-search result, persist that choice across restarts, and keep each published result bound to the engine shown in its title.

## User Contract

- General settings contains one `搜索引擎` select with `Bing`, `百度`, and `Google`.
- Bing is the default for new and legacy settings.
- Changing the select saves immediately. The settings window remains open while saving and after either success or failure.
- While a save is pending, the select is disabled to prevent overlapping writes.
- A successful save affects only browser-search results generated afterward.
- A failed save restores the previous durable value and shows the existing settings error treatment.
- Resetting settings restores Bing and persists it through the existing reset flow.
- An ordinary launcher query shows one engine-specific result titled exactly `Bing 搜索`, `百度搜索`, or `Google 搜索`, with the subtitle `搜索：{query}`.
- Math results and slash-command routes remain exclusive and do not show a browser-search result.
- Executing the result closes the launcher only after Windows accepts the generated HTTPS URI. Failure keeps the launcher visible and shows `操作不可用，请重试。`.

## Data Contract

Rust owns a strict enum:

```rust
enum WebSearchEngine {
    Bing,
    Baidu,
    Google,
}
```

The persisted `Settings` field is `web_search_engine`; its serialized and frontend name is `webSearchEngine`. Missing values deserialize as `Bing`. Unknown values are invalid and use the existing corrupt-settings recovery path rather than silently selecting another engine.

`SettingsView`, `UserSettingsUpdate`, the frontend settings snapshot, and reset defaults carry the same three-value contract. No custom engine names, aliases, parameters, or URLs are supported.

## Provider Registry

The backend contains a fixed mapping:

| Engine | Result title | HTTPS endpoint | Query key |
| --- | --- | --- | --- |
| Bing | `Bing 搜索` | `https://www.bing.com/search` | `q` |
| Baidu | `百度搜索` | `https://www.baidu.com/s` | `wd` |
| Google | `Google 搜索` | `https://www.google.com/search` | `q` |

URLs are built with the structured URL API. The query remains one encoded value, including spaces, Unicode, and reserved characters. The frontend and plugins never provide an endpoint or complete URL.

## Result Ownership And Execution

When application search publishes an ordinary-text result set, the backend reads the current durable setting once and creates a private action:

```rust
OpenWebSearch {
    engine: WebSearchEngine,
    query: String,
}
```

The public DTO contains only the engine-specific title, the exact subtitle `搜索：{query}`, opaque request ID, and opaque result ID. The action snapshots the engine, so changing settings cannot alter already published results.

Execution first resolves the action through the existing main-window `ResultRegistry`. The backend then maps the captured engine to its fixed endpoint, encodes the captured query, and asks Windows to open the HTTPS URI. A stale or unknown result performs no side effect.

If Windows rejects the URI request, execution returns `webSearchFailed` and does not clear or hide the launcher. If Windows accepts it, execution uses the existing launcher clear-and-hide path and returns `launchRequested`. Acceptance by Windows does not assert that the provider is reachable over the network.

## Settings Flow

The select lives in the existing General settings form and follows the current theme-setting interaction pattern. A change creates one settings save owner and temporarily locks the form. Success updates the durable baseline. Failure restores the prior engine without closing or navigating away from settings.

The existing atomic settings write, backup, and recovery behavior remains authoritative. No second settings file or provider-specific storage is introduced.

## Failure Behavior

- Missing persisted field: use Bing.
- Unknown persisted enum: invoke existing invalid-settings recovery.
- Persistence or worker failure: restore the durable selection, keep settings visible, and show the settings failure message.
- Stale or unknown result authorization: reject before opening a URI.
- Invalid captured query or URL construction failure: return `webSearchFailed`, keep the launcher visible.
- Windows URI launch failure: return `webSearchFailed`, keep the launcher visible.
- Provider network failure after Windows accepts the URI: owned by the browser and outside UiPilot's observable success contract.

## Testing

Focused automated coverage must include:

- legacy settings default to Bing;
- Bing, Baidu, and Google round-trip through persistence;
- an unknown engine uses existing invalid-settings recovery;
- reset persists Bing;
- the settings select saves immediately, disables while pending, remains open, and rolls back on failure;
- ordinary queries publish the selected title, the exact subtitle `搜索：{query}`, and a private action containing the matching engine;
- changing settings does not mutate an existing action;
- math and slash-command searches do not publish a browser-search result;
- all provider URLs preserve Unicode, spaces, and reserved characters as one query value;
- stale authorization and URI launch failure perform no launcher hide;
- successful URI launch occurs before launcher clear-and-hide.

## Manual Acceptance

1. Select each engine and execute an ordinary query; verify the matching title and provider.
2. Restart UiPilot and verify the last successful selection remains active.
3. Reset settings and verify the selection returns to Bing after restart.

Manual acceptance may change foreground focus and open the default browser. It requires explicit user action; automated development must not control the user's mouse or keyboard.

## Non-Goals

- Custom search providers or URLs.
- Provider-specific query parameters, regions, accounts, or safe-search controls.
- Detecting the default browser's internal search-engine preference.
- Verifying provider reachability or page-load success.
