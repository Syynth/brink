# @brink-lang/studio

## 0.12.0

### Patch Changes

- Updated dependencies [6cb663a]
  - @brink-lang/web@0.12.0

## 0.11.1

### Patch Changes

- Updated dependencies [c246a4a]
- Updated dependencies [ae66340]
- Updated dependencies [7baa01f]
- Updated dependencies [aa43bb6]
- Updated dependencies [edf92bc]
- Updated dependencies [d350551]
- Updated dependencies [3c1e1e1]
- Updated dependencies [c03a73a]
- Updated dependencies [83717d3]
- Updated dependencies [302c6a2]
- Updated dependencies [4a08940]
- Updated dependencies [b86fee8]
- Updated dependencies [1e1be68]
- Updated dependencies [c36b8c4]
- Updated dependencies [71dd2fc]
- Updated dependencies [213a7f5]
- Updated dependencies [730c947]
- Updated dependencies [a0d9ee2]
- Updated dependencies [7ac0a5d]
- Updated dependencies [1198586]
- Updated dependencies [058f410]
- Updated dependencies [7500e27]
- Updated dependencies [bcb5cd3]
- Updated dependencies [c62687c]
- Updated dependencies [8870113]
- Updated dependencies [e16e8f8]
- Updated dependencies [820f6c5]
- Updated dependencies [45eb96b]
- Updated dependencies [e8cb050]
- Updated dependencies
- Updated dependencies [fe0c16d]
- Updated dependencies [6266cbf]
- Updated dependencies [9e9f07a]
- Updated dependencies [878be79]
- Updated dependencies [c66409b]
- Updated dependencies [86c4bee]
- Updated dependencies [fdf94f6]
- Updated dependencies [9d559a3]
- Updated dependencies [cc1d11e]
- Updated dependencies [62cb759]
- Updated dependencies [a350dcf]
- Updated dependencies [3ad1bc5]
- Updated dependencies [2b7dd5a]
  - @brink-lang/web@0.11.1

## 0.8.3

### Patch Changes

- Updated dependencies [8a3635d]
- Updated dependencies [34951ec]
- Updated dependencies [81ddfa7]
- Updated dependencies [9c58d6e]
- Updated dependencies [f68c094]
- Updated dependencies [b9ad39f]
- Updated dependencies [b7b7eb0]
- Updated dependencies [d29671d]
- Updated dependencies [ca45425]
- Updated dependencies [abc369a]
- Updated dependencies [30e09f9]
- Updated dependencies [2541c08]
- Updated dependencies [5b07740]
- Updated dependencies [d02c4e2]
- Updated dependencies [20d2bfa]
- Updated dependencies [d38fa08]
- Updated dependencies [9bef954]
- Updated dependencies [1e71455]
- Updated dependencies [c9475df]
  - @brink-lang/web@0.11.0

## 0.8.2

### Patch Changes

- Updated dependencies [73e2746]
- Updated dependencies [36bf266]
- Updated dependencies [973858f]
- Updated dependencies [54c37df]
- Updated dependencies [1bca37c]
- Updated dependencies [6289b0e]
  - @brink-lang/web@0.10.0

## 0.8.1

### Patch Changes

- Updated dependencies [5075db7]
- Updated dependencies [cbc27aa]
  - @brink-lang/web@0.9.0

## 0.8.0

### Minor Changes

