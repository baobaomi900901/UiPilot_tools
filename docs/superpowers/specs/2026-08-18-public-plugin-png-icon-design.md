# Public Plugin PNG Icon Design

**Status:** Draft - interaction decisions approved; written review pending

## Goal

Give each public plugin one optional, package-owned PNG identity icon that UiPilot renders consistently in host-controlled surfaces. Preserve schema-v1 compatibility, keep plugin Runtime output unable to choose icons, and retain the package installer's atomic failure behavior.

## User Contract

- A public plugin may include one optional icon at the exact package-root path `icon.png`.
- No manifest field is added. Presence of `icon.png` opts into the custom icon; absence uses UiPilot's default plugin glyph.
- The same icon identifies the plugin in command discovery, plugin-owned main results, the public-plugin settings list, install confirmation, and the plugin child-window header.
- The host displays the PNG on a theme-aware neutral tile. The PNG is never recolored, cropped, stretched, made interactive, or used as accessible text.
- Plugin Runtime responses cannot provide, replace, or override an icon. Every main-result row from one plugin uses that plugin's package icon.
- The system tray icon and native Windows taskbar icon do not change per plugin.

## Package Contract

The public-plugin package format remains schema version 1. `plugin.json` gains no new key.

`icon.png` is optional. When present, all of the following are required:

- the canonical relative path is exactly `icon.png`, including case;
- it is the package's only PNG resource;
- its encoded size is between 1 byte and 128 KiB;
- a standard PNG decoder can completely decode it;
- its decoded dimensions are exactly 128 by 128 pixels;
- it is a static single-frame PNG, not APNG.

The installer recognizes this one file as `image/png`. It does not infer the format from the extension alone and serves it with `X-Content-Type-Options: nosniff`. Any other `.png` path remains invalid in this version, including `Icon.png`, `assets/icon.png`, or an unused second PNG.

The package snapshot retains the validated original PNG bytes as an immutable resource. A missing icon remains valid and is not a warning. The fixed convention cannot become inconsistent with a manifest path because no such field exists.

## Validation And Atomicity

Package enumeration may recognize a lowercase `.png` candidate so the manifest-independent snapshot can be built, but post-snapshot validation accepts it only when the complete PNG resource set is either empty or exactly `{ "icon.png" }`.

Validation uses a bounded PNG decoder rather than custom signature or header parsing. It reads through the image data so truncated streams, invalid chunks, and malformed compressed data fail before prepare succeeds. Dimension and animation checks occur before the staged package can be committed.

Any icon validation failure returns the existing path-free invalid-package failure. A failed fresh install persists no plugin record. A failed update preserves the active package, generation, Runtime, settings, permissions, and previous icon.

## Host Icon URLs

UiPilot serves validated icons through a narrow branch of the existing `uipilot-public-plugin` custom protocol. General plugin assets do not become readable by the main window.

Installed icon URLs have this conceptual form:

```text
uipilot-public-plugin://localhost/__uipilot_icon/installed/<plugin-id>/<generation>/icon.png
```

Prepared-install icon URLs have this conceptual form:

```text
uipilot-public-plugin://localhost/__uipilot_icon/prepared/<prepare-token>/icon.png
```

The backend constructs every URL. Frontends never assemble a URL from an arbitrary filesystem path. Requests must have the exact path shape, no query or fragment, and the current identity:

- `main` may read a current installed icon, including an installed disabled or faulted plugin shown in settings;
- a `plugin-shell-*` window may read only its own current plugin icon;
- `main` may read a prepared icon only while its caller-bound prepare token is live;
- plugin Runtime windows, plugin content windows, `find`, unrelated shells, stale generations, expired tokens, canceled prepares, committed prepares, and uninstalled plugins are denied.

Denied and missing-resource responses use one fixed non-disclosing failure. Installed URLs include generation in their cache identity and may be immutable-cached. Prepared URLs are `no-store` and stop resolving immediately after prepare ownership ends.

## Wire Contracts

Icon references are host-owned optional metadata:

```typescript
interface PublicPluginInventoryItem {
  // existing fields omitted
  iconUrl: string | null
}

interface PublicPluginPrepareSummary {
  // existing fields omitted
  iconUrl: string | null
}

interface ResultItem {
  // existing fields omitted
  pluginIconUrl?: string
}

interface PublicPluginWindowIdentity {
  name: string
  iconUrl: string | null
}
```

`pluginIconUrl` is separate from the existing application `icon` data URL. The frontend accepts it only when it matches the exact host icon URL grammar. A malformed value is discarded and falls back to the default plugin glyph; it never becomes a navigation or execution target.

