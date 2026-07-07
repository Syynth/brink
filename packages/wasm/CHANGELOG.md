# @brink-lang/web

## 0.9.0

### Minor Changes

- 5075db7: Add the speculative-evaluation web binding (F4.3, part of #439): a sandboxed,
  side-effect-proof fork of a running story that never mutates it, driven by
  its own composable verbs.

  `StoryRunnerHandle.speculate(options?)` forks a `SpeculationHandle` exposing
  `goToPath`/`advance`/`advanceAsync`/`choose`/`evalFunction`/
  `evalFunctionAsync`/`resumeFunctionEval`/`resumeFunctionEvalAsync`/
  `resolveExternal`/`takePendingPromise`/`pendingExternalName`/`transcript`/
  `externalsReport` — the composable primary surface. Externals are gated by a
  caller-supplied `name -> "query" | "effect"` policy map plus a `"watch" |
"eval"` context (mirrors `brink_runtime::KindTieredHandler`): query externals
  always run live; effect externals only run live under `context: "eval"` with
  `liveEffects: true` armed, and otherwise fall back to the ink fallback body.
  An async (`Promise`-returning) bound external is awaited transparently by the
  `*Async` verbs, exactly like `StoryRunnerHandle.continueStoryAsync`.

  `StoryRunnerHandle.evaluate(source, opts)` is a thin convenience over those
  verbs for the common cases: a knot/stitch path (`"cellar.intro"`) is driven to
  its next natural stop (a `done`/`end` line, or a `choices` line reported via
  `reachedChoices` rather than picked); a function call with literal arguments
  (`"check(1, 2)"`) is evaluated via `evalFunction`. Anything else (an arbitrary
  expression, a non-literal argument) reports a diagnostic rather than running —
  that's the Tier-1/F5 boundary (`docs/speculative-eval-spec.md`). `opts.signal`
  (an `AbortSignal`) cancels an in-flight evaluation, dropping the speculation
  and rejecting with an `AbortError`.

  Function-evaluation results marshal through a new richer `TypedValue`
  (`int`/`float`/`bool`/`string`/`null`/`list`/`divert`) instead of the
  scalar-only `ExternalValue` the external-binding boundary uses — a `list`
  carries its resolved member names/ordinals and a `divert` its resolved
  knot/stitch destination, rather than collapsing to `null`.

  Also renamed `docs/scratch-eval-spec.md` to `docs/speculative-eval-spec.md`
  and threaded the speculative/`Speculation`/`speculate` naming through it and
  its cross-reference in `docs/scoped-flow-state-spec.md` — it is now framed as
  that plan's Tier-1 (arbitrary-expression) follow-on to the Tier-0 fork-based
  `Speculation` this release ships.

  The oracle corpus is unaffected — this is purely additive to the runtime and
  web binding.

- cbc27aa: Add Tier-1 fragment support to `StoryRunnerHandle.evaluate()` (F5.1, part of
  #440): an arbitrary author-typed expression (`"has(sword) && gold > 2"`,
  `"gold"`), content (`"You have {gold}"`), or lone divert (`"-> cellar"`) — not
  just a bare knot path or a literal-arg call (Tier 0) — now evaluates instead
  of coming back as a dead-end diagnostic.

  Mechanism: the fragment is wrapped as a synthetic knot/function
  (`=== function __eval_<hash>() ===\n~ return (...)` for an expression,
  `=== __eval_<hash> ===\n...` for content — classified by trying the
  expression wrap first and falling back to content), recompiled against the
  project's full sources via a new `brink-web` entrypoint,
  `compile_fragment(entry, sources, syntheticSource)` (multi-file/`INCLUDE`-
  aware, unlike the single-file `compile()`), then run through the already-
  shipped F4 `Speculation` machinery: a fresh `StoryRunnerHandle` over the
  recompiled program, seeded from the live runner's current state
  (`load(liveRunner.save())`, name-keyed — globals by name, visit/turn counts
  by content-hashed id, both stable across the recompile), `speculate()`, then
  `evalFunction`/`goToPath` exactly as the Tier-0 path already does. The
  speculation and its scratch runner are discarded when done; nothing touches
  the live runner. `evaluate()`'s return shape (`SpeculationResult`) is
  unchanged — Tier-1 is invisible to the caller beyond accepting more `source`.

  Since a `StoryRunner` holds no reference to the file set it was compiled
  from, `evaluate()` gains an `opts.projectSource: { entry, files }` option —
  required only for a Tier-1 `source`, supplied by the consumer (the editor,
  which has the project's live sources). Without it, or when a fragment fails
  to compile as either an expression or content, `diagnostics` comes back
  non-empty and nothing runs (no crash).

  The scratch runner starts with no external bindings of its own, so
  `evaluate()` copies the live runner's registered bindings and
  lenient-unbound policy onto it first (`StoryRunner.binding_names`/
  `get_binding`/`lenient_unbound`, new) — a query/effect external the fragment
  touches resolves the same way it would on the live runner, matching Tier-0's
  guarantee (Tier-0 gets this for free by forking the same runner).

  Compiled fragments are cached per `StoryRunnerHandle`, keyed by
  `(program checksum, fragment source)`: a fragment compiles once per program
  version, then every re-evaluation (e.g. a watch panel re-running on every
  step) is a cache hit. The cache is bounded (200 entries, FIFO eviction) so a
  long session of one-off watches can't grow it without bound. A new
  `StoryRunnerHandle.checksum()` (mirroring `StoryRunner::checksum` /
  `programChecksum`, but read off the already-linked program so it survives
  `reload`) keys the cache to the running program's identity.

  The oracle corpus is unaffected — this is purely additive to the compiler's
  web binding and the web/TS speculative-eval wrapper; the runtime's own
  drive/episode path is untouched.

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

- f72f181: Expose the Rust `StorySession` journal/replay layer (#370, PR #385) on `@brink-lang/web` as `StorySessionHandle` (#387): `advance`/`continueSingle`/`continueToPause`, `choose`/`resolveExternal`, turn-boundary `setVar`/`goToPath`/`saveState`/`loadState`, journaled `callFunction`, `snapshot`/`diff` (+ standalone `diffSnapshots`), `exportJournal`/`StorySessionHandle.restore`, `reload`/`continueReplay`, and `restart`. Fixes the wire-format lie where `awaiting_external` was smuggled into the `Line` union: `advance()` now returns a distinct `StepOutcome` (`{ type: "line", line } | { type: "awaiting_external", deferred, name? }`), keeping the two park states (promise-in-flight vs. deferred out-of-band) explicit. New TS types (`StepOutcome`, `SessionJournal`, `StateSnapshot`, `StateDiff`, `ReplayOutcome`, etc.) ship from `@brink/wasm-types` and are re-exported here.
- 9d1dd69: Add the spec-mandated deferred+debounced journal-append persistence hook to `StorySessionHandle` (#390, `docs/story-session-spec.md`) that #387/#389 dropped: `onJournalDirty(listener)` registers a callback that fires **after** the call stack that grew the journal has fully unwound (never synchronously inside `advance`/`choose`/`resolveExternal`/`setVar`/`goToPath`/`loadState`/`callFunction`/`reload`/`continueReplay`, and never re-entrantly while another `StorySessionHandle` method is on the stack), coalescing bursts of calls into a single notification. The signal is intentionally minimal — `{ eventCount: number }` (new `JournalDirtySignal` type from `@brink/wasm-types`) — hosts pull the actual journal via the existing `exportJournal()`. `onJournalDirty` returns an unsubscribe function; `restart()` resets the dirty baseline so a fresh journal isn't reported dirty. `crates/brink-web` gains one additive `WebSession.journal_event_count()` accessor as the cheap dirty-signal source.
- 1f91422: Story-graph edges now carry source spans (#371): each `StoryGraphEdge` lists
  its `occurrences` — the divert sites that produced it, as UTF-16 spans
  (`{file, start, end}`), one entry per site on aggregated edges. Path targets
  anchor on the target path's span; `-> DONE`/`-> END` on the divert statement.
  New `StoryGraphEdgeOccurrence` type exported; the field is optional and
  omitted only for synthesized diverts with no source anchor.
- a11b115: Studio migration onto the public `StorySession` (#388, deliverable 3 of docs/story-session-spec.md):

  - **`StorySessionHandle` gains the Program Explorer / State View / shared-flow surface** that was only on `StoryRunnerHandle` before: `debugSnapshot()` (live position — globals, call stack, visit counts, pending choices, RNG), `programModel()` / `programInkt()` (static, compile-bound), and the shared-flow quartet `spawnFlow` / `continueFlow` / `chooseFlow` / `destroyFlow` / `flowNames` / `flowDebugSnapshot`. A flow spawned this way shares the _session's own_ globals/visits/rng (true ink concurrent-flow semantics) — the same VM instance the session drives, not a second one. This was a real gap in the shipped #389/#393 bindings (flagged in the design round's critique of the studio-migration proposals): without it, `@brink/studio-store`'s `LocalSessionProvider` couldn't migrate onto the session without regressing shared flows (#200) or the State View.
  - `crates/brink-web`'s `WebSession` now retains the decoded `StoryData` (mirroring `StoryRunner`) so `program_model`/`program_inkt` can be derived without a second decode, and delegates `debug_snapshot`/the flow quartet through the documented `StorySession::story()`/`story_mut()` escape hatch (journal-bypass, by design — flow stepping was never meant to journal).
  - `debugSnapshot().pending_choices[].index` now carries the choice's raw, pre-filter `pending_choices` position (the same index `choose()` expects), instead of leaving consumers to infer it from array position — which is wrong whenever an invisible-default choice sits at the same pause point, since invisible-default choices are filtered out of `pending_choices` but still occupy a slot in the runtime's underlying list.

  `@brink/studio-store`'s `LocalSessionProvider` (private, not published) now drives `StorySessionHandle` instead of `StoryRunnerHandle`: choice/continue/restart flow through the session, replay-on-recompile flows through `reload()`'s typed `ReplayOutcome` (`replayed` / `diverged` / `failed`), and persistence is push-based via `onJournalDirty` (no polling, no bespoke save timing). The pre-migration `{choiceLog}` localStorage blob gets a one-time migration to the journal format the first time a fresh session starts (replayed against the new session exactly like the old silent re-walk, but building a real journal along the way) rather than a hard reset — divergence still truncates + parks + notifies exactly as before.

## 0.7.0

### Minor Changes

- 8be15da: Unified all structural-op results into a single **breaking** `StructuralResult` (replaces `MoveResult`/`SymbolRenameResult`) with an op-wide safe-by-default breakage gate. Added `deleteSymbol`, atomic `rename_dir`, `extract_to_knot`/`extract_to_function`, document-agnostic `findReferencesAt`/`referencesToSymbol`, `resolve_code_action`, and auto-import ops. BREAKING: consumers of `MoveResult`/`SymbolRenameResult` migrate to `StructuralResult` (the `safe`/`introduced_diagnostics`/`cross_file_edits` fields are preserved).

## 0.6.0

### Minor Changes

- b0746e7: Knot/stitch **Rename** — a full, cross-file, safe-by-default rename on the shared symbol context menu (editor / Binder / Story Graph) and the editor's **F2**. A clean rename applies immediately; one that would introduce diagnostics flips to an in-place breakage report whose only override is an explicit **Force rename** (mirroring the `brink ide rename` CLI's `--unsafe` gate). An open symbol-view tab survives its own rename (re-keyed in place).

  F2 is now a full cross-file rename — the previous single-file F2 was a bug. `@brink-lang/web` gains `rename_symbol` / `rename_symbol_at` and drops the superseded `rename_doc` / `rename` exports (and the corresponding `doRename` handle methods).

## 0.5.1

### Patch Changes

- 080a715: Fix: ordinary words that happen to match ink keywords (e.g. "and", "or", "not") are no longer highlighted as code when they appear in prose. Keyword highlighting is now limited to expression/logic contexts, so narrative text renders as plain text. (#275)

## 0.5.0

### Minor Changes

- a6bceef: Binder file lifecycle — manage whole files and folders directly in the binder.

  - **Delete** files and folders from the context menu, with undo.
  - **Rename** files and folders inline (F2 or the context menu). Every `INCLUDE` that points at a renamed or moved file is rewritten automatically, and `..`-relative include paths now resolve correctly across the toolchain.
  - **Move** files by dragging onto a folder, drag a file back out to the project root, and multi-select to move several files at once — all undoable, with one "Moved N files" step.
  - Renaming a file keeps its open editor tab in place (pin, split, and selection are preserved) instead of reopening it.

  `@brink-lang/web` gains the `rename_file` session op, which computes the edit set for a file move: the re-keyed file content plus the referrer `INCLUDE` rewrites.

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
