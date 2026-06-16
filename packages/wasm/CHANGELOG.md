# @brink-lang/web

## 0.4.1

### Patch Changes

- facc579: Argument-widget fixes.

  - **Embedded host content theming/positioning** — widget popovers (the color
    picker, host pickers, the call Form) now mount inside the `.brink-studio` root
    and use `position: fixed`, so embedded host content inherits the theme tokens
    and positions correctly when the studio is embedded in a host page (rather than
    rendering unstyled or mis-placed against `document.body`).
  - **Auto-open on completion-accept** — the completion kind map was keyed by the
    wrong casing, so every completion was typed `"text"`. This both mis-iconed
    completions and disabled "open the Form when accepting a function completion".
  - **The call Form is driven by the signature metadata**, not the live call-site,
    so a partial or over-full call still renders its declared widgets (e.g. an
    arg-group picker) instead of degrading to plain text fields; Apply writes a
    well-formed call.

## 0.4.0

### Minor Changes

- 755868c: Argument widgets — rich, type-driven call-site authoring.

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

## 0.3.0

### Minor Changes

- bcd23b7: Add program-identity, flow-control, and host-value APIs.

  - `programChecksum(bytes)` — the source-identity checksum of compiled `.inkb`
    bytes (matches `ProgramModel.checksum`) without constructing a runner.
  - Shared-context flows on `StoryRunnerHandle`: `spawnFlow`, `continueFlow`,
    `chooseFlow`, `destroyFlow`, `flowNames`, `flowDebugSnapshot` — concurrent
    flows of one story that share globals / visit counts / rng.
  - `EditorSessionHandle.setHostValues` / `clearHostValues` — push host-provided
    values for `host`-source semantic types into the editor's value cache (the
    author-time argument picker).

## 0.2.0

### Minor Changes

- 20764ef: Add `StoryRunnerHandle.goToPath(path)` — ink's `ChoosePathString` equivalent. Moves the play head to a named knot or stitch (`"knot"` / `"knot.stitch"`); subsequent `continue*` calls run from there. The session keeps its state: variables and visit/turn counts survive, and the jump itself counts as a visit to the target, exactly like a `-> path` divert. Pending choices are abandoned (callstack reset); the transcript so far is kept. Throws on an unknown path (naming it), and refuses to jump while the story is parked on an unresolved async external — resolve it (or `reset()`) first.
