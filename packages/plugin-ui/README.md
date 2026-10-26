# Plugin UI: first subset

Dependency-free, package-local ES modules and CSS for List, Detail state and ActionBar.
This is the first reusable subset, not a complete Form/ActionPanel framework or M5 completion.
No Host bridge, storage, networking or business state is owned by this module.

## Consume

Copy src/ui.js and src/ui.css into your plugin's package/dist/ui/, import
./ui/ui.js from the content module and link ./ui/ui.css from its HTML.
Ship both files in the plugin archive; the Host does not install a UI library.
Keep the original native HTML buttons, inputs, labels and domain empty state.

- createList(container, {onActivate}): update({items, selectedId, disabled, emptyText})
  takes unique stable string IDs and text titles. move(delta) only focuses rows.
  Native button Enter/Space and clicks invoke onActivate(id) through one click path.
  Unmodified arrows move within the list; endpoints do not wrap. Update reuses nodes,
  preserves focused identity, and only restores a removed row's neighbor when focus
  was inside the list. No initial selection or focus is assigned.
- createDetail({empty, content, label}): update({hasContent, busy, text}) controls
  visibility, plain status text and content aria-busy, preserving all form children.
- createActions({id: {button, onInvoke}}): update({id: {disabled}}) controls native
  disabled state. The consumer still guards asynchronous requests and ownership.
- Every instance exposes dispose(); after disposal updates/events have no effect.

Use prefixed layout/sidebar/toolbar/detail/detail-label/actions classes in HTML;
createList applies its list/item classes itself. CSS uses only public Host theme
tokens, a 220px sidebar, 48px headers, scrollbar space and a compact narrow layout.
The consumer owns basic control styles and sets data-theme from Host updates.
Stable IDs are a caller contract; do not recycle an ID for a different entity.

## Templates consumer

From repository root:

    node packages/plugin-ui/scripts/sync-templates.mjs
    node packages/plugin-ui/scripts/sync-templates.mjs --check
    node --test packages/plugin-ui/tests/ui.test.mjs examples/public-plugins/com.uipilot.templates/tests/*.test.js

Only the two named files are synchronized to the fixed templates destination.
--check is read-only and exits nonzero on a missing/different copy. Ordinary
templates tests also check equality and exercise drift with an isolated fixture.

The library tests exercise two independent lists/actions, focus, keyed reconciliation,
plain text, disabled state and disposal. Panel tests load the real ES module with its
relative import. JSDOM does not verify layout, real native button key synthesis or Host
focus transfer; Windows Host keyboard, narrow layouts and both themes require user acceptance.