- 6785663: Dialogue dialect editor integration (#368): the screenplay behavior (`@Name:<>` character cues, `(beat)<>` parentheticals, the dialogue chain) is now driven by a `DialogueDialect` — a versioned, pure-JSON schema — instead of hardcoded regexes.

  - **`brinkStudio({ dialect })`** (default `AT_CUE_DIALECT`, byte-identical to the old hardcoded behavior). `dialect: null` tears down the screenplay layer — classification, decorations, dialect transition rows, dialect keybinding behaviors — for true headless composition (pair with `theme: false`, #363); the structural weave keymap (Choice/Gather/Narrative Tab/Enter transitions) stays active, per the spec's structural-rows-stay-interpreter-owned rule. A custom `DialogueDialect` object drives classification/decorations/transitions/conversions with zero editor code changes.
  - **`@brink-lang/web`**: `EditorSessionHandle` gains `setDialect(dialect)` / `clearDialect()` (wrapping the wasm `set_dialect`/`clear_dialect` seam from #386), and the `DialogueDialect` schema types + the `LineContext.dialect` facet are published from the type surface.
  - **`setDialect(view, dialect)`** live-reconfigures an already-mounted editor: swaps the screenplay compartment, forces reclassification, and re-runs the wasm `set_dialect`/`clear_dialect` when a document handle is present.
  - **`extendDialect(base, overrides)`** adds a kind (or overrides chain/transitions/templates) without forking a preset.
  - Classification is authoritative in Rust (`brink_ir::dialect` + `line_contexts_with_dialect`, landed in #386) when a wasm document handle is present. Without one, the editor falls back to a thin TS interpreter over the identical JSON (`ResolvedDialect`), pinned against the same conformance corpus (`tests/dialect_fixtures/at_cue.json`) as the Rust side so both paths agree on every case.
  - Screenplay geometry (`screenplay.ts`'s hidden decorations, atomic ranges, edit guard, cursor clamps) is now derived from the resolved dialect's hidden-group match indices — computed once at classification time and cached, never re-matched in per-keystroke hot paths. The `CHAR_SUFFIX_LEN`/`GLUE_LEN` constants and the public `characterName()` export are gone; the geometry is dialect-derived and internal.
  - The Tab/Enter/Shift-Tab transition table and name-surgery keybindings now consult a dialect's declared overlay rows before the built-in structural weave table (inert for the default preset, which ships no overlay rows).

  ### BREAKING CHANGE: `ElementType` enum → open string union (0.x hard cut, ruled 2026-07-05)

  `ElementType` used to be a numeric TS `enum`. It is now a `const` object of kebab-case kind strings mirroring the existing `brink-<kind>` CSS class scheme — `ElementType.Character`-style call sites migrate mechanically (the values still compare correctly), but the type is now `string`, and two PascalCase leaks are now kebab-case:

  - `@brink-lang/studio`'s published `StudioApi`: `StudioPublicState.element.type` was `"KnotHeader"`, `"NarrativeText"`, `"Choice"`, … — now `"knot-header"`, `"narrative"`, `"choice"`, ….
  - `@brink/studio-store`'s duplicate `ElementType` enum is deleted; it now re-exports the real one from `@brink-lang/editor` (still available as `ElementTypeEnum`).

  Full PascalCase→kebab mapping table in `docs/editor-consumer-guide.md`. No compat shim — both packages are pre-1.0.

### Patch Changes

- fabd5a5: Chromium 88 (RMMZ/NW.js) compatibility: remove every `color-mix()` from the editor and studio themes — Chromium 88 has no `color-mix()` (Chrome 111+), so those declarations were dropped wholesale, most visibly leaving text selection with no fill.

  - Behind-text highlight layers (`.cm-selectionBackground`) now use a solid `var(--bs-accent)` fill plus layer `opacity`, which composites identically and works on any host that defines the base tokens.
  - The active line uses a new optional theme token `--bs-active-line-bg`, falling back to the opaque `var(--bs-surface-bg)` for hosts that define only base tokens.
  - All other alpha-tinted highlights (search/selection matches, bracket matching, binder/search/graph chrome) are written as `rgb(var(--bs-X-rgb) / N%)` over new per-theme sRGB triplet tokens (`--bs-accent-rgb`, `--bs-error-rgb`, …) defined by the built-in Mocha/Latte themes.
  - Opaque two-color mixes (story-graph node borders/fills, conflict banner) are precomputed per theme as `--bs-graph-*` / `--bs-conflict-banner-bg` tokens.

  Visual output on modern Chromium is unchanged; hosts embedding `@brink-lang/editor` with a custom token set get correct selection/active-line out of the box and can define the new tokens for the tinted variants.

- ed2446b: Headless-ready editor (#363): the `brinkTheme` skin is now opt-out — pass `theme: false` to `brinkStudio(...)` (or `DocumentSessions`'s new options bag) for a fully headless editor, or pass your own CM `Extension` to substitute it; the default is unchanged and brink-studio now opts into `brinkTheme` explicitly. All presentational inline styles on editor-owned popups and widgets (code-actions menu, inline element picker, widget popover, argument-form anchor, inlay hints, color swatch + picker) moved onto classes; dynamic values (popup coordinates, swatch colors) ride on CSS custom properties consumed by a new always-on, zero-specificity (`:where(...)`) structural stylesheet (`ensureStructuralStyles`, exported), so hosts can restyle the full class taxonomy directly. The taxonomy — element line classes (an open `brink-<kind>` scheme), structural decoration classes, floating-surface classes, and custom properties — is now documented as a semi-stable contract in docs/editor-consumer-guide.md.
- Updated dependencies [3cf1062]
- Updated dependencies [58d93ee]
- Updated dependencies [6785663]
- Updated dependencies [f72f181]
- Updated dependencies [9d1dd69]
- Updated dependencies [1f91422]
- Updated dependencies [a11b115]
  - @brink-lang/web@0.8.0

## 0.7.0

### Minor Changes

- 8be15da: Wire the #311 editor features into the studio: inline rename, external-conflict merge view, editable search buffer, code-actions menu (with extract-to-knot/function), auto-import, and the INCLUDE-block fold.

### Patch Changes

- Updated dependencies [8be15da]
  - @brink-lang/web@0.7.0

## 0.6.0

### Minor Changes

- b0746e7: Knot/stitch **Rename** — a full, cross-file, safe-by-default rename on the shared symbol context menu (editor / Binder / Story Graph) and the editor's **F2**. A clean rename applies immediately; one that would introduce diagnostics flips to an in-place breakage report whose only override is an explicit **Force rename** (mirroring the `brink ide rename` CLI's `--unsafe` gate). An open symbol-view tab survives its own rename (re-keyed in place).

  F2 is now a full cross-file rename — the previous single-file F2 was a bug. `@brink-lang/web` gains `rename_symbol` / `rename_symbol_at` and drops the superseded `rename_doc` / `rename` exports (and the corresponding `doRename` handle methods).

### Patch Changes

- Updated dependencies [b0746e7]
  - @brink-lang/web@0.6.0

## 0.5.1

### Patch Changes

- 080a715: Fix: screenplay indents (character / parenthetical / dialogue) no longer collapse to flush-left on browser engines without CSS container-query support (older Chromium-based embeds such as NW.js / CEF). The layout now degrades to viewport-relative scaling there, and keeps pane-relative scaling on engines that support container queries. (#188)
- Updated dependencies [080a715]
  - @brink-lang/web@0.5.1

## 0.5.0

### Minor Changes

- a6bceef: Binder file lifecycle — manage whole files and folders directly in the binder.

  - **Delete** files and folders from the context menu, with undo.
  - **Rename** files and folders inline (F2 or the context menu). Every `INCLUDE` that points at a renamed or moved file is rewritten automatically, and `..`-relative include paths now resolve correctly across the toolchain.
  - **Move** files by dragging onto a folder, drag a file back out to the project root, and multi-select to move several files at once — all undoable, with one "Moved N files" step.
  - Renaming a file keeps its open editor tab in place (pin, split, and selection are preserved) instead of reopening it.

  `@brink-lang/web` gains the `rename_file` session op, which computes the edit set for a file move: the re-keyed file content plus the referrer `INCLUDE` rewrites.

### Patch Changes

- Updated dependencies [a6bceef]
  - @brink-lang/web@0.5.0

## 0.4.4

### Patch Changes

- 5431d8e: Clickable value-list picker. A value-list argument (a semantic type with a
  declared `values` list) now renders an interactive chip instead of a passive
  label: click it to open a filterable dropdown of the items and rewrite the
  literal in place. Hosts get a click-to-pick combobox for free from a declared
  value-list — no custom `ArgumentWidget` required (#224).

## 0.4.3

### Patch Changes

- 9ce2764: Fix host-widget Edit on non-string arguments. A host `ArgumentWidget` on a
  non-string semantic type (e.g. an `int`) opened and called `host.resolve(...)`
  but never wrote back when replacing an existing literal — the in-place edit
  resolved the literal range with a quote-only finder, so a bare literal like `1`
  was a silent no-op. Bare int/float/bool literals are now handled, so host
  widgets can edit already-filled arguments of any type (#242).

## 0.4.2

### Patch Changes

- 05325c0: Argument-widget + editor polish.

  - **Bundle the editor font** — the studio now self-hosts JetBrains Mono
    (Latin, regular/bold/italic), so embedders without it installed (e.g. RPG
    Maker MZ / NW.js) no longer fall back to the system monospace.
  - **Typed widgets in the Host Functions panel** — composing a fresh call from
    the panel now uses the same value-list dropdowns, host widgets, and
    arg-group controls as the in-editor call Form, not plain text fields.
  - **Host-sourced value-lists in the Form** — a slot whose semantic type
    declares `values: host` now surfaces its dropdown items from the pushed host
    cache, not just static manifest items.

- Updated dependencies [05325c0]
  - @brink-lang/web@0.4.2

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

- Updated dependencies [facc579]
  - @brink-lang/web@0.4.1

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

### Patch Changes

- Updated dependencies [755868c]
  - @brink-lang/web@0.4.0

## 0.3.0

### Minor Changes

- bcd23b7: Live inspector and host-aware authoring.

  - The story session is driven by a `SessionProvider`, so the transcript, State
    View, and Story Graph render against a provider rather than the wasm runner
    directly — the groundwork for inspecting a VM running in a host.
  - Capability-gated session commands, program-identity degraded mode, and
    multi-session support (independent runners + shared-context flows) with a
    session/flow picker.
  - A host-aware argument picker: a value dropdown and inline value labels for
    `EXTERNAL` arguments whose semantic type declares a value source (static, or
    pushed live by a host), plus a `StudioExtensions.argumentProviders` surface
    for embedders to supply those values.

### Patch Changes

- Updated dependencies [bcd23b7]
  - @brink-lang/web@0.3.0

## 0.2.1

### Patch Changes

- Updated dependencies [20764ef]
  - @brink-lang/web@0.2.0

## 0.2.0

### Minor Changes

- 6276f29: File-content egress for embedding hosts (#154, closing #137): a debounced,
  batched `onFilesChanged(changes: FileChange[])` mount option fed by every
  mutation path (editor edits, binder structural ops, search replace,
  `file.new`), an `api.getFiles()` / `api.getDirtyFiles()` pull surface,
  `file.save` (Mod-S) / `file.saveAll` commands that flush and deliver
  immediately, and a `dirtyFiles` count on `StudioPublicState` (additive —
  `version` stays 1). Also: a `wasmLocation` mount option forwarded to
  `initWasm` for IIFE-plugin hosts, and a Chromium-88 `adoptedStyleSheets`
  feature-detect shim in the mount bootstrap (NW.js / RPG Maker MZ).
