# @brink-lang/editor

## 0.13.0

### Minor Changes

- abe28a2: Host-registered argument handlers for primitive base types (#990). A host can
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

### Patch Changes

- 909bb26: `dialect.ts`'s `ResolvedDialect.compile` no longer unconditionally constructs
  its per-kind `RegExp`s with the `d` (`hasIndices`) flag (#1013). That flag
  needs V8 9.0 / Chromium 90+ — NW.js-hosted embedders on older Chromium (e.g.
  RPG Maker MZ's bundled NW.js is Chromium 88, with no newer official runtime)
  threw `SyntaxError: Invalid flags supplied to RegExp constructor 'd'` at
  construction, black-screening the embedder at boot before a single line was
  ever classified.

  Support is now feature-detected once at module scope. Modern engines keep the
  `d`-flag path unchanged (indices read straight off the match). Older engines
  fall back to a capture-group walk that reconstructs the same per-named-group
  `[start, end)` spans by locating each group's captured text within its
  nearest enclosing named group's span — correctly handling nested groups (e.g.
  `parenthetical`'s `content` wrapping `content_inner`) — with no loss of
  `DialectMatch` fidelity (`kind`/`attrs`/`hiddenSpans`/`contentSpan` are
  byte-identical to the `d`-flag path on every input the at-cue conformance
  corpus and generalization suite cover).

- Updated dependencies [17ad933]
- Updated dependencies [f53c6c7]
- Updated dependencies [7e8aa7f]
- Updated dependencies [b9a86e2]
  - @brink-lang/web@0.13.0

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

## 0.9.3

### Patch Changes

- 96a4513: Rename-collision analysis (#722) no longer runs synchronously on the paint
  path. Root-caused from PR #714's RCA of the #696 e2e flake: the inline
  rename widget's breakage/collision query (`renameSymbolAt`, a wasm call)
  used to run inline in the debounce/Enter handler and could block paint for
  several seconds under load.

  The debounce settle (and an Enter with no cached result) now flips the "⚠
  breaks N" badge into a disabled, `aria-busy` **pending** state (`⋯`,
  `.brink-inline-rename-badge--pending`) synchronously — so a paint can land
  before the heavy call runs — then defers the actual query to the next idle
  slot (`requestIdleCallback`, falling back to a macrotask where unavailable)
  via a small `scheduleIdleWork`/`cancelIdleWork` helper. Apply/force stays
  disabled until the deferred query resolves; Enter never forces a synchronous
  call. `query`'s signature is unchanged (still plain and synchronous) — only
  its _scheduling_ moved, so existing synchronous callers/tests keep working
  unmodified. Same shared `InlineNameInput` also covers extract-to-knot/
  function (#315 H), so both prompts get the same off-paint-path behavior.

  No web-worker architecture (out of scope for #722) — a query that is itself
  slow still blocks once it starts; this mitigates by not starting it inside
  the same frame as the triggering keystroke.

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

## 0.9.2

### Patch Changes

- 601e999: HIR overlay now also refreshes when a view MOUNTS after a compile was
  already delivered (#518, follow-up to #494/#502). The 0.9.1 fix refreshed
  mounted views on `deliverCompile`, but for a slot without a view the refresh
  was dropped, not queued — so in the mount-after-initial-compile order (an
  external embedder's passive-load sequence: `ProjectSession.initialize()` →
  `triggerCompile()` → the framework commits the editor mount afterwards) the
  overlay showed whatever its `StateField` last held and nothing ever
  repainted it: a passive load never compiles again, and a remount reuses the
  cached `EditorState`, so the field's `create()` never re-runs and a value
  cached blank at unmount persisted until the first keystroke.

  `DocumentSessions.mountView` now self-serves the missed refresh: when a
  compile has already been delivered (`lastCompileDelivered`), it dispatches
  `refreshHirOverlayEffect` to the freshly mounted view — after the slot's
  wasm handle is (re)opened, so the projection read is live at mount time.
  The overlay's refresh trigger set is now
  {compile-deliver} ∪ {view-mount-after-a-deliver}, covering both mount
  orders and cached-state remounts.

  Also documents (hir-overlay.ts, editor-consumer-guide) that a host-dispatched
  `refreshHirOverlayEffect` is matched by object identity, so it must come from
  the same module instance of `@brink-lang/editor` that built the view's
  extensions — a bundler-duplicated copy produces an effect the field silently
  ignores, which can make host-side refresh workarounds appear to "read empty".

- 8333685: Fixed narrative-run folding in screenplay mode (#417). When a narrative run
  IS a choice's body (a character cue + dialogue directly under `* [Talk]`),
  the fold now anchors on the choice line itself and hides the whole body
  beneath it, instead of anchoring one line down on the cue. The collapsed
  pill no longer duplicates the anchor line's visible text ahead of the chip —
  the fold now hides the whole anchor line and the pill IS the line, matching
  the existing decl-fold placeholder shape. The pill's snippet also strips the
  dialect's cue sigils and shows the first CONTENT line (or the cue's bare
  name when the run has no content line), rather than raw text like
  `@Jackie:<>`.
- Updated dependencies [e2acdbb]
- Updated dependencies [6ed8a8d]
- Updated dependencies [eb06ccc]
- Updated dependencies [1154eb4]
- Updated dependencies [0f6ae50]
- Updated dependencies [e96d2a1]
- Updated dependencies [bd69ac6]
- Updated dependencies [f40c345]
- Updated dependencies [f25362a]
- Updated dependencies [ebce613]
- Updated dependencies [3c5808f]
- Updated dependencies [eaff136]
- Updated dependencies [ba69a35]
- Updated dependencies [124bb9e]
- Updated dependencies [75b8a3b]
- Updated dependencies [350b663]
- Updated dependencies [9e1257d]
- Updated dependencies [9213d77]
- Updated dependencies [f835cfd]
- Updated dependencies [b8392a2]
- Updated dependencies [6089ed6]
- Updated dependencies [1c389ec]
- Updated dependencies [81f0055]
- Updated dependencies [6e007d3]
- Updated dependencies [0308aec]
  - @brink-lang/web@0.10.1

## 0.9.1

### Patch Changes

- 66457c1: Fixed a silent dialect-classification drop in `computeLineInfos` (#426):
  when a mounted view's wasm document handle was present but not yet synced
  (or a host's `line_contexts_doc` returned `[]`, as some test/mock sessions
  do), every line was classified via the bare regex fallback and the TS
  dialect interpreter (`applyDialectFallback`) was never run — character cues,
  parentheticals, and chained dialogue lines silently rendered as plain
  narrative with no diagnostic. The same TS dialect fallback the no-handle
  path already ran is now also run over the regex-classified tail whenever the
  handle yields fewer line contexts than the document has lines, so dialect
  classification survives that path.
- 8f20d1e: HIR overlay now refreshes when the initial compile completes (#494). The
  overlay's projection StateField seeded at view creation — before the first
  async compile/analysis finished — and only recomputed on doc-changing
  transactions, so a passively loaded file rendered no `brink-hir-*` marks or
  rails until the first edit. `DocumentSessions` now dispatches a redecorate
  effect to every mounted view whenever a compile result is delivered, and the
  new `refreshHirOverlay(view)` / `refreshHirOverlayEffect` exports let hosts
  with custom wiring re-read the projection from their own compile-complete
  signal (mirrors `refreshGutterMarkers`).

## 0.9.0

### Minor Changes

- 73e2746: Line-classification fixes (#478) — deliberate behavior changes to the
  `line_contexts` contract and `LineInfo`:

  - A choice line with an inline divert (`* [Go] -> hub`) now classifies as
    `choice` (was `divert`), so Tab/Enter smart-editing transitions work on
    it again.
  - Every gather-label line — continuation labels, `LabeledBlock` labels,
    top-level labeled gathers — uniformly classifies as `gather` with
    `gather_continuation` weave at its sigil depth. Previously a labeled
    block with an inline divert showed `divert` while the visually identical
    continuation form showed `gather`.
  - Choices inside conditional/sequence branches report their sigil depth
    (was 0), so depth-dependent transitions and gutter depth markers work
    inside arms.
  - Blank lines inside a choice body inherit the body weave (element stays
    `blank`); the editor maps them to `ChoiceBody` so Tab works anywhere in
    the body — replacing the old single-shape TS post-pass, and covering
    deeper blank runs it missed.

- 36bf266: Machinery/narrative fold runs are now opt-in (#479). `foldingRanges` /
  `folding_ranges_doc` return structural folds only unless the host enables
  run computation via the new session-level `setFoldRunsEnabled(true)`
  (mirrors `setDialect`; also on `DocumentHandle`), and the editor's default
  active fold kinds are `structural` only — hosts implementing prose/logic
  view modes activate `machinery`/`narrative` with `setActiveFoldKinds` and
  collapse with `foldAllOfKind`. Runs are additionally bounded by weave
  containers (choice branches / gather continuations), so a run fold never
  crosses weave structure; conditional scaffold + arms still fold as one
  pure-routing region, and inline `{a|b}` alternatives don't fragment
  narrative runs.
- 4b5b3ab: HIR structural overlay in the editor (#454 phases 3–5): a queryable projection
  StateField renders `brink-hir-*` inline marks with `data-*` identity, per-line
  rail attributes plus a concentric rails gutter (knot/stitch/choice/gather/
  branch), and identity-keyed occurrence highlighting. New `getHirProjection`
  option on `BrinkStudioOptions`; `hirSpansAt`/`hirIdentityAt` query helpers.
  Default skin styles only unresolved refs, occurrences, and rails — hir marks
  stay visually inert for host theming.
- 1bca37c: LineInfo on one shared projection (#480). The HIR projection is now
  computed once per edit and cached on the session — `getLineContextsDoc`,
  `getFoldingRangesDoc`, and `getHirSpansDoc` all share it instead of each
  re-projecting. `LineContext` gains two additive fields the editor now
  consumes instead of deriving: `option_path` (option identity from real HIR
  nesting — the TS weave re-walk only serves the pre-wasm regex fallback)
  and `standalone` (structural divert-vs-tunnel/thread fact — no more text
  sniffing in the editor or fold-run natures). Span kinds `tunnel_stmt` and
  `thread_stmt` split out of `divert_stmt`, which now means a simple
  `-> target` statement only.
  Also fixes `has_tags`: it is now true for **any** line carrying an
  author-written tag — tagged choice lines (`* Choice # tag`), tags inside
  inline conditional/sequence branches, and standalone `#` lines — where the
  legacy walk under-reported (decision 2026-07-10; verified against the C#
  reference, whose runtime surfaces choice-line tags).

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

- 33f49a7: Fix #403: the narrative fold pill's cast summary now routes through `detectCast` (the #366/#399 public dialect extractor) instead of reading the `speaker` attr raw off `LineInfo.dialect.attrs`. A custom dialect whose chain carries a differently-named attr (e.g. `narrator` instead of `speaker`) now surfaces correctly in the pill; the default at-cue dialect's pill output is unchanged (still shows the carried `speaker` value).
- 81578e6: Fix #405: `preparePlaceholder` now breaks an exact-span tie between a structural fold and a machinery/narrative fold deliberately (structural wins), instead of relying on the accidental push order of `getFoldingRanges()`. No visible behavior change for the ordering hosts already ship (structural is pushed first in `folding_ranges_impl`), but the precedence is now pinned and covered by a test that constructs the tie with the ranges pushed in the opposite order.
- d9e6ad0: Fix #406: reconcile the at-cue preset's `parenthetical` shape's `content_group` (which spans the parens-inclusive `"(text)"`, needed so `content_span`/markup geometry keeps the parens as visible content) with a convert/strip round-trip's need for the bare text between the parens. Before this fix, a dialect declaring a `convert` transition row targeting `parenthetical` from a bare-content source kind rendered the target template (`"${content}<>"`) with the bare extracted text, dropping the opening paren entirely (`"radio<>"` instead of `"(radio)<>"`) — latent since the at-cue preset itself ships `transitions: []`, but it would have bitten the first dialect declaring such a row. Adds `PatternShape.template_group` (optional, additive — defaults to `content_group`, byte-identical for every dialect that doesn't set it): the group whose captured value fills `template`'s placeholder for convert/strip round-trips. No visible change for the default dialect's classification geometry or `data-*` attributes.
- d346261: Fix a false-positive in the conditional-scaffold classification pass added
  by #413: ordinary narrative containing inline logic that happens to
  start or end with a brace (a standalone inline conditional used as
  narrative content, e.g. `{visited: You were here before.}`, or narrative
  ending in a value interpolation, e.g. `You have {gold}`) was incorrectly
  swept into `Logic` classification (`brink-logic`) merely because the line
  started with `{` or ended with `}`. Only a conditional/sequence block's
  own genuine opening/closing brace (bare `{`/`}`, or `{` followed by a
  switch expression ending in `:`) is scaffold now — inline logic embedded
  in narrative keeps its narrative/dialogue classification.
- f35b20c: Fix a headless-contract leak (#414, follow-up to #363): the line-decoration
  pass stamped two inline `style` attributes regardless of `theme: false` —
  `padding-left` for weave-depth indent on choices/gathers, and
  `text-align: right` on standalone diverts — which beat host stylesheets and
  left headless embedders unable to restyle them.

  Both are now taxonomy instead:

  - Weave depth rides as `data-depth="N"` (choices/gathers at depth > 1).
  - Standalone diverts carry the `brink-divert-standalone` class.

  `brinkTheme` ships the previous look (indent scaled by depth, right-aligned
  standalone diverts) via CSS attribute/class selectors, so `brink-studio`
  renders unchanged. Headless hosts (`theme: false`) restyle
  `[data-depth="N"]` / `.brink-divert-standalone` directly — the
  line-decoration pass never emits a `style` attribute.

- d346261: Fix two screenplay-mode classification gaps (#413): a `~`-sigil logic line
  immediately after a chained dialogue line was swallowed into the
  cue→dialogue chain (rendered `brink-dialogue` instead of `brink-logic`),
  and lines in/around conditional blocks (`{`, `- cond:`, `- else:`, `}`, and
  cue/dialogue lines inside conditional arms) got no classification at all.

  Sigil classification now always wins over chain continuation. Conditional
  scaffold lines classify as logic; cue/dialogue lines written inside a
  conditional or sequence arm classify normally (Character/Parenthetical/
  Dialogue) and participate in the dialogue chain, matching top-level
  narrative. Choice-body narrative is unaffected — it still classifies but
  never chains, per the existing spec-mandated split.

  Emitted classes for lines that already classified correctly are
  byte-identical; only the previously-broken lines gain classes.

- Updated dependencies [5075db7]
- Updated dependencies [cbc27aa]
  - @brink-lang/web@0.9.0

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
