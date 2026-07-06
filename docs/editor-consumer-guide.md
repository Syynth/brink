# Building a tool on `@brink-lang/editor`

For hosts that embed the ink editor **without** the `brink-studio` app. Everything of
substance from the #311 editor sweep lives in `@brink-lang/editor` (the CM6 editor,
published from `packages/ink-editor`) and `@brink-lang/web` (the wasm handle). `studio`
is just *one* consumer of these — your tool is on the same footing. This guide maps the
reusable seams, the per-feature host contract, and the thin studio wrappers to copy from.

## The two entry points

1. **`brinkStudio(options: BrinkStudioOptions): Extension`** (`extensions.ts`) — the editor
   itself. A CodeMirror 6 extension factory. You pass callbacks that bridge to `@brink-lang/web`;
   the editor supplies all the UI (completions, hover, inline rename, code-actions, folding, …).
   Features light up **only when you provide their callback** — omit `getCodeActions` and there's
   no code-actions menu, etc.
2. **`ProjectSession`** (`project-session.ts`) — the reusable session/file/dirty/conflict layer.
   Owns the wasm `EditorSessionHandle`, tracks dirty state, applies cross-file edits, and runs the
   external-conflict detection. This is the object your tool builds on; `studio` uses the same class.

You wire a `FileProvider` (implement the interface, or use the shipped `InMemoryFileProvider`) into
`ProjectSession`, then mount `brinkStudio(...)` per document.

## The host contract — `BrinkStudioOptions`

Provide only what you want. Each optional callback is the *bridge* to a `@brink-lang/web` op;
the editor owns the resulting UX.

