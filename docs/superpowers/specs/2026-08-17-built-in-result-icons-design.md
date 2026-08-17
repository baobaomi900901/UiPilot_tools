# Built-in Result Icons Design

**Status:** Approved for implementation

## Goal

Replace the generic square shown for UiPilot's three built-in result types with recognizable colored vector icons while preserving existing result ordering and behavior.

## User Contract

- `/find` uses a colored open-folder icon with a search badge.
- Built-in calculator results use a colored calculator icon.
- Browser-search results use a colored browser icon with a search badge.
- Ordinary application results continue to use their existing PNG icon and fallback behavior.
- Icons remain 28 by 28 CSS pixels and do not resize result rows.
- Light, dark, selected, keyboard, and forced-colors behavior remain usable.

## Architecture

Add an optional semantic `iconKind` to the result DTO and view snapshot. The allowed built-in values are `find`, `calculator`, and `webSearch`; arbitrary plugins and applications cannot select these kinds through title text.

The backend assigns `calculator` and `webSearch` when publishing those built-in results. The frontend assigns `find` to its local `/find` result. `LauncherView` renders the semantic kind before considering the existing PNG icon, then falls back to the current square only when neither is available.

## Visual Design

Use the already-installed MIT-licensed `@ant-design/icons` package:

- Find: `FolderOpenTwoTone` with a small blue `SearchOutlined` badge.
- Calculator: `CalculatorTwoTone`.
- Web search: `ChromeOutlined` with a small green `SearchOutlined` badge.

Composite icons use a stable 28 by 28 wrapper. The search badge is positioned inside that wrapper and cannot affect layout. Decorative SVGs remain hidden from assistive technology because the result title supplies the accessible name.

## Data And Failure Behavior

`iconKind` is presentation-only and never participates in result identity, authorization, execution, or persistence. Unknown wire values are rejected by the existing exact response parser. A missing kind preserves backward compatibility. PNG load failure retains the existing square fallback; semantic vector icons have no loading state.

## Testing

- The protocol parser accepts the three exact values, accepts omission, and rejects unknown values.
- Calculator and browser-search publications expose the correct semantic kind without exposing private actions.
- The local `/find` result carries `find`.
- The view renders each Ant Design icon and keeps ordinary PNG/fallback rendering unchanged.
- Existing keyboard, result ordering, and execution tests remain unchanged.

## Acceptance

Entering ordinary text shows `/find` with the folder-search icon and browser search with the browser-search icon. Entering a valid calculation shows the calculator icon. All three icons are colored, remain aligned at the existing size, and do not change what Enter executes.