The activation snapshot supplies the icon identity for command suggestions and plugin-owned main results. Runtime response DTOs contain no icon field. The plugin shell obtains `PublicPluginWindowIdentity` through a caller-guarded, read-only command derived from its exact shell label; it cannot request another plugin ID.

## UI Rendering

A shared `PluginIcon` component owns URL validation, loading failure fallback, stable sizing, and theme styling.

- command discovery and plugin main-result rows use a 28 by 28 pixel container;
- public-plugin settings rows use 36 by 36 pixels to the left of the plugin name;
- install confirmation uses 32 by 32 pixels;
- plugin child-window headers use 20 by 20 pixels, followed by the manifest `name` instead of the fixed `UiPilot` text.

The source image uses `object-fit: contain`. Each size has a fixed box before loading, with a small theme-token neutral background, a one-pixel theme-token border, and the established small corner radius. Transparent and opaque PNGs are both supported.

When no icon exists or loading fails, the same box renders a Lucide plugin glyph. Images adjacent to a visible plugin name use empty alternative text and are not focusable, draggable, clickable, or tooltip owners.

## Data Flow

1. Prepare snapshots all package files and parses `plugin.json`.
2. Icon validation accepts no PNG or exactly one valid root `icon.png`.
3. Prepare summary exposes a token-bound preview URL when an icon exists.
4. Commit moves the already-validated immutable package into the active generation.
5. Inventory, command discovery, and plugin result publication construct installed generation-bound URLs from host state.
6. The shared frontend component requests the URL and renders the PNG or its local fallback.
7. Update, uninstall, cancel, expiry, or generation replacement retires the old URL through existing ownership state; no separate icon cache lifecycle is introduced.

## Failure Behavior

- Missing optional icon: continue with the default glyph.
- Invalid packaged icon: reject prepare atomically with the existing fixed invalid-package error.
- Icon URL generation failure: omit the URL and keep the surrounding operation usable.
- Stale or unauthorized icon request: fixed denial with no path, plugin existence, token, or generation detail.
- Frontend URL or decode failure: render the default glyph without failing search, settings, results, or the child window.
- Plugin update failure: retain the prior active version and icon.

## Demo Packages

The user-supplied files are:

```text
examples/public-plugins/com.uipilot.demo-win/package/icon.png
examples/public-plugins/com.uipilot.demo-return/package/icon.png
```

Both are 128 by 128 PNGs and below the size limit. Each demo package advances from version `1.0.1` to `1.0.2`. Repository staging and packaging expectations increase by one resource per demo.

## Testing

Focused automated coverage is limited to the feature's risk boundaries:

- table-driven package validation for absent, valid, corrupt, wrong-size, APNG, wrong-path, extra-PNG, and oversized cases;
- failed update retains the current generation and icon;
- current `main` and matching shell access succeeds while Runtime, content, unrelated labels, stale generations, and expired prepare tokens fail;
- inventory, prepare summary, command suggestions, plugin main results, and window identity expose only host-generated icon URLs;
- Runtime results cannot inject an icon;
- the shared frontend component renders all four sizes, uses the neutral tile, and falls back without changing surrounding operation state;
- both version-`1.0.2` demo packages stage and package with their exact root icon.

Full Rust tests, frontend tests, schema generation checks, formatting, and production build remain required. Real-window checks are manual: Dark and Light themes, install preview, `/d`, a `demo-return` result, and the `demo-win` child header. No synthesized mouse or keyboard input is permitted.

## Acceptance

1. A plugin without `icon.png` installs and displays the same default glyph in every host surface.
2. An invalid root icon or PNG at any other path fails prepare without changing an installed version.
3. Installing the version-`1.0.2` demos shows their PNGs in the confirmation UI and settings list.
4. `/d` shows each demo PNG beside its command suggestion.
5. `demo-return` result rows use the demo-return icon and retain existing copy behavior.
6. The `demo-win` child header shows its icon and manifest name while pin, close, focus, and singleton behavior remain unchanged.
7. Dark and Light themes use the same unmodified PNG on the approved neutral tile without layout movement.

## Out Of Scope

- per-result Runtime icon overrides;
- multiple icon sizes, light/dark variants, SVG, ICO, WebP, animated PNG, or arbitrary plugin image assets;
- changing icons from the settings UI;
- exposing filesystem paths or general package assets to the main window;
- plugin-specific tray or native taskbar icons;
- changing plugin content-page image permissions.
