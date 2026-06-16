---
"@brink-lang/studio": minor
"@brink-lang/web": minor
---

Argument widgets — rich, type-driven call-site authoring.

- A whole-call **Form** that renders one control per argument, chosen by the
  argument's type: a text input, the built-in color picker, a host-declared
  **value-list dropdown**, or a host **custom widget** — including **arg-groups**
  (one widget over several parameters, e.g. a 2D point picker) whose editor
  embeds inline. The Form holds live draft state, so an arg-group's inter-arg
  context resolves from the current form (pick a map, then a spot on that map)
  before anything is written.
- **Inline editing** of typed arguments in the editor: color swatches,
  value-list name labels, host-rendered chips, and arg-group chips — Edit a
  filled literal, Fill an empty slot, or open the Form (an opt-in inline glyph,
  the always-on hover-card action, the `Mod-Shift-A` keybind, or the Host
  Functions panel).
- A host **argument-widget API** (`StudioExtensions.argumentWidgets`): built-in
  and host-provided widgets, popover/modal editor surfaces, and arg-group
  widgets that receive resolved inter-arg context.
- The `argument_widgets` IDE query now reports per-slot value-list items and
  per-group inter-arg context indices across the wasm boundary, so the studio
  can render dropdowns and resolve context from live form state.
