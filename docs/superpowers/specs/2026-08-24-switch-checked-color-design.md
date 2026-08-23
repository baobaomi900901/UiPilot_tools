# Switch Checked Color Design

**Status:** Approved by user on 2026-08-24

## Goal

Make every host-rendered Ant Design `Switch` use a green checked track in both light and dark themes. Preserve the existing unchecked appearance and every other component color.

## Design

`uiThemeConfig` owns the shared Ant Design theme for the launcher, settings, Find view, and public-plugin settings. Add a component-scoped `Switch` override there:

- checked track: `#16a34a`
- checked hover track: `#15803d`

Do not change the global `colorPrimary`, semantic primary tokens, individual `Switch` call sites, or add CSS selectors. Disabled behavior continues to be derived by Ant Design from the component theme.

## Verification

- A focused theme test asserts the exact `Switch` component tokens for light and dark configurations.
- Existing theme tests remain green.
- TypeScript and the production frontend build pass.

No real window, mouse, keyboard, or foreground-focus testing is required for this token-only change.
