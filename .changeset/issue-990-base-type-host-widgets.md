---
"@brink-lang/editor": minor
---

Host-registered argument handlers for primitive base types (#990). A host can
now register a widget against a **base type** (`bool` | `int` | `float` |
`string`), not just a `host.<vendor>.<name>` semantic id — e.g.
`setHostWidgets([{ type: "bool", … }])` gives every `bool`-typed argument the
host's own toggle, in the host's own design system, with zero brink-shipped
opinion on primitives. This runs through the same `matchHostWidget` fallback
that already resolved semantic types; it is now an intentional, documented,
tested part of the contract.

New API surface:

- `getHostWidget` and `matchHostWidget` are now exported, so a host (or a
  consumer's tests) can introspect widget registration/resolution directly.
- `ArgumentWidget.editor.surface` gains `"inline"` alongside `"popover"` /
  `"modal"` — mounts the widget's control directly in the argument Form's row
  (where a text field would sit) instead of `buildHostField`'s summary-chip +
  expandable editor. The right shape for a primitive control (a bool toggle,
  a number stepper) that IS the field, not something you open an editor on.
  Only the Form honors it; the in-editor Edit/Fill affordances have no row to
  mount into and keep the popover chrome for the same widget.

**Behavior change:** the argument Form's per-field control precedence is now
**color → hostWidget → values → text** (previously values was checked before
hostWidget). A field that declares both a host widget and `values` now gets
the widget — before this fix a host widget could never win over a plain
values dropdown on the same type, which also blocked rich pickers (e.g. an
icon-grid item browser) for semantic types that also carry `setHostValues`
labels. A field with `values` and no `hostWidget` is unaffected.
