# Plugin Settings Scrollbar Design

## Goal

Make the plugin settings page scrollbar visually identical to the main result list scrollbar.

## Design

- Keep `.settings-form` as the only settings-page scroll container.
- Reuse the main `.result-list` scrollbar width, transparent track, thumb color, radius, dark-mode color, and forced-colors behavior.
- Share the existing scrollbar CSS selectors instead of adding JavaScript, a component, or a nested plugin-list scroll area.

## Verification

- Add one source-level CSS regression assertion covering the shared selectors.
- Run the focused frontend test and production build.
