# sdk-test Visual Alignment Design

**Status:** Draft - awaiting written review

## Goal

Align UiPilot's user-visible host surfaces with the visual language of
`D:\code\desktop-worktrees\sdk-test` while retaining UiPilot's current Ant
Design component stack, compact window geometry, behavior, and ownership
contracts.

The aligned surfaces are:

- the main launcher;
- Settings, including General and public-plugin management;
- the `/find` window;
- the host-owned container and title bar of public-plugin windows.

## Decision

UiPilot keeps `antd` and does not adopt Tailwind or Radix. A local theme layer
projects sdk-test's semantic colors and component treatment into Ant Design and
UiPilot's existing CSS.

UiPilot adds `lucide-react` and uses Lucide for host action icons. Existing
`@ant-design/icons` imports are migrated within the four in-scope surfaces and
the dependency is removed only after the repository contains no remaining
imports.

This is visual alignment, not shared cross-repository source code. UiPilot and
sdk-test remain independently versioned projects.

## User Contract

- `system`, `dark`, and `light` keep their existing persistence and switching
  behavior.
- All four in-scope host surfaces use one coherent light or dark appearance at
  the same time.
- Window dimensions, information density, navigation structure, keyboard
  behavior, focus transfer, pin behavior, auto-hide behavior, and command
  execution do not change.
- Public-plugin content remains plugin-owned. Only the host title bar, outer
  surface, pin button, close button, and host-provided theme variables change.
- The launcher keeps the previously accepted colored semantic treatment for
  built-in result icons. Host command buttons use neutral Lucide icons.
- Forced-colors behavior and accessible names remain supported.

## Non-Goals

- Migrating to Tailwind, Radix, shadcn/ui, or a shared npm package.
- Copying sdk-test layouts or converting UiPilot into a workbench interface.
- Changing window sizes, result density, settings information architecture, or
  `/find` columns.
- Restyling HTML, CSS, or controls supplied by a public plugin.
- Changing settings persistence, Tauri commands, capabilities, native window
  events, or result authorization.
- Adding animation, decorative cards, gradients, or new theme choices.

## Theme Architecture

### Shared React Theme

A small UiPilot-owned theme module is the only source for Ant Design theme
configuration. It exposes:

- the final color scheme type, `light | dark`;
- the semantic token set for each scheme;
- a function that returns the Ant Design `ThemeConfig` for a scheme;
- the shared compact component sizing and radius values.

LauncherView, FindView, and PluginWindowView consume this module instead of
constructing independent Ant Design configurations. Existing media-query and
persisted-preference ownership remain unchanged; the module receives only the
already-resolved final scheme.

### Shared CSS Tokens

A dedicated theme stylesheet defines UiPilot-prefixed custom properties derived
from sdk-test's current `index.css` contract:

```text
background
foreground
surface/card
surface-raised/popover
primary and primary-foreground
secondary
muted and muted-foreground
accent and accent-foreground
destructive
border
input
ring
```

The canonical light and dark values follow sdk-test's existing OKLCH tokens.
Where Ant Design requires a parseable derived color, the theme module owns the
corresponding stable sRGB value. Components must not introduce another local
palette for generic surfaces or controls.

Existing plugin-facing variables such as `--uipilot-color-surface`,
`--uipilot-color-text`, `--uipilot-color-border`, and
`--uipilot-color-accent` remain compatible and are mapped from the new host
tokens.

### Typography And Geometry

- The host font stack follows sdk-test's neutral sans-serif treatment, with a
  Windows CJK fallback.
- Letter spacing remains `0`.
- Base surface radius follows sdk-test; compact controls use the corresponding
  smaller radius.
- Existing fixed dimensions and responsive grid tracks remain stable.
- Inputs, buttons, selects, tabs, tooltips, popovers, confirmation dialogs, and
  switches receive their common appearance through Ant Design tokens before
  adding component-specific CSS.

## Surface Treatment

### Main Launcher

