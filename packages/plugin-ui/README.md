# Plugin UI primitives

Dependency-free, package-local ES modules and CSS for List, Detail state,
ActionBar, native Form binding and ActionPanel. Components own presentation and
DOM events; consumers own values, validation rules, requests and business state.
They have no Host bridge, storage or network access. This is a bounded reusable
subset, not a dynamic form renderer or completion of the whole M5 milestone.

## Consume

Copy all four files from src/ into your plugin's package/dist/ui/:
ui.js, ui.css, form.js and action-panel.js. Import ./ui/ui.js from the content
module and link ./ui/ui.css from its HTML. Ship all four files in the archive;
the Host does not install a UI library. Keep native HTML controls and labels.

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
createList applies its list/item classes itself. CSS uses public Host theme tokens,
a 220px sidebar, 48px headers, scrollbar space and a compact narrow layout.
The consumer owns basic control styles and sets data-theme from Host updates.
Stable IDs are a caller contract; do not recycle an ID for a different entity.

## Form

createForm({fields}) binds an ordered array of {id, control, error, validate?}.
IDs are unique and stable; control is a caller-owned native input/select/textarea,
and error is a text element with a unique DOM id. validate(control) returns a
message or null. No values, checked state, coercion, truncation or submission
are managed by the component. Use the uipilot-ui-field-error class on error nodes.

```js
import { createForm } from './ui/ui.js'
const fields = [{
  id: 'title', control: titleInput, error: titleError,
  validate: (control) => control.value.trim() ? null : 'Enter a title.',
}]
const form = createForm({ fields })
// Call in an explicit user action; only validate the relevant fields.
if (form.validate(['title'])) saveDraft()
// Apply server errors only after checking session, draft and edit ownership.
form.setErrors(new Map([['title', 'This title is unavailable.']]))
form.clear(['title'])
form.setFields(fields)
// On teardown:
form.dispose()
```

validate(ids?) returns whether the requested fields (all fields when omitted)
are valid and may focus the first invalid control only in a focused document.
setErrors(Map) updates only known supplied IDs and never focuses. clear(ids?)
clears component-owned errors. Error text uses textContent, aria-invalid and an
added aria-describedby token; existing description tokens are preserved.
input/change clears that field's error without validation or requests. setFields
preserves state for identical IDs/controls/error elements, releases removed or
replaced fields and binds new controls. dispose releases listeners and errors.

## ActionPanel

createActionPanel({dialog, trigger, list, closeButton, cancelButton, actions})
uses a caller-owned native dialog with a visible title and aria-labelledby,
a list container, and visible close/cancel buttons. Each action has a unique
{id, label, onInvoke}; labels are plain text and rows are ordinary type=button.

```js
import { createActionPanel } from './ui/ui.js'
const actionPanel = createActionPanel({
  dialog, trigger, list, closeButton, cancelButton,
  actions: [{ id: 'save', label: 'Save', onInvoke: saveDraft }],
})
actionPanel.update(new Map([['save', { disabled: busy, hidden: false }]]))
// Trigger clicks call open() automatically. A content shortcut may also call it.
actionPanel.open()
actionPanel.close() // cancel, with guarded restoration of prior focus
// Session replacement/teardown must not explicitly restore previous-session focus.
actionPanel.close({ restoreFocus: false })
actionPanel.dispose()
```

update accepts a Map or own record properties by action ID; supplied entries set
both disabled and hidden, omitted entries stay unchanged. Nodes are stable and
updates keep focus inside the modal when its focused action becomes unavailable.
open is ignored when disposed, already open, the trigger is unavailable or another
dialog is open. Initial focus goes to the first available action, otherwise cancel.
Plain Up/Down/Home/End navigate without wrapping; Enter/Space and Tab/Shift+Tab
retain native button/dialog behavior. IME and modified direction keys are untouched.

Invoking a currently available action synchronously closes without explicit focus
restoration, then calls its callback once. Cancellation never calls an action.
Queued close events dispatch no business and restore no old focus. Native dialog
focus restoration still belongs to the browser. Consumers guard background Host
and document shortcuts while any modal is open, and retain request/recovery guards.

## Templates consumer

From repository root:

    node packages/plugin-ui/scripts/sync-templates.mjs
    node packages/plugin-ui/scripts/sync-templates.mjs --check
    node --test packages/plugin-ui/tests/*.test.mjs examples/public-plugins/com.uipilot.templates/tests/*.test.js

Only the four named files are synchronized to the fixed templates destination.
--check is read-only and exits nonzero on a missing/different copy. Ordinary
templates tests check equality and exercise drift with an isolated fixture.
Tests import real ES modules. JSDOM does not verify layout, real native button
key synthesis, native modal trapping or Host focus transfer; Windows Host keyboard,
narrow layouts and both themes require user acceptance.