| Option | Feature it enables | Backing `@brink-lang/web` call |
|---|---|---|
| `compile`, `getSemanticTokens`, `getTokenTypeNames` | required base | `compile`, `semantic_tokens`, … |
| `getCompletions` + `autoImport` | completions + **auto-import** (#312): out-of-scope completions tagged `from <file>`, accept inserts the `INCLUDE` | `completions_*`, `auto_import_include_doc` / `auto_import_apply_include_doc` → `AutoImportResult` |
| `getFoldingRanges` | folding incl. the **INCLUDE-block fold** (#313, `INCLUDE … (N files)`) and **fold kinds** (#365: structural/machinery/narrative run-based folds + JetBrains-style summary pills) | `folding_ranges` → `FoldRange[]` |
| `getCodeActions` | **code-actions** menu (#321); apply via the resolve op | `code_actions_*` + `resolve_code_action` |
| `prepareRename` + `renameSymbolAt` + `commitRename` (+ `onRenameBreakage`) | **inline rename** (#323) with the live **"⚠ breaks N"** badge (#324) and the inline breakage report | `prepare_rename` + `rename_symbol_at` → `StructuralResult` |
| `getHover`, `gotoDefinition`, `findReferences`, `getSignatureHelp`, `getInlayHints`, `getArgumentWidgets` | the corresponding LSP-style features | `hover`, `goto_definition`, `find_references*`, … |
| `onPlayFrom`, `onSymbolContextMenu`, `onNavigateToFile` | host hooks for your own chrome | — |
| `theme` | the editor skin (#363): absent ⇒ the default `brinkTheme`; `false` ⇒ **headless** (you style the class taxonomy below); an `Extension` ⇒ your own CM theme | — |
| `getGutterMarkers` (+ `onGutterMarkerClick`) | **host gutter markers** (#343): your own per-line gutter affordances (breakpoints, annotations, run/flag icons) | — (host-supplied data) |
| `dialect` | the **dialogue dialect** (#368): absent ⇒ the `AT_CUE_DIALECT` preset (`@Name:<>` cues); `null` ⇒ tears down the whole screenplay layer; a `DialogueDialect` object ⇒ your own convention | `set_dialect` / `clear_dialect` (tooling-only; never affects the runtime) |

Notes on the host gutter contract (#343):
- `getGutterMarkers(source, fromLine, toLine) => HostGutterMarker[]` returns markers
  (`{ line, className?, text?, title?, onClick? }`, `line` 1-based) for the inclusive line range.
  It is currently queried for the whole document; the range parameters let the contract narrow to
  the viewport later without an API change.
- Markers render in a **dedicated gutter slotted after (to the right of) the built-in
  play-from-here ▶ gutter**, so your affordances coordinate with the editor's own instead of
  needing a raw CM6 `gutter()`. Ordering is deterministic: by line, your array order within a line.
- Clicks fire the marker's own `onClick(line)` first, then the shared
  `onGutterMarkerClick(marker, line)`.
- The set recomputes on document changes. When it changes for *external* reasons (a breakpoint
  toggled in another panel), call the exported `refreshGutterMarkers(view)` (or dispatch
  `refreshGutterMarkersEffect`) to re-query.
- Composing extensions directly instead of using `brinkStudio`? The same feature is exported
  standalone as `hostGutterExtension(options)`.

Notes on the rename contract (the one with the most moving parts):
- `renameSymbolAt(offset, newName) => StructuralResult` is called **debounced on each keystroke** to
  drive the badge count (`result.introduced_diagnostics.length`, `result.safe`).
- `onRenameBreakage(result, ctx) => boolean` lets you **override how the breakage report is presented**
  (return whether to proceed). The editor's *default* is the inline report; studio uses this hook to
  route non-editor (binder/graph) renames to its own modal. Your tool decides its own rendering here.

## Per-feature: what you get vs. what you rebuild

| Feature | Reusable export (`@brink-lang/editor`) | You provide | Studio wrapper to reference (thin) |
|---|---|---|---|
| Find panel (#319) | `findPanel(options?)` extension | just add it to your editor's extensions | — (studio doesn't use it) |
| Inline markup (#367) | `inlineMarkup(rules)` extension + `rmmzAngleTagRule` preset; matches decorate as `brink-markup-<name>` + `data-*`, scoped to narrative content (never over ink syntax) | your `InlineMarkupRule[]` + host CSS for the classes | — (zero rules by default) |
| Fold INCLUDE block (#313) + fold kinds (#365) | `foldingExtension` + `getFoldingRanges` callback; `foldAllOfKind`/`unfoldAllOfKind`/`setActiveFoldKinds` | the wasm folding call; your mode-entry auto-collapse invocation | — |
| Auto-import (#312) | completion tag + accept-side insert (wired via `brinkStudio`) | `getCompletions` + `autoImport` callbacks | — |
| Code-actions apply (#321) | menu + apply dispatch (via `getCodeActions`) | `getCodeActions` + the resolve op | — (not enabled in studio) |
| Inline rename + breaks-N (#323/#324) | the whole in-editor UX in `rename.ts` (input, badge, inline report) | rename callbacks above | binder/graph menus + modal `SymbolRenamePrompt` (only if you have non-editor rename entry points) |
| Merge view (#320) | `ConflictView` class + `ConflictViewOptions`; detection via `ProjectSession`/`FileChangeHub` | mount `ConflictView`; call the resolve methods | `studio-ui/ConflictMergeView.tsx` (~77 lines: React mount + store) |
| Editable search buffer (#322) | `SearchResultsBuffer` class; pure model `buildResultsRows` / `mapRowEditToSource`; `searchSources` engine | mount the buffer; feed it search results | `studio-ui/SearchResultsBufferView.tsx` (~85 lines) |
| Option identity / branch rails (#364) | `data-option-path` (full weave lineage, e.g. `"0.2.1"`) + `data-option` (innermost index) line attributes on choice/body lines, emitted automatically | nothing — style them from CSS (e.g. per-branch `border-left` rails colored from the path) | — (headless: brink emits identity, the host styles it) |

## The external-conflict flow (#320) — the one with real session API

Data-loss prevention lives in `ProjectSession` + `FileChangeHub`, not in any UI:
- Pass `onFileConflict: (conflict: FileConflict) => void` to `ProjectSession`. It fires when an
  external change arrives for a **dirty** buffer whose content differs — the editor buffer is
  **kept** (no clobber), and you're handed `{ path, disk, buffer, baseline }`.
- Query state: `dirtyPaths()`, `conflictedPaths()`, `hasConflict(path)`.
- Resolve: `resolveConflictUseDisk(path, disk)`, `resolveConflictKeepMine(path)`,
  `resolveConflictMerged(path, merged)`.
- Render: mount `ConflictView` (the self-contained 2-way `@codemirror/merge` banner + side-by-side
  surface) into a container, wiring its buttons to the three resolve methods.

## `ProjectSession` — the reusable surface (selected)

`addFile` · `closeFile` · `deleteFile` · `renameFile` · `getFiles` · `requestFile` ·
`applyEdit(path, newSource)` (the canonical cross-file edit seam — everything routes through it) ·
`compileProject` · `dirtyPaths` / `markAllSaved` / `save` · the conflict methods above ·
`refreshIncludes` · `initialize` / `destroy`.

## `DocumentSessions` view-state persistence (#347)

Per-tab cursor + scroll save/restore across app reloads:

- `viewState(docKey, groupId?) → { anchor, head, scrollTop } | null` — snapshot seam. Works for
  **every open tab**: mounted views read live; backgrounded tabs read their cached `EditorState`
  (selection) plus the scroll snapshotted at unmount. `groupId` addresses one `(docKey, groupId)`
  slot exactly (split views); without it, prefers focused → mounted → cached.
- `restoreViewState(docKey, snapshot, groupId?)` — applied on next mount (or immediately when
  mounted), no focus steal, offsets clamped so stale/corrupted snapshots degrade instead of
  throwing. Pass `groupId` to restore each pane of a split view independently.
- `revealAt(docKey, offset)` remains the "jump to a diagnostic" primitive (focus + center scroll).

Persistence loop: on quit, `viewState` every open tab into your session blob; on boot, reopen the
tab set, then `restoreViewState` each entry.

## `@brink-lang/web` structural ops (all return `StructuralResult`)

After #316 every mutating structural op returns one shape —
`StructuralResult { ok, path?, new_source?, cross_file_edits, safe, introduced_diagnostics }` — so you
gate them all the same way (show the breakage report when `!safe`, apply `cross_file_edits` on proceed):
`rename_symbol` / `rename_symbol_at` · `move_stitch` · `promote_stitch` · `demote_stitch` · `reorder_*`
· `delete_symbol` (#316) · `rename_dir` (#314) · `extract_to_knot` / `extract_to_function` (#315) ·
`resolve_code_action` (#321) · `find_references_at` / `references_to_symbol` (#317, document-agnostic).

## Boundary helpers (#369)

Small pure helpers every host needs at the editor ↔ host seam, published from
`@brink-lang/editor` so you don't carry shim copies:

- **`CompileResult` / `Diagnostic`** — re-exported from `@brink-lang/web`, so importing them
  through the editor gives the *same module identity* as importing `@brink-lang/web` directly.
  No structural `CompileResultLike` shims.
- **`sortDiagnostics(diagnostics)`** — the canonical **positional** sort: file path, then start
  offset, then errors before warnings (end offset and message as deterministic tiebreakers).
  Non-mutating. Note: presentation ORDER is a **host choice** layered on this canonical
  positional sort — re-group the sorted list however your UI wants (severity-first, per-file
  sections, …); the helper is the shared baseline, not a rendering policy.
- **`lineColAt(text, offset)`** — offset → 1-based `{ line, col }`, clamped to the text.
  UTF-16 offsets, matching `Diagnostic.start`/`end` and `editor.reveal` source spans.

(`docKeyFor` / `parseDocKey` are also published — see `document-sessions.ts`.)

## Story Session — runtime session management (#370 #387 #389)

Beyond the editor, `@brink-lang/web` exposes **`StorySessionHandle`** — the TypeScript class
consumers import. (It wraps the raw wasm-bindgen `WebSession` class, which also appears in the
package's exports; use the handle — it adds the journal-dirty notification and the ergonomic
`restore` shape.) The handle is a stateful runtime session that wraps
the story VM with journaling, replay, and snapshot/diff semantics. This is the surface for
**persistence**, **rewind/replay**, and
**save-game mechanics**. The runtime journal is the durable save artifact; the rest of the API
manages stepping, turn-boundary mutations, and divergence detection.

### The stepping + StepOutcome split

At the stepping layer, `StorySessionHandle` distinguishes **two park states**:

- **`StepOutcome` — deferred external** (`{ type: "awaiting_external", deferred: true, name? }`):
  Host must call `resolveExternal(value)` to resume. This is the out-of-band park from `advance()`.
  The session records the external's result in the journal; the next step proceeds normally.
- **Promise-in-flight** (internal pause, never surfaced): The session awaits an async operation
  internally. Use `continueSingle()` / `continueToPause()` for inline externals — they resolve
  through the fallback body or a JS-bound handler without surfacing a park.

Methods:

- **`advance(): StepOutcome`** — one step (may park on deferred external). The returned
  `StepOutcome` is a tagged union: `{ type: "line", line }` carries a `SessionLine` (narratives,
  choices, end markers); `{ type: "awaiting_external", ... }` awaits resolution.
- **`continueSingle(): SessionLine`** — advance until output or a yield point. **Never** surfaces a
  deferred external; externals resolve inline or error. Returns a single `SessionLine`.
- **`continueToPause(): SessionLine[]`** — run to the next pause (choices / done / end). Returns an
  array of `SessionLine`; the last is always a terminal variant (`choices`, `done`, or `end`).
- **`choose(index: number): void`** — select a choice by index (journaled). Must be called when
  parked on `choices`. No validation — bad index errors on next step.
- **`resolveExternal(value: unknown): void`** — resolve the parked external. No-op if not awaiting
  (safe to call spuriously). The value is recorded in the journal; `advance()` next step.
- **`hasPendingExternal(): boolean`** — check if parked on a deferred external without stepping.

The **constructor option `deferred: string[]`** forces named externals to always park as
`awaiting_external`, even when the story defines a fallback body — useful for host-critical
externals (sound, dialogs, save prompts).

### Turn-boundary mutations + journaled inputs

These methods **queue until the next pause** (or error mid-turn):

- **`setVar(name: string, value: unknown): boolean`** — set a global. Returns `false` if no such
  global is declared (no journal entry written). Turn-boundary only.
- **`goToPath(path: string, args?: unknown[]): void`** — jump to a knot/stitch. Turn-boundary only.
- **`saveState(): SaveState`** — capture game state (globals, turn/visit counts, RNG). Does not
  journal. Return value is opaque; use it with `loadState` or `exportJournal` for persistence.
- **`loadState(saveState: SaveState): void`** — restore from a captured state (turn-boundary,
  journaled). Fails if the save is incompatible (different story); tolerant of global schema drift.
- **`callFunction(name: string, args?: unknown[]): unknown`** — evaluate an ink function from the
  host (journaled as a `Call` event). The function's externals **do not journal** — they resolve
  through an isolated handler, keeping the visible story untouched.

Attempting these mid-turn (while the story is outputting or paused on choices) errors. Drain the
turn first: `continueToPause()` until the story is at a pause (done / choices / end), then apply
the mutation, then step again.

### Journal + replay: the persistence loop

The journal is the **durable save artifact** — a JSON record of every input: choice selections,
mutations (`set_var`, `go_to_path`), externals and their results, `load_state` records, and
function calls. Checksum + seed embedded. Optionally includes a fast-restore checkpoint (the latest
`SaveState`).

**To save:** call `exportJournal(): SessionJournal` (JSON). Persist the result in a save slot.

**To load:** call `StorySessionHandle.restore(storyBytes, journal, seed?, deferred?)`, which
returns **`{ session, outcome }`** — the restored session plus the `ReplayOutcome` from that
restore/replay. The session fast-restores from the embedded checkpoint if the program checksum
matches; otherwise it replays the journal against the new program.

The `ReplayOutcome` shapes:

- **`{ type: "replayed", warnings: ReplayWarning[] }`** — Journal played to completion. Warnings
  are soft (e.g. choice labels drifted but indices are stable).
- **`{ type: "diverged", at_event: number, expected, found }`** — The program changed in an
  incompatible way. Journal truncated at the divergence; the session is parked at the reached
  position. `found` describes what replay hit instead of the recorded event (unknown path,
  out-of-range choice index, etc.).
- **`{ type: "failed", at_event: number, reason }`** — Runtime error, budget exhausted, or the
  replay parked on a deferred external (`reason: { type: "awaiting_external", name }`). Session
  parked at the reached position.

**The journal cap**: The journal is capped at a fixed limit, `SESSION_JOURNAL_CAP` (65,536 events).
Beyond that, appends are dropped. Hitting the cap sets `journal.truncated = true` and degrades to
fast-restore-only (no replay after recompile).

### Snapshot + diff: typed state observation

State snapshots are **first-class**, diffable artifacts (distinct from the legacy string-valued
`DebugState`):

- **`snapshot(): StateSnapshot`** — capture the current game state (globals, list membership,
  turn/visit counts, callstack summary). Returns JSON; opaque unless inspecting in dev.
- **`diff(a: StateSnapshot, b: StateSnapshot): StateDiff`** — pure diff of two snapshots. Shows
  added/removed/changed globals, list membership deltas, frame push/pop, turn-index changes.
  Standalone export: `diffSnapshots(aJson, bJson)`.

Snapshots are **typed** (globals carry their ink type tags, e.g. `{ Int: 10 }`, `{ String: "x" }`).
Diffs are deterministically ordered.

### Deferred externals: the park protocol

When the story hits an external with a fallback body and the external is in the `deferred` list
(constructor option), or when `advance()` surfaces an unresolved external:

1. Host receives `StepOutcome.awaiting_external` (with optional external name).
2. Host calls `resolveExternal(value)` to supply the result.
3. Host calls `advance()` again; the session resumes from the park with the resolved value.
4. The result is recorded in the journal (`JournalEvent.external`).

**Limitation — live-mode replay only**: When replaying a journal against a recompiled program
(`reload()`), if the replay hits a deferred external, it parks (`AwaitingExternal` failure reason)
and must be resumed via `continueReplay()` after resolving. **Recorded-mode replay** (the default
— journal events served back) never parks on externals; they're replayed from the journal.

### Escape hatch: journal bypass

The wrapped `FlowInstance` is reachable for advanced use cases (e.g., shared flows that never
journal):

- **`FlowInstance` methods are not exposed** on the session; the journal layer is observation-only.
- **Shared flows** (#200) keep working normally — their externals never journal, per the story
  spec.
- The session never steps shared-flow branches; they're transparent to the journal.

### Persistence loop pattern (with the journal-dirty hook)

The typical save-game flow with the session:

```typescript
// Auto-persist: the push signal (shipped in #390 / PR #393). Fires deferred +
// debounced — after the call stack that grew the journal has fully unwound,
// coalescing bursts (rapid choose/advance) into one call. Returns unsubscribe.
const unsubscribe = session.onJournalDirty(() => {
  const journal = session.exportJournal();
  // persist to durable storage (IndexedDB, API, save slot)
});

// On app restart / load game:
const journal = /* fetch from storage */;
const { session: restored, outcome } = StorySessionHandle.restore(storyBytes, journal);
if (outcome.type === "replayed") {
  // Good — resume from the journal.
} else if (outcome.type === "diverged" || outcome.type === "failed") {
  // Handle divergence: offer the user the reached position or a fresh start.
}
```

`onJournalDirty` is the **push signal** — it never fires synchronously inside a session method.
`exportJournal()` remains the **pull signal** — call it on demand (user save, periodic autosave,
app quit).

## What is genuinely studio-only (you rebuild in your framework)

- **React mount wrappers** for the class-based UIs (`ConflictView`, `SearchResultsBuffer`). Thin —
  the studio versions are 77–85 lines; copy the shape, swap the framework.
- **State management**: studio uses Zustand slices (`conflict`, `search`, `symbol-menu`). You hold
  that state your own way.
- **App entry points**: binder/graph context menus, the modal rename prompt for non-editor surfaces.
  You have your own tool surface; the editor's in-canvas features don't need these.

Bottom line: you inherit every feature's substance from the editor + wasm packages and reimplement
only thin mount glue + your own state/entry-points.

## The dialogue dialect (#368) — cue/parenthetical/dialogue conventions

The editor's "screenplay" behavior — recognizing `@Name:<>` character cues, `(beat)<>`
parentheticals, and the dialogue chain that follows a cue — is not hardcoded. It is driven by a
**dialect**: a versioned, pure-JSON schema (`DialogueDialect`, exported from `@brink-lang/editor`)
describing your project's dialogue-line conventions. See `docs/dialect-spec.md` for the full schema.

- **Default**: omit the option (or pass nothing) and you get `AT_CUE_DIALECT` — byte-identical to
  the pre-#368 hardcoded `@Name:<>` behavior. Nothing changes for existing hosts.
- **Headless**: `brinkStudio({ dialect: null })` tears down the *entire* screenplay layer —
  classification, decorations (hidden sigils, atomic ranges, the edit guard), dialect transition
  rows, and the dialect-specific keybinding behaviors — for true headless composition (pair with
  `theme: false`, #363). Structural classification (Choice/Gather/Divert/…) AND the structural
  keymap (Choice/Gather/ChoiceBody/Narrative Tab/Enter transitions, Home/End, arrows, the
  Alt-Enter picker) keep working — structural rows are interpreter-owned per the dialect spec;
  only the dialect-specific layer is gone.
- **Custom**: pass your own `DialogueDialect` object. `extendDialect(AT_CUE_DIALECT, overrides)`
  adds a kind (or overrides transitions/templates) without forking the preset.
- **Live reconfigure**: `setDialect(view, dialect)` swaps an already-mounted editor's dialect —
  decorations, the wasm-side classification (when a document handle is present), and a
  forced reclassification, all in one call. Pass `null` to tear down the layer live.

When a wasm document handle is present, the Rust compiler (`brink_ir::dialect` +
`line_contexts_with_dialect`) is authoritative for dialect classification — `set_dialect`/
`clear_dialect` on `EditorSessionHandle` register it. Without a handle (e.g. a bare CodeMirror
state in a test), the editor falls back to a **thin TS interpreter** (`ResolvedDialect` in
`@brink-lang/editor`), implementing the same specification as the Rust side. Both are pinned
against the same conformance corpus (`tests/dialect_fixtures/at_cue.json` for the default dialect)
so both paths produce identical results for that corpus; custom dialects are validated once and
used by both compiler and editor without re-implementation.

The dialect is an **authoring-time/tooling artifact only** — it is never embedded in compiled
`.inkb` output and never instructs the runtime. A game wanting the same cue-parsing logic at
runtime imports `DialectParser` (below) as an ordinary library and passes it the same JSON — that's
your wiring, not something the editor or compiler does for you.

### `DialectParser` + `detectCast` (#366) — a pure-TS parser, no editor required

`DialectParser` (from `@brink-lang/editor`) is a standalone class over a `DialogueDialect` — no
CodeMirror, no wasm session. Construct once per dialect (patterns compile once), then:

- **`parseSource(text)`** classifies plain `.ink`-style source text line-by-line using the dialect's
  patterns. Returns one `SourceLine` per input line: `{ index, text, kind, attrs }`, where `kind`
  is `null` for a line that didn't classify. A blank line always breaks the chain. This is a
  source-side parse — it never interprets ink's own structural syntax (`->`, `<-`, `#`, `{}`);
  a source line that happens to look like a divert/thread/tag/logic line is just narrative text
  to the dialect layer. **Both the Rust compiler and the TS editor use the same `ResolvedDialect`
  interpreter for dialect classification when a handle is present; the contract is pinned against
  the conformance corpus** (`tests/dialect_fixtures/at_cue.json` for the default dialect). Custom
  dialects are always verified against the Rust compiler's line table — that is the authoritative
  source for compiled output.
- **`parseEmitted(text)`** walks *runtime-emitted* text (the post-glue output of
  `continue_line()`) into `EmittedSegment[]` per the pinned **composite-segment iteration
  protocol**: a cue + parenthetical + trailing text emitting as ONE line is the normal case (three
  segments: `character`, `parenthetical`, then a plain-text remainder). A non-reserved-prefix
  shape (e.g. a parenthetical) never opens a composite line — it only peels as a *continuation*
  immediately after a reserved-prefix (cue) segment, never from arbitrary prose.
- **`detectCast(lines, dialect)`** is the #366 answer to cast detection: given `parseSource`
  output, it collects the distinct values of whichever attr a dialect's `chain` rules `carry`
  forward (in the at-cue preset, `speaker`), in first-appearance order. Dialect-agnostic — it does
  not hardcode `speaker` or the `character` kind name. `characterName()` (the old internal
  content-region helper in `screenplay.ts`) is **not** exported publicly; `detectCast` is the
  replacement.

Pair `detectCast` with `StoryRunnerHandle.linesTable()` (`@brink-lang/web`, below) for a
runtime/compiled-output view of the same cast, or with `parseSource` over live editor buffers for
an authoring-time view.

## The compiled lines table (#366) — `StoryRunnerHandle.linesTable()`

`StoryRunnerHandle.linesTable()` (`@brink-lang/web`) returns the compiler's own line table: one
entry per compiled scope (root, knot, or stitch), **project-wide** — `INCLUDE`s are already
resolved by the compile, so a multi-file project's lines all appear in one table. Each line
carries:

- `content` — the line's text: a plain string, or `{ template: [...] }` for a line with
  interpolation slots/plural selects (literal parts, `{ slot }` references, and `{ select }`
  branches — the same shape `export-xliff` produces for translators).
- `hash` — a stable source-identity hash for the line's content.
- `source` — `{ file, range_start, range_end }` when known: which source file the line came from
  and its byte range in that file.
- `slots` / `audio` — interpolation-slot names and an optional audio reference, when present.

This is static for the loaded program — it does not require a running `Story`, just a compiled
`StoryRunnerHandle`. It's the same `LinesJson` shape the `export-xliff` CLI path already produces
(`brink_intl::export_lines`), exposed to web hosts instead of reinvented. First consumer: cast
detection (pair with `detectCast` above) feeding a speaker-color settings surface; the same
exposure serves per-speaker word counts and the #362 line-fit metrics epic — any host-side analysis
that needs to walk emitted lines project-wide.

## Fold kinds (#365) — structural/machinery/narrative

`FoldRange` (from `getFoldingRanges`/`folding_ranges`) carries a `kind: "structural" | "machinery" |
"narrative"`:

- **`structural`** — everything the pre-#365 folding pass emitted (knot/stitch declarations, doc
  comments, conditionals, sequences, choice sets, the INCLUDE-block fold). User-invoked in every
  mode; **never auto-collapsed**.
- **`machinery`** — a maximal run of `>= 2` consecutive machinery-natured lines (logic `~`, VAR/
  CONST/LIST decls, standalone diverts, conditional/sequence scaffold lines). Run-based over the
  per-line classification (base, or a registered dialect's declared `nature`) — a conditional whose
  branch bodies are narrative prose does not become a machinery fold just because it's one
  construct; the narrative lines break the run.
- **`narrative`** — the symmetric run of `>= 2` consecutive narrative-natured lines (plain prose,
  or dialect kinds like `character`/`parenthetical`/`dialogue`).

### Active-kinds set + bulk fold/unfold

`foldingExtension` takes a live-reconfigurable **active-kinds set** — which kinds the fold service
will actually fold at all. `setActiveFoldKinds(view, kinds)` reconfigures it on an already-mounted
view (mirrors `setDialect`'s own compartment pattern); it defaults to all three kinds active.
Removing a kind from the set doesn't force-unfold anything already folded — it only stops the fold
service from offering *new* folds of that kind.

`foldAllOfKind(kind)` / `unfoldAllOfKind(kind)` are exported `Command`s (`(view) => boolean`, CM6
convention) that bulk-fold/unfold every current range of one kind. **Mode auto-collapse is always
host-invoked** — call `foldAllOfKind("machinery")(view)` on your own mode-entry hook; the extension
itself never forces a collapse. Typical modes:

- Narrative-lens view: active kinds = `structural` + `machinery`, then `foldAllOfKind("machinery")`
  on entry — collapse the logic, read the prose.
- Logic-focused view: active kinds = `structural` + `narrative`, then `foldAllOfKind("narrative")`.
- Hybrid view: active kinds = `structural` only — nothing auto-collapses.

### Summary pills — JetBrains-style fold placeholders

Machinery/narrative folds render a placeholder pill instead of the generic `…` — the JetBrains
principle of showing the pertinent content, not just a count. DOM: `brink-fold-pill` + a kind class
(`brink-fold-pill-machinery` / `brink-fold-pill-narrative`) + `brink-fold-pill-icon` /
`brink-fold-pill-summary` / `brink-fold-pill-count` child spans — class-addressable, **zero inline
styles** (you style all of it).

- **Machinery pill**: an effects summary — salient external calls / assignments / divert targets
  from the run, capped at 2 items with a "+N more" remainder (e.g. `⚙ change_party_member(…) ⇒
  leave · +1 more`).
- **Narrative pill**: a scene summary — the run's first-line snippet, its cast (via the registered
  dialect's carried `speaker` attribute on `LineInfo.dialect.attrs` — **not** a re-hardcoded
  `characterName()`, which stays package-internal), and the line count (e.g. `❞ "Hello there,
  friend." — Alice · 2 lines`).
- **Decl pill** (the existing `=== name === …` declaration fold): now carries
  `data-decl-kind="knot" | "stitch" | "function"` on `.brink-fold-decl` plus a
  `.brink-fold-decl-icon` slot span, so hosts render the same glyphs their binder rows use.

## Styling a headless editor — theme opt-out + the class taxonomy (#363)

The editor is **headless-ready**: every element it renders carries a stable class, and the skin is
optional. There are three styling layers:

1. **The structural stylesheet** — always on. A tiny stylesheet (`<style
   id="brink-editor-structural-styles">`) injected once per document, on demand, by the surfaces
   that need it (also exported as `ensureStructuralStyles(doc?)` for iframe mounts). It carries only
   *load-bearing* rules — popup `position: fixed` + coordinate plumbing, data-driven widget colors —
   and every selector is wrapped in `:where(...)` (zero specificity), so **any** host rule overrides
   it without specificity games.
2. **`brinkTheme`** — the opt-in skin. The default for `brinkStudio(...)`; references the studio's
   semantic `--bs-*` tokens. Pass `theme: false` to omit it entirely (headless), or pass your own
   `Extension` to substitute a CM theme. `DocumentSessions` forwards the same option
   (`new DocumentSessions(project, callbacks, extraExtensions, { theme: ... })`); brink-studio opts
   into `brinkTheme` explicitly.
3. **Your host CSS** — styles the taxonomy below directly. This is the intended integration for
   embedders (celeris screenplay lens, etc.): no CSS-variable contract, just classes.

### Line element classes — an open scheme

Every non-blank `.cm-line` gets an element class (applied by the screenplay decorations, so it is
present with zero configuration). The scheme is **open and string-keyed**: classes follow the
`brink-<kind>` naming scheme, and hosts/dialects may introduce additional kinds — a new kind
appearing is *not* a breaking change. Style what you know; let unknown kinds fall through to your
defaults.

Core kinds (stable):

| Class | Line kind |
|---|---|
| `brink-knot-header` / `brink-stitch-header` | `=== knot ===` / `= stitch` headers |
| `brink-narrative` | plain prose |
| `brink-choice` / `brink-choice-body` | `*`/`+` choice line / its indented body |
| `brink-gather` | `-` gather |
| `brink-divert` | `-> target` |
| `brink-logic` / `brink-var-decl` | `~ ...` / `VAR`/`CONST`/`LIST` |
| `brink-comment` | `//` and `/* ... */` |
| `brink-include` / `brink-external` | `INCLUDE ...` / `EXTERNAL ...` |
| `brink-tag` | `# tag` lines |
| `brink-character` / `brink-parenthetical` / `brink-dialogue` | screenplay elements (`@Name:<>`, `(beat)<>`, dialogue prose) |

Additive line classes: `brink-section-start` (the first line of a knot's comment+header block).

Two pieces of *data-driven placement* still ride on the line as inline styles (they are computed
per-line from weave depth / divert shape, not skin): `padding-left` on choices/gathers at depth > 1,
and `text-align: right` on standalone diverts.

### `ElementType` is now a string (BREAKING CHANGE, 0.8.0, #368, ruled 2026-07-05)

`ElementType` used to be a numeric TS `enum`. It is now a `const` object of the same kebab-case
kind strings the CSS classes above already used — `ElementType.Character` still works as a
call-site value (mechanical migration), but the *type* is now `string` (`LineInfo.type: string`),
matching the open scheme: a registered dialect's declared kinds flow through as plain strings that
just aren't named on the `ElementType` object.

This surfaced two places where the wire values were the PascalCase **enum member names**, not the
kebab-case kind — both are now the kebab-case kind, same as the CSS class taxonomy:

| PascalCase (old) | kebab-case (new) | CSS class |
|---|---|---|
| `KnotHeader` | `knot-header` | `brink-knot-header` |
| `StitchHeader` | `stitch-header` | `brink-stitch-header` |
| `NarrativeText` | `narrative` | `brink-narrative` |
| `Choice` | `choice` | `brink-choice` |
| `ChoiceBody` | `choice-body` | `brink-choice-body` |
| `Gather` | `gather` | `brink-gather` |
| `Divert` | `divert` | `brink-divert` |
| `Logic` | `logic` | `brink-logic` |
| `VarDecl` | `var-decl` | `brink-var-decl` |
| `Comment` | `comment` | `brink-comment` |
| `Include` | `include` | `brink-include` |
| `External` | `external` | `brink-external` |
| `Tag` | `tag` | `brink-tag` |
| `Blank` | `blank` | `brink-blank` |
| `Character` | `character` | `brink-character` |
| `Parenthetical` | `parenthetical` | `brink-parenthetical` |
| `Dialogue` | `dialogue` | `brink-dialogue` |

Affected surfaces:
- **`@brink-lang/studio`'s published `StudioApi`** — `StudioPublicState.element.type` (read via
  `studioApi.select((s) => s.element)`) used to be the PascalCase name (e.g. `"KnotHeader"`); it is
  now the kebab-case kind (e.g. `"knot-header"`).
- **`@brink/studio-store`** — the duplicate `ElementType` enum that used to live in `studio-store`
  is deleted; it now imports the real one from `@brink-lang/editor` (re-exported as
  `ElementTypeEnum` for call-site compatibility).

No compat shim — this is a 0.x hard cut (both packages are pre-1.0). New/custom kinds (from a
registered dialect) are additive, not breaking, per the open-scheme contract above.

### Structural decoration classes

In-line decorations applied inside `.cm-line` content: `brink-hidden-sigil` (concealed syntax
sigils), `brink-depth-sigil` (the depth widget replacing nested choice/gather sigils),
`brink-choice-bracket` (the `[...]` choice-suffix bracket), `brink-fold-decl` /
`brink-fold-decl-header` / `brink-fold-decl-icon` (fold affordances; `brink-fold-decl` also carries
`data-decl-kind`, #365), `brink-fold-include` / `brink-fold-include-label` (the INCLUDE-block fold,
#313), and `brink-fold-pill` / `brink-fold-pill-machinery` / `brink-fold-pill-narrative` /
`brink-fold-pill-icon` / `brink-fold-pill-summary` / `brink-fold-pill-count` (the machinery/
narrative summary-pill fold placeholders, #365).

### Floating surfaces + widget classes

All editor-owned popups/widgets are class-addressed; none carry presentational inline styles.
Dynamic *values* (popup coordinates, swatch colors) ride on CSS custom properties consumed by the
structural stylesheet — your CSS can re-consume or ignore them:

| Surface | Classes | Custom properties |
|---|---|---|
| Code-actions menu | `brink-code-actions-menu`, `brink-code-action-item` | `--brink-popup-left/top` |
| Inline element picker (Alt-Enter) | `brink-element-dropdown brink-inline-picker`, `brink-element-dropdown-item`, `brink-element-dropdown-key` | `--brink-popup-left/top` |
| Widget popover / modal | `brink-widget-popover`, `brink-widget-modal`, `brink-widget-modal-backdrop` | `--brink-popup-left/top` |
| Argument form | `brink-arg-form`, `brink-arg-form-{row,label,input,select,picker,edit,editor,host,title,buttons,btn,btn-primary}`, `brink-form-anchor` (invisible cursor-anchor scaffolding) | `--brink-popup-left/top`, `--brink-anchor-height` |
| Inlay hints | `brink-inlay-hint`, `brink-inlay-hint-pad` (trailing gap requested by the hint) | — |
| Color swatch + picker | `brink-color-swatch`; `brink-cp`, `brink-cp-{sv,sv-thumb,hue,row,hex,presets,preset}` | `--brink-swatch-color`; `--brink-cp-{hue,x,y,color}` |
| Value-list chip + picker (#224) | `brink-value-chip`, `brink-value-{picker,filter,list,item,item-label,item-detail}` | — |
| Fill ghost / host chip | `brink-fill-ghost`, `brink-host-chip` | — |
| Inline rename (#323/#324) | `brink-inline-rename`, `brink-inline-rename-{row,input,badge,force,cancel,report,report-head,report-list,report-item,report-loc,report-msg,report-actions}` | — |
| Hover / signature help | `brink-hover-tooltip`, `brink-hover-action`; `brink-signature-help`, `brink-sig-{label,active-param,doc}` | — |
| References / play gutter | `brink-reference-highlight`; `brink-play-gutter`, `brink-play-gutter-icon` | — |
| Conflict merge view (#320) | `brink-conflict-{banner,message,actions,btn,use-disk,keep-mine,merge,apply-merge,captions,caption,caption-disk,caption-yours}` | — |

Semantic token classes on syntax highlights follow the `tok-<type>` scheme (`tok-function`,
`tok-keyword`, ... — one per semantic token type name).

### Stability

This taxonomy is a **semi-stable contract**: hosts style these names directly, so *renaming or
removing* a documented class, attribute, or custom property is a breaking change (major bump).
*Additions* — new element kinds, new classes on new surfaces, new custom properties — are not
breaking and can land in minor releases. The element-class scheme is explicitly open-ended (see
above); do not treat the kind table as exhaustive.