- Keep the current input-and-results structure and row heights.
- Use the shared surface, input, border, muted-text, focus-ring, and selected-row
  tokens.
- Preserve built-in result icon meaning and color while replacing their glyphs
  with Lucide equivalents where available.
- Do not add a title bar, card wrappers, or explanatory text.

### Settings

- Keep the two vertical tabs and overlay scrollbar behavior.
- Apply shared tokens to the header, tab states, forms, public-plugin sections,
  buttons, destructive actions, popovers, and scrollbars.
- Public-plugin entries remain unframed sections rather than nested cards.
- The empty legacy plugin inventory remains hidden under its existing UI rule.

### `/find`

- Keep the current query header, categories, results, preview, footer, and
  status regions.
- Apply the same surface hierarchy and control treatment as the launcher.
- Pin and close use Lucide icons with existing tooltips, accessible names, and
  state semantics.
- Category selection and result selection use the shared accent treatment.

### Public-Plugin Window Container

- Restyle only the host-owned shell and title bar.
- Pin and close use the same components and states as `/find`.
- The shell publishes the mapped plugin theme variables before plugin content
  renders.
- Plugin content dimensions, scrolling, scripts, and CSS remain untouched.

## Icon Contract

Host actions use Lucide icons whenever an equivalent exists. The minimum mapping
includes close, pin, refresh, folder/open, install/import, delete, reset, save,
search, calculator, file search, and browser search.

Icon-only buttons retain tooltips and stable accessible labels. Selected pin
state is communicated by both visual state and `aria-pressed`; color alone is
not the state contract.

## Migration Order

1. Add the shared React theme module, CSS token layer, and Lucide dependency.
2. Migrate the public-plugin host container.
3. Migrate `/find`.
4. Migrate the main launcher and Settings.
5. Remove superseded palette rules and remove `@ant-design/icons` only if no
   imports remain.

Each stage must leave the application buildable. Temporary coexistence is
allowed during the migration, but the completed state has one generic host
palette and one host action-icon library.

## Failure And Compatibility Behavior

- Theme resolution failure is not introduced because the theme module is pure
  local data with no I/O.
- Existing persisted theme values remain authoritative.
- Public plugins continue receiving the same variable names, so compliant
  packages do not require a manifest or runtime update.
- Missing icons fail at build time; no runtime icon download or fallback is
  added.
- No native-window or foreground-focus automation is part of visual tests.

## Testing

Focused automated coverage must verify:

- light and dark semantic token projections are complete and deterministic;
- all three host React views use the shared theme module;
- in-scope host action icons use Lucide and retain accessible names;
- existing theme preference, focus, pin, close, scrolling, keyboard, and result
  execution tests remain green;
- plugin-facing theme variable names remain present;
- no Tailwind or Radix dependency is added;
- no `@ant-design/icons` dependency is removed while imports remain;
- TypeScript and the production Vite build pass.

## Manual Acceptance

The user verifies both light and dark modes for:

1. the main launcher with application, `/find`, calculator, and browser-search
   results;
2. General and Plugins settings tabs, including scrolling, popovers, switches,
   and destructive actions;
3. `/find`, including pin, close, category selection, result selection, and
   preview;
4. a public-plugin window, including host title bar, pin, close, and unchanged
   plugin content.

Screenshots should be compared with sdk-test for palette, typography, borders,
radii, control states, and icon language rather than workbench layout. Any step
that changes foreground focus or opens real windows requires explicit user
action; development must not control the user's mouse or keyboard.

## Acceptance Criteria

- The four in-scope host surfaces visibly belong to one sdk-test-aligned design
  language in light and dark modes.
- UiPilot still uses Ant Design and does not add Tailwind or Radix.
- Generic host colors are sourced from the shared UiPilot theme layer rather
  than view-specific palettes.
- Window geometry and all previously accepted behaviors remain unchanged.
- Public-plugin content remains isolated and functional.
- Automated tests and the production frontend build pass before manual
  acceptance begins.
