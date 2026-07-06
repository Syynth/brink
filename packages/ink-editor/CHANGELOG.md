# @brink-lang/editor

## 0.8.0

### Minor Changes

- 3cf1062: Fold kinds (#365): `FoldRange` now carries a `kind: "structural" | "machinery" | "narrative"`.

  - **`structural`** — everything the folding pass emitted before #365 (knot/stitch declarations, doc comments, conditionals, sequences, choice sets, the INCLUDE-block fold). User-invoked in every mode; never auto-collapsed.
  - **`machinery`** — a maximal run of `>= 2` consecutive machinery-natured lines (logic `~`, VAR/CONST/LIST decls, standalone diverts, conditional/sequence scaffold lines). Run-based over the per-line classification (base, or a registered dialect's declared `nature`, #368) — never HIR-block-based, so a narrative-bearing conditional's scaffold lines don't drag its prose branches into a machinery fold.
  - **`narrative`** — the symmetric run of `>= 2` consecutive narrative-natured lines (plain prose, or dialect kinds like `character`/`parenthetical`/`dialogue`).

  Editor-side (`@brink-lang/editor`):

  - `foldingExtension` takes a live-reconfigurable **active-kinds set** (default: all three); `setActiveFoldKinds(view, kinds)` reconfigures a mounted view.
  - New exported commands `foldAllOfKind(kind)` / `unfoldAllOfKind(kind)` — bulk fold/unfold every current range of one kind. Mode auto-collapse is always **host-invoked** (call these on your own mode-entry hook); the extension itself never forces a collapse.
  - Machinery/narrative folds render a JetBrains-style summary pill instead of the generic `…` placeholder: `brink-fold-pill` + `brink-fold-pill-machinery`/`brink-fold-pill-narrative` + `brink-fold-pill-icon`/`brink-fold-pill-summary`/`brink-fold-pill-count` child spans — class-addressable, zero inline styles. The machinery pill summarizes salient calls/assignments/divert targets (capped at 2, "+N more"); the narrative pill shows a first-line snippet, cast (via the registered dialect's carried `speaker` attribute — not a re-hardcoded `characterName()`), and line count.
  - The existing declaration fold placeholder (`.brink-fold-decl`) now carries `data-decl-kind="knot" | "stitch" | "function"` plus a `.brink-fold-decl-icon` slot span.

  `brink_ir::ElementNature` (narrative/machinery/structural) and `ResolvedDialect::nature_of` are new in the Rust dialect schema, consumed by `brink-ide::folding::machinery_and_narrative_folds` — never re-hardcoding a kind list in Rust or TS.

- 58d93ee: Compiler lines table + public `DialectParser` (#366): a host can now work out the cast (and similar per-line analyses) from the compiler's own line table instead of duplicating the `@Name:<>` convention.

  - **`@brink-lang/web`**: `StoryRunnerHandle.linesTable()` returns the compiled program's line table — one entry per scope (root/knot/stitch), project-wide (`INCLUDE`s already resolved by the compile), each line carrying its text (plain or a slot/select template) and, when known, its source span (`file` + byte range). Reuses the exact shape the `export-xliff` CLI path already produces (`brink_intl::export_lines`) rather than inventing a second representation. Static for the loaded program — no running `Story` required.
  - **`@brink-lang/editor`**: `DialectParser` (pure TS, no CM6/wasm dependency) — `parseSource(text)` classifies plain `.ink`-style source line-by-line against a `DialogueDialect` (mirrors `element-type.ts`'s classify + chain passes); `parseEmitted(text)` walks _runtime-emitted_ text (the post-glue output of `continue_line()`) into composite segments per the pinned iteration protocol: a cue + parenthetical + trailing text emitting as ONE line is the normal case, and a non-reserved-prefix shape (e.g. a parenthetical) never opens a composite line — it only peels as a continuation after a reserved-prefix (cue) segment.
  - **`detectCast(lines, dialect)`** ships as the #366 answer to cast detection: it walks `parseSource` output and collects the distinct values of whichever attr a dialect's chain rules `carry` forward (dialect-agnostic — not hardcoded to `speaker`). `characterName()` is NOT exported publicly (stays `screenplay.ts`-internal, per the dialect-spec ruling).

  First consumer: celeris cast detection feeding its speaker-color settings surface. The same lines-table exposure serves future analyses (per-speaker word counts, the #362 line-fit metrics epic).

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

- 047db46: Publish the tier-1 boundary helpers (#369): re-export `CompileResult`/`Diagnostic` from `@brink-lang/web` for module identity, and export the canonical `sortDiagnostics` (positional: file → offset → errors-first; presentation order is a host choice layered on top) and `lineColAt` (offset → 1-based line:col).
- f06d4ff: Generalize the last two at-cue-hardcoded sites so custom `DialogueDialect`s are functionally complete (#395, a follow-up from #368):

  - **Dialect `convert` and `strip` transition rows** now extract a line's content via the resolved dialect's OWN declared shapes (`ResolvedDialect.convertibleShapes()`), not the hardcoded `@name:<>`/`(text)<>` regexes. A custom dialect's non-at-cue wrapping kinds (e.g. `<<name>>`) now convert and strip correctly via a `transitions` row's `convert`/`strip` actions.
  - **`contentRegions`** (the inline-markup content-region scoping core) now accepts an optional third `geometry` argument (a line's cached `LineInfo.dialect`); when given, a Character/Parenthetical-shaped line's content bounds derive from `geometry.contentSpan` instead of the fixed at-cue affix-length constants, so a dialect overriding those kinds with different affix widths scopes markup correctly.
  - `extractLineContent` gains an optional `shapes: ConvertibleShape[]` parameter (tried before the built-in at-cue shapes); `ConvertibleShape` is now exported.

  Both changes are additive — omitting the new optional parameters reproduces the exact pre-#395 behavior for the default (at-cue) preset.

- ed2446b: Headless-ready editor (#363): the `brinkTheme` skin is now opt-out — pass `theme: false` to `brinkStudio(...)` (or `DocumentSessions`'s new options bag) for a fully headless editor, or pass your own CM `Extension` to substitute it; the default is unchanged and brink-studio now opts into `brinkTheme` explicitly. All presentational inline styles on editor-owned popups and widgets (code-actions menu, inline element picker, widget popover, argument-form anchor, inlay hints, color swatch + picker) moved onto classes; dynamic values (popup coordinates, swatch colors) ride on CSS custom properties consumed by a new always-on, zero-specificity (`:where(...)`) structural stylesheet (`ensureStructuralStyles`, exported), so hosts can restyle the full class taxonomy directly. The taxonomy — element line classes (an open `brink-<kind>` scheme), structural decoration classes, floating-surface classes, and custom properties — is now documented as a semi-stable contract in docs/editor-consumer-guide.md.
- bfd7e50: Host gutter-marker contribution API (#343): `BrinkStudioOptions.getGutterMarkers(source, fromLine, toLine) => HostGutterMarker[]` and `onGutterMarkerClick` render host-supplied per-line markers (breakpoints, annotations, run/flag icons) in a dedicated gutter slotted after the built-in play-from-here gutter. Purely additive — absent callback changes nothing. Deterministic ordering (by line, host array order within a line), per-marker + shared click dispatch, recompute on doc changes, and an exported `refreshGutterMarkers(view)` / `refreshGutterMarkersEffect` for external marker-set changes. Also exported standalone as `hostGutterExtension`.
- 4ad5d81: Extensible inline-markup rules (#367): `inlineMarkup(rules)` lets hosts register inline-markup shapes (single `pattern`, or `open`/`close` pair with an optional `contentClass`) that decorate as `brink-markup-<name>` marks with `data-*` attributes from named capture groups. Matching is content-region scoped — rules run only within the narrative content text of classified lines and never over ink syntax (glue `<>`, threads `<-`, divert arrows, choice brackets, sigil prefixes, hidden screenplay sigils). Ships zero rules by default; the RMMZ-style angle-tag rule is exported as the optional `rmmzAngleTagRule` preset. Styling is entirely host-side (classes only, per #363).
- fd69c0a: Emit option identity on choice/body lines (#364): every `Choice` line and its `ChoiceBody` lines now carry `data-option-path` (the full lineage of zero-based option indices through the weave, e.g. `"0.2.1"` — nested weaves first-class) and `data-option` (the convenience innermost index) as CM6 line attributes alongside the existing element class. Gather lines close their level's groups, so the next option at that depth starts a new group at index 0; knot/stitch headers reset the weave. Hosts can render per-branch rails (e.g. colored `border-left` on body lines) from these attributes without re-deriving the weave. Also exports the pure `assignOptionPaths` post-pass and adds `optionPath` to `LineInfo`.
- dfdc1e2: DocumentSessions: per-view cursor+scroll save/restore seam (#347). New `viewState(docKey, groupId?)` reads `{ anchor, head, scrollTop }` from the live view when mounted or from the cached slot (EditorState selection + unmount scroll snapshot) for backgrounded tabs, so hosts can persist every open tab, not just the focused one. New `restoreViewState(docKey, state)` re-applies a saved snapshot on the next mount (or immediately when mounted) via the pending-reveal mechanism — full selection + pixel scroll, no focus steal. Scroll now also survives in-session background/remount cycles.

### Patch Changes

- fabd5a5: Chromium 88 (RMMZ/NW.js) compatibility: remove every `color-mix()` from the editor and studio themes — Chromium 88 has no `color-mix()` (Chrome 111+), so those declarations were dropped wholesale, most visibly leaving text selection with no fill.

  - Behind-text highlight layers (`.cm-selectionBackground`) now use a solid `var(--bs-accent)` fill plus layer `opacity`, which composites identically and works on any host that defines the base tokens.
  - The active line uses a new optional theme token `--bs-active-line-bg`, falling back to the opaque `var(--bs-surface-bg)` for hosts that define only base tokens.
  - All other alpha-tinted highlights (search/selection matches, bracket matching, binder/search/graph chrome) are written as `rgb(var(--bs-X-rgb) / N%)` over new per-theme sRGB triplet tokens (`--bs-accent-rgb`, `--bs-error-rgb`, …) defined by the built-in Mocha/Latte themes.
  - Opaque two-color mixes (story-graph node borders/fills, conflict banner) are precomputed per theme as `--bs-graph-*` / `--bs-conflict-banner-bg` tokens.

  Visual output on modern Chromium is unchanged; hosts embedding `@brink-lang/editor` with a custom token set get correct selection/active-line out of the box and can define the new tokens for the tinted variants.

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

- 8be15da: Editor enhancement sweep (#311): inline rename with a live "⚠ breaks N" badge and inline breakage report; external-conflict 2-way merge view (with the dirty-buffer no-clobber data-loss fix); editor-owned editable search results buffer; INCLUDE-block fold; auto-import "from <file>" completion affordance; in-view find panel; code-actions apply with extract-to-knot/function. First published release of `@brink-lang/editor` (promoted from `@brink/ink-editor`). See `docs/editor-consumer-guide.md`.

### Patch Changes

- Updated dependencies [8be15da]
  - @brink-lang/web@0.7.0
