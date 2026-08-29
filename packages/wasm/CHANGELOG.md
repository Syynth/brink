# @brink-lang/web

## 0.17.0

### Minor Changes

- 953daff: `getDiagnosticRegistry()` — every diagnostic code the compiler knows, with
  its title, default severity, whether `[lints]` can override it, its written
  explanation when one exists, and an author-facing category for the codes a
  project can actually configure.

  Read this rather than keeping a code list in TypeScript: a hand-maintained
  copy is wrong the moment a code is added, and wrong silently.

  The `overridable` flag matters more than it looks: only 30 of the 189 codes
  can be overridden at all — the analyzer refuses every code whose default
  severity is not `warning`. A UI that ignores it offers a level picker for a
  code the analyzer then discards.

  Each row also names the source surfaces the code can arise on, so a
  `strict-ink` project is not offered settings for `.brink`-only diagnostics
  it can never produce.

### Patch Changes

- 40e941a: `[lints] Exxx = "allow"` now suppresses the diagnostic. It previously did
  nothing at all — `effective_severity` returned the code's default severity
  for `allow`, and every consumer reported it — so a project had no way to
  turn a diagnostic off.

  Any diagnostic whose default severity is not `Error` can be overridden,
  including the advisory tier: `E189`, the ink `TODO:` note, is configurable.

- 0b07df5: Debugger D8's control half (issue #3186) bridged through wasm to the studio
  (#3232) — D9 (#3187) bridged only the read half (program → source position
  resolution). `StoryRunnerHandle` and `StorySessionHandle` (`@brink-lang/web`)
  gain `debugRun`/`debugStep`/`debugBreakpointAdd`/`debugBreakpointRemove`/
  `debugBreakpointSetEnabled`/`debugBreakpoints`, wrapping the runtime's
  `Story::debug_run`/`debug_step`/`BreakpointSet` (feature `debug-hooks`, now
  built unconditionally into the `brink-web` wasm package rather than a
  build-time toggle nothing in the studio's pipeline passes). `@brink/wasm-types`
  gains the `Breakpoint`/`DebugRunOutcome`/`DebugStopReason`/`StepMode` wire
  shapes.

  `@brink/studio-store` gains a `DebugSessionProvider` capability extension on
  `SessionProvider` (one extension covering both pause/step/breakpoints and
  D9's previously-uncaptured position-resolution capability, per the issue),
  implemented by `LocalSessionProvider`, plus a new debug slice
  (`debugCapable`/`debugBreakpoints`/`debugLastOutcome`/`debugStatus`) and the
  `debug.run`/`debug.stepInto`/`debug.stepOver`/`debug.stepOut`/`debug.
breakpointAdd`/`debug.breakpointRemove`/`debug.breakpointToggle` commands,
  registered alongside `story.*` at the app boundary.

  **Scope honesty**: this is real, working plumbing (proven over a real
  `WebSession` in `crates/brink-web/src/session.rs`'s `debug_control_tests`,
  plus a vitest suite over the store slice) — but the studio still cannot
  compile a project WITH debug info at all (#3229, a separate, un-made
  maintainer ruling on the toggle mechanism), so an end user will not see any
  of this working yet. No UI consumes the new slice either — the editor
  gutter / current-line highlight is a separate, later ticket. This PR lands
  the bridge ahead of #3229 because the plumbing is independent of which
  toggle mechanism wins.

- b43ebbc: The debug info section's file table now carries each file's `source_hash`
  and line index (#3261).

  `source_hash` lets a consumer detect that the source it is measuring against
  is not the source the program was compiled from — the debounced-recompile
  window, or an edited file on disk — and answer "stale" instead of a
  confidently wrong address. Per-file, so one dirty file no longer degrades
  debugging everywhere.

  The line index lets `file:line` resolve with no source text supplied at all,
  which is what a remote debugger frontend needs and what keeps line-to-byte
  conversion in one place instead of one per consumer.

  Also makes `content_hash` a specified stable hash (FNV-1a 64) rather than
  `std`'s `DefaultHasher`, whose algorithm Rust documents as unspecified
  between releases. Hashes are now written into artifacts and compared later,
  so the algorithm is part of the wire contract.

- e4a20b3: Debugger D7 (issue #3185): the runtime can now name a call frame's live
  temps/parameters, not just count them. `DebugFrame` (`brink-runtime`) gains
  an additive `locals: Option<Vec<DebugLocal>>` — `slot`/`name`/`value` per
  declared `~ temp` or parameter currently in scope, resolved from D6's
  `DebugInfo` `LocalsTable` (populated here — D6 shipped only the structural
  framing) against the call frame's own `temps`. `None` when the linked
  program carries no `DebugInfo`; `Some(vec![])` when it does but the frame
  genuinely has no locals, so a consumer can tell the two cases apart. The
  existing `temps: usize` count is unchanged.

  Values are exposed structurally, not as another display string like
  `DebugGlobal.value`: the new `DebugValue` enum models every kind the debug
  surface can currently distinguish by name (int, float, bool, string, null,
  list — member names, divert target — resolved path, struct — shape name
  plus named fields, recursively, and handle — kind plus id), falling back to
  the existing display-string form only for the long tail of kinds with no
  dedicated variant yet.

  `DebugState`'s JSON (`debug_snapshot()`/`flow_debug_snapshot()` on
  `EditorSession`/`StoryRunner`) carries this same `locals` field on each
  call-stack frame, and `@brink/wasm-types` gains the matching `DebugLocal`/
  `DebugValue`/`DebugField` mirror types (a `type`-tagged union for
  `DebugValue`, so a JS consumer can `switch` on it). This is a wire-
  observable addition (new optional key plus new exported types), so it
  needs this changeset. Nothing renders it yet: the State View locals panel
  (#3140) is separate follow-up UI work that consumes this surface, not part
  of this PR.

- 132a3a4: Debugger D4 (issue #3182): the runtime can now report exactly where
  execution is, not just which knot/stitch it's nearest to. `DebugFrame` and
  `DebugSnapshot` (`brink-runtime`) gain an additive `position: Option<{
container_idx, offset }>` — a public mirror of the VM's internal call-frame
  position, cross-checked against the already-public `Program::resolve_address`
  in the new proof tests. The existing `location`/`current_location` strings
  are unchanged.

  `DebugState`'s JSON (`debug_snapshot()`/`flow_debug_snapshot()` on
  `EditorSession`/`StoryRunner`) now additionally carries this same
  `position` field on the snapshot and on each call-stack frame — the exact
  JSON the studio's State View already parses on every refresh. This is a
  wire-observable addition (new optional key), so it needs this changeset,
  but nothing renders it yet: resolving `(container_idx, offset)` to a source
  location and wiring it into the State View UI is separate follow-up work
  (D6 `#3184` / D9 `#3187`, `docs/debugger-spec.md` §6), not part of this PR.

- 76bbdeb: Add a per-session debug-info compile toggle (#3229).

  `EditorSessionHandle.setDebugInfoEnabled(enabled)` / `debugInfoEnabled()`
  control whether this session's compiles emit the D6 `DebugInfo` section.
  Off by default, matching the ship policy; a host turns it on for the
  session it is about to debug and off when that session ends.

  This is what makes the debugger reachable at all: without the section, the
  runtime position, locals table and program→source resolver landed by
  D4/D6/D7/D9 resolve to nothing, because the studio's live session runs on
  exactly the bytes the editor session compiles.

  The caller must recompile for the flag to take effect — it governs what the
  next compile emits. Toggling bumps the session generation, so the next
  compile is a real one. The studio store exposes it as `setDebugInfoEnabled`,
  which recompiles for you.

- 5079c84: Debugger D9 (issue #3187): the wasm bridge for D4's runtime position (#3182)
  and D6's `DebugInfo` section (#3184) — the program→source resolver the
  studio Location protocol's `program` space names as landing "with its
  consumer" (`docs/studio-shell-spec.md` §6.1).

  `@brink-lang/web`:

  - `StoryRunnerHandle.resolveDebugPosition(containerIdx, offset)` and
    `StorySessionHandle.resolveDebugPosition(containerIdx, offset)` resolve a
    runtime `(containerIdx, offset)` position — exactly what `debugSnapshot()`'s
    `position`/call-stack frame `position` fields report — to the source range
    it was compiled from, via the loaded program's `DebugInfo` section. Returns
    `null`, not a throw, when the program carries no `DebugInfo` section (a
    compile without `--debug-info`) or the position doesn't resolve; callers
    must gate on program-identity checksum before trusting a non-null result
    (`docs/live-inspector-spec.md` §5's `sessionDegraded`).
  - `ProgramModel`'s `KnotNode` gains `container_idx` (the container's index in
    the compiled program, matching a runtime `DebugPosition`) and its `disasm`
    changes shape from `string[]` to `{ offset, text }[]` — each decoded
    instruction now keeps the byte offset it decoded from, so a "current
    instruction" highlight in the Program Explorer has something to key on.
    This is a breaking shape change to `disasm`, gated behind the same
    `--debug-info`-independent Program Explorer feature that already ships —
    every consumer in this repo is updated in this same PR.

  `@brink-lang/studio` (bundles `studio-shell`/`studio-ui`):

  - `@brink/studio-shell` implements the `program` Location resolver
    (`makeProgramResolver`) and the `session → program` half of the chain
    (`resolveSessionPositionRef`), plus the `programIdx:offset` address
    encoding (`encodeProgramAddress`/`parseProgramAddress`).
  - The Program Explorer (`ProgramView`) highlights the currently executing
    knot and instruction, gated on `sessionDegraded` — suppressed, not stale,
    the moment the running program's checksum diverges from the studio's
    latest compile.

- b0f5ccf: Content-logic delimiters classify as operators: the `{`/`}` around inline alternatives, conditionals, and interpolations — and the `|` between alternative branches — now carry an operator semantic token instead of no token at all, so they render in the code color rather than blending into the surrounding dialogue/action prose (author feedback). Prose-absorbed and escaped braces/pipes stay uncolored, in both the ink and native classifiers.
- 0fed188: The `\` of an escape now carries its own `escape` semantic token, so the
  editor dims it while the character it protects reads as ordinary prose. An
  escape exists to say "this character is text"; the mark that says so should
  be legible when looked for and invisible when reading.

  Also fixes the same mis-highlight #3154 fixed for `.ink` on the NATIVE
  surface: an escaped `{` in a `.brink` prose line was painted as
  interpolation syntax, because the native prose carve-out
  (`is_prose_run_container`) listed `TEXT`/`CUE_NAME`/`TAG`/`SCENE_TITLE` but
  not `ESCAPE`.

  `escape` is appended to the token legend as index 18; existing indices are
  unchanged.

- cf2d5a4: `[project] drafts` in `brink.toml`: path globs naming work the author has
  deliberately not wired into the story. A file matching one that is also
  unreachable from the entry is a **draft** — it shows no "not included"
  banner, and is marked as a draft wherever the studio names it (the Binder
  row, the Continuous section heading, the Single File header, the Code
  view's tab).

  Reachability wins: a marked file the entry still `INCLUDE`s is not a draft
  at all, so draft status can never exclude a file the story reaches.

  New: `EditorSessionHandle.getDraftPaths()`, and a `documentMark` slot on
  `ShellProvider` for any host that wants a status beside a document's name.

- 237fd39: Escaped characters no longer receive semantic tokens. `\*`, `\[`, `\{` and
  friends are prose by definition, but the escaped sigil's parent is an `ESCAPE`
  node rather than `TEXT`, so it slipped past the classifier's prose carve-out and
  `\*Party` painted its asterisk as an operator in the middle of a line of dialogue.
- 42efdf1: Fixed a silent data drop in codegen (issue #3181): a content line that took
  the `EmitContent`/`ChoiceOutput` _flattening_ path (recognized-line
  recognition declined it — e.g. text mixed with an inline
  conditional/alternation) always shipped with `LineEntry.source_location:
None`, even when a real location was available from `hir::Content::ptr`.
  Only the recognized-line path populated it before.

  `lir::Content` now carries a `source_location` resolved the same way the
  recognized path already does, threaded through to every `add_line` call in
  the flattening path. This changes compiled output — `LineEntry
.source_location` (reachable through `EditorSession`/`StoryRunner`'s
  `program_inkt()`, the Program Explorer's `.inkt` text dump) is now populated
  for flattened-path lines that previously had none — and also fixes
  `brink-intl`'s `lines.json`/XLIFF export for the same lines, which copies
  `source_location` verbatim for the translation toolchain.

  A content line inside a string-literal interpolation
  (`lir::StringPart::Literal`) still has no location — that gap is deeper
  than this fix reaches (HIR string literals carry no span at all today,
  tracked separately, not folded into this PR) — and a tag's own line-table
  entry (`ContentPart` inside `lir::Content::tags`) still has none either,
  since `hir::Tag::ptr` is discarded when tags are lowered and reusing the
  enclosing content's range would misattribute a tag's own byte span.

- 87521b2: Hover content delivered to consumers that cannot resolve link targets — the
  language server and `brink ide hover` — has its `[text](#N)` references
  flattened back to plain labels.

  `#N` indexes `HoverInfo.links`, which only a renderer holding that list can
  resolve. Without this the LSP would hand an editor a live markdown link
  pointing at a fragment that does not exist.

- fae5eb5: References in the hover card are now navigable. The cells an `effects` row
  names, and the file in _Defined in_, are links to their declarations —
  clicking one reveals it, the same route goto-definition already used.

  The card named things without letting you reach them, which made it a
  readout rather than a way to move.

  - `HoverInfo` gains `links`, and content refers to them as `[text](#N)`. An
    index rather than a path inside the link target, deliberately: a path in
    markdown has to survive `)` and `:` inside it, and that escaping is a
    silent-corruption bug waiting on the first bracket in a filename.
  - Atoms with nowhere to go stay plain text — `calls` atoms are raw external
    names with no symbol to point at, and the compiler-owned `rng` cell has no
    declaration. A link that navigates nowhere is worse than plain text.
  - An embedder that passes no navigate hook gets plain text too, the same
    rule "Add to dictionary" follows.
  - Effect atoms are now individually code-styled rather than the whole row
    being one code span, and clause labels and status words (`pure, silent,
total`) read as prose.

- 0d32184: `[project] indent` is now the single source for indentation width, and the
  default when it is unset is **4** (ruled 2026-08-27).

  - `brink-fmt` no longer keeps a default of its own — it defaulted to two
    spaces while the editor indented by four, which is exactly the
    disagreement this setting exists to prevent.
  - `brink fmt` discovers the `brink.toml` for each file it formats.
  - The language server reads the project's width and ignores the client's
    `tabSize`, which would otherwise be a silent second source.
  - The editor's `indentUnit` reads the configured width instead of
    hardcoding four spaces; the indent guides follow it automatically.

  New: `EditorSessionHandle.getConfiguredIndent()`, and `DEFAULT_INDENT` from
  `@brink-lang/editor`.

  Also: the status bar no longer says "— file not analyzed" for a draft
  (#3145), matching the out-of-scope banner it accompanies.

- cfa5738: A character cue now teaches the prose dictionary the spelling the prose
  actually uses, and the cue line itself is no longer spell-checked.

  An ink cue is written in caps (`@GRISWOLD:<>`) while the prose that mentions
  the same character is not (`Griswold`), and dictionary matching is literal —
  so seeding the cue's own spelling left every prose mention of the character
  underlined.

  Two halves, and neither works alone:

  - Cue names are seeded in title case rather than as written. Seeding _both_
    spellings does not work: with `["GRISWOLD", "Griswold"]` in the dictionary
    the all-caps use is still reported, because Harper's proper-noun metadata
    drives a capitalization rule that fires regardless.
  - Character-cue lines are excluded from prose ranges. A cue is the speaker's
    name, not prose — the same category as the knot and stitch names prose
    checking already excluded — but an ink cue line is an ordinary content span
    to the HIR projection, so it was being checked. With title-case seeding it
    would now be reported.

  `griswold` in prose is still flagged, which is the point: it is a real
  misspelling of a proper noun. Parentheticals and dialogue lines are still
  checked — those are written prose.

  `@brink-lang/editor` exports `withoutCueLines`, the second half, for hosts
  composing prose ranges themselves.

- a260c8c: Add the inverse debug resolver (#3246): `resolveSourceRange(file, start, end)`
  on both `StorySessionHandle` and `StoryRunnerHandle`.

  D9 mapped a running program address to source. This is the other direction —
  the span of source text to the program address to break on — which is what a
  breakpoint gutter needs, since the runtime keys breakpoints by
  `(containerIdx, offset)` while an editor speaks in source.

  Takes a half-open **byte** range rather than a line number: the runtime holds
  no source text and no line table, so line-to-byte conversion belongs with the
  caller, where the source already is.

  Returns `null` when the span holds no executable code — a comment, a blank
  line, a line whose code folded away — or when the artifact carries no debug
  info. That `null` is a real answer callers must render, not an error to
  swallow: refusing to arm a breakpoint visibly is better than arming one that
  can never hit.

- 736061f: Line-granular stepping (#3264), alongside the existing instruction stepping.

  `Story::debug_step_line(mode, …)` advances to the next **source line** — the
  granularity every GDB-style debugger means by `step`/`next`/`finish`. The
  existing `debug_step` remains and is unchanged: both granularities are
  first-class, because the studio presents the `.inkt` disassembly beside the
  source and drives each directly.

  `DebugStopReason` gains `noLineInfo`, reported when a line-granular step is
  asked for on an artifact that cannot say which line execution is on — no
  debug info, or a file compiled without source text. It is reported rather
  than quietly behaving like instruction stepping, which would turn a missing
  line index into "why does step take four presses" instead of a legible
  "this build has no line info".

- 029dae2: Prose checking: spelling and light grammar over a manuscript's prose.

  The engine is Harper, in its own lazily-loaded wasm module — 6.5 MB gzipped,
  larger than the entire compiler, so it is never in the main bundle and an
  embedder that registers no checker pays nothing.

  What makes it usable on fiction rather than hostile to it: the checker only
  ever sees `content` spans with interpolations subtracted (never diverts,
  tags, or logic), and its dictionary is seeded from the project's own names —
  including the character cues, so writing the manuscript teaches it. Without
  that, every invented name reports as a misspelling.

  `@brink-lang/web` gains `getProseDictionary`, `getConfiguredProseDialect`
  and `getConfiguredProseEnable`. `@brink-lang/editor` gains the `ProseChecker`
  seam and a shared diagnostic-source registry, so the compile and the prose
  check no longer overwrite each other's squiggles. `@brink-lang/studio` gains
  the Prose settings section and registers the checker.

- c3ebae8: The author's prose dictionary now lives in `brink.toml`, under `[prose]
dictionary`, and is visible and editable in Project → Prose.

  It previously went to a `.brink-dictionary` sidecar with no UI anywhere, so
  "Add to dictionary" wrote a file nothing displayed — the word stayed
  underlined until the next compile and there was no way to see the list or
  undo an entry. The settings panel now shows the words, adds and removes
  them, and the editor action writes to the same place.

  Matching is literal: `Griswold` and `GRISWOLD` are two separate entries.

  Package-level notes:

  - `@brink-lang/web` gains `EditorSession.getConfiguredProseDictionary()`,
    reading `[prose] dictionary` from the applied config. Like the other
    `configured*` readers it is wholesale-replaced on every apply, so a word
    removed from the file stops being a known word.
  - `@brink-lang/editor` gains a `onAddToDictionary` document-session option
    and no longer owns dictionary storage: the list is the embedder's
    `brink.toml`, so the editor package no longer writes it. The
    `PROSE_DICTIONARY_FILE` export is removed. An embedder that does not pass
    `onAddToDictionary` no longer sees the "Add to dictionary" action at all,
    rather than seeing one that silently does nothing.

- b6d2af7: Retired the dormant `Opcode::SourceLocation` (ruled 2026-07-19, Q-R1): the lossy `line:col` debug carrier the brink compiler never emitted. The Program Explorer's disassembly (`program_model()`) no longer recognizes byte `0xFE` as `source_location LINE:COL` — a bytecode blob carrying that byte now disassembles as a decode error at that offset instead. No compiled program is affected (codegen never emitted this opcode), but the disassembler's behavior for arbitrary/malformed bytecode changed, so this ships as a changeset per house rule. Debug info's real replacement is a new strippable `SectionKind::DebugInfo` section (tag `0x11`), tracked separately under epic #452.
- ef99ec9: Hovering a function that calls `RANDOM` no longer shows a raw internal
  handle. The effects row printed `writes: GlobalVar(0x5eed0000d1ce)`; it now
  reads `writes: rng`.

  The compiler-owned RNG state cell has no symbol-index entry, so the hover
  row's name lookup fell through to the id's debug form. Naming now goes
  through one shared authority (`brink_analyzer::effect_atom_name`) used by
  both surfaces that print effect atoms — the hover row and the `E103`
  exceedance message — so an author reads the same name in both, and the same
  one they would write in `@[effects(writes rng)]`.

- 641e278: Three new semantic token types, split out of the operator/keyword buckets so themes can color marks by what they do (theme ruling 2026-08-25): `marker` (choice bullets, gather dashes, weave brackets — position-checked, so expression-position `*`/`+`/`-` stay operators), `divert` (`->`, `->->`, `<-`, glue), and `halt` (`END`/`DONE`). Header equals-runs now classify with their definition (namespace/function) instead of as operators, so a knot header reads as one mark.

## 0.16.0

### Minor Changes

- 8bd2fcb: Out-of-scope editor banner (#3017): the compile closure is now surfaced
  through the wasm boundary — `EditorSession.compilation_closure()` /
  `EditorSessionHandle.getCompilationClosure()` return the project-relative
  paths of the exact file set the latest compile built from (empty before
  any compile; read-only). The studio renders a banner above the editor of
  any source file outside that closure ("Not included in the project —
  nothing INCLUDEs this file, so it is not analyzed"), with a one-click
  "Add INCLUDE to <entry>" action for the ink flow, plus a "— file not
  analyzed" status-bar note. Absent diagnostics look identical to clean
  diagnostics; this makes the difference visible.
- 9985bcf: References dressing (card-stack PR E). New wasm entry point
  `find_references_with_kinds_at` (wrapper:
  `EditorSessionHandle.findReferencesWithKindsAt`): every reference site
  classified by how it uses the symbol — `decl`, `call` (UFCS-desugared
  calls included), `divert`, `read`, or `write` (assignment targets and
  `++`/`--`). In the Search panel's references mode, the declaration card
  pins first with an accent border and `decl` badge, and every site
  carries its kind badge; the store re-resolves through the kinds variant
  at the declaration anchor (plain locations remain the graceful
  fallback).

### Patch Changes

- eba0faa: Bounded-edit ingress (#3064 C1): `applyEditsDocument(doc, edits)` applies a CM6 change list Rust-side — the full document no longer crosses the wasm boundary on every keystroke, and the write is source-only: the fused eager whole-project analysis that `updateDocument` forced per keystroke (and that nothing on the keystroke path consumed — diagnostics are debounced-compile-driven) is no longer computed until something actually pulls it. The editor's element-type field uses the delta path automatically for single-range edits on file handles, falling back to the full push for multi-cursor batches, fragment views, and older wasm builds/mocks. `updateDocument` is unchanged for compatibility.
- c0f357b: `EXTERNAL` functions are renameable behind the Force gate (ruled 2026-08-24): `prepare_rename`/`rename` accept them (declaration + every call site), but the safe-rename verdict is ALWAYS unsafe, carrying a new `E190` entry naming the host binding ("the engine must re-register the external under the new name") — so the rename only applies through the breakage report's Force path. Builtins remain non-renameable.
- d8cfbcd: Ink lowering no longer lowers every knot and declaration twice per edit (#3088): the db road's assembler harvests the declaration surface from a decl-only composition instead of a discarded whole-file lowering. Large-file keystroke re-analysis drops ~35% (the HIR-lower stage 24 → 14 ms on the 5.9k-line bench fixture). Behavior fix riding along: the file-level `#@module`/`#@was` arbitration diagnostics (E095 self-alias, E049 `#@was` without `#@module`) were silently dropped with that discarded sink and now reach editor diagnostics; E049 is error-severity, so an orphaned `#@was` now fails compilation loudly instead of being ignored.
- 1609068: Keystroke micro-work toward the 8 ms frame budget (#3064): a config epoch invalidates delta-slice caches on dialect/host-manifest swaps (fixing a stale-classification bug under unchanged segment keys); one manifest fetch per document version; element-type derives per-line infos per segment under the delta protocol's version keys; the keystroke path serves the edited knot's semantic tokens from a classifier-only slice (no analysis pull — the symbol index and resolution passes leave the synchronous path entirely) with resolution-refined colors landing on the deferred refresh; occurrence highlights defer during large-document typing bursts (selection moves stay instant). Per-keystroke instrumented work on a 6k-line document drops to ~6–7 ms, with most keystrokes completing below the Event Timing API's 16 ms reporting floor.
- 29541b3: Hovering a `LIST` or a list item now shows the full member set — declared order, every member's numeric value including the defaulted ordinals (mirroring the LIR lowering's rule: count from 1, an explicit value resets the counter), default-active parens preserved, the hovered member bolded. Internally the hover is now assembled from an ordered section-provider table (head line + Markdown blocks), so future per-kind hover content is a one-provider addition; the _Defined in_ note moved to the end as a footer.
- 78e4dd0: Option A total (ruled 2026-08-24): the editor's per-edit analysis routes
  through the db's incremental `analysis_query` — the `IdeSnapshot` deep
  clone that cost ~28–33 ms per keystroke at large-file scale (#3063) is
  deleted, and `updateDocument`'s wasm share drops accordingly. Wire-visible
  changes: the internal perf counters `ide.snapshotClone`/`ide.applyAnalysis`
  retired (`ide.analyze` now measures the incremental pull; compare
  recorded runs across the boundary with that in mind), and `getStoryGraph`
  returns an empty graph instead of `null` on a fresh session (analysis is
  always available now; the `StoryGraph | null` type is kept). Also closes
  the #2885 options-sync gap: an equal-options `compileProject` can never
  cold-invalidate the live analysis.
- 80dd24f: Outbound delta protocol (#3064 option A): per-keystroke wasm→JS payloads for line contexts and semantic tokens drop from whole-document JSON (~1.4 MB combined on a 6k-line file) to a small segment manifest plus the edited knot's slice. New wasm surface: `getSegmentManifestDoc` (per-segment version keys — salsa identity `index:generation`, stable across shift edits, changed exactly when a segment's content changes, ABA-safe by generation) plus `getSegmentLineContextsDoc`/`getSegmentSemanticTokensDoc` slice fetches. `DocHandle.lineContexts()`/`semanticTokens()` assemble transparently from a version-keyed slice cache — same return types, no consumer changes — and fall back to the whole-document queries for fragment views, native files, older wasm builds, and mocks. Delta-reconstructed results are parity-gated against the assembled queries across the full corpus.
- 85a1700: Ink files now lower through the per-knot segment road (#3084): a keystroke inside one knot re-lowers that knot's segment only — every other knot's lowering memo backdates, shifted-but-unchanged knots included — and the analysis path no longer pays a whole-file parse per edit. Large-file keystroke re-analysis drops accordingly (see `docs/per-knot-incremental-lowering-spec.md` for the measured before/after). Output is byte-identical (HIR, symbols, admission) with one declared exception: the per-file diagnostics ARRAY now arrives in a deterministic segment-major order instead of the old kind-grouped interleaving — the diagnostic set, ranges, codes, and messages are unchanged, only vector order moves, and only for files where multiple diagnostic sources interleave.
- bd2e490: Two-range model for container spans (#3054): `HirSpan` gains `content_end_line` — the TIGHT end (last line of actual content, trailing whitespace and the next declaration's doc block excluded) alongside the structural `end_line` that runs to the next sibling. Rails and their tooltips use the tight range, so a two-line function no longer paints (or reports) itself through the next function's docs; choice rails get eight golden-step color buckets so siblings are distinct; conditional-branch tooltips show the condition.
- 3c8d180: ink `TODO:` author notes now surface as `E189` Info-severity diagnostics (issue #3050). Lowering previously dropped `AUTHOR_WARNING` nodes silently; each now emits one diagnostic whose message carries the note's text (`TODO: <text>`), visible through every diagnostics channel (`compile`, Problems). Info severity never gates a compile, and the code is `[lints]`-tierable like any other (`E189 = "allow"` hides TODOs).
- d043c59: `line_contexts` reports a new `todo` line element for ink `TODO:` author-note lines (#3050) — a trivia-facet classification like comments (the HIR never sees `AUTHOR_WARNING`), so the editor's line-class road marks TODO lines on the wasm path, not just the regex fallback.
- 62fdee9: Performance (#3065, no behavior change — wire output byte-identical,
  pinned by an every-offset equivalence test): the per-compile pulls'
  byte→UTF-16 offset conversions (`getProjectOutline`, `getStoryGraph`,
  `compileProject` diagnostics) now go through a per-file prefix-sum index
  built once per pull instead of a linear scan from offset 0 per offset —
  previously 17,744 scans per compile cycle on a studio-scale project,
  making outline/story-graph O(symbols × file size).
- c0d9a61: `ClassifierSession` — the capability-stripped main-thread session (editor worker architecture W3, `docs/editor-worker-spec.md` §4). The wasm module exports a new single-document session whose surface is exactly the keystroke path's needs — delta/full-text ingress, segment manifest, per-segment line contexts and classifier tokens, dialect config — with no project method exported and write paths that never trigger an analysis pull (parity with the full session's slices is pinned Rust-side). `@brink-lang/web` wraps it as `ClassifierSessionHandle` (feature-detected: `available` is false on older builds and mocks). In the editor, full-file document handles attach a `ClassifierMirror`: the keystroke path's line contexts and fast tokens serve from the classifier's own analysis-free instance (with its own version-keyed slice cache), and the fast-token road blends positionally — cached refined slices keep their colors while uncached (edited) segments serve from the classifier. Mocks and older wasm keep the previous session-road behavior exactly.
- bfdde5e: Wasm-internal perf counters (measure-first ruling, 2026-08-24).
  `EditorSessionHandle` gains `setPerfEnabled`/`getPerfCounters`/
  `resetPerfCounters` over a new counter store inside the wasm: the
  per-keystroke reanalysis decomposed by phase (`ide.updateSource`,
  `ide.snapshotClone`, `ide.analyze`, `ide.applyAnalysis`), the editor
  compile (`ide.compile`), the per-compile outline/story-graph builds, and
  an `ide.byteToUtf16` call counter. `perfCompileProbe(entry)` runs the
  #2885 revision-stamp experiment directly — two back-to-back compiles with
  no edits, returning `[firstMs, secondMs]`. Counters are off by default and
  cost one branch per site while disabled; behavior of every existing call
  is unchanged.

## 0.15.0

### Minor Changes

- 9fc8665: Binder "Library" section for mounted `std/` files (issue #2343, part 2 of #2306's ruling "Mounted stdlib
  presents as a read-only library node" — part 3, session-level read-only enforcement, shipped separately
  in #2342). `list_files`/`project_outline`/`story_graph` (`@brink-lang/web`) switch from **excluding**
  mounted stdlib files entirely (#2231's phantom-row fix) to **listing them flagged** (`mounted: boolean` on
  `ProjectFile`/`FileOutline`/`StoryGraphNode`) — dropping the exclusion without adding a consumer that
  renders the flag would reintroduce the exact phantom-row bug #2231/#2303 fixed, so this ships both
  together. `EditorSession::remove_file` (`@brink-lang/web`) and `rename_file` now refuse a mounted path
  (the delete/rename route gap #2343's review found: previously unreachable only because `list_files`
  excluded the mount from the Binder) — `remove_file` gains a `boolean` return (previously `void`).

  `@brink-lang/studio`'s Binder renders a visually distinct, collapsed-by-default "Library" section below
  the project's own file tree: browsable (expand/collapse a folder tree, click/double-click to open a file
  read-only) but with no drag, rename, delete, or new-file affordances. `@brink/studio-store`'s search slice
  (internal) excludes mounted files from `runSearch`'s candidate list — "Excluded from save-all and
  search/replace" per the ruling — and `ProjectSession.markAllSaved` (`@brink-lang/editor`) does the same for
  `file.saveAll`. The binder slice's `applyMoveResult`/`undo` now surface an `applyEdit` refusal (a structural
  move or undo landing on a mounted path) as a "skipped N read-only file(s)" warning instead of a silent
  no-op behind a success toast.

  A mounted file's CM6 view (`@brink-lang/editor`'s `DocumentSessions`) is now genuinely non-editable —
  `EditorState.readOnly` + `EditorView.editable.of(false)`, the same pattern `conflict-view.ts` uses for its
  "ON DISK" pane — rather than relying solely on the wasm-layer write refusal to make a keystroke silently
  revert. `ProjectSession` gains a public `isReadOnly(path)` query for this. Navigation (goto-def/hover) into
  a mounted file lands in the same read-only view via the existing open-file path — no special-casing needed.

### Patch Changes

- 3b94ac6: F34 + F35 (ruled 2026-07-19): the comparator write-guard, keyed on
  `ExecMode`, plus bevy-brink's profile-defaulted mode. Observable through
  `@brink-lang/web`, brink dialect only (vanilla ink cannot reach a
  `sort_by`/`sorted_by` comparator):

  - **F34 — comparator write-guard.** In the re-entrant comparator runner
    (the `sort_by`/`sorted_by` value-call boundary), a WORLD-WRITE performed
    by a comparator mid-sort now faults under `ExecMode::Dev` with the new
    tracked fault `ComparatorWroteState` (sibling to
    `ComparatorNotAFunction`/`ComparatorReturnType`/`ComparatorEscaped`).
    Under `ExecMode::Prod` the check is skipped — the write executes,
    defined and deterministic, because the stable merge-sort's comparison
    sequence is fixed (the mode changes WHERE execution stops, never WHAT
    the sort produces). World-write = global-var writes (direct, or through
    a `ref`-parameter pointer / path projection) and every RNG-cell advance:
    a `rand` draw inside a comparator IS a world-write and dev-faults — a
    random comparator is exactly the nondeterminism the pure·silent contract
    bans. Reads stay legal at runtime (E119's static bound owns the read
    posture — no runtime read-guard), and visit-count increments from the
    comparator's own in-story dispatch are NOT world-writes (explicitly
    exempt). This is the gradual-mode runtime residual of the E119 gate,
    reached only by an opaque comparator whose origin the checker cannot
    prove.

  - **F35 — bevy-brink profile-defaulted `ExecMode`.** Core `brink-runtime`
    keeps `ExecMode::default() == Dev`. Where `bevy-brink` spawns a flow it
    now stamps a host-selected mode whose default keys off the build
    profile: `Dev` under `debug_assertions`, `Prod` in a release build — so
    a shipped game defaults to keep-moving and an in-editor session to
    fault-loud. Carried by the new `BrinkExecMode<M>` resource; a host pins
    a mode regardless of profile via `BrinkPlugin::with_exec_mode`, with a
    per-flow runtime override still available through
    `FlowInstance::set_exec_mode`.

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- 462f61b: Analyzer/IDE: semantic-type honesty for unregistered host types (issue
  #1027).

  `external_check::resolve_type` (hover, signature help, argument pickers) and
  `infer::type_ref_to_ty` (strict inference) now classify a `TypeRef` through
  one shared helper (`type_resolution::classify`), so a semantic-type name the
  host manifest doesn't register resolves identically on both paths — never a
  confidently-typed name on one side and `Unknown` on the other. That
  divergence was the real story behind #1004: hover rendered `id: var_id`
  with full confidence for an unregistered `var_id` while strict inference
  correctly resolved it to `Unknown`.

  Hover and signature help are also honest about it now: an unregistered
  semantic type renders with an explicit warning marker and an `E040`
  cross-reference (`id: var_id ⚠ unregistered semantic type — E040`) instead
  of a bare, confident name. A registered type (base keyword or a name found
  in the host manifest's `types`) renders exactly as before.

- 87fe945: Analyzer: strict-mode void-return inference for functions with no explicit
  return value (issue #1028).

  A function whose body never carries a value-returning `return <expr>` —
  whether it falls off the end or only ever bare-`return`s, e.g. a wrapper
  that calls a void external and returns nothing — now infers its return type
  as void, matching what an explicit `): void ===` annotation already did,
  instead of escaping as `Unknown` (`E065`) under `types = strict`. A function
  with a real return path (even one whose value's type inference can't pin
  down) is unaffected — void inference reads "never returns a value", never
  "returns a value inference gave up on".

  typed-mode-spec §3 documents `void` as the annotation for a no-return
  function but is silent on what the same-shaped body should infer as when
  unannotated; this closes that gap with the conservative, non-escaping
  reading (spec gap flagged in the PR).

- f7e54e3: Collapsed the editor session's two context-assembly paths into one (#1032).
  `compileProject` now assembles its artifact by querying the session's **own**
  `ProjectDb` — the same database the background analysis pass reads — via the
  new `IdeSession::compile`, instead of standing up a throwaway compiler driver
  (and a second, fresh `ProjectDb`) per call. One db means one file set, one
  lowering, and one analysis-options input feeding both compile and analysis, so
  the two can no longer diverge on host manifest / T1b dialect / TM-3 type policy:
  the class of bug that produced #1004 (manifest missing from the compile path) is
  now structurally unrepresentable rather than closed by wiring each input into a
  second driver.

  Observable through `@brink-lang/web`:

  - `compileProject` diagnostics are now keyed into the session's own db, so an
    included file's error span resolves against that file's real source (correct
    UTF-16 offsets and tab attribution) instead of a throwaway-driver `FileId`
    that could index a different file in a multi-file project.
  - An unknown entry path now returns a clean `{ ok: false, error: "entry file
not found in session: <path>" }` (previously a driver I/O error string).
  - **Bugfix:** an error in a file the compiled entry doesn't `INCLUDE` — a WIP
    scratch file, a second unrelated story open in the same editor session — no
    longer blocks that entry's `compileProject`. Sharing one db for compile and
    analysis meant compile's error gate briefly widened from entry-reachable to
    every file loaded in the session (a regression caught in review before this
    shipped); it's now scoped back to the entry's transitive `INCLUDE` closure,
    matching both the prior throwaway-driver behavior and the CLI's
    `discover`-scoped compile path. The unrelated file's error still surfaces
    through the editor's regular per-file diagnostics — it just no longer fails
    a different entry's build.

  `compileProject`'s JS signature is unchanged. Manifest/dialect/policy behavior
  for single-file projects is unchanged. The CLI's one-driver-per-invocation
  compile path is untouched.

- e3ae45a: IDE: extend semantic-type honesty to inlay hints and the argument-widget
  slot data (issue #1053).

  #1027 made hover and signature help honest about an unregistered semantic
  type — an explicit warning marker and `E040` cross-reference instead of a
  bare, confident name. Parameter-name inlay hints and the `type_name` carried
  on argument-widget slots (`getArgumentWidgetsDoc`) still rendered the bare
  name regardless of registration.

  Both now reuse #1027's `ResolvedType::is_registered()` / `honest_type_display`
  convention exactly: an inlay hint's type portion renders
  `id: var_id ⚠ unregistered semantic type — E040` for an unregistered type,
  unchanged for a registered one. Argument-widget slots gain a new
  `type_display` field carrying the same honest string — `type_name` itself
  stays the bare written name (widget-kind matching, e.g.
  `matchHostWidget`'s `type_name` fallback, depends on it being raw).

- f36faf9: Analyzer: E067 void-assignment check extends to inferred-void functions
  (issue #1054).

  `~ x = f()` / `~ temp x = f()` where `f` resolves to a function with no
  explicit `): void ===` annotation, but whose body never carries a
  value-returning `return <expr>` (issue #1046's inferred-void reading), now
  fires `E067` under `types = strict` — the same "assigning a void call is an
  error" diagnostic an explicitly-annotated `void` function already got.
  Before this fix `collect_void_defs` only ever consulted the knot's own
  `return_type` annotation, so assigning the result of a function that
  returns nothing purely by inference was silently accepted.

  A function with a _declared_, non-`void` return type whose body never
  returns a value is unaffected by this change — that shape is the
  `E150` checker error (issue #1551, "declares a return type but its body
  never returns a value"), not void, and is deliberately excluded from this
  check.

- f71aa3d: NS-A1 (#1107): `Option[T]` lands as the third compiler-known parameterized
  builtin, with the ruled stdlib verb flips as brink-dialect intrinsics —
  text `find`, seq `index_of`/`min`/`max`/`first`/`last`/`pop`, map
  `get`/`contains_value`/`clear` — all returning typed absence (`none` /
  `some(x)`) instead of sentinels or faults. New compileable surface
  (`some(x)` constructor, bare `none` literal, the ten verbs), a new wire
  value tag (`VAL_OPTION`) with lossless `TypedValueJs::Option` on the JSON
  boundary and value-or-null marshalling on the native JS boundary, and a
  new compile diagnostic E107 (bare `none` needs a type from context).
  Vanilla-ink stories are byte-identical; the oracle corpus is unaffected.
- ae7b829: NS-A2 (#1108): the effect-row extension wave — three new row dimensions
  (`emits`, `tags`, `faults`; conservative-total, per-SCC inferred, bool v1)
  and the `@[effects(…)]` assertion final form (args from
  {pure, silent, total} plus the existing reads/writes/calls clauses,
  exceedance-only). The rows themselves are additive metadata (a new
  `EffectRows` section version carrying an extension-flags byte; episodes
  byte-identical), but the assertion surface is compile-behavior observable
  through `@brink-lang/web`: new annotation-line syntax `@[effects(…)]`
  parses in the brink dialect, and new diagnostics ship — E108
  (`silent` exceeded: the definition can produce content), E109 (`total`
  exceeded: the definition can fault), E110 (warning: the `#@effects(…)`
  tag spelling is deprecated — it keeps parsing as an alias), E111 (unknown
  annotation name), E112 (annotation line outside the knot/stitch
  leading-run placement). Vanilla-ink stories are unaffected; the oracle
  corpus is byte-identical.
- 5a95959: NS-A3 (#1109): the protocol registry machinery — the CLOSED
  `display`/`compare`/`iterate` set with per-protocol effect contracts.
  Observable through `@brink-lang/web`, brink dialect only:

  - **New hard diagnostic E113** (F6, ruled 2026-07-19): the registry method
    names `display`/`compare`/`next` are reserved — an author declaration of
    any callable or value-bindable kind (knot/stitch/function, param, temp,
    VAR, CONST, EXTERNAL, for-loop variable) is a compile error, not an
    E035-style warning. (E114/E115 — impl contract/shape validation — also
    ship, but impl registration has no source spelling until the
    code-dialect sitting, so they are unreachable from `.ink` input.)
  - **Struct display gains its structural default** (F1: one display path):
    interpolating or `string()`-ing a whole struct now renders
    `Name { field: value, … }` in declared field order (previously a
    provisional positional `{1, 2}`), recursing through nested
    structs/collections/Options; `string()` stays total (a stale shape from
    a foreign save falls back to the positional form).

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- 39f3801: NS-A4 (#1110): the §4b ordering doctrine in the VM + dev/prod ExecMode.
  Observable through `@brink-lang/web`, brink dialect only:

  - **Four new verbs**: `sort(ref a)` / `sort_by(ref a, cmp)` (imperative,
    statement-only — E055/E056/E058 postures) and `sorted(a)` /
    `sorted_by(a, cmp)` (functional twins, F0 ruled 2026-07-19). Two new
    opcodes (`seq_sorted` 0xF8, `seq_sorted_by` 0xF9) appear in
    disassembly. The doctrine order: int/float (numeric promotion), bool,
    string (USV-lexicographic), arrays lexicographic element-wise; stable.
    `min`/`max` gain the arrays-lexicographic roster leg too.
  - **Dev/prod ExecMode** (runtime knob, default **Dev**): a float NaN
    comparand in `sort`/`sorted`/`min`/`max` is now a turn-terminating
    fault in dev mode (previously: A1's always-pinned placement); prod mode
    keeps the pinned non-fabricating total order (`-0 == +0` ties, NaN
    greatest). Hosts opt in via `Story::set_exec_mode` /
    `FlowInstance::set_exec_mode`. The mode is never embedded in `.inkb`
    and never persisted — rows stay mode-independent.
  - **New diagnostic E119**: a `sort_by`/`sorted_by` comparator written as
    an inline `#fn(target)` whose inferred row provably breaks pure·silent
    (global reads/writes, external calls, emits, tags) is a compile error —
    exceedance-only; opaque comparators pass and fall to the runtime
    residual (`ComparatorEscaped` and friends).
  - **F29(a)** (ruled by delegation 2026-07-19): a protocol
    `display`/`compare` impl whose row is provably total no longer inherits
    the conservative faults bit at the E114 contract gate — the
    conservative union applies only to opaque or genuinely fault-bearing
    impls.

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- bb503cc: NS-A5 (#1111): ranges land as a real Value kind (F7) plus the language's
  first value refinement, the inhabited range (`NonEmptyRange`). New
  compileable surface in the brink dialect: `a..b` / `a..=b` range literals
  (E051 under strict-ink), ranges joining the closed iterable set
  (`for i in 0..n` — O(1), never materialized), `len`/indexing over ranges,
  content equality (`1..=6 == 1..7`; display preserves the written form),
  `pick(range)` → `Option[int]`, the `non_empty(r)` →
  `Option[NonEmptyRange]` validator, and `rand::int` as the range leg of the
  one value-directed `int(x)` verb (draws once, writes the RNG cell). New
  wire value tag (`VAL_RANGE`, 0x11) across `.inkb`, the runtime transcript,
  and `.inkt`, with a lossless `TypedValueJs::Range` on the JSON boundary
  and a `{start, end, inclusive}` object on the native JS boundary. Three
  new opcodes (`RangeMakeExcl`/`RangeMakeIncl`/`RangeNonEmpty`). New
  compile diagnostic E117 (`types = strict` only, the E078 template):
  `int(r)` demands NonEmptyRange evidence — provably-empty literals error,
  provably-inhabited literals (CONST refs folded) coerce free, computed
  bounds route through `non_empty`. Under gradual typing the refinement is
  inert (F8's general rule) and the new turn-terminating
  `EmptyRangeDraw` runtime fault is the residual. Vanilla-ink stories are
  byte-identical; the oracle corpus is unaffected.
- aeebad7: NS-A6 (#1112): rng-as-cell formalization + the `std::rand` draw verbs.
  The RNG is formalized as ONE named runtime state cell (the
  `rng_seed`/`previous_random` pair stories have always saved), owned by
  `std::rand` and named `DefinitionId::RNG_CELL` in the effect-row space —
  every draw is an ordinary **write** to it (no new row dimension), on both
  surfaces: the frozen ink `RANDOM`/`SEED_RANDOM`/`LIST_RANDOM` and the new
  brink-dialect verbs. Observable through `@brink-lang/web`:

  - New brink-dialect intrinsics (lowercase, E035-shadowable,
    strict-ink-gated): `float()` → uniform `[0,1)` (nullary; the unary
    `float(x)` conversion is unchanged — arity disambiguates, and any other
    arity is an E031 naming both forms) · `chance(p)` → bool (F3 ruled:
    `p` clamped to `[0,1]`, NaN → false, total; always exactly one draw) ·
    `pick(coll)` → `Option[T]` over arrays and flags subsets (empty →
    `none`, no draw; ranges deferred to A5) · `shuffle(a)` statement-only
    in-place Fisher-Yates (E056 in expression position, E058 arity, E055
    rvalue receiver) · `shuffled(a)` functional twin · `seed(n)`
    statement-only, lowering to the frozen `SEED_RANDOM` op (one cell, two
    surfaces, no drift). `rand::int` is deliberately NOT shipped — it
    arrives with A5's inhabited-range refinement.
  - Four new opcodes (`rand_float` 0xEC, `rand_chance` 0xED, `rand_pick`
    0xEE, `rand_shuffle` 0xEF) with inkt spellings.
  - The draw algorithm is pinned as a stability contract (per-draw
    `seed = rng_seed + previous_random` chain, 24-bit-exact `[0,1)` float
    shaping, top-down Fisher-Yates); seeded replay is transcript-identical
    and the cell round-trips through saves exactly as before.
  - Effect-surface: draw-bearing defs show the write in their row, so
    `@[effects(pure)]` exceedance (E103) now names `rng`, a new `writes:
rng` clause spelling covers draw-bearing defs (a user cell named `rng`
    shadows it), and wake conditions calling draw-bearing defs are rejected
    by the existing purity gate (E105).

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- 4fd4658: NS-A7 (#1113): `Weighted[T]` + heap verbs (stdlib-spec §8, collections+).
  Observable through `@brink-lang/web`, brink dialect only:

  - **Five new verbs**: `weighted(w1, v1, w2, v2, …)` builds a
    `Weighted[T]` table (the brink-dialect spelling of the chartered
    `Weighted { w: v }` literal until B5) — evidence-by-construction:
    statically-malformed tables (empty, dangling weight, literal
    zero/negative/non-int weights) are the **new compile error E120** in
    both type regimes; computed weights fault at construction
    (`WeightedBadWeight`), so a table that exists is always rollable.
    `roll(w) → T` draws through the one RNG cell (seeded-deterministic,
    total over any existing table). The humble heap over ordinary arrays:
    `heap_push(ref a, x)` (statement-only, §4b dev NaN entry-fault / prod
    pinned placement), `heap_pop(ref a) → Option[T]` and
    `heap_peek(a) → Option[T]` (empty → `none`); min-heap by the §4b
    doctrine order — the same comparison core as `sort`/`min`/`max`.
  - **One new opcode** `Collect` (0xFA + kind byte: `weighted_new`,
    `rand_roll`, `heap_push`, `heap_pop`, `heap_peek`) appears in
    disassembly, and **one new value form**: `Weighted` (wire tag 0x19;
    `(weighted (3 "sword") …)` in `.inkt`; construction-literal display
    `Weighted { 3: sword, … }`; F17 multiset equality —
    order-insensitive, multiplicity-sensitive; always truthy; marshals to
    JS as `[{weight, value}]` natively and a typed `weighted` entry list
    on the JSON boundary; survives SaveState round-trips).

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- 640d1d1: NS-A8 (#1114): the numeric tower lands per the ruled mini-spec
  (`docs/tower-mini-spec.md` T1–T5) — `vec2`/`vec3`/`vec4`/`quat`/`mat2`/
  `mat3`/`mat4` as glam-backed compiler-known value kinds with brink-dialect
  constructors, `dot`/`cross`, the tower-wide two-arg `min`/`max` plus
  `clamp`/`lerp`, glam's operator conventions on the frozen arithmetic
  operators (componentwise `+`/`-`/`*`, scalar scale, `mat * vec` transform,
  `quat * quat` composition, `quat * vec` rotation, componentwise negation),
  glam-named component access (`v.x`, `m.y_axis`), componentwise-IEEE
  equality (NaN lanes make a value unequal to itself; tower kinds are NOT
  orderable — ordering contexts fault). New wire value tags
  `VAL_VEC2`..`VAL_MAT4` (0x12–0x18) carrying hand-serialized little-endian
  f32 lanes (never glam's memory layout), one new opcode `Tower(kind)` at
  0xF7, a lossless `TypedValueJs::Tower` kind+lanes form on the JSON
  boundary, `{x, y, …}` / `{x_axis, …}` objects on the native JS boundary,
  and a new compile diagnostic E118 (tower kinds can never implement
  registry protocols — `compare` for a tower kind is impossible by
  construction). Vanilla-ink stories are byte-identical; the oracle corpus
  is unaffected.
- 3ddd90e: F27 truthiness removal + `@[effects(…)]` paren respell + wake-gate gap
  (#1120, #1128). `Option[T]` has no truthiness (F27, ruled 2026-07-19,
  superseding NS-A1's falsy-none): a condition-position Option is now a
  compile error under `types = strict` (new diagnostic E116) and a
  turn-terminating runtime fault under gradual (`OptionTruthiness`) — write
  `== none` / `== some(x)` instead. The `@[effects(…)]` clause grammar is
  respelled to the Rust-meta-item paren shape — `@[effects(reads(gold, hp),
writes(mood), silent)]`; bare top-level idents are always flags, so a flag
  can never be swallowed into an open clause; the colon spelling inside an
  annotation is now E101. The deprecated `#@effects(…)` tag alias keeps its
  legacy colon grammar frozen (E110 unchanged). The `await`-condition purity
  gate (E105) now also rejects draw/fault-bearing stdlib intrinsics called
  directly in the condition (`await chance(0.5)`, `await pop(a)`), consulting
  the same intrinsic effect table effect inference harvests from. Vanilla-ink
  stories are byte-identical; the oracle corpus is unaffected.
- f87adc2: F31 partial-b (#1145): three more tower operator rows land on the frozen
  arithmetic operators — `mat * mat` (composition, matching sizes),
  `mat * scalar` (scale, int operands promote, one direction only), and
  `vec / scalar` (scale down, int operands promote, one direction only; IEEE
  float division, so a zero divisor yields `inf`/`nan` lanes rather than a
  fault, per `docs/tower-mini-spec.md` T4). Every other currently-faulting
  glam-native form — `mat ± mat`, `quat * scalar`, `vec / vec`,
  `scalar / vec`, `scalar * mat`, … — is unchanged and still faults. Brink-
  dialect only; the oracle corpus is unaffected by construction.
- 199c822: #1160: `brink.toml` gains a `[lints]` table — per-code severity overrides
  (`deny`/`warn`/`allow`) plus a `deny-warnings` flag. Resolved through
  `AnalysisOptions::apply_project_config` and consulted by every
  diagnostic-partitioning call site in `brink-db`/`brink-driver`, so
  `brink compile`, `brink ide`, and `brink-environment::compile` now respect
  it: a project's `[lints]` table can turn a previously-warning-only
  diagnostic into a build failure. Absent `[lints]` (the default, unchanged
  case) is byte-identical to prior behavior. Only codes whose default
  severity is `Warning` are overridable — a hard-error-by-default diagnostic
  is never consulted against the table. An unknown or non-overridable code in
  `[lints]` is now reported as a warning rather than silently accepted and
  ignored.

  **No behavior reachable through `@brink-lang/web` changes in this PR.**
  `EditorSession::apply_project_config` (the wasm editor session's own
  config-file entry point) applies only `[project] dialect`/`types`; it does
  not go through `AnalysisOptions::apply_project_config` and does not read
  `[lints]`/`deny-warnings` at all. `IdeSession`'s `AnalysisOptions`
  construction sites are hardcoded to a no-op `LintPolicy::default()`. (An
  earlier draft of this changeset incorrectly claimed the wasm editor session
  already picked this up through a shared seam — it doesn't; tracked as a
  follow-up in Syynth/brink#1366.)

- 8add320: Native `.brink` files can now suppress a diagnostic at the site that
  produces it: `@[allow(E151, E014)]` above a declaration silences those codes
  for the whole span of that declaration (#1161). Previously `@[allow(…)]` was
  an unknown annotation name and hard-failed the compile with E111, so the
  only suppression available was the line-scoped `//brink-disable` comment or
  the project-wide `[lints]` table.

  Three rulings, all observable through `compileProject`:

  - **Only warnings are suppressible.** A code whose default severity is
    `Error` is rejected with the new **E154** — matching the `[lints]` table's
    own hard-error exemption, so an annotation can never be used to ship a
    broken artifact. The admission-validator diagnostics are exempt for the
    same reason and because they never route through the suppression filter.
  - **A source-level `allow` beats a project-level `deny`.** Suppression runs
    before severity resolution, so `@[allow(E151)]` removes the diagnostic
    even under `[lints] E151 = "deny"` or `deny-warnings = true`. The
    annotation names one declaration; `brink.toml` cannot.
  - **A suppression that does nothing is loud.** An unknown or misspelled code
    is the new **E153**; a missing, empty, or non-identifier argument list is
    the new **E155**. One bad argument discards the whole directive.

  Ink-dialect behavior and the oracle corpus are unaffected — the `allow`
  tenant exists only on the native `@[…]` channel. The E111 and E112
  diagnostic _messages_ changed to name `allow` alongside the existing names.

- 1d5c985: #1162: a new `Severity` tier — `Info`/`Hint` — sits below `Warning`, plumbed
  through `brink_ir::Severity`, `brink-project-config`'s `LintLevel`
  (`"info"`/`"hint"` alongside `"allow"`/`"warn"`/`"deny"`), and
  `brink_analyzer::effective_severity` (a `[lints]` entry can now down-level a
  `Warning`-default code to either advisory tier; both are immune to
  `deny-warnings`, same as `allow`). No existing diagnostic code's _default_
  severity changes — this only adds the tier and the opt-in mechanism to reach
  it.

  Reachable through `@brink-lang/web`: `EditorSessionHandle::setLintOverrides`
  now accepts `"info"`/`"hint"` as per-code levels (previously rejected as
  unrecognized), and `diagnostic_to_js` renders `"Info"`/`"Hint"` for a code
  configured that way — both proved by
  `set_lint_overrides_hint_relevels_e014_and_still_compiles` and
  `diagnostic_to_js_renders_info_and_hint_tiers`. `brink-lsp`'s
  `severity_to_lsp` maps `Info`/`Hint` to LSP's `INFORMATION`/`HINT`
  respectively (not collapsed onto `WARNING`), and
  `initializationOptions.lints` accepts the same two new strings.

- 1ef7797: Fixed (#1168): a user-defined function's `Option[T]`-shaped return no
  longer escapes strict inference as `Option[Unknown]` when the wrapped
  value is an annotated param / ascribed temp passed straight through with
  no other body evidence — `~ return some(x)` for an annotated `x`,
  `get(m, k)` on an annotated map param, and a `for` loop over an annotated
  `array<T>` param now infer their concrete element type instead of
  tripping a false `E065` (brink dialect only; observable through
  `@brink-lang/web`'s diagnostics since the checker's inference walk
  changed). Body-derived evidence that genuinely disagrees with an
  annotation (e.g. a param annotated `string` but only ever compared
  against an `int` literal) is unaffected — that case still infers from
  usage, unchanged. An unascribed temp merely copying an annotated param's
  value (`~ temp v = x`) does not inherit the annotation transitively — that
  boundary case still infers `Option[Unknown]`, unchanged by this fix.
- 319f9dc: Fix #1171: `len(s)` on a string faulted `NotIndexable("string")` at
  runtime — `collection_len` handled `Array`/`Map`/`Range` but had no
  `Value::String` arm. Added one returning the char count (Unicode scalar
  values via `str::chars().count()`), matching the char-count semantics
  `char_at`/`find` already use for string indexing elsewhere in the
  runtime, and the stdlib verb table's `len(… | string): int`. Compile-time
  inference was already correct; this closes the runtime gap.
- 257e7a9: #1177 (B0.8 Wave B, `docs/decision-log.md` 2026-07-23 "Code-ground
  sitting"): `brink-syntax-native`'s code-ground statement layer (B0.8 Wave
  A, #1294) gains `if`/`else`/`else if`, `while`, `for name in expr`, and
  `until <cond>;` control flow, plus HIR lowering for the whole statement
  layer (`let`/assignment/expression statements from Wave A, and the four
  new control-flow forms). `while`/`for`/`in`/`until` are new hard-reserved
  keywords.

  Lowers to the existing `~ { … }` T1b closed statement set
  (`IfStmt`/`WhileStmt`/`ForStmt`/`AwaitStmt`) — no new HIR nodes. `until` is
  native's sole condition-park spelling, lowering to the same `AwaitStmt`
  node the brink-dialect's `~ await cond` produces (`await` is not a
  keyword on the native surface at all).

  Reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file (`brink-db`'s `lowered_query`): a `var`/`const`
  initializer containing a statement-block with this new control flow
  (`var x = { if a { … } };`) now lowers its control-flow statements for
  real — diagnostics included — instead of the block's contents being
  silently unlowered. The block's own _value_ (blocks-as-values) still has
  no HIR representation, so the outer `STMT_BLOCK` still reports its own
  E129 the same way an unlowered `LAMBDA_EXPR` does — never a silent drop,
  never a panic. Wiring this statement layer into `flow`/`fn` declaration
  bodies (replacing the content-ground `BLOCK` those still use) is a later,
  separate slice (#1309).

- c852cbe: #1178 (B0.8b): adds `brink_ir::hir::emit_native` — a pretty-printer from
  lowered `HirFile` back to `.brink` native source, and the new dev-only
  `brink-respell` crate (`publish = false`, never shipped) that composes it
  with the existing ink frontend to mechanically respell ink corpus fixtures
  into `.brink`.

  Not wired into any compile/analysis path — `emit_native` is called only by
  `brink-respell`'s own tests, not by `brink-db`'s `lowered_query` or any
  other seam a `@brink-lang/web` session reaches. No behavior change for any
  existing `.ink` or `.brink` session; this is new, additive public API
  surface on `brink-ir` with no live caller in the shipped pipeline.

- cf076d5: #1179 (B0.9, Q6(b)): a native `.brink` accept-list admission gate —
  `brink_analyzer::validate_native_accept_list`, the inverse of the ink
  `dialect_gate` reject-list. Enumerates the HIR shapes a well-formed native
  lowering is allowed to produce and refuses anything else, loudly (a hard,
  non-suppressible diagnostic, never a silent drop), at the same seam B0.3's
  `validate_admission` runs at.

  Four checks, each a fresh reserved `DiagnosticCode` (`E133`-`E136`):
  `root_content` carrying anything other than empty or the synthesized `flow
main()` entry divert; any `IncludeSite` (native has no `INCLUDE` graph); a
  `ThreadStart` outside the two legal splice positions B0.7's choice-point
  lowering produces; a `ChoiceSet` carrying a non-neutral weave-fold value.

  Reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file (`brink-db`'s `lowered_query` → `lower_native_file`,
  keyed off the producing frontend at the pipeline level, never a tree tag):
  the gate now runs on every native lowering alongside B0.3's own admission
  validator. In practice a real `.brink` file's lowering never produces any
  of the four rejected shapes today — this is defense-in-depth, closing the
  gap for a future B0.x slice (or a bug) that might.

- 2f0b5cf: Analyzer: native-only lint for an asymmetric choice-branch dead-end
  (`E151`, issue #1219).

  A native `{? … }` choice branch that falls through (no `->`/`return`)
  while a sibling branch diverts onward, at a genuine dead end (nothing
  follows the choice point to reconverge into), now emits a `Warning`-
  severity `E151` diagnostic — the relocated residual value of ink's
  retired "ran out of content" runtime error (decision-log 2026-07-22,
  "Flows end implicitly (native)"). On by default (not opt-in): fires on
  every compile, like any other `Warning`-base-severity code. Never blocks
  compilation on its own. A choice set whose continuation is non-empty
  (the dissolved-gather reconvergence shape —
  `docs/native-surface-charter.md` §5) is never flagged, nor is a choice
  set where every branch shares the same tail shape (all divert, or all
  fall through). Re-levelable and suppressible through the project's
  `[lints]` table / `//brink-disable` like every other `Warning`-base-
  severity diagnostic code — including promotion to a hard error via
  `[lints] E151 = "deny"` or a project-wide `[lints] deny-warnings = true`.

- 4bae57f: Fix #1251: `brink-syntax-native`'s `expr::expression_bp` parsed every
  symmetric-precedence infix operator (`-`, `/`, `%`, `<`, `>`, `<=`, `>=`,
  `==`, `!=`, `&&`, `||`) right-associative instead of left-associative —
  the recursive call for an infix RHS reused the just-consumed operator's
  own precedence as the child's `min_bp`, so a second operator at the
  _same_ precedence was pulled into the RHS instead of being left for the
  parent's loop. `a - b - c` parsed as `a - (b - c)` instead of
  `(a - b) - c` (`10 - 3 - 2` = 9 instead of the correct 5). Fixed with the
  standard Pratt-parser recursion, `min_bp = prec + 1` (added as
  `Prec::next`, saturating at the highest level). Unobservable for `+`/`*`
  (mathematically associative); observable for every other operator on
  this list.
- 9b1d832: Native parser (family.rs): four grammar fixes (ruled 2026-07-22) — flat `else if` chains; same-line colon-form `else:` recognition; alternation markers (`!`/`~`/`&`/`|`) win over interpolation with a `{(!x)}` paren escape and a malformed-alternation diagnostic for `{|x| x}`; and empty alternations `{~}`/`{&}` now emit a diagnostic. Observable through the web editor's parse/diagnostics for `.brink` files.
- ec58199: Native parser: a bare `flags F =` (no members after `=`) is now a parse error, and `flags F = ()` is the explicit empty set (LIST parity, ruled 2026-07-22). Fixes the one silent zero-progress recovery path in the flags declaration. Observable through the web editor's diagnostics for `.brink` files.
- 9586408: Issue #1263 (ruled #1260): the native `.brink` parser now warns when `<-`
  appears outside a choice point instead of silently swallowing it as prose.
  Charter §11 narrows threads to scoped splices inside `{? … }` choice
  points, so a stray `<-` is almost always a misremembered ink thread — the
  new `E131` diagnostic flags it at warning severity (never blocking
  compilation, since `<-` can also be literal dialogue punctuation) and
  raises confidence in its message when the tokens after `<-` are shaped
  like a real knot/flow reference. Only affects native `.brink` sources;
  ink `.ink` sources and the oracle corpus are unaffected.
- b5fcf8e: `brink-syntax-native`: two content-family parser fixes (issue #1264).
  Whitespace separating two interpolations on a content line (`{a} {b}`) is
  no longer dropped — `content_items_until`'s significant-whitespace policy
  now folds the pending trivia into its own `TEXT` node before a genuine
  bare interpolation is parsed, so `"{a} {b}"` renders `"Alice Bob"` instead
  of `"AliceBob"`. A choice line's trailing `#tag`s (`* Choice #tag1` inside
  `{? }`) are now consumed by `choice()`'s own `tag_line_tail` call and
  produce a real `TAG` node, instead of falling through to the enclosing
  `choice_point` loop's `error_recover` and being wrapped in `ERROR` nodes.

  `brink-web` transitively depends on `brink-syntax-native` via both
  `brink-ir` and `brink-db` (non-optional): `brink-db::lowered_query`
  dispatches `.brink`-extension files to `brink_syntax_native::parse` (the
  #1106 seam), and `EditorSession::update_file` → `IdeSession::update_and_analyze`
  passes the path through with no extension gate. Both fixes are therefore
  wasm-observable for `.brink` files — most concretely the tag fix, which
  changes the editor's diagnostics for `* Choice #tag` inside `{? }` from
  ERROR-node parse errors to clean.

- c074d71: `brink-syntax-native`: a divert target's call-style args (`-> knot("x")`)
  are now captured on the `DIVERT_TARGET` node instead of silently orphaning
  into an unrelated sibling `CONTENT_LINE` with zero parse errors (issue
  #1265, bug #1196). Charter §11 keeps `-> knot(args)` verbatim from ink.
  `DivertTarget::call_args()` reads the captured `ARG_LIST` back; the
  existing `DivertTarget::path()` shape is unchanged (`ARG_LIST`, when
  present, is a direct sibling of `PATH`, not wrapped in a `CALL_EXPR`).

  Purely a native-surface parser fix — `brink-syntax-native` is off the ink
  compiler pipeline, so vanilla-ink stories and the oracle corpus are
  unaffected.

- 7239301: Fix (#1285): a `.brink` `use` line whose path starts with `::` (e.g.
  `use ::foo;`) no longer partially parses as a malformed `USE_DECL` with
  confusing errors, and no longer silently falls through as unremarked prose
  either. `at_use_decl`'s lookahead now only commits to `USE_DECL` when the
  token after `use` is an identifier; a leading `::` instead reports a
  targeted diagnostic ("a `use` path cannot start with `::`") before falling
  through, so the typo is surfaced instead of becoming player-facing text.
- 74b8586: `brink-ir`/`brink-db`: native `@[was("old::module::path")]` module-rename
  migration (issue #1286). A native `.brink` module's identity is derived
  purely from its filesystem path (`native_module_path`) and folded into every
  `DefinitionId`, so moving the file — or relocating the `brink.toml` root —
  changes every id and breaks player saves keyed on the old ids. A file-level
  `@[was("story::old::path")]` annotation declares the rename: `lower_native`
  now parses it into `HirFile.module.was`, which the already-wired read path
  (`brink-db::queries::module_map_query`) and alias-table codegen
  (`brink-analyzer::manifest`) turn into an `AliasEntry { old, new }` so a save
  carrying a pre-rename `DefinitionId` still resolves. Previously the
  annotation was silently dropped (`hir.module.was` was always `None`), so no
  migration was possible. A `@[was]` with no quoted old path is now diagnosed
  (`E132`, warning) rather than silently ignored.

  `brink-web` transitively depends on `brink-ir`/`brink-db` (non-optional) and
  `brink-db::lowered_query` dispatches `.brink`-extension files through the
  native frontend (the #1106 seam), so the new parse/lower is wasm-observable
  for native files — most concretely the disappearance of the spurious `E129`
  ("construct parses but has no HIR lowering yet") the editor previously
  reported on a top-level `@[was]` line, and the new `E132` for a malformed one.

- ff8794e: #1294 (B0.8 Wave A, `docs/decision-log.md` 2026-07-23 "Code-ground
  sitting"): `brink-syntax-native`'s parser gains a statement layer over
  its expression skeleton — `let name = expr;`, assignment `x = expr;` /
  `x.field = expr;` (RMW paths), bare expression statements `expr;`, and
  the `{ stmt; stmt; tail }` statement-block (an unterminated trailing
  expression is the block's tail — blocks-as-values ruled, CST shape only,
  no value lowering yet). A statement-block is reachable as an ordinary
  expression (`var x = { let y = 1; y };` now parses with zero errors,
  where it previously failed to parse the `{` at all).

  Parser only — no HIR lowering. A `.brink` file that uses this new
  syntax (reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file, `brink-db`'s `lowered_query`) now parses
  cleanly instead of surfacing a parse error, but its `STMT_BLOCK` still
  reports a lowering diagnostic (E129, "not yet lowered") the same way an
  unlowered `LAMBDA_EXPR` already does — never a silent drop, never a
  panic. Lowering these statements is a later wave.

- 5a7c18e: Native multi-file linking (issue #1296, decision-log 2026-07-23 "Native
  multi-file linking"): a multi-file native (`.brink`) project now links **every
  discovered module** into the one `StoryData`, not just the entry file.

  Native modules carry no `INCLUDE` edges, so the ink codegen closure — the
  entry file's transitive `INCLUDE` closure — reached only the entry file and
  every sibling `.brink` module silently missed codegen. Codegen now selects a
  native-aware closure (`compilation_closure_files`) that ranges over the whole
  discovered `.brink` module set: the discovery set is the compilation unit. The
  entry file still designates the start flow (compilation universe ≠ execution
  entry), and a `.brink` file that fails to compile is now an error even if no
  other module references it (Rust parity: the whole module tree compiles).

  Ink projects are unaffected — a project whose entry is an `.ink` file keeps the
  exact `INCLUDE`-transitive-closure behavior. The oracle corpus is unchanged.

- 2ccae0b: #1309 (B0.8 body-dialect seam, charter §4, `docs/decision-log.md`
  2026-07-23 "Native interleaving & body-dialect spelling"): a `flow`/`fn`
  declaration's body now honors the body-dialect selector on its opening
  brace. Plain `{ }` is the per-keyword default — **`fn` bodies now default
  to code-ground `STMT_BLOCK`, not prose-ground `BLOCK`** (the B0.8 Wave A
  seam this issue was tracking); `flow` bodies keep defaulting to
  prose-ground `BLOCK`. `~{ }` forces a code-ground body (charter §3's
  "Compound guard" — a code-bodied `flow`, now honestly spellable); `>{ }`
  forces a prose-ground body (a prose-bodied `fn`).

  A code-ground body lowers its statements through the existing B0.8
  `control_flow::lower_stmt_block` (`let`/assignment/expression statements,
  `if`/`while`/`for`/`until`, `return`/`break`/`continue`) and wraps the
  result as the container's sole `Stmt::LogicBlock` — the same shape a
  brink-dialect container whose entire body is one `~ { … }` block already
  produces. No new HIR nodes.

  Reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file (`brink-db`'s `lowered_query`): existing `.brink`
  sources with a `fn` body written in prose (content lines, choices,
  diverts) now need the `>{ }` override to keep parsing as prose — plain
  `{ }` on a `fn` parses as code-ground statements instead. `flow` bodies are
  unaffected unless authored with the new `~{ }` override.

  Line-escapes ("grains" — `~ stmt` inside a prose body, `> text` inside a
  code body) are NOT part of this slice — tracked as a follow-up, not yet
  parsed.

- 269fc6f: #1322 (B0.8 Wave B tail, `docs/decision-log.md` 2026-07-23 "Code-ground
  sitting"): `brink-syntax-native`'s code-ground statement layer gains
  `return e?;` (value return), `break;`/`continue;` (new hard-reserved
  keywords), and compound/RMW assignment (`x += e`, `x.field += e`), plus
  HIR lowering for all three. `return`'s valued form reuses the existing
  `RETURN_STMT` node (content-ground `return`/`return -> x` already used
  it); `break`/`continue` are brand-new node kinds with no content-ground
  counterpart.

  Lowers to the existing `~ { … }` T1b closed statement set
  (`BlockStmt::Return`/`Break`/`Continue`, `Assignment { op: AssignOp::Add |
Sub, .. }`) — no new HIR nodes. Compound assignment mirrors the
  brink-dialect's own `+=`/`-=` operator set exactly (`AssignOp` has no
  `Mul`/`Div` variant on either frontend).

  Reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file (`brink-db`'s `lowered_query`): a `var`/`const`
  initializer containing a statement-block using any of these forms (`var x
= { a += 1; return a; };`) now lowers them for real — diagnostics
  included — instead of the block falling into the loud, generic E129
  "unrecognized statement" arm. `#fn` function values remain unimplemented
  — see `brink_ir::hir::lower_native::expr`'s module doc for the honest gap.

- cb56346: Compiler: native `fixup_return_kind` now recurses into `Stmt::LogicBlock`
  (issue #1334).

  `fixup_return_kind` (`brink-ir`'s native HIR lowering) walks structural
  nesting to recompute `ReturnKind` after a body lowers, correcting every bare
  `return` inside a non-function `flow` to `ReturnKind::TunnelRedirect` (bare
  `return` there means ink's tunnel `->->`, not a function return). The
  `Stmt::LogicBlock` arm — a `~{ }` code-ground body, or an `if`/`while`/`for`
  nested inside one — was a no-op, so a bare `return` reached only through a
  logic block kept the always-`Explicit` stamp `lower_return_stmt` gives it at
  parse time, which would misfire `brink-analyzer`'s E032 ("return outside
  function") for perfectly valid tunnel-return code once code-ground bodies
  are reachable through this path.

  Fixed by adding a parallel recursion (`fixup_return_kind_in_block_stmts`)
  over `LogicBlock`'s closed `BlockStmt` set — `if`/`else if`/`else`,
  `while`, `for` — applying the same non-function-bare-return correction at
  every nesting depth. Off the ink pipeline; the oracle ratchet is
  unaffected (5577/1027/0 episodes, 350/14/390 cases — unchanged).

- 2df4377: #1335 (B0.8b): closes several `brink_ir::hir::emit_native` construct-coverage
  gaps discovered by re-checking the issue's gap list against current
  `main` and a full-corpus diagnostic sweep (`brink-respell`'s new
  `full_corpus_sweep.rs` test):

  - Two emitter completeness bugs, not native-grammar gaps — a choice body
    can absorb a same-line divert _and_ further statements (a leading
    `[Divert, EndOfLine, …]` shape only a two-element pattern was matching
    before), and an `else`/fallback choice with a bare `-> target` body (no
    display text at all) had no same-line-divert spelling; both now emit
    via the general braced-block form.
  - A bare `(name)` label immediately followed by a `{?}` choice point (a
    `Stmt::LabeledBlock` whose first statement is a `ChoiceSet`, not
    `Content`) now emits — the labeled-line dispatcher only recognized a
    `Content`-leading shape before.
  - `Import` (`use`/`import` declarations) is spelled back instead of
    refused outright — issues #1581/#1590 already fixed `Import.module` to
    be the real `::`-joined module name upstream of this emitter, so the
    blanket refusal predates that fix.
  - A newly-discovered silent-drop bug: `HirFile::allow_scopes`
    (`@[allow(…)]` suppression scopes, issue #1614/#1161) was never
    checked, so a file using it would round-trip with its suppression
    quietly gone. Now refused loudly instead.

  Not wired into any compile/analysis path — same posture as #1178's and
  #1335's first changeset: `emit_native` is called only by `brink-respell`'s
  own tests (dev-only, `publish = false`, never shipped). No behavior change
  for any existing `.ink` or `.brink` session; this only shrinks the
  emitter's own refused-construct set.

  The full-corpus sweep (~396 oracle cases) still cannot mechanically
  respell the whole corpus end to end: 187/396 now succeed (up from 177),
  with the remaining ~209 blocked overwhelmingly by missing **native
  grammar** (not emitter gaps) for prose-body code-ground statements
  (`~ x = expr`-style assignment/temp-decl/expression-statement/thread-start
  splices, a function body's `return` with a value, `else if` chains),
  alternations (grammar exists but the emitter itself never grew the arm —
  a real, separately-scoped follow-up), and `INCLUDE` files. See the PR
  body for the full breakdown and two additional findings (an `E033`
  dead-code true-positive and a root-content addressing mismatch) that are
  real but out of this slice's scope.

- 3b18503: #1335 (B0.8b): `brink_ir::hir::emit_native` now respells two constructs it
  previously refused — a labeled dissolved-gather continuation and a
  genuinely mid-flow labeled content line (`Stmt::LabeledBlock`) — using
  G-1's `(name)` content-line-label spelling (ruled 2026-07-20). Adds one
  native-only round-trip fixture (`tests/tier1-brink-respell/labeled-mid-flow-gather/`).

  Not wired into any compile/analysis path — `emit_native` is called only
  by `brink-respell`'s own tests (dev-only, `publish = false`, never
  shipped), the same posture #1178's changeset already recorded. No
  behavior change for any existing `.ink` or `.brink` session; this only
  shrinks the emitter's own refused-construct set.

- 0dcdd10: #1342 (B0.9 close): the native strict-only enforcement point —
  `brink_analyzer::native_strict_only_error`, a fresh `DiagnosticCode::E137`.
  A native `.brink` file compiled with an explicit `types = gradual` knob is
  now a hard error: gradual typing does not exist on the native surface
  (decision-log 2026-07-19 "Typing posture ruled"), and this closes the gap
  PR #1341 (issue #1179) left open — that slice delivered only the HIR-shape
  accept-list (`E133`-`E136`), not "the strict-only ruling's enforcement
  point" docs/b0-sequencing.md §B0.9 also discharges.

  Wired at `brink-db`'s per-file diagnostics seam (`per_file_diagnostics_query`),
  which already has both a file's `Language` classification and
  `AnalysisOptions` access — `lower_native_file` cannot host this check (issue
  #1179's finding: no `db`/`AnalysisOptions` there). Keyed on the _explicit_
  `types` field, not the dialect-defaulted resolution, so a bare `.brink`
  compile with no `types` config is unaffected; only an explicit
  `types = gradual` (a CLI flag, a `brink.toml` entry, or a programmatic
  `AnalysisOptions`) reaching a native file trips it.

  Reachable through any `@brink-lang/web` session that compiles a
  `.brink`-extensioned file with an explicit gradual `types` policy.

- 51d243b: #1348: the T1b ink dialect gate (`dialect_gate::check`, `E051`; and
  `strict::config_error`, `E064`) no longer fires against native `.brink`
  source. `dialect` is an ink-only axis (docs/t1b-surface-spec.md §1),
  orthogonal to native's `Language` classification — a native project has no
  "dialect" to be strict-ink about, so a native compile with `types = strict`
  used to require a spurious, unrelated `dialect = brink` just to dodge `E064`
  (surfaced by PR #1346's own strict-positive native test), and any native
  construct the gate recognizes (`STRUCT` declarations, postfix indexing,
  sigil literals, …) — all ordinary native syntax — could spuriously trip
  `E051` under `dialect`'s `StrictInk` default.

  `brink_analyzer::per_file_diagnostics` and `strict_diagnostics` both gained
  an `is_native` flag; `brink-db`'s `per_file_diagnostics_query` and
  `whole_project_diagnostics_query` compute it from the file's/project's
  `Language` classification and skip the two ink-only checks accordingly. Ink
  dialect gating is unaffected — `is_native = false` is byte-identical to
  before this flag existed.

  Reachable through any `@brink-lang/web` session that compiles a
  `.brink`-extensioned entry: `EditorSession::compile_project` → `IdeSession::compile`
  → the same salsa `story_data()` seam `per_file_diagnostics_query` /
  `whole_project_diagnostics_query` sit behind.

- 137c169: #1349 (companion to the closed #1286): `brink-syntax-native`'s
  annotation-arg grammar (`@[name(args)]`) gains an unquoted `::`-separated
  module-path arg production — `@[was(story::old::path)]` now parses to a
  `PATH` node (reusing `expr::path`'s existing `PATH`/`PATH_SEGMENT`
  shape, exposed via `AnnotationArg::path`) instead of failing with
  "unexpected token in annotation arguments". A single-segment path (no
  `::`) is unaffected and still parses as the existing bare-ident arg.

  Reachable through any `@brink-lang/web` session that parses a
  `.brink`-extensioned file containing an `@[…]` annotation line whose
  first arg is an unquoted `::`-path — the diagnostics for that specific
  shape change (parse error → clean parse). `lower_native::module`'s
  `@[was(...)]` lowering still only consumes the quoted-string arg form
  (`hir.module.was` is unaffected); wiring the new unquoted-path shape into
  that lowering pass is a follow-up, not done here.

- e839fa9: #1355 (follow-up to #1349/#1286): `lower_native::module::was_old_path`
  (`crates/internal/brink-ir/src/hir/lower_native/module.rs`) now accepts
  **both** spellings of the `@[was(...)]` rename-migration arg — the original
  quoted string (`@[was("old::path")]`) and the unquoted `::`-path form
  `@[was(old::path)]` whose grammar #1349 shipped. Previously the unquoted
  form parsed cleanly but was diagnosed `E132` ("malformed migration
  directive") here, so the native module-rename migration (#1286) was not
  usable end-to-end with the unquoted spelling despite both PRs having
  landed.

  Reachable through any `@brink-lang/web` session that lowers a
  `.brink`-extensioned file whose `@[was(...)]` arg is unquoted: the
  diagnostics change (E132 → none) and `hir.module.was` — and therefore the
  `brink-analyzer` alias table that maps a pre-rename `DefinitionId` to its
  current one — are populated exactly as they already were for the quoted
  form.

- 529bc3f: Fix (#1358): editor analysis now judges native `.brink` source by the native
  rule set, so two native modules can declare a same-named flow without one
  disappearing from the editor's symbol index.

  The editor session analyzes off the project database, through an analyzer
  entry point that has no file paths and so cannot tell native source from ink
  — the session has to declare it, and it never did. Everything downstream of
  that ran the ink arm over native files. The consequence a wasm consumer can
  observe: a native project's module is its path and is always declared, so
  two modules declaring the same flow name were treated as a duplicate
  definition, the later one was dropped from the index with a
  `duplicate knot definition` warning, and every feature keyed off that index
  — hover, go-to-definition, completion, the story graph — missed it. Both now
  coexist, each resolving through the module its file imported.

  The same declaration also selects the native arm of the diagnostic passes,
  which is what the language server publishes as inline squiggles: a `.brink`
  file no longer reports its ordinary syntax (struct declarations,
  construction literals, type annotations, multi-line logic blocks) as
  `E051` "brink extension" errors, no longer reports `E064` when a project
  dials `types = strict` (the only policy native has), and now does report
  `E137` when a project explicitly dials `types = gradual`, which native
  source cannot compile under.

- 72b978c: #1361: `compile()`/`compile_fragment()` (`crates/brink-web/src/compile.rs`)
  now build an in-memory `SourceTree` from the caller-supplied document(s) and
  run the #1306 producer — `Project::load(&tree, entry, &overrides)` →
  `compile(&env)` — instead of driving a throwaway `Driver` through
  `brink_compiler::compile(entry, read_file_closure)`. Two observable
  differences:

  - `compile_fragment()` now honors a `brink.toml` if one happens to be
    present among the served `sources_json` (previously ignored — the old
    path used `brink_compiler::compile`'s hardcoded default
    `AnalysisOptions` with no config discovery at all). `compile()` cannot
    observe this: its `InMemory` tree only ever contains the single
    `"main.ink"` key, so `Project::load` never discovers a `brink.toml` for
    it. No existing caller passes a `brink.toml` through `compile_fragment()`
    today, so this is a nil delta for current callers of `@brink-lang/web`.
  - `compile(source)` previously served `source` verbatim for _every_
    requested path (`|_path| Ok(source.to_owned())`), so an `INCLUDE foo.ink`
    in a single-source playground compile always resolved (against `source`
    itself). The new single-key `InMemory` tree only serves `"main.ink"`, so
    the same `INCLUDE` is now a hard `brink_environment::LoadError` — the
    result is `ok: false` with a populated `error` string and an **empty**
    `warnings` array, not diagnostics. No existing caller feeds `INCLUDE` into
    `compile()` today, so this is also a nil delta for current callers, but
    it is a result-_shape_ change (error string vs. diagnostics) for any
    future caller that does.

  `EditorSession::compile_project` (brink-ide's `IdeSession::compile`, the
  live-editing salsa db shared with brink-lsp) is untouched — out of scope for
  this issue.

- 741ac65: #1366: `EditorSession::apply_project_config` (the wasm editor session's
  `brink.toml` entry point) now merges the file's `[lints]` table / `deny-warnings`
  flag onto the session's resolved lint policy via
  `AnalysisOptions::apply_project_config` — the same merge point
  `brink-driver`/`brink-cli`/`brink-lsp` already use (#1160) — and
  `IdeSession` carries the resolved policy through
  `set_lint_policy`/`analysis_options`/`snapshot` instead of a hardcoded
  no-op `LintPolicy::default()`. (`brink-web`'s `EditorSession` is the only
  caller of `set_lint_policy` as of this PR; `brink-cli`'s IDE surface does
  not yet forward a resolved policy into its own `IdeSession`.)

  This delta is bigger than a re-rendered `severity` string: `IdeSession::compile`
  feeds the resolved lints into the same closure-diagnostics partitioning
  `brink compile` uses, so a `[lints]` entry that promotes a diagnostic to
  `Error` — a per-code `"deny"` override or `deny-warnings = true` — now
  makes `EditorSession::compile_project` return `ok: false` with
  `story_bytes: null` for a file that previously compiled successfully with
  only a warning. Callers that apply a `brink.toml` with `[lints]` should
  expect this compile-failure outcome, not just a relabeled diagnostic.
  Unknown or non-overridable lint codes surface through the same warnings
  channel `apply_project_config` already uses for unrecognized `[project]`
  keys. Absent `[lints]` (the default, unchanged case) is byte-identical to
  prior behavior.

- 5680e1e: #1367 (follow-up to #1160/#1366): the four diagnostic **display** sites that
  still read the raw `DiagnosticCode::severity()` default now render
  `brink_analyzer::effective_severity` instead — `brink-web`'s
  `diagnostic_to_js` (used by `compile`/`compile_fragment`/`EditorSession::
compile_project`), `brink-lsp`'s `diagnostic_to_lsp` (every publish site,
  including a new `LanguageOptions.lints` field so a discovered `brink.toml`'s
  `[lints]` table — previously resolved but never stored — actually reaches
  the published severity), and `brink-ide`'s `structural_result::
introduced_diagnostics` (the safe-by-default breakage report `brink ide`'s
  rename/move/delete commands and the wasm editor's `*_safe`/`gate` calls
  both surface).

  Reachable through `@brink-lang/web`: `EditorSession::compile_project`'s
  `warnings[].severity` now promotes `E063` (annotation-vs-inference
  mismatch) to `"Error"` under `types = strict`, matching the build-gating
  severity `brink-db`'s partitioning already used — previously it always
  showed `"Warning"` regardless of policy
  (`compile_project_severity_reflects_strict_types_e063_promotion`).
  `IdeSession`/`EditorSession` still have no `[lints]`-resolution input wired
  (the #1160 changeset's tracked gap), so a `[lints]` override itself has no
  observable effect through `@brink-lang/web` yet — only the `types`-driven
  `E063` carve-out does. The plain `compile`/`compile_fragment` wasm entry
  points always use `AnalysisOptions::default()` (no policy ever configured),
  so their output is unchanged.

- b6fdef9: #1384: `brink-project-config`'s `ConfigError` (malformed TOML syntax, or a
  recognized `brink.toml` key holding an out-of-range value) now carries the
  file's path on every variant (`Toml`/`NotATable`/`WrongType`/`InvalidValue`
  join the existing `Io`), and a byte span where the `toml` crate provides
  one (`Toml`, i.e. malformed syntax — `ConfigError::span()`). Continues
  #1369, which threaded the discovered path into `LoadError::Config`/
  `ConfigRead` but left `ConfigError` itself pathless.

  **Observable through `@brink-lang/web`:** `EditorSession::discover_project_config`
  now resolves `brink.toml` through the new path-carrying `parse_str_at`
  (rather than the pathless `parse_str`), so a rejected `Result`'s error
  message text changes shape slightly — it now comes from `ConfigError`'s own
  `Display` (which names the file, and for malformed syntax, its line/column)
  rather than the hand-rolled `"invalid brink.toml at {config_key}: {e}"`
  wrapper this function used to build. Still always a rejected `Result`, never
  a panic, for the same malformed-`brink.toml` inputs as before.
  `EditorSession::apply_project_config` (the pathless entry point — an
  embedder pushing raw TOML text it read through its own host API, with no
  discovered location to give) is unchanged: it still calls the original
  pathless `parse_str`, which now falls back to the bare `brink.toml` label
  rather than an unlabeled error.

- 916837b: #1397: `AnalysisOptions::apply_project_config`'s `[lints]` handling now
  **replaces** the resolved lint policy (per-code overrides plus
  `deny-warnings`) with whatever `config` currently resolves, instead of
  merging `config`'s entries key-by-key into whatever was already resolved.
  A code (or `deny-warnings`) present in an earlier call but omitted from the
  current one now reverts to its base severity, instead of staying stuck.

  This is observable through `@brink-lang/web`: `EditorSession`
  (`apply_project_config`/`discover_project_config`) is a long-lived session
  that re-applies `brink.toml` on every change (#1366) — previously, deleting
  a `[lints]` entry (or the whole table) from `brink.toml` left the
  previously-applied override permanently stuck on that session, since
  nothing ever removed it. It now reverts correctly on the next apply.
  `brink-cli`, `brink-lsp`, and `brink-environment`/`bevy-brink` build a
  fresh `AnalysisOptions` on every call to this function already, so this is
  a no-op behavior change for them.

- d27382f: #1407: `brink.toml` gains a `[project] unprune-dirs` key — an explicit
  escape hatch for a project that legitimately keeps native `.brink` sources
  under a directory name discovery otherwise prunes by default (`target`,
  `.git`, `node_modules`; see `brink_source_tree::IGNORED_DIR_NAMES`). Also
  adds a diagnostic naming any pruned directory that plausibly held a wanted
  source file, and documents (rather than leaves ambiguous) a deliberate
  decision **not** to add `.gitignore`-awareness, since discovery is a
  deterministic-compilation input (#1306) and `.gitignore` resolution is not
  fully determined by tracked repository content alone.

  **Reachable through `@brink-lang/web`, traced:** `EditorSession::
apply_project_config` and `EditorSession::discover_project_config` both
  call `brink_project_config::parse_str`/`parse_str_at` directly and return
  every `ConfigWarning` to the JS caller as a JSON string array. Before this
  change, a served `brink.toml` setting `[project] unprune-dirs = [...]`
  produced an "unknown key `project.unprune-dirs`" warning in that array; now
  the key is recognized (no warning for a real `target`/`.git`/`node_modules`
  entry, or a differently-worded "not a pruned directory name, no effect"
  warning for anything else — likely a typo). The escape hatch's actual
  _functional_ effect (widening what a native discovery walk descends into)
  is **not** reachable through `@brink-lang/web`: `RealFs`/`Walk` are host-only
  and never constructed on a wasm-reachable path (`brink-web`'s `compile`/
  `compile_fragment` build `brink_source_tree::InMemory` directly, whose
  `list()` is unaffected by `unprune-dirs`). Only the config-parsing/warning
  text changes.

- cd70ad8: #1414: `EditorSessionHandle` gains `discoverProjectConfig(entry)`, the
  web-mount counterpart of `brink compile`/`brink ide`'s `brink.toml`
  discovery. Previously the wasm editor session had no discovery of its own —
  `applyProjectConfig` only applied text an embedder had already located and
  read through its own host filesystem API (Node `fs`, the File System Access
  API, …), unlike every other mount, which resolves `brink.toml` by walking a
  `SourceTree` (`brink_project_config::discover_from_entry_in_tree`).
  `discoverProjectConfig` closes that gap for brink-web specifically: it
  walks the session's own in-memory document tree (whatever `updateFile` has
  loaded) up from `entry`'s directory, exactly like `brink compile`/
  `brink ide` walk a real filesystem — no host-specific directory-walk code
  required. Serve `brink.toml` as an ordinary document
  (`updateFile("brink.toml", text)`) and call `discoverProjectConfig(entry)`
  once; it applies `[project] dialect`/`types` and `[lints]`/`deny-warnings`
  exactly like `applyProjectConfig` (same explicit-call precedence, same
  warnings-array/re-analyze contract), and returns `[]` when no `brink.toml`
  is found anywhere in the tree. `applyProjectConfig` is unchanged and stays
  available for embedders that prefer handing text in directly.
- 8531452: #1417: extends the `[lints]`/`deny-warnings` CLI/API override tier
  (#1373's `brink compile` `--deny`/`--warn`/`--allow`/`-D warnings`,
  #1394's `BrinkPlugin::with_config`) to `brink ide`, `brink-lsp`, and the
  wasm `EditorSession` — the three surfaces #1417 named as still honoring
  only a discovered `brink.toml`, so a project that denies a warning saw it
  demoted back to a warning in the editor/LSP even though a real
  `brink compile` of the same project would fail.

  - `brink ide` gains repeatable `--deny`/`--warn`/`--allow <CODE>` and
    `-D warnings`, mirroring `brink compile` exactly (shared resolution via
    the new `brink-cli::lint_overrides` module) — every subcommand that
    loads a project (`def`, `check`, `rename`, `effects-diff`, …) now
    honors them, always winning over the same code in a discovered
    `brink.toml`.
  - `brink-lsp` gains `initializationOptions.lints` (an object,
    `{ "<CODE>": "deny" | "warn" | "allow" }`) and
    `initializationOptions.denyWarnings`, applied last in
    `resolve_language_options` — the same `CLI/API > file > default`
    precedence `dialect`/`types` already had.
  - **`EditorSession` (wasm-observable)**: two new methods,
    `set_lint_overrides(json)` (replace the explicit per-code override map;
    `"{}"` clears it) and `set_deny_warnings_override(bool)` /
    `clear_deny_warnings_override()`. Always win over an applied
    `brink.toml`'s `[lints]` table, in either call order — the file tier
    reapplies the explicit overrides on every reload rather than clobbering
    them. `compile_project` now reflects the resolved policy exactly as
    `brink compile`/`brink ide`/`brink-lsp` would.

  Absent any override (the pre-#1417 default in all three surfaces) is
  byte-identical to prior behavior.

- cbc6683: Removed `LineFlags::STARTS_WITH_WS`/`ENDS_WITH_WS` (`brink-format`). A grep
  audit found zero production consumers: `STARTS_WITH_WS` had none at all, and
  the only `ENDS_WITH_WS` reader (`OutputBuffer::ends_in_whitespace`) was
  `#[cfg(test)]`-only. Live whitespace-only/empty suppression in
  `brink-runtime` uses `ALL_WS`/`EMPTY` exclusively, which this does not
  touch.

  Traced against the C# reference runtime before removing: its output-stream
  whitespace handling (`PushToOutputStreamIndividual`, `TrimNewlinesFromOutputStream`,
  `TrimWhitespaceFromFunctionEnd`) always operates on whole tokens
  (`isNewline`/`isNonWhitespace`/`isInlineWhitespace`), never on whether a
  mixed-content token merely starts or ends with whitespace. There is no
  sub-token leading/trailing whitespace concept in ink's reference semantics,
  so these flags encoded a distinction the runtime never needed — this is a
  dead-code removal, not a conformance gap.

  `LineFlags` is derived at `.inkb` decode time, not stored on the wire there,
  so this has no `.inkb` format-version impact. `.brkt` (the transcript save
  format) does persist `LineFlags` bits (`transcript.rs`'s `encode_part`/
  `decode_part`), so the bit values for the surviving `ALL_WS`/`EMPTY` flags
  are left unchanged from their prior positions to avoid reinterpreting
  existing `.brkt` files. No observable rendering effect, since neither
  removed flag had a live consumer.

- d7994d5: Fix #1448: a story whose **root weave** ran out of content faulted with
  "ran out of content. Do you need a '-> DONE' or '-> END'?" instead of
  ending its turn. inklecate appends an implicit level-1 gather plus
  `-> DONE` to the root weave (`FlowBase.cs:69-72`); brink only had the
  root container's own trailing `Done`, which a gather can never reach —
  a gather is entered by `goto`, which clears the container stack.

  LIR lowering now synthesizes that terminus (a `g-final` gather holding a
  single `-> DONE`) and diverts the root weave's outermost loose end into
  it. Root scope only: a knot, stitch, tunnel, or function that runs out of
  content is a genuine authoring error and keeps reporting one, matching
  C# ink.

  Playground/editor stories written without a trailing `-> DONE`/`-> END`
  after a root-level weave now end cleanly. Oracle conformance: 5,577 →
  5,598 passing episodes, 350 → 358 passing cases.

- 867e75c: B1 (#1460): the `or`-coalescing surface spelling lands on the native
  `.brink` dialect — `x or default`, per the ruled typing
  (`docs/stdlib-spec.md` §1.6a, `docs/decision-log.md` "Option[T] ruled"
  2026-07-18): `(Option[T], T) -> T` unwraps `some(v)` and falls back to
  `default` on `none`; `(Option[T], Option[T]) -> Option[T]` preserves
  optionality so a chain (`a or b or default`) associates left. New keyword
  `or` in the native lexer/parser (`InfixOp::Coalesce`, distinct from
  `InfixOp::Or` — ink's boolean `||`, oracle-frozen and untouched), one new
  opcode `Coalesce` at `0xFB`. The web package's disassembly view
  (`program_model.rs`) and `.inkt` text format (read + write) both gain the
  `coalesce` mnemonic. Vanilla-ink and brink-dialect stories are
  byte-identical; the oracle corpus is unaffected — the new opcode is
  reachable only through native lowering.

  The condition-position `as`-binding (`if EXPR as NAME`) named alongside
  `or`-coalescing in issue #1460 is **not** included in this patch — its
  precise grammar is unruled beyond a usage sketch in a DRAFT sequencing
  document (`docs/stdlib-sequencing.md`, Finding F16, never promoted to a
  decision-log ruling), so it is deferred per house rule 7 pending a design
  round.

  Review follow-up: a statically-detectable coalescing mismatch (a
  non-Option left-hand side, or a fallback type that disagrees with the
  Option's element type — `{5 or 9}`, `{some(1) or "text"}`) now raises
  `E066` at the coalescing expression's own site under `types = strict`,
  instead of silently collapsing to an unreported `Conflicted` type. The
  mnemonic/opcode assignment and the typing/runtime semantics (including
  eager evaluation of both operands — no short-circuiting) are unchanged
  from the original patch; only diagnostic coverage improved.

- faf45f2: #1461 (B2, `docs/b0-sequencing.md`/`docs/stdlib-spec.md` §5/§9 F10 ruling):
  `brink-syntax-native`'s `for` grammar gains an optional second binding —
  `for key, val in expr { … }`, two-binding map iteration — landing the one
  additive HIR field the B0 fence reserved (`ForStmt.val_name`,
  docs/b0-sequencing.md:356). Lowers to the F10-ruled desugar: key iteration
  plus `let val = container[key]`, no pair shape ever materializes. The
  existing single-binding form (`for name in expr`) is unaffected — same
  LIR shape as before, byte-for-byte.

  Reachable through any `@brink-lang/web` session that analyzes a
  `.brink`-extensioned file (`brink-db`'s `lowered_query` → `lower_native`,
  the same seam #1177's control-flow lowering used): `for k, v in m` now
  parses and lowers instead of erroring on the comma, and the analyzer
  binds `v`'s type from the map's value type when the iterable is a `[K:
V]`. The ink/brink-dialect `~ { for … }` grammar is untouched — it has no
  two-binding syntax and never sets `val_name`.

- b8e3246: Issue #1462 (D5): UFCS method-call syntax now **auto-refs** its receiver when
  the resolved free function's first parameter is declared `ref`. A `.brink`
  source that writes `gold.bump(1)` against `fn bump(ref n, amount)` compiles
  and runs the mutation for real — internally the desugar spells the reference
  as `bump(ref gold, 1)` (desugar notation; the native surface has no
  call-site `ref` keyword, so the spellable equivalent is the unmarked
  `bump(gold, 1)`, and a dotted receiver becomes an explicit T1e projection,
  `party.leader.heal(5)` → `heal(ref party.leader, 5)`) and rides the existing
  ref-argument/projection lowering, so a `ref` parameter's write lands in the
  receiver's own cell instead of a copy.

  A non-`ref` first parameter is unchanged: plain by-value desugar, with no
  lvalue requirement on the receiver. A receiver that cannot be written
  through is refused with `E143` ("cannot mutate …") instead of being silently
  desugared by value — a `CONST` receiver, or a projection rooted in a
  frame-local (T1e's durable-root rule).

  Web-observable through `compileProject`: a native `.brink` entry calling a
  `ref`-first-parameter function through method syntax previously always
  refused to compile (`E143`, "not supported yet"); it now compiles, and its
  `StoryData` performs the mutation. `E143`'s message and title change with
  it — the code now names the ruled refusal, not the missing feature. `.ink`
  compiles are unaffected (ink's own lowering cannot produce the multi-segment
  callee path this keys on).

- 80ede86: B4 display-boundary None-render (#1463, `docs/stdlib-spec.md` §1.6b):
  an interpolation whose **final** value is `Option::None` now renders as
  nothing instead of the interim total `"none"` (F28) — absence renders as
  absence, the honest narrative meaning. This is cut by _position_, not by
  type or dialect: nested compositions are never forgiven (`Option[T] ≠ T`
  strictness holds everywhere else), and `string(none)` keeps rendering the
  total `"none"` forever, unaffected. The forgiveness never loses
  information — the append-only output transcript still records the raw
  `Option::None` value, so a forgiven render is always traceable by
  inspecting `OutputBuffer::transcript()`. Vanilla-ink stories are
  byte-identical (`Option[T]` is a brink-dialect-only extension surface,
  never reachable from ink-dialect source); the oracle corpus is
  unaffected.
- db2a6fa: #1464 (B5 — the build of #1103, RULED 2026-07-23; `docs/stdlib-spec.md`
  §9.6): the native surface gains the **one construction initializer**
  `TypeName { … }`, and its meaning is **protocol dispatch**, not closed
  compiler grammar.

  `brink-syntax-native` produces one node shape — `CONSTRUCT_LITERAL` with
  `CONSTRUCT_ENTRY` children covering the ruled element form (`Flags { Red,
Blue }`) and pair/field form (`Map { "a": 1 }`, `Point { x: 1 }`) — with a
  Rust-style no-construct-literal restriction in `if`/`while`/`for` and
  content-ground `{if …}`/`{match …}` heads so a head's brace still opens its
  body (`(…)` lifts it again). Meaning comes from the new `construct`
  registry, `brink_ir::hir::construct::ConstructTarget`: a closed enum (the
  NS-A8 protocol-fence shape), **std-only this round** — `Map` →
  `Expr::MapLiteral`, `Flags` → `Expr::ListLiteral`, `Weighted` → the
  existing total `weighted(…)` intrinsic — with an unregistered name falling
  through to the declared-struct reading. User-type opt-in (the `impl`
  spelling), the validating `construct → Option` member's spelling, and the
  spread form (`Map { ..other }`) stay deferred with the ruling; none is
  stubbed.

  Two new diagnostics: **E138**, a duplicate key in a map literal
  (#1103's cascade ruling (A) — a compile error, not a silent last-wins
  overwrite), and **E139**, entries in the wrong form for their target
  type.

  Web-observable on two paths. (1) A `.brink` entry compiled through
  `compile_project`/`compile_fragment` reaches the native pipeline
  (`brink_compiler::compile` dispatches on the entry's extension), so
  construction literals now compile and play instead of failing to parse.
  (2) **E138 also fires for the brink dialect's own `#{…}` spelling** — both
  surfaces lower to the same `MapLiteral`, so any `dialect = brink` source
  with `#{k: 1, k: 1}` in it now fails to compile where it previously
  last-won silently.

- f285bec: Fix (#1471): `x or default` (B1 `or`-coalescing, #1460) now **short-circuits**
  — `default` is evaluated only when `x` is `none`, matching the C# `??`/Kotlin
  `?:` conventions the operator's precedence placement was modeled on. The
  version PR #1469 shipped evaluated both operands unconditionally (flagged
  there as an unruled implementation decision); the maintainer has now ruled
  short-circuiting is required, so `x or expensive()` runs `expensive()` exactly
  once, and only on `none`.

  The binary `Coalesce` opcode (`0xFB`) is retired — a binary opcode can't
  short-circuit, since both operands would already be evaluated onto the stack
  before it runs. `InfixOp::Coalesce` lowers to a real branch instead, backed by
  a new opcode reusing the same byte: `CoalesceSome(rel)` pops the left-hand
  `Option`, and on `some(v)` pushes the unwrapped `v` and jumps past the
  right-hand operand's bytecode entirely; on `none` it falls through to evaluate
  the right-hand operand as before. The collapse-vs-two-Option typing decision
  (`(Option[T],T)->T` vs `(Option[T],Option[T])->Option[U]`) can no longer be
  read off the right-hand operand's runtime value, so lowering consumes the
  analyzer's recorded per-step types (#1492) instead and re-wraps with a
  `MakeSome` after the branch only when the recorded verdict says the step keeps
  its `Option`. When the left-hand type cannot be statically pinned the runtime
  check remains the semantics: an `Option` coalesces, a plain value faults.

  The web package's disassembly view (`program_model.rs`) and `.inkt` text
  format (read + write) drop the `coalesce` mnemonic for `coalesce_some <rel>`.
  Native-surface-only, so vanilla-ink and brink-dialect stories (and the oracle
  corpus) are unaffected — the opcode is reachable only through native
  `or`-coalescing lowering.

- 7545fdf: B1b (#1475): the `as` binding lands on the native `.brink` surface — one
  construct in both of the language's condition positions, per the ruling in
  `docs/decision-log.md` 2026-07-26 ("The `as` binding: one construct, both
  condition positions, `{if}` spelling"):

  - **Statements:** `if EXPR as NAME { … }`, `while EXPR as NAME { … }`
    (the `while` form rebinds on every iteration).
  - **Templates:** `{if EXPR as NAME: … else: …}`, riding the already-ruled
    `{if}` spelling rather than a second binding grammar.

  The binding is immutable, typed `T` from the condition's `Option[T]`, and
  scoped strictly to the success arm — an `else`/`else if` arm never sees it.
  For v1 the binding must be the **entire** condition; composing it with
  `&&`/`||` is an error (let-chains can land later, additively).

  One new opcode, `OptionBind(slot)` at `0xFC`: it pops an `Option`, writes
  the unwrapped payload into the binding's temp slot on `some`, and pushes
  the bool the construct branches on. The web package's disassembly view
  (`program_model.rs`) and the `.inkt` text format (read + write) both gain
  the `option_bind` mnemonic — this is the web-observable surface of the
  change. Vanilla-ink and brink-dialect stories are byte-identical and the
  oracle corpus is unaffected: the new opcode, node kind and HIR fields are
  reachable only through native `.brink` lowering.

  New diagnostics: `E145` (an `as` over a `&&`/`||` composition), `E146` (an
  `as` in a choice guard — ruled, but sequenced with the `.inkb` v6 Choice
  record, so it is diagnosed as _not yet supported_ rather than half-
  lowered), `E147` (an `as` over a statically known non-`Option` condition),
  `E148` (a write to a binding). The runtime gains a matching
  `AsBindingNotOption` fault as `E147`'s gradual-mode residual.

  F27's `E116` ("an `Option[T]` has no truthiness") no longer fires on a
  condition that carries an `as` binding — the binding is the third explicit
  spelling that ruling named, alongside `== none` and `== some(x)`.

- 6262d13: B3a (#1482): UFCS resolution — `recv.name(args)` on the native `.brink`
  surface is now resolved by a type-directed analyzer pass instead of failing
  as an unresolved reference. A field on the receiver's type wins outright
  (hard error `E140` when that field is not callable, never a silent
  fall-through), otherwise the call desugars onto a free function in ordinary
  lexical scope; neither is one diagnostic naming both attempts (`E141`), an
  unknown receiver type demands an annotation (`E142`), and a `ref` first
  parameter was refused here (`E143`); auto-ref lands separately in #1462, in
  this same release. A resolved call is refused at lowering (`E144`) until the
  verdict side table has a codegen consumer.

  Web-observable through `compileProject`'s diagnostics: a `.brink` entry with
  method-call syntax previously reported `E025` ("unresolved variable
  reference") at every such site and now reports the specific ruled code, so
  consumers filtering or grouping on `Diagnostic.code` see the new values.
  Compiling `.ink` sources is completely unaffected — ink's own lowering
  cannot produce the multi-segment callee path this pass keys on.

- ef2973c: Issue #1484: the stdlib `remove` verb's accidental two-posture divergence
  (array index removal faults out of bounds; map key removal is
  idempotent-total) is fixed by renaming, not flattening. Seq index removal
  is now `remove_at(a, i)`, joining the `_at` faulting-index family with
  `char_at`; `remove` now uniformly names identity-based, idempotent-total
  removal (map keys today; flags values once flags land). No deprecation
  shim — `remove` no longer accepts an array (`NotIndexable`), and
  `remove_at` no longer accepts a map. Bytecode gains one opcode
  (`SeqRemoveAt`, `0xFD`) for the split primitive; wasm-observable via
  `@brink-lang/web`'s bytecode disassembly view and any compiled program
  using either verb.
- fd10f7a: #1487/#1488/#1489 (NG-A/NG-B/NG-C, RULED 2026-07-26 — `docs/decision-log.md`
  "NG-C ruled: `: type` returns everywhere"): the native `.brink` surface
  gains its type-annotation grammar, in **one spelling for every position**.

  `brink-syntax-native` grows a `type_expr` production (`TYPE_ANNOTATION` /
  `TYPE_EXPR` / `TYPE_NAME` / `TYPE_GENERIC` / `TYPE_FN`, structurally the
  brink dialect's own TM-2 shape) and wires `(: type)?` into parameters
  (`fn probability(g: Guest)` — shared by `fn`/`flow`/`extern`), bindings
  (`let x: int = 1;`, `var hp: int = 10`, `const MAX: int = 100`), the
  `fn`/`flow` return clause after the parameter list
  (`fn probability(g: Guest): float { … }`), and lambdas
  (`|g: Guest|: bool { … }`, grammar-only — lambda lowering is still fenced).

  `brink-ir::hir::lower_native` populates the HIR slots that already existed
  for the ink dialect: `Param.annotation`, `TempDecl`/`VarDecl`/`ConstDecl`
  `.annotation`, and `Knot.return_type`. Because these are the _same_
  `hir::TypeExpr` values, `brink-analyzer`'s strict-mode annotation firewall
  now reaches native source with no analyzer change: an annotated parameter
  or binding is exempt from `E065` Unknown-escape, which was previously
  unreachable from a `.brink` file (native is strict-only, #1342). Declaring
  a return type is also the ruled coroutine-vs-state toggle, so a
  value-returning `flow` no longer picks up the implicit `-> DONE` on
  fall-through.

  Web-observable: a `.brink` entry compiled through `compile_project` /
  `compile_fragment` reaches the native pipeline (`brink_environment::compile`
  dispatches on the entry's extension via `brink_driver::is_native`), so a
  source carrying any of these annotations now parses and compiles where it
  previously failed with a parse error.

- 52fb2d3: NG-D (ruled 2026-07-27): the native `.brink` surface gains an
  array/sequence literal, `[1, 2, 3]` — square brackets, expression position
  only, lowering directly to the same `Expr::ArrayLiteral` HIR shape the
  brink dialect's `#[…]` sigil literal already produces. The B5-symmetric
  `Array { … }` construction-registry spelling was weighed and rejected:
  brackets were already lexed and idle in expression position, and the
  everyday collection literal deserves the lightest spelling. `[]` (empty)
  and nested arrays (`[[1, 2], [3]]`) are both accepted; every existing
  dialect-agnostic analyzer pass over `Ty::Array`/`Expr::ArrayLiteral`
  (inference, containment, comparator contracts, …) applies unchanged.
  Observable through `@brink-lang/web`: a `.brink` project compiled through
  the wasm package can now parse and run source using this literal, where it
  previously failed with a parse error. Fixes #1490.
- b895c4f: Issue #1492 (RULED 2026-07-26, `docs/decision-log.md` "Lowering consumes
  analyzer types, never re-derives"): `or`-coalescing chains are now typed by
  the analyzer as a whole, and the verdict is published for LIR lowering.

  The user-visible change is one widened diagnostic, brink dialect + `types =
strict` only (vanilla ink cannot produce an `or`-coalescing expression at
  all, so the ink corpus is untouched):

  - **E066 now judges every step of a chain, not just the innermost.**
    `{some(1) or none or "text"}` previously passed analysis silently —
    a chain's outer step had no left-hand type to check against, because
    the analyzer classified operands one node at a time and an `Expr::Infix`
    operand classifies to nothing. Each step's recorded result type is now
    fed in as the next step's left-hand type, so the mismatch is reported
    where it always was. A well-typed chain compiles exactly as before.

  No runtime, bytecode, or codegen behavior changes: `Opcode::Coalesce` is
  untouched, and its doc now states the ruled gradual-mode posture (with an
  unpinned left-hand type, the runtime check _is_ the operator's semantics —
  an `Option` coalesces, a plain value faults).

- 4de4d3f: Fix #1495: `push`/`insert`/`remove_at`/`remove`/… on a struct-field lvalue
  (`push(a.items, 3)`, `a: Bag`, `Bag.items: Array<int>`) used to compile
  clean and silently misroute the mutation onto the _root_ variable instead
  of the field, faulting at runtime with `NotIndexable("record")` — a bare
  `ident.ident` chain always parses as one multi-segment `hir::Expr::Path`
  (never `hir::Expr::FieldAccess`), and the mutator's bare-variable fast path
  resolved that whole path's range to the root symbol.

  `try_lower_mutator_stmt`'s lvalue dispatch now mirrors
  `try_lower_field_assignment`'s existing split: a single-segment path (or
  one that doesn't resolve to a struct-field root) keeps the bare-variable
  fast path unchanged; a single-level struct-field projection (`a.items`)
  routes through a new `lower_field_mutator` (take root → `RecordSet` the
  mutated field → write back — the same discipline `p.field = v` already
  uses); a **chained** projection (`o.inner.items`, 3+ segments) is rejected
  with the same non-suppressible `E074` `try_lower_field_assignment` already
  raises for a chained field _write_, rather than falling through to the
  same silent misroute.

- ad09a98: Fix #1502: in a multi-file project, the implicit final gather #1448 added is
  now synthesized for the **entry file only**, not once per `INCLUDE`d file.

  Brink lowers root-level content one chunk per file, so #1500 attached a
  terminus to every file's chunk. A trailing weave in an `INCLUDE`d file
  therefore ended the story silently at that file's last gather, swallowing
  everything the entry file had after the `INCLUDE`. C# ink guards the implicit
  gather with `if (isRootStory)` (`FlowBase.SplitWeaveAndSubFlowContent`): an
  included file is parsed as `Story(isInclude: true)`, its root content becomes a
  nested weave container, and running off the end of that container reports
  "ran out of content. Do you need a `-> DONE` or `-> END`?" — the loud fault
  brink now reports again.

  Playground/editor projects that `INCLUDE` a file ending in an unterminated
  weave get the same diagnosis the reference compiler gives instead of quietly
  truncating; a trailing weave in the entry file still ends cleanly. Oracle
  conformance: 5,601 → 5,605 passing episodes, 361 → 363 passing cases (two new
  `tier3/includes` cases; no existing episode changed).

- 98a1ae6: Fixes issue #1503: a `ChoiceSet` whose empty, unlabeled continuation sits
  inside a knot or stitch (not the file's root content) no longer gets an
  implicit `-> DONE`. Falling off the end of a knot/stitch without an
  explicit `-> DONE`/`-> END` is a genuine ink runtime error
  ("ran out of content. Do you need a '-> DONE' or '-> END'?"), not a safe
  implicit end — only root content gets the safe implicit end. Root-content
  `ChoiceSet`s are unaffected; they keep emitting the implicit `-> DONE`.
  Observable through `@brink-lang/web`: a story compiled from source with
  this pattern used to run one extra (incorrect) step and end `Done`
  instead of surfacing the runtime error.
- 9dec659: Fix #1504: anonymous root-content container ids are now qualified by the file
  that owns them, so two files with root-level weave content no longer collide.

  Root-level weave content was scoped under an _empty_ path in every file, and
  address allocation is a pure hash of that path with no collision avoidance —
  so the entry file's first root choice and an `INCLUDE`d file's first root
  choice were both `c-0` and received the **same** `DefinitionId`. That id is
  the linker's address key (last-write-wins) and the save key for visit counts,
  so the collision was a live silent miscompile: the player was offered the
  included file's choices, picked one, and the _entry_ file's choice body ran.
  The scope path is now qualified by the owning file's project path
  (`hir::root_content_scope_path`). Per-file rather than per-module: an
  `INCLUDE`d file with no `#@module` inherits its includer's module
  (`docs/modules-spec.md` §1), so a module qualifier would leave the exact shape
  #1504 was filed against still colliding.

  The same change makes the synthesized root terminus content-pure: it was keyed
  `#root-terminus.{file_id}`, the one address in `brink-ir` derived from a
  `FileId` rather than from a path, so an editor/LSP session that registered a
  sibling file before the entry minted a different id for the same source tree.

  **Observable through `@brink-lang/web`**: `brink-web`'s compile session calls
  `brink_compiler::compile` directly, so a playground/editor project whose entry
  file and an `INCLUDE`d file both carry root-level weave now compiles instead
  of failing with #1673's `E060` duplicate-`DefinitionId` guard — and the story
  it compiles to runs the choice body the player actually picked.

  ⚠ **This is an identity break, not a plain bug fix.** It re-keys existing
  definitions:

  - **Anonymous visit counts and sequence positions in existing saves are
    invalidated**, with no migration path and no load-time diagnostic. Anonymous
    containers (`c-N`, `g-N`, `b-N`, `s-N`) have no author-visible name, so
    `#@was`/alias rebinding — which is name-based — cannot teach the loader the
    old id. The blast radius is bounded by construction: globals are name-keyed
    and an anonymous count is unreadable from author expressions, so this
    surfaces at most as a re-shown once-only choice or a restarted sequence.
  - **Translations are _not_ affected — verified, and this corrects the earlier
    sequencing note on #1504.** `brink-intl`'s export keys a translation scope on
    a `ScopeLineTable::scope_id`, and codegen opens a line table only for a
    scope-kind container (`Root`/`Knot`/`Stitch`); root-level choices and gathers
    inherit the **root** scope's id, which is the hash of the empty path and is
    not qualified by file. So no XLIFF unit id for a root-level line moves.
    Pinned by `root_content_translation_scope_id_is_unaffected_by_the_qualifier`
    in `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`. Worth
    stating explicitly because #1690's alias-aware rebinding could not have
    helped here: it rebinds by id through the alias table, and an anonymous
    container has no `#@was` site to populate an edge from.

  Oracle conformance: 5,607 passing episodes before and after — no existing
  episode changed. One tier-1 corpus case was added
  (`tests/tier1/includes/root-weave-in-entry-and-included-file`) for the shape
  that had no coverage at all; it compiles now (it tripped `E060` before) but
  still fails on a separate, pre-existing divergence — brink does not accumulate
  root-weave choices across the `INCLUDE` splice the way C# ink does, the same
  gap `tier3/includes/choice-accumulation-across-include` and
  `tier3/includes/root-content-splice-site` already record.

  ⚠ **Known limitation, flagged in review: the qualifier was not normalized —
  fixed by [#1696](https://github.com/Syynth/brink/issues/1696).**
  `hir::root_content_scope_path` used to qualify by the file's _raw registered
  path_, not a normalized root-relative key, so anonymous root-content identity
  was a function of how the entry (and any `INCLUDE`d file) happened to be
  spelled when it was registered: `brink compile story.ink`, `./story.ink`, and
  an absolute spelling of the same file minted three different anonymous
  container-id sets for identical source, and `brink-lsp` (which keys by
  absolute OS path) and the CLI (which keys by whatever spelling the caller
  passed) disagreed on ids for the same tree beyond the registration-order
  parity this PR restored (`root_content_ids_agree_between_discover_and_editor_
order` covers registration order only, with the same path spelling in both
  orders — see its doc comment). #1696 derives the qualifier from a
  root-relative key via `brink_driver::native_source_root` +
  `brink_db::modules::root_relative_key` (the mechanism #1572 built for exactly
  this hazard in native modules, extended to cover ink) — see
  `.changeset/issue-1696-ink-root-content-key-normalization.md` for that
  change's own save/translation-impact writeup and
  `docs/root-content-identity-findings.md`'s "Known limitation" section for the
  full history.

- d22cef5: #1505: `brink-syntax-native`'s `struct_field` grammar widened from a bare
  dotted `PATH` to the full `type_expr` production (function types, generic
  instantiations). A `.brink` source compiled through `@brink-lang/web` may
  now declare a function-typed field (`greet: fn(int): int`) or a
  container-typed field (`list<int>`, `map<K, V>`) where it previously hit a
  parse error. A module-qualified struct field type — whether spelled
  `geo::Point` or the documented `geo.Point` form (`docs/modules-spec.md`) —
  is a new, documented gap — `type_expr` accepts a single `IDENT` only,
  matching the same restriction the brink dialect's own type-annotation
  grammar already has everywhere else.
- 11cdf95: Issue #1506: LIR/codegen now consumes the `ufcs_resolution` verdict table
  PR #1497 (#1482) shipped. A `.brink` method-call site (`recv.name(args)`)
  that resolves cleanly at analysis now actually compiles and runs instead of
  being refused with `E144` at lowering: field access wins over a same-named
  free function and lowers as a call through the field's value, a free
  function in ordinary lexical scope desugars to `name(recv, args)`, and a
  T1b/NS stdlib prelude verb (or classic ink builtin) desugars the same way
  through the existing builtin/stdlib dispatch. `E144` remains as a defensive
  fallback for a resolved site with no recorded verdict (only reachable by a
  caller that skips the analyzer's `ufcs` pass); it is no longer the blanket
  refusal every resolved method call hit.

  Web-observable through `compileProject`: a native `.brink` entry using
  method-call syntax onto a free function or a T1b/NS stdlib prelude verb
  (including the collection mutators — `m.insert(k, v)`, `a.push(v)`, etc.,
  lowered the same statement-only way their bare-call form is) — previously
  always a compile refusal (`E144`) — now compiles and its `StoryData` runs
  the call for real. The field-access `FieldCall` verdict also lowers for
  real, and is now reachable from native `.brink` source: NG-E (#1505) widened
  `brink-syntax-native`'s `struct_field` grammar from a bare path to the full
  `type_expr` production, so a function-typed struct field
  (`greet: fn(int): int`) is a real, parseable shape — `brink-ir/tests/ufcs_field_call.rs`
  runs the whole pipeline (parse through LIR lowering) on that real `.brink`
  fixture rather than hand-patching a lowered `TypeExpr` after the fact.
  `.ink` compiles are unaffected (ink's own lowering cannot produce the
  multi-segment callee path this pass keys on). Auto-ref (a free function
  reached through method syntax whose first parameter is `ref`) was out of
  scope here; it lands separately in #1462, in this same release.

- 38db35c: Fix (#1507): editor hover and go-to-definition now report the UFCS
  resolution verdict for `recv.verb()` call sites on native `.brink` files,
  instead of falling through to the receiver's own binding. The D2 ruling
  (#1482) justified the `ufcs_resolution_query` side table partly on this IDE
  payoff, but the editor never queried it — hovering or jumping from `verb` in
  `recv.verb(args)` showed/jumped to `recv`'s declaration instead.

  Hovering the method segment now shows whether the call dispatches through a
  struct field (`FieldCall`), a resolved free function (`FreeFnDesugar` /
  `FreeFnAutoRef` for D5 by-reference dispatch), or a stdlib/builtin prelude
  verb (`PreludeDesugar`); go-to-definition jumps to the free function's
  declaration when there is one, and does nothing (rather than jumping to the
  receiver) when the verdict has no `DefinitionId` to jump to. The override is
  scoped to exactly the method segment's own range — hovering/jumping from the
  receiver itself is unaffected.

- 186546c: Choice-guard `as` un-deferred (#1508, decision log 2026-07-26): the
  native-only `* {if EXPR as name} [text]` binding now compiles and runs for
  real, capturing the unwrapped `Option<T>` payload at **presentation
  time** (ordinary COW value semantics — the same rule closure capture
  uses). The picked choice's own body sees the value the player saw, even
  if the same-name source is mutated between the choice appearing and
  being picked. `E146` ("not yet supported") is retired — a story that
  previously failed to compile on this construct now compiles and runs.

  No wire-format change: the guard's binding reuses the same `OptionBind`
  opcode and frame-slot machinery `if EXPR as name { … }` already uses
  (issue #1475), and the captured value rides the pending choice through
  selection via the existing thread-fork snapshot that already restores
  tunnel/function temps across a pick — verified end to end, including
  across a `StorySnapshot` detach/reattach round trip. `Story`/`Choice`'s
  public shape is unchanged. Oracle corpus unaffected (native-only
  construct, no ink counterpart).

- 2ae8fc9: Compiler: `hir::Stitch` carries a `return_type` (issue #1509).

  NG-C (#1489) widened `Knot` with a `: type` return-type annotation but left
  `hir::Stitch` — a nested flow, the ruled general form of a stitch — without
  the same field, so a return-typed nested flow stayed fenced behind `E129`
  (native) or failed to parse at all (ink's `= name(params): type`).

  Both frontends now parse and lower a stitch's return-type clause onto the
  same `hir::TypeExpr` a knot's does, and a native nested flow that declares
  one is exempted from the implicit `-> DONE` grace — the same
  coroutine-vs-state toggle `Knot.return_type` already drives — instead of
  having its return clause silently ignored or flagged as unlowered.

- 63bc2a3: Issue #1517: HIR infix expressions (`lhs op rhs`) now carry their own
  source `Provenance`, so an `or`-coalescing chain and its own left spine
  are separately addressable in the analyzer's typing side table. Before
  this, an infix node's only identity was the union of the ranges reachable
  in its subtree, which a chain shared with its left spine whenever the
  trailing operand carried no range of its own (`some(a) or f() or 99`), so
  the analyzer had to drop _both_ verdicts rather than risk serving one
  node's verdict under another's key.

  Web-observable effect is narrow but real: a `types = strict` brink-native
  chain whose key previously collided lost its recorded shape verdict and
  fell back to the runtime coalesce check; it now keeps the verdict and
  lowers to the shape its types imply. No diagnostic, ink-dialect, or
  bytecode change otherwise — the oracle corpus holds at 5,599 episodes.

- 276bf6c: Fix (#1526): IDE analysis now mints the project database's `DefinitionId`s
  for native `.brink` files, so editor hover stops silently dropping
  db-backed detail on native projects.

  A native file's module is its path (`market/barter.brink` →
  `story::market::barter`) and always qualifies identity, but the analysis the
  editor session runs was module-blind — it hashed every symbol by bare name.
  The ids it handed to hover therefore missed in the db's per-definition
  queries, so hovering a knot/stitch in a `.brink` file showed no effect row,
  no TM-2 declared parameter/return annotations, and no inferred types, on
  every native project (single- or multi-file). Most ink projects were
  unaffected, since their undeclared stem-modules don't qualify identity and
  both paths already agreed — but an ink file carrying a declared `#@module`
  annotation qualifies identity the same way a native file's path does, so
  those projects get the same fix: changed identity, plus import-scoped
  resolution and cross-module coexistence in the editor.

  The analysis pass is now fed the database's resolved module map, which is
  where path-derived native identity is minted and stays minted — the
  analyzer never recomputes it. Same fix reaches the LSP's background analysis
  pass and the overlay/projection gates used by rename and move.

- cc52b83: Compiler: a struct construction literal is now a legal `VAR`/`CONST`
  declaration default, so struct-typed durable globals are spellable (issue
  #1530).

  `VAR p = Point#{x: 1.0, y: 2.0}` — and, on the native surface,
  `var p: Point = Point { x: 1.0, y: 2.0 }` — used to be refused outright with
  `E075`, because the LIR's compile-time constant domain had no
  record-carrying value. A well-formed literal now folds into a real record
  that is baked into the compiled story, so reading a field of such a global
  before anything writes to it yields the declared value.

  That unblocks the T1e projection-receiver path end to end: a projection's
  root must be a durable cell, so `g.hp.heal(5)` — a method call whose
  receiver is a projection off a global — had no spelling that could reach it,
  and `E143`'s own advice ("bind the receiver to a durable cell") pointed at
  something the language could not express.

  `E075` is narrowed rather than removed: a declaration default is baked into
  the story with no runtime construction step left to fault at, so a literal
  that omits a declared field or supplies an undeclared one stays a compile
  error under both `types` policies (under `types = strict` the analyzer's
  more precise `E069`/`E070` reports first). Its message changes accordingly.
  Two knock-on diagnostic changes: an unresolved shape name in that position
  now reports `E073` (the same code the expression-position path uses), and a
  never-constant _field value_ now reports `E077`, the same code an array
  element or map value in that position already did — previously the whole
  literal was rejected before any field was examined.

- 96173a5: Issue #1531 (RULED 2026-07-27): frame-local projection receivers are
  legal for UFCS auto-ref. `let g = Guest { … }; g.hp.heal(5)` — a `ref`
  first-parameter method call whose receiver is a `let`/param-bound struct
  field, one field level deep — now compiles and mutates the caller's
  binding, instead of refusing with `E143`. A frame-local cell is a valid
  projection root; the mutation needs no effect row because it is
  unobservable outside the frame. LIR lowering never reuses the durable-only
  `RefProjection`/`MakeProjection` machinery for this case — it splices a
  read/call/write-back RMW sequence instead, the same discipline `g.hp = 5`
  already rides. The durable-rooted case (`party.leader.heal(5)` where
  `party` is a `VAR`) and its effect-row requirement are unchanged. A
  frame-local projection more than one field deep still refuses with `E143`
  (no lowering support beyond one level, matching plain assignment's `E074`
  boundary).
- 39124bb: Issue #1532 (PR #1501 review follow-up on #1484's `remove`/`remove_at`
  split): new compile diagnostic `E149` — a `remove(a, i)` call whose first
  argument is statically known to be an array is now a compile error under
  `types = strict` (the brink dialect's own implicit default), not just a
  runtime `NotIndexable` fault. Only fires when the checker can prove the
  receiver is an array from its own body-local uses (a `temp`/param, not a
  `VAR` — a global's static type has no `Array`/`Map` representation in this
  checker); `types = gradual` is unaffected, keeping the runtime fault as
  its backstop. No behavior change for valid `remove(map, key)`/`remove_at`
  call sites, and the oracle corpus (vanilla ink) is unaffected.
- acc6b0b: Fix (#1539): `find_references` and `rename` now also resolve a UFCS
  call site (`recv.verb(args)`) to the free function it dispatches to,
  matching the fix #1534 already landed for hover/go-to-definition.

  Before this fix, both editor operations keyed off `ResolutionMap`
  alone, whose entry for a UFCS call spans the receiver — so querying
  references from (or renaming) a free function called only via UFCS
  syntax silently missed those call sites entirely. Renaming a free
  function that had UFCS call sites produced a broken program: the
  declaration moved to the new name, but every `recv.verb(...)` call
  site was left referring to the old name.

  Both operations now enumerate a target's UFCS call sites through the
  same `ufcs_resolution_query` verdict table hover/go-to-definition
  already read, narrowly scoped to each call's own method segment.

- 73b12c8: Compiler: `VAR`/`CONST` globals now carry a static `array`/`map`/`struct`/
  `fn`/`handle<K>` type (issue #1540).

  A global's declaration-derived type used to travel as `InferredType`, whose
  domain is scalars plus `divert` and `list<L>`. Every other shape —
  `array<T>`, `map<K, V>`, a nominal `STRUCT`, `fn(T…): R`, `handle<K>` — was
  silently discarded on the way into the globals map that every typed check
  reads, so a collection-typed global was invisible to all of them: `E149`
  (`remove` is map-only) could fire for a `temp` but never for
  `VAR arr = #[…]`, and `int(someArrayGlobal)` compiled clean where the `temp`
  spelling reported `E078`. (`option<T>` and `range` have no annotation
  grammar at all today, so they never reached this widening either way.)

  `Sig` now carries `value_ty`, the declaration's type at full fidelity —
  resolved from the annotation with no downcast, else from the initializer
  literal (`#[…]` / `#{…}` / `Name#{…}` included), else from a `#fn(…)`
  initializer. The narrow `value_type` field is unchanged, so hover and the
  program model see exactly what they saw before.

  Collection-typed diagnostics also reach the UFCS spelling now
  (`arr.remove(0)`, not just `remove(arr, 0)`): inference types a
  multi-segment callee as unknown before intrinsic dispatch runs, so those
  call sites recorded no facts at all — the strict check now reads the B3a
  verdict table, which already carries the receiver's resolved type beside
  the verb's name.

  Programs that were relying on a collection-typed global going unchecked may
  see a diagnostic they did not see before; that diagnostic is reporting a
  real mistyping.

- e5d78d1: IDE: fix UFCS rename/find-references corrupting `receiver.method(...)` call
  sites (issue #1550, the mirror of #1539).

  `resolve::resolve_function`'s UFCS-shaped-callee fallback records the
  resolved reference for a `recv.verb(args)` call site's _receiver_ spanning
  the whole `recv.verb` path (this is intentional — the D2 UFCS pass keys off
  that same whole-path range to type the receiver). `rename`'s and
  `find_references`' plain-reference loops used that range directly, so
  renaming just the receiver (e.g. `g` in `g.greet(3)`) rewrote the entire
  path, silently dropping the method segment and producing a broken program
  (`newname(3)` instead of `newname.greet(3)`) from what looked like a safe
  rename.

  `brink-ide::rename` and `brink-ide::navigation::find_references` now narrow
  a UFCS receiver's reported reference/edit range down to the receiver's own
  first segment (via a new `ufcs_hover::ufcs_receiver_head_range_at_path`,
  mirroring the method-segment narrowing issue #1539 already added) before
  emitting it.

- 7182df5: Issue #1551: `strict::check_def`'s return-value checks (Unknown/Conflicted
  escape `E065`/`E066`, and a new fall-through check) now run for **any**
  def carrying a declared, non-`void` return-type annotation — a
  value-returning `flow`/nested `flow` (knot/stitch) — not just `is_function`
  `fn`s. Declaring a return type on a flow (#1509/#1546) was previously
  legal and completely unchecked.

  New compile diagnostic `E150`: a def declares a non-`void` return type but
  its body may fall through (reach the end, or only ever bare-`return`)
  without ever executing a value-carrying `return <expr>`. This is the
  checker error `docs/decision-log.md`'s 2026-07-22 implicit-end ruling
  (item 3) promised but deferred: "a flow that declares a return type must
  produce a value... falling through without a value is a checker error",
  distinct from a runtime "ran out of content" — an implicit `-> DONE` is
  never treated as satisfying a declared return value. Strict-mode-only
  (`types = strict`, the brink dialect's own implicit default); `types =
gradual` is unaffected.

  This also fixes a latent gap in the pre-existing `is_function` case: an
  annotated `fn f(): int { … }` with no `return` anywhere previously
  inferred as void via a blanket "never returns a value ⇒ void" shortcut and
  skipped checking entirely, silently accepting a declared `int` the body
  never produced. It is now `E150` too, the same as a flow/stitch.

  The oracle corpus (vanilla ink, gradual-typed) is unaffected — no vanilla
  `.ink` fixture declares a flow/stitch return type or exercises this
  strict-only check.

- a5d1b37: Type-name conformance sweep (#1552): the annotation-surface generic
  heads are renamed per the 2026-07-19 casing partition (module segments
  snake_case, type names UpperCamel) — `array<T>` → `Array<T>`, `map<K,V>`
  → `Map<K,V>`, `list<L>` → `List<L>`, `handle<K>` → `Handle<K>`.
  Primitives are unaffected and stay lowercase (`int`, `float`, `bool`,
  `string`, `void`, `divert`).

  `Option<T>` and `Weighted<T>` are now annotatable — previously
  unspellable on the annotation surface, so a function returning `Option`
  had no way to pin its return type against strict inference (#1168).

  The old lowercase generic-head spellings (`array<T>`, `map<K,V>`,
  `list<L>`, `handle<K>`) no longer resolve and are a hard `E061`
  ("unknown type") through the brink-dialect pipeline.

- 67bf45d: Fix (#1553): the editor session's registered options now reach its own
  project database, cross-file hover names the defining file, and module
  stem-collision errors (`E085`) surface in the editor.

  Three web-observable gaps, all the same class — the editor path silently
  seeing different analysis inputs than a real compile:

  - **Options never reached the database.** The editor session runs its live
    analysis off-database, but many features read database queries directly
    (per-file diagnostics, the symbol index, effect rows, inferred types), and
    those are gated on the options _input_. Only `compile()` ever wrote it, so
    a session that never compiled read every one of those under the defaults:
    the declared dialect, typed-mode policy, `[lints]` table and host manifest
    were silently absent. Cross-module duplicate coexistence (`brink`-only)
    and the native strict-only check (`E137`) were gated off outright. Every
    option setter now writes the session's options through.

  - **Cross-file hover text could never render.** `hover`/`hover_doc` passed
    only the hovered file to the lookup set, so a definition in _another_ file
    was never found and the `` *Defined in `path`* `` note was always dropped.
    The whole project is now in scope, matching the LSP.

  - **Stem collisions were dropped in the editor.** `E085` (a file with no
    `#@module` whose stem is another file's declared module name) is produced
    by the project database's module resolution, not by the analyzer pass the
    editor runs, so a collision a compile catches never reached the editor.
    It is now folded back into the editor's analysis and the LSP's background
    pass.

- f58b1f6: IDE: renaming the head of a plain dotted field access (`p.x.y`, not a UFCS
  call) no longer corrupts the reference site (issue #1560, the non-UFCS-call
  half of the #1550/#1539 corruption class).

  `resolve::lookup_variable`'s dotted-field-access fallback records a plain
  field-access reference's resolved range as the _whole_ `p.x.y` path, not
  just the head segment — renaming `p` previously rewrote that whole span,
  collapsing `p.x.y` into `newname` and silently dropping `.x.y`. `rename` and
  `find_references` now narrow to the head segment's own range for this case,
  the same way they already do for a UFCS call's receiver (`recv.verb(args)`,
  #1550).

- ad9d451: Native `.brink` per-declaration `@[…]` annotations now lower instead of
  hard-failing (#1563). `@[effects(pure, silent, total, reads(…), writes(…),
calls(…))]` above a `flow`/`fn` head populates the container's effects
  assertion at both levels (top-level knot and nested stitch) and is checked
  by the same exceedance pass that judges ink assertions (E103/E108/E109);
  previously every such line was rejected with E129 ("parses but has no HIR
  lowering yet"), so the whole surface was unreachable from a `.brink` file
  compiled through `compile_project`. Misplaced or unknown annotations are now
  diagnosed on the channel's own codes (E111 unknown name, E112 unrecognized
  placement, E100/E101/E048 for the assertion grammar) rather than the blanket
  E129. The E111 and E112 diagnostic _messages_ also changed on both surfaces
  to name the native placement alongside ink's. Ink-dialect behavior and the
  oracle corpus are unaffected.
- aef14d6: IDE: qualified references no longer collapse when their target is renamed,
  and references written inside `VAR`/`CONST` initializers are narrowed like
  every other reference (issue #1571, variants 4–5 of the whole-path
  `ResolvedRef` corruption class started by #1539/#1550/#1560).

  - **Tail-segment corruption.** When a reference's resolved target is a
    stitch, a list item or a label, the segment naming it is the path's _last_
    one (`market` in `-> hub.market`, `Red` in `Colors.Red`). Rewriting the
    whole-path range collapsed the reference to `-> newname` / `Crimson`,
    silently dropping the qualifier. `rename`, `find_references` and
    `prepare_rename` now narrow to the tail segment, in every path-bearing
    position (diverts, tunnels, threads, divert-target values, list literals
    and plain expressions).
  - **Declaration initializers.** The HIR walk behind every one of these
    narrowings covered only the block tree, so a reference written in
    `VAR n = p.x` or `CONST k = Colors.Red` never matched and was rewritten at
    its whole-path range. The walk now covers declaration initializers too.
  - **`prepare_rename`** applies the same narrowing as `rename`, so pressing
    F2 on the head of `p.x.y` (or the receiver of `recv.verb(…)`) highlights
    only that segment instead of the whole path.
  - **Semantic tokens** no longer paint a dotted path one uniform colour: the
    field segments of `p.x.y` are reported as `property` (a new, appended
    entry in the token-type legend), and a qualified list-item/stitch/label
    reference colours the segment that actually names the symbol.

- 5ee89a8: Fix #1573: `Story::did_safe_exit` (and the new `FlowInstance::did_safe_exit`)
  are promoted off the `testing`-only feature gate onto the production
  runtime surface. A `Line::Done` is delivered both for an explicit
  `-> DONE` and for a flow that ran out of content — until now the only way
  to tell them apart outside the `testing` feature was to issue an extra
  `continue`/`advance` call and catch `RuntimeError::RanOutOfContent`. Hosts
  (bevy-brink, brink-web, brink-cli, brink-ide) can now read
  `did_safe_exit()` directly after a `Line::Done` instead. No story
  output/execution behavior changes — this only widens what was already
  computed internally to be a normal `pub fn`.
- b615f7d: Fix (#1581): a native `use story::market::barter::haggle;` now names a real
  module, so qualified-import matching can succeed at all.

  `use` lowering built `Import.module` by joining the path with `.` **and**
  keeping the leaf segment — `story.market.barter.haggle` — while the module it
  names is `story::market::barter` (`::`-joined, no leaf). Two independent
  mismatches, so the string could never equal a real module name: every
  `ImportScope`/`import_covers` match failed, cross-file references fell through
  to the bare-name fallback (which picks a flat first-winner when two modules
  export the same name), and a correctly imported public symbol was still
  reported as needing an import (`E025`).

  Now the leaf is the imported _item_ and the prefix is its `::`-joined module,
  matching the module names `native_module_path` mints. Editor-visible
  consequences: `use a::b as c;` — previously rejected as an unrepresentable
  module alias (`E129`) — is an ordinary aliased item import; a `use` of the
  file's own module is now recognized as a self-import (`E090`); and a
  reference imported from a declared module resolves to _that_ module rather
  than to whichever homonym happened to be indexed first. A single-segment
  `use module;` still names the module itself (the qualified form), as does
  `import a::b;`, whose path is now `::`-joined too.

- cc34968: Issue #1582 (RULED 2026-08-03): the native `.brink` grammar gains a `pub`
  visibility marker — `pub flow`, `pub fn`, `pub var`, `pub const`,
  `pub struct`, `pub extern`, `pub flags`. Absent `pub`, a declaration stays
  Private (the already-ratified 2026-07-23 default, unchanged). `pub`
  produces the existing `VisibilityMark::Public`, so `effective_visibility`
  and every downstream cross-module gate (`brink-analyzer::modules`) are
  unchanged — this is a grammar + lowering change, not an analyzer change.

  **Grammar-level break, worth naming even though harmless today:** `pub`
  becomes a reserved word on the native surface (confirmed zero occurrences
  as an identifier across every in-tree `.brink` source before this change).
  Any consumer that tokenizes `.brink` text independently of this crate
  (none currently in-tree) would need to account for it.

  `import`/`use`/`module` do not take `pub` (no `VisibilityMark` slot on
  their HIR shapes); neither do the ink dialect's own knot/stitch
  declarations (a different grammar, untouched).

- 34f740a: Compiler: import aliases (`IMPORT { name AS alias } FROM mod` / `use
mod::name as alias;`) are now honored by resolution, not just recorded
  (issue #1590).

  `ImportItem.alias` used to be read only by the `E089` duplicate-import
  check — `resolve::import_coverage_for_file` keyed bare-import coverage on
  the source name and `lookup_by_name` only ever looked candidates up by
  their own definition spelling, which never contains an alias. So `use
story::market::barter as b;` licensed `barter`, not `b`: a reference to
  the alias reported unresolved (`E024`). Pre-existing on ink's `IMPORT …
AS`, but issues #1581/#1588 newly accept the native `use … as` spelling
  (previously rejected as `E129`), which is what made this reachable from a
  live native project for the first time.

  `ImportScope` now carries an alias table, and `lookup_by_name` falls back
  to it when a direct name lookup finds nothing. Ruling: the alias is
  **additive**, not Rust's shadow-and-revoke — both the alias and the
  original (source) name resolve through the same import afterward. This
  follows from `lookup_by_name`'s existing "byte-identity guarantee" fast
  path, which already returns a globally-unique name unconditionally,
  ignoring `ImportScope` entirely; a strict revoke-on-alias rule would only
  ever take effect in the rarer ambiguous-candidate case, so it would hold
  sometimes and not others. Tested in both dialects, including the negative
  case (a file that never imported the module gets neither the alias nor
  the bare name).

  Companion fix: the `E025` (import-required) diagnostic message no longer
  hardcodes ink's `IMPORT { name } FROM mod` syntax — it never carried a
  dialect signal to render the right one, and the native `use` spelling
  reads wrong to native authors. `brink-ide::import_fix`'s `AddImport`
  quick-fix (which does know the referring file's dialect, via
  `ProjectDb::is_native`) now renders `use module::name;` for a native
  referrer and `IMPORT { name } FROM module` for an ink one.

- c41b0c7: Analyzer: E150 fall-through check no longer false-positives when the
  value-returning `return` lives in a stitch (issue #1591).

  `=== function f(): int === / = compute / ~ return 5` previously raised
  `E150` ("declares a return type but body never returns a value") even
  though the function runs correctly — `check_def`'s E150 path only read
  the knot's own `BodyTypes.has_value_return`, missing a `return <expr>`
  reached purely by falling through into a stitch. Issue #1551 fixed this
  exact blind spot for the E065/E066 escape check, and #1054/PR #1585 fixed
  it again for E067's inferred-void collection; this closes the third and
  last copy.

  `E150` fall-through and `E067` inferred-void now share one
  `has_value_return_over_stitches` reading instead of each carrying its own
  copy. Also settles a previously unruled question in
  `docs/typed-mode-spec.md` §3: "the body" of a function/knot/stitch, for
  `E150`/`E067` return-value purposes, is the def's own block _plus_ its
  stitches — a stitch is reachable by fall-through and is part of the same
  definition's execution.

  The `E065`/`E066` return-type escape check is deliberately **not** part of
  this merge: it reads the def's own inferred return-type signature
  (`sig.return_ty`), which is computed per-def and is never merged over
  stitches, so it keeps reading the def's own body's has-value-return fact
  rather than the merged one.

- 874c40b: Analyzer: `use`/`IMPORT`'s trailing segment is now dual-reading (issue
  #1592, ruled 2026-07-27). `use story::market::barter;` where `barter` is a
  **module** (not an item) previously licensed nothing and produced no
  diagnostic at all — `story::market` (a pure directory prefix holding
  `barter`, never a file's own module) had no `declared_exports` entry to
  check `barter` against, so the well-formedness check could neither confirm
  nor refute it. Two changes:

  - **A trailing segment that resolves to a real submodule now licenses that
    module** — its public exports become reachable via qualified access
    (`barter::haggle`, never bare `haggle`) in the importing file, exactly
    as an explicit `use story::market::barter;` written as a qualified
    import would grant. A trailing segment resolving to an item keeps
    today's behavior unchanged.
  - **A trailing segment resolving to neither an item nor a module now raises
    `E088`** (previously silent) — the retired no-op. This guard also widened
    incidentally: `E088` now fires for a bare import naming an item of any
    **declared** module that exports nothing publicly at all (not just a
    pure-directory prefix), since the check now needs real visibility into
    the _module_ (`known_module_names`, a strict superset of the old
    `declared_exports`-only guard) to validate the dual-reading in the first
    place. A private-but-existing item was previously silent for the same
    structural reason as the pure-directory case; it now diagnoses too.
  - **Precedence, decided and documented**: when a trailing segment resolves
    as _both_ an item of the parent module _and_ a declared submodule, both
    readings apply — the item is bare-importable under its own name, and the
    submodule is also licensed for qualified access under its own name. No
    exclusion between the two (`resolve::import_coverage_for_file`'s doc
    comment has the full rationale).
  - Self-import (`E090`) now also fires for the leaf-item shape when the
    resolved module is the importing file's own (previously only the
    qualified `import mod;` form was checked) — except when the trailing
    segment resolves as a **submodule of the importer's own module**
    (`story::market` writing `use story::market::barter;`), which is the
    import the `E025` import-required gate makes mandatory to reference the
    child's exports, not a self-import.
  - **Aliasing a trailing segment that resolves as a module now raises
    `E129`** (`use a::b as c;` where `b` is a declared submodule) instead of
    silently dropping the alias while still licensing `b`'s exports under
    their original names — mirrors the existing `E129` rejection of the
    single-segment `use a as m;` module-alias shape.

  `#@module(...)` places no structural constraint on an ink module's own
  name (it accepts any non-empty string, `::`-joined or not); the oracle
  corpus is unaffected because no `#@module`/`IMPORT`/`use` construct appears
  anywhere in it, not because of any structural property of ink module names.
  Scoped by #1582 (native visibility, open/needs-design): native definitions
  have no working `Public` marker yet, so an _all-native_ project still
  cannot prove this end-to-end; the mechanics are proven with an `.ink`
  defining side (`#@public`), same limitation `native_use_import_scope.rs`
  (#1581) already documented.

- 0c9db81: `StoryRunnerHandle.compileFragment` (`evaluate()`'s Tier-1 fragment-compile
  step) now picks its synthetic-symbol wrap syntax from the project entry's
  dialect instead of hardcoding ink's `=== ===` knot syntax:

  - A `.brink` native entry gets native wrap syntax — `fn NAME() { return
(EXPR); }` for the expression attempt, `flow NAME() { CONTENT }` for the
    content fallback.
  - An `.ink` (or extensionless) entry keeps ink's `=== function NAME() ===` /
    `=== NAME ===` wraps, unchanged.

  Previously, appending ink knot syntax to a `.brink` entry was a native
  parse error, so `evaluate()`'s Tier-1 fragment path could never succeed for
  a native project — `compile_fragment` itself was already dialect-agnostic
  (#1387/#1595), but its only caller never spoke native syntax. Fixes #1598.

- 65f96b0: Compiler: inline conditional/sequence branches now recognize as `Plain`/`Template` line-table entries instead of always falling back to fragmented `EmitContent` (issue #1667, the 2026-03-15 decision-log ruling).

  `hir::normalize_file` already lifted an inline conditional/sequence out of its content line and spliced the surrounding prefix/suffix text into each branch (added the same day as the ruling), giving each branch its own child container that reaches the ordinary content recognizer independently — but the spliced `Text` parts were never merged, so the recognizer's `Plain` pattern (exactly one `Text` part) could never match. Every branch with an inline conditional/sequence silently fell back to `EmitContent`, which still emits one line-table entry per fragment — the exact "runtime assembles text from parts, translators see shredded fragments" shape the ruling was meant to retire.

  `normalize.rs::extend_merging_text` merges adjacent `Text` parts at each splice seam, but only trims the right-hand side's leading whitespace — so a merged seam can still carry doubled whitespace or a literal tab through to codegen. Rendered output is unchanged anyway, because `add_line_with_hash` in `brink-codegen-inkb` already runs every `Plain`/`Template` line's text through `collapse_whitespace` before it lands in the line table — that pass, not the merge itself, is what keeps compiled output identical before and after this fix. A branch like `{x: sunny|rainy}` in `"It was {x: sunny|rainy} today."` now compiles to two independent `RecognizedLine::Plain` line-table entries ("It was sunny today." / "It was rainy today.") instead of three fragmented `EmitContent` entries per branch.

  `source_hash` impact: a branch that reaches `Plain`/`Template` now gets one clean `source_hash` over its full composed text, instead of several fragment-level hashes under `EmitContent`. Any `.xlf` translation unit exported from the old fragmented line table is orphaned by this change — a real translation-memory migration question for any story with inline conditionals/sequences that already has translated content, not something this fix absorbs silently.

  Known gap, not fixed here: inline conditionals/sequences embedded directly in a choice's own display/bracket/inner text (`* Pick {x: A|B}`) are untouched — `normalize_file` never walks choice display text, only choice bodies — and still assemble from parts at runtime. Filed as a follow-up on issue #1667.

- e4fb577: Compiler: `#@was` on a knot or stitch now emits one compiled alias-table
  entry per descendant re-keyed by the rename, not just one for the renamed
  declaration itself (issue #1671).

  Renaming a knot re-keys every stitch and label beneath it, because their
  qualified names embed the knot's name — but `#@was` previously minted
  exactly one alias entry (the knot's own), so a declared rename still lost
  every descendant's saved visit count and translations. The compiler now
  walks every stitch/label whose qualified name is prefixed by the renamed
  container and mints a bridging entry for each, while it still knows every
  descendant's path — the loader cannot recover this at load time, since a
  `DefinitionId` is a hash and no path can be derived from one. Table growth
  is bounded by the renamed container's subtree size.

- 7e8d3a2: IDE: rename writes `#@was` automatically, and undeclared renames get an
  authoring-time hint (issue #1672).

  `docs/modules-spec.md` §5 rules that the IDE's rename refactor writes the
  `#@was(old_name)` migration directive automatically — this was never
  implemented. `rename`/`rename_safe` (the single chokepoint every rename
  surface funnels through — CLI, LSP, and the studio/web editor) now stamps
  `#@was` onto a renamed knot, stitch, `VAR`, `CONST`, or `LIST`'s declaration,
  in the same edit set as the rename itself. Only under `dialect = brink`
  (`#@was` is itself a brink extension; stamping it under strict ink would
  introduce a fresh `E051` on every rename), and never over an existing
  `#@was` (a second rename of an already-migrated declaration keeps its
  original record).

  New, separate: `brink_ide::rename_detection::detect_undeclared_renames`
  diffs a file's current declared-symbol manifest against a previous one and
  reports an unambiguous 1:1 rename shape (one name vanished, one same-kind
  name appeared, unambiguously) as a `RenameSuspicion` — the authoring-time
  detection for a rename that never went through the refactor (a hand edit, a
  `sed`, a merge). Wired into `brink-lsp`: a `DiagnosticSeverity::HINT`
  diagnostic asks the author directly ("`hub` disappeared and `plaza`
  appeared — did you rename it?") rather than guessing.

- b308544: #1673: codegen now refuses to emit a `StoryData` where two containers share
  a `DefinitionId`, failing loudly with an `E060` internal-codegen-error
  diagnostic instead of silently letting the linker's last-write-wins address
  map drop one container's entry. This closes the exact failure mode #1504
  demonstrated: two files with root-level weave content colliding on an
  anonymous id, where a player picking a choice from one file's weave ran the
  _other_ file's choice body.

  **Observable through `@brink-lang/web`**: `brink-web`'s compile session
  (`session.rs`/`compile.rs`) calls `brink_compiler::compile` directly, so
  any project shape that trips this guard now fails compilation there too,
  surfaced as an `E060` diagnostic rather than compiling to a broken story.

  The #1504 root cause (unqualified anonymous scope paths) is unchanged and
  still blocked on the FG-4d identity ruling — this guard only changes the
  failure mode from silent-wrong-output to loud-compile-error. One other
  existing, source-reachable path now also trips it: two knots sharing an
  author name (`E022`, warning-severity) collide on the same content-hashed
  id and previously compiled silently; that shape now fails to compile too.
  Whether `E022` itself should be promoted to a hard error is a separate,
  undecided design question.

- fbd074e: #1674: anonymous-container state — report it, and lint the opt-in.

  - **`LoadReport` gains `anonymous_states_dropped: u32`.** A saved visit/turn
    count for an anonymous scope (an unlabeled once-only choice or a
    sequence — no author `(label)`) that no longer resolves against the
    current program is counted here, rather than being silently unreported
    (it was, and still is, retained under its saved id either way — this
    changes what's _reported_, not what's _retained_). Additive, wire-visible
    through `@brink-lang/web`'s `load()`/`loadBytes()` JSON
    (`saveState.load`/`Story.loadState` equivalents): the field is always
    present now (`0` on a clean load), where it was previously absent from the
    shape entirely.
  - **New diagnostic `E157`** — an unlabeled once-only choice, or a sequence
    with genuine durable state, carries an anonymous, position-derived
    identity that a later content edit can shift. **Off/info by default**
    (`Severity::Info`, immune to `deny-warnings` unless explicitly raised) —
    a project that never touches `[lints] E157` sees no new build-breaking
    behavior, but the wasm editor session's own diagnostics list (which
    already renders every code's `effective_severity`, `EditorSession`'s
    `apply_project_config`/`apply_lint_overrides`) will start surfacing this
    as a new `Info`-tier entry for any unlabeled once-only choice or
    qualifying sequence already in a project's source. Tier-able through
    `[lints] E157 = "warn"/"deny"/"hint"/"allow"` like any other diagnostic.

  Oracle-neutral: neither change touches compiled `StoryData`/bytecode, so
  the oracle corpus is byte-identical.

- e4fc530: The fn-value verb layer, slice 2 (#1679): `filter_map`, and the ruled
  effectful pair `each`/`map_each` (stdlib-spec §4). Observable through
  `@brink-lang/web`, brink dialect only:

  - **`filter_map(a, f) → [U]`** (`f: fn(T): Option[U]`) — the Option-mapper
    companion of `map`: keeps `f(x)` unwrapped when `some(v)`, drops it when
    `none`, in iteration order. Pure·silent-required, exactly like
    `map`/`filter`/`fold` — a non-Option callback return is a turn-terminating
    fault, the same posture as `filter`'s non-bool predicate return.
  - **`each(a, f) → void`** (`f: fn(T)`) and **`map_each(a, f) → [U]`**
    (`f: fn(T): U`) — the ruled effectful spellings. Unlike the pure quartet,
    their callback's output reaches the transcript instead of being captured
    and discarded, and the dev-mode world-write guard is disarmed: a global
    write or RNG draw inside their callback is legal (in either mode), where
    the identical write inside `map`'s callback is an E119 compile error (a
    provable inline `#fn(target)`) or a `ComparatorWroteState` dev-mode fault
    (an opaque callback). Sequential in iteration order, never fused, and
    deliberately absent from E119's roster — their whole purpose is to be the
    legal home for the effects the pure quartet's gate rejects. The
    escaping-behavior faults (a callback that presents a choice, reaches
    `-> DONE`/`-> END`, or calls a host external) still apply to both — that
    limitation is architectural (no handler exists mid-opcode), not a purity
    rule.
  - **No new opcode.** All six verbs share `SeqVerb` (0xA1); `filter_map`,
    `each`, and `map_each` add three more `SeqVerbOp` kind bytes to the
    three the pure trio shipped with.

  The whole family is brink-dialect surface (strict-ink rejects it), so
  vanilla-ink stories are unaffected and the oracle corpus is byte-identical.

- 666edaf: The fn-value verb layer's pure trio (#1679): `map`, `filter`, `fold`
  (stdlib-spec §4). Observable through `@brink-lang/web`, brink dialect only:

  - **Three new verbs**, each taking a function value:
    `map(a, f) → [U]` (`f: fn(T): U`), `filter(a, pred) → [T]`
    (`pred: fn(T): bool`), and `fold(a, init, f) → U`
    (`f: fn(U, T): U`, left fold; an empty array returns `init` untouched —
    no absence case, so no `Option`). Callbacks are pure·silent-required
    (RULED 2026-07-18), which is what makes iteration order unobservable and
    lets the implementation fuse freely.
  - **One new opcode** `SeqVerb` (0xA1 + kind byte: `map`, `filter`, `fold`)
    appears in disassembly. Each kind evaluates its callback re-entrantly per
    element with output isolated — the same machinery `SeqSortedBy` uses, so
    a callback that presents a choice, reaches `-> DONE`/`-> END`, calls a
    host external, or diverges is a turn-terminating fault, as is a
    non-array receiver, a non-function callback, or a non-bool `filter`
    predicate return.
  - **E119 is now the shared pure-callback gate.** It already rejected a
    provably impure/unsilent `#fn(target)` comparator on
    `sort_by`/`sorted_by`; it now covers the trio's callbacks too, and its
    title changed from "sort comparator must be a pure, silent function" to
    "callback must be a pure, silent function". Per-site messages name the
    verb and its callback's role, so comparator diagnostics read the same as
    before apart from that title.

  The trio is brink-dialect surface (strict-ink rejects it), so vanilla-ink
  stories are unaffected and the oracle corpus is byte-identical.

- 0de4a8f: Analyzer: a call through a fn-typed parameter is now a **row variable** the
  caller instantiates, not the pessimal effect-row floor (part of issue #1680 —
  docs/effects-spec.md §6 mechanism 1 / §6.1b, Fork B and Fork C ruled
  2026-07-28).

  A higher-order definition — one whose body calls through one of its own
  `fn`-typed params — used to fall straight to the touches-everything floor, and
  so did every one of its callers, however precisely that caller knew what it was
  passing. Its row now carries a **hole** at that param's declaration index (the
  "row with a hole" Fork C ruled for the wire), and each call site fills the hole
  with the effect row of the fn value it actually passes. The definition read on
  its own is exactly as unbounded as before; the precision arrives one hop up.

  Both halves are harvested structurally by the existing body walk — a `#fn`
  target is a syntactic name, and a local's origin summary is a syntactic write
  set — so no inferred row ever decides a call-graph edge and the query graph
  stays acyclic (§6.1a).

  The user-visible effect is in the effect-row surfaces: `brink-ide`'s effects
  display/hover, `brink-db`'s emitted `EffectRows` table, and the `@[effects(…)]`
  contract. A definition that calls a higher-order knot with a traceable callback
  now shows a real, non-opaque row where it previously showed the unbounded one,
  and an `@[effects(…)]` bound covering that row is satisfied where it previously
  reported an `E103` exceedance ("no effects assertion can cover this
  definition"), or `E108`/`E109` against `silent`/`total`.

  The conservative direction is preserved on every fallback: a param the body
  reassigns or that is declared `ref`, an argument that did not trace to an
  in-project creation site, a second call site passing something untraceable in
  the same position, or a callback whose own row is still parametric all keep the
  pessimal floor. The `.inkb` `EffectRows` section's **encoding** is unchanged —
  a row still carrying a hole is closed to opaque on the way out, using the same
  `EffectRowEntry` shape as before this change. But a caller whose row now
  _instantiates_ a filled hole emits real, non-opaque `reads`/`writes`/`calls`
  where it previously emitted the pessimal placeholder, so the emitted bytes for
  those definitions differ from `main` — that is this change's headline payoff,
  not a wire-format no-op.

- a9cdbf8: Analyzer: `Ty::Fn` now carries an **effect row**, and the unifier joins it
  (issue #1680 steps 2 and 3 — `docs/effects-spec.md` §5 / the new §6.1c).

  The row is the structural set of in-project **creation targets** whose fn
  values may inhabit a slot — the keys effects-spec §7's `DefinitionId → row`
  table is looked up by, not a computed row. It is minted only at syntactic
  creation sites (a `#fn(target, …)` literal, and a global cell's `#fn`
  initializer through the declaration-derived signature path), carried through
  `bind` unchanged, and joined by `unify` as set union with an absorbing
  `unknown` top element — so a slot accumulates every fn value assigned into it
  "through copies, parameters, returns, and nesting", and a single source
  typed `unknown` keeps the slot conservative (a write typed plain
  `Ty::Unknown` — an unresolved reference, or an unregistered `EXTERNAL`'s
  return — is the unifier's identity instead, and leaves the other operand's
  row untouched).

  **The diagnostic surface is deliberately unchanged.** Effect rows are inferred
  provenance, never part of the written type language, so they must not decide
  whether an argument fits a parameter: the new `infer::assignable` erases rows
  on both sides and replaces the structural `unify(param, arg) == param` test at
  all four assignability checks — both `ValueCallKind::ArgMismatch` sites,
  `annotations`' `E063`, and `structs`' `E071`. Without that, two `fn(int): int`
  values born at different targets would join to a third row and fire an `E063`
  whose own message is self-refuting ("expected `fn(int): int`, found
  `fn(int): int`"), promoted to a hard **error** under `types = strict`.

  No new diagnostic, none removed, and no change to emitted bytecode — rows live
  only in the analyzer's type universe. What this unblocks rather than delivers
  is §6 mechanism 3 (the heap): effect inference still cannot read the
  type-carried row, because that walk runs with empty globals and empty
  signatures by design. Which stratum should read it is the open question §6.1c
  now names.

- 1e91561: #1683 (partial — the "element kind + per-line element data" payload):
  `brink_runtime::Element` is new — `{ kind: String, data: BTreeMap<String,
String> }`, added as `OutputLine.element` alongside the existing
  `text`/`tags`/`block_id` fields. Every line reports the degenerate
  `Element::narrative()` case (`kind: "narrative"`, empty `data`) — this PR
  wires the type and field through the runtime and the `@brink-lang/web`
  marshal layer (`LineJs`/`ElementJs`, both the legacy `Line` union and the
  `StorySession` `SessionLine` shape) so the schema exists and is stable, but
  does **not** yet populate it from an `@[element]` handler's classification
  (kind = handler name, data = its named captures). That population needs
  either new `.inkb` line-table storage (for a single-line, return-based
  handler like `heading`/`transition`) or a VM-level scoping mechanism (for a
  `block`-capturing handler like `cue`/`parenthetical`, whose call emits more
  than one line dynamically) — neither is built here; see the tracked
  follow-up linked from #1683. `@brink-lang/web` consumers reading
  `Line.element`/`SessionLine.element` today always see the narrative
  default regardless of source markup.
- bdeecb2: Track 1 step 4 (#1684): the runtime output contract migrates from `Line`
  to `Step`/`OutputLine`, per `docs/prose-dialect-spec.md` §7 (RULED).

  - **Terminals carry no payload.** `Step::Choices`/`Step::Done`/`Step::End`/
    `Step::Suspended` no longer bundle trailing text — any content that used
    to fuse onto a terminal event now arrives first as its own ordinary
    `Step::Line(OutputLine)`, and the bare terminal follows on the very next
    `continue_single`/`advance` call.
  - **`block_id` is new.** Every `OutputLine` carries a `BlockId` — an opaque
    id grouping the uninterrupted run of adjacent content it belongs to
    (`docs/prose-dialect-spec.md` §3.7/§8d.2). In today's schema-less-ink
    degenerate case this simply counts runs between turn boundaries (a choice
    selection, a `Done` resume, or a host-directed jump); the richer
    attachment-derived assignment rides the element/markup layer (#1683).
  - **`@brink-lang/web` wire shape**: the exported `Line` JSON union keeps its
    existing `type` discriminants (`"text"`/`"choices"`/`"done"`/`"end"`/
    `"suspended"`/`"awaiting_external"`), but terminal variants now always
    serialize `text: ""`/`tags: []` instead of fused content, and the
    `"text"` variant gains an additional `block_id` (number) field. Any
    trailing content a host displayed by reading a terminal's `text` field
    must instead be read off the preceding `"text"` message.
  - **Ratchet unaffected by construction**: `termination.rs::push_terminal`
    (the test harness's terminal-classification fold, reserved for this
    exact migration since PR #1513) now stamps a terminal's classification
    onto the harness's last open step (or synthesizes an empty one if none
    precedes it in the turn) instead of being a pass-through — this keeps
    oracle episode comparison behavior-identical across the split.

- cb874b5: Native lambdas lower (#1685): a `|x| …` lambda in a `.brink` source no
  longer disappears behind the blanket "construct not supported by this
  lowering" diagnostic (E129). It lowers to a real HIR node — pipes with the
  ruled colon return, optionally annotated params, single-expression or
  braced-block bodies with the trailing expression as the value — so its body
  is now analyzed, its params resolve as locals (hover/go-to-definition see
  them), and a write to a captured binding is reported as the new compile
  error E156 instead of passing unnoticed. Because a lambda has no runtime
  representation yet, compiling one still fails, but with a targeted E052
  naming the missing lifting step rather than E129. Ink sources are entirely
  unaffected — ink's grammar cannot spell a lambda — and the oracle corpus is
  unchanged.
- f766b2a: Fix #1696: an `.ink` entry's anonymous root-content container ids are now
  qualified by a **root-relative key**, not the entry's raw registered path
  spelling.

  `hir::root_content_scope_path`'s qualifier (added by #1504) used whatever
  string the caller passed as the entry — `brink compile story.ink`,
  `./story.ink`, and an absolute spelling of the identical file minted three
  different anonymous container-id sets for byte-identical source, and
  `brink-lsp` (which keys its project database by absolute OS path) and the CLI
  (which keys by whatever spelling the invocation used) disagreed on ids for
  the same tree. `prepare_driver` now registers an ink project root
  (`ProjectDb::ink_root`) via `brink_driver::native_source_root(entry)` — the
  same `brink.toml`-walk-up-or-entry's-own-directory rule a native `.brink`
  compile already used to root-relativize its own module identity (#1572) — and
  every `file_paths` map the stamping/lowering passes read
  (`normalized_stamped_query`, `chunk_lowering_ctx_query`, `lir_lowering_query`)
  now strips that root before qualifying, via the renamed, now-shared
  `brink_db::modules::root_relative_key` (previously `native_root_relative_key`,
  native-only).

  **Reachability, corrected (review finding on #1706, re-traced against the
  real call graph):** `@brink-lang/web` is **not** reached by this mechanism.
  `brink-web`'s compile entry point is `compile_over_tree`, which always goes
  through `Project::load` + `brink_environment::compile` — never
  `brink_compiler::compile`/`compile_path` directly (every call to those in
  `crates/brink-web/src` is inside a `#[cfg(test)]` /
  `#[cfg(all(test, target_arch = "wasm32"))]` module, exercising nothing
  reachable from the published package). `brink_environment::compile` never
  calls `set_ink_root`, and does not need to: `Project::load` already seeds
  `ProjectDb` with root-relative source keys, so `root_relative_key` is the
  identity function on that path (`ink_root` stays `None`) both before and
  after this PR. The CLI's `brink compile` is the same story — it calls
  `brink_environment::compile` too, via `compile_entry` in
  `crates/brink-cli/src/main.rs`, not `brink_compiler::compile*`.
  This changeset is filed per the standing "crates-only PRs need a
  `@brink-lang/web` patch" policy (decision 2026-07-11) despite the traced nil
  delta, so the release still carries a record of the identity-re-keying below
  for anyone reading the changelog.

  **The surfaces this PR actually re-keys** are the callers who use the
  `brink-compiler` library's `compile`/`compile_path` entry points directly,
  bypassing `brink_environment`/`Project::load`'s already-root-relative
  registration: the oracle harness (`compile_path` in
  `brink-test-harness`), `bevy-brink` (`brink_compiler::compile*` call sites
  in `crates/bevy-brink/src/{request,ground_truth,source_loader,brkt,
test_support,replay,locale,capability}.rs` and `bindings/tests.rs`), and any
  other external consumer of the `brink-compiler` crate — plus `brink-lsp`,
  whose `register_native_root` now also calls `set_ink_root`.

  ⚠ **This is a second identity break on top of #1504's, not a plain bug fix,
  for those surfaces.** It re-keys existing definitions again:

  - **Anonymous visit counts and sequence positions in existing saves are
    invalidated a second time**, for any project whose entry is registered
    under a spelling other than the bare project-root-relative one — which
    includes a bare CLI invocation run from somewhere other than the resolved
    project root, an absolute-path CLI invocation, and _every_ file the LSP
    holds, always, since it keys by `file://` URI. `root_relative_key` leaves a
    path that is already root-relative unchanged, so a CLI compile invoked from
    exactly the resolved project root with a bare relative entry is
    byte-identical to before this change; a compile invoked with an absolute
    or `./`-prefixed entry, or from elsewhere in the tree, is not. Same
    no-migration-path caveat as #1504: anonymous containers (`c-N`, `g-N`,
    `b-N`, `s-N`) have no author-visible name, so `#@was`/alias rebinding
    cannot teach the loader the old id.
  - **Translation _scope_ ids are not affected**, for the same reason #1504's
    changeset gives: `brink-intl`'s export keys a translation scope on
    `ScopeLineTable::scope_id`, and codegen opens a line table only for a
    scope-kind container (`Root`/`Knot`/`Stitch`); root-level choices and
    gathers inherit the **root** scope's id, the hash of the empty path, which
    no file qualifier — raw or root-relative — has ever touched. Pinned by
    `root_content_translation_scope_id_is_unaffected_by_the_qualifier` in
    `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`, unchanged
    by this PR.
  - **Translation export's per-line `source.file` reference _does_ change**
    (review finding on #1706 — narrowing an earlier, overbroad "translations
    are not affected" claim in this changeset). The same `file_paths` map
    `chunk_lowering_ctx_query`/`lir_lowering_query` now root-relativize also
    feeds `brink-ir`'s `build_source_location`
    (`crates/internal/brink-ir/src/lir/lower/recognize.rs`), which populates
    `LineEntry::source_location.file`; `brink-intl`'s `export_lines`
    (`crates/internal/brink-intl/src/export.rs`) emits that verbatim as
    `SourceJson.file` in `lines.json`/XLIFF. For any of the direct-library
    surfaces listed above whose entry was registered under a non-root-relative
    spelling, the `source.file` an exported translation unit points at changes
    from that raw spelling to the root-relative one — a metadata-only change
    (the export's scope/line _identity_, `scope_id` and `hash`, is untouched;
    only the human-readable source-file annotation moves). `@brink-lang/web`'s
    own translation export (`story_runner.rs`'s `export_lines`, over a
    `brink_environment`-compiled `StoryData`) is unaffected, per the
    reachability correction above.

  Oracle conformance: the harness compiles every case through an _absolute_
  entry path (`CARGO_MANIFEST_DIR`-derived), so this change does move every
  oracle case's anonymous root-content ids from an absolute-path qualifier to
  a root-relative one. That move is invisible to the oracle comparison, which
  diffs `Line` output (text/tags/choices), never internal `DefinitionId`
  values, and the normalization cannot introduce a new id collision (stripping
  a shared root prefix is injective — two distinct raw paths under one root
  stay distinct after stripping). See the PR body for the exact CASES/EPISODES
  count re-run against this change.

  Pinned by `root_content_ids_are_stable_across_entry_path_spellings` in
  `crates/brink-compiler/tests/issue_1504_root_content_identity.rs` (flipped
  from the `..._known_limitation` assertion #1693's review left in place) —
  asserts `main.ink`, `./main.ink`, and an absolute spelling of the same file
  now mint identical container ids.

- af56482: Lambda lifting (#1709): a `|x| …` lambda in a `.brink` source now compiles
  and runs. #1685 landed lambdas as far as HIR, after which LIR lowering
  raised a targeted E052 ("no runtime representation yet"), so compiling any
  source containing a lambda still failed. Lowering now lifts the lambda body
  into a synthesized top-level function and creates an ordinary function value
  over it — `PushFnRef` when the lambda captures nothing, `MakeClosure` (the
  existing `VAL_CLOSURE` `{name, is_ref, payload}` environment) when it does.
  Capture is by value always, per the 2026-07-19 ruling: each capture is
  evaluated once at the point the lambda value is made, so a later write to
  the enclosing local is not visible through the value, and no capture is ever
  a `ref`. Both ruled body spellings work — the single expression and the
  braced block whose trailing expression is the value.

  The practical consequence is that a lambda literal is now a legal callback
  for the pure verb trio `map`/`filter`/`fold` (#1679); before this, `#fn(named
function)` was the only fn-value spelling those verbs could be handed. Note
  that "pure-required" still cannot be checked through a lambda callback:
  `Ty::Fn` carries an effect row since #1680 step 3, but no inferred type is
  threaded into the E119 gate (`docs/effects-spec.md` §6.1c's stratum question
  is open), so it continues to judge only inline `#fn(target)` callbacks, and
  the dev-mode world-write guard remains the runtime residual. Ink sources are
  entirely unaffected — ink's grammar cannot spell a lambda — and the oracle
  corpus is unchanged.

- 4917db1: Track 1 step 5a (ruled 2026-07-25, `docs/prose-dialect-spec.md` §8b/§8d):
  the native `.brink` prose ground gains the screenplay preset's **block
  elements** — scene headings with a trailing `[slug]` then tags
  (`INT. MARKET SQUARE - NIGHT [market] #tense #act1`, in that ruled line
  order), **header-scoped stitch bodies** (a scene runs to the next heading
  or the enclosing close; scenes are flat siblings, and deeper nesting keeps
  the general `flow x { }` spelling), block cues `@VENDOR` with extensions on
  the tag channel (`@VENDOR #(v.o.)`), the compact cue `@KID: Says who?` as a
  second declared pattern beside the block cue, chain-gated parentheticals
  `(hushed)`, and trailing `#tag`s on a `flow` header line as container-level
  per-flow tags. The lyrics element stays dropped.

  This slice is the **grammar** only: attachment, the conventions `lower:`
  column and the per-flow tag API are separate issues, so every one of these
  shapes is reported as not-yet-lowered (`E129`) instead of being read as
  ordinary prose or silently dropped. Observable through `@brink-lang/web`:
  a `.brink` source compiled through the wasm package now classifies these
  lines structurally and diagnoses them, where the same lines previously
  compiled into player-facing narration. Part of #1715.

- 78cfd24: Track 1 step 5b of #1351 (issue #1716) — the inline markup layer, native
  `.brink` dialect only. `.brink` files compile through the wasm package's
  native path (`brink-db`'s `Language::Native => lower_native_file`), and
  story playback runs through `brink-runtime`, both of which changed:

  - **XML-shaped inline spans** (`docs/prose-dialect-spec.md` §4.1):
    `<name attr="v">content</name>`, self-closing allowed (`<pause/>`,
    `<sfx name="bell"/>` — the point-marker shape, §8b.11). Freeform by
    default (§4.2): an unrecognized tag name is not a parse error.
  - **Nesting doctrine** (§4.3), enforced structurally, and the **final
    escape set** (§8d.6): `\<` `\{` `\#` `\\`, a `\` before anything else is
    now a compile error (previously a bare backslash did nothing).
    **Breaking change for authors:** any existing `.brink` prose containing a
    bare backslash — Windows paths like `C:\Users\`, emoticons like `\o/`,
    or any other unescaped backslash — will now fail to compile. Fix by
    doubling the backslash: `C:\\Users\\`, `\\o/`.
  - **Behavior change**: a `.brink` line containing `<...>`-shaped markup
    previously rendered as literal text (no grammar recognized it). It now
    parses as a real span; story playback renders the span's text with the
    tag stripped (`brink-runtime`'s `Line::Text` has no structured span
    surface yet — that's a separate, later ruling, §7/§9.1) — so
    `<b>bold</b>` now plays back as `bold`, not `<b>bold</b>`.
  - **Wire**: `LinePart::Span` adds the `PART_SPAN` tag to the existing
    `.inkb`/`.inkl` part-tag dispatch. `PART_SPAN` was never part of the v4
    RFC's pre-reserved tag inventory (unlike `VAL_VEC2`/`VAL_WEIGHTED`, which
    needed no bump because materializing them just filled in an
    already-reserved slot), so its introduction is its own one-bump event:
    `.inkb` `VERSION` 5 → 6, `.inkl` version 1 → 2 (`docs/format-spec.md` §
    Versioning). Hash-transparent (§4.4): markup normalizes out of
    `source_hash`, so `Hello <wave>world</wave>` and `Hello world` hash
    identically — a translated line does not re-key when an author bolds a
    word.

- b1122e3: Compiler: `@[element]` / `@[style]` per-declaration annotation declaration
  surface for the native `.brink` prose-dialect authoring surface (issue
  #1719, `docs/prose-dialect-spec.md` §3.5b).

  `@[element(args = "…")]` above a `flow`/`fn` declares the portable-regex
  pattern the prose-dispatch `!name` sigil surface will eventually match a
  content line against — this slice parses it, validates the pattern
  compiles, and validates its named capture groups each bind a real
  parameter on the declaration (the spec's "captures bind params by name,
  compile-checked" contract). A companion `@[style(key = "value", …)]`
  requires a paired `@[element]` on the same declaration, validates its keys
  against the paired pattern's captures plus the two special keys `line`/
  `dispatch`, and classifies each value against the closed built-in
  presentation vocabulary (alignment, emphasis, case, conceal, raw hex
  color) with any other name falling back to a custom `brink-*` CSS hook —
  never a diagnostic, per the spec's own fallback rule.

  Five new diagnostic codes (`E159`–`E163`) reach a project's compile
  diagnostics, which is what makes this `@brink-lang/web`-observable even
  though the feature itself is native-only: a malformed `@[element]` or
  `@[style]` annotation that previously hard-failed with the generic `E111`
  (unrecognized annotation name) now gets a targeted code.

  **Declaration surface only** — the `!name` sigil dispatch rewrite itself
  (matching a content line, binding captures, rewriting to a call) is not
  implemented by this slice; neither is any editor-side consumption of
  `@[style]` (that lands on the held editor track, issues #1131/#1350). See
  `docs/prose-dialect-spec.md` §3.5b's Deferred list.

- 6cd41cc: Issue #1719's remaining scope: a native `@[style(...)]` declaration is now
  readable through the shared `brink_ide::hover::hover` query — hovering a
  knot/stitch that carries one appends a `**style**` line rendering its
  entries (`key = "value"`, built-in tokens spelled from the closed
  vocabulary, `Custom`/color values shown as-written). `StyleToken` was
  previously produced by `hir::lower_native::annotation` and read by
  nothing; this is the compiler-side query half only — no CSS class, no
  semantic-token modifier, no buffer decoration is produced. Observable
  through `@brink-lang/web`'s editor hover, brink dialect only (`.ink` files
  never populate `style_annotation`).
- 18dffa4: Issue #1720: the built-in screenplay preset (`std/conventions/
screenplay.brink`), Track 1 step 8 of #1351. Widens `@[element(claims =
"…")]`/`@[element(args = "…", block)]` natural-notation dispatch (issues
  #1838/#1839) to two grammar shapes it did not reach before: a real
  `@NAME` cue's name and a chain-gated `(delivery)` parenthetical's text
  are now claim candidates, the same way a wholly-literal `CONTENT_LINE` or
  slug/tag-free `SCENE_HEADING` already were — matched line, captures bound
  to params by name, exactly one call. Adds `ElementKind::Cue` and
  `ElementKind::Parenthetical`.

  The shipped preset covers `heading` (bare `INT.`/`EXT.` headings, no
  explicit slug), `transition` (a bare all-caps line ending in `:`), `cue`
  (block-capturing directly-following dialogue), and `parenthetical`
  (block-capturing directly-following dialogue). A cue directly followed by
  a parenthetical (the common screenplay shape) is two independent claims,
  not one joined attachment: the cue's own block capture sees zero lines
  (the ruled block-capture terminator ends a run at any element-level line,
  and a parenthetical is one), and the parenthetical claims the dialogue on
  its own next iteration.

  Not covered: compact cues (`@NAME: text`), any cue/heading carrying a tag
  extension, and a heading carrying an explicit `[slug]` (every worked-page
  heading in the spec uses one) — `candidate`'s literalness rule declines
  all three. Promoting a heading to a real HIR stitch (a genuine divert
  target) is not built anywhere in the compiler; a project wanting that
  still needs an ordinary `flow name() { … }`. Not reachable via `use
std::conventions::screenplay` yet either — no `std::`-namespaced module
  resolution exists in the compiler, and `fn conventions()` registration/
  comptime (issue #1840) hasn't landed — so this ships as authored source
  only, proven end to end via a project that inlines the same handler
  declarations directly.

- 025c865: Analyzer: collapse the effect row's opaque floor when every fn value reaching a
  call site was created in-project (issue #1726, Fork A of #1680 —
  docs/effects-spec.md §6.1a).

  A new per-definition structural atom records the targets whose fn values a body
  **creates** (`#fn(target, …)` literals, including through `bind(…)` chains),
  harvested by the same body walk that already produces the direct-call edges and
  referenced globals — empty globals, empty signatures, structural id sets only.
  No inferred row or signature is ever consulted to decide an edge, so the call
  graph stays row-independent and the SCC batching and effect fixpoint are
  unchanged.

  The user-visible effect is the narrowing this unlocks. Previously a call through
  a local was narrowed only when that local was written **exactly once**; a local
  reassigned to a second known `#fn` origin fell back to the pessimal,
  touches-everything floor, where no `@[effects(…)]` assertion could cover it.
  Now the row is the **join over every write's creation target**, which
  over-reports at worst and so keeps the conservative-total direction. A
  definition that calls through such a local shows a real, non-opaque row in the
  effects-diff/hover surfaces (brink-ide's `effects()` display) and in
  `brink-db`'s emitted `EffectRows` table, and an `@[effects(…)]` bound that
  covers the join is now satisfied where it previously reported an `E103`
  exceedance ("no effects assertion can cover this definition"), or `E108`/`E109`
  against `silent`/`total`.

  The guard is unchanged: a single write whose value did not trace to an
  in-project creation site — a parameter, a call's return value, a heap load —
  keeps the pessimal floor, because such a value can come from anywhere,
  including a host callback. Lambda literals are out of scope (they have no index
  symbol at HIR time).

- 689f1f7: #1728: `content::tag()`'s free-text scan, in `brink-syntax-native` (the
  _native_ `.brink` frontend — `@brink-lang/web` pulls it in transitively
  through `brink-db`, which its `EditorSession`/`IdeSession` use), no longer
  stops at the first literal `}` inside a `#tag` — a `}` that only closes a
  `{…}` the tag's own raw text already echoed (an embedded interpolation or
  alternation brace, e.g. `Hello #tag {gold} coins.`) no longer fools the
  enclosing block's closer into ending early. Previously this produced a
  spurious "unexpected token" parse error; that source now parses with zero
  errors. An unbalanced `}` (including a legitimate enclosing-block closer
  with no matching `{` inside the tag) still terminates the tag immediately,
  exactly as before.

  This is scoped to `.brink` native-syntax files only — `brink-db`'s
  `file_language` routes `.brink` paths to this native parser and every
  other extension (including `.ink`) to `brink-syntax`, the separate ink
  frontend this fix does not touch. An `.ink` project sees zero behavior
  change from this release.

- d7fb30e: Compiler: host-manifest validation of the inline markup vocabulary — the
  second half of `docs/prose-dialect-spec.md` §4.2 (issue #1733), completing
  what PR #1732 landed as freeform-only.

  The host capability manifest gains a `markup` section: an array of
  `{ name, attrs }` span kinds declaring which `<tag attr="v">…</tag>` names
  a project may use and which attributes each accepts. It sits beside
  `externals` because the markup vocabulary is host-authored and can be
  generated from engine code (a text-effect plugin declaring its own tags),
  by §3.4's authorship test — element conventions are project-authored and
  live on a different surface.

  **Freeform stays the default.** With no manifest — and with a manifest
  that declares only `externals`/`types` — markup is never diagnosed, exactly
  as before. Declaring at least one span kind is the only thing that turns
  checking on. Two new codes then reach a project's diagnostics: `E164` for
  an undeclared tag, `E165` for an undeclared attribute on a declared kind.
  Both default to `Warning`, which is what makes their severity configurable
  (`[lints] E164 = "deny"` to make a vocabulary binding, `@[allow(E164)]` or
  `// brink-disable E164` to silence it locally) — a hard-error code would be
  neither overridable nor suppressible.

  Web-observable through `EditorHandle.setHostManifest(json)`: a manifest
  JSON carrying a `markup` key now takes effect, and the resulting `E164`/
  `E165` warnings appear in the background analysis the editor renders and in
  `compile()`'s `warnings` array. Attribute _values_ are unchecked — they are
  static text by construction, so only attribute names are vocabulary.

- 55976d2: Issue #1738 — a consistency audit of the escape/markup layer (§8d.6,
  `docs/prose-dialect-spec.md` §4.6) across every native prose scanner found
  one clear bug and fixed it: `\#` inside a `#tag`'s own text (or an `@NAME`
  cue's own name) is now recognized as escaping `#`'s tag/name-terminating
  role, matching the ruled, final four-character inline escape set (`\< \{
\# \\`) that already worked everywhere else. Before this fix, `\#` inside a
  tag body still split the tag in two at the `#`, leaving a dangling
  backslash — e.g. `Bell tolls #sound \#not a new tag` compiled to _two_
  runtime tags (`sound` and `\#not a new tag`) instead of one (`sound
\#not a new tag`). Runtime tags surface through `brink_runtime::Line`'s
  `tags` field, which the wasm package re-exports, so this is
  wasm-observable through story playback. The backslash itself is not
  stripped from the tag's own literal text — matching the pre-existing `\{`
  precedent in the same two scanners, not a new "strip the backslash"
  behavior.
- 029512d: Implemented `\!`/`\@` as the prose-dialect's ruled line-start escapes (#1744,
  `docs/prose-dialect-spec.md` §8d.6). Previously any leading `\!`/`\@` on a
  native `.brink` content line hit the same "backslash before anything else is
  a compile error" diagnostic as an unrecognized inline escape; now they
  produce a literal `!`/`@` as the first character of the line, matching the
  ruling. `\@NAME` at line start no longer opens a `CUE` — it stays plain text.

  Observable through `@brink-lang/web`: native-dialect source that previously
  failed to compile with an "invalid escape sequence" diagnostic on a
  line-start `\!`/`\@` now compiles and runs. Anywhere else in a line, `\!`/`\@`
  are unaffected and remain the same compile error.

- 405be81: Fix #1749: `InferPass::infer_lambda` now absorbs a block-bodied lambda's
  own statements (`let`/assignment, not just the trailing value expression)
  into the enclosing definition's effect row. Previously only the lambda's
  tail expression was visited, so a block-bodied lambda's `~ temp`/assignment
  statements were silently dropped from the row — a conservative-total
  (`docs/effects-spec.md` §3) soundness violation. Expression-bodied lambdas
  (`|x| expr`) were already sound and are unaffected. This can change
  effect-row-derived diagnostics (e.g. strict-mode reads/writes/calls
  checks, `@[effects(…)]` exceedance) for stories with a block-bodied lambda
  whose statements (not tail) perform effects; the oracle corpus is
  unaffected (no block-bodied lambda in that shape exists in the corpus
  today).
- 9e89eb2: Effects (#1755): a `ref` parameter bound at a `#fn` **creation** site
  (`#fn(heal, player_hp)` — docs/t1c-spec.md §2) now records a write to the
  bound cell in the creating definition's effect row. `ref` binds at two
  grammar positions, and only the _call_-site one was recorded: the write a
  creation-site binding causes was filed nowhere at all — not at the creation
  site, not in the callee's body (where the target resolves as a parameter,
  never a global), and not at the eventual value call (which knows the target
  def but not the cell it was created against). That was an under-report, the
  one direction docs/effects-spec.md §3 forbids a row to move.

  Compile-behavior observable through `@brink-lang/web`: `@[effects(…)]`
  exceedance (`E103`) now correctly fires on a definition whose declared bound
  omits a cell it writes through a creation-site `ref` binding, where it was
  previously silent — a false negative on the one diagnostic that surface
  produces. Rows also widen for such definitions wherever a row is read (IDE
  hover, `brink check`, the `.inkb` `EffectRows` section). No other diagnostic
  changes; the oracle corpus is byte-identical.

- 12b5302: Analyzer: five diagnostic passes now see inside a block-bodied lambda's
  statements (issue #1764, the audit umbrella over #1749's effect-row
  instance).

  Each of these passes has a hand-written recursion for file-level
  `VAR`/`CONST` initializers — the one position the shared HIR visitor does
  not cover — and every one of them stopped at a lambda's _trailing value
  expression_, silently skipping everything the lambda does before it. Note
  that a lambda-valued `VAR`/`CONST` default is already a hard compile error
  (`E083`) independently of this change, so the practical effect is an
  _additional_ diagnostic surfacing inside a file that was already refused —
  LSP-visible (this package's compile-diagnostics API reports it), but it
  does not change whether the file compiles. A construct inside
  `|…| { let x = …; … }` was invisible to:

  - **`E106` / `E138`** — map-literal key domain and duplicate keys;
  - **`E069` / `E070` / `E071` / `E084`** — struct construction shape
    agreement and duplicate fields;
  - **`E078`** — the `int(x)` / `float(x)` conversion domain;
  - **`E152`** — a statically always-false `contains(map, needle)`;
  - **`E066`** — `or`-coalescing type mismatch. This one also feeds the gate
    on building a coalesce table at all (absence is safe by design — the
    effect is a lost static shape, not a miscompile).

  Native (`.brink`) source only: lambdas exist on no other surface. Vanilla
  ink stories are unaffected and the oracle corpus is byte-identical.

- 0b94925: Strict mode's Unknown-escape (`E065`) / Conflicted-escape (`E066`)
  checking now reaches inside a lambda literal's own body (#1770), the same
  way it already does a top-level def's own params/temps. Before this fix,
  `strict.rs` never looked inside an `Expr::Lambda` at all — an unannotated
  or genuinely conflicting param/temp declared inside a lambda's own body
  raised no diagnostic whatsoever, regardless of how nested the lambda was.

  ```brink
  fn f(n: int): int {
    let g = |x: int|: int {
      let t;
      x
    };
    return n;
  }
  ```

  used to compile with zero diagnostics under `types = strict`; the
  lambda's own unannotated, unused `let t` (genuinely `Unknown`) now
  reports `E065`. An ascription on the same temp (`let t: string;`) still
  exempts it, exactly like a top-level `~ temp`.

  Recorded per-lambda (`InferPass::infer_lambda`, folded into a new
  `BodyTypes::lambda_escapes` field), covering both params and body-declared
  temps, for every lambda anywhere in a body including one nested inside
  another lambda's own body. Deliberately excludes a lambda's own
  return-type slot — issue #1994's `LambdaAnnotationMismatch` (`E174`)
  already owns a materially different, eager check for a lambda's return
  annotation disagreeing with its body. Strict-mode-only; `types = gradual`
  is unaffected.

  Widening this check over the existing native corpus surfaces new,
  expected findings — every one an unconstrained lambda param that
  `docs/typed-mode-spec.md` §2 already specifies as an `Unknown` escape
  (call-site-driven inference is forbidden), the same category several
  top-level params already fall into in `tests/tier1-native/`.

- 96998ef: Fix #1773: the E113 reserved-protocol-name walk (`display`/`compare`/
  `next`, stdlib-spec §9.6) now descends into a lambda's own `|…|` params,
  wherever the lambda literal sits — a VAR/CONST default, a temp initializer,
  an assignment, a return value, a divert/tunnel/thread-start argument, a
  content interpolation, a choice/if/while condition, or a native choice
  label's `start_content`/`bracket_content`/`inner_content`. Previously a
  lambda param named `display`, `compare`, or `next` was silently accepted
  while an identically-named top-level fn/knot/stitch param was rejected.

  This makes new hard E113 errors appear on `.brink` files that declare such
  a lambda param, in both the studio Problems panel and through
  `EditorSession`/`IdeSnapshot::analyze`.

- 25e3742: A native `var`/`const` declaration default may now be a lambda literal
  (`const twice = |x| x * 2`), not just a bare-name function reference
  (#1862). Previously this raised `E083` ("declaration default is not a
  compile-time-constant expression") — RULED 2026-08-01 (`docs/decision-log.md`
  #1774), the gate is lifted: a file-scope lambda has no enclosing frame to
  capture from, so the creation-site-capture concern that justifies gating a
  lambda everywhere else never applies here. The lambda still folds through
  the same lambda-lifting machinery (#1709) as any other lambda, just handed
  an empty enclosing frame.
- 533daf9: Issue #1779: fixed a soundness gap in effect-row narrowing where a value-call through a lambda's own parameter could resolve against an unrelated enclosing local's write summary if the two shared a bare name (lambda params are indexed in the same flat name keyspace an enclosing `~ temp` gets). Left unfixed, this would silently under-report the call's effect row instead of falling back to the pessimal floor — the direction docs/effects-spec.md §3 forbids.

  Not observable through `@brink-lang/web` today: reproducing the collision requires combining an ink-only construct (`#fn(...)`, the only fn-value creation site) with a native-only one (`|...| ...` lambdas) in the same body, and no current frontend parses both together. This closes the gap in `InferPass` itself so it stays closed once that convergence happens, and is a pure classification-time restriction (never widens narrowing, only ever falls back to `Unknown` more often) — vanilla-ink stories and the oracle corpus are unaffected (episode count unchanged: 5,607).

- 62e63ba: Analyzer: `E164`/`E165` now point at the exact markup span, not the whole
  content line (issue #1782).

  `hir::SpanPart` carries its own `Provenance` (a new `NodeClass::Span`),
  stamped from the span's `SPAN` syntax node during native lowering. Markup
  vocabulary diagnostics (`E164` undeclared tag, `E165` undeclared attribute)
  now anchor to that per-span range instead of falling back to the enclosing
  content line's range (or, inside a choice's display text, the enclosing
  choice's range).

  Two consumer-visible effects: a content line with several undeclared spans
  now gets one squiggle per span instead of several identical whole-line
  squiggles, and repeating the same undeclared tag twice on one line now
  produces two diagnostics with distinct ranges instead of two byte-identical
  ones. Diagnostic _codes_ and _message text_ are unchanged; only `range`
  narrows.

  Analyzer-side only — `LinePart::Span` (the `.inkb`/`.inkl` wire shape) is
  untouched, since `E164`/`E165` are emitted during HIR analysis, before LIR
  lowering/codegen ever runs.

- 3436d7f: Parser: `element::cue_name()` (the `@NAME` cue-name raw scan) now tracks
  brace depth instead of stopping unconditionally at the first `}` (issue
  #1786). A cue name containing a balanced `{…}` — e.g. `@NAME {gold}
coins.` inside a `flow f() { … }` body — was mistaking that balanced
  `}` for the enclosing block's own closer, ending the block early and
  turning otherwise-clean source into a parse error. Fixed the same way
  `content::tag()` was fixed for the sibling case in #1777/#1728: an
  `L_BRACE` preceded by an odd number of consecutive raw `BACKSLASH`es is
  excluded from the depth counter, since `\{` is the literal-brace escape
  (#1716), not a metacharacter — an even count means the backslashes
  escape each other, leaving the brace unescaped and depth-counted
  (#1852).
- 96eb265: Analyzer: a block-bodied lambda's tail expression is inferred inside the
  lambda's own frame (issue #1789).

  `InferPass::infer_lambda` snapshots and restores the five frame-scoped
  fields (`return_ty`, `has_value_return`, `locals`, `annotated`,
  `local_fn_origins`) around a block-bodied lambda's body, so the lambda's
  own locals never leak into the enclosing definition. The restore was
  landing _between_ the body's `stmts` and its trailing tail expression,
  because the tail was reached through `LambdaBody::value_exprs()` in a loop
  that sat after the restore. The tail was therefore inferred against the
  enclosing definition's `locals` — and since `locals` is keyed by bare
  name, that failed in both directions on a shadowed name:

  - a temp declared by the lambda's own statements was invisible to the
    lambda's own tail, so the `E063` arity check (which needs a known
    callee type) was skipped there entirely (an over-applied call through a
    lambda-local `fn` temp in tail position was never checked for arity —
    a spurious `E065` Unknown-escape fired in its place instead, not
    silence);
  - a use in argument position in the tail unified its type into whatever
    _enclosing_ local shared that bare name, turning e.g. an enclosing `int`
    temp `Conflicted` and reporting a spurious `E066` on a temp the
    enclosing body never misuses.

  The frame window now wraps both the statements and the tail, so both
  directions stay inside the lambda and are discarded by the restore. Under
  `types = strict`, code hitting the second case stops seeing a false-positive
  `E066`, and code hitting the first starts seeing the diagnostic it should
  always have produced (`E063` arity errors on a call in tail position, in
  place of the spurious `E065` it used to get).

  A third, incidental direction: a _captured_ (not shadowed) enclosing
  temp used only from a lambda's tail no longer gets narrowed by that use.
  `observe`'s effect from the tail is now confined to the lambda's own
  frame and discarded by the restore, same as any other tail-position
  write — so an unannotated enclosing temp that previously picked up a
  type from a tail-only capturing use now stays unannotated and `E065`-
  escapes there too, matching how the same use already behaved from
  statement position under #1750. Not a new failure mode — just this fix's
  frame-window change reaching a third case pinned by
  `native_lambda_tail_capture_use_no_longer_narrows_enclosing_capture`.

- 70a1385: Analyzer: `E165` now points at the exact undeclared attribute, not the
  whole enclosing span (issue #1829).

  `hir::SpanPart::attrs` now carries per-attribute `Provenance` (a new
  `SpanAttr` type, `NodeClass::SpanAttr`), stamped from each attribute's
  `SPAN_ATTR` syntax node during native lowering — the attribute-axis
  counterpart of #1782/#1820's per-span fix. `E165` (undeclared attribute)
  now anchors to that per-attribute range instead of falling back to the
  whole enclosing span's range.

  Consumer-visible effect: a span carrying several undeclared attributes now
  gets one squiggle per attribute instead of several identical whole-span
  squiggles, and repeating the same undeclared attribute name twice on one
  span now produces two diagnostics with distinct ranges instead of two
  byte-identical ones. Diagnostic _codes_ and _message text_ are unchanged;
  only `range` narrows. `E164` and `E173` are unaffected: `E164` never had
  this collapse (it is span-, not attribute-, scoped), and `E173` (a
  _missing_ required attribute) has no attribute node in source to point at,
  so it stays span-ranged.

  Analyzer-side only — `LinePart::Span` (the `.inkb`/`.inkl` wire shape)
  keeps its flat `Vec<(String, String)>` attrs, since `E165` is emitted
  during HIR analysis, before LIR lowering/codegen ever runs.

- 7915095: Compiler: a native (`.brink`) tag whose text begins with `@` — the shape
  of an ink-dialect compiler directive (`#@private`, `#@was("…")`,
  `#@local`, `#@module("…")`, `#@effects(…)`) — now raises a targeted
  diagnostic (`E172`, issue #1835) instead of compiling silently as an
  ordinary runtime tag.

  `#@…` is not its own grammar production in either dialect — it is an
  ordinary tag, and only ink's HIR lowerer gives a leading `@` special,
  compile-time-consumed meaning. Native's tag lowering never checked for
  it, so an author porting a file from ink, or splitting time between the
  two dialects, got no error and no warning: the directive text silently
  became literal tag content on the compiled story.

  `E172` is `Warning`-severity and `@[allow(E172)]`-suppressible, not
  `Error` — a project may legitimately want a literal `@`-led runtime tag,
  so the diagnostic never blocks a compile that means it. The message
  names the native `@[name(…)]` annotation equivalent to switch to when
  the tag names a real ink directive that has one (`was`, `effects`), and
  says plainly that there is none when it doesn't (`module`, `public`,
  `private`, `local`). `#@allow` gets its own wording — ink's directive
  recognizer doesn't know `allow` either, so the message never calls it an
  ink-dialect spelling, only notes that native's own `@[allow(…)]`
  annotation (an unrelated diagnostic-suppression channel) shares the
  name. Any other unrecognized name gets a shape-only wording that never
  asserts ink would recognize it.

  `brink-web` transitively depends on `brink-ir`'s native lowering
  (`brink-db::lowered_query` dispatches `.brink`-extension files to native
  parsing/lowering, non-optional), so this new diagnostic is
  wasm-observable for `.brink` projects — an `@`-led tag now reports
  `E172` in the editor instead of compiling silently as ordinary tag text.

- f73db83: Compiler: natural-notation `@[element(claims = "…")]` handlers now dispatch
  prose lines (issue #1838, `docs/decision-log.md` 2026-07-31 "Conventions are
  annotated handlers").

  Issue #1715 landed the native prose grammar — scene headings, cues,
  parentheticals — and nothing lowered any of it, so a writer could type a
  scene heading and the compiler would only report it as not-yet-lowered. This
  slice makes the first of those shapes mean something.

  `@[element(claims = "…")]` is the new spelling beside `args = "…"`: a
  pattern that claims a prose line carrying no `!name` sigil. A claimed line —
  a wholly literal content line, or a scene heading — is matched, its named
  captures bind the handler's parameters by name, and the line lowers to
  **exactly one call** on the handler, whose value is the line. `args` (the
  `!name`-dispatched remainder pattern) is unchanged and still does not
  dispatch.

  Web-observable through the compile-diagnostics surface and through compiled
  output for `.brink` sources:

  - a new diagnostic `E167` — a claiming handler declaring a parameter its
    pattern never captures (the converse of `E159`'s existing capture check,
    needed because every argument of the rewrite comes from a capture);
  - `E159`'s message widened to name both clause spellings, and an
    `@[element]` carrying both `args` and `claims` now raises it;
  - `E112` (misplaced annotation) for a claim anywhere but a top-level `fn` —
    only a `fn` is callable as the expression the rewrite produces;
  - a scene-heading-shaped line that a handler claims now compiles and
    produces output instead of reporting `E129`. An _unclaimed_ heading still
    reports `E129`, unchanged.

  Every claimed line is recorded on the lowered file (matched kind, handler
  name and declaration range, the claiming annotation's range, captures as
  source spans, disposition), so nothing the compiler rewrote is invisible to
  tooling.

  Block capture and `fn conventions()` registration are the ruling's other two
  build slices (issues #1839/#1840) and are not in this one.

- c2d0c9f: #1839: `@[element(claims = "…", block)]` / `@[element(args = "…", block)]`
  now capture the **following run** — terminated by a blank line or any
  non-`CONTENT_LINE` (element-level) line — into a `content`-typed trailing
  parameter, via a new internal `hir::Expr::Fragment` / `lir::Expr::Fragment`
  lowering form (`docs/decision-log.md` 2026-08-01 "Content-as-value").
  Interior lines are lowered through the ordinary body-item dispatch loop, so
  a handler that would claim one of them still claims it — no special case.
  Only `@[element(…, block)]`-declared handlers are affected; a declaration
  with no `block` clause is byte-identical to before.

  **`brink-runtime` fix, reachable through `@brink-lang/web`'s compile+play
  path**: `OutputBuffer::has_content`/`ends_in_newline` (and the test-only
  `ends_in_whitespace`) checked the _outer_ transcript (or nothing at all)
  while inside a `BeginFragment`/`EndFragment` capture, because no earlier
  fragment use had ever captured more than one recognized line — every
  earlier caller (`emit_slot_expr`'s call-composition pattern) composed
  exactly one call's side-effect output. A multi-statement block capture is
  the first thing that exercises a fragment holding several
  `EndOfLine`-terminated lines, and the bug glued them together with no
  separator at all. Fixed to check the active fragment's own capture buffer,
  matching the existing `capture`-scoped branch. Any `@brink-lang/web`
  consumer that constructs a multi-line `Value::FragmentRef` (only reachable
  through this new block-capture mechanism today) is affected; ordinary
  single-line fragment composition (an interior call's output, template slot
  composition) is unaffected.

- f59a88c: Compiler: `@[element(…, block)]` declaration-surface parsing and
  validation (issue #1839, `docs/decision-log.md` 2026-07-31 "Conventions
  are annotated handlers").

  `@[element(args = "…", block)]` declares that the annotated handler
  captures the run **following** its matched line into a trailing
  `content`-typed parameter — the ruled block-capture contract. This PR
  delivers only the declaration surface, matching the precedent #1719 set
  for `element`/`style`: `ElementAnnotation` gains a `block: bool` field,
  and a `block`-flagged declaration with no qualifying trailing
  `content`-typed parameter (or one that collides with a named capture)
  raises the new `E166` (`Error` by default, so not `[lints]`-configurable
  or `@[allow]`-suppressible, matching `E159`/`E160`).

  **Not delivered here:** the `!name`/natural-notation dispatch rewrite that
  actually matches a line, finds a block's terminator (a blank line or any
  element-level line), captures the following run as a `Value::FragmentRef`,
  and calls the handler — that is issue #1838's natural-notation dispatch,
  not yet landed, and this PR does not invent it. See the tracked remainder
  on issue #1839.

  **Not usable end-to-end yet, even for the declaration surface alone:**
  `content` is not a recognized annotation type name
  (`brink_analyzer::annotations::is_known_leaf`), so under `dialect = brink`
  (the dialect brink-lsp and brink-web resolve from `brink.toml`) the
  qualifying trailing parameter `E166` requires — literally annotated
  `content` — itself raises `E061` on the same compile. A `block`-flagged
  declaration parses and validates cleanly (no `E166`), but compiling it
  under the brink dialect still fails, on `E061`, until `content` joins the
  annotation vocabulary (a separate, not-yet-filed ruling). See
  `docs/diagnostics/E166.md`'s note and the regression test
  `e166_block_declaration_surface_parses_but_content_param_still_trips_e061`
  (`crates/brink-compiler/tests/e0xx_diagnostics.rs`).

  Web-observable through `EditorHandle.compile()`/background analysis: a
  `.brink` file with a `block`-flagged `@[element(…)]` annotation now
  lowers `block` onto its `ElementAnnotation` instead of falling through
  unrecognized, and a malformed one surfaces the new `E166` diagnostic
  alongside the existing `E159`/`E160` codes on that same channel.

- 16a548e: Compiler: `E169` — pattern-claiming handlers confined to the `brink.toml`-named
  conventions module (issue #1844).

  The 2026-07-31 §9.1 ruling's item (4) settled an asymmetry: a `!name`-dispatched
  `@[element(…)]` handler stays legal anywhere (it self-announces at the call
  site), but a pattern-claiming `@[element(claims = "…")]` handler — which can
  silently reinterpret ordinary prose as a call — is confined to ONE file, the
  conventions module named by `brink.toml`'s new `[project] elements` key.
  `#1838`/`#1847` already enforced the _placement_ half (`E112`: a claim must be
  a top-level `fn`); this lands the _module_ half.

  - `brink-project-config` parses `[project] elements` (a built-in preset name
    or a project-relative `.brink` path) into `ProjectConfig`.
  - `brink-analyzer`'s `AnalysisOptions` carries it through `apply_project_config`.
  - A new per-file `HirFile::claim_handlers` record (independent of
    `element_matches`) captures every declared claiming handler's name and
    annotation range, regardless of whether it ever won a claim in its own file.
  - `brink-db`'s `conventions_confinement_diagnostics_query` — the one seam with
    both a file's real module identity and the resolved pointer — compares the
    two and emits `E169` (default `Error`, matching `E112`'s posture) naming the
    file the handler should live in.

  Only fires when `elements` names a project-relative path; an unset `elements`
  key or a bare preset name (`elements = "screenplay"`) enforces nothing yet —
  see `E169`'s own doc for the exact boundary and the tracked follow-up (#1863)
  for consuming an _evaluated_ `fn conventions()` registry, a separate,
  larger piece of work this PR does not attempt.

- bee5bdb: `content` is now a resolvable type in the native type system (#1846):
  `fn radio(chan: string, text: content)` — the ruled #1719 example — and
  any other `content`-typed parameter or annotation no longer trips `E061`
  ("unknown type"). `content` is a distinct nominal leaf, deliberately not
  coercible to or from `string` — the whole point of the type is that a
  captured prose value stays translation-resident rather than silently
  flattening to a plain string.

  This is the type-resolution prerequisite only. The dispatch mechanism
  that actually binds a captured run to a `content`-typed parameter (the
  `@[element(args = "…", block)]` block-capture rewrite) is issue #1839's
  scope and is not delivered here.

- 220957f: Compiler: close two silent-drop/undocumented gaps in the natural-notation
  element-claiming dispatch (issues #1847, #1848).

  A claiming `@[element(claims = "…")]` `fn` declared inside a `module { … }`
  block previously validated as a legal placement (it reads as un-nested by
  `flow`/`fn`-depth alone) but was never scanned by the handler-collection
  pass, so it silently registered nothing to claim with. It is now diagnosed
  misplaced (`E112`), the same as a claim on a `flow` or a nested `fn`.

  New diagnostic `E168`: two claiming handlers with byte-identical patterns,
  where the later one never actually won a claim in the file — dead code
  under the interim first-match-wins dispatch order. (A later byte-identical
  twin is not _unconditionally_ dead: it is the only handler that can claim a
  line inside the earlier twin's own body, since a handler can never claim
  inside its own declaration — so the check runs after the whole file is
  lowered and only fires when the later twin produced zero actual claims.)
  That interim order (declaration order, until issue #1840's `fn
conventions()` registration order supersedes it) is now documented at the
  dispatch site and in `docs/prose-dialect-spec.md` §3.5b, along with the
  known gap: a genuine overlap between two _different_ (non-identical)
  patterns is not yet detected.

- 3316a25: Compiler: a `@[element(claims = "…")]` handler's captured parameter
  declaring a numeric/struct/generic/`fn` type now raises a targeted
  diagnostic (`E171`, issue #1849) at the declaration, instead of silently
  compiling and binding the wrong type. The generic form of this mismatch
  (an ordinary direct call's arguments checked against the callee's
  declared parameter types, `E063`) only appears once issue #1864 lands
  direct-call argument type-checking, which does not exist yet — today
  the mismatch is simply silent.

  `hir::lower_native::element::try_claim` binds every named capture as a
  plain `Expr::String` literal, unconditionally, regardless of the
  receiving parameter's declared type — so `@[element(claims = "^Take
(?<n>\d+)$")] fn take(n: int)` could never actually receive an `int`.
  Numeric capture coercion is `docs/prose-dialect-spec.md` §3.5b's own
  Deferred item — the underlying gap stays deferred, not built here — but
  the silence around it is closed: `E171` fires at the mismatched
  parameter's own type annotation, and a handler that fails this check is
  never registered as a claiming handler at all (the same posture
  `E160`/`E166`/`E167` already take), so the offending line is left
  unclaimed rather than rewritten into a call that could never type-check.

  `content`-typed captured params are exempted (not flagged) — the spec's
  own ruled `fn radio(chan: string, text: content)` example and the
  `tier1-native/annotations-element` golden fixture both declare one today
  and compile clean; see `E171`'s own doc
  (`docs/diagnostics/E171.md`) for why.

  `brink-web` transitively depends on `brink-ir`'s native lowering
  (`brink-db::lowered_query` dispatches `.brink`-extension files to native
  parsing/lowering, non-optional), so this new diagnostic is wasm-observable
  for `.brink` projects — a claiming handler with a numeric/struct/generic/
  `fn`-typed captured param now reports `E171` in the editor instead of
  compiling silently wrong.

- 80735d8: Parser: two native-scanner lookahead fixes.

  `content::tag()` and `element::cue_name()` now count consecutive raw
  `BACKSLASH`es before an `L_BRACE` instead of checking only the
  immediately preceding token, so `\\{` (an escaped backslash followed by
  a real, unescaped brace) is depth-counted correctly instead of being
  mistaken for an escaped brace (issue #1852).

  `element::cue_name()` now guards its `COLON` stop with the same
  depth-zero check already used for `R_BRACE`, so a colon inside a
  balanced `{…}` (e.g. `@NAME {a:b}`) is treated as part of the
  interpolation rather than the cue name's terminator (issue #1851).

- 6453c13: Compiler: new diagnostic `E170` (issue #1859) extends `E168`'s
  byte-identical-pattern check to genuinely overlapping (non-identical)
  `@[element(claims = "…")]` patterns. When a later-declared handler's
  pattern is provably subsumed by an earlier one's — every string the later
  pattern can match, the earlier one also matches — and the later handler
  never actually won a claim in the file, it is unreachable under the
  interim first-match-wins dispatch order.

  Subsumption is proven by generating a set of candidate strings from the
  later pattern's structure (recursing into named capture groups and
  expanding every alternation branch) and checking that the earlier pattern
  accepts every one of them — a sound-but-incomplete heuristic, so a missed
  case is a false negative, never a false positive.

- 470cef5: Native bare-name fn values (#1862): in a `.brink` source, a statically-named
  function used in expression position is now a **function value** —
  `map(items, double)`, no sigil — while a call still keeps its parentheses
  (`double(4)`), so reference-vs-call stays unambiguous. This is the 2026-08-01
  ruling; `#fn(…)` is deliberately _not_ given a native spelling, because `#`
  already opens a tag in native content position.

  This fixes a silent mis-compile, not just a missing feature. Until now the
  same bare name lowered to the knot's **visit count**, so `map(items, double)`
  compiled clean and reached the runtime as `map` over an `int`, failing with
  "callback must be a function value `fn(T): U`, got int". Web consumers
  compiling a `.brink` entry (`compileFragment`/session compiles) therefore see
  both a behavior change — such a reference now produces a callable value — and
  one new compile error: a target with a `ref` parameter can never be referenced
  by bare name, because a bare name binds no arguments and every `ref` parameter
  must be bound at creation (E080 at the reference site). The partial-application
  form `#fn(f, a)` keeps no native spelling at all and stays ink-only.

  Ink is untouched: a bare function-knot name in `.ink` source is still a visit
  count, `#fn(…)` remains ink's only fn-value spelling, and the oracle corpus is
  unchanged. Respelling ink into native (`brink-respell`) follows the same split
  — a zero-bound `#fn(f)` now emits as the bare name `f`; the binding form still
  refuses loudly rather than emitting a lambda, whose by-value capture would
  silently differ from a `ref` binding.

- 0d28d28: Direct-call arguments are now type-checked against the callee's declared
  parameter types under `types = strict` (#1864): `h("hi")` with `fn h(x:
int)` now reports `E063` instead of compiling with zero diagnostics. This
  was a pre-existing hole — a call through a function _value_ (T1c) was
  already checked; an ordinary direct call resolving straight to a known
  knot/stitch was not, which made `content`'s (#1846) "never coerces to or
  from string" invariant inert in practice at call sites (`take(mk())` with
  `fn take(x: content)` and `mk()` returning `string`).

  Scoped to arguments `structs::classify_expr_ty`'s existing inference
  substrate can statically classify (literals, call results, index
  expressions, global `VAR`/`CONST` references) and deliberately excludes a
  `~ temp`/param argument whose own type this same call's `observe` join
  already drives to `Ty::Conflicted` — that case already reports `E066`
  separately, so this check never double-reports it. Strict-mode-only;
  `types = gradual` is unaffected and keeps deferring to the existing
  runtime type-mismatch fault. A call through a function value (UFCS or
  otherwise) and a call to an `EXTERNAL` binding are unaffected — those are
  `strict::check_value_calls`'s and `external_check`'s own domains
  respectively.

- ea92b07: New diagnostic: E188 warns when a declared STRUCT's own name collides with
  a reserved builtin/tower type name (issue #1865).

  `annotations::resolve` checks builtin leaves (`int`/`float`/`bool`/`string`/
  `content`/`divert`) and NS-A8 tower kinds (`vec2`/`vec3`/`vec4`/`quat`/
  `mat2`/`mat3`/`mat4`) before it ever consults declared struct names — a
  deliberate ordering (the same one that keeps `int`/`float` unshadowable),
  unchanged by this fix. But a project declaring `STRUCT content { … }`
  previously compiled clean with every `content`-typed annotation silently
  resolving to the builtin, never the struct, and nothing said so anywhere.

  `E188` now fires at the struct's own declaration, naming both the struct
  and the reserved name it collides with. Warning-tier: the declaration
  still compiles and constructs normally (`content#{...}` still reaches the
  struct) — only a bare type annotation spelling the colliding name is
  affected. Does not fire for the generic heads (`List`/`Array`/`Map`/
  `Option`/`Weighted`/`Handle`), `void`, or a name shared with a declared
  `LIST`/registered `Handle<K>` kind — none of those actually collide, verified
  rather than assumed.

- ae3eece: Runtime function evaluation (`begin_function_eval`/`resume_function_eval`)
  now honors a caller-supplied VM step budget instead of always sharing the
  hardcoded 1,000,000-step production ceiling (#1868).

  `WebSpeculation::eval_function`/`resume_function_eval` already parsed a
  `steps` option (`speculate(options)`'s `steps` field, marshaled into
  `Budget.steps`) but silently ignored it for function evaluation — only
  `advance()` honored it. A tiny `steps` budget passed to `speculate()` now
  also caps a runaway (e.g. infinitely recursive) `eval_function`/
  `resume_function_eval` call with the expected step-limit error, instead of
  burning the full production budget before giving up.

  **Consumer-breaking note:** `Budget::default().steps` is 100,000, and
  `speculate()` already fills an unset `steps` option from that default. Before
  this fix, an unset `steps` was silently never consulted by
  `eval_function`/`resume_function_eval`, which instead ran under the runtime's
  hardcoded 1,000,000-step production ceiling. After this fix, a JS caller
  using `speculate({})` (or any `speculate(options)` call that omits `steps`)
  followed by `evalFunction`/`resumeFunctionEval` now gets the 100,000-step
  default applied — a 10x tightening. A legitimately expensive function that
  completed under the old 1,000,000-step ceiling can now fail with a
  step-limit error unless the caller passes an explicit, larger `steps` value
  to `speculate()`.

  Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.

- a6863e3: `brink-analyzer`: a bare `[project] elements` preset name (issue #1874,
  remainder of #1844's item 5) is now validated against a closed built-in-
  preset-name set in `AnalysisOptions::apply_project_config`. An
  unrecognized name (a typo, or a preset that hasn't shipped yet) now
  surfaces a `ConfigWarning` instead of being accepted silently. A
  path-shaped value (e.g. `"conventions.brink"`, `"scenes/conventions.brink"`)
  is never rejected by this check — that shape is a project-relative
  pointer to a custom conventions module, not a preset name.

  The closed set is empty today: no built-in preset has shipped as a real
  `std::conventions::*` module yet (#1720, the screenplay preset, is still
  open), so even `elements = "screenplay"` itself is currently reported as
  unrecognized — every mount that threads `brink.toml` through
  `apply_project_config` (the CLI, `brink ide`, `brink-lsp`, and
  `@brink-lang/web`'s `EditorSession::apply_project_config`) now surfaces
  that warning where it previously surfaced nothing at all.

- 1104a9f: Analyzer: native bare-name fn values now infer a real function type with the
  target's effect row (issue #1876).

  On the native (`.brink`) surface a statically-named function has been a fn
  value since #1862, but inference still typed the reference `Unknown` — so
  nothing downstream could see it. It now types as
  `fn(T…): R` built from the target's signature and carries that target's
  effect row (`FnRow::of_target`), exactly as the ink `#fn(name)` spelling
  already did, and is harvested as an ordinary fn-value creation site (a
  call-graph edge plus the creation atom the effect fixpoint follows).

  Author-visible consequence under `types = strict`: passing a bare function
  name where a non-function type is declared is now an ordinary `E063`
  type-mismatch at compile time — the typo hazard (`total(double)` for
  `total(double(x))`) that the 2026-08-01 ruling accepted an unsigilled
  spelling on the grounds the type checker catches. A bare name handed to a
  declared `fn(T…): R` parameter still checks clean. Ink is unchanged: the
  same bare name there is still a knot's visit count.

- 9243ec0: VAR/CONST/`~ temp` initializers and plain assignments are now type-checked
  against declared type annotations under `types = strict` (#1877, the
  remainder of #1864 left after PR #1875's direct-call-argument half):

  - `VAR v: int = "hi"` and `CONST V: int = "hi"` now report `E063` instead
    of compiling with zero diagnostics — the TM-2 firewall previously let an
    explicit annotation silently _replace_ the initializer's own inferred
    type rather than being checked against it.
  - `~ temp t: int = "hi"` now reports `E063` — the ascription was recorded
    purely as an Unknown-escape fallback, never compared against its own
    initializer.
  - A plain assignment (`~ v = "hi"`) against a target's already-known
    declared type now reports `E063` too: a global `VAR`/`CONST` target
    (never checked before — globals are never joined into the `Ty::Conflicted`
    lattice at all), and an annotated `~ temp` target whenever the
    disagreement wouldn't already be independently reported as `E066` via the
    existing Conflicted-escape join (no double-reporting). A `Param`
    assignment target is deliberately excluded from this new check — a param
    annotation is a signature-firewall slot `annotations::mismatches` (E063)
    already owns, and disagreements there are already reported through it.
  - The global `VAR`/`CONST` check above compares against the declaration's
    full derived type, not only an explicit `: type` annotation: an
    **unannotated** `VAR v = 5` is checked too, since its declared type is
    read the same way as an annotated one (the initializer literal's own
    inferred type) — `VAR v = 5` followed by `~ v = "hi"` now reports `E063`.

  This closes the gap `content`'s (#1846) "never coerces to or from string"
  invariant still had at these positions after #1875 landed the direct-call
  half. Strict-mode-only; `types = gradual` is unaffected and keeps
  deferring to the existing runtime type-mismatch fault.

- f07284d: Fix a live regression: `EditorSession` (and `brink-lsp`) now carries
  `brink.toml`'s `[project] conventions` pointer all the way into the live
  analysis/compile db, instead of silently discarding it after validation.
  Before this fix, every `IdeSession`-backed editor and every `brink-lsp`
  background analysis pass fed the db a hardcoded `None` regardless of what
  `brink.toml` configured. On the `brink-lsp` path this meant
  `external_claim_handlers_query` never saw the conventions module's
  `@[convention]` handlers, so they claimed no prose outside their own file —
  unclaimed scene headings elsewhere fell to `lower_native`'s `E129` arm and
  dropped their whole scene body from analysis (hover/go-to-def/completions
  inside them saw content that did not exist). The confinement gate
  (`E169`, #2289) itself is a `brink-db`-only query neither the LSP's
  background loop nor `IdeSession`'s off-db analyzer path ever calls, so it
  was never reachable from either editor surface — this fix does not change
  `E169` behavior.

  `IdeSession` gains a `conventions: Option<String>` field + `set_conventions`
  setter, mirroring the existing `set_language_dialect`/`set_type_policy`
  wiring; `EditorSession::apply_parsed_config` and `brink-lsp`'s
  `LanguageOptions`/`analysis_loop` now forward the resolved value the same
  way. `@brink-lang/web`'s `explainMatch`/`explainMatchDoc` (issue #2113) are
  also now reachable end to end for the first time — previously they always
  reported "unconfigured" through the editor even when a real conventions
  module was declared.

- a6d86e5: UFCS-desugared calls (`recv.name(args)` → `name(recv, args)`) are now
  argument-type-checked against the desugared free function's declared param
  types under `types = strict` (#1881, the third and final position in the
  #1864 argument-type-checking family — #1875 did direct calls, #1899 did
  declaration initializers and assignments).

  - Both the receiver (the desugar's first positional slot) and every
    written argument are now checked: `g.greet(3)` where `fn greet(name:
string)` is declared and `g` is `int`-typed now reports `E063` instead
    of compiling with zero diagnostics.
  - Covers both desugar shapes: the plain by-value free-call
    (`UfcsVerdict::FreeFnDesugar`) and the D5 auto-ref desugar for a `ref`
    first param (`UfcsVerdict::FreeFnAutoRef`) — a `ref` param's declared
    type is read the same way in both cases (the referent's own type, never
    a separate "reference" type), so a genuine receiver-type mismatch is
    caught through auto-ref too, with no false positive from `ref`-ness
    alone.
  - The resolved free-function target's declared param types come from
    `InferenceResult::signatures`, the same firewall-facing projection a
    direct call's own check reads; the per-argument types are recorded by
    `infer::body`'s existing body walk (the only place argument expressions
    have types) and consumed by `ufcs::UfcsVisitor`'s own resolution pass,
    which already had the resolved target at the point it emits
    `E140`-`E144`.

  Strict-mode-only, reported as `E063` — the same code the direct-call and
  assignment-site siblings use, no new code minted. `types = gradual` is
  unaffected (UFCS is native-only, and native compiles are strict-only, so
  there is no native `types = gradual` compile this check could ever reach
  regardless).

- 3dd7936: Issue #1887: the E119 pure-callback contract gate (`sort_by`/`sorted_by`
  and the `map`/`filter`/`fold`/`filter_map` quartet, `docs/stdlib-spec.md`
  §4/§4b) now also recognizes a **native bare-name callback** — `map(items,
double)`, the sigil-free spelling ruled 2026-08-01 (#1862) for the
  `.brink` surface (`#` is already the tag sigil in native content
  position, so `#fn(target)` has no native spelling). Previously the gate
  matched a callback argument structurally on the ink/brink `#fn(target)`
  literal only, so a native bare-name callback that writes a global,
  performs an effectful call, emits content, or touches the tag channel
  compiled clean instead of being rejected — this only reaches a project
  under brink-dialect analysis over native (`.brink`) source, and only
  changes behavior for a callback argument that provably resolves to a
  statically-named function definition (an opaque reference — a var,
  param, or `bind(…)` result — is unaffected, matching the pre-existing
  exceedance-only posture for the ink spelling).
- f81379d: Analyzer: fix a false `E065` on valid native code — a fn value in
  declaration-initializer position now types (issue #1895).

  `var f = double` on the native (`.brink`) surface has produced a real
  runtime fn value since #1862 (`lir::lower::decls::fold_path_ref` folds it to
  a `FnRef`), and `E080`'s `ref`-parameter obligation has always been checked
  there. Typing was the odd one out: `signature::declared_fn_type` only
  recognised the ink `#fn(…)` literal, so `f` got no declaration-derived type
  at all, the global never reached `collect_globals`, and a later `f(3)`
  classified as an unknown-callee value call — reporting `E065` under
  `types = strict` on an otherwise-correct program.

  The bare-name arm is now gated on the same two conjuncts lowering uses (the
  declaring file is native, and the target is a statically-named function
  definition), so the two sides can never disagree about which initializers
  are fn values when the target is an actual knot (a top-level stitch
  promoted to knot status is a narrower, still-open case — `docs/t1c-spec.md`
  §2a). `f` types as `fn(T…): R` from the target's signature and
  carries the target's effect row, exactly as the body-position spelling does
  since #1876 — a real mismatch through the global is now an ordinary `E063`
  rather than an opaque `E065`. A bare name shadowed by a same-named
  `VAR`/`CONST`/list item still declines to `Unknown`, because lowering
  resolves that name to the shadowing global. Ink is unchanged: `VAR g = f`
  there is still a knot's visit count, never a fn value.

- 19e6cbb: A plain struct-field assignment (`~ p.x = expr`) is now type-checked
  against the field's declared type under `types = strict` (#1900, split from
  #1864/#1877): the field's declared type was never checked before, on either
  of the two root sources #1899 covers for a bare assignment target.

  ```ink
  STRUCT Point = #{x: float, y: float}
  VAR p: Point = Point#{x: 0.0, y: 0.0}
  ~ p.x = "wrong"
  -> DONE
  ```

  used to compile with zero diagnostics under `types = strict`; it now
  reports `E063`. Covers a `VAR`/`CONST` root and an annotated `~ temp`
  root, mirroring #1899's own two root sources. A field name the struct
  shape doesn't declare, or an unresolvable root type, stays silently
  unchecked ("Unknown never disagrees" — not this check's job). Strict-mode-
  only; `types = gradual` is unaffected and keeps deferring to the existing
  runtime type-mismatch fault. Scoped to a plain (non-`ref`) assignment
  target only — a `ref`-mediated field write is a different aliasing
  channel this check does not reach.

- fa52c61: A UFCS method call that desugars to a free function
  (`recv.name(args)` → `name(recv, args)`) now carries that function's own
  declared return type instead of escaping inference as `Unknown` (#1909).

  Previously, `fn f() { let n = 21; return n.double(); }` reported `E065`
  ("`f`'s return type escapes strict inference as Unknown") on a `.brink`
  project with `dialect = "brink"`, while the byte-equivalent
  `return double(n);` compiled clean — identical bodies, only the call
  spelling differing. The `Unknown` also propagated: an `Unknown` reaching a
  call position is what the value-call check's own `Unknown` arm turns into
  a further diagnostic downstream.

  - The desugar's call-graph edge is recorded too, which is what makes the
    target's signature reliably available before the caller is solved.
  - Both desugar shapes are covered: the plain by-value free call and the D5
    auto-ref desugar for a `ref` first parameter.
  - Deliberately unchanged: a prelude verb (`m.len()`), a struct-typed
    receiver (where field access wins over a same-named free function), a
    projected receiver (`a.b.c()`), an ambiguous cross-module name, and a
    wrong-arity call all keep their previous `Unknown` result — none of them
    can be decided without inputs this stage does not have, and guessing
    would risk contradicting the resolution pass's own verdict.

  Observable through the wasm package's `.brink` compile/analysis surface:
  a native project that previously failed to compile on this spurious
  `E065` now compiles, and the inferred signature reported for the enclosing
  function is concrete rather than `Unknown`.

- 21a40e8: #1910: under `types = strict`, a pure verb call
  (`map`/`filter`/`fold`/`filter_map`/`map_each`) with an inline lambda
  callback, and a lambda literal bound straight to a local, both used to
  escape strict inference as `Unknown` even when the callback's own body
  unambiguously pinned the type — `map(items, |x| x * 2)` over `Array<int>`
  reported `E065` on its result, and `let f = |x| x + 1;` reported `E065` on
  `f` itself, unless an enclosing annotation happened to ascribe the same
  type from outside.

  `InferPass::infer_lambda` used to rebuild a lambda's own `fn(T…): R` type
  from written annotations alone once its body walk finished, discarding
  everything the walk itself had learned — the same mono-HM narrowing a
  top-level `fn`'s own params/return already get. Fixed by reading that
  narrowing back (param types from `self.locals`, the return type from the
  tail/`return` statements), shadowed by param name for the walk's duration so
  it neither reads nor leaks through an enclosing same-named local. `fold`'s
  own typing rule also now prefers the seed's type over a callback whose
  return is merely `Unknown` (never over one that is `Conflicted`, which is
  real information).

  Reachable through any `@brink-lang/web` session compiling a `.brink` file
  under `types = strict` (a `brink.toml` `[project] types = "strict"`, or an
  explicit `--types strict`/`AnalysisOptions` request) that calls a pure verb
  with an inline lambda, or binds a lambda literal to a `let`.

- 8f0f38b: Analyzer: `string + int`/`string + float` concatenation no longer reports
  `E066` under `types = strict` (issue #1911).

  `+` between a `string` and an `int`/`float`, in either operand order, is
  ink's core display-concatenation idiom (`"score: " + points`, `keys + ":"

  - total`). The strict checker previously unified `+`as a same-type
operator with no exception for it, so concatenating a numeric value into a
string marked the numeric binding`Conflicted`and reported`E066`on code
that compiles, runs, and produces the correct output today — a false
positive on legal, idiomatic ink.`docs/typed-mode-spec.md`§4 now rules
this pairing as`string`-typed display concatenation, matching the
runtime's own `Add` behavior (`value_ops::binary_op`'s `String`/`Int`and`String`/`Float`arms, which already stringify the numeric operand
unconditionally). The carve-out is scoped exactly to that runtime
behavior:`Add`only, and`Int`/`Float`only —`string + bool`and
string-numeric`-`/`\*`/`/`/`%`still report`E066`, since the runtime
defines no such operation. Covers the `+=` compound-assignment spelling
too (`keys += total`), not just infix `+`— it reaches the runtime's same`Add` arm through a separate inference seam (`Stmt::Assignment`/
`BlockStmt::Assignment`) that needed the identical carve-out.

- 22bac8a: Analyzer: returning an annotated parameter now exports that parameter's
  declared type instead of `Unknown` under `types = strict` (issue #1912).

  `fn passthru(t: content) { return t; }` reported `E065` — "return type
  escapes strict inference as Unknown" — on a return type that is _exactly_
  the annotated parameter type, while the annotated-return twin
  `fn passthru(t: content): content { return t; }` was clean. Handing a
  parameter straight back out lost its type: `ty_of_def` types a parameter
  read from the body walk's own `locals` alone, which an annotation never
  seeded. Filed against `content` (which only became a resolvable type in
  issue #1846) but never `content`-specific — `int`, `float`, `bool` and
  `string` all lost the same way.

  `infer::body::InferPass::infer_return` now runs the returned value through
  `or_own_annotation`, the read-site annotation fallback issue #1168 already
  applies to `some(x)`, `get(m, k)`'s return shape and a `for` loop's
  iterable. A `return` value is joined into the def's return type and never
  `observe`d back onto the expression, so it meets that fallback's stated
  contract: safe only at read sites that produce no counter-evidence.
  `docs/typed-mode-spec.md` §2 now carries the rule.

  The TM-2 annotation firewall is unchanged: the fallback overlays an
  `Unknown` only, so a parameter the body genuinely constrains still exports
  its own independent derivation and `E063` (annotation disagrees with
  inferred usage) keeps comparing two derivations. One consequence is a new
  true positive that could not fire before — `fn f(t: content): string
{ return t; }` now reports `E063`, where the body's `Unknown` used to be
  silently overlaid by the return annotation.

- 329560b: A UFCS call through a struct's own fn-typed field (`recv.field(args)`,
  `UfcsVerdict::FieldCall`) is now argument-checked under `types = strict`,
  closing a gap #1914 (issue #1881) deliberately left uncovered: a wrong-arity
  or wrong-typed call through a field compiled clean with zero diagnostics.
  `E063` now fires for both an arity mismatch and a per-argument type
  mismatch, phrased like the existing "call through a value" (T1c) diagnostic
  family — matching `strict::check_value_calls`'s own wording, since a field
  call is structurally that same "call through a function value" case, just
  reached via field access. Gradual mode is unaffected (this class of static
  check is strict-only, matching the sibling `FreeFnDesugar`/`FreeFnAutoRef`/
  `PreludeDesugar` checks already shipped); a correctly-typed, correct-arity
  field call still compiles clean.
- b42e3e5: A UFCS call into a T1b/NS stdlib prelude verb (`xs.push(v)`, `m.get(k)`,
  `m.insert(k, v)`, and the rest of that family) now gets the same
  argument-domain checking its direct-call spelling already had — the UFCS
  sibling of issue #1881's `FreeFnDesugar`/`FreeFnAutoRef` argument-type
  check (#1919).

  Previously, `m.get(k)` on a project with `dialect = "brink"` and
  `types = strict` reported no diagnostic even when `k`'s statically-known
  type disagreed with `m`'s declared key type, while the byte-equivalent
  `get(m, k)` was already caught. A prelude verb has no `DefinitionId`/
  declared parameter list to compare against, so the check keys the
  expected argument type off the receiver's own inferred container type
  (an array's element type, a map's key/value types) instead.

  Covered: `push`/`heap_push` (array element), `insert`/`get`/`remove`
  (map key, plus `insert`'s value), `index_of`/`contains` (array element
  or map key), `contains_value` (map value). `remove`'s array leg is
  unaffected — that shape stays the existing `E149` migration diagnostic
  (issue #1540), a disjoint diagnostic family from this one.

  Observable through the wasm package's `.brink` compile/analysis surface:
  a native project's UFCS-spelled prelude call with a statically
  disagreeing argument now reports `E063` where it previously compiled
  clean.

- c1ed5cd: Analyzer: a UFCS-shaped call into an `EXTERNAL`/`extern` target is now
  argument-checked through the db-backed inference path, not just the pure
  `infer_project` path (issue #1921).

  `brink_analyzer::solve_scc`'s `known_sigs` was already seeded with each
  `EXTERNAL`'s declaration-derived signature (issue #786), but externals are
  never members of any SCC `batch`, so the `signatures` map `solve_scc`
  _returned_ was filtered down to exactly `batch`'s own members — an
  external's seeded signature never survived past that one call.
  `brink-db`'s `type_inference_query` aggregates every SCC's own
  `SolvedScc::signatures` into the project-wide `InferenceResult::signatures`
  that `ufcs::check_ufcs_arg_types` reads, so on the db-backed path (the CLI,
  LSP, and `@brink-lang/web` all run through this) that lookup always missed
  for an `EXTERNAL` target, and a UFCS call's argument types went completely
  unchecked there — even though the identical mismatch, through the pure
  `infer_project` path, was already caught (that path's `solve_batches`
  sibling returns `known_sigs` wholesale, no batch filter). The direct-call
  spelling of the same call was unaffected either way (it reads
  `ctx.known_sigs`, not `InferenceResult::signatures`).

  ```brink
  extern set_volume(level)

  fn get_name() {
    return "loud";
  }

  fn total() {
    let n = get_name();
    n.set_volume();
  }
  ```

  used to report zero diagnostics under `types = strict` on the db-backed
  path; it now reports `E063` there too, matching the pure path and the
  direct-call spelling `set_volume(n)` already did.

- 540d094: Analyzer: a lambda's value-position read of an annotated param now exports
  that param's declared type instead of `Unknown` under `types = strict`
  (issue #1941).

  PR #1938 fixed `fn f(t: content) { return t; }` — a `fn`'s `return` reading
  an annotated param straight back out now exports the param's declared type.
  The structurally parallel lambda shape was not covered:
  `|t: content| { t }` (a block-bodied lambda's tail) and `|t: content| t`
  (an expression-bodied lambda's sole expression) both still typed `Unknown`,
  even though both are exactly the same "hand a param straight back out" read
  as `return t;`.

  `infer::body::InferPass::infer_lambda` now runs both value-position reads
  through `or_own_annotation` — #1168's read-site annotation fallback,
  already applied to `some(x)`, `get(m, k)`'s return shape, a `for` loop's
  iterable, and (since #1938) a `fn`'s `return`. Unlike a plain `fn`/`flow`,
  which gets its `annotated` fallback map seeded from `def.params` for free
  at pass-creation time, nothing ever seeded a _lambda's_ own param
  annotations into that map — `infer_lambda` only ever shadowed (cleared)
  whatever an enclosing same-named local's annotation left behind. The fix
  also seeds `self.annotated` with the lambda's own resolvable param
  annotations for the duration of its body walk, restored via the same
  shadow/restore mechanism issue #1910 already uses for every other
  frame-scoped map this function touches.

  This seed's reach is the whole body walk, not only the two value-position
  read sites above: `self.annotated` is consulted by `own_annotation`'s
  bare-name fallback at every `or_own_annotation`/`annotated_callee_ty`
  consumer reachable during the walk (an intrinsic's argument-position
  overlay, a `for` loop's iterable, a direct-call callee's own annotated
  type), exactly like a `fn`/`flow`'s own pass-creation seed already covers
  its whole body, not only its `return`s. One exclusion: a param name the
  lambda's own body re-binds via a fresh same-spelled `TempDecl`/`if`/
  `while`/`for` binding is never seeded — `check_declared_assign_target`'s
  `SymbolKind::Temp` arm reads this same map for its own mismatch report and
  cannot tell the param's annotation apart from the fresh local's (absent)
  one, so seeding it would falsely flag the fresh local's own assignment
  against the shadowed param's type.

  The TM-2 annotation firewall is unchanged: the fallback overlays an
  `Unknown` only, so a lambda body that genuinely constrains its param still
  exports its own independent derivation, and a lambda's own explicit return
  annotation (`|t|: T { … }`) still overlays only when the tail/expression
  comes back `Unknown`. `docs/typed-mode-spec.md` §2 now carries the rule.

- 90e0989: Analyzer: `E185`, a plain dotted assignment target naming an unknown struct
  field (issue #1944).

  `~ p.bogus = expr` — a plain assignment to a struct field the shape doesn't
  declare — used to compile clean under `types = strict` with zero
  diagnostics: `check_declared_field_assign_target` (PR #1939) deliberately
  stays silent on an unresolvable field name ("Unknown never disagrees"), and
  `ref_projection::check_strict`'s `E098` only covers an unknown segment in
  `ref`-argument position, not a plain assignment target. Construction
  literals already had this check (`structs::check`'s `E070`); plain
  assignment targets did not.

  `structs::check_field_assign_mismatch` now reports `E185` the moment it
  resolves the receiver's declared shape and finds no field by the assigned
  name — fired only once a real struct shape is known (an Unknown/untyped
  receiver stays silent, unchanged), and only for a single-level dotted
  target (`p.x = v`); a chained target (`o.i.a = v`) never reaches this check
  at all, since LIR already rejects it outright with the non-suppressible
  `E074` regardless of the field name. Reaches both `Stmt::Assignment` and
  the T1b `~ { … }` `BlockStmt::Assignment` form, and both analysis roads
  (`brink-db`'s db-direct `ProjectDb::diagnostics` and the off-db
  `IdeSnapshot::analyze`), since both call the same
  `brink_analyzer::strict_diagnostics` → `structs::check_assignments` seam.

- 217ba82: Fix (issue #1972, second slice): the native `.brink` surface's content-ground
  `~` line escape (`stmt::logic_line`, charter §8.2) now also accepts a
  `~{ … }` multi-statement logic block and a `~ until cond` condition-park
  (native's sole `await` spelling — `await` itself is retired) at prose-body
  position, alongside the assignment/bare-call/temp-decl shapes #1991/#1972's
  first slice already wired. Both lower to the existing `Stmt::LogicBlock`/
  `Stmt::Await` HIR the whole-body `~{ }` override and the ink-dialect's own
  `~ { … }`/`~ await` already produce — a `~{ }` block containing only
  temp-decl/assignment/call/return/`until` statements now parses, lowers, and
  runs; nested `if`/`while`/`for` inside it is a narrower, emitter-only
  residual (still refused, not guessed). Observable through `@brink-lang/web`
  since the wasm package re-exports the native compiler/runtime pipeline.

  Review fix: a call-only `~{ … }` block (e.g. `~{ shout(); }`) now correctly
  terminates its output with a line boundary instead of gluing into the
  content line that follows it — mirrors the trailing-`EndOfLine` rule the
  single-statement `~ let`/`~ x =`/`~ expr` escapes already apply for a
  call-carrying expression. Also refuses to emit (instead of producing
  unparseable source) a code-ground `return -> target` inside a `~{ … }`
  block, since that respelling only has meaning at content-ground/tunnel
  position.

- 4c6c8a5: #1972: native `.brink` source gains a content-ground temp declaration —
  `~ let name: type = expr` — at the same prose-body position `~ x = expr`/
  `~ expr` already used (charter §8.2's logic-line escape, extended). A
  `.brink` file that previously reached for this spelling compiled with
  `~ let` diagnosed as an unrecognized expression atom (`E015`-shaped, from
  `expr::expression`'s fallback); it now parses, lowers, and executes as a
  real temp declaration. `brink_ir::hir::emit_native` (the shared native
  pretty-printer) also gained printer support for all three content-ground
  statement shapes (`Stmt::TempDecl`/`Assignment`/`ExprStmt`), which were
  previously refused outright even though the grammar/lowering for the
  latter two already existed.

  Reachable through `@brink-lang/web`: `brink-syntax-native`/
  `hir::lower_native` are the same parse/lower path the wasm editor session
  (`EditorSession::compile_project` and background analysis) uses for any
  `.brink` document — a project authoring `~ let` prose-body statements now
  compiles instead of diagnosing, and the diagnostic surface for a malformed
  one changes shape.

- 20ab18e: brink-syntax-native/brink-ir: a value-carrying `return <expr>` now parses
  and lowers at content-ground (prose-body) position (issue #1973).

  `parser/divert.rs::return_stmt` previously only recognized a bare `return`
  or the tunnel-redirect `return -> target` at prose-body position — a
  trailing value expression (`return hp > 0`) was left as dangling,
  unreachable content, raising `E033`. It now parses a value expression there
  too, mirroring the code-ground `return expr?;` form (`fn` bodies) that
  already supported one; `lower_native::body` lowers the value into
  `Stmt::Return.value`, and the `brink-respell` emitter (`emit_native.rs`)
  spells it back out instead of refusing with `"return with a value
expression"`.

  This is a pure grammar/lowering/emitter fix, not a semantics change: a
  value-carrying `return` inside a non-function `flow` still correctly fails
  with `E032` ("explicit return outside function") exactly as a bare one
  would — `brink-analyzer`'s existing check is untouched. Only a `return`
  inside a real `fn` newly compiles/round-trips.

- 1adefcc: Fix (issue #1991): the native `.brink` surface's `~ stmt` content-ground
  line escape ("charter §8.2, RULED 2026-07-23: ink's logic line, kept")
  used to compile clean with zero diagnostics and silently print as literal
  story text, never running the statement — `~ n = 5` printed `~ n = 5` to
  the reader and left `n` unchanged. `~ stmt` now parses and lowers to a
  real assignment (`=`/`+=`/`-=`) or a bare expression statement (e.g. a
  function call), the same as the ink-dialect frontend's own logic line.
  Observable through `@brink-lang/web` since the wasm package re-exports the
  native compiler/runtime pipeline.
- 814276c: Fix (issue #1992): the native `.brink` surface's `> text` code-ground line
  escape ("charter §8.2, RULED 2026-07-23: `>` emits a prose line inside a
  code body") used to be accepted by the parser but had no HIR lowering,
  failing with a loud `E129` diagnostic for every token on the line. `>
text` inside a `fn`'s default (or a `flow`'s `~{ }` "Compound guard"
  override) code-ground body now lowers to real content emission — the same
  output the whole-body `>{ }` selector already produced, at line
  granularity — the mirror image of issue #1991's `~ stmt` fix at the
  opposite ground. The escape also parses inside a nested `if`/`while`/`for`
  body, but still lowers loudly (`E129`) there in this slice — a
  deliberately narrower first cut, not a silent gap. Observable through
  `@brink-lang/web` since the wasm package re-exports the native
  compiler/runtime pipeline.
- e976041: Issue #1993: `RuntimeError::RanOutOfContent` is now a tuple variant
  carrying a `RanOutOfContentCause` (`Tunnel` / `Function` / `Plain` /
  `Unknown`) instead of a bare unit variant — a breaking change for any
  consumer matching the old shape. The four messages mirror C#'s
  `Story.cs` call-stack selection (`CanPop(Tunnel)` / `CanPop(Function)` /
  `!canPop` / backstop) word-for-word.

  `RanOutOfContentCause` is exported alongside `RuntimeError`.

  In practice only `Plain` is reachable through any story today — its
  message text is byte-identical to the old unit variant's, so this ships
  with no behavioral change for `@brink-lang/web` consumers. The other
  three causes classify correctly at the instant a frame's content is
  discovered exhausted, but the classification is only ever persisted for
  the exhaustion that actually produces the terminal `Done`: this runtime's
  own frame-popping (unlike C#'s) always unwinds an exhausted Tunnel frame
  even with nothing pending, so a Tunnel or Function frame's exhaustion
  cascades down to the root frame's own `Plain` exhaustion before the
  deferred fault ever reads it (tracked in #2005; see
  `tunnel_fall_off_classifies_as_plain_not_tunnel_today` /
  `function_fall_off_classifies_as_plain_not_function_today` in
  `crates/brink-runtime/tests/terminal_classification.rs`).

- c1be12d: Lambda annotation precedence + eager incompatibility diagnostic (issue #1994, closing #1932, RULED 2026-08-01). Observable through `@brink-lang/web` under `types = strict`, brink dialect only (lambda syntax is native-only):

  - A lambda's own **written** parameter/return annotation (`|k: int|: int { … }`) now governs that slot's resulting type unconditionally, narrowing #1910's body-derived read-back to the **unannotated** case only. Previously, a wrong body derivation could silently override a correct written annotation with no diagnostic anywhere.
  - A body-derived type that disagrees with the lambda's own written annotation is now reported as a new diagnostic, **`E174`**, raised eagerly at the lambda's own declaration — not deferred to wherever the lambda is later called. Unlike the gradual/advisory `E063`, `E174` is `Error`-severity by default and not `[lints]`-downgradable.
  - A param the lambda's own body re-binds (`|t: int| { let t = "a"; … }`) is excluded from this precedence change (falls back to the pre-#1994 posture unconditionally) — the shadowing local's type is not the param's own narrowing.
  - Unannotated params/returns are unaffected: #1910's fix (body-derived wins) is unchanged.

  See `docs/typed-mode-spec.md` §2 for the full ruling, reconciled against the "annotation = firewall" wording alongside the existing top-level-`fn`/`flow` precedence rule.

- 260a94a: Issue #1995/#1920 (ruled 2026-08-01): `ref` parameter arguments are now
  checked **invariantly**, not covariantly. `assignable(Float, Int)` is
  `true` (by-value widening), so `fn scale(ref x: float)` called with an
  `int` cell used to be accepted — the callee then writes a `float` back
  through a cell that is statically declared `int`, an unsound write-back.

  Both by-ref call-checking sites now use a new invariant predicate
  (`ref_assignable`, requiring the argument's type to match the parameter's
  declared type exactly, still row-insensitive):

  - The direct-call argument check (#1864/PR #1875).
  - The UFCS-desugared argument/receiver check (#1881/PR #1914) — covers
    both the receiver slot (D5 auto-ref) and any later `ref` parameter.

  This is a `.brink`-dialect-only, native-surface change (vanilla ink has
  no `ref` parameters or UFCS calls to reach it) that **rejects some code
  that compiled before this fix** — a widening `ref` argument now reports
  `E063` under `types = strict`, the same code the covariant checks already
  used. Observable through `@brink-lang/web` because the wasm package
  re-exports the same diagnostics.

- 2a4b311: Native `.brink` prose-dialect markup: span tag names may now contain `-` as
  an internal separator (`<fade-in>`), issue #1996 (RULED 2026-08-01,
  `docs/prose-dialect-spec.md` §4.1). Both the open (`<fade-in>`) and close
  (`</fade-in>`) forms are supported; a leading or trailing hyphen (`<-x>`,
  `<x->`) is still a parse error. This is scoped to span-tag position only —
  plain identifier lexing elsewhere in the language is unchanged. Before this
  fix, a hyphenated tag name failed to parse (`expected GT, found MINUS`).
- 422d968: #1997 (ruled 2026-08-01, closing #1780): the host capability manifest's
  `markup` section gains a required-attribute flag, and its `attrs` schema
  widens to make room for typed attribute values later without another
  breaking change.

  - **(a) Required attributes.** Each declared attribute
    (`ManifestSpanKind.attrs`) can now carry `required: true`. A span of a
    declared kind that omits one of that kind's required attributes reports
    the new `E173`, gated the same way `E164`/`E165` already are (only for a
    span whose name the manifest declares, one diagnostic per missing
    attribute) and defaulting to `Warning` for the same `[lints]`-
    configurability reason.
  - **(b) Widened attribute schema — headroom, not typing.**
    `ManifestSpanKind.attrs` moves from `Vec<String>` (bare attribute names)
    to `Vec<ManifestSpanAttr>` (`{ name, required }`, plus a reserved,
    currently-inert `ty` slot). **This is schema headroom only — attribute-
    value typing is NOT implemented.** Span attribute values stay static text
    by construction; the reserved slot exists only so a future PR that adds
    typing needs a new check, not another manifest shape change.

  **This is a breaking wire-format change to the `markup` section itself**,
  observable through `@brink-lang/web`'s `EditorHandle.setHostManifest` /
  `ManifestSpanKind`/`ManifestSpanAttr` TS types: a bare attribute-name array
  (`"attrs": ["amount"]`) is no longer accepted — hosts must migrate to
  `"attrs": [{ "name": "amount" }]`. See `docs/host-capability-manifest.md`
  § "Markup vocabulary" for the updated shape.

  Oracle ratchet unaffected (tooling/author-time manifest validation only,
  never consumed by the runtime or codegen).

- 881726e: Issue #2001 (the tracked remainder of #1995/#1920 after PR #1999): the
  `#fn(target, args…)` partial-application creation site now checks its
  by-ref bound arguments invariantly too.

  `infer_fn_literal` (the `#fn` literal's own bound-argument loop) recorded
  call-graph edges, fn-value creation, and `ref`-param write tracking, but
  performed **no argument-type check at all** — the exact soundness hole
  #1995/#1920 closed for the direct-call and UFCS-desugared sites, in a
  third spelling: ink `VAR i = 3` + `~ temp f = #fn(scale, i)` against
  `function scale(ref x: float, k: int)` used to yield zero diagnostics.
  `#fn`'s own `fn_values::check` (`E080`) only verifies a `ref` position is
  bound to _some_ durable cell, never that the cell's static type agrees
  with the declared `ref` param type, so this was a genuinely separate gap.

  A `ref`-bound argument whose type does not match the declared `ref`
  param type exactly (`infer::ty::ref_assignable`) now reports `E063` under
  `types = strict`, mirroring the direct-call/UFCS checks' `ref`-arm
  handling (this loop only ever checks `ref`-bound arguments, so its
  observed-local carve-out is always the partial one, never the full skip
  the direct-call check's non-ref arm uses).

  By-value (non-`ref`) bound arguments at this creation site are
  deliberately left unchecked — `infer_fn_literal` has never had a
  by-value argument check either, and #2001 named that as new checking
  needing its own scope call, not an assumed yes.

  This is a `dialect = brink` (extension-syntax) change gated at the
  `#fn` literal itself, which is brink-extension syntax — vanilla ink
  does have `ref` parameters (e.g. `function alter(ref x, k)`), but an
  ink-dialect file has no param type annotations and no strict policy,
  so there is no declared type for an argument to disagree with, and
  `#fn` isn't reachable from a vanilla-ink file at all. Observable
  through `@brink-lang/web` because the wasm package re-exports the same
  diagnostics.

- 9c211d5: Issue #2004: `!name` line-start sigil dispatch, the self-announcing half
  of the §9.1 conventions-handler dispatch split. A content line beginning
  `!name` now dispatches by name (or its `@[element(name = "alias")]`
  override) to a top-level `fn` annotated `@[element(args = "…")]`, binding
  the pattern's named captures to the handler's params by name and
  rewriting the line to exactly one call — the same mechanism `claims =
"…"` natural-notation dispatch already uses, minus the pattern match.

  Composes with `\!`, the ruled line-start escape (§8d.6) — an escaped `!`
  never opens a dispatch. A dispatch whose name is undeclared, whose
  remainder doesn't match the handler's pattern, or whose remainder isn't
  wholly literal falls through to the existing loud `E129` ("parses
  cleanly but has no HIR lowering yet") rather than silently reading as
  plain prose.

  Not in this slice: dispatching to a `flow` target (only a top-level `fn`
  dispatches, matching `claims`'s own restriction), the `block` capture's
  dispatch mechanism (issue #1839), cross-file dispatch-name resolution,
  and the ruled duplicate-dispatch-name/unmatched-remainder diagnostics
  (both interim — first-declared-wins and the generic `E129` fallback,
  respectively — pending a diagnostic-code allocation).

- a4f14ba: Issue #2045: a _recognized_ inline escape (`\< \{ \# \\`, §8d.6) now
  strips its backslash from a tag's rendered text, in parity with ordinary
  content — this is a **breaking change** for any `.brink` file relying on
  the backslash surviving into rendered tag text.

  `content::tag()`'s raw free-text scan already gave `\#`/`\{` _structural_
  recognition (#1738/#1852: an escaped `#`/`{` doesn't end the tag early),
  but never stripped the backslash from the tag's own materialized text —
  so `Hello \# world #a \#b` produced `Hello # world` for the content line
  (backslash stripped, via `markup::escape`) but `a \#b` for the trailing
  tag (backslash retained) — two different treatments of the same escape on
  one line. Fixed at the materialization point (`ast::Tag::text()`, shared
  by `hir::lower_native::body::lower_tag`): a recognized escape's backslash
  is now stripped there too, so the tag observed through `Line::Text`'s
  `tags` field reads `a #b`.

  Migration: a `.brink` file whose tag text contains a recognized escape
  (`\#`, `\{`, `\<`, or a bare `\\`) and depends on the backslash surviving
  into the rendered tag will see it disappear (a bare pair collapses from
  two backslashes to one). To keep one literal backslash immediately before
  a literal `#`/`{`/`<` in the _same_ tag, use three backslashes (e.g.
  `\\\#`, not `\\#` or `\\\\#`): the odd count is what keeps the following
  character from ending the tag early (unchanged structural parity,
  #1738/#1852), and the materialized text then collapses the leading pair to
  one backslash while the trailing backslash escapes the final character —
  `#tag \\\#more` renders as `tag \#more`. An even count (`\\#`, `\\\\#`, …)
  still ends the tag at that unescaped character exactly as before this fix,
  splitting into a new sibling tag instead. This matches what ordinary
  content already requires for the same effect.

- 92eb241: Fix (issue #2056): a `flow`'s whole-body `~{ }` "Compound guard" override
  and a `fn`'s default code-ground body (both lowered by
  `hir::lower_native::body::lower_stmt_block_as_body`) now correctly
  terminate a call-containing statement run's output with a line boundary
  instead of gluing it into whatever content follows. This is the same
  output-boundary defect PR #2055 fixed for the single-statement
  content-ground `~` escape (`lower_logic_line`'s `needs_eol` rule) — this
  fix reaches the structurally distinct sibling call site
  (`flush_code_ground_run`), which built `Stmt::LogicBlock` directly and
  never went through `lower_logic_line`, so it never inherited that fix.
  Observable through `@brink-lang/web` since the wasm package re-exports the
  native compiler/runtime pipeline.
- a7556a5: Issue #2077: a scene heading's `@[convention(claims = "…")]` pattern now
  matches even when the heading carries an explicit `[slug]` and/or trailing
  `#tag`s — before this fix, the compiler's natural-notation claim dispatch
  (`hir::lower_native::element::candidate`) declined a slug- or tag-bearing
  heading outright, so a preset's `heading` handler could never claim any of
  `docs/prose-dialect-spec.md`'s own worked-page examples (every one of them
  spells an explicit slug).

  - The pattern still only ever sees the heading's title text — the
    `[slug]`/`#tag`s are stripped before matching, not appended to it, so no
    existing preset pattern needs to change.
  - The slug is now captured and delivered on `HirFile::element_matches` as
    a reserved capture (`ElementMatch::slug`) — tooling-visible, but not
    wired into the rewritten call (that remains heading→stitch promotion,
    issue #2078, a separate unowned issue).
  - The heading's own trailing tags now reach `Content.tags`, the same
    channel any other tagged line already uses, instead of being silently
    dropped once a slug/tag-bearing heading became claimable.
  - The built-in screenplay preset (`std/conventions/screenplay.brink`,
    mounted into every compiled project's `Environment` manifest since
    #2080) is directly affected: its `heading` handler can now claim a
    slugged heading end to end (`scene_entered`'s `slug` argument stays an
    empty string either way — wiring the captured slug into that call is
    #2078's territory, not this fix's).

  This changeset is filed because the claim/decline behavior change is
  compiler-level (`brink-ir`), and `@brink-lang/web` re-exports it through
  every native compile that runs a project with `@[convention(claims = …)]`
  handlers.

- ef4d386: Issue #2079 (`docs/decision-log.md` 2026-08-06 "Compact cue desugars to cue

  - content line"): a compact cue (`@NAME: dialogue`, `docs/prose-dialect-
spec.md` §8b.9) is now a claim candidate — `hir::lower_native::element::
candidate` widens with a `COMPACT_CUE` arm alongside the existing `CUE`/
    `SCENE_HEADING`/`PARENTHETICAL` ones.

  * `@[convention(claims = "…")]` matching is offered only the compact cue's
    **name segment** (its `CUE_NAME` sub-node) — exactly the same text a
    block cue's own `@NAME` line offers — never the fused dialogue. Before
    this, `COMPACT_CUE` was invisible to `candidate()` entirely and every
    compact cue fell to the loud `E129` default regardless of what any
    project or preset declared.
  * The fused dialogue lowers as an **ordinary content line**, landing
    inside whatever run the matched handler's `attach`/`block` flavor
    captures (or, for a plain handler, right after its own call) — it keeps
    full interpolation rights, since literalness only ever applies to the
    name segment. It does not, however, get a free pass on structure: a
    dialogue carrying a fused `LABEL` (a leading `(word)`) or a fused
    `DIVERT_STMT`/`TUNNEL_CALL`/`CHOICE_POINT` (a trailing `->`/`->->`/`{?}`)
    declines the WHOLE claim (loud `E129`) rather than being silently folded
    into the captured run, matching what `capture_block`'s own terminator
    search already requires of an ordinary sibling line.
  * Observable consequence for `std::conventions::screenplay` (mounted into
    every compiled project's `Environment` manifest since #2080): `cue`
    (attach mode, issue #2166) now claims `@NAME: dialogue` the same way it
    claims a bare `@NAME` line — the compact form's dialogue carries `cue`'s
    `speaker` attach data (`OutputLine.element.data`) exactly like a block
    cue's own following dialogue does.

  No `use std::conventions::screenplay` import path exists yet (#2167/#2198),
  so this preset is still only reachable by inlining its source — this
  changeset is filed because the mounted preset's own lowering shape
  changed, which `@brink-lang/web` re-exports through the `Environment`
  manifest every compile mounts it into.

- e44f1fa: #2080: `brink-environment`'s `Project::load` now mounts the built-in
  stdlib source (`std/conventions/screenplay.brink`, embedded at compile
  time via `include_str!`) into every `Environment`'s manifest, alongside a
  project's own sources. This is the compiler's sole production compile
  path (`brink_environment::compile`), which `@brink-lang/web`'s wasm
  `compile.rs` entry point goes through directly — so every wasm compile
  now sees one extra source key (`std/conventions/screenplay.brink`) join
  the manifest. What that key's presence _does_ on a given compile depends
  on the entry's dialect: for a **native** (`.brink`) entry, discovery is
  tree-is-universe, so the mounted module joins the compilation closure and
  is compiled as an ordinary native module alongside the project's own
  files. For an **ink** (`.ink`) entry — `@brink-lang/web`'s ordinary
  case — the closure instead follows the entry's `INCLUDE` graph, which has
  no edge into the mounted key, so it stays manifest-only: present, never
  lowered, contributing nothing to that compile.

  This is a **mount only**: the stdlib module is present in every
  compiled project's manifest, and — for a native entry — its module
  identity mints exactly as it would for a project file at that path; but
  nothing in it is marked `pub` and no confinement rule scopes imports into
  it yet. There is no `use std::…` surface reachable from a project's own
  source in this PR (that needs #1582's pub marker and #2167's
  closure-scoped confinement, tracked separately). A project whose own
  source happens to already use the same key
  (`std/conventions/screenplay.brink`) is unaffected — its own file wins
  over the embedded copy rather than being silently overridden.

- b2b1ad5: Analyzer: a fn-valued global `const`'s call site now resolves (issue #2083).

  Calling a fn-valued global `const` from anywhere other than its own
  declaration — either the bare-name form (`const twice = double`, #1862) or
  a lambda-literal decl default (`const twice = |x| x * 2`, #1774) — used to
  fail with `E025` ("unresolved variable reference"). RCA found the bug was
  never a `brink-db` incremental-resolution gap, despite the issue's own
  report suspecting one: `brink_analyzer::resolve::resolve_function`'s
  call-site "try variables" lookup searched only `SymbolKind::Variable`,
  never `SymbolKind::Constant` — a `var`-bound fn value's call site already
  resolved (`resolve_variable`'s own bare-_read_ lookup already searches
  `[Variable, Constant]` together; the call-site lookup was a one-sided
  omission). Fixed by adding `Constant` to that lookup, so both the
  `brink-db` db-direct road and the off-db `IdeSnapshot::analyze` road agree.

- f5395de: `comparator_contract`'s `E119` pure-callback-verb gate (`sort_by`/
  `sorted_by`/`map`/`filter`/`fold`/`filter_map`, issue #1110/#1679) now
  checks file-level `VAR`/`CONST` initializer expressions, including a
  decl-default lambda's own body (`const doIt = || map(xs, impureCallback)`,
  legal since #1774). Previously `collect_sites` started only from
  `root_content` + knot/stitch bodies, so an impure named callback written
  directly in a declaration initializer — or nested inside a decl-default
  lambda's body — silently compiled clean instead of being refused. Issues
  #2085/#1769.
- c3ac050: Issue #2091: an empty `content`/Fragment interpolation no longer renders
  its own blank output line. This covers TWO distinct call sites that
  produce a `Value::FragmentRef` in this position, and the fix cannot
  distinguish between them — both are suppressed identically:

  - a `block`-capturing handler (issues #1838/#1839) whose captured run is
    empty — most commonly a cue immediately followed by a parenthetical,
    where `hir::lower_native::element::capture_block`'s terminator ends the
    run at zero interior lines; and
  - an ordinary **display-position call-composition** slot — e.g. a line
    whose only content is `{ f() }` — where `brink-codegen-inkb::content::
emit_slot_expr`'s `BeginFragment`…`EndFragment` wrapping (emitted for
    _every_ template slot whose expr is a function call, both dialects) also
    produces a `Value::FragmentRef`, and `f` emitted no side-effect text and
    returned an empty value.

  Either way, interpolating that fragment alone on a template line used to
  still consume a visible blank line between real content, both in
  `continue_single`'s streaming `Line`-at-a-time API and in
  `continue_maximally`/`flush_lines`'s batch form.

  Fixed at the output-resolution layer
  (`brink-runtime::output::resolve_lines`/`take_first_line`), not at the
  line table: a resolved line is suppressed only when its text comes out
  empty, it carries no tags, _and_ at least one of its parts interpolated a
  `content`-typed value that itself rendered empty (not necessarily
  "captured nothing" — a captured line that itself renders empty, or a
  call-composition fragment whose function simply returned `""`, both reach
  the same state). The compiled line-table entry a suppressed line's
  `LineRef` points at is untouched — present-but-empty, not omitted or
  renumbered — so locale hot-swap (which matches a swapped-in line vector to
  the transcript by index) keeps working unchanged.

  Deliberately scoped to exactly this case: a line that resolves empty for
  any other reason — a literal blank line, or a self-closing inline markup
  span (`<pause/>`) with no children — still renders its pre-existing blank
  beat (see the `inline-markup-point-marker` fixture, issue #1716), which
  this issue explicitly treats as a separate, already-settled question.

  `tests/tier1-native/conventions-screenplay-preset/`'s golden fixture
  (from issue #1720/PR #2081, whose `expected.txt` had pinned the stray
  blank line as-is) is updated to reflect the corrected output.

- 0d17b32: Analyzer: UFCS resolution now reaches a decl-default lambda's own body
  (issue #2096).

  `ufcs::resolve` used to drive its `UfcsVisitor` with plain
  `hir::visit::visit`, which never reaches a file-level `VAR`/`CONST`
  initializer — so a UFCS-shaped method call written directly inside a
  decl-default lambda's own body (`const callGreet = |g| g.greet(3)`, legal
  since #1774's ruling) was never visited by the pass at all and fell through
  to LIR lowering's defensive `E144` refusal instead of being analyzed for
  real.

  `ufcs::resolve` (and its `project_has_ufcs_call` laziness gate) now drives
  the same `HirVisitor`-shaped visitor with
  `hir::visit::visit_with_decl_initializers` — the shared entry point issue
  #1571/#2098 built for exactly this class of gap — so this call site is
  analyzed like any other. A receiver whose type resolves (e.g. an annotated
  lambda param) now resolves and runs the desugared call for real; an
  unannotated receiver with nothing else constraining its type still refuses
  to compile, but now with the accurate `E142` ("annotate the receiver")
  rather than the old defensive, structurally-caused `E144`.

- 60b83cd: #2098: migrated six analyzer passes (`coalesce`, `contains_domain`,
  `conversions`, `map_keys`, `structs`'s two visitors, `range_refinement`)
  from a hand-rolled second walk of `VAR`/`CONST` initializer expressions
  onto the shared `hir::visit::HirVisitor` entry point
  (`visit_with_decl_initializers`), which now grows two new hooks
  (`enter_var_decl`/`enter_const_decl`) so a stateful visitor can reset its
  own per-declaration bookkeeping.

  **No behavior change intended or observed.** This is a pure internal
  refactor: every pre-existing test for all six passes' decl-initializer
  diagnostics passes unchanged, a new regression test for `coalesce`'s
  diagnostic-anchor bookkeeping was added and verified to fail without the
  fix, and the oracle ratchet holds at exactly 5607/5607 episodes (compared
  directly against unmodified `origin/main`, which shows the identical
  365/397 case count — confirming the small case-level drift already on
  `main` predates this PR). Filed as a patch changeset per this repo's
  standing rule that any crates-only PR touching analyzer-pass internals
  gets one, since `@brink-lang/web` re-exports the same diagnostic engine.

  `comparator_contract.rs` and `ufcs.rs` are **not** migrated in this PR —
  both were found, while scoping this work, to carry a real latent gap a
  naive migration would silently paper over (documented in the PR body and
  tracked as a follow-up).

- 736e8d4: Format: `SaveState`'s suspended-flow section (`SuspendedFlow`, FS-1) gains
  two fields — `next_block_id` and `pending_element` — per the 2026-08-05
  ruling on issue #2108 ("block metadata persists, and `next_block_id`
  persists with it"), for two independent reasons. Element-attachment data
  (`@[convention(..., attach = X)]`, #2260) accumulates only in the VM
  output buffer's `pending_element`/transcript, which does not survive a
  park, so a flow parked (`await`) inside an open attach run needs
  `pending_element` to avoid silently resetting the attributed
  speaker/metadata to empty on resume. `next_block_id` needs to persist on
  its own account regardless of attachment: restarting it at 0 would give
  the same uninterrupted run a different id after resume (and could collide
  with ids already emitted), breaking `BlockId`'s "same id iff same
  uninterrupted run" contract. Both fields carry `#[serde(default)]` (an
  older save decodes as `0`/empty, identical to pre-#2108 behavior) and
  `pending_element` uses `skip_serializing_if` to omit the key entirely when
  no run was open at park time, so the common case's wire form is
  unaffected.

  Format-only, matching the rest of `SuspendedFlow`: the FS-2/FS-3 compiler
  synthesis and runtime spill/restore that would populate or consume a
  _live_ value are still unbuilt, so `Story::save_state`/`load_state` (and
  therefore every current `@brink-lang/web` save/load call) still always
  produce/consume `suspended: None` — no observable runtime behavior
  changes today. The changeset is filed because the wire shape of a type
  `@brink-lang/web` re-exports (`SaveState`) changed.

- 4dcafc9: `brink-runtime`: `OutputLine.element.data` is now populated for `attach =
StructName` convention handlers (issue #2108, the element output model
  ruled 2026-08-03). An attaching convention (`cue`, `parenthetical`) consumes
  its own claimed line — no `Step::Line`/event for it at all — and its
  returned struct's fields merge into the run that follows, with every line
  materialized while the run is open carrying a copy. `Element.kind` is
  unchanged (`"narrative"` regardless); classifying `kind` itself for a
  non-attach single-line handler (`heading`/`transition`) remains unbuilt.

  Two new bytecode opcodes (`AttachElement`/`EndElementRun`) and two new
  `OutputPart` variants carry this — the latter deliberately transient
  (never reach the persisted `.brkt` transcript format, matching
  `Checkpoint`'s existing precedent), so this is in-memory-only for now; a
  save/resume story for `Element.data` has not been designed.

  `brink-web` re-exports `OutputLine`/`Element` through the same marshal
  legs #1684 built (`LineJs`/`ElementJs`, `@brink-lang/web`'s
  `Line`/`SessionLine` TS types) — a `.brink` project using `@[convention(...,
attach = StructName)]` now sees non-empty `element.data` on the wasm
  surface for the first time. The disassembler view (`program_model.rs`)
  also gained the two new opcodes' mnemonics (`attach_element`/
  `end_element_run`).

- 06cacc4: Issue #2113 (NS-T seam 3/6): the explain-match query, discharging the
  "no invisible expansion" compensation for conventions-claimed prose
  lines — for any line, whether it's matched, by what handler (fn name +
  declaration location), what it bound (captures as byte spans), the
  patterns attempted on a miss (registration order), and any other
  handler shadowed on a hit.

  - **`brink_ir::explain_match`/`ExplainMatchCache`** (new, `brink-ir`):
    a pure composition over #2112's `classify_line` output and #2111's
    `ConventionsProjection::entries` — no second walk. `ExplainMatchCache`
    memoizes on `(line text, projection)` and additionally caches the
    _compiled_ pattern set per projection (the w133 perf finding on PR
    #2257: `classify_line` compiled a fresh `Regex` per call, per entry).
  - **`EditorSession::explain_match`/`explain_match_doc`** (new,
    `@brink-lang/web`): the wasm binding, wrapping a per-session
    `ExplainMatchCache`. Returns JSON with **raw byte ranges** throughout
    (not this crate's usual UTF-16) — a matched handler's declaration
    range lives in the project's conventions module, a file this session
    may never have opened, so there is no single file to convert against;
    see `editor/explain_match.rs`'s own doc.
  - **`EditorSessionHandle.explainMatch`/`explainMatchDoc`** (new,
    `@brink-lang/web`'s TS wrapper) mirror `getHover`/`getHoverDoc`'s
    shape and re-export the new `ExplainMatch`/`ExplainClassifiedMatch`/
    `ExplainAttempted`/`ExplainHandler`/`ExplainCapture` interfaces from
    `@brink/wasm-types` — every range on them stays raw-byte and
    **file-absolute** (not adjusted for a fragment view set by
    `setViewContext`/`openFragment`); see the docstrings on the two
    methods for the caveat.
  - **`ElementKind` ("matched kind") composition is deliberately deferred**
    — `crate::ExplainMatchCache`'s own module doc explains why: the one
    function that derives it reads a parsed CST node with surrounding-line
    context (a parenthetical is chain-gated on the preceding line being a
    live cue), which this query's bare-text entry points cannot supply.
    Left as a follow-up for a caller holding a real parsed document.

  ⚠ **Reachability caveat, discovered while writing this PR's own
  end-to-end test, pre-existing and not introduced here:**
  `brink_ide::session::IdeSession::analysis_options` hardcodes
  `conventions: None` on every call — `EditorSession::apply_project_config`
  validates `[project] conventions` far enough to warn on an unrecognized
  value, but never wires it into the live `ProjectDb`'s real
  `AnalysisOptions`. So `conventions_projection()` (and therefore this
  query, and the pre-existing `E169` confinement diagnostic) is always
  empty through the `EditorSession`/wasm editor path today, for every
  project configured the only way an embedder can. The query itself is
  proven correct against real project data one layer down
  (`brink-db`/`brink-ir`); see the PR description for the follow-up issue
  tracking the `IdeSession` wiring gap.

- 50c1107: Issue #2115 (NS-T seam 5/6, backported design from #2111–#2115's
  2026-08-03 "Conventions × the editor" ruling): `DialogueDialect`
  (#368)'s surviving `transitions`/`templates` fields — Tab/Enter/Shift-Tab
  succession rows and template/picker metadata, the editing-time dual of
  chain rules — now **re-key against declared convention kinds instead of
  carrying an independent element list**, and `brink_ir::ConventionsProjection`
  (the compiler's `@[convention]`-handler projection) gains a
  `with_succession` method plus `transitions`/`templates` fields for
  validating succession rows against the projection's declared convention
  kinds. The compiler never interprets them (§5 of
  `docs/prose-dialect-spec.md`, "ignored by the compiler"); per the
  2026-08-05 ruling _"Succession is EDITOR-OWNED and externally defined"_
  (PR #2304), they stay in-process validator state and are never carried
  into a serialized wire shape.

  - **Observable behavior change, `set_dialect`:** `brink-web`'s
    `set_dialect(json)` calls the same `brink_ir::dialect::validate` this
    slice extends — a `DialogueDialect` JSON payload whose `templates`
    array names a `kind` that `elements` never declared (and that isn't a
    reserved structural kind) is now rejected with a `JsError`
    (`DialectError::TemplateUndeclaredKind`), where it previously validated
    silently. `transitions` was already checked this way (reported as
    `DialectError::TransitionUndeclaredKind`); `templates` was not — this
    closes that gap for both callers of the shared `validate_succession`
    helper at once, each kind of row now reported under its own error
    variant.
  - **New API surface (brink-ir):** `ConventionsProjection::with_succession`,
    `dialect::validate_succession`, and `dialect::reserved_structural_kinds`
    are now exported from the crate root alongside `Templates`,
    `TemplateEntry`, `TransitionAction`, `TransitionRow`.

  Scope fence held: this is validator-only, in-process state — it never
  travels beyond tooling. Actually wiring Tab/Enter succession in CM6 stays
  held as editor-frontend work (NS-T hold, 2026-08-01 sequencing ruling).

- 52e6809: Fix #2121: `push(a.items[0], v)` and `a.items[0] = v` — a **Path-then-Index**
  lvalue whose root is itself a struct-field projection (`a.items`, `a: Bag`,
  `Bag.items: Array<Array<int>>`/`Array<int>`) — used to compile clean and
  silently misroute the write onto the _root_ variable `a` instead of
  `a.items[0]`, faulting at runtime with `NotIndexable("record")` — the "one
  level down" remainder of #1495/PR #2106's fix: a bare `ident.ident` chain
  always parses as one multi-segment `hir::Expr::Path` (never
  `hir::Expr::FieldAccess`), and wrapping that `Path` in an `Index` reaches
  `lower_indexed_assignment`/`lower_lvalue_container_chain` — a different
  call chain than #2106's fix, which only taught the _bare_ Path-lvalue
  dispatch about this shape.

  Both call sites now reject this shape with the same non-suppressible
  `E074` `try_lower_field_assignment`/`lower_mutator_call` already raise for
  a chained field write/mutator, rather than falling through to the same
  silent misroute.

- 22540ca: Fix #2122: `if get(bag) as b { b.items = […] }` and `if get(bag) as b { push(b.items, 1) }` — an as-binding's struct-field write/mutator — used to compile clean and silently mutate the (supposedly immutable) binding, instead of raising the `E148` every other write shape (plain/compound assignment, indexed-assignment root, bare in-place mutator, `ref`-argument passing) already raises.

  `lower_single_level_field_write` and `lower_field_mutator` (`crates/internal/brink-ir/src/lir/lower/blocks.rs`) each resolve a `Param`/`Temp` root's slot themselves (`ctx.temp_slot(&head_name)`, the _head_ of a two-segment `p.field` path) rather than routing through `stmts::lower_assign_target` — the choke point that already refuses a write to an `as`-binding slot for every other shape. Their root is the head of a two-segment path, not the whole assignment target `lower_assign_target` resolves, so calling that function directly is not a drop-in substitute; instead, the E148-diagnosing logic is now factored into a shared `stmts::reject_as_binding_write` helper that both functions call at their own root-resolution site, alongside `lower_assign_target`'s own (refactored) call to the same helper.

  This PR does **not** address this issue's other named gap — `CONST` roots are still not rejected on any assignment path (not just the two mutator/field-write functions the issue names: plain `CONST c = 1 … c = 5` is also silently accepted today, with no diagnostic anywhere in the compiler). Fixing that needs a new `DiagnosticCode` that was not pre-assigned for this item, so it is reported back to the issue rather than a code being self-allocated.

- d64cefc: Issue #2123: the loop-append COW cliff #576 closed at the root persisted
  one struct field deeper — `push(a.items, v)`/`insert`/`remove`/`remove_at`/…
  on a single-level struct-field projection (`a: Bag`, `Bag.items: Array<int>`)
  paid a fresh `Arc::make_mut` copy on _every_ call instead of mutating in
  place, an O(n²) cliff in a loop. Fixed without adding a new opcode: the
  lowering now drops the record's own reference to the mutated field (via the
  existing `RecordSet`) before the RMW runs, and takes rather than clones the
  RMW's own operand and result temps, so the field's `Arc` becomes the sole
  owner whenever nothing else aliases it.

  Observable behavior change (brink dialect only — this mutator shape is a
  T1b/TM-4 extension, unreachable from vanilla ink): a mid-RMW fault
  (`insert`/`remove`/`remove_at` on an author-supplied key/index that's
  invalid) now leaves the struct's _mutated field_ — not the whole record —
  as `Value::Null`. The struct itself stays a structurally valid record with
  every other field untouched; previously the field mutator's take/write-back
  ordering left the whole root completely unchanged on the same fault. This
  is a narrower version of the trade-off the root-level `push`/`insert`/…
  mutators (`lower_bare_mutator`, issue #576) already document and test.

- a5e5896: Issue #2127 (found while closing out #1995/#1920, deliberately fenced out
  of it): a divert with arguments (`-> knot(a, b)`) now checks its `ref`
  position arguments invariantly too.

  `InferPass::infer_target` (the `-> knot(args)` divert-with-args site)
  computed `arg_tys` and then explicitly discarded it (`let _ = arg_tys;`)
  — it called `record_ref_param_writes` so a `ref` param's _write_ was
  tracked for effect purposes, but performed **zero argument-type
  checking**, for `ref` or by-value positions alike. Unlike the three
  sibling sites #1995/#1920 already fixed (direct call, UFCS-desugared
  call, `#fn` creation site), there was no existing covariant check here
  to invert — this is a whole check that never existed.

  A `ref`-bound argument at a divert-with-args site whose type does not
  match the declared `ref` param type exactly (`infer::ty::ref_assignable`)
  now reports `E063` under `types = strict`, reusing the same
  `DirectCallArgMismatch` fact and `check_direct_call_args` reporting path
  the direct-call and `#fn`-creation-site checks already use (a divert
  target is, like a `#fn` literal, "not a call at all" but the same
  by-ref binding shape) — mirroring `infer_call`'s `ref` arm, including its
  observed-local carve-out (scoped to the `ref` arm only, same as the
  `#fn` creation-site fix).

  By-value (non-`ref`) argument positions at this site are deliberately
  left unchecked. Per the issue's own scope note (and the precedent PR
  #2014 set for `infer_fn_literal`'s by-value params): whether/how to
  check by-value divert-target arguments is its own design call, not an
  assumed yes.

  Observable through `@brink-lang/web` because the wasm package re-exports
  the same diagnostics.

- 115bb40: Issue #2134: `EditorSession::completions`/`completions_doc` now offer every
  `@NAME` cue harvested anywhere in the project — not just the active
  document — right after typing `@` at the start of a line
  (`CompletionContext::CueName`). This is the completion-UI consumer the
  #2114 harvest index landed for but nothing yet called: a cue declared only
  in a sibling file, never imported, now completes while editing an unrelated
  file, with no conventions handler or host manifest required
  (`docs/prose-dialect-spec.md` §5, "harvest by default"). Reads a new
  range-free projection (`ProjectDb::harvest_completion_names`) instead of
  the raw harvest index — the same `Eq`-cutoff seam `resolution_index_query`
  gives the symbol index. Correction (review finding): the projection still
  depends on the whole-project harvest merge (`harvest_index_query`), so a
  completion request does NOT skip that merge — what the projection buys is
  an `Eq`-stable value a _memoized downstream_ consumer could backdate on
  across a pure range-shifting edit. No such consumer exists yet today (both
  `brink-lsp` and `brink-web` read `harvest_completion_names()` directly,
  per request), so the measured present-day incrementality benefit is zero;
  the seam is there for whoever memoizes on top of it next.
- f958d24: Issue #2136: native (`.brink`) HIR lowering now wires `-> knot(args)`
  divert/tunnel-call/return-redirect arguments into `DivertTarget::args`
  (and `Return::onwards_args` for `return -> knot(args)`) instead of
  discarding them and raising a hard `E129` ("parses but has no HIR
  lowering yet"). A native divert, tunnel call, or return-redirect with
  arguments now compiles and runs, with the arguments reaching the target's
  params exactly like the ink-dialect path already did — observable through
  `@brink-lang/web`'s re-exported diagnostics (no more `E129` for this
  construct) and compiled-story runtime behavior.
- 8632205: Issue #2147 (gap 1 of #2091's follow-through review): the empty-`content`/
  Fragment blank-line suppression PR #2140 added to
  `brink-runtime::output::{resolve_lines, take_first_line}` did not extend to
  `OutputBuffer::end_capture`'s string-capture path (`resolve_parts`) — the
  `Opcode::EndStringEval` resolution an unrecognized choice display or any
  `~ temp x = "..."` string-eval capture rides. A blank line contributed
  purely by an empty `content`/Fragment interpolation inside a captured
  string still rendered, inconsistent with the streaming/batch path.

  `resolve_parts` now applies the same per-line suppression: a line within
  the captured text is dropped entirely (not left behind as a blank line) when
  it resolves fully empty and at least one of its parts interpolated a
  `Value::FragmentRef` that itself rendered empty — same invariant, same
  scope boundary (a non-`FragmentRef` empty slot still keeps its blank line)
  as the existing `resolve_lines`/`take_first_line` fix. Trailing-line
  handling is also brought to parity: an unterminated final segment (no
  trailing newline part) that resolves empty and Fragment-derived now drops
  its introducing newline too, matching `resolve_lines`' own final-entry
  suppression.

  `resolve_parts` is also reached from `OutputBuffer::resolve_fragment` —
  the resolver `ChoiceDisplay::Fragment` reads through
  (`story/mod.rs`/`story/flow_instance.rs`, and `brink-cli`'s `tui/app.rs`)
  — and, recursively, from resolving a fragment's own interior when it is
  itself multi-line. So this change also affects: a captured choice's
  display text when it is itself an empty capture, and the interior
  rendering of a nested, multi-line fragment (a blank line contributed by
  an inner, rendered-empty fragment inside an outer fragment's own captured
  region now vanishes too), which the streaming/batch path already did for
  top-level transcript lines.

- 231bb5f: Issue #2156: a divert-with-args site (`-> knot(args)`, a tunnel call, or a thread-start) whose argument count disagrees with its resolved target's declared parameter count now raises a new diagnostic, **`E176`**, on both dialects. This closes a gap where arity was never checked for a divert at all — `brink_ir::symbols::project`'s divert-reference projection previously hardcoded `arg_count: None` regardless of how many arguments the divert supplied, so the existing arity-check mechanism (`E031`'s, gated on the reference carrying a real argument count) could never fire for a divert. `E176` is `E031`'s sibling for the divert/tunnel/thread-start call shape, `Warning`-tier by default like `E031`, and does not fire when the divert resolves through a `Variable` or a divert-typed local parameter (a stored/forwarded divert-target value has no declared parameter row to check against). Unknown-target-name checking for a divert-with-args site was investigated alongside this and found already correct (`E024`, pre-existing) — no new code needed there.
- 9fac670: Issue #2164 (`docs/decision-log.md` 2026-08-03): `@[element(…)]`'s
  pattern-claiming half splits into its own `@[convention(claims = "…",
order = N)]` annotation, and `order` becomes a required, bare-integer
  precedence property.

  - **`@[convention(claims = "…", order = N)]`** — pattern claiming: competes
    for prose lines it did not announce, confined to the `brink.toml`-named
    conventions module, and now REQUIRES `order` (no default — precedence is
    total, explicit, and authored, never inferred from declaration position).
    Two new diagnostics: **E178** (missing `order`) and **E179** (duplicate
    `order` within one module, reported against both declarations).
  - **`@[element(args = "…", block)]`** — unchanged in meaning, narrowed to
    `!name` dispatch only: self-announcing, legal anywhere, no `order` at
    all (a self-announcing handler never competes for a line).
  - The claiming walk's dispatch order is now `order`-sorted rather than
    declaration-order (the retired issue #1848 interim rule) — observable in
    which handler wins when two claiming patterns can both match one line.

  Existing `@[element(claims = "…")]` source must be rewritten as
  `@[convention(claims = "…", order = N)]`; `@[element(args = "…")]` is
  unaffected.

- f628345: Issue #2165 (`docs/decision-log.md` 2026-08-03 "`fn conventions()` is
  DISSOLVED — handler precedence is a property of the `@[element]`
  annotation"): deletes the `fn conventions()`/`register` machinery the
  2026-08-03 ruling dissolved. `register` was never wired to any real
  end-to-end behavior beyond confinement/effect bookkeeping — there are zero
  real `register(...)` calls anywhere in the tree — but its presence was
  itself observable:

  - **`register` is no longer a recognized intrinsic name.** An unresolved
    call to `register(...)` now surfaces the ordinary `E025`
    (unresolved-name) diagnostic again, exactly as any other undeclared
    identifier does — it is no longer silently accepted pending a separate
    `E175` placement check.
  - **`E175` is now retired and never raised.** It documented `register`'s
    placement rule; the code stays reserved (never reassigned, per this
    repo's diagnostic-code stability convention) but no pass emits it
    anymore.
  - **`conventions_registry` is no longer a recognized effects-assertion
    cell name.** `@[effects(writes(conventions_registry))]` now fails with
    `E102` (unknown name) instead of matching the (now-deleted)
    compiler-owned registry cell.

  No project in the wild is expected to hit any of these: the intrinsic was
  only ever legal inside a project's conventions module's `fn conventions()`,
  a function no real `.brink` project has ever declared.

- 4a1dee1: Issue #2166: the built-in screenplay preset (`std/conventions/
screenplay.brink`, mounted into every compiled project's `Environment`
  manifest since #2080) migrates `cue`/`parenthetical` off the interim
  single-file `block`-capturing shape onto **attach mode**
  (`@[convention(…, attach = StructName, order = N)]`, issue #2178).

  - `cue`/`parenthetical` now each claim and consume **only their own
    matched line** — neither declares `block` anymore, so neither
    wrap-and-re-emits the dialogue that follows. Each declares a plain
    `struct` (`Cue { speaker: string }` / `Parenthetical { delivery: string
}`) via `attach`, matching its own return type (`E180`-checked).
  - Observable consequence: a claimed `@NAME` cue line or chain-gated
    `(delivery)` parenthetical line now renders as its declared struct's
    structural-default text (e.g. `Cue { speaker: VENDOR }`) instead of the
    old block-wrapped `NAME` / dialogue splice; the dialogue line(s) that
    follow are ordinary, unclaimed prose, rendered verbatim on their own —
    neither handler receives them anymore.
  - `heading`/`transition` are unchanged (plain, non-`block`,
    non-`attach` `@[convention]` handlers).
  - `order` values (`heading = 10`, `transition = 20`, `cue = 30`,
    `parenthetical = 40`) are unchanged in value but now deliberately
    justified in the preset's own module doc (none of the four patterns
    actually overlap today; the ordering anticipates future narrowing).

  No consumer reads `attach` as attachment metadata yet (no `use
std::conventions::screenplay` import path exists — #2167/#2198 — so this
  preset is still only reachable by inlining its source, as `tests/tier1-
native/conventions-screenplay-preset/` does); this changeset is filed
  because the mounted preset's own source and lowering shape changed, which
  `@brink-lang/web` re-exports through the `Environment` manifest every
  compile mounts it into.

- 4bfcdab: Fix #2174: a classic (non-block) indexed-assignment logic line —
  `~ a[i] = v`, written outside any `~ { … }` block — silently dropped the
  whole statement with **zero diagnostics**: the classic-line statement
  dispatch never routed an `Index` assignment target to
  `lower_indexed_assignment` at all, so the write vanished. This affected
  every classic-line indexed assignment, not only the struct-field-projected
  root shape #2121 fixed for the `~ { … }` block surface — a bare-variable
  target (`a[0] = 99`, no struct involved) compiled clean and the assignment
  simply never happened.

  Classic-line indexed assignment now shares the exact same dispatch the
  block form already had: a bare-variable root lowers correctly, and a
  struct-field-projected root (`a.items[0] = v`) is rejected with the same
  non-suppressible `E074` `reject_field_projection_index_root` already
  raises for the block form (#2121), instead of either silently dropping or
  silently misrouting.

- 78b4c2d: Issue #2178 (split from #2164's 2026-08-03 design-backport comment, item 2):
  `@[convention(…)]` gains an optional `attach = StructName` clause — the
  handler's declared output **schema**.

  - **`attach = StructName`** declares which keys a claiming handler attaches
    to the run it claims, by naming an ordinary declared `struct`: the schema
    is a type, not a new declarative sub-language. The governing split: keys
    are declared (this clause), values are computed (the handler body).
  - The declaration's own `: Type` return-type annotation must name the same
    struct `attach` does, or the declaration is **E180** and is never
    registered as a claiming handler at all (the same "never a partial one"
    posture `E159`/`E178` already take).
  - `@[element(args = "…")]` has no `attach` clause of its own — like `order`,
    this is `@[convention]`-only, since a self-announcing handler's output
    isn't a claim result to attach.

  No runtime behavior changes for an existing `@[convention]` declaration that
  does not use `attach` — this is purely additive.

- 309c00c: Issue #2179: a `@[convention]` handler whose transitive call closure reaches a `Query`-kind (world-reading) or unclassified (`Plain`-kind) `EXTERNAL` now raises a new diagnostic, **`E182`**, anchored at the real offending call site. A `@[convention]` handler may call pure functions and `Effect`/`Presentation`-kind externals ("commands"), but must never read world state — classification has to stay a pure function of the text, since the editor, the projection cache, and explain-match all depend on it. This is web-observable: the wasm editor's diagnostics surface `E182` in its warnings JSON exactly like any other analyzer diagnostic.
- 19e18be: `brink.toml`'s `[project] elements` key — the pointer to the project's
  conventions module (issue #1844) — is renamed to `[project] conventions`
  (issue #2180). The key predates the 2026-08-03 split of `@[element]`
  (`!name`-dispatched) from `@[convention]` (pattern-claiming) and, post-split,
  named a module of the latter, not the former.

  The old `elements` spelling is **not** hard-broken: it is still accepted as
  a deprecated alias — parsed into the same value and behaving identically
  downstream (including `E169`'s conventions-module confinement check) — but
  now surfaces a `ConfigWarning` naming the rename. If a `brink.toml` sets
  both `elements` and `conventions`, `conventions` wins and a second warning
  names the conflict. This is user-visible through
  `@brink-lang/web`'s wasm-exported `EditorSession::apply_project_config`,
  which surfaces every `ConfigWarning` `parse_str_at` returns.

- aa26464: Mutating a record field projection (`pop(a.items)`, `heap_pop(a.items)`, `~ a.count++`, or passing `a.items` to a `ref` parameter) previously compiled clean and misrouted the mutation onto the whole record — faulting at runtime or, in the implicit-`ref` case, silently replacing the record's value. All four shapes now refuse at compile time under the existing non-suppressible E074 code, each with a message naming its actual shape (field-projection mutator argument, increment/decrement target, or implicit-`ref` argument — the last pointing at the supported explicit spelling, `f(ref a.items)`).
- 31155ad: Issue #2197: fixed a stdlib-mount codegen collision, and closed the
  bare-name visibility gap it exposed. Observable through `@brink-lang/web`,
  because the real compile path (`brink_environment::compile`, which
  `@brink-lang/web`'s `compile.rs` calls) always mounts `std/` alongside a
  native project's own sources.

  - **The bug (worse than filed):** since #2080/#2190 mounted
    `std/conventions/screenplay.brink` into every native `Environment`, a
    project declaring its own same-named `extern`/`fn`/knot (exactly
    `tests/tier1-native/conventions-screenplay-preset/story.brink`'s shape —
    its own `scene_entered` extern + fallback + convention handlers, mirroring
    the shipped preset) hard-failed with `[E060] internal codegen error:
duplicate DefinitionId … assigned to two different containers`. Root
    cause: several LIR-lowering/HIR-stamping self-identity lookups
    (`lir::lower::mod::lookup_container_id`, `lir::lower::decls::
lookup_global`, `hir::stamp::lookup_label_id`) did a bare, file-blind
    `index.by_name` scan for "what id did the analyzer assign to the thing
    _this file_ just declared" — correct when at most one candidate existed
    per name, but M-2d's cross-declared-module coexistence (#790) now lets a
    project's own declaration and the mounted std module's same-named one
    both live in the index, and the blind scan picked the same one for both
    files' lowering passes. Fixed by preferring the entry declared in the
    file currently being lowered (falling back to the old unscoped match when
    none exists, so every pre-#2197 corpus stays byte-identical).
  - **The bare-name visibility gap (#2080's SCOPE FENCE, `docs/decision-log.md`):**
    stdlib symbols are reachable only via an explicit `use std::…` — there is no
    implicit inclusion. That import mechanism doesn't exist yet (#1582/#2167),
    so today a std-mounted candidate is invisible to bare-name resolution,
    full stop — `brink-analyzer`'s `resolve::lookup_by_name_direct` now
    excludes any `Other`-classified (not-in-scope, not-imported) std-module
    candidate before it can win the flat-fallback tie-break, including when
    it is the _sole_ candidate (previously silently reachable via the
    `!multiple` fast path). This narrows exactly one of M-2d's three
    resolution tiers (`Other`) for std candidates only; `InScope` (a std
    file referencing its own declarations) and `Imported` (a future real
    `use std::…`) are untouched.
  - Added `brink-test-harness/tests/issue_2197_std_mount_module_qualification.rs`,
    compiling the golden fixture through `brink_environment::compile` (the
    real production path every oracle/tier1-3 corpus entry point bypasses)
    and asserting the project's own `scene_entered` keeps its exact
    `DefinitionId` across an isolated vs. mounted compile, plus a full
    transcript match — not merely that compilation no longer errors.

- a64d78e: New diagnostic: E187 rejects a write to a CONST (issue #2201).

  `lir::lower::stmts::lower_assign_target` treated `SymbolKind::Constant`
  identically to `SymbolKind::Variable`, with no distinction at all — a
  story reassigning a declared `CONST` compiled clean, with zero
  diagnostics anywhere in the pipeline, and the mutated value was
  observable in the story's own output.

  `E187` now rejects every write channel that resolves a `CONST` root:
  plain/compound assignment, a postfix `++`/`--`, an indexed-assignment
  root, a bare in-place mutator (`pop`/`heap_pop`/`push`/`insert`/`remove`/
  `remove_at`, bare or indexed-lvalue), a struct-field write/mutator whose
  root is a `CONST`, and passing the `CONST` by `ref`.
  A `VAR` reassignment, a `CONST` read, and a local that merely shares a
  `CONST`'s name all stay legal, unaffected by this change. Applies to
  both `.ink` and `.brink` source, mirroring ink's own compile-time
  rejection of `CONST` reassignment.

- 9943755: `brink-ir`: a labeled choice/gather/block nested inside a
  content-embedded inline conditional/sequence (`{if …}`/`{~ …}` etc.
  sharing a line with prose, not on its own line) now keeps the same
  same-file-preferred label lookup issue #2197/#2213 already gave the
  primary weave walk (issue #2215). This also covers the same shape
  reached transitively through a block-capture's own captured plain
  content line — a top-level labeled container can never itself be
  absorbed into a block-capture (issue #1839's `is_plain_content_line`
  terminator stops the captured run at any `CONTENT_LINE` carrying a
  `LABEL`/`CHOICE_POINT`/`DIVERT_STMT`/`TUNNEL_CALL`), so the only way a
  label reaches `Expr::Fragment` is via a captured line's own mid-line
  inline conditional/sequence — the same shape as above, just nested one
  level deeper.

  `stamp_lambdas_in_expr`'s `Fragment` arm and
  `stamp_lambdas_in_content_part`'s `InlineConditional`/`InlineSequence`
  arms used to call `lookup_label_id` with `file: None` — the pre-#2197
  unscoped lookup. When two declared modules legitimately coexist with a
  same-named flow (M-2d, e.g. the stdlib mount alongside a project's own
  declarations) and each nests an identically-labeled choice inside such a
  construct, the unscoped lookup could silently prefer the wrong file's
  `DefinitionId`, colliding two distinct containers onto one id — the same
  `[E060] internal codegen error: duplicate DefinitionId` class #2197 fixed
  elsewhere, reachable this time only through the lambda-stamping
  traversal. `brink-web` transitively depends on `brink-ir`, so this is
  wasm-observable for any `.brink`/`.ink` source reaching this shape.

- c91926b: Issue #2216 (follow-up to #2197/#2080): `brink-analyzer`'s
  `resolve::lookup_unique_by_name` — the scope-free UFCS-receiver lookup used
  by `infer::body`, which has no `ImportScope` to consult — now excludes any
  `std…`-mounted candidate the same way `lookup_by_name_direct` already
  does for the scoped path, including when it is the function's sole
  candidate. Without this, a name whose only candidate was declared in the
  mounted `std/` tree would resolve through this path with no `use std::…`
  import, disagreeing with `lookup_by_name`'s stdlib-invisibility rule
  (#2080's SCOPE FENCE) and loosening `lookup_unique_by_name`'s own documented
  "strict subset of `lookup_by_name`" guarantee.

  **Reachable today, but no observable diagnostic delta found.** The prior
  wording here claimed this was unreachable because the only caller "resolves
  struct/UFCS-callable receivers" — false: `infer::body`'s
  `infer_ufcs_free_fn_result` looks up the trailing method segment against
  `&[SymbolKind::Knot, SymbolKind::External]` and records a call-graph edge
  _before_ it ever checks whether the receiver is a struct (that check only
  gates the call's own _result type_, further down). `std/conventions/
screenplay.brink` ships exactly those kinds (`fn heading`/`transition`/
  `cue`/`parenthetical`, `extern scene_entered`), so a project with no
  same-named `fn`/`extern` of its own reaches this path on an ordinary
  `x.heading(...)`-shaped call today. We tried to pin the resulting delta
  (the spurious call-graph edge, or the differing inferred result type) as an
  observable diagnostic through the real `brink_environment::compile` path
  and could not: for a non-struct receiver, `brink_analyzer::ufcs`'s own
  resolver (a separate, properly import-scoped pass this PR does not touch)
  already declines the call outright before this function's answer matters;
  for a struct receiver whose shape declares a matching field — the one
  shape where `ufcs` settles the call via field access without needing this
  function's answer at all — the spurious edge did not surface as an
  `#@effects` exceedance in the cases we tried either. So the change is real
  and reachable at the analyzer's internal-state level (confirmed by the two
  new `resolve.rs` unit tests), but we did not find a corpus/oracle- or
  diagnostic-observable case to pin end to end; see the PR body's
  "Reachability" section for the fixtures tried. Included per this repo's
  `@brink-lang/web` changeset convention for any patch touching resolution
  behavior reachable through `brink_environment::compile`, regardless.

- f6838e2: Fix #2222: inside a choice's inline conditional/sequence branch (a
  `lower_inline_block`-lowered `~ { … }`-less classic line), two more of
  `mod.rs`'s classic-line dispatch arms are now mirrored, matching the
  `Index`-assignment parity fix from #2211/#2174:

  - A **struct-field assignment** (`~ p.hp = 99`) no longer silently
    corrupts the record. Before this fix it compiled with zero
    diagnostics but resolved its target to the bare root `p`, overwriting
    the whole record with the RHS and faulting at runtime
    (`RuntimeError::NotARecord`) the next time `p` was read as a struct.
    It now writes the field correctly.
  - A **collection mutator call** (`~ push(a, 9)`) is no longer rejected
    with a spurious `E056` ("collection mutator used in expression
    position"). It now lowers and executes, consistent with the `~ { … }`
    block form and the top-level classic line.

- d120ecb: `brink-ir`: two files that legitimately declare a same-named knot (M-2d —
  `native_module_path` always differs per file, so `insert_symbol` lets them
  coexist rather than raising a duplicate-definition diagnostic) no longer
  fail to compile with `[E060] duplicate DefinitionId` when both knots hold
  an anonymous container at the same structural position (issue #2229) —
  whether that container is one the HIR stamping pass mints (unlabeled
  choice/gather/conditional-branch/sequence-branch) or one minted at LIR
  time (an inline-sequence wrapper, e.g. an alternation inside choice text).

  Three id-affecting changes ship together, all inside the one ruled break
  class (Option A, `docs/decision-log.md` 2026-08-20):

  - `hir::stamp_container_ids`'s per-knot loop qualifies a knot's interior
    anonymous-container hashing scope with the same `#file:{path}` prefix
    root content already carried (#1504).
  - `lir::lower_knot_chunk` gives the knot chunk's `IdAllocator` that same
    per-file prefix, covering the LIR-minted inline-sequence wrappers the
    stamping pass never sees (review finding — the stamping fix alone left
    this shape colliding).
  - Synthesized choice path segments are spelled `c-{n}` (matching the
    documented `c-N`/`g-N`/`b-N`/`s-N` scheme) instead of the bare `c{n}`,
    which an authored knot legally named `c0` could equal — under the now
    shared `#file:` namespace that was a new single-file `E060` regression
    (review finding); a dashed segment can never equal an authored
    identifier.

  Consequence (accepted, not a defect): anonymous-container `DefinitionId`s
  shift — every anonymous choice container everywhere, and every
  knot-interior anonymous container/wrapper — so saved visit counts keyed
  to those old addresses detach on recompile (`LoadReport` degrades
  tolerantly). Name-keyed state (labels, knots, stitches, globals) is
  unaffected. `brink-web` transitively depends on `brink-ir`, so this is
  wasm-observable for any `.brink`/`.ink` source reaching these shapes.

- 5fabf50: `EditorSession` (the wasm/studio editor session `@brink-lang/web` exposes)
  now mounts the shared stdlib — the third producer named by issue #2231, the
  `brink-web` sibling of #2198/#2225 (`brink-cli`/`brink-lsp`). Every
  `(root-relative key, source text)` pair from
  `brink_environment::stdlib_sources()` is fed through `IdeSession::
update_source` at construction, before any dialect/type-policy setter runs,
  mirroring `Project::ide_session()`'s ordering precedent. Previously
  `EditorSession::new()` built a bare `IdeSession::new()` with no stdlib
  mounted at all, so a symbol declared in a mounted stdlib module (e.g.
  `std/conventions/screenplay.brink`) was absent from the project-wide
  symbol index in the playground/studio editor, unlike a real compile.
  Note: `std::`-qualified paths (`use std::conventions::screenplay`) are not
  yet resolvable at all — that needs #1582's pub marker and #2167's
  closure-scoped confinement, neither of which has landed — so this mount
  does not yet make hover/completion/goto-definition resolve _through_ a
  `std::` path; it only makes the mounted symbols visible to the same
  project-wide indexing every other file gets. Mounted stdlib files are also
  excluded from client-facing listings (`list_files`/`project_outline`/
  `story_graph`) so they don't appear as phantom rows in the Binder or
  project-wide search.
- 8e6427a: Analyzer: `lookup_unique_by_name`'s std-visibility gate no longer
  disagrees with `lookup_by_name` for a referrer declared inside the std
  tree itself (issue #2233).

  `#2216` (PR #2224) taught `lookup_unique_by_name` — the scope-free lookup
  `infer::body::infer_ufcs_free_fn_result` uses to type a UFCS-shaped call's
  result — the same std-invisibility gate `lookup_by_name_direct` uses. But
  `lookup_unique_by_name` has no `ImportScope` to consult, so its gate
  excluded every std-mounted candidate unconditionally, including when the
  _referrer_ was itself declared inside std: `lookup_by_name`'s own
  `InScope` tier keeps resolving a std file's own sibling references, so the
  two lookups silently disagreed for that one case.

  `BodyCtx` now carries a `referrer_module` hint (the referring def's own
  declared module — the same string `ImportScope::file_module` would carry
  for that file), threaded from `ProjectCtx::body_ctx`.
  `lookup_unique_by_name` takes it as a new parameter and only excludes a
  std candidate when its module differs from the referrer's own — the exact
  `Candidacy::InScope` "referrer and candidate share a declared module"
  rule, reproduced without a full `ImportScope`. A referrer inside std
  looking up a _different_ std submodule's candidate, or a name genuinely
  ambiguous between a now-visible std sibling and a coexisting ordinary
  candidate, both still resolve to `None` (declined) rather than guessed at —
  this narrows the over-broad exclusion, it does not widen resolution.

- 9c8d51a: Fix #2238: a `STRUCT` shape table now supports two same-named shapes
  coexisting (a project's own `struct Cue { … }` alongside a mounted std
  preset's own same-named `struct Cue { … }`), resolved by referrer file —
  the same rule #2197 already applies to knots/externals — instead of a
  single project-wide bare-name winner. Previously the mounted preset's
  shape could silently claim the bare name ahead of the project's own
  declaration, and the project's construction literal would then bind
  against the wrong (narrower) shape, faulting at runtime with `struct shape
id <u32::MAX> out of range`. Observable through `@brink-lang/web` for any
  native project that both declares its own struct and mounts a std preset
  declaring a same-named one.
- e5b980d: Issue #2240: `brink-ir`'s struct-shape table builder no longer silently
  drops a declared `STRUCT` when its own definition can't be resolved.

  - **New diagnostic `E181`** (non-suppressible backstop, the `E060`/`E073`
    posture): `lir::lower::structs::build_shape_table` raises it if a
    declared struct's own self-declaration lookup comes back `None` — the
    narrow case where an analyzer-dropped intra-module duplicate's only
    surviving same-name sibling is itself std-declared. Before this, the
    struct silently vanished from the shape table and the seeded name table,
    shifting every subsequent `ShapeId`/`NameId` and the bytecode built from
    them with no diagnostic at all.
  - Reachable today, not only from a future std mount: an ordinary project
    declaring its own `struct Cue`/`struct Parenthetical` (the two names the
    mounted screenplay preset already declares) can collide with std's, with
    no `#@module` on either side required — `symbol_index_query` shares one
    index across every registered file regardless of the compilation
    closure, so the project's own struct gets dropped as an ordinary
    duplicate whenever its file sorts after the std key in `FileId`-mint
    order, and `E181` now fires instead of a silent drop. See
    `docs/diagnostics/E181.md` for the exact condition and the fix (rename,
    or declare a `#@module`).
  - `lir::lower::structs::build_struct_shape_data` (the `NameId`-free
    cutoff-friendly twin `brink-db`'s `struct_shape_data_query` memoizes)
    performs the identical lookup and deliberately does **not** duplicate
    this diagnostic — see its own doc comment and `E181`'s doc for why: it
    is a pure `Eq`-cutoff salsa data query with no diagnostic sink to push
    into, and every real compile always computes it alongside
    `build_shape_table` in the same salsa revision, over the same inputs, so
    the same drop condition always raises `E181` from that side instead.

- cf57b22: Analyzer: `declared_shapes`'s struct-shape table is now referrer-scoped,
  not a flat bare-name winner (issue #2241).

  `brink-analyzer::structs::declared_shapes` used to return a flat
  `BTreeMap<String, ShapeInfo>` populated by plain last-`insert`-wins — with
  the stdlib mount (#2080), a project's own `struct Cue { … }` coexisting
  with a same-named `struct Cue { … }` from a mounted std preset meant
  whichever file was iterated last silently overwrote the other's
  `ShapeInfo`, regardless of which one a checking site actually meant. This
  table feeds real diagnostics (`E069`/`E070`/`E071` construction-literal
  field checks, `E063` dotted-assignment field checks, `E098` `ref`
  lvalue-path segment checks, and UFCS field-call/receiver-type
  resolution), so a construction literal, assignment, or UFCS call could be
  validated against the wrong struct's fields.

  `declared_shapes` now returns a `ShapeTable`: `get_by_def` for a shape
  already pinned to an exact `DefinitionId` (a construction literal's own
  shape name, which the analyzer already resolves with full module-scope
  `Candidacy` via `resolve::resolve_struct_ref`), and `resolve(name,
referrer, index)` for every other lookup — the candidate declared in the
  referrer's own file, else whichever remains once mounted `std…`-declared
  candidates are excluded. Every consumer (`structs::check`,
  `structs::check_assignments`, `ref_projection::check_strict`,
  `ufcs::resolve`) now resolves per its own referring file instead of
  reading a global winner.

- 546ded5: Issue #2245: `std::` (and every future mounted library) is now a top-level
  **peer root** of `story::`, never a subdirectory of it — correcting
  `brink_db::modules::native_module_path`, which used to prefix every
  derived native module path with the literal `"story"` unconditionally.
  The #2080 stdlib mount (`std/conventions/screenplay.brink`) used to mint
  `story::std::conventions::screenplay` — the standard library filed as a
  subdirectory of the user's own project. It now mints
  `std::conventions::screenplay`, a peer of `story`, matching the
  2026-08-04 "peer roots" ruling (`docs/decision-log.md`). An ordinary
  project file is unaffected: `market/barter.brink` still mints
  `story::market::barter`.

  Observable through `@brink-lang/web`: the real compile path
  (`brink_environment::compile`, which `@brink-lang/web`'s `compile.rs`
  calls) always mounts `std/` alongside a native project's own sources, and
  `DefinitionId` is a hash of `(module, name)` — every std-declared
  definition's id changes as a direct, expected consequence (ruled
  time-bounded acceptable pre-release, decision-log addendum 2026-08-04; no
  saves or `.inkb` artifacts in the wild depend on it yet). A project's own
  (non-std) definitions keep byte-identical ids.

  `is_std_module`'s string-prefix test (`story::std…`) — previously
  reinvented independently in `brink-analyzer::resolve` and
  `brink-ir::lir::lower::decls`, because those crates cannot share a helper
  in that direction without a dependency cycle — is now `brink-ir::symbols`'s
  own root-identity check (`std…`), consumed by both former call sites.
  `native_module_path` derives its root the same structural way: a
  root-relative key's leading path segment decides whether it qualifies
  under `std` or `story`.

  The oracle ratchet (`RATCHET_EPISODE_COUNT`, `crates/internal/
brink-test-harness/tests/oracle_snapshots.rs`) does not move: this is a
  pure identity renumbering, and the ratchet compares episode content — text,
  tags, choices, state — never ids.

- 3bbd8d9: Compiler: LIR lowering no longer skips the std-exclusion for a struct
  shape name resolved as the sole candidate for its bare name (issue #2246).

  `ShapeTable::resolve`'s "fast path" used to return a bucket's only
  candidate unconditionally, bypassing the referrer-scoped, std-excluding
  resolution every multi-candidate lookup already went through (issue
  #2238) — so a struct name that only a mounted `std…` module
  declares, with no project-side declaration of that name anywhere, would
  silently resolve through with no import, the same "reach into std with
  no import" class #2197/#2238 closed for every other bare-name lookup.
  `resolve` now always routes through `decls::lookup_global`.

  Separately, a struct construction literal's shape name (`Name#{…}` /
  `Name { … }`) is a `RefKind::Struct` reference the analyzer already
  resolves against the referrer's module scope — lowering now consumes
  that recorded resolution directly (both in expression position and in a
  `VAR`/`CONST` declaration default) instead of re-deriving it through
  `ShapeTable`, removing a duplicate resolution implementation for this
  reference kind.

- 56ce7bf: Compiler: a struct field's declared type and a `VAR`/`CONST`/`temp` TM-2
  type annotation are now `RefKind::Type` references the analyzer resolves
  against the referrer's module scope, instead of a private `brink-ir`
  primitive re-deriving the answer (issue #2249, the remainder of #2246 left
  open).

  Before this issue, `hir::lower::types` registered no HIR reference at all
  for a type annotation's nominal leaf — `symbols::project`'s own doc called
  it "a nominal-only grammar, resolved later by a different mechanism". That
  different mechanism was `ShapeTable::resolve`, a `brink-ir`-side lookup
  re-implementing referrer scoping and std-exclusion on its own
  (`decls::lookup_global`'s fallback, which excludes every std-declared
  candidate unconditionally with no referrer-inside-std carve-out). Lowering
  now consumes the analyzer's own `resolve::resolve_type_ref` resolution
  directly for all four call sites this fed (`build_shape_table`'s field
  loop, `build_struct_shape_data`'s identical loop,
  `structs::record_global_annotation`, `context::LowerCtx::
record_temp_annotation`), and `ShapeTable::resolve` — with no production
  caller left — is deleted.

  **Observable delta:** a referrer _inside_ a mounted std module referencing
  a _sibling_ std file's struct in a type annotation, with no explicit
  import, now resolves (`lookup_by_name_direct`'s `InScope` tier) where it
  previously could not (`lookup_global`'s unconditional std-exclusion had no
  referrer-is-std carve-out) — the same static-offset (`RecordGet`/
  `RecordSet`) chase issue #2246 already restored for a construction
  literal's shape name. A TM-2 annotation naming an _unresolvable_ type
  (including a std-only struct an ordinary project file never imported)
  still raises no diagnostic of its own — that annotation-content check
  (`E061`, `brink-analyzer::annotations::check`) is unaffected and remains
  project-flat.

  Two sibling `brink-ir` lookups audited against the same question
  (`collect_externals`' extern-to-fallback-fn pairing, `context::LowerCtx::
lookup_address_id`'s local-label addressing) were found **not** to fit this
  pattern — both are self-declaration lookups with no corresponding
  user-written reference to register a `RefKind` for — and are unchanged.

- 4a664ec: Issue #2262: `brink-ir`'s `CONST`/`VAR`/`EXTERNAL` global-declaration collectors no longer silently drop a declaration when its own definition can't be resolved.

  - **New diagnostic `E184`** (non-suppressible backstop, the `E060`/`E073`/`E181` posture): `lir::lower::decls::collect_globals` (`CONST`/`VAR`) and `collect_externals` (`EXTERNAL`) raise it if a declaration's own self-declaration lookup comes back `None` — the same narrow "every surviving same-name candidate is std-declared" condition `E181` already backstops for `STRUCT` (issue #2240), now shared via a `lookup_global_or_diagnose` helper. Before this, the declaration silently vanished from `PreludeDecls` (no `lir::GlobalDef`/`lir::ExternalDef`) with no diagnostic at all.
  - **Reachable today for `EXTERNAL`**: an ordinary project (no `#@module`, no `dialect` override needed) that declares its own `EXTERNAL scene_entered(...)` collides with the std-mounted screenplay preset's own `extern scene_entered` — see `brink-environment`'s `external_self_declaration_silently_drops_when_colliding_with_a_std_preset_name` for the reproduction through the real compile pipeline. `CONST`/`VAR` stay reachable only in principle today (`std` declares neither yet), the same status `E181` itself carried before its own reachable case was found.

- c025a9f: Issue #2264: a `@[convention(…, block, attach = StructName)]` handler
  declaring both clauses on the same declaration now diagnoses `E186`
  (mutually exclusive) instead of silently accepting the annotation with
  `attach` inert. Before this fix, `block` always won `try_claim`'s dispatch
  and the `attach` clause was parsed and stored but never consulted — no
  event, no `OutputLine.element.data` merge, and no author signal at all.
  Observable through `@brink-lang/web`: a project with this shape now
  reports a new diagnostic where it previously analyzed clean.
- 85cb6e5: Analyzer: `E061` (unrecognized type name) is now referrer-scoped (issue
  #2272, split out of #2249/PR #2271).

  `annotations::check`'s struct-name recognition used to read a project-flat
  `declared_struct_names` set with no `ImportScope`/std-exclusion — an
  unimported std-only struct name (e.g. `~ temp c: Cue` with `Cue` never
  imported) read as "recognized" even though `resolve::resolve_type_ref`
  already silently excluded it from resolution by design. Net effect: that
  shape raised no diagnostic anywhere. `check` now routes a bare `Named`
  annotation through the same `ImportScope`/`Candidacy` lookup
  `resolve_type_ref` uses, so `E061` fires — naming the module the struct
  is declared in (and noting it isn't reachable from this file yet) when
  the name is declared but out of scope, and falling back to the original
  "not a recognized type" message when the name is unknown project-wide. `names.lists`/`names.handles` (the `List<L>`/`Handle<K>`
  checks) and `check_reserved_type_names` (`E188`) are unaffected —
  `resolve_type_ref` never scopes those vocabularies, so there was no
  referrer-scoping precedent to mirror there.

  Also: a knot/stitch/lambda parameter's and return-type annotation's bare
  type name now registers a `RefKind::Type` reference (mirroring the
  `VAR`/`CONST`/struct-field/`temp` registration issue #2249 already added),
  closing the goto-def/rename/find-references gap for a struct used only as
  a parameter or return type.

- 9397a1a: Editor: semantic tokens now classify native (`.brink`) syntax correctly
  instead of reading as `variable` (issue #2280).

  `EditorSession::semantic_tokens`/`semantic_tokens_doc` previously always ran
  `brink_ide::semantic_tokens::classify_token` over the file's **ink**-parsed
  CST, even for a native `.brink` file — `ProjectDb::parse` runs the ink
  frontend regardless of extension, only `ProjectDb::parse_native` is
  dialect-correct. Running ink's grammar over native source produced a garbled
  tree (ink has no `struct`/`flow`/`@[...]` grammar), so `struct`, a struct's
  own name, its field names, an annotation's name/argument names, and type
  references all fell back to the generic `variable` colour, and a quoted
  string containing a character class (`"...[A-Z]..."`) was shredded into
  several differently-coloured fragments because ink's tokenizer treats `[`/
  `]` as significant.

  `brink_ide::semantic_tokens` gains a native-CST classifier
  (`semantic_tokens_native`/`semantic_tokens_range_native`,
  `classify_native_token`) that dispatches on `brink_syntax_native::SyntaxKind`
  directly, and `EditorSession::semantic_tokens_impl` now checks
  `IdeSession::is_native` and routes a native file through
  `IdeSession::syntax_root_native` instead. `struct`/`flow`/`@[...]`
  declarations, struct fields, and annotation names/args now get distinct
  token types; a string literal's interior (including lexer-significant `[`/
  `]` inside it) renders uniformly as `string`.

- 3be1e5f: Fix #2287: a module-qualified divert (`-> barter::haggle`, after
  `use story::market::barter;`) now resolves — the native lowering was
  normalizing `::` to `.`, making it indistinguishable from ink's own dotted
  `knot.stitch` addressing, so it could never match. The over-permissive flip
  side is also fixed: a bare `-> haggle` after only a module-qualified import
  now correctly stays unresolved (`E025`/unresolved-divert), rather than
  silently accepting a name only a symbol-level or glob `use` should bring
  into scope.
- d43ec7e: Issue #2289 (`docs/decision-log.md` 2026-08-05 "Conventions are
  PROJECT-WIDE by definition…"): corrects two defects in the §9.1 conventions
  confinement ruling that had drifted apart since #1844.

  - **A conventions module's `@[convention]` handlers now claim prose across
    the WHOLE PROJECT, not just their own declaring file.** Before this fix,
    a correctly-configured `[project] conventions` module claimed nothing
    outside the one file that declared its handlers — reach was silently
    file-local despite the confinement rule's whole purpose being to
    centralize conventions for the entire project.
  - **A `@[convention]` handler declared with `[project] conventions` entirely
    unset is now `E169`, not a silent pass.** A claiming handler with no
    configured module names no module for the declaration to belong to, so it
    is a misconfiguration rather than an opt-out.

  Both changes are observable through `@brink-lang/web`: a `.brink` project
  compiled with a conventions module configured will now see prose in every
  project file matched against that module's handlers (previously only the
  declaring file's own prose was matched), and a project that declares
  `@[convention]` handlers without configuring `[project] conventions` will
  now fail to compile with `E169` where it previously compiled silently.

- 967bd1b: Editor: folding, inlay/color hints, argument widgets, and line conversion no
  longer read a native (`.brink`) file's editor state from ink's mis-parse of
  its source text (issue #2291, same defect class as #2280/#2286).

  `EditorSession::folding_ranges` now routes the machinery/narrative fold-run
  pass through `IdeSession::syntax_root_native` +
  `brink_ide::line_context::line_contexts_native`/
  `line_contexts_with_dialect_native` for a native file, instead of
  `syntax_root`'s always-ink parse.

  `inlay_hints`, `color_hints_doc`, and `argument_widgets_doc` walk
  `root.descendants()` casting to ink-only typed AST nodes
  (`ast::FunctionCall`, `ast::DivertTargetWithArgs`, `ast::TempDecl`) — there
  is no native-CST equivalent of that pass yet, so a native file now returns
  no hints/widgets rather than ones computed from a mis-cast tree (verified:
  ink's parse of a native `-> target(args)` divert-with-args produces a real,
  wrong `DIVERT_TARGET_WITH_ARGS` node when the callee happens to resolve in
  the project's real symbol index, rendering a plausible-looking but
  ink-computed parameter/color/argument-widget hint).

  `convert_element`/`convert_element_doc` now return no edit for a native
  file: the feature rewrites bare-line `*`/`+`/`-` ink choice/gather sigils,
  which have no native equivalent at all (native choices only exist inside an
  explicit `{? ... }` choice point) — applying it to a `.brink` file would
  write invalid native syntax. `format_document`/`format_document_doc` return
  the source unchanged for a native file rather than relying on
  `sort_knots_in_source`'s ink-knot-header search coincidentally finding
  nothing.

- b353095: Editor: `document_symbols`/`project_outline` now include `struct` and
  `const` declarations (issue #2292).

  `brink-ide::document::document_symbols` projected `manifest.structs` and
  `manifest.constants` into nothing — its top-level decl-group list only
  walked `variables`, `lists`, and `externals`, so a native `.brink` file's
  `struct Cue { ... }` and `const MAX = 100` never appeared in the outline,
  `textDocument/documentSymbol`, or the studio Binder's `project_outline()`
  road, even though both symbols were already correctly indexed everywhere
  else (cross-file resolution, hover, the LSP `SymbolKind::STRUCT`/`CONSTANT`
  mapping). Adding `(&manifest.structs, SymbolKind::Struct)` and
  `(&manifest.constants, SymbolKind::Constant)` to the decl-group list
  surfaces both alongside knots.

- a7e313d: #2293: prose text no longer classifies as `variable`/`operator`/`string` in
  semantic tokens, for both ink and native (`.brink`) files. This is the
  remainder of the #2280/#2286 prose-classification gap — that PR fixed
  declarations (`struct`, `speaker`, `convention`, ...) plus keyword lexemes
  and `SCENE_TITLE` absorbed into prose, but left ordinary dialogue/narration
  words, prose punctuation (`-`, `!`, `?`, `->`, `<>` reachable inside a raw
  text/tag run), and a literal quote mark in dialogue still painting as code.

  Fixed at the classifier level, not by suppressing output: a token whose
  parent is a pure-prose CST node (`TEXT` for ink; `TEXT`/`TAG`/`SCENE_TITLE`
  for native, `CUE_NAME` already handled) now classifies as no token at all,
  matching the CST-presentation gap's own established precedent (`is_prose_
run_container`, #2286) and how `@codemirror`'s decoration model already
  treats an unclassified range — plain default-foreground text, not a
  missing/broken highlight. No new token type was introduced; the LSP
  semantic-token legend is unchanged.

- d72cad2: Fix #2298: extends #2287/#2296's module-qualified-import fix beyond
  diverts. A bare tunnel-function call (`haggle()`, ink allows a knot as a
  function via tunnels) after only a module-qualified import
  (`use story::market::barter;`, no symbol-level import of `haggle`) now
  correctly stays unresolved instead of being wrongly accepted — the same
  over-permissive bug #2287 reported for diverts, reproduced at a call
  site. `lookup_divert`'s remaining `Stitch`/`Label`/`Variable`+`Constant`
  steps share the same exclusion now too (latent for native today, since a
  `flow` always classifies `Knot`), and the `Constant` omission on that
  `Variable` step (issue #2083's thread) is fixed alongside it — a
  `CONST target = -> knot` can now be diverted to via `-> target`. The
  resulting `E024`/`E025` diagnostic for a module-imported-but-bare
  reference now names the qualified-import-only candidate it skipped
  ("import it from `module`"), mirroring the framing `modules::check`'s own
  E025 already gives an unimported reference.
- 62dba1d: Session-level read-only enforcement for a mounted stdlib file (issue #2306, ruled 2026-08-06 "Mounted
  stdlib presents as a read-only library node", part 3 of the ruling — built first per its own sequencing
  note). #2231/PR #2303 mounted the stdlib into `EditorSession` and hid mounted files from
  `list_files`/`project_outline`/`story_graph`, but a by-id route that resolves a file outside those three
  listings — a doc handle opened via goto-def navigation into an inherited symbol, or a bulk TS-level caller
  like project-wide search/replace — could still write through to the mounted copy and hand the edit to the
  host to persist, silently forking the stdlib into the project.

  `EditorSession` (`@brink-lang/web`) gains `is_read_only(path)`, and `update_document` /
  `auto_import_apply_include_doc` now refuse (returning the existing "did not apply" sentinel for each —
  `"null"` and `{ ok: false, error }` respectively) when the handle's file currently resolves to a mounted
  id — `open_document`/`open_fragment` still succeed on a mounted path, so it stays browsable/openable, only
  writing through the handle is rejected. `update_file` is deliberately left unguarded: it is the host's
  whole-file "this is the content now" API, and a real project file placed at a mounted key must keep
  winning by construction-time ordering (the existing shadowing contract). `update_source` — the singleton-
  session sibling, including its fragment-splice branch — is **also** left unguarded in this PR: it has no
  in-repo caller today, but as published `@brink-lang/web` surface an external embedder driving the
  singleton API can still reach the same silent-fork hole this PR otherwise closes. That gap is not fixed
  here; tracked as a known follow-up rather than guessed at.

  `EditorSessionHandle.isReadOnly` (`@brink-lang/web`) exposes the new query. `ProjectSession.applyEdit`
  (`@brink-lang/editor`) — the shared seam every bulk-edit caller (search/replace, results-buffer edits,
  binder undo) already routes through per issue #137 — now checks it before writing and returns `boolean`
  (previously `void`) so a caller can react to a refusal instead of assuming success.
  `ProjectSession.initialize()`/`addFile()`/the external-change handler are unaffected: they call
  `session.updateFile` directly, exactly like a legitimate shadow write.

  `@brink/studio-store`'s search slice (internal, not independently versioned) surfaces a refusal from the
  three `applyEdit` callers (`replaceSearchMatch`, `replaceAllSearchMatches`, `applySearchRowEdit`) as a
  "read-only" notification instead of silently continuing.

- 370715c: Issue #2310 (#2113 remainder, NS-T seam 3/6): `explainMatch`/
  `explainMatchDoc`'s `winner` now carries `kind` — the claimed line's
  compile-time structural shape (`content_line` / `scene_heading` /
  `bang_dispatch` / `cue` / `parenthetical`).

  `brink_ir::explain_match` itself still cannot derive this correctly
  from its own bare-text inputs (one shape is chain-gated on the
  _previous_ line, which a standalone line of text can't answer — see
  `brink-ir`'s `hir::explain` module doc). `kind` is composed one layer
  up instead, in `EditorSession::explain_match`/`explain_match_doc`:
  read straight off `HirFile::element_matches` via a live salsa query
  (recomputed off the current revision, never a stored snapshot that
  could lag an edit), not re-derived. It is present only when a compiled
  `ElementMatch` for that same line exists and its handler agrees with
  the live `winner` — absent (never a guess) on every `Unmatched` line,
  on any `shadowed` runner-up (only the winning claim has a compiled
  record to read a kind from), on an ink-dialect file (`element_matches`
  is always empty there), or on a line the compiler structurally
  declined to claim on its own (a heading carrying a `[slug]`/tags, or a
  line folded into a block handler's captured run) even though the live
  walk matched it.

  `ExplainClassifiedMatch.kind` (`@brink/wasm-types`) is optional and
  new; every other field on `ExplainMatch`/`ExplainClassifiedMatch`/
  `ExplainAttempted` is unchanged. In practice `kind` cannot yet surface
  `"cue"`, `"parenthetical"`, or `"bang_dispatch"`: the native frontend
  hands a claiming handler's pattern only the inner `CUE_NAME`/`TEXT`
  run (excluding the `@`/parens), which the built-in screenplay preset's
  own `cue`/`parenthetical` patterns require and so never match against
  the live raw-line walk, and `!name` dispatch handlers are registered
  on a path the live walk never consults at all. Only `"content_line"`
  and `"scene_heading"` are reachable today — see
  `crates/brink-web/src/editor/explain_match.rs`'s own module doc.

- 8d92c9c: Explain-match wasm DTOs now carry `mode`, `disposition`, and the resolved
  `attach` schema (issue #2311, #2113 follow-up). `ExplainClassifiedMatch`
  (the winner/shadowed shape) gains `disposition` and `attach`; it already
  carried `mode`. `ExplainAttempted` (the miss shape) gains all three —
  `mode`, `disposition`, and `attach` — which it previously exposed none of.
  `attach`, when present, is `{ kind: "resolved", name, fields }` or
  `{ kind: "unresolved", name }`, mirroring `brink_ir::ConventionAttachSchema`;
  its field types are the new recursive `ExplainSchemaTypeShape` mirroring
  `brink_ir::SchemaTypeShape`. `attach` is omitted (not `null`) when the
  handler declared no `attach = StructName` clause.

  This also closes a gap one layer down: `brink_ir::ClassifiedMatch` (the
  hit-case record `crates/internal/brink-ir/src/hir/classify.rs` produces)
  did not carry the `attach` schema at all — only `ConventionProjectionEntry`
  (the miss-case/attempted record) did. `ClassifiedMatch` now carries it
  through from the projection entry unchanged, so both the hit and miss
  wasm shapes can expose it.

- 1156ff3: `packages/wasm-types`'s `SaveState` TS interface (re-exported through
  `@brink-lang/web`) was missing `global_ids` (pre-existing drift) and
  `suspended` (widened further by #2307/#2108) entirely. Both are now
  mirrored, plus the `SuspendedFlow`/`WakePolicy`/`WakeSource` shapes
  `suspended` needs (issue #2313).
- c3c6eab: Editor: a native project's own declarations no longer spuriously collide
  with the mounted stdlib's same-named declarations when the project's own
  `brink.toml` is loaded into the same session (issue #2318).

  `std/conventions/screenplay.brink` declares `struct Cue`, `fn cue`, and
  `fn heading`. Under #2245's peer-root ruling `std::` is a peer of `story::`,
  not a parent, so a project declaring its own `Cue`/`cue`/`heading` must
  coexist with the mount rather than collide with it — `brink compile` on
  such a project already exits `0`. Only `EditorSession`'s off-db analysis
  disagreed, and only when the project's `brink.toml` shared the session (as
  `EditorSession`'s real callers load it, so the Binder can list/edit it):
  `ProjectDb::is_all_native` — the gate `IdeSession`'s M-2d cross-declared-
  module coexistence check reads — used to answer `false` the moment the db
  held even one non-`.brink` file, including a `brink.toml` config document,
  disabling the exemption for what was, in every sense a compile cares about,
  a fully native project. The visible symptom was a self-contradictory pair
  of diagnostics for any name shared with the mount: reported as both a
  duplicate definition and as undeclared outside `use std::…` in the same
  run.

  `ProjectDb::is_all_native`/`project_is_all_native` now ignore any tracked
  file with neither a `.brink` nor an `.ink` extension when deciding whether
  a project is "all native" — such a file (a `brink.toml`, or any other
  non-source document a host's file tree loads into the same session) no
  longer counts as "an ink file" against the check.

- 633fb8f: Fix #2320: a relative `[project] conventions` pointer resolved against the
  process's current directory instead of the declared `native_root`, and an
  unresolvable pointer was silently swallowed with no signal reachable from
  wasm.

  **The resolution fix lives at the pointer's read site, not in the shared
  key normalizer.** `brink_db`'s `expected_conventions_module` routed the
  pointer through `root_relative_key`, which absolutizes a relative path
  against the **process's cwd** — correct for registered file keys (the ink
  CLI registers the entry in its cwd-relative spelling, so a bare `main.ink`
  compiled from `cwd = root/sub` must key as `sub/main.ink`; changing that
  arm breaks CLI content identity by invocation cwd), but wrong for the
  pointer: a relative `conventions` value is written in `brink.toml`, whose
  directory defines the root, so relative means **root-relative by
  definition**. This was reachable only through one-shot `brink compile`/
  `brink check` (usually invoked with cwd == project root, masking it) until
  PR #2316 wired `brink-lsp`'s persistent `analysis_loop` to resolve the
  pointer for the server process's whole life: the LSP never calls
  `std::env::set_current_dir`, so a session launched from a directory other
  than the project root (e.g. `native_root=/project` launched from cwd
  `/project/scenes`) silently confined against the wrong module for the
  entire session. The pointer now resolves through its own
  `conventions_pointer_key` (a relative pointer passes through untouched; an
  absolute one still strips against the root), and `root_relative_key`'s
  file-key semantics are untouched.

  **`@brink-lang/web` is affected by the silent-swallow half.**
  `EditorSession` never declares a `native_root` — its files are keyed by
  already tree-relative virtual paths with no OS-filesystem anchor, so the
  cwd bug could not bite there. But the silent-drop this issue reports for
  `brink-web` — a `[project] conventions` pointer that resolves to no real
  file in the project (most commonly: one discovered at a nested
  `brink.toml` document key) — took the "does not match any file" arm, which
  was a bare `tracing::warn!` returning zero diagnostics. `brink-web`'s wasm
  build has no `tracing` subscriber at all, so that warning reached nothing
  an embedder could ever observe: `compile_project`'s returned warnings
  stayed empty, indistinguishable from "everything is fine," while
  `ConventionsProjection` (what `explain_match` reads) stayed silently empty
  too. Both `conventions_confinement_diagnostics` (the off-db road
  `IdeSnapshot::analyze`/`EditorSession` actually run) and its db-direct
  sibling `conventions_confinement_diagnostics_query` now push a real `E169`
  diagnostic in this case instead of staying silent — one per declared claim
  handler, anchored on each handler's annotation, worded to blame the
  pointer/file mismatch itself ("does not match any file … fix the
  `conventions` pointer or the project layout") and covering both the
  typo'd/moved/deleted case and the nested-`brink.toml`-key case, never
  `conventions_module_diagnostics`'s "move it there" message, since there is
  no correct destination to name when the pointer doesn't resolve.

  Pinned by `conventions_pointer_key_ignores_the_process_cwd` (unit, red
  before this fix) plus the two-road e2e pair
  `conventions_confinement_survives_a_relative_pointer_with_native_root_and_a_nested_lsp_cwd`
  and `off_db_road_agrees_with_native_root_and_a_nested_lsp_cwd` (both
  reproduce the LSP's exact `native_root`/nested-launch-cwd scenario and
  assert the _confinement_ arm specifically, so a resolution regression
  cannot hide behind the new unresolvable-pointer diagnostic),
  `unresolvable_pointer_is_e169_naming_the_pointer_not_a_destination`
  (`brink-analyzer`), `an_unresolvable_conventions_pointer_is_e169_naming_the_pointer_not_a_destination`
  (`brink-db`), and `compile_project_surfaces_an_unresolvable_conventions_pointer_as_e169`
  (`brink-web`, exercised through the real wasm-exported `compile_project` entry
  point `packages/ink-editor`'s `ProjectSession` calls).

- 885ca6f: Fixed brink-db treating non-source documents (`brink.toml`, `.md`, `.json`,
  `.ink.json`) as ink source (issue #2329, the general follow-up to
  #2318/#2327). These files no longer lower through the ink frontend, join the
  project symbol index, or contribute diagnostics — a project's own
  `brink.toml`/README/oracle-regeneration JSON can no longer plant a bogus
  symbol-index entry or a bogus diagnostic just by sharing a session with real
  source files. Observable through `@brink-lang/web`'s symbol index/outline:
  the index and diagnostics streams the wasm package re-exports now only ever
  reflect real `.ink`/`.brink` source. The files stay in the session (config
  discovery still reads `brink.toml`; the editor still opens `.md` files as
  plain documents) — this is a classification fix, not deletion.

  Also fixed `file_language`'s case-sensitive extension comparison (`.INK` was
  classified inconsistently from `.ink` on a case-insensitive filesystem) —
  extension matching (`file_language`, `is_source_file`, and
  `has_recognized_source_extension`) is now case-insensitive throughout.

- 8f30776: `[project] entry` now has a real schema slot and wins over a host's `entryFile` argument (issue
  #2331, ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`").

  - `brink_project_config::ProjectConfig` gains `entry: Option<String>`, validated the same way as
    `conventions` (any non-empty string; existence/resolution is left to the consuming mount, kept
    dependency-free per #1234).
  - `EditorSession` (`@brink-lang/web`) tracks the discovered file's `[project] entry` and exposes it
    via the new `configured_entry()`/`EditorSessionHandle.getConfiguredEntry()` — `null` when no
    `brink.toml` was found, or one was found that doesn't set `entry`.
  - `ProjectSession` (`@brink-lang/editor`) now owns entry-file precedence: after
    `discoverProjectConfig` runs, a discovered `entry` that resolves to a real file in the session
    supersedes the constructor's `entryFile` argument (for both `compileProject()` and
    `getEntryFile()`); an `entry` that does NOT resolve to a real file falls back to the current
    `entryFile` and is reported through the existing `onProjectConfigWarnings` channel — no new
    warning channel invented. The `entryFile` constructor option is now only the configless fallback
    (and the seed path `brink.toml` discovery walks up from).
  - `mountStudio` (`@brink-lang/studio`) opens the initial tab from `project.getEntryFile()` (read
    after `initialize()`, so any config supersession has already happened) instead of its raw
    `entryFile` option.
  - `packages/brink-desktop`'s `resolveEntryFile` regex peek at `[project] entry` is deleted (not
    merely unused) — it shrinks to the plain configless-fallback chain, since `ProjectSession` now
    supersedes its guess whenever `brink.toml` sets a valid `entry`. Not independently versioned
    (`@brink/desktop` is private).

  The embedded playground's `?fixture=native` project (`packages/brink-studio/src/main.tsx`'s
  `NATIVE_FIXTURE`) already sets `entry = "story.brink"`, agreeing with the `entryFile` argument
  `main.tsx` passes for that fixture — this change is a no-op there by construction, not by luck; a
  test asserts the agreement holds (`config wins` test in `project-config-application.test.ts`
  additionally exercises a real mismatch to prove supersession, not just agreement).

- 76cc702: #2333: `EditorSession::apply_project_config`/`discover_project_config` now
  dedupe `brink.toml`-driven warning strings against the set returned by the
  previous call before returning. Every edit-flush of `brink.toml`
  re-analyzed the whole project and, when the file's warnings hadn't actually
  changed (a standing typo mid-edit elsewhere in the file, or simply
  re-applying an unchanged file), re-returned the identical warning text —
  the host (`brink-studio`'s `onProjectConfigWarnings`) appends every
  returned string to the 500-entry-capped Output log unconditionally, so a
  config-editing session was silently evicting real compile history one
  re-application at a time.

  Behavior: a warning already surfaced by the immediately preceding call is
  omitted; a genuinely new or changed warning still appends; a warning that
  clears (the file is fixed) and later reappears (undo, or the same typo
  retyped) appends again — the last-emitted set is replaced wholesale on
  every call, not accumulated, so "resolved" is representable. Deleting
  `brink.toml` entirely also clears the last-emitted set, so a later file
  reintroducing the same warning text isn't suppressed by a stale record.
  This only changes the JSON string array returned by these two methods; it
  does not touch `compile_project`'s diagnostics (the Problems-panel/
  db-direct-road surface stays exactly as before) and does not change when
  warnings first appear, only whether an unchanged repeat re-appends.

- d8ddd78: Internal refactor, no observable behavior change: `EditorSession::apply_parsed_config`
  now resolves `[project]`/`[lints]` into one `AnalysisOptions` and forwards
  `dialect`/`types`/`lints`/`conventions` onto its `IdeSession` through the new
  shared `IdeSession::apply_analysis_options` seam (issue #2334), instead of
  four hand-copied setter calls each individually change-guarded. The same
  field (`conventions`) had been dropped by this hand-written forwarding three
  times running across three separate `IdeSession` producers (#1880 → #2316,
  then #2317 → #2325) — routing every producer through one seam means a future
  `AnalysisOptions` field only needs a forwarding decision made once. Verified
  byte-identical against the full `brink-web` test suite (including the
  `acceptance_gate`) and the `#1005`/`#1397` precedence-tier regression tests.
- 246b800: The off-db analysis road (`session.analysis()` / `IdeSnapshot::analyze`)
  now runs the `[project] conventions` confinement/unconfigured checks
  (`E169`), matching the db-direct road (issue #2335).

  A pattern-claiming `@[convention(claims = "…", order = …)]` handler declared
  outside the project's configured conventions module — or declared at all
  when no `[project] conventions` is configured — used to be silently
  accepted by this analysis path (`analyze_with_modules` never read
  `opts.conventions`), even though the identical db-direct query
  (`conventions_confinement_diagnostics_query`) already flagged it.

  No `@brink-lang/web`-exported surface renders `session.analysis()` today —
  `compile()`/`EditorSession::compile_project()` and `editor_dto::
diagnostic_to_js` all go through the db-direct `compile` road instead, so
  `packages/ink-editor` and `packages/brink-studio` see no new squiggles from
  this change. The real beneficiary is `brink-lsp`'s `analysis_loop`, which
  calls the fixed function directly with no `ProjectDb` in between. This
  changeset is still required because `@brink-lang/web` re-exports the fixed
  function's behavior, but the observable delta for today's JS consumers is
  nil.

- 8e6a225: Prose dialect: a trailing `#tag` on a `CUE` or `PARENTHETICAL` line
  (§8d.4, `@VENDOR #(v.o.)`) no longer declines the claim outright (issue
  #2350).

  `hir::lower_native::element::candidate` strips the tag before pattern
  matching, exactly as issue #2077 already ruled for a scene heading's
  `[slug]`/`#tag`s — one literalness doctrine across every claimed shape.
  The stripped tag is recovered and delivered through the existing
  `Content.tags` channel, the same interim carrier heading tags already use
  pending #474's per-flow tag API. An `attach = StructName` claim (issue
  #2178) still declines a tag-bearing cue/parenthetical outright, for the
  same reason it already declines a tag-bearing heading: attach mode emits
  no `Stmt::Content` at all, so there is no line for the recovered tag to
  ride on.

- d4eab47: Issue #2351: `explainMatch`/`explainMatchDoc` now agree with the compiler
  on cue, parenthetical, compact-cue, and slugged/tag-bearing scene-heading
  lines — previously they misreported exactly these four shapes.

  `brink_ir::hir::classify::classify_line` matched a preset's pattern
  against a line's _whole raw text_, while the compiler's own claiming
  path (`hir::lower_native::element::candidate`/`try_claim`) matches
  against a _sub-node's_ extracted text (a cue's `CUE_NAME` alone, a
  parenthetical's inner `TEXT` alone, a compact cue's `CUE_NAME` segment,
  a scene heading's `SCENE_TITLE` stripped of its slug/tags). The two
  matchers structurally could not agree: a real `@VENDOR` cue line the
  compiler claimed reported a flat `matched: false` from `explainMatch`,
  because the `cue` handler's own pattern (with no `@` in its character
  class) can never match the whole `"@VENDOR"` text.

  `explain_match_impl` (`crates/brink-web/src/editor/explain_match.rs`)
  now finds the claim-candidate CST node under the cursor (via the new
  `brink_ir::nearest_element_candidate`) and classifies that node's exact
  sub-node text — the same input `try_claim` uses — instead of the raw
  line, whenever the file's native parse tree has one for this line.
  Falls back to the pre-#2351 raw-text walk unchanged for anything else
  (an ink-dialect file, or a line outside the five claim-candidate
  shapes). `ExplainMatchCache::explain` gained a new `node: Option<&SyntaxNode>`
  parameter for this — a node-derived classification is never inserted
  into the cache's raw-text memoization map (a chain-gated shape like
  `PARENTHETICAL` can select a different sub-node for byte-identical text
  depending on parser context alone, so caching it under the bare-text
  key would be unsound); it still reuses the cache's already-compiled
  pattern set.

  As a consequence, `winner.kind` (issue #2310) can now finally report
  `"cue"` and `"parenthetical"` for real `@NAME`/`(delivery)` lines — its
  own composition logic was already correct, it was simply never reached
  because the live walk missed those lines outright before this fix.
  `"bang_dispatch"` is still not reachable (a `!name`-dispatched handler
  is registered on a path this walk never consults at all) — tracked
  separately.

  Review follow-up: the claim-candidate node lookup now probes from the
  line's own first non-whitespace byte, not the caret's raw offset — three
  of the five claim-candidate shapes fuse their own trailing content into a
  child `CONTENT_LINE` (an indented cue's surrounding whitespace/newline
  sit outside the `CUE` node, a compact cue's literal dialogue and a
  `!name` bang-dispatch's remainder are both fused `CONTENT_LINE` children),
  so the previous caret-offset probe could return the wrong node — or a
  false claim for a bang-dispatch line the compiler never makes — purely
  because of which column the cursor happened to sit on. The answer no
  longer depends on caret column within the line.

- 79fdaf4: Analyzer/IDE: `ConventionsProjection` now carries a row for every declared
  `!name` sigil-dispatch (`@[element(args = "…")]`) handler, not just
  `@[convention]` claiming handlers (issue #2352).

  `ConventionsProjection::from_decls` gains a second row source
  (`dispatch_decls: &[DispatchHandlerDecl]`) and a new `dispatch` field,
  populated by `brink_db::queries::analysis::conventions_projection_query`
  from the conventions module's own `HirFile::dispatch_handlers`. Before this
  fix, a project with only a `!name`-dispatched handler projected to a
  completely empty `ConventionsProjection` — the handler was structurally
  invisible to every consumer reading `EditorSession`'s live projection
  (`self.session.db().conventions_projection()`), the same read
  `explain_match_impl` uses.

  Dispatch rows live in a separate `dispatch` list rather than merged into
  `entries`: a `!name` handler has no real precedence to compare against a
  claim handler's authored `order` (dispatch is an O(1) name-keyed lookup,
  never a ranked walk over competing handlers), and every row's `attach` is
  always `None` (`@[element]` has no `attach` clause at all). Wiring this row
  into `classify_line`'s interactive matching walk (so `explainMatch` reports
  `matched: true` for a real `!name` line) is left as follow-up work — this
  slice only makes the row exist and be reachable, which is what issue #2352
  asks for.

  Each dispatch row also carries a `dispatch_name`, separate from `name`: the
  handler's own function name is the declaration-site anchor, but the `!`
  -sigil spelling an author actually writes is the `name = "…"` alias when one
  is declared — a consumer matching a raw `!name` line against this
  projection must key off `dispatch_name`, not `name`, or an aliased handler
  becomes unfindable under its only author-writable spelling.

  **Known limitation, left open pending a ruling on #2352**: `!name` dispatch
  is file-local at the language level — a `!name` line only ever resolves
  against handlers declared in the same file — but `dispatch` is populated
  only from the ONE configured conventions-module file, the same scope
  `entries` uses. A `!name` handler declared in an ordinary (non-conventions)
  story file — the common case, since dispatch has no confinement rule the
  way `@[convention]` does — contributes no row here at all. See
  `ConventionsProjection::dispatch`'s own doc for the same limitation stated
  in full.

- d18f149: Editor: native (`.brink`) files now get inlay hints, color hints, and
  argument widgets (issue #2359).

  #2291 (PR #2358) fixed folding and `line_contexts` to route `.brink` files
  through the native CST instead of always ink-parsing the source text
  regardless of extension, but left `inlay_hints_impl`/`color_hints_impl`/
  `argument_widgets_impl` returning `[]`/`null` for a native file — there was
  no native-CST equivalent of the underlying `brink-ide` passes yet. This adds
  `brink_ide::inlay_hints::inlay_hints_native`,
  `brink_ide::color::color_hints_native`, and
  `brink_ide::argument_widgets::argument_widgets_native` (mirroring the ink
  passes' shape over `brink_syntax_native::SyntaxKind`) and wires the
  `is_native` dispatch in `crates/brink-web`, the same pattern #2358 used for
  folding. A `.brink` file now gets real inlay hints, color-picker hints, and
  argument-widget slot data instead of silently none at all.

- d44e75f: Fixed #2366: a mid-line block comment (`Prose /* mid */ here`) no longer
  fragments its `CONTENT_LINE` into two separate lines with the comment
  hoisted to `SOURCE_FILE` level. `mixed_content`'s catch-all arm now retries
  past a zero-progress stop the same way its `L_BRACE` sibling arm already
  did — but narrower than a blanket trivia-skip: only the comment token(s)
  are elided, so whitespace on either side of the comment survives (matches
  inklecate's own output for the `astrochili__narrator` corpus's
  `comments.ink`, which produces a double space where a comment used to sit,
  not a single space). A stray non-trivia stop token (`}`, `|`, `\` before
  newline) still breaks the loop rather than retrying, so this cannot spin.
  Parse-tree shape and diagnostics are editor-observable through
  `@brink-lang/web` — the previous spurious "expected newline at end of
  content line" diagnostic on a mid-line block comment is gone, and the CST
  now has one `CONTENT_LINE` instead of two.
- 07740e1: Fix #2764: the E116 `Option[T]`-condition-truthiness walk
  (`option_conditions.rs`) now descends into a lambda's own block body,
  wherever the lambda literal sits — a VAR/CONST default, a temp
  initializer, an assignment, a return value, a divert/tunnel/thread-start
  argument, a content interpolation, or a condition expression itself — the
  same descent `walk_expr_for_lambdas` added for E113 (#2762). Previously an
  `if`/`while`/choice condition on an `Option[T]` value sitting inside a
  lambda's own `|…| { … }` body was silently unchecked, unlike the identical
  condition written at top level.

  Only _statically classifiable_ conditions fire: a captured outer local, a
  global, or a direct Option-returning intrinsic call. A binding the lambda
  itself introduces (its own param, or a name its own block binds) is not
  classifiable from the enclosing def's finalized locals and is pruned out
  of the lookup before the block is checked, so it stays silently unchecked
  (the `RuntimeError::OptionTruthiness` runtime fault remains the backstop)
  rather than misclassified as a same-named outer binding.

  This makes new hard E116 errors (under `types = strict`) appear on
  `.brink` files with a _captured_ Option condition sitting inside a
  lambda's own body, in both the studio Problems panel and through
  `EditorSession`/`IdeSnapshot::analyze`.

- 1939b97: Fix #2772: the E116 `Option[T]`-condition-truthiness walk
  (`option_conditions.rs`) now visits `hir.root_content` — the file-scope
  content sitting before the first knot/stitch header. This is the third gap
  found in the same walk after #2764/PR #2768 fixed the other two (no `Expr`
  descent at all, and `check()` never walking `hir.variables`/`hir.constants`).
  Previously a condition on an `Option[T]` value sitting in root content got
  no E116 diagnostic at all, while the byte-identical condition written
  inside a knot fired correctly.

  Root content's own `~ temp` locals are now resolved through the same
  synthetic `DefinitionId` scheme `strict.rs::body_def_ids` (issue #1903)
  already established for this scope, rather than treating root content like
  a bare declaration value with no locals of its own.

  This makes new hard E116 errors (under `types = strict`) appear on `.brink`
  files with an `Option[T]` condition sitting in file-scope content before
  the first knot, in both the studio Problems panel and through
  `EditorSession`/`IdeSnapshot::analyze`.

- 77cd00a: Analyzer: fix a false-positive/false-negative hazard for expressions inside
  a lambda's own block or expression body, under `types = strict` (issue
  #2773).

  `MistypeCtx.locals`/`BodyTypes::locals` key local bindings by bare name,
  with no notion of lexical scope. `hir::visit::walk_expr`'s `Expr::Lambda`
  descent (issue #1685) has always walked into a lambda's own body as part of
  the ordinary expression tree, so every analyzer pass that classifies a
  `Path`/`Call`/`Index` expression from this map while visiting an expression
  — `int(x)`/`float(x)` domain checks (E078), `int(r)` range-refinement
  (E117), `contains(m, k)` key-domain checks (E152), `or`-coalescing operand
  typing, UFCS receiver resolution, and struct-construction field typing
  (E071) — was live-exposed to misattributing a lambda's own param or
  block-local temp the type of a same-named _outer_ binding of a different
  type, the moment the lambda body happened to reuse an outer name. A lambda
  parameter/temp genuinely shadowing an outer local now classifies from its
  own type (or "unclassifiable", never the outer binding's) throughout its
  own body, for every one of the checks above.

  Two of those consumers are **not** diagnostics-only, and both change
  observable behavior:

  - **`or`-coalescing changes emitted bytecode.** The analyzer records a
    `CoalesceShape` per chain step, which reaches `lir::lower::expr`'s
    `lower_coalesce_chain` through `coalesce_lir_lookup`. A chain whose
    left-hand operand is an unannotated lambda param shadowing an outer
    binding previously recorded `PreserveOption`/`Collapse` derived from the
    _outer_ binding's type — the wrong binding, so the wrong code. It now
    records `RuntimeCheck`, which is the honest posture for an operand whose
    Option-ness is not knowable at that point.

  - **UFCS receivers can now be a hard error where the code previously
    compiled.** "Unclassifiable" means silence for E071/E078/E116/E117/E152,
    but a UFCS receiver with no knowable type is `E142` ("annotate the
    receiver"). An _unannotated_ lambda param used as a method receiver, whose
    name shadows an outer binding, previously resolved from that outer binding
    and compiled; it now raises `E142`. This makes the shadowing case agree
    with the already-existing `E142` for any other unannotated receiver, and
    the fix is to annotate the lambda parameter.

- 8628395: Fixed a doubled `E116` diagnostic message. `option_conditions.rs`'s
  `check_condition` built its own `format!` that repeated
  `DiagnosticCode::E116.title()`'s wording verbatim right after the title,
  so an `Option[T]` truthiness condition (`if optionValue { ... }` instead
  of `== none`/`== some(x)`) rendered the sentence twice in a row.

  The message is now:

  > an `Option[T]` has no truthiness — test `== none` / `== some(x)` in the
  > condition (F27, docs/stdlib-spec.md §1.6)

  This is observable through `@brink-lang/web` — the diagnostic renders in
  the studio's Problems panel for both the db-direct and off-db analysis
  roads.

- 7c8480a: Issue #2781: the native `.brink` parser now reports `` expected `<` or end
of type name, found L_BRACKET `` (surfacing as `E037`) when a `var`/`const`
  type annotation is followed by `[` — e.g. `var x: Option[int] = none` —
  instead of silently reinterpreting the rest of the line as narrative prose
  and dropping the initializer. `[…]` is not the type-argument delimiter (the
  2026-07-27 angle-bracket ruling retracted `Option[T]`; `[…]` is reserved for
  array literals, #1490). `fn`/`flow` params, return types, and lambda params
  already failed loudly on this input (#2780); `var`/`const` annotation
  position was the one remaining silent-drop gap this closes.
- 88c6754: Fix #2782: an explicitly `: Option<T>`-annotated param — an ordinary `fn`
  param or a lambda's own param — now reaches the E116 `Option[T]`-condition-
  truthiness check (`option_conditions.rs`). Previously only an
  **inference-derived** `Option[T]` (e.g. `let r = some(3)`) was classified
  there; a written annotation was silently dropped before classification,
  even though `annotations::resolve` has handled `Option<T>` since #1552.

  Two fix sites, one per shape:

  - An ordinary `fn`/knot/stitch param: `infer::body::infer_def_body` already
    overlaid an unconstrained param's annotation onto the signature it exports
    (`InferredSig::params`), but never onto `BodyTypes::locals` — the
    bare-name-keyed map every body-level classifier (including this one)
    actually reads for a param/temp's type. Now overlaid there too, under the
    same "body wins, annotation only covers `Unknown`" firewall.
  - A lambda's own param: `pruned_locals_for_lambda` pruned a lambda's own
    bindings out of the enclosing scope's locals (issue #2773's shadowing
    fix) but never seeded the lambda's own annotation back in. Now seeded
    directly, excluding any param name the lambda's own block re-binds
    (mirroring `infer_lambda`'s identical guard on its own `self.annotated`
    seed) — with a positive shadowing test confirming this doesn't reopen
    #2773's hazard.

  This makes new hard E116 errors (under `types = strict`) appear on
  previously-clean `.brink` files with an annotated-Option-param truthiness
  condition, in both the studio Problems panel and through
  `EditorSession`/`IdeSnapshot::analyze`.

- 8db452d: Issue #2792: the native `.brink` parser now reports the same targeted
  message — `` expected `<` or end of type name, found L_BRACKET `` — at
  every position that reads a type name when it's followed by `[` (`fn`/`flow`
  params and return types, lambda params and returns, `let`, `var`/`const`,
  and struct fields), instead of each position's own incidental "expected
  NEXT_TOKEN" fallout (`expected R_PAREN, found L_BRACKET`, `expected PIPE,
found L_BRACKET`, `expected a braced body after the fn header`, and worse).
  A lambda's own return annotation (`|y: int|: Option[int] { none }`) used to
  produce **zero** diagnostics — the leftover `[int]` silently parsed as the
  lambda's body, dropping the real one — and now fails loudly with the same
  message. `[…]` is not the type-argument delimiter (the 2026-07-27
  angle-bracket ruling retracted `Option[T]`; `[…]` is reserved for array
  literals, #1490). Recovery (the parser-generated garbage each position
  leaves after the diagnostic fires) is unchanged — #2792 scoped that out as
  a separate, bigger design question.
- 2c7a43d: Analyzer: resolve a `#fn(target)` literal naming a declared list item that
  collides with a stdlib list verb (e.g. `pop`), instead of silently dropping
  the reference (issue #2830).

  `resolve_function`'s lookup chain (externals → knots → lists-by-full-name →
  variables → locals) fell through to `is_t1b_stdlib_name`'s silent "handled
  at LIR lowering" skip whenever a bare name matched both a real declared list
  item and a T1b stdlib verb name — the ref was neither resolved nor
  diagnosed, violating the `completeness` invariant every reference is either
  resolved or diagnosed. A real declared list item now wins over the
  stdlib-name fallback at a `#fn` literal site, mirroring `resolve_variable`'s
  existing precedence for `RefKind::Variable` refs and the "author symbol
  shadows builtin" doc comment already on `is_t1b_stdlib_name`.

  Restricted to `#fn` literal sites (`arg_count: None`) — a real call site
  (e.g. `push(arr, 5)`) keeps resolving to the stdlib verb regardless of a
  same-named list item, so an author's `LIST` declaration can never silently
  divert a stdlib call to a list-item lookup that faults at runtime.

  Observable through `@brink-lang/web`: resolution results and diagnostics
  change at these sites — go-to-definition/hover targets and what the
  studio's Problems panel renders.

- 59528ec: Issue #2837: `lower_call`'s resolved-target match now refuses a non-callable symbol kind with a new diagnostic, **`E183`**, instead of silently emitting a call against it. This is reachable from real author source (a `temp`/param called before its own declaration — a genuine forward reference) as well as from a defensive-backstop shape (`ListItem`/`Label`/`Stitch`/`Struct` at a call position), and is web-observable: the wasm editor's Problems panel now reports `E183` for the forward-reference shape on both analysis roads. Calling a T1b block-scoped temp (`~ { … }`) after its own block has closed continues to report `E082`, not `E183`, matching `lower_path`'s existing guard for the identical mistake.
- db3f8e4: Issue #2856: fixed a silent-drop bug where an author-declared symbol
  (`VAR`, knot, external, list, or local) sharing a name with a classic
  uppercase ink built-in (`RANDOM`, `FLOOR`, `CHOICE_COUNT`, …) never
  actually shadowed it — the reference was silently discarded at both the
  analyzer's resolution pass and, separately, at LIR-lowering's call-site
  codegen, with no diagnostic and a clean compile. `{RANDOM}` against a
  declared `VAR RANDOM = 42` rendered as empty text instead of `42`; a knot
  `=== function FLOOR(x) ===` called as `FLOOR(5)` ran the real built-in
  instead of the author's knot. Both are now fixed: a declared symbol always
  wins resolution first, matching the existing (and now-corrected doc
  comments') stated behavior for the T1b lowercase stdlib names, and the
  `E035` "name shadows a built-in function" warning at the declaration site
  is what it always claimed to be — informational, not a lie about what
  actually happens at the reference site. Also documents, and makes
  enforceable via a named predicate, the `brink-analyzer` `completeness`
  proptest's boundary around these reserved-but-shadowable names.
- bd95b30: Compiler: `x++`/`x--` on a bare variable inside a `~ { … }` block now
  actually mutates it (issue #2894).

  `blocks.rs`'s `BlockStmt::ExprStmt` arm had no postfix-to-`Assign`
  conversion the way `stmts.rs`'s classic-line arm does — so a bare-variable
  postfix statement inside a block lowered to a pure, discarded
  `lir::Expr::Postfix`: it computed `x + 1`/`x - 1` and threw the result
  away, with no diagnostic and no effect. `~ { x++ }` compiled clean and
  silently did nothing.

  `BlockStmt::ExprStmt` now converts a bare-variable postfix operand into a
  real `Assign { op: Add/Sub, value: 1 }`, mirroring the classic-line
  conversion exactly. A field-operand postfix (`~ { a.count++ }`) continues
  to refuse with the same non-suppressible `E074` issue #2185/PR #2897
  established for the classic-line spelling, routed through the identical
  `reject_field_projection_index_root` guard — this fix does not reintroduce
  that misroute for the block surface.

- dadf0ce: Fix #2903: an index-operand postfix (`a[0]++`, `m["k"]++`) compiled clean
  and silently never mutated anything, on both the `~ { … }` block surface
  and the classic-line surface — the sibling gap PR #2900's review found next
  to #2894's bare-variable fix.

  An `Index` operand is neither `Path` nor `FieldAccess`, so the
  field-projection guard `try_lower_postfix_stmt` already had never matched
  it, and `lower_assign_target`'s bare-`Path`-only match fell through to
  `None` — the postfix value was computed and discarded, the same
  silent-drop shape #2894 fixed for a bare variable, just on an index target.

  An index-operand postfix now routes through `lower_indexed_assignment`,
  the same take/mutate/write-back RMW discipline `a[0] += 1` already uses —
  proven correct for both a list index and a map key before relying on it.
  A struct-field-projected index root (`p.items[0]++`) still refuses with
  the same non-suppressible `E074` issue #2121 established for `p.items[0]
= v`, rather than silently misrouting; a plain field-operand postfix
  (`a.count++`) keeps refusing with the identical E074 issue #2185/#2897
  established, unaffected by this fix.

  The RMW sequence this routes to can splice several `lir::Stmt`s, which the
  classic-line statement dispatcher's single-`Option<Stmt>` return can't
  express — `mod.rs`'s top-level classic-line dispatch and `content.rs`'s
  `lower_inline_block` (choice-text inline conditionals/sequences) now
  intercept an index-operand postfix with their own multi-statement-splicing
  arm before it can reach that truncating fallback, mirroring the existing
  `try_lower_indexed_assignment` precedent for `~ a[i] = v`.

- 98d2ad2: Analyzer: `E063`/`E185` now also reach a dotted field-assignment target
  whose receiver is an unannotated `~ temp` initialized from a construction
  literal (issue #2906).

  `~ temp p = Point#{x: 0.0, y: 0.0}` followed by `~ p.bogus = 1` or
  `~ p.x = "s"` used to compile clean under `types = strict` — the
  dotted-assignment recording site (`check_declared_field_assign_target`)
  only ever resolved a Temp root's shape from an explicit ascription
  (`self.annotated`), never from the initializer's own inferred type, even
  though `~ temp p: Point = …` (the annotated spelling) already resolved and
  fired both diagnostics correctly. The fallback now consults the
  initializer's own inferred `Ty::Struct` shape whenever there is no explicit
  ascription, feeding it through the exact same
  `structs::check_field_assign_mismatch` fact/check seam `E063`/`E185`
  already use for an annotated temp or a `VAR`.

  Conservative by construction: an unannotated temp reassigned anywhere in
  its def's body — to a different concrete struct, or to anything the
  analyzer can't resolve at all (an unannotated `EXTERNAL` call's return,
  say) — withdraws the inferred shape rather than risk a false positive; a
  genuinely unresolved receiver (an unannotated function parameter, an
  unknown call result) still stays silent, unchanged. Reaches both
  `Stmt::Assignment` and the T1b `~ { … }` `BlockStmt::Assignment` form, and
  both analysis roads (`brink-db`'s db-direct `ProjectDb::diagnostics` and
  the off-db `IdeSnapshot::analyze`).

- 36d6630: Fixed #2960: a mid-line comment (`/* ... */`) inside choice text (start
  content before `[`, or between two `{...}` interpolations) no longer
  fragments the choice into a spurious `expected newline after choice`
  diagnostic with trailing text spilling into a bogus following content
  line. `choice_content_elements` and `choice_content_element`'s `L_BRACE`
  arm now retry past an elided comment the same way `content::mixed_content`
  already does (#2366/#2958), reusing the same `Parser::skip_comment_tokens`
  helper. Observable through `@brink-lang/web` as fewer/different parse
  diagnostics and CST shape for `.ink` sources with a mid-line comment in
  choice text.
- 3893794: Fixed #2976: a mid-line comment (`/* ... */`) inside an inline-alternative
  branch (`{a|b}`, `{cond: a|b}`, sequences, and multiline conditional
  branch bodies) no longer fragments the alternative into a destroyed parse
  (the `|` becoming an `ERROR` node and the closing `}` becoming
  `STRAY_CLOSING_BRACE`). `inline::branch_content`'s catch-all arm and
  `branchless_cond_body`'s `multiline_branch_text` call site now retry past
  an elided comment the same way `content::mixed_content` (#2366/#2958) and
  `choice::choice_content_elements` (#2960/#2974) already do, reusing the
  same `Parser::skip_comment_tokens` helper. (A matching retry was also
  added to `multiline_branch_body`'s `multiline_branch_text` call site for
  symmetry, but that site is unreachable today -- its loop's leading
  `skip_ws()` already elides comment trivia before that call site can see
  zero progress.) Observable through `@brink-lang/web` as fewer/different
  parse diagnostics and CST shape for `.ink` sources with a mid-line comment
  inside an alternative.
- dc35b98: Faster project compiles and recompiles (#460). The per-knot LIR chunk memos
  behind `compileProject` used to rebuild their whole knot-_invariant_ lowering
  environment — the flattened
  resolution lookup over every project resolution, the reconstructed struct-shape
  tables, and the file-id→path map — once per knot, so the LIR layer cost scaled
  as (knots × project size). It is now built once per project revision and shared
  by every knot.

  On a 50-file × 20-knot project the per-knot LIR layer drops from 34.3 ms to
  4.0 ms cold (0.8 ms → 0.2 ms on a one-line-edit recompile), and end-to-end cold
  compile from ~341 ms to ~307 ms.

  No observable output change: the compiled artifact is byte-identical (pinned by
  new cold-vs-warm `.inkb` identity tests and the existing incremental fuzz
  harness), diagnostics are unchanged, and no JS signature moves.

- ff1e121: Issue #530: `signature`/`db.signature` stayed the decls-only `signature_query`
  (`resolution_index_query` drops `Param`/`Temp` locals entirely — issue #517),
  so it always returned `None` for a local `DefinitionId` — a silent "hover
  shows nothing for locals" trap. Added `db.local_signature(file, def)`, a
  per-file locals path that resolves a local's own TM-2 `: type` annotation
  without merging the decls-only and full symbol indexes (per #531).

  `brink-ide::hover` now wires it in (`inferred_local_type_str`), reachable
  through `@brink-lang/web`'s `EditorSession` hover: a `Param`'s declared
  annotation still wins over inference exactly as before, and a `~ temp x:
type = …` ascription — previously skipped straight to body inference even
  when it disagreed with the declared type — now correctly wins too.

- e2e5ec4: Analyzer: static key-domain warning for `contains(m, needle)` (`E152`,
  issue #582, companion to #580).

  Under `types = strict`, a `contains(m, needle)` call where `m` is
  statically visible as a map and `needle` is statically visible as
  outside the int/string/bool key domain (a float, array, map, struct,
  function, LIST, divert-target, `Option`, range, `Weighted`, tower, or
  handle value) now emits a `Warning`-severity `E152` diagnostic: the call
  can never do anything but return `false` at runtime (#580's ruling), so
  the always-false result is now flagged at compile time instead of
  discovered as a silent, empty membership test. Reaches a literal needle,
  a global `VAR`/`CONST`-valued needle or receiver, and a call- or
  index-valued needle or receiver — anywhere the project's whole-program
  type inference can classify the expression. Deliberately does **not**
  flag a needle whose type is in the key domain but disagrees with the
  map's own declared key type (e.g. a `string` needle against a
  `map<int, _>` receiver), nor a `contains` call on an array receiver
  (no key-domain restriction there), nor anything under `types = gradual`
  (the runtime's own total `false` return stays the sole signal there).
  Re-levelable and suppressible through the project's `[lints]` table /
  `//brink-disable` like every other `Warning`-base-severity diagnostic
  code.

- 6fae1a6: IDE: quick-fix affordances for the T1c creation-site diagnostics
  (E079–E081) and the `call()`/`bind()` strict over-arity diagnostics
  (issue #744).

  `code_actions`/`resolve_code_action` now offer:

  - **E081** (`#fn(target, args…)` over-binding): "remove extra argument(s)",
    trimming the bound-argument list back to the target's declared param
    count.
  - **E080** (`#fn(target, args…)` unbound `ref` param): "bind ref
    argument(s)", appending the matching durable global `VAR`(s) — offered
    only when every unbound `ref` param through the target's last declared
    `ref` param has an unambiguous same-named `VAR` in scope, so the fix
    always leaves the call fully bound.
  - **`call(f, args…)`/`bind(f, args…)` strict over-arity** (`E063`, issue
    #733's checker): "remove extra argument(s)", trimming the call's trailing
    args back to the count the callee's known type accepts.

  `E079` (target is not a function definition) has no offered fix — no single
  mechanical rewrite recovers the author's intent. Both modules are
  ink-frontend-only (`#fn(...)` has no native-dialect spelling; the
  `call`/`bind` fix is scoped to the ink frontend in this PR, with native
  `.brink` sites tracked as a follow-up).

- 8c52feb: Analyzer: narrow effect rows at indirect/value call sites when the callee's
  origin is statically known (issue #872, docs/effects-spec.md §8's
  "read the concrete `EffectRow` off a stored `Ty::Fn`" precision rung).

  Previously, any call through a function value (`f(args)`, `call(f, args…)`)
  unconditionally forced the enclosing definition's effect row to the pessimal,
  touches-everything floor — even when the value provably came from exactly one
  `#fn(target, …)` creation site. Now, a call through a write-once local (or an
  inline `#fn`/`bind(…)`-chain literal evaluated right at the call site) whose
  origin is a single, statically-known def narrows to that def's real row
  instead, pulled in through the same SCC effect fixpoint a direct call already
  uses. The narrowing is proven sound before it's trusted: a local reassigned
  more than once anywhere in its body, or an origin that can't be traced to a
  single def, keeps the old pessimal floor unconditionally — conservative-total
  is never traded for precision.

  This is observable through `@brink-lang/web`'s effects-diff/hover surfaces
  (brink-ide's `effects()` display) and `brink-db`'s emitted `EffectRows`
  table: a definition that calls only through a known fn-value local now shows
  a real, non-opaque row instead of "touches everything" where it previously
  did.

- aadc9b5: HIR overlay: conditional/sequence arm prose now projects `content` spans
  (issue #981).

  `hir_projection::project_hir` walks into conditional/sequence branch
  bodies correctly, but the ink-compat lowering (`brink-ir`'s
  `hir::lower::block::{branch,branchless}`) always flushed a branch-body
  `Content` node with `ptr: None` — those bodies have no per-line
  `CONTENT_LINE` wrapper node the way a top-level line does, so `content.ptr`
  was unconditionally absent. `ContentAccumulator` now tracks the covering
  source range of the raw tokens (text/glue/escape/inline-logic) it buffers
  for a branch body and stamps a synthetic-but-real-range `Provenance` on
  flush — the same posture `conditional_with_expr::branchless_first_arm_span`
  already uses for a branch's own span: it never resolves back to a live
  syntax node, but carries an exact byte range for span-consuming tools.

  Content nodes inside conditional/sequence arms (both the branchless implicit
  first arm and explicit `- cond:`/`- else:` arms) now emit their own `content`
  span, nested within the construct's mark, instead of being covered only by
  the whole-construct `Conditional`/`Sequence` mark. Top-level prose and the
  construct-extent spans themselves are unchanged.

  This also changes compiled output: `LineEntry.source_location` (part of
  `StoryData`'s line table, reachable through `EditorSession`/`@brink-lang/web`)
  is now populated for arm content lines that previously had none.

  Two further editor-observable consequences (surfaced by review):

  - Prose-only arms now also project their `ConditionalBranch`/
    `SequenceBranch` CONTAINER spans: `hir_projection`'s `stmt_extent`
    derives a branch's extent from its statements' ptrs, so a body whose
    only statements were ptr-less `Content` previously projected no branch
    container at all.
  - Line-context weave classification follows: arm prose lines inside a
    choice body report `w=1/ConditionalBranch` (depth inherited from the
    enclosing weave per `derive_weave`'s documented branch convention)
    instead of the scaffold fallback's `w=0`, and lines inside a
    `{ stopping: … }` block classify as `SequenceBranch` (weave depth 0 at
    top level) instead of being pattern-matched as `GatherContinuation` —
    the latter was a misclassification: those lines are sequence branches,
    not weave gathers.

- 55cc2b1: Analyzer: the unset-`types` default is now dialect-keyed (NS-A9, ruled
  2026-07-19) — a `"brink"`-dialect session with no explicit type policy
  resolves `types = strict`; a `"strict-ink"` session resolves `gradual`
  exactly as before. Resolution happens at one seam
  (`brink_analyzer::resolve_type_policy`), and an explicit choice always
  wins: `setTypePolicy(...)`, a `brink.toml` `types` key applied through
  `applyProjectConfig`, or the CLI's `--types` all override the
  dialect-keyed default.

  Observable through `@brink-lang/web`: a brink-dialect editor/compile
  session that never calls `setTypePolicy` now surfaces the strict-mode
  diagnostics (`E065`/`E066`/`E067`, narrowed coercion lattice) that
  previously required an explicit `setTypePolicy("strict")`. Opting out is
  `setTypePolicy("gradual")` or `types = "gradual"` in `brink.toml`.

  Also: `setTypePolicy` with an unrecognized value now behaves like never
  calling it at all (the dialect-keyed default stays in effect) instead of
  being treated as an explicit gradual opt-out — carrying the pre-NS-A9
  "any other value keeps the default" contract forward, so garbage input
  can never silently opt a brink session out of strict.

  The oracle-anchored strict-ink surface is untouched by construction:
  strict-ink + unset `types` resolves `gradual`, and strict-ink + explicit
  `strict` remains the `E064` config error.

- 46eb61b: Native parser: `{|…}` is always a stopping-sequence alternation (ruled 2026-07-22, correcting the earlier "malformed lambda" clause). `{|x| x}`, `{|heads|tails}`, and `{|heads| tails}` are all valid two-branch stopping-sequences; the fragile space-after-separator "malformed lambda" heuristic is removed. A lambda in content position is spelled `{(|x| x)}`. Observable through the web editor's diagnostics for `.brink` files (a `{|x| x}` that previously errored no longer does).

## 0.14.0

### Minor Changes

- 9481137: `brink.toml` — the project settings file for dialect + type policy (#1005).

  New API surface: `EditorSessionHandle.applyProjectConfig(toml: string): string[]`.
  Parses a `brink.toml`'s `[project] dialect`/`types` and applies it to the
  session (dialect/type-policy warnings for unrecognized keys are returned,
  never thrown). Call it once at session construction, before any explicit
  `setLanguageDialect`/`setTypePolicy` — those calls always override the file
  (the file supplies the default; explicit calls win), matching the new
  `brink compile`/`brink ide` behavior: both now discover a `brink.toml`
  (walking up from the entry file to the nearest ancestor) and apply its
  `[project] dialect`/`types`, with `--dialect`/`--types` overriding the file
  when actually passed. A missing `brink.toml` changes nothing — no
  regression for existing consumers that don't ship one.

### Patch Changes

- a6e8a6a: Analyzer: strict-mode `E078` (`int()`/`float()` out-of-domain argument)
  no longer classifies only literal-shaped arguments (issue #983, sibling of
  #670).

  `conversions::check`'s domain check previously only recognized a
  divert-target expression, a LIST literal, or a `#[...]`/`#{...}`/`Name#{...}`
  collection/struct literal passed _directly_ as the `int(x)`/`float(x)`
  argument — a variable-, call-, or index-valued argument with a statically
  provable out-of-domain type slipped through uncaught at compile time (still
  caught at runtime by the `InvalidConversionDomain` fault, but only strict
  mode's compile-time convenience was missing it).

  `conversions::check` now reuses `structs::classify_expr_ty`/
  `structs::MistypeCtx` verbatim — the exact inference-substrate
  classification issue #670 added for `structs::check`'s own `E071` — to
  resolve a `Path` (param/temp via `BodyTypes::locals`, or global `VAR`/CONST
  via its declaration-derived type), a `Call` (the resolved callee's
  `InferredSig::return_ty`), or an `Index` expression (its base's classified
  array-element/map-value type) to a concrete `Ty`, then checks whether that
  `Ty` falls outside the permitted `int`/`float`/`bool`/`string` domain.
  Whenever the resolution lands on `Unknown` or `Conflicted`, the argument
  stays silently unchecked — the same gradual-mode conservatism the literal
  check already had.

## 0.13.0

### Patch Changes

- 17ad933: Strict typed mode now consumes host-manifest external signatures on the
  compile path, and the compile/warnings diagnostic channel gained `code` and
  real `range` fields (#1004).

  Under `dialect = brink, types = strict`, a `compileProject` (or
  `compileFragment`) whose host manifest types an `EXTERNAL`'s params no longer
  reports those params as escaping strict inference — the manifest
  `ManifestParam.ty` resolves the param the same way it already did for
  hover/pickers. Each registered `EXTERNAL` declaration is escape-checked
  against the exact `collect_external_sigs` resolution that seeds call-site
  argument checking (one shared helper across the analysis and compile paths),
  so a manifest-typed external stays clean while a genuinely unresolvable
  declared type (an empty `ty`, or one naming a semantic type absent from the
  manifest `types` vocabulary) is still reported — anchored at that external's
  own declaration span rather than collapsing onto one arbitrary line. An
  `EXTERNAL` with no manifest entry at all stays unchecked, as before.

  Additive wire-shape change on the `CompileResult.warnings[]` diagnostic
  objects (also `compile` / `compileFragment`): each entry now carries

  - `code` — the structured diagnostic code string (e.g. `"E065"`), so
    consumers can filter/group programmatically instead of string-matching
    `message`; and
  - `start` / `end` populated from the diagnostic's real source span (external
    escapes previously would have anchored at a fallback location).

  Existing fields (`message`, `start`, `end`, `severity`, `file`) are unchanged;
  `code` is purely additive.

- f53c6c7: `load_state`/`load_journal` (and any other JSON deserialize boundary that
  walks a `Value::Map`) now reject a crafted or corrupted save payload that
  carries a duplicate map key with a decode error, instead of silently
  keeping the last occurrence (#985, follow-up to #909's content-based
  `OrderedMap` equality).

  `OrderedMap`'s `Eq` is content-based and assumes every key appears at most
  once. Before this fix, `serde`'s derived `Deserialize` for `OrderedMap`
  walked the wire `entries` list verbatim, so a hand-crafted save/journal
  JSON payload with a repeated key could construct a map that violated that
  invariant. `OrderedMap` now has a hand-written `Deserialize` that rejects a
  repeat with a decode error (never a panic) — the same duplicate-key
  rejection the `.inkb`/`.inkt`/transcript binary codecs already apply on
  their own `VAL_MAP` decode paths. A save/journal file with no duplicate
  keys round-trips exactly as before; this only changes behavior for
  already-invalid input.

- 7e8aa7f: Analyzer: strict-mode `E066` (Conflicted-escape) no longer spuriously fires
  on a temp whose only "conflicting" use was a dotted field read (issue #994).

  A dotted `Path` (`t.field`) whose head resolves to a `Param`/`Temp` reaches
  the TM-4b resolution fallback (docs/typed-mode-spec.md §6), which maps the
  whole multi-segment path's range to the _head_ variable's `DefinitionId` —
  there is no static field-type table yet, so `t.field` and bare `t` were
  indistinguishable to the body-inference pass's usage-observation step. That
  step was folding the field-read's usage-context type back into the _head_
  temp's own accumulated type, manufacturing a `Conflicted` join (and a
  spurious `E066`) whenever the two disagreed, even though the temp itself
  was never actually misused. A dotted head resolving to a global
  `VAR`/`CONST` never had this problem (cross-type-reassignment detection for
  globals isn't implemented in this slice) — a `Param`/`Temp` head now gets
  the same treatment: a dotted field read is never folded back into the
  head's own type, only a bare (single-segment) reference is.

- b9a86e2: FS-3w review-fix cluster (#999, #1000).

  - **`FlowHandle.continueMaximally` is now capped**, matching the Rust
    runtime's `continue_maximally` (#999). It forwards to a new raw
    `continue_flow_maximally` wasm binding (`StoryRunner`/`WebSession`,
    backed by `Story::continue_flow_maximally_shared_with`) instead of
    looping the single-line `continue_flow` client-side without a bound. An
    infinite-emitting flow now throws at the runtime's
    `FlowInstance::LINE_LIMIT` (10,000 lines/turn) — the same
    `RuntimeError::LineLimitExceeded` shape `continueStory`'s cap already
    surfaces — instead of growing an unbounded array and hanging or
    exhausting memory on the host.
  - **`StorySessionHandle.spawnFlow` now returns a `FlowHandle`**, aligned
    with `StoryRunnerHandle.spawnFlow` (#1000). `StorySessionHandle` also
    gains `flow(name)` and `continueFlowMaximally(name)` to match. Session
    consumers can now drive a spawned flow via the flow-addressed API the
    same way runner consumers already could.

## 0.12.0

### Minor Changes

- 6cb663a: FS-3w — flow-addressed web surface (slice 1 of FS-3, issue #978).

  New API surface, shipping against today's runtime so consumers migrate the
  interface shape early (FS-3r later changes behavior, not interface):

  - **Flow handles.** `StoryRunnerHandle.flow(name)` and the new return value
    of `spawnFlow(name, path?)` are addressable `FlowHandle` objects — each
    spawned/ambient flow has its own `Line` stream via `continue()` /
    `continueMaximally()`, plus `choose()`, `debugSnapshot()`, and
    `destroy()`. Thin views over the existing name-addressed flow methods.
  - **Story-level drive is documented sugar for the primary flow.**
    `continueStory` / `continueSingle` (and the async variants) drive the
    always-present default flow. No behavior change — existing consumers are
    unchanged.
  - **`Line` gains the `"suspended"` type** (a flow parked at an `await`).
    Runtime-unreachable until FS-3r — the E052 fence keeps `await` from
    lowering, so nothing constructs it today; it ships now purely so the API
    shape is stable.
  - **`wakeCheck()`** (on `StoryRunnerHandle`, `StorySessionHandle`, and the
    raw `WebSession`) re-evaluates parked flows' wake conditions and returns
    the woken flow ids. Returns an empty list until parks exist (FS-3r);
    dirty-tracking is not built here.

## 0.11.1

### Patch Changes

- c246a4a: Analyzer: new `E106` warning for statically-visible non-key-domain
  map-literal keys (docs/t1b-surface-spec.md §3, issue #598).

  `#{key: expr, …}` map-literal keys are ratified to the int/string/bool
  domain at runtime (`RuntimeError::InvalidMapKeyType`). §3 already claimed
  "the analyzer warns on statically-visible non-key types", but nothing
  implemented it — `MapLiteral` lowering did zero key-domain checking, so a
  float, array (`#[...]`), nested map (`#{...}`), struct (`Name#{...}`),
  function-value (`#fn(...)`), or ink `LIST` literal used directly as a key
  compiled silently and only failed at runtime.

  `brink-analyzer::map_keys::check` now flags every such entry with `E106`
  (warning severity), wired into `per_file_diagnostics` unconditionally under
  `dialect = brink` (map literals don't exist under `strict-ink` at all —
  already rejected whole by the dialect gate's `E051`). Policy-independent
  like the construction-literal duplicate-field check (`E084`): fires
  identically under both `types = gradual` and `types = strict`, no shape
  resolution needed. A dynamic key (a variable, call, index, or any other
  non-literal expression) is not statically visible and is never flagged —
  the runtime fault remains the sole backstop for those.

  Observable through `@brink-lang/web`: any brink-dialect project compiled
  through the wasm runtime with a non-key-domain literal map key now surfaces
  this new diagnostic in the returned diagnostics array.

- ae66340: Issue #628: `InferredType::List` (the phase-0 `signature()`/hover stub) now
  carries the declaring LIST's name instead of dropping it. A VAR initialized
  directly to a list literal (`VAR w = (sunny)`) previously fed
  `infer::collect_globals` an `Unknown` type via a lossy `InferredType -> Ty`
  conversion — weakening typed-mode inference for list VARs and, under
  `types = strict`, spuriously tripping the Unknown-escape check (`E065`) for
  anything assigned from such a VAR, unlike sibling nominal types
  (`Ty::Struct`, `Ty::Handle`) which were already treated as clean.

  Observable through `@brink-lang/web`: hovering a list-literal-initialized
  VAR/CONST now shows its nominal type, e.g. `w: list<Weathers>`, instead of
  the bare `w: list` it showed before.

- 7baa01f: brink-fmt: canonicalize whitespace around type-annotation colons (#642).

  Type annotations in knot parameters, return types, VAR/CONST/LIST declarations,
  TEMP declarations, and struct fields now render with canonical spacing:
  `name: type` (no space before colon, one space after), regardless of source
  spacing. This normalizes `name:type` (no space), `name: type` (space), and
  `name:  type` (multiple spaces) to a consistent canonical form, matching the
  ink language reference's documented style.

  Changes apply to:

  - Knot headers: `=== function f(x:int, y: int): int ===` → `=== function f(x: int, y: int): int ===`
  - Declarations: `VAR gold:int = 100` → `VAR gold: int = 100`
  - Logic lines: `~ temp name:string = who` → `~ temp name: string = who`
  - Struct fields: `STRUCT P = #{x:int, y: float}` → `STRUCT P = #{x: int, y: float}`

  Formatting remains idempotent: re-formatting an already-canonical annotation
  produces identical output.

  Observable through `@brink-lang/web`: the editor's "Format knot" code action
  now produces canonicalized annotation spacing in formatted output.

- aa43bb6: Analyzer: `E071` (mistyped struct construction field, strict mode) now
  classifies variable-, call-, and index-valued initializers, not only
  literal-shaped ones (issue #670).

  `STRUCT` construction-literal type checking previously only classified
  literal-shaped field initializers (scalars, arrays, maps, nested struct
  literals) — a variable, function call, or indexing expression stayed
  silently unchecked, deferring entirely to the runtime fault. `E071` now also
  consults the whole-project inference substrate (`BodyTypes::locals` for a
  param/temp, the declaration-derived type for a global `VAR`/`CONST`, the
  resolved callee's `InferredSig::return_ty` for a call, and the base's
  classified element/value type for an index) when the initializer's own
  shape isn't literal. Whenever that resolution lands on `Unknown` or
  `Conflicted` — unresolved, unannotated, or genuinely contradictory — the
  field stays silently unchecked, same "Unknown never disagrees" posture as
  every other gradual-mode-aware check in this analyzer.

- edf92bc: Added the M-2 module imports + visibility surface (docs/modules-spec.md
  §2/§4/§7), building on M-1's module name model.

  - **`IMPORT` grammar** — both forms: bare `IMPORT { a, b AS c } FROM mod`
    and qualified `IMPORT mod`. `FROM`/`AS` stay contextual soft keywords;
    only `IMPORT` is reserved. Superset-parsed always; the brink-dialect gate
    rejects `IMPORT` under strict-ink (E051-class), like `#@module`.
  - **`#@private` / `#@public` visibility** on every importable definition
    (knot, function, VAR, CONST, LIST, STRUCT). Effective visibility follows
    declaration-flips-default: a declared module defaults private, an
    undeclared stem-module defaults public, and the per-definition directive
    overrides that.
  - **Diagnostics** (§7): private-cross-module reference (E087), unresolved
    import (E088), duplicate import (E089), self-import (E090), qualified
    ambiguity code reserved (E091), redundant-override warning (E092), and
    conflicting visibility directives (E093). `#@private`/`#@public` are
    brink-dialect-gated under strict-ink (E051).

  Compat: purely additive and brink-gated. The entire pre-modules world keeps
  visibility public and stays in the permeable flat namespace, so no existing
  story's resolution changes.

- d350551: T1d-2 (#767): manifest handle-kind vocabulary + the `handle<K>` typed-mode
  annotation form (`docs/t1d-spec.md` §3). A registered `HostManifest` can now
  declare a handle kind — `{ "name": "AudioInstance", "base": "handle" }` — and
  the brink-dialect typed annotation grammar gains `handle<K>` (`docs/typed-mode-spec.md`
  §3's first amendment), resolving to a new `Ty::Handle(K)` lattice point:
  pointwise kind match, cross-kind = `Ty::Conflicted` (the #627 lattice). Under
  `types = strict`, a mismatched/unregistered handle kind reuses the existing
  `E065`/`E066`/`E061` machinery — no new diagnostic codes. `Ty::Fn` composes
  with handle-typed params/returns for free (the existing pointwise row
  unification needed no special-casing).

  Observable through `@brink-lang/web`:

  - `HostManifest`'s `BaseType` (`packages/wasm-types`, re-exported by
    `@brink-lang/web`) gains a `"handle"` variant — a host can register
    `{ "base": "handle" }` semantic types.
  - `setHostManifest`'s diagnostics now recognize `handle<K>` annotations: a
    `handle<K>` naming an undeclared/unregistered kind reports `E061` (same
    code, extended message); a declared kind resolves cleanly.

  Scope: this slice wires the manifest vocabulary, the grammar/lattice, and
  the annotation-firewall/diagnostic-content seams (`per_file_diagnostics`,
  `strict::check`'s escape-exemption path). It does not thread the manifest
  through the salsa fine-grained-incremental type-inference substrate
  (`brink-db`'s FG-2 `solve_scc_query`/`call_edges_query` pipeline, or the
  non-salsa `infer_project`/`signature()` seams) — so a genuine cross-kind
  handle mismatch detected purely from body-usage inference (as opposed to an
  explicit annotation) isn't caught yet. Flagged as a follow-up, not silently
  dropped.

- 3c1e1e1: Host semantic-access enforcement for `#@private` definitions (M-2b,
  docs/modules-spec.md §4 boundary rules 2/3), building on M-2's compile-time
  visibility surface.

  - **Per-definition visibility compiled into `StoryData`** — a new optional
    `.inkb`/`.inkt` `Visibility` section (tag `0x0E`) enumerates every
    `#@private` definition's `DefinitionId`. Omitted entirely for all-public
    stories, so the entire pre-modules corpus stays byte-identical and no
    format version bump is needed. Writer + reader + round-trip land together
    for both codecs.
  - **Runtime refuses host semantic access to private defs.** With visibility
    enforcement on (the default), `getVar`/`setVar` on a `#@private` variable
    no-op (`undefined`/`false`), and `goToPath`/`goToPathWithArgs`/`runKnot`/
    `callFunction` into a `#@private` knot or function error. The host is
    outside every module.
  - **Persistence is unaffected.** Save/load/journal/replay serialize the whole
    state, including private cells — persistence routes through `DefinitionId`,
    never the enforced name-based host surface, so pause/resume still holds.
  - **Documented dev-tooling override (play-from-here).** A new
    `setDevVisibilityOverride(allow)` on the story runner and session runs the
    story with enforcement off so editors and debug hosts can start flows at
    private knots and inspect private state; the studio's "play from here"
    sessions enable it automatically. Production hosts leave it off. A host
    capability, not a language switch — the compiled program is identical
    either way.

- c03a73a: M-2c: public cross-module resolution now requires an `IMPORT`
  (docs/modules-spec.md §2), completing the M-2 module surface.

  - **Import-required resolution (`E025`)** — a reference resolving to a
    _public_ definition in another **declared** module which the referring
    file did not `IMPORT` is now `E025` with a did-you-mean-`IMPORT` message.
    Bringing the name in (bare `IMPORT { name } FROM mod`) or importing the
    module qualified (`IMPORT mod`) clears it. The restriction keys off the
    _target's_ module being declared, so the permeable legacy world is
    untouched: a plain multi-file `INCLUDE` project with no `#@module` is one
    big default-public module and every cross-file bare reference keeps
    resolving byte-identically (§3). Only genuinely multi-_declared_-module
    projects are constrained; strict-ink and the existing single-module brink
    corpus resolve exactly as before.
  - **`E091` qualified ambiguity** — a `IMPORT mod` (qualified) whose module
    name also names a definition visible bare in the same file makes `mod.y`
    ambiguous; flagged at the import (fixed with an alias).
  - **`E092` redundant-override warning** — a `#@public`/`#@private` that
    merely restates its module's visibility default is now covered by
    end-to-end reachability tests.

- 83717d3: T1d-2b (#774): threads the registered `HostManifest`'s handle-kind
  vocabulary through `infer_project`/`solve_scc` (and `brink-db`'s FG-2
  `signature_query`/`solve_scc_query` salsa substrate) into inference —
  `docs/t1d-spec.md` §3's remaining gap, disclosed as deferred in T1d-2
  (#767, PR #769). `handle<K>` param/return/temp annotations now resolve to
  `Ty::Handle(K)` during body-usage inference, not just at the
  `signature()`/annotation-firewall seam.

  Observable through `@brink-lang/web`: under `types = strict` with a
  registered `HostManifest` declaring two or more handle kinds, a genuine
  cross-kind handle mismatch detected purely from body-usage inference (e.g.
  two locals of different declared handle kinds compared or reassigned
  together, with neither side's slot independently exempted by its own
  annotation) now reports `E066` (Conflicted-escape) — reusing the existing
  TM-3 machinery, no new diagnostic code. This is the #767 acceptance
  criterion ("binding declared `handle<AudioInstance>` rejects
  `handle<Timer>` at compile time") becoming reachable end-to-end. `types =
gradual` is unaffected — TM-1 inference stays advisory-only there,
  byte-identical.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — vanilla ink has
  no handles by construction, so this is oracle-inert.

- 302c6a2: Added the M-3 renames surface (docs/modules-spec.md §5), completing the
  modules spine: `#@was(old_name)` on modules and definitions, a compiled
  old→new `DefinitionId` alias table in `.inkb`/`.inkt`, and a rehydration
  miss-path lookup that rebinds saved state deterministically instead of
  silently orphaning it under a stale id.

  - **`#@was(old_name)` directive** — on a file-level `#@module` declaration
    (records the module's rename) and on any definition (VAR, CONST, LIST,
    EXTERNAL, knot, stitch). Brink-dialect-gated, like `#@module`/`#@private`.
    A self-alias (`old_name` equals the current name) warns "nothing to
    migrate" (E095); a missing/empty argument is E094.
  - **Compiled `AliasTable` section** (`.inkb` format v5, section tag
    `0x0F`, since `0x0E` was independently claimed by the M-2b `Visibility`
    section) — one-byte section-locally-versioned old→new `DefinitionId`
    rows, sorted for the runtime's binary-search lookup. Matching `.inkt`
    text atoms (`(alias_table (alias $old -> $new))`). Empty for every story
    that uses no `#@was`, including the entire pre-M-3 corpus.
  - **Rehydration miss-path lookup** — `Story::load_state`/the free
    `load_state` function now consult the alias table when a saved visit/
    turn-count id, or a divert-target/fn-token/closure-target id embedded in
    a saved global's value, doesn't match the current program. Still
    unresolved after that surfaces a teaching message in the new
    `LoadReport::unresolved_renames` field (only for a program that actually
    carries alias-table entries — an ordinary content edit with no `#@was`
    stays exactly as silent as before).
  - Retrofits the pre-existing silent save-break on a plain knot rename (no
    module involved) with the same machinery.

  Compat: the `.inkb` format version bumped 4 → 5 (a brand-new mandatory
  section, not part of the v4 RFC's pre-reserved inventory) — checked-in
  `.inkb` artifacts regenerate. `LoadReport` gained a field
  (`unresolved_renames`), changing the JSON shape `StoryRunner::load`/
  `load_bytes` return. The alias table itself is additive and brink-gated;
  the entire pre-M-3 corpus emits an empty table and sees no behavior
  change.

- 4a08940: Close the `FlowInstance`-level host visibility gap left by M-2b (#772/#781):
  `begin_function_eval`/`begin_function_value_eval`/`choose_path_string(_with_args)`
  now refuse `#@private` definitions on any `FlowInstance` driven directly,
  not just through `Story`.

  - **`WebSpeculation.goToPath`/`.evalFunction`/`.resumeFunctionEval`** (the
    wasm bindings over `brink_runtime::Speculation`, which drives a
    `FlowInstance` clone directly rather than a `Story`) now correctly refuse
    a `#@private` knot or function with the same `PrivateAccess` error the
    `StoryRunner`-level `go_to_path`/`call_function` surface already enforced
    — previously a speculative fork could read past a private boundary that a
    live `Story`-mediated session already blocked.
  - Same documented dev-tooling override: a `FlowInstance`'s own visibility
    enforcement flag mirrors `Story`'s, and `Story` keeps every flow it owns
    (default, named, shared) synced to its own setting, so a `Story`-level
    `setDevVisibilityOverride`/play-from-here session behaves identically
    whether or not it composes a `Speculation`.

- b86fee8: M-2c stopgap: cross-**declared**-module same-name duplicate definitions
  are now a hard error under `dialect = brink` (issue #784).

  - **`E096`** — two _declared_ modules (`#@module(name)`, different names)
    each defining a same-name, same-kind symbol (a knot, stitch, VAR/CONST,
    LIST, STRUCT, EXTERNAL, or label) is now a compile error, reported at
    _both_ definitions' spans. Flat resolution (unchanged by this stopgap —
    true import-scoped resolution is tracked separately, #790) binds a bare
    name to whichever declared-module definition merge happens to see first,
    so two declared modules sharing a name silently made that binding
    order-dependent. Escalating to a hard error makes flat resolution correct
    by construction until scoping lands.
  - A duplicate _within_ one declared module (same module name on both
    files), or involving any undeclared/legacy file, keeps the existing
    `E022`/`E023`/`E026` warning — unchanged.
  - Gated to `dialect = brink` only: under `strict-ink` (the default), this
    code never fires — the compat/oracle corpus is untouched.

- 1e1be68: Closed two M-3 rehydration miss-path gaps disclosed by the renames PR
  (#782 / docs/modules-spec.md §5): a saved VAR/CONST/LIST global whose own
  name was renamed (`#@was`) — declared-module or bare — now rebinds through
  the compiled alias table instead of being dropped as unknown, and a saved
  `Value::List`'s active items/origins now deep-rebind on a rename exactly
  like `Array`/`Map`/`Record` already did.

  - **`SaveState` gains a `global_ids` field** — each saved global's
    compiled `DefinitionId` at save time, keyed by the same name as
    `globals`. Additive and `#[serde(default)]`, so an older save missing
    the field just falls back to the pre-existing unknown-global report — no
    behavior change for saves that don't use `#@was`. This is what lets the
    miss path recover a renamed global's identity: a VAR/CONST/LIST living
    in a **declared** module hashes as `(module, name)`, so the bare name
    string alone can't reconstruct it once the name itself changed.
  - **`Value::List` is now deep-rebound** — `load_state`'s recursive
    id-rebind walk previously covered `DivertTarget`/`FnRef`/
    `VariablePointer`/`Closure` and their `Array`/`Map`/`Record` containers,
    but fell through to a no-op for `Value::List` itself; its `items`/
    `origins` `DefinitionId`s are now walked and rebound the same way.
  - A global-name miss that resolves via the alias table rebinds silently
    (same discipline as address/global-pointer misses); still unresolved
    (only checked for a program that carries alias-table entries at all)
    reports through `LoadReport::unresolved_renames` alongside the existing
    `unknown_globals` entry.

  Compat: `SaveState`'s JSON shape gains one field (`global_ids`) — decoders
  that deserialize leniently (ignore unknown/extra fields) are unaffected;
  `StoryRunner`/`StorySession`'s `save_state`/`load_state` round-trip it
  transparently.

- c36b8c4: Issue #786 (T1d follow-up): extends the strict call-checking machinery to
  `EXTERNAL` binding call sites — a manifest-registered binding declared to
  take `handle<AudioInstance>` now rejects a `handle<Timer>` argument at
  compile time, closing the last disclosed gap from T1d-2 (#767, PR #769)
  and T1d-2b (#774, PR #779): those two slices covered a _local-vs-local_
  handle-kind mismatch found by body-usage inference, but not a _binding's
  own declared param_ vs. a call-site argument.

  Mechanism: `infer::collect_external_sigs` resolves each manifest-registered
  `EXTERNAL`'s declared parameter/return types to `Ty` (handle kinds via the
  same `declared_handle_kinds` vocabulary `handle<K>` annotations already
  resolve against) and seeds them into `known_sigs` before body inference
  runs — a call to the binding now types its arguments through the exact
  same `known_sigs`/`observe`/`unify` path an ordinary knot/stitch call
  already uses. A cross-kind argument folds to the pre-existing `Ty::Conflicted`
  lattice point and reports through the existing `E066` (Conflicted-escape)
  diagnostic — no new diagnostic code.

  Observable through `@brink-lang/web`: under `types = strict` (`IdeSession
.set_type_policy("strict")`) with a registered `HostManifest` (`setHostManifest`)
  declaring two or more handle kinds and at least one `EXTERNAL` binding whose
  manifest entry declares a handle-kinded param, a call site passing an
  argument of a _different_ declared handle kind now reports `E066` where it
  previously reported nothing. `types = gradual` is unaffected — the existing
  runtime fault at the binding boundary stays the only enforcement there,
  byte-identical. An `EXTERNAL` with no matching registered manifest entry
  (inline-doc-only) stays unchecked, same as before this issue.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
  diagnostic surface only, no compiler/codegen change reachable by vanilla
  ink (no handles by construction), so this is oracle-inert by construction.

- 71dd2fc: M-2d: true import-scoped resolution (docs/modules-spec.md §2), relaxing the
  #784/#793 `E096` stopgap.

  Resolution now consults each file's own `IMPORT` list and declared module: a
  bare reference with same-name candidates across different declared modules
  binds to the module _this file_ imported, rather than to the flat
  duplicate-winner. Because same-name public definitions across declared
  modules can now be disambiguated per-importer, they are **legal** — the
  `E096` "duplicate definition declared in two different modules" hard error is
  retired; two modules may each export `ambush` and two files may import
  different ones, each binding its own.

  - **Import-scoped `lookup_by_name`** (`brink-analyzer::resolve`) — a new
    per-file `ImportScope` (own declared module + imported modules) threads
    through every resolution lookup site. With zero or one candidate (all of
    strict-ink and every single-module project) the fast path is byte-identical
    to the previous flat resolver, so no existing corpus resolution moves.
  - **Coexistence in the index** (`brink-analyzer::manifest`) — a cross-declared
    -module same-name/same-kind pair is no longer dropped as a duplicate; both
    are indexed. Within-module and legacy/undeclared duplicates keep the
    ordinary `E022`/`E023`/`E026` warning; strict-ink is untouched.

  Byte-identical strict-ink and single-module resolution; oracle ratchet
  (5,577) unchanged.

- 213a7f5: Added the M-4 modules tooling tail (docs/modules-spec.md §9): editor
  affordances riding the existing code-action, folding, and formatting seams.

  - **Auto-import quick-fix** — a cursor on an out-of-scope module reference
    (`E025`, import-required) now offers an _"Import `name` from `module`"_
    quick-fix that inserts the `IMPORT` line in the right place: below any
    existing `IMPORT` block, else below the `INCLUDE` block, else at the top
    under the `#@module` header. The offer is session-aware (it reads the
    module-qualified db that produces the live `E025` squiggle) and resolves
    as a pure source rewrite through the same `resolve_code_action` seam. It
    surfaces in both the wasm editor's code-action menu and the LSP.
  - **Import-block folding** — a run of two or more leading `IMPORT`
    statements folds into a single `IMPORT … (N modules)` region, mirroring
    the `INCLUDE` block fold.
  - **`IMPORT` formatting** — `brink fmt` canonicalizes import spacing:
    `IMPORT {  a , b  AS c } FROM  m` becomes `IMPORT { a, b AS c } FROM m`,
    and `IMPORT   mod` becomes `IMPORT mod`. Malformed (mid-edit) imports are
    left verbatim.

  Compat: purely additive and brink-gated. Every trigger requires a
  `#@module`/`IMPORT` construct absent from the entire pre-modules corpus, so
  no existing story's diagnostics, folds, or formatting change.

- 730c947: Circular-`INCLUDE` error messages are now deterministic.

  `IncludeGraph::find_cycle` (`crates/internal/brink-db/src/include_graph.rs`)
  previously picked its DFS start node from a `HashMap`'s key iteration order,
  so which rotation of a multi-file `INCLUDE` cycle got reported in
  `DiscoverError::CircularInclude` depended on that map's per-process
  `RandomState` seed. `brink-web`'s wasm-exported `compile` / `compile_fragment`
  / `compile_project` (`crates/brink-web/src/compile.rs`, `session.rs`) reach
  this path through `brink_compiler::compile` -> `brink_driver::discover` ->
  `ProjectDb::find_cycle`, and surface the message verbatim into the JSON
  `error` field. A multi-file project with a circular `INCLUDE` chain compiled
  through `@brink-lang/web` now gets a stable, reproducible cycle-rotation
  string across runs instead of one that could vary process to process.

- a0d9ee2: Close the `spawn_flow` by-id visibility gap left by M-2b (#772/#781/#783/#796):
  `Story::spawn_flow`'s `DefinitionId` entry point and `Story::spawn_flow_shared`'s
  resolved `container_idx` entry point now refuse a `#@private` target with the
  same `PrivateAccess` error the named-lookup paths already enforce.

  - **`StorySessionHandle.spawnFlow`/`StoryRunnerHandle.spawnFlow`** (the wasm
    bindings over `brink_runtime::Story::spawn_flow_shared`, which resolve the
    target path to a `container_idx` themselves via `find_address` before
    calling in) now correctly refuse a `#@private` knot with `PrivateAccess`
    instead of silently starting a flow at it — previously a host holding (or
    resolving) a private target's address could bypass the name-based refusal
    entirely.
  - Same documented dev-tooling override: `Story::set_visibility_enforcement`
    still governs both entry points, matching every other refusal surface.

- 7ac0a5d: Issue #805 (PR #794 / issue #786 lineage): widens `EXTERNAL` call-site
  checking under `types = strict` to three cases the wave-10 reconciliation
  flagged as missing:

  1. **Scalar semantic types from the manifest vocabulary** — a binding
     declared to take a manifest-registered scalar semantic type (e.g.
     `switch_id`, `base: int`) now rejects a mismatched literal type
     (`string`) at compile time, not just `handle<K>` kinds.
  2. **Inline-doc-only externals** — an `EXTERNAL` documented purely via an
     inline `///` `@param`/`@returns` doc comment, with no matching
     `ManifestExternal` entry in the registered manifest, now gets a checked
     signature too (previously disclosed as out of scope by PR #794).
  3. **Return-position kind checking** — a binding's own declared _return_
     type (handle or scalar) now flows into and is checked at its call site's
     usage, not just its declared param types.

  Mechanism: `infer::collect_external_sigs` now merges a binding's declared
  signature from both sources (inline doc wins by param name, else the
  registered manifest entry wins by position — the same merge order
  `external_check::analyze_externals` already uses for its own enrichment),
  and resolves every param/return `TypeRef` against the full registered
  `SemanticTypeDef` table (scalar bases in addition to `handle<K>` kinds).
  Mismatches fold to the pre-existing `Ty::Conflicted` lattice point and
  report through the existing `E066` diagnostic — no new diagnostic code.

  Observable through `@brink-lang/web`: under `types = strict`
  (`IdeSession.set_type_policy("strict")`) with a registered `HostManifest`
  (`setHostManifest`), a call site now reports `E066` for (1) a literal
  mismatching a binding's declared scalar semantic type, (2) a cross-kind
  argument to an inline-doc-only binding, or (3) a caller local receiving a
  binding's declared return kind and later used against a conflicting kind —
  none of which previously reported anything. `types = gradual` is
  unaffected — byte-identical.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
  diagnostic surface only, no compiler/codegen change reachable by vanilla
  ink, so this is oracle-inert by construction, same as #786.

- 1198586: Issue #815 (FG-4 train): `IncludeGraph::topological_order` no longer appends
  project files unreachable from the compile entry point as a "shouldn't
  happen in practice, but be safe" fallback. Only `entry` and files it
  transitively `INCLUDE`s now feed `lir_lowering_query`'s inputs.

  Mechanism: `topological_order` used to run a post-order DFS from `entry`,
  then append every remaining live file (sorted by `FileId`) that DFS didn't
  reach. `brink-driver::discover` (the one-shot CLI/oracle-corpus compile
  path) never produces unreached files — it only ever loads `entry` plus its
  transitive `INCLUDE` closure — so the fallback was always a no-op there,
  which is why the oracle corpus (5,577 episodes) is byte-identical before and
  after this change. The fallback mattered only for `ProjectDb`'s other role
  as the long-lived editor-session model, where files are added independently
  of any single entry point and an unrelated file (or an orphaned one, e.g.
  after removing an `INCLUDE`) can coexist with `entry` in the same session
  with no `INCLUDE` edge between them at all.

  Observable through `@brink-lang/web`: in a multi-file editor session (an
  `IdeSession`/equivalent with more than one file loaded and an entry set via
  `setEntry`), a file with no `INCLUDE` relationship to the current entry no
  longer contributes its globals/lists/externals/knots to the compiled
  `StoryData`, and editing that unrelated file no longer invalidates the
  entry's compiled LIR. That file's own diagnostics are unaffected — they run
  as independent per-file passes (`analysis_diagnostics_query`/
  `diagnostics_query`) that were never routed through `topological_order`, so
  they still surface exactly as before.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — every corpus
  case has a single entry-reachable file set, so this is oracle-inert by
  construction.

- 058f410: T1e-1: `ref lvalue-path` path-projection grammar, HIR, and creation-site
  checks (docs/t1e-spec.md §2/§6, issue #831, tracking #828). No LIR/VM
  support lands in this slice — every path projection still hits a
  deliberate "not yet lowerable" fence (see `E099` below).

  - New expression-position grammar: `ref` followed by an lvalue-shaped
    operand — a plain path, a dotted field chain, `[…]` indexing, or a mix
    (`ref npc.hp`, `ref party[leader].hp`) — legal only as a direct argument
    of a call, `#fn(…)`, or `bind(…)`. Superset grammar (always parses);
    under `dialect = strict-ink` it's a hard `E051` at analysis, same as
    every other brink extension — the oracle/strict-ink corpus is untouched.
  - **`E080`** (reused, not a new code) now also covers the `ref
lvalue-path` form: a projection's root must be a durable global `VAR`
    (`#@local` flow-locals included) — a `temp`/param root or a `CONST` root
    is a compile error, same rule T1c's unmarked ref-argument form already
    enforces.
  - **`E097`** — a `ref` projection outside ref-argument position (a
    standalone value, or nested inside another expression) — a deliberate
    v1 narrowing, tracked as icebox #825.
  - **`E098`** — under `types = strict` only, a projection segment (dotted
    field or `[…]` index) that disagrees with the root's statically-known
    declared shape (`VAR name: Shape = …`).
  - **`E099`** — a path projection with at least one real segment (dotted
    field or `[…]` index — not a bare single-name `ref`) reaches lowering:
    no `MakeProjection`/`ProjRead` support exists yet (lands in T1e-2,
    tracking #828), so this is a clean, targeted stop rather than a silent
    drop or a miscompile. A bare single-name `ref x` (zero segments) is not
    a real projection and lowers exactly like today's unmarked
    ref-argument form — never hits this fence.

- 7500e27: T1e-2: real `MakeProjection`/`ProjRead`/`ProjWrite` lowering, root-cell RMW,
  persistence, and `.inkt` support for path projections (docs/t1e-spec.md
  §3/§4, issue #842, tracking #828). Replaces the T1e-1 `E099` lowering fence
  for every real path-projection ref-argument (`heal(ref npc.hp, 5)`,
  `#fn(heal, ref party[leader].hp)`) with genuine execution.

  - **`Value::Projection`** (wire tag `VAL_PROJECTION`, first emission of that
    reserved tag): `(root cell, ordered segments)`, each segment `Index(i32)`
    or `Key(Value)` — the range-segment kind (`2`) stays RESERVED, never
    emitted (icebox #829, sequence slices). Structural equality (same root +
    equal segments), `Arc`-wrapped for O(1) clone.
  - **`MakeProjection` opcode**: emitted at every real path-projection
    `ref`-argument creation site — index/field-name segment expressions
    evaluate once, in source order (snapshot-at-creation, spec §1(1)).
  - **Root-cell RMW** (`ProjRead`/`ProjWrite`, spec §3: take → walk →
    `make_mut` spine → write → store back): a projection-bound `ref`
    parameter's reads/writes dereference through the identical walk, reused
    by `GetTemp`/`SetTemp`/`TakeTemp`'s dispatch — purely additive, no
    behavior change for any pre-T1e program.
  - **`ProjectionInvalidated`** turn-terminating runtime fault (spec §1(2)):
    a shrunk array, a removed map key, or a struct field the current shape no
    longer declares, checked at read/write time against the root's _current_
    value — never a clamp, never silent.
  - **Persistence**: a projection serializes as an ordinary value; rehydration
    validates the root cell exactly like `VariablePointer` today, including
    the `#@was` alias-table miss path.
  - **`.inkt` atoms** land with a reader in this same PR (the `docs/t1e-spec.md`-
    adjacent #742 discipline): `(projection <cell> (segments (index N) |
(key V) …))`, plus per-codec round-trips (`.inkb`, `.inkt`, transcript).
  - Fixes a pre-existing gap in `#fn(target, ref …)` ref-argument validation
    that rejected every `ref`-marked argument (even the T1c-era bare-path
    form) as "not an lvalue" once T1e's `ref` grammar wrapper was in play —
    `#fn(heal, ref npc.hp)` now validates and lowers correctly.

  Not observable through `@brink-lang/web` directly (no wasm-facing API
  change), but the wire format (`VAL_PROJECTION`) and VM fault surface
  (`ProjectionInvalidated`) are new behavior any consumer executing compiled
  `.inkb` through the wasm runtime can now encounter, so this ships as a
  patch per the wasm-observable-behavior convention.

- bcb5cd3: T1e-3: path-projections tooling tail (docs/t1e-spec.md §8 item 3, issue
  #850). Closes the T1e milestone (#828).

  - **Fixed a display bug**: a `#fn`/closure-bound `ref` parameter captured
    via a path projection (`#fn(heal, ref npc.hp)`) rendered its display form
    as `fn heal(ref hp = ref npc.hp, amount)` — the projection's own `ref `
    prefix nested inside the outer `ref hp = ` the fn-value display already
    supplies. Now renders `fn heal(ref hp = npc.hp, amount)`, matching the
    spec's `ref npc.inventory[3]` path-display convention. Fixed at both the
    runtime (`string(f)`/interpolation, `brink_runtime::value_ops`) and the
    static IDE hover renderer (`brink-ide`'s `fn_value_hover`) that mirrors
    it, so `@brink-lang/web`'s hover surface picks up the same correction.
  - **Completion**: right after typing `ref ` in a call's argument position,
    completion now offers only durable `VAR`s (the only legal `ref
lvalue-path` root, E080) instead of the full argument-position set
    (which also includes `CONST`/param/temp — none of them legal ref roots).
    Path _continuations_ after a `.`/`[` aren't attempted (needs the root's
    resolved shape, out of scope for "where cheap").
  - **`brink-fmt`**: `ref lvalue-path` arguments inside a `~ { … }` block now
    format with the canonical zero-space convention around `.`/`[`/`]`
    (`ref npc.hp`, `ref inventory[idx]`), matching the display form's own
    spacing rather than preserving whatever spacing the author typed.
  - **bevy-brink pass-through audit**: added end-to-end tests locking in that
    a path-projection ref-argument can never reach an `EXTERNAL`/host binding
    as a raw `Value::Projection` — structurally impossible today (an
    `EXTERNAL` declaration has no `ref`-parameter grammar), and any value
    _derived_ from reading a projection-bound parameter inside ink always
    arrives at a binding pre-resolved to a plain snapshot.
  - New book chapter, "Path Projections"
    (`docs/book/src/toolchain/dialect/path-projections.md`), with
    compile-checked `ink`/`text` examples following the Function Values
    chapter's precedent.

- c62687c: Indexed assignment to an absent map key now inserts (JS/Python semantics)
  instead of faulting `MapKeyNotFound` — issue #856, ruled 2026-07-15.
  `memo[k] = v` on a fresh key works, matching the existing `insert()`/
  `push()` stdlib mutators' insert-on-absent behavior; a repeat assignment to
  the same key still overwrites in place rather than growing the map.

  - **`IndexSet`'s map branch** (`brink-runtime`'s `write_index_upsert`, used
    by the `IndexSet` opcode) is now insert-on-absent for a valid-domain key
    (int/string/bool) — array bounds and the map key-domain check are
    unaffected (still turn-terminating faults, no silent growth).
  - **Reads are unaffected** (value-model-spec §11c): `m[k]` (`IndexGet`) and
    `MapGet` still fault `MapKeyNotFound` on a missing key. Path-projection
    writes (`ref`-bound `ProjWrite`, `docs/t1e-spec.md` §4) also keep the
    strict fault-on-missing-key behavior — only the direct `IndexSet` opcode
    changed.
  - **Compiler lowering**: plain `a[idx] = v` (`lower_flat_indexed_assignment`)
    no longer runs a non-mutating pre-check read before taking the root — that
    precheck existed to catch the very fault this issue retires, and it can't
    distinguish "absent map key" from "array out of bounds" before deciding
    whether to fault. Compound assignment (`+=`/`-=`) is unaffected (the
    precheck's value is still needed as the operand). Net effect: a fault
    during plain `a[idx] = v` (array out-of-bounds, an invalid-domain map key,
    or a non-collection root) can now leave the root `Value::Null`, matching
    the documented, already-shipped trade-off `insert`/`remove`'s
    author-supplied keys make (`fault_during_insert_leaves_root_null`) —
    compound assignment still leaves the root untouched on a fault.

  Observable through `@brink-lang/web`: any consumer executing compiled
  `.inkb` through the wasm runtime can now see `memo[k] = v` on a fresh key
  succeed instead of raising `MapKeyNotFound`, so this ships as a patch per
  the wasm-observable-behavior convention. Oracle ratchet unaffected (brink-
  dialect collections only — vanilla ink has none): 5,577 episodes still pass.

- 8870113: Stdlib slice 1 completion: `char_at(s, i)` string-indexing primitive
  (docs/t1b-surface-spec.md §5, issue #857) — a corpus finding that blocked
  string-algorithm ports (levenshtein/tokenizers/edit-distance) with no way
  to read a character out of a string.

  - **Chars, not bytes**: `i` indexes Unicode scalar values (`str::chars`),
    not UTF-8 bytes — a byte-indexed read would panic or split a multi-byte
    sequence for any non-ASCII text. Returns the char at `i` as a
    single-character `String` (ink has no separate char type).
  - **Turn-terminating fault** (value-model-spec §11c: no silent garbage) on
    `i` outside `[0, char_count)`, a non-`Int` `i`, or a non-`String` `s` —
    never a clamp, never a silently-empty result. New `RuntimeError`
    variants `CharAtOutOfBounds`/`CharAtIndexNotInt`.
  - **VM-native** (`CharAt` opcode, `0xDD`), lowercase name, author-
    shadowable with a warning (`E035`) per the existing stdlib-slice-1
    ruling (`is_t1b_stdlib_name`).
  - **Typing rule** declared at introduction: fixed `Ty::String` return
    (a char-as-1-string result), independent of argument types — the domain
    check is a runtime/gradual-mode concern at the `CharAt` op, matching the
    `int`/`float`/`string` conversion intrinsics' posture.
  - `.inkt` text support lands with a reader in this same PR (writer +
    reader + round-trip test, matching the `#742`-adjacent discipline).

  Observable through `@brink-lang/web`: new VM opcode/fault surface any
  consumer executing compiled `.inkb` through the wasm runtime can now
  encounter, so this ships as a patch per the wasm-observable-behavior
  convention.

- e16e8f8: Issue #858: `brink-fmt` now retokenizes single-line `~ expr` logic lines
  through the CST instead of passing the statement's own text through
  unchanged (only the outer `~ ` prefix was previously normalized). A
  single-line logic line now gets the same canonical single-space-around-
  tokens rendering, and the `ref lvalue-path` zero-space convention around
  `.`/`[`/`]`, that a `~ { … }` multi-line block statement already received —
  e.g. `~ temp x   =   0` now formats to `~ temp x = 0`, and
  `~ heal(ref  party[ leader ] . hp,   5)` now formats to
  `~ heal(ref party[leader].hp, 5)`. Reachable through @brink-lang/web via the
  editor's "Format knot" code action (`code_actions`/`resolve_code_action`),
  which runs the whole document through `brink_fmt::format`.
- 820f6c5: T2-2: `#@effects(…)` author-facing assertion surface + the exceedance
  compile error (docs/effects-spec.md §10, sitting 2 — 2026-07-14; issue
  #861, tracked from #859). Builds on T2-1's advisory `effects(def)`
  substrate (issue #860).

  - **Grammar** (the `#@` directive channel, brink-dialect-gated → `E051`
    under strict-ink): `#@effects(reads: gold, writes: alarm, calls: audio)`
    declares an upper-bound effect row on a knot/stitch; `#@effects(pure)` is
    sugar for the empty row. Placement mirrors `#@local` — top of a
    knot/stitch body.
  - **The only diagnostic is exceedance** (`E103`): the definition's inferred
    effect row is not covered by (⊄) its declared bound. Per the sitting-2
    ruling there is no drift policy — an inferred row _narrower_ than its
    bound stays silent; nothing else warns.
  - A clause naming an identifier that isn't a declared global `VAR`/`CONST`
    (`reads`/`writes`) or a declared `EXTERNAL` (`calls`) anywhere in the
    project is `E102`; malformed directive grammar (missing argument, unknown
    clause keyword, non-identifier value) is `E100`/`E101`.
  - Wired lazily: an unannotated project never triggers effect-row inference
    — only defs that actually carry `#@effects(…)` cause `effects(def)` to be
    computed.

  Oracle byte-identical (5,577 episodes unmoved) and the strict-ink corpus
  untouched — this is a brink-dialect-only analysis surface with no format,
  codegen, or runtime change. Ships as a `@brink-lang/web` patch because the
  new diagnostic codes (`E100`–`E103`) are editor-observable (LSP/IDE
  diagnostics) through the wasm analysis pipeline.

- 45eb96b: T2-3: first real emission into the reserved `EffectRows` `.inkb` section
  (docs/effects-spec.md §11, format-v4-rfc §2). The wire surface grows —
  compiled `.inkb` artifacts now carry a factored effect-row table — even
  though the runtime does not consume rows yet (additive metadata; the
  linker never reads them, so episodes stay byte-identical).

  - **Section graduated** — `EffectRows` (tag `0x0D`) moves from reserved
    (count-0) to a real, section-locally-versioned section (version byte
    bumped, no format `VERSION` bump — the reservation existed for exactly
    this). Writer and reader land together, with `.inkt` text atoms and
    per-codec round-trips (inkb + inkt).
  - **Factored rows** — each entry ships a direct part (reads / writes /
    call atoms / opaque) plus a per-dispatch list (`{cell, narrowable-bit,
static fallback}`, empty in v1 — a flat row would foreclose §7
    narrowing). Every knot/stitch ships its container row (the host's
    resume-scheduling estimate, §12.1), keyed in a `DefinitionId → row`
    table.
  - **Reserved parameter slots** — each call atom carries a
    capability-parameter slot populated `(any)` in v1 (component-granular;
    path-granular #826 is the later consumer) and a reserved
    handle-parameter slot (t1d-spec §7), left `None` in v1.

- e8cb050: T2-4 effects tail (docs/effects-spec.md §10, issue #863): IDE hover now shows
  a knot/stitch's inferred **effect row** on a stable line — `reads: …; writes:
…; calls: …`, or `pure`, or `opaque` for a definition that dispatches through
  a function value. Purely advisory display; the only contract remains the
  optional `#@effects` assertion (`E103` exceedance, unchanged).

  Editor-observable through the shared `brink_ide::hover` path (LSP/wasm hover),
  hence a `@brink-lang/web` patch. No behavior change to compiled output — effect
  rows are additive metadata the runtime never reads.

- Fixed a silent-no-op compiler bug (#869): a direct call through a computed
  fn-value callee — `handlers[state]()`, `obj.field()`, `get_handler()()` —
  used to compile clean and silently drop the call entirely (the parser left
  the trailing `(args…)` unconsumed, so it resurfaced as prose text on the
  content line instead of being parsed as part of the call). Direct-call
  syntax is scoped to a bare variable/temp/param callee (t1c-spec §3); any
  other callee shape now parses as a real (if always-rejected) `CALL_EXPR`
  node and produces a loud, unconditional compile error (`E104`) naming the
  ratified `call(f, args…)` form as the fix, in every dialect and mode.

  Compat: previously-compiling sources using one of these computed-callee
  shapes as a direct-call target now fail to compile with `E104` instead of
  silently dropping the call — the only prior alternative was a wrong,
  silently-corrupted output, so this is a strict improvement, not a
  regression. `call(f, args…)` (the explicit form) is untouched and already
  dispatches through exactly these callee shapes correctly.

- fe0c16d: Fix: T2-2's `#@effects(…)` `reads`/`writes`/`calls` clause resolution
  (`resolve_cell`/`external_declared`) bypassed M-2d's import-scoped
  resolution (issue #790), independently picking a flat, smallest-id
  same-named candidate instead of routing through the shared
  `ImportScope`/`lookup_by_name` machinery every other reference uses
  (issue #881, tracked from #859; the #811 lesson: twin semantic checks
  share one helper, never re-derive).

  Under multi-module projects where two declared modules each publicly
  export a same-name `VAR`/`CONST`/`EXTERNAL`, this could attribute a
  `#@effects` assertion's clause to the _wrong_ module's cell relative to
  the one the asserting definition's body actually reads/writes/calls
  (via the real import-scoped resolver) — producing a spurious `E103`
  exceedance diagnostic, or, by luck of id ordering, silently masking a
  real one.

  `resolve_cell` and `external_declared` now resolve through
  `brink-analyzer::resolve::lookup_by_name` with the asserting file's own
  `ImportScope`, exactly like every other reference resolves — same-name
  cross-module cells are now attributed per-importer, consistently with
  what the body's own resolution binds.

  Oracle byte-identical (5,577 episodes unmoved); single-module and
  strict-ink projects are unaffected (the fast path is byte-identical
  whenever there is at most one same-named candidate).

- 6266cbf: T2-3 follow-up (#882): wire the ruled freeze semantics into `EffectRows`
  emission. The section-local encoding version bumps 1 → 2 (still no format
  `VERSION` bump) — every row gains a leading `is_entry` byte, so a compiled
  `.inkb`/`.inkt` artifact's `EffectRows` bytes change even though runtime
  behavior does not (the section remains additive metadata the linker never
  reads; episodes stay byte-identical — oracle ratchet unchanged at 5,577).

  - **Entry set respects visibility** — a `#@private` definition's row now
    ships with `is_entry: false`: it is not a legitimate host-lookup entry
    point (`docs/effects-spec.md` §10; host semantic lookup on it is refused
    per `docs/modules-spec.md` §4 rule 2). Every other definition defaults
    `is_entry: true`, unchanged from T2-3.
  - **The row itself is never dropped.** `#@private` hides the _name_, not the
    _cell_ (`docs/modules-spec.md` §4 rule 1) — a private knot/stitch/function
    can still be captured as a first-class fn-value token a _public_ path
    holds, and the dispatch-narrowing machinery (§7) resolves such tokens by
    `DefinitionId`, not by name. So the `DefinitionId → row` table always
    carries every def's row regardless of `is_entry`; only host-facing lookup
    is gated by it. This is unconditional (not a reachability computation over
    whether a public path actually captured such a token today).
  - **Writer and reader land together for both codecs** (`.inkb` + `.inkt`),
    each with its own round-trip test for both `is_entry: true` and
    `is_entry: false`, plus an end-to-end `ProjectDb`-level test proving a
    `#@private` def's row is excluded from the entry set but still resolvable
    in the table, alongside an unaffected public row.

- 9e9f07a: Fixed a `.inkt` dump-parity bug (#883, the #742/#871 class): the
  `struct_shapes` section (TM-4 struct/record shape declarations) was fully
  round-tripped through the binary `.inkb` format but silently dropped
  entirely by the `.inkt` textual dump — neither written nor read, despite
  the module doc's claim that every `StoryData` field is represented. A
  compiled story containing `STRUCT` declarations now shows its
  `struct_shapes` section in the `.inkt` debug view (`program_inkt()`,
  surfaced in brink-studio's compiled-output panel) instead of it vanishing.

  Also added a structural exhaustiveness guard to `brink-format`'s
  `proptest_inkt` suite: a match over every `Opcode`/`Value` variant with no
  wildcard arm, so a future variant added to either enum without matching
  generator coverage fails to compile instead of silently escaping fuzz
  coverage — the mechanical fix for this recurring bug class (tracked from
  #397).

- 878be79: Fixed a duplicate `E046` diagnostic on directives with dynamic content
  (`#@effects({expr})`, `#@was({expr})`, `#@private`/`#@public` with dynamic
  content). `apply_scope_directives` had its own generic `d.dynamic` check
  that fired for every directive, including ones with a dedicated handler
  (`effects_assertion_from_directives`, `was_from_directives`,
  `visibility_from_directives`) that independently re-checks `d.dynamic` and
  emits its own `E046`. The generic check is removed in favor of the
  dedicated handlers' own checks — unknown dynamic directives (no dedicated
  handler) still get exactly one `E046` via the fallback arm.

  Compat: strictly fewer diagnostics for an already-invalid construct
  (dynamic content is never valid in a directive); no change for any
  directive that isn't dynamic.

- c66409b: Fixed `Map`/`Map` and `Record`/`Record` equality (`==`/`!=`) faulting with a
  `TypeError` at runtime instead of comparing. `value_ops::binary_op` had no
  match arm at all for these two variant pairs, even though `Value`'s own
  `PartialEq` already implements the ratified structural-equality-with-an-
  `Arc::ptr_eq`-fast-path rule (value-model-spec §4) — the same comparison
  `contains()`'s Array branch already exercises for element containment.
  Both arms now delegate to `Value`'s `PartialEq`; ordering operators
  (`<`, `>`, `<=`, `>=`) on maps/records still fault, as before — no ordering
  is defined for either.

  Note: map equality currently follows `OrderedMap`'s existing (insertion-
  order-sensitive) derived `PartialEq` unchanged. Whether two maps with the
  same entries in a different insertion order should compare equal is a
  separate, still-open question tracked in #909 (parked for a maintainer
  ruling) — this fix does not decide it either way, and map-equality
  semantics may change once that ruling lands.

- 86c4bee: Map/record `==`: map equality is now content-based, not
  insertion-order-sensitive (issue #909, ruled 2026-07-18 —
  `docs/decision-log.md` "Map/record equality is insertion-order-insensitive").

  `#{a:1, b:2} == #{b:2, a:1}` now evaluates `true`. Previously,
  `OrderedMap`'s derived `PartialEq` compared its backing `Vec<(MapKey,
Value)>` positionally, so two maps holding identical key/value pairs
  inserted in different orders compared unequal — a silent correctness bug,
  since ink authors have no way to observe or control the internal `Vec`
  layout an equality check was leaking.

  `OrderedMap` now hand-implements `PartialEq` as a content comparison: same
  entry count (fast-path reject on size mismatch), then every key in one map
  looked up and value-compared in the other — order-independent by
  construction. Every equality-derived operation (`==`, `!=`, and any future
  membership/contains-style check built on `Value::eq`) picks this up
  automatically through `Value`'s existing `PartialEq` delegation to
  `Value::Map`'s `Arc::ptr_eq` fast path and structural fallback — no call
  site changes needed.

  **Unchanged**: iteration order (`iter`/`keys`/`values`) and
  serialization/wire order both stay insertion-order — only equality ignores
  it. Record equality (shape-ordered fields, not insertion-ordered) is
  unaffected by this ruling.

  Observable through `@brink-lang/web`: any ink script comparing two map or
  record values containing maps via `==`/`!=` now gets content-based results
  regardless of the order the maps' keys were built in.

- fdf94f6: FS-1 (#915, tracking #889): the FlowFrame suspended-flow section in
  `SaveState` — format only (`docs/flow-suspension-spec.md` §2/§9). No
  compiler `await` support and no runtime spill/restore land in this slice;
  `Story::save_state`/`load_state` always produce/consume `None`.

  - `SaveState` grows an optional `suspended: Option<SuspendedFlow>` field
    behind `#[serde(default)]`/`skip_serializing_if` — an older save missing
    the key still deserializes, and an unsuspended save's wire form is
    byte-identical to before (no `"suspended": null` noise).
  - `SuspendedFlow` (section-locally versioned via
    `SUSPENDED_FLOW_SECTION_VERSION`, independent of `SAVE_FORMAT_VERSION`):
    the parked flow's current container `DefinitionId`, its tunnel-return
    stack (`Vec<DefinitionId>`), a name-keyed frame record (an ordinary
    `Value`, so no new wire representation), and a `WakePolicy` (await-site
    id + optional condition fn token + a `WakeSource` host-source
    discriminant). All identity rides name-stable `DefinitionId`s, never
    instruction offsets — the same recompile-stability contract as the rest
    of `SaveState`.
  - Round-trip tests per `docs/flow-suspension-spec.md` §7: both
    `WakeSource` variants, the absent/backward-compat case, and a
    frame-shape-drift case proving the name-keyed encoding survives a
    missing/extra/renamed crossing-local between save and load (the
    tolerant _decode_ itself is FS-3 scope).

  Inert wire growth: this is purely additive surface with no producer yet,
  so no existing save or story's observable behavior changes.

- 9d559a3: Fixed `Array`/`Array` equality (`==`/`!=`) faulting with a `TypeError` at
  runtime instead of comparing. `value_ops::binary_op` had no match arm at all
  for this variant pair, even though `Value`'s own `PartialEq` already
  implements the ratified structural-equality-with-an-`Arc::ptr_eq`-fast-path
  rule (value-model-spec §4). The arm now delegates to `Value`'s `PartialEq`;
  ordering operators (`<`, `>`, `<=`, `>=`) on arrays still fault, as before —
  no ordering is defined.

  Unlike the parked map-ordering question in #909, array equality is
  unambiguously order-sensitive by construction — element order is observable
  array structure, not an incidental insertion artifact — so there is no
  analogous ruling to park here: `[1, 2] == [2, 1]` is `false`.

- cc1d11e: FS-2 (#928, tracking #889): the FlowFrame compiler slice — `await`
  grammar/HIR/lowering, the effect-free condition purity gate, and the LIR
  lowering fence (`docs/flow-suspension-spec.md` §3/§5). Compiler + analyzer
  only; the runtime spill/restore is FS-3.

  New syntax reaches the wasm parser surface, so the whole grammar is
  observable through `@brink-lang/web`:

  - `await <cond>` parses at statement/logic position — the top-level
    `~ await …` logic line and inside a `~ { … }` block — plus the
    persistent-await `while await <cond> { … }` loop. `await` is a contextual
    (soft) keyword: it stays an ordinary assignable identifier everywhere
    else (`await = 5`, `while await { … }`), so no existing ink is affected.
  - Under the default strict-ink dialect, `await` is a brink extension and is
    rejected with `E051`, like every other superset construct.
  - Under `dialect = brink`, an `await` condition must be **effect-free**
    (read-only): reads are the wake dependency set, but a transitive write to
    a global cell or an effectful call is a compile error — a new diagnostic,
    `E105`, built on the effects machinery. A bare fn-value reference used as
    a dynamic condition (`await ready`) is read-only by construction and is
    never flagged.
  - Every `await` construct is then fenced at LIR lowering with `E052` (the
    reserved "parses/analyzes before its lowering lands" code): its runtime
    spill/restore semantics are FS-3, so a program using `await` refuses to
    lower to bytecode rather than silently dropping the suspension point.

  Vanilla ink has no `await`, so no existing story's compiled output or
  runtime behavior changes.

- 62cb759: FS-2 follow-up (#928, tracking #889): harden the `await`-condition purity
  gate (E105) flagged in PR #935's review.

  - The purity walk (`brink-analyzer::await_purity`) now recurses into
    `Expr::StructLiteral` field initializers in both the effectful-condition
    check and the salsa callee-collection path. An effectful call nested in a
    struct-construction condition (`await Flag#{on: raise_alarm()}`) previously
    slipped past E105 because `StructLiteral` was treated as a non-recursing
    leaf; it is now correctly rejected. (`FnLiteral` stays a leaf — a lambda
    body is not invoked during condition re-evaluation.)
  - Added end-to-end coverage: a two-hop transitive write
    (`condition → outer() → inner() → writes a global`) trips E105, and an
    effectful call inside a struct-construction condition trips E105.

  Wasm-observable: a program with such a condition, which previously produced
  no E105, now surfaces the purity error through the diagnostics surface.

- a350dcf: Runtime `==`/`!=` completeness sweep (issue #939, tracked from #397):
  `VariablePointer`, `TempPointer`, and `Projection` values no longer
  fault with a type error when compared with `==`/`!=` — they now compare
  correctly (token equality for the pointers, structural same-root-cell +
  equal-segments equality for projections), delegating to `Value`'s own
  `PartialEq` exactly like the prior fixes for `FnRef`/`Closure`/`Handle`/
  `Array`/`Map`/`Record` (#918, #931).

  Also fixes a float-equality inconsistency: direct float `==`/`!=` used
  to tolerate an `f32::EPSILON` fudge factor while a float nested inside
  an array/map/record/projection always compared by exact IEEE equality.
  Both routes now use exact equality (matching the C# reference ink
  runtime's plain `x == y` and the already-shipped collection-equality
  behavior) — a small behavior change: two floats that previously
  compared equal only because they happened to land within
  `f32::EPSILON` of each other (e.g. accumulated rounding error from
  independent arithmetic paths) now compare unequal, same as arrays/maps
  already did with the same inputs.

- 3ad1bc5: brink-format: `read_inkb`'s container decoder now rejects a `.inkb` whose
  declared `param_count` disagrees with the number of per-param name/mode
  metadata entries that actually follow it (#954, sibling of the `.inkt`
  reader's same guard, #745).

  `ContainerDef::params`'s documented invariant is that `params.len()` always
  equals `param_count` whenever per-param metadata is present at all. Before
  this fix, `decode_container` built a `ContainerDef` from the two
  independently-read counts with no consistency check, so a mutated/corrupt
  `.inkb` could construct exactly the inconsistent state the `.inkt` reader
  now rejects. Fixed by validating the invariant at decode time and returning
  a new `DecodeError::ParamCountMismatch` on mismatch — a defined decode
  error, never a panic (the format fuzz lanes wired up in #948 exercise this
  exact path).

  Observable through `@brink-lang/web`: `read_inkb` is called unconditionally
  (not feature-gated) from `brink-web`'s session/story-runner/compile paths,
  so a corrupted `.inkb` payload with this specific inconsistency now surfaces
  as a clean decode error instead of constructing invariant-violating data.

- 2b7dd5a: Runtime: `brink-runtime`'s uppercase `INT()`/`FLOAT()` builtins no longer
  silently fold an unconvertible value to `0`/`0.0` (issue #955, the
  cast-operator leg of the wildcard-fan-out class #950 explicitly scoped
  out).

  `value_ops::cast_to_int`/`cast_to_float` (backing `Opcode::CastToInt`/
  `CastToFloat`) used to end in a `_ => Value::Int(0)` / `_ =>
Value::Float(0.0)` wildcard arm — so a future `Value` variant would
  silently cast to zero instead of getting a considered answer, the same
  hazard class #950 fixed for the marshal/serialize legs. The reachable
  domain (`Int`/`Float`/`Bool`/`String`, including the legacy
  silent-0-on-string-parse-failure fallback) is **unchanged** — verified
  byte-identical against the oracle (5,577 episodes, unmoved). Every other
  `Value` variant (`List`, `DivertTarget`, `VariablePointer`, `TempPointer`,
  `Null`, `FragmentRef`, `Array`, `Map`, `Record`, `FnRef`, `Closure`,
  `Handle`, `Projection`) now raises `RuntimeError::InvalidConversionDomain`
  instead — none of `value-model-spec.md`, `t1c-spec.md`, `t1d-spec.md`, or
  `t1e-spec.md` rules a conversion for these, so faulting is the conservative
  default (the same value-model-spec §11c "no silent garbage" precedent the
  T1b lowercase `int()`/`float()` intrinsics already follow), reusing the
  same fault variant with an uppercase `target` label (`"INT"`/`"FLOAT"`) to
  distinguish it from the lowercase intrinsics' own faults.

  Observable through `@brink-lang/web`: any JS host driving a story through
  `continue_single`/`continue_flow`/`advance` where the ink script calls
  `INT()`/`FLOAT()` on one of the previously-wildcarded variants now sees the
  call reject with a runtime-error `JsError` instead of silently continuing
  with a zero. None of these variants are reachable from vanilla ink source
  today (they're brink-only value kinds), so this cannot fire from a
  plain-ink story — only from brink-specific constructs (records, function
  values, handles, path projections) an author explicitly casts.

## 0.11.0

### Minor Changes

- c9475df: Added `EditorSessionHandle.setLanguageDialect(value)` and
  `EditorSessionHandle.setTypePolicy(value)` (#693), mirroring
  `setSemanticTypeCheck`/`setExternalCheck`. The raw
  `WasmEditorSession.set_language_dialect` (#611) and
  `WasmEditorSession.set_type_policy` (#660) wasm levers existed, but
  `EditorSessionHandle` — the surface `@brink-lang/web` consumers actually
  use — exposed neither, so no JS caller could opt into the brink dialect or
  the typed-mode policy at all (every new construct raised `E051` with no
  opt-in path). `setLanguageDialect("brink" | "strict-ink")` and
  `setTypePolicy("strict" | "gradual")` now delegate to the wasm session and
  bump the generation counter, same as every other mutating call on the
  handle.

### Patch Changes

- 8a3635d: Fixed formatter line classification for constructs containing inline
  `/* … */` block comments (observable via `format_document`). The line
  classifier used to mark any physical line containing a block-comment
  token anywhere in its subtree as a pure comment line, which skipped the
  line's real construct entirely:

  - a single-line `STRUCT Point = #{x: float, /* mid */ y: float}` was
    passed through verbatim instead of being normalized by the struct
    renderer;
  - a block comment on a multiline struct's `#{ /* c */` opening line (or
    a `~ { /* c */` logic-block opening line) caused the entire body to
    lose its indentation;
  - a one-liner `~ x = 5 /* foo */` logic line skipped `~`-spacing
    normalization.

  A single-line block comment nested inside a construct whose renderer
  handles comments itself (struct bodies, `~ { … }` block bodies, plain
  `~` logic lines) is now left to that construct's formatting.
  Free-floating comments — banners, multi-line comments, and comments
  outside those regions (e.g. `STRUCT Point /* c */ = #{…}` or trailing
  after a block's closing `}`) — keep the verbatim treatment.

- 34951ec: Fixed the formatter silently dropping a comment attached to a
  `~ { … }` logic block outside its body (observable via
  `format_document`). A comment that is a direct child of the logic line —
  trailing after the closing brace (`} /* note */`, `} // note`) or
  leading between `~` and `{` (`~ /* c */ {`) — was deleted, because the
  block body was rebuilt from the inner statement block alone. The block
  renderer now emits leading comments on the header line and trailing
  comments on the closing line. A leading comment on the opening line no
  longer de-indents the body to column 0, and a single-line block that
  carries a trailing comment now expands to the canonical multiline form
  (matching the comment-free case) instead of being frozen verbatim.
- 81ddfa7: Fixed a fuzzer-discovered parser bug (PR #672 workstream C's new
  `parse_lossless` fuzz target, which builds with debug-assertions on):
  a `bump_assert` invariant inside the parser could fire on legitimately
  reachable token sequences — e.g. an un-flushed `WHITESPACE` token
  still sitting at the parse position when `conditional_with_expr_standalone`
  dispatches into `expression()` on a `#fn(...)`/sigil-literal expression
  inside a `MULTILINE_BLOCK` — crashing the parser with a
  `debug_assert_eq!` panic in debug builds. In release builds (including
  the shipped `@brink-lang/web` wasm), the same mismatch compiled away
  silently: the parser consumed the unexpected token with no diagnostic
  at all, corrupting the tree instead of erroring.

  `bump_assert` now always emits a proper parse error on a mismatch, in
  every build profile. Observable through `@brink-lang/web`: compiling
  ink source that hits this token-position edge case no longer panics in
  debug tooling, and — this is the real production-facing change — no
  longer silently mis-parses in the shipped wasm build; it now returns a
  normal `ok: false` result with a recovery-error diagnostic, like any
  other malformed input.

- 9c58d6e: Fixed a fuzzer-discovered linker panic (PR #672 workstream C's new
  `vm_no_panic` fuzz target for malformed `.inkb`, previously masked by
  a CI job structure that never actually ran it — see the accompanying
  CI fix — now caught on its first real run). `link()` indexed
  `StoryData::name_table` with a container/address-path `NameId` taken
  straight from the input bytecode with no bounds check; an out-of-range
  `NameId` panicked with `index out of bounds`.

  `link()` now returns `RuntimeError::InvalidNameId` on an out-of-range
  `NameId` instead of panicking. Observable through `@brink-lang/web`:
  `new StoryRunner(story_bytes)` (and every other entry point that links
  caller-supplied `.inkb` bytes) no longer panics/traps the wasm module
  on malformed/corrupted input — it returns a normal error result, like
  any other malformed input.

- f68c094: `brink-fmt`'s `STRUCT` declaration formatting (TM-4b) no longer silently
  drops comments living inside the struct body. Observable through
  `@brink-lang/web` via the `FormatKnot` code action
  (`brink_ide::code_actions::format_region` → `brink_fmt::format`):

  - Multiline `STRUCT` bodies now preserve leading, interleaved, and
    same-line trailing comments between/around fields instead of dropping
    them.
  - Single-line `STRUCT` bodies preserve interleaved block comments instead
    of dropping them.
  - Removed an unreachable dead branch in the multiline struct renderer.

- b9ad39f: Fixed (#674): the brink-dialect assignment-target grammar now recognizes
  an `Index` base for a `.field` write — `arr[i].field = v` parses as a real
  assignment target instead of failing with a generic "expression is missing
  an operand" (E015) parse error. The compiler still rejects this shape (a
  chained/mixed field write, T1e) but now reports the intended `E074`
  diagnostic — "chained field-write projection (p.a.b = v) is not
  supported" — pointing at the target expression, matching the diagnostic
  `o.inner.v = 2.0` already got. Observable through editor/compile
  diagnostics for `.ink` source under `dialect = brink`; no change to
  `p.field = v` (single-level, still lowers via RMW take/make_mut/write-back)
  or to plain `arr[i] = v` indexed assignment.
- b7b7eb0: Struct construction literals (`Name#{field: expr, …}`, TM-4c): fixes #675
  and #676 per the ruling in decision-log "Struct construction literals:
  source-order evaluation, duplicate field is a compile error" (2026-07-14).

  - (#676) Initializers now evaluate in **source** order (left-to-right as
    written), not the shape's declaration order — codegen reorders only the
    already-evaluated _values_ into shape offsets afterward. Previously, when
    the author's field order differed from the shape's declaration order and
    two or more initializers had observable side effects, those effects fired
    in shape order instead of source order.
  - (#675) A duplicate field in a construction literal is now a real compile
    error (`E084`), naming the repeated field, under both `types = gradual`
    and `types = strict`. Previously a duplicate silently kept the last
    initializer's value while the earlier, shadowed initializer's expression
    — including any side effect — was dropped without lowering it at all.

  Observable through `@brink-lang/web`: `compile_project`/`compile_fragment`
  now return a diagnostic (`E084`) for a construction literal with a
  duplicate field, and the compiled bytecode for a well-formed literal whose
  source field order differs from its shape's declared order now evaluates
  initializers in the order the author wrote them.

- d29671d: RCA'd #680 ("`ref`-argument call co-occurring with a `temp` decl in the
  same `~ { }` block resolves to the wrong global slot"): the `ref`-argument
  call was a red herring — the actual defect is reading a T1b block-scoped
  `temp` (`~ { … }`) from _outside_ its own block. LIR lowering's fallback
  for "temp not currently visible" (kept for inklecate-compat forward-
  reference emulation of classic, non-block temps) previously caught this
  case too, silently compiling to a phantom global id that was never
  registered — a runtime-only `UnresolvedGlobal` fault with no compile
  diagnostic.

  Observable through editor diagnostics: referencing a block-scoped `temp`
  (by value or by `ref` argument) after its `~ { … }`/`while`/`for`/`if`
  block has already closed is now a real, non-suppressible compile error
  (`E082`) instead of a silent runtime fault. A `ref`-argument call
  co-occurring with a `temp` decl in the _same_ block — the issue's literal
  repro shape — was already correct and is unaffected.

- ca45425: Fixed #692: a scalar `VAR`/`CONST` declaration default whose _whole_
  value is a non-constant reference or call (`VAR x = someOtherVar`,
  `VAR x = f()` — including either wrapped in a prefix/infix operation,
  e.g. `VAR x = -f()`) previously folded silently to `Null` through
  `eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm and its
  catch-all, with zero diagnostic. This is the same silent-fold bug
  #673/#679 fixed one level down inside array/map/struct declaration-
  default literals (`E075`/`E076`/`E077`), left unfixed at this bare
  top-level scalar position.

  Observable through `@brink-lang/web`: compiling such a declaration
  default now surfaces a real, non-suppressible compile error (`E083`)
  instead of silently producing a `Null` global. A `VAR`/`CONST`
  referencing another `CONST`, or a `Path` reference nested _inside_ a
  collection/struct/fn literal, is unaffected (the latter remains the
  pre-existing, separately-tracked gap #679's scope notes named).

- abc369a: T1c-2 (#700): function values (`#fn(…)`) now lower, execute, and persist —
  the first live use of the V4-reserved `PushFnRef`/`MakeClosure`/`CallValue`
  opcodes and `VAL_FN_REF`/`VAL_CLOSURE` value tags. Observable through
  `@brink-lang/web`:

  - **Program model + disassembly**: a `#fn(…)` baked into a declaration
    default renders as a function-value (`fn <path>(…)`) rather than
    erroring or showing `null`, and the new opcodes disassemble
    (`push_fn_ref` / `make_closure` / `call_value`).
  - **Speculation / eval-function results**: a function value crosses the
    typed-value JSON boundary as an opaque token (`{ "type": "fn", target,
bound }`) — the host never dereferences the env (spec §6); the
    callback-invocation surface lands in T1c-3.
  - **Runtime dispatch**: calling a function value (direct `f(args…)` or
    explicit `call(f, args…)`) works; a non-function callee, a wrong-arity
    explicit call, a rehydration mismatch (a saved closure whose target
    param was renamed/re-moded after a recompile), or invoking a closure
    that `ref`-binds a flow-private `#@local` cell are turn-terminating
    faults — never silent garbage.
  - **Persistence**: function values save/load as ordinary values (save
    state, journal, speculation snapshots); `ref`-bound cells round-trip
    losslessly through the transcript codec.

  The `#inkb` wire format gains per-container parameter name/mode metadata
  (an additive trailing field) so a rehydrated closure can be validated
  against the current signature.

- 30e09f9: T1c-3 (#701): the `bind`/`call` function-value stdlib, the authoritative
  display form, and structural equality land — all observable through
  `@brink-lang/web`:

  - **`bind(f, args…)` stdlib intrinsic**: val-only currying over an existing
    function value — consumes the head of the remaining param row and returns
    a new function value (lowercase, brink-dialect-gated, author-shadowable
    with the E035-class warning, effect-transparent). Lowers to the new
    `bind_value` opcode (`0xD9`), which disassembles alongside `call_value`.
    Over-binding more args than the target has remaining params, or binding a
    non-function value, is a turn-terminating fault (spec §3).
  - **Display form**: `string(f)` and `{f}` interpolation now render the stable
    signature-like form — `fn heal(ref hp = player_hp, amount)` (bound `val`
    args print their value, bound `ref` args print the captured cell name,
    unbound params print bare). This is a permanently observable surface (spec
    §5), property-tested for stability.
  - **Structural equality**: `==`/`!=` on two function values compare
    structurally (same fn token + equal bound rows); any ordering operator
    (`<`, `>=`, …) is a runtime fault in gradual mode / a type error in strict
    (spec §5). Function values remain rejected as map keys.

  Crates-only work (bevy-brink also gains the host callback-invocation surface,
  `call_ink_function_value`), but the runtime-observable behavior above flows
  through `@brink-lang/web`, so it carries a patch per the wasm-observable rule.

- 2541c08: T1c-4 (#702) mechanical tail — corpus growth, a new "Function Values" book
  chapter, and IDE polish. Only the IDE polish is observable through
  `@brink-lang/web`:

  - **Hover on a fn-value slot** (a `VAR`/`CONST`/`temp` bound directly to a
    `#fn(target, args…)` literal, at its declaration or a later plain
    assignment) now shows the bound signature display form — the same
    `fn heal(ref hp = player_hp, amount)` shape `string(f)` renders at
    runtime (spec §5), built statically from the HIR. Every other hover case
    is unchanged; a slot never bound to a direct `#fn(...)` literal (a
    `bind()` result, a copy of another variable, an ordinary value) shows
    nothing extra, same as before.
  - **Completion after `#fn(`** now offers only statically-named function
    definitions (the same shape `#fn`'s E079 creation-site check requires),
    not the generic value-symbol list every other call-argument position
    offers. Completion everywhere else (including `#fn(name, ` — past the
    first argument) is unchanged.

  Crates-only otherwise: the tier1-brink corpus wing grows (a triple-level
  `bind`-of-`bind` chain, a wrong-typed-argument fault, the cross-flow
  `#@local` `ref`-bind fault, and save/load with a live function value inside
  an array/map), and grammar fuzzing extends to `#fn` in both dialects
  (parser is dialect-agnostic, so this is parser-layer coverage) — none of
  this changes any wasm-observable behavior.

- 5b07740: Fix #708: a bare `INCLUDE` (no path) no longer aborts compilation with a
  raw I/O error. Discovery now skips the empty include path the parser
  already flagged, so the parser's `E037` ("expected file path")
  diagnostic reaches the caller. Observable through `@brink-lang/web`:
  `compile_project`/`compile_fragment`/editor compiles on a project
  containing a bare `INCLUDE` now return `ok: false` with an `E037`
  warning entry (placed on the offending line) instead of a generic
  `error: "I/O error: file not found: …"` string with no source location.
- d02c4e2: T1c follow-up (#712): a global `VAR`/`CONST` initialized with `#fn(...)`
  (or annotated `fn(T…): R`) now carries its declaration-derived `Ty::Fn`
  through to call-position checking under `types = strict`, instead of
  escaping as `Unknown`. Observable through editor diagnostics:

  - Calling directly through such a global (`heal_player(5)`, no local temp
    in between) type-checks against the target's known signature: arity/
    argument-type mismatches report `E063` exactly as they already did for a
    `#fn(...)`-initialized local temp.
  - An explicit `VAR f: fn(int): int = …` annotation on the global now wins
    over inference, matching the existing annotation-wins firewall rule.
  - Reassigning a fn-typed local from two globals with genuinely
    incompatible signatures still reports the pre-existing `E066`
    (Conflicted-escape) — previously masked because both globals silently
    escaped as `Unknown`, which unified without a conflict.
  - Gradual mode is unaffected — these checks only ever run under
    `types = strict`.

- 20d2bfa: T1c-2 completion gap fix (#721): the direct-call form `f(args…)` — where
  `f` is a variable/temp holding a function value — dispatches through the
  same `call_variable` opcode as the classic divert-target-variable call.
  That opcode carried no argument count, so the popped-arg count for the
  function-value arm was derived from the resolved target's arity instead
  of the count actually supplied at the call site; a gradual-mode arity
  mismatch on the direct form could leave a stray value on the stack
  instead of faulting.

  `call_variable` now carries an explicit `argc` operand (codegen emits the
  exact count pushed at that call site; the divert-target-variable-call arm
  ignores it, unchanged). Observable through `@brink-lang/web`:

  - **Disassembly**: `call_variable` now renders as `call_variable
argc=<n>` in program-model output.
  - **Runtime dispatch**: a wrong-arity direct call `f(args…)` now faults
    with the same `FunctionValueArity` turn-terminating fault as the
    explicit `call(f, args…)` form, instead of risking a corrupted value
    stack.

- d38fa08: Fixed #743: a bare `VAR` reference _nested one level inside_ a
  `VAR`/`CONST` declaration-default collection/struct/`#fn` literal — an
  array element, a map value, a struct field, or an `#fn(name, args…)`
  bound `val` arg — previously folded silently to `Null` through
  `eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm, with zero
  diagnostic. This is the residue #679's scope notes flagged and #692/
  `E083` deliberately left alone (`E083` governs only the _whole_
  top-level default, not a construct nested one level in).

  Observable through `@brink-lang/web`: compiling such a nested `VAR`
  reference (array element / map value / `#fn` bound `val` arg) now
  surfaces the existing, non-suppressible `E077` — the same code
  `#673`/`#679` already use for any other never-constant nested element
  kind — instead of silently producing a `Null` entry. A struct field
  was already covered (any struct literal used as a declaration default
  is unconditionally `E075`, regardless of field content). A `Path`
  reference resolving to a `CONST`/list item/knot/stitch/function is
  unaffected — it still folds for real.

- 9bef954: T1d-1 (#757): `Value::Handle` — the runtime + format spine for opaque
  host-resource tokens (`docs/t1d-spec.md` §2/§6), the first emission of the
  V4-reserved `VAL_HANDLE` wire tag. No literal syntax and no new opcode —
  handles enter the script world only via bindings. Observable through
  `@brink-lang/web`:

  - **Native binding-argument marshal** (`value_to_js`): a handle passed as
    an argument to a JS-implemented external now crosses as a plain object
    `{ kind, id }` (`kind` the raw manifest `NameId`, `id` a decimal string
    so a full-range `u64` never loses precision as an `f64`) instead of
    silently folding to `null` (the #667 wildcard-arm hazard class).
    Deliberately **not** reconstructed by `js_to_value` — letting any JS
    object shaped `{kind, id}` become a real `Handle` would let a binding
    forge a capability token out of thin air.
  - **Speculation / eval-function results** (`value_to_typed_js`): a handle
    crosses the typed-value JSON boundary as `{ type: "handle", kind, id }`
    — `kind` resolved to its manifest name where possible (`"?"` for a stale
    `NameId`), `id` as a decimal string for the same precision reason.
  - **Program model / disassembly**: a handle default value (reachable once
    T1d-2 wires manifest-aware bindings into declaration defaults) renders
    as `handle <Kind>#<id>`, not `null`.

  Runtime-side (not directly `@brink-lang/web`-observable, but load-bearing
  for the above): `Value::Handle { kind: NameId, id: u64 }` with token
  equality (`kind == kind && id == id`), no ordering (any `<`/`>`/`<=`/`>=`
  is a runtime `TypeError` fault), and never a legal map key. `string(h)`
  displays as `handle <Kind>#<id>`. Handles save/load and journal-replay as
  ordinary values. The `.inkt` textual format gains a matching `(handle
<kind> <id>)` atom and `:handle` declared-type keyword, both with a real
  reader landing in this same PR (the #742 write/read-asymmetry class this
  PR does not repeat for its own new atom).

- 1e71455: Added the M-1 module name model (docs/modules-spec.md §1/§5): every `.ink`
  file is a module named by its stem, and a file-level `#@module(name)`
  directive declares the module explicitly. `DefinitionId`s are now hashed
  as `(module, name)` for **declared** modules; INCLUDE-glued files inherit
  their includer's module. An undeclared file whose stem collides with a
  declared module's name is a compile error (`E085`), and a malformed
  `#@module` (missing/empty name, or a second declaration) is `E086`.
  `#@module` is brink-dialect-only — under strict-ink it is rejected with
  the standard `E051`-class diagnostic.

  Identity is unchanged for the entire pre-modules world: an undeclared
  stem-module contributes nothing to the hash, so every existing story's
  `DefinitionId`s — and every saved game — stay byte-identical.

## 0.10.1

### Patch Changes

- e2acdbb: CONST declarations now accept a TM-2 inline type annotation
  (#641, docs/typed-mode-spec.md §3: "optional anywhere"): `CONST name: type
= expr`, mirroring the `VAR` annotation surface end to end.

  Superset grammar (`brink-syntax`): `const_declaration` now peeks for
  `at_type_annotation` after the identifier, same discipline as
  `var_declaration` — an unannotated `CONST` produces the exact same CST as
  before this change. HIR (`brink-ir`) gains an `annotation: Option<TypeExpr>`
  field on `ConstDecl`, lowered structurally with no validity checking.

  Analysis (`brink-analyzer`): `dialect_gate` flags a `CONST` annotation as
  `E051` under `strict-ink`, same as every other TM-2 annotation site.
  Annotation _content_ checks (`E061` unknown type name / `E062` reserved
  `fn(...)` type) run through the same `finish_analysis`-gated call as `VAR`
  — brink dialect only (maintainer ruling 2026-07-13), verified rather than
  re-gated. `signature()`'s firewall now resolves a `CONST`'s annotation and
  has it override the literal-inferred `value_type`, same annotation-wins
  rule as `VAR`.

  `brink-fmt` renders the annotation for free through the existing
  single-line declaration renderer — idempotence tests added, no renderer
  change. `brink-ide`'s parse → HIR → analyze → project pipeline doesn't
  crash on annotated or reserved/unknown-type `CONST` sources. Grammar fuzz
  coverage (`proptest_syntax.rs`) extended with a `CONST`-typed strategy
  mirroring the existing `VAR`-typed one.

  Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
  brink-dialect annotation syntax, and the grammar addition is fully
  optional/additive.

- 6ed8a8d: #585: a nested choice (or labeled gather block/conditional/sequence)
  embedded inside an un-lifted inline conditional in a choice's own
  display/bracket/inner text (e.g. `_ Pick {x > 0: - true: _ nested -> END

  - else: text}`) is now a targeted, Error-severity compile error (`E059`),
replacing a `debug_assert!(false, …)` guard on the arm that handles it.

  This is a real behavior change for real ink, not just defense-in-depth:
  in a release build (including the shipped `@brink-lang/web` wasm), the
  `debug_assert!` was compiled out, so `lower_stmt` silently returned `None`
  and dropped the nested statement — `lower_to_program` still produced
  `Some(program)` with no diagnostic, and `lir_query` treated that as a
  successful compile. The web playground would silently accept this input
  and produce a wrong story with the nested construct missing, with no
  indication anything was lost. With `E059` now Error-severity, `lir_query`
  gates on it (`program: None` once `lir_errors` is non-empty), so this same
  input now fails to compile in the web build instead of silently
  miscompiling. Sibling of #578's analogous `E057`/`E058` hardening
  (`t1b-4-diagnostics-hardening.md`), which shipped the same kind of
  changeset for the same reason.

  #586's codegen backstop (out-of-loop `LogicBreak`/`LogicContinue` in
  `brink-codegen-inkb`) is unaffected: that input is already rejected
  non-suppressibly upstream by LIR lowering's `E057` before a `Program`
  ever reaches codegen, so no valid compile path's observable behavior
  changes — no changeset needed for that half.

  Oracle corpus: unchanged, 5,577 passing episodes.

- eb06ccc: #603: formatting a T1b `~ { … }` multi-line logic block whose CST subtree
  contains a parse error (mid-edit or otherwise malformed input) now bails to
  byte-for-byte verbatim pass-through for that block only, instead of running
  it through `render_logic_block`'s indentation-aware reindenting. #602's
  reindenting assumed a well-formed subtree; on malformed input it could
  corrupt the block (a trailing `//` comment between an `if` condition and its
  `{` swallowed the brace onto the comment line; a multi-line call under a
  parse error injected spurious blank lines and broke idempotence; a lone
  `else`/brace line got mangled into mismatched braces). Well-formed `~ { … }`
  blocks are unaffected and continue to reindent as before; everything outside
  `~ { … }` blocks is untouched.

  This is reachable from the web playground: `brink-web` depends on
  `brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
  `formatting.rs`; `brink-web` exposes those as the `code_actions` /
  `code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
  "Format knot" / "Format stitch" code actions). Running a code action on a
  knot/stitch containing a malformed `~ { … }` block (the normal state of a
  block mid-edit) now leaves it untouched instead of corrupting it.

- 1154eb4: Added a recursion-depth cap (128 levels, `MAX_DECODE_DEPTH`) to the
  `VAL_ARRAY`/`VAL_MAP` decoder in both the `.inkb` reader
  (`brink_format::read_inkb`, reachable from `@brink-lang/web`) and the
  runtime transcript reader (`.brkt`). Previously a crafted file of deeply
  nested single-element arrays (~5 bytes/level) could recurse unboundedly and
  stack-overflow the reader (#553). Nesting beyond the cap now returns a
  proper decode error (`DecodeError::MaxDepthExceeded` /
  `TranscriptError::MaxDepthExceeded`) instead of crashing the wasm module.
  Valid data — including hand-built collections nested exactly at the cap —
  decodes byte-identical to before; the oracle corpus is unaffected (still
  5,577 passing episodes).
- 0f6ae50: Wired `completions()`/`completions_doc()`, `signature_help()`/
  `signature_help_doc()`, and `folding_ranges()`/`folding_ranges_doc()` up to
  the #589 IDE entry points (#600), which had landed in `brink-ide` and
  `brink-lsp` but were never called from the wasm bridge:

  - Completion now offers the T1b stdlib slice 1 functions (`len`/`keys`/
    `values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5)
    as `kind: "stdlib"` items, once the new `set_language_dialect("brink")`
    session method is called (defaults to `"strict-ink"`, matching
    `AnalysisOptions::default()` — stdlib names are never offered until a host
    opts in, mirroring `brink-lsp`'s `initializationOptions.dialect`).
  - Signature help now calls `signature_help_with_dialect`, so a call to one of
    those same names shows its signature (mutators render their first
    parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) once brink dialect
    is set — falling back to `None` under the default, exactly like completion.
  - Folding now includes `~ { … }` logic-block folds (and their nested
    `if`/`while`/`for` sub-folds) as `kind: "structural"` ranges, unconditionally
    — no dialect gate, since the construct parses and lowers identically in
    both dialects (only strict-ink flags it as a diagnostic, `E051`).

  New host-facing API: `EditorSession.set_language_dialect(value: "brink" |
"strict-ink")`. No other wasm-observable behavior changed.

- e96d2a1: Fixed permanent spurious `E051` ("brink extension") diagnostics in the
  playground for `brink`-dialect projects (#611, the wasm-side twin of #599).

  `IdeSession::analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`
  (`brink-ide`) all built `AnalysisOptions` with `..AnalysisOptions::default()`
  for the `dialect` field, ignoring the session's declared T1b compiler
  dialect entirely — `EditorSession.set_language_dialect` (#589/#600) only
  gated stdlib completion and signature help, never the background analysis
  pass that produces diagnostics. A project opened with brink-dialect syntax
  (`~ { … }` blocks, `#[…]`/`#{…}` sigil literals, postfix indexing) kept
  showing `E051` on every valid construct no matter what dialect was set.

  `set_language_dialect` now forwards into `IdeSession`, which threads the
  declared dialect through all four analysis entry points and re-analyzes
  immediately (like `set_external_check`/`set_semantic_type_check`). No other
  wasm-observable behavior changed.

- bd69ac6: New pure conversion intrinsics `int(x)`, `float(x)`, `string(x)` under the
  brink dialect (maintainer-ruled domains: permissive numerics + bool;
  parse failure is a turn-terminating fault; float→int truncates toward
  zero matching `INT()`). New compileable surface reachable through the
  wasm compile entry points; out-of-domain arguments are `E078` compile
  errors under `types = strict`.
- f40c345: Wired `types = strict` (TM-3, #619) reachability through `brink-ide`,
  `brink-lsp`, and `brink-web` (#660) — PR #656 landed the strict-mode checks
  themselves but left `IdeSession`'s two `AnalysisOptions` literals hardcoded
  to `TypePolicy::Gradual` (no setter), so strict mode was reachable only via
  the compiler CLI's `--types strict`; the IDE/LSP/web surface could not turn
  it on at all.

  - `IdeSession` (`brink-ide`) gains `set_type_policy`/`type_policy`,
    mirroring `set_language_dialect`/`language_dialect` exactly. `snapshot()`
    and `analysis_options()` now thread the registered policy through instead
    of a hardcoded `TypePolicy::default()`.
  - `brink-lsp` reads `initializationOptions.types` (`"strict"` or
    `"gradual"`; defaults to `"gradual"`), mirroring the existing `.dialect`
    handling, and feeds it to both the foreground session and the background
    `analysis_loop`.
  - `EditorSession.set_type_policy(value: "strict" | "gradual")` (`brink-web`)
    mirrors `set_language_dialect` and re-analyzes immediately. `strict`
    requires `set_language_dialect("brink")`, or analysis (and
    `compile_project`) reports a single project-level `E064` config-error
    diagnostic instead of running the normal passes — the caller's
    responsibility, same as the CLI.

  **wasm-observable**: `EditorSession.set_type_policy` is a new host-facing
  entry point, and `compile_project`/background analysis now surface
  `E065`/`E066`/`E067`/error-severity-`E063` diagnostics (or `E064`) for a
  project that opts in — behavior no wasm consumer could previously reach at
  all. No other wasm-observable behavior changed; the default (`Gradual`,
  never calling `set_type_policy`) is byte-identical to before.

- f25362a: Fixed #673: a collection or struct literal used as a `VAR`/`CONST`
  declaration default (`VAR arr = #[1, 2, 3]`, `VAR m = #{"a": 1}`, `VAR p =
Point#{x: 1.0, y: 2.0}`) used to compile silently to `Value::Null` with no
  diagnostic — `brink-ir::lir::lower::decls::eval_const_expr` (the
  compile-time constant-folding path `VAR`/`CONST` defaults go through) had
  no arm for `ArrayLiteral`/`MapLiteral`/`StructLiteral` and fell through to
  its catch-all `_ => ConstValue::Null`.

  - Array/map literal defaults (including nested ones, and constant
    references inside them, e.g. `#[SOME_CONST, 2]`) now constant-fold into
    the real `ConstValue::Array`/`Map` — the same representation
    `brink-codegen-inkb` already materializes into a real `Value::array`/
    `Value::map` global default (this wiring already existed for
    expression-position array/map literals; declaration defaults now share
    it). A map key that isn't a compile-time-constant scalar (int/string/
    bool) in a declaration default is a new compile error (`E076`) — a
    declaration default has no runtime `MapNew` construction step left to
    fault at the way a mid-story map literal does. Likewise, an array
    element or map value whose expression can never constant-fold (a
    function call, indexing, field access, or `++`/`--` — e.g. `VAR arr =
#[f(), 2]`) is a new compile error (`E077`), never a silently-`Null`
    element. The check is keyed off the source expression kind, so
    constant-foldable shapes (`#[1 + 2, -SOME_CONST]`, nested literals)
    are unaffected.
  - A struct construction literal used directly as a declaration default is
    a new compile error (`E075`) — `ConstValue` has no record-carrying
    variant (adding one is a format question outside this fix), and unlike
    arrays/maps there's no existing runtime-construction step for a
    declaration default to defer to. Construct the struct via an ordinary
    assignment after declaration instead (`VAR p = 0` then `~ p =
Point#{...}`).

  `E075`, `E076`, and `E077` are LIR-lowering diagnostics, so — like `E053`/
  `E073`/`E074` — they're never suppressible via `// brink-disable`/
  `// brink-disable-all`.

  Oracle corpus: unchanged, 5,577 passing episodes — vanilla ink has no
  collection/struct sigil literals for this to affect.

- ebce613: T1c-1: `#fn(name, args…)` function-value creation — grammar, HIR, typing,
  and strict call checking (#699, docs/t1c-spec.md §2/§4/§8). Observable
  through editor diagnostics:

  - `#fn(…)` parses in expression position (superset grammar, the
    `#[…]`/`#{…}`/`Name#{…}` sigil family); under `strict-ink` it rejects at
    analysis with the standard E051 "brink extension" diagnostic. Prose
    position is unchanged — `#` still opens a tag.
  - New creation-site diagnostics under `dialect = brink`: E079 (target is
    not a statically-named function definition), E080 (a `ref` param unbound
    at creation, or bound to a non-durable lvalue — temps/params, CONSTs,
    rvalues, and field projections all reject; only VAR cells are durable),
    E081 (more args bound than the target declares).
  - `fn(T…): R` type annotations are now legal (E062 retired — it no longer
    fires) and resolve to a real checker type; unknown names inside a fn type
    still flag E061.
  - Under `types = strict`, calls through function values are statically
    checked via the existing TM-3 codes: Unknown callee → E065, Conflicted
    callee → E066, non-callable/arity/argument-type mismatches → E063 (the
    `int → float` coercion applies to call arguments).
  - Compiling a program that actually uses `#fn` under `dialect = brink`
    still rejects at lowering with a targeted E052 ("not yet implemented" —
    LIR/codegen/VM land in T1c-2). No behavior change for strict-ink
    projects or gradual-mode diagnostics.

- 3c5808f: Added `EditorSessionHandle.setSemanticTypeCheck(level)` (#532), mirroring
  `setExternalCheck`. Previously `WasmEditorSession.set_semantic_type_check`
  was only reachable on the raw wasm session, which `EditorSessionHandle`
  holds in a private field — the `@brink-lang/web` public wrapper had no
  method to reach it, so the severity lever was dead code for consumers of
  the package. `setSemanticTypeCheck("tolerant" | "error")` now delegates to
  the wasm session and bumps the generation counter, same as every other
  mutating call on the handle.
- eaff136: Added the T1b superset grammar (#569, docs/t1b-surface-spec.md §§1-4):
  multi-line `~ { … }` logic blocks (assignment, `temp`, `if`/`else if`/`else`,
  `while`, `for … in …`, `break`/`continue`, `return`, expression statements),
  `#[…]`/`#{…}` sigil collection literals in expression position, and postfix
  indexing (`a[0]`, chained `grid[y][x]`) plus indexed assignment. Parsed
  through CST/AST/HIR; nothing lowers to LIR or codegen yet (lands in T1b-2).

  Introduced a compiler dialect gate (`AnalysisOptions::dialect`,
  `Dialect::{StrictInk, Brink}`, default `StrictInk`) — a new analysis input,
  not embedded in `.inkb`. Under `StrictInk` (the default every existing
  caller gets), every extension construct now produces a targeted diagnostic
  (`E051`) at its exact span instead of whatever parse/analysis error it
  previously produced for that byte sequence. Under `Brink`, the same
  constructs produce a "not yet implemented — lands in T1b-2" diagnostic
  (`E052`). Both dialects still fail to compile source using this syntax —
  this is a diagnostic-quality change for a previously-unsupported syntax
  shape, not new compileable output.

  Plain ink is unaffected: the oracle corpus remains byte-identical (5,577
  passing episodes) since none of it uses the new syntax, and the new grammar
  is purely additive (`if`/`while`/`for`/`break`/`continue`/`in` are
  contextual keywords, recognized only at block-statement-start position
  inside a new `~ { … }` block — they remain ordinary identifiers everywhere
  else, so no existing knot/variable/function name is reserved).

  The dialect gate's diagnostics are analysis diagnostics, so they can be
  suppressed like any other (`// brink-disable-all` or a line directive). LIR
  lowering now defends against that case directly: if a suppressed gate lets
  extension syntax reach LIR lowering, compilation still fails with a new
  internal-error diagnostic (`E053`) instead of silently dropping the
  construct or corrupting the compiled output.

- ba69a35: T1b-2 (#570): the brink dialect now compiles and runs the full T1b surface
  (docs/t1b-surface-spec.md §§2-4) — `~ { … }` logic blocks (`if`/`else if`/
  `else`, `while`, `for x in arr`/`for k in map`, `break`/`continue`, `return`,
  block-scoped `temp`), `#[…]`/`#{…}` sigil collection literals (constant
  literals go through a new V4 literal pool, `PushLiteral(idx)`; dynamic
  literals through new `ArrayNew`/`MapNew` opcodes), and postfix indexing
  (`a[0]`, chained `grid[y][x]`) including indexed assignment via the ratified
  RMW discipline (take → `make_mut` → write-back on the root cell; chains
  lower to nested RMW through synthetic temps, never interior references).
  Out-of-bounds array indices and missing map keys are turn-terminating
  runtime faults on both read and write — no silent growth on write-past-end.

  The `Brink` dialect no longer rejects any of this ("not yet implemented —
  lands in T1b-2", `E052`) — it just compiles. `StrictInk` is unaffected
  (`E051` still rejects every extension construct at its exact span).

  Block-scoped `temp` declarations (including `for` loop variables) thread
  into the same symbol manifest the IDE's cross-ref/rename/unused-variable
  tooling reads, and get a new warning (`E054`) when they shadow an
  already-visible temp (an outer classic `~ temp` or an enclosing block).

  Format: a new `LiteralPool` `.inkb`/`.inkt` section (additive alongside the
  existing `ListLiterals` section — `PushList` is unaffected) and twelve new
  opcodes in the previously-reserved `0xBE`-`0xC9` block (`ArrayNew`, `MapNew`,
  `IndexGet`, `IndexSet`, `CollectionLen`, `MapGet`, `MapInsert`, `MapRemove`,
  `MapContains`, `CollectionKeys`, `CollectionValues`, `PushLiteral`) — inert
  until this compiler surface emits them, so no existing `.inkb` output
  changes shape unless it uses T1b syntax. Also fixes a pre-existing gap found
  while adding this: the `.inkt` text format's `value`/`type_name` grammar
  could not parse an `Array`/`Map` value back (only write one) — global
  variable defaults with a collection default (possible since #525) now
  round-trip correctly too.

  Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
  never reaches any of this new surface by construction.

- 124bb9e: T1b-3 (#571): the brink dialect ships stdlib slice 1
  (docs/t1b-surface-spec.md §5) — lowercase free functions, brink-dialect-gated
  (`strict-ink` never sees them). Pure: `len(x)`, `keys(m)`, `values(m)`,
  `contains(x, v)` (arrays: element containment; maps: key containment).
  Mutating: `push(a, v)`, `insert(x, k_or_i, v)`, `remove(x, k_or_i)` — all
  three require an lvalue first argument (a variable, temp, or indexed path)
  and lower through the same take → `make_mut` → write-back RMW discipline
  indexed assignment uses (§4); passing an rvalue (`push(#[1, 2], 3)`) is now
  a targeted compile error (`E055`), and using a mutator's result — they
  return nothing — is a compile error too (`E056`).

  An author-defined function of the same name shadows the builtin, with a
  warning (`E035`, reusing the existing "name shadows a built-in function"
  code); imported vanilla ink that defines e.g. `len` keeps working under the
  brink dialect. Under `strict-ink`, an unresolved call to any of the seven
  names is now rejected the same way other brink-extension syntax is (`E051`).

  VM-native: the array-generalized `MapInsert`/`MapRemove`/`MapContains`
  opcodes (reserved+live since #575, now compiler-emitted) are the mutators'
  primitives — despite the `Map*` names, they now also handle `Array`
  containers (index-based insert-with-shift/remove-with-shift/element-scan),
  since the frozen v4 collection-opcode block has no dedicated array-append
  opcode and the RFC's one-bump rule reserved exactly this set. `push(a, v)`
  desugars to `insert(a, len(a), v)`. No wire-format change — same opcode
  bytes, generalized VM-side semantics.

  Also fixes a latent gap this surface exposed: diagnostics produced during
  LIR lowering (as opposed to the earlier analysis phase) were always treated
  as warnings regardless of their own severity, so an Error-severity one could
  never actually block compilation. `E055`/`E056` are the first Error-severity
  diagnostics LIR lowering ever produces; the pipeline now partitions
  lowering-phase diagnostics by severity like every other diagnostic source,
  so they correctly fail the compile instead of silently compiling anyway.

  Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
  never reaches any of this new surface by construction.

- 75b8a3b: T1b-4 diagnostics/semantics hardening (#577, #578, #580, #581, #568):

  - **#577**: `break`/`continue` used outside any enclosing `while`/`for`
    loop is now a targeted, Error-severity compile error (`E057`). Previously
    it lowered unconditionally to an unguarded jump, and codegen silently
    degraded that to a no-op (`Opcode::Nop`) instead of ever surfacing an
    error — the compiler would accept clearly-wrong ink and produce dead
    bytecode for it. The check runs at LIR-lowering time (the same layer as
    the T1b-3 mutator checks, `E055`/`E056`), so it is a real, non-suppressible
    compile error, not a suppressible analysis diagnostic.
  - **#578**: an inline multiline conditional/sequence that keeps its
    `InlineConditional`/`InlineSequence` shape all the way to LIR lowering
    (rather than being lifted to a top-level statement by HIR normalization —
    reachable via a choice's own display/bracket/inner text, which
    normalization never touches, or via a second inline construct on one
    content line) could contain a `~ { … }` T1b logic block. Lowering that
    case hit an internal `debug_assert!`-guarded "unreachable" arm — a panic
    in debug builds, a silent statement drop in release. It now routes
    through the same real lowering path top-level blocks use.
  - **#580** (RULED): `contains(map, needle)` with a `needle` outside the
    map-key domain (a float, array, map, …) now returns `false` instead of
    faulting — total on both the array and map branches, matching the array
    branch's existing behavior. Indexing/mutation faults on a bad key are
    unchanged (value-model-spec §6); `contains` never had a "the key isn't
    there" failure mode to escalate to a fault the way those do.
  - **#581** (RULED): a collection mutator (`push`/`insert`/`remove`) called
    with the wrong argument count is now a targeted, Error-severity compile
    error (`E058`) naming the expected signature (e.g.
    `push(container, value)`), replacing the generic `E031` warning the arity
    check used to share with ordinary function-call arity checking. E031
    never blocked compilation, so a malformed mutator call used to silently
    vanish from the lowered bytecode with no compile failure. Pure-function
    arity checking is unchanged.
  - **#568**: a debug-build `console.warn` diagnostic for the third lossy-leg
    failure mode at the `value_to_js` wasm boundary (alongside the existing
    key-coercion-collision (#555) and key-reordering (#564) diagnostics): a
    `Value::Float` map value whose lossless `f32` → `f64` widening would print
    with more digits in a real JS engine than the value's own shortest
    decimal (e.g. `0.1f32` widens to the `f64` whose shortest round-trip
    decimal is `0.10000000149011612`). No value precision is actually lost —
    the widening is exact — but the extra digits are a genuine
    "where-did-these-come-from" surprise. Diagnostic-only; `value_to_js`'s
    marshaling is unchanged.

  Oracle corpus: unchanged, 5,577 passing episodes.

- 350b663: Added hover text for the T1b stdlib slice 1 functions (`len`/`keys`/
  `values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5,
  #589): hovering one of these names now shows its signature (mutators render
  their first parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) and a
  one-line semantics summary, unconditionally — like the existing built-in
  (`INT`/`FLOOR`/…) hover text — so the info is available even in a strict-ink
  project, where a use of the name is otherwise flagged as a brink extension.
  No other wasm-exposed behavior changed: the new dialect-gated stdlib
  completion, signature help, and `~ { … }` block-folding queries land in
  `brink-ide` and are wired into `brink-lsp` only in this PR — the
  `@brink-lang/web` bridge (`brink-web`) does not yet call them.
- 9e1257d: T1b-4 (#576): closes the indexed-write COW cliff value-model-spec §5
  promises but PR #575 hadn't yet delivered — `blocks.rs`'s RMW lowering read
  the root/intermediate cells it mutates via `GetGlobal`/`GetTemp`, which
  `Arc`-clone the slot instead of consuming it, so `array_make_mut`/
  `map_make_mut` always saw a shared `Arc` and COW-copied on every write —
  O(n) per write, O(n²) for a loop of indexed writes or `push`es.

  Two new opcodes in the previously-reserved sharing-discipline block
  (`docs/format-v4-rfc.md` §3): `TakeGlobal(DefinitionId)` at `0xCA` and
  `TakeTemp(u16)` at `0xCD` (freshly claimed, adjacent to the reservation —
  `0xCB`/`0xCC` stay reserved for `StoreVarIfNew`/`EqVars`). Both move a
  slot's current value out, leaving `Value::Null` behind, instead of cloning;
  `TakeTemp` auto-dereferences like `GetTemp` (a `ref` parameter's pointed-to
  location is taken, the pointer itself untouched).

  The compiler now emits them for the **flat** RMW shape — `a[i] = v`/
  `a[i] op= v` and `push`/`insert`/`remove` on a bare variable, the exact
  loop-append case the spec's "one cliff" targets — with every other
  sub-expression (index, value, and for indexed assignment the pre-mutation
  `current` read) evaluated _before_ the take, so an expression referencing
  the same variable by name still sees its pre-mutation value. **Chained**
  indexed assignment/mutators (`grid[y][x] = v`, `push(grid[y], v)`) are
  unchanged: a nested element is still referenced from inside its parent
  until the write-back cascade completes, so a take at any level but the
  root buys nothing there — the sanctioned §7 clone-based fallback stays in
  place for that shape.

  **Fault-during-RMW slot state** (a new, deliberately-defined behavior): for
  indexed assignment and `push`, a fault (out-of-bounds index, missing map
  key, non-collection root) is now caught by a non-mutating pre-check
  _before_ anything is taken, so the root is **never** lost to a fault on
  these paths — identical to the pre-#576 behavior. `insert`/`remove` at an
  arbitrary author-supplied key don't get an equivalent free pre-check; a
  fault there leaves the taken root holding `Value::Null` — a documented,
  tested trade-off consistent with this VM's pre-existing no-rollback-on-
  fault model (a fault anywhere mid-turn already leaves earlier same-turn
  mutations applied).

  Benchmark (`crates/brink-runtime/benches/runtime.rs`, `loop_append_bench`):
  10k sequential `push`es on a freshly-created array — measured 464.8ms
  median before this change, 13.91ms median after (~33x), consistent with
  closing an O(n²) cliff.

  Oracle corpus: unchanged, 5,577 passing episodes — T1b syntax never reaches
  the strict-ink corpus by construction.

- 9213d77: Formatting a T1b `~ { … }` multi-line logic block (docs/t1b-surface-spec.md
  §2) now reindents its internals instead of passing them through verbatim
  (#573): 4-space indent per nesting level inside the block, opening brace on
  its statement's line, closing brace on its own line at the parent depth,
  one statement per line, comments and blank lines preserved in place
  (blank-line runs collapse to one, trailing comments stay attached). This
  supersedes T1b-1's verbatim pass-through contract. Everything outside `~ {
… }` blocks is untouched.

  This is reachable from the web playground: `brink-web` depends on
  `brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
  `formatting.rs`; `brink-web` exposes those as the `code_actions` /
  `code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
  "Format knot" / "Format stitch" code actions). A knot or stitch containing a
  `~ { … }` block now formats differently through the playground.

- f835cfd: Format VERSION 4 (T1a-4, #526): the single planned Tier-1 format bump. The
  `.inkb` binary and the runtime transcript (`.brkt`) now serialize
  `Value::Array`/`Value::Map` as tree encodings (a length prefix then
  recursively-encoded children, insertion order preserved, map keys restricted to
  the scalar `int`/`string`/`bool` domain) instead of folding a collection to
  `null`. A binding/external that returns a collection and is inline-emitted
  (`{ext()}`) now round-trips through the persisted transcript byte-for-value
  identical. The strict reader hard-rejects any version but 4. The reserved
  Tier-1 value-tag/section/opcode surface (function values, closures, handles,
  projections, records) is frozen at v4 per the §9 one-bump rule so later
  milestones add data without another bump. Compiled `.inkb`/transcript bytes read
  through `@brink-lang/web` change accordingly; no opcode emits collections yet,
  so scalar behavior and the oracle are byte-identical.
- b8392a2: Tier-1 value model, state plumbing (T1a-3, #525): collection values
  (`Array`/`Map`) now cross the wasm JSON boundary as structured trees instead of
  folding to `null`. `eval_function`/`resume_function_eval` results serialize an
  array as `{ "type": "array", "items": [...] }` and a map as
  `{ "type": "map", "entries": [{ "key": ..., "value": ... }] }`, preserving
  insertion order and each key's scalar type (int/string/bool). A JS binding that
  returns a native array or plain object is now read back as an ink `Array`/`Map`
  (recursively; object keys become string map keys in JS property order), and an
  ink collection passed to a binding marshals to a native JS array/object.
  Snapshot-only per value-model spec §8 — the boundary copies trees; the host
  never retains a handle into script state. No opcode emits collections yet, so
  scalar behavior and the oracle are byte-identical.
- 6089ed6: Added TM-2 inline type annotation syntax (#618, docs/typed-mode-spec.md
  §3): `name: type` after knot/stitch params and `VAR`/`temp` declarations,
  `): type ===` in the function-header return position, and the `~ temp
name: type = expr` ascription form. Type names are lowercase nominals —
  `int`, `float`, `bool`, `string`, `divert`, `void`, `list<L>` (nominal per
  a declared `LIST`), `array<T>`, `map<K, V>`; `fn(T…): R` function types
  parse but are reserved until T1c (a targeted diagnostic, `E062`, fires on
  any use). An unrecognized type name gets a targeted diagnostic (`E061`).

  Superset grammar, same dialect-gate pattern as T1b: `brink-syntax` always
  parses annotations regardless of dialect; under `strict-ink` every
  annotation is a brink-extension diagnostic (`E051`) at its span, same as
  every other T1b extension construct. `E061`/`E062` are unconditional in
  both dialects (they check annotation _content_, independent of whether the
  syntax itself is allowed).

  Annotations feed `signature()`'s firewall: an annotated knot/stitch param
  or knot return type is exposed on `Sig` (`param_annotations`,
  `return_annotation`); an annotated `VAR` overrides its literal-inferred
  `value_type` (annotation wins over inference) — the existing
  `infer::collect_globals` seam picks this up with no further change. A new
  `annotation_mismatches` function compares an annotation against TM-1 body
  inference and reports a disagreement (`E063`, advisory/warning severity —
  strict-mode policy is TM-3's call). `~ temp` ascriptions parse and lower to
  HIR but aren't yet wired into body inference (that would touch
  `infer::body::BodyCtx`, out of scope per #638).

  `brink-fmt` renders annotations for free through its existing single-line
  token-collapsing passes (knot headers, declarations, logic lines) — no
  renderer changes were needed, only idempotence tests. `brink-ide`'s
  parse → HIR → analyze → project pipeline doesn't crash on annotated or
  reserved/unknown-type sources. Grammar fuzz coverage (`proptest_syntax.rs`)
  extended with a depth-bounded type-expression strategy so the superset
  parser never panics on type-annotated input.

  Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
  brink-dialect annotation syntax, and the grammar addition is fully
  optional/additive at every position it touches.

- 1c389ec: TM-4 (#620) foundation: `Value::Record` lands in the shared value core —
  closed-shape records with an interned `ShapeId` and a flat, shape-ordered
  field vector, following the exact COW/equality/serialization machinery
  already ratified for `Array`/`Map` (Arc-shared field vector, `make_mut`
  copy-on-write, structural equality with an `Arc::ptr_eq` fast path, plus a
  shape-identity check — two records are never equal unless their shapes
  match). Round-trips through `.inkb`, the `.inkt` text format, and the
  runtime transcript (`.brkt`), all via the new `VAL_RECORD` (`0x0F`) wire tag
  (`docs/format-v4-rfc.md` §1).

  Format: the reserved `StructShapes` `.inkb` section (`0x0C`) goes live —
  shape id, name, and ordered field names, wired into `write_inkb`/`read_inkb`
  alongside the existing sections (`SECTION_COUNT` 11 → 12; every checked-in
  `.inkb` fixture regenerates once with the extra section, per the
  single-version regenerate-on-mismatch policy). Three new opcodes go live in
  the previously-reserved field-op block (`0xCE`-`0xD0`): `RecordNew(shape_id)`,
  `RecordGetDyn(name_id)`, `RecordSetDyn(name_id)` — the by-name field
  construct/get/set ops correct in both dialects (turn-terminating fault on a
  missing field, matching the existing `MapGet`/`IndexGet` fault pattern).
  Static-offset field ops (`RecordGet(offset)`/`RecordSet(offset)`, the
  strict-mode performance payoff `docs/typed-mode-spec.md` §6 anticipates)
  stay named and numbered (`0xD1`-`0xD2`) but reserved — no `Opcode` variant
  yet, the same "reserved, decode rejects" discipline `StoreVarIfNew`/`EqVars`
  already established.

  No compiler surface (grammar/HIR/analyzer/LIR/codegen for `STRUCT`
  declarations or `Name#{…}` construction) is included in this PR — every new
  opcode/section is inert until a follow-up compiler milestone emits it,
  mirroring how T1a's collection-value reservation preceded T1b's live
  grammar/codegen wiring. See the PR description's scope note for what
  remains open against issue #620.

  Oracle corpus: unchanged, 5,577 passing episodes — nothing in the compiler
  pipeline changes; the new surface is reachable only through direct
  hand-assembled bytecode (this PR's own VM tests) and the `.inkb`/`.inkt`/
  transcript round-trip tests.

- 81f0055: TM-4b (#665): the struct compiler surface lands — grammar, HIR, and
  analyzer, diagnostics-only (codegen lands with TM-4c, #666), per
  `docs/typed-mode-spec.md` §6.

  - **Grammar** (brink-syntax): `STRUCT Name = #{ field: type, … }`
    declarations (single-line or multi-line — the body mirrors the
    construction literal's shape); `Name#{field: expr, …}` construction
    literals in expression position; postfix `.field` access wherever the
    existing dotted-`PATH` grammar doesn't already cover it (a bare
    `ident.ident` chain still parses as one `PATH`, unchanged). All brink
    extension syntax — superset grammar, byte-identical CST for every
    non-struct program.
  - **HIR** (brink-ir): `StructDecl`/`StructFieldDecl` items, `Expr::StructLiteral`,
    `Expr::FieldAccess`, `SymbolKind::Struct` manifest registration.
  - **Analyzer** (brink-analyzer): resolution fallback for field access
    (static dotted paths like `knot.stitch`/`List.Item` resolve first and
    win; only a head resolving to a variable/temp/param makes `.field` a
    field access); dialect gate flags every new construct under strict-ink
    (`E051`); `Ty::Struct` nominal joins the TM-2 annotation grammar
    (declared struct names no longer trip `E061`); strict-mode-only
    construction checks naming the offending field — missing (`E069`), extra
    (`E070`), mistyped (`E071`); unresolved shape names (`E068`).
  - **LIR**: struct constructs (construction literals, field access — both
    the new grammar and the ambiguous-path resolution-fallback case) reject
    with a real, non-suppressible `E072` diagnostic — the T1b-1 discipline
    (grammar/HIR/analyzer land before codegen) plus the E053-backstop lesson
    (a real diagnostic, not a `debug_assert!`-guarded silent drop).

  Wasm-observable surface: the parser accepts the new grammar (new
  `SyntaxKind`s reach `brink-ide`/`brink-web`'s CST-derived tooling); five new
  diagnostic codes (`E068`-`E072`) can now be produced and surfaced through
  `brink-web`'s diagnostics API; `editor_dto::symbol_kind_str` gains a
  `"struct"` arm (was previously unreachable — a new `SymbolKind` variant);
  the semantic-tokens legend gains a 13th token type, `"struct"` (existing
  indices unchanged, purely additive).

  Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
  `STRUCT`/`Name#{…}`/the new field-access grammar, and LIR lowering rejects
  every struct construct rather than emitting bytecode.

- 6e007d3: TM-4c (#666): structs become executable — LIR lowering + codegen for
  construction, field reads, and single-level field writes, per
  `docs/typed-mode-spec.md` §6.

  - **LIR** (brink-ir): `Expr::StructLiteral` lowers to `RecordNew(shape_id)`
    with initializers reordered into shape declaration order (each evaluated
    exactly once; see `lower_struct_literal`'s doc for the evaluation-order
    caveat when the author's field order differs from the shape's own).
    `Expr::FieldAccess` (and the ambiguous multi-segment-`Path` shape a bare
    `p.x` parses as) lowers to a `RecordGet` read, chaining through nested
    struct-typed fields. `p.field = expr`/`p.field op= expr` lowers through
    the ratified take → `make_mut` → write-back RMW discipline, mirroring
    `lower_indexed_assignment`'s single-level (`n == 1`) fast path — a
    **chained** write (`p.a.b = v`) or a **mixed** chain (`arr[i].field = v`)
    is a real, non-suppressible `E074` diagnostic (T1e boundary), never a
    silent miscompile. `E072` (the old "reject every struct construct"
    backstop) is retired; `E073` is its narrower replacement (a construction
    literal naming an unresolved shape reaching LIR).
  - **Codegen** (brink-codegen-inkb): emits the `StructShapes` table (shape
    ids interned deterministically, declaration order — never `HashMap`
    iteration); field ops default to the by-name `RecordGetDyn`/`RecordSetDyn`
    forms. Under `types = strict`, when a field access's record shape is
    provably known at compile time (a `VAR`/`temp` carrying a TM-2
    struct-typed annotation, or a direct construction-literal chain — never
    general type inference, which `brink-ir` cannot depend on), it emits the
    static-offset `RecordGet`/`RecordSet` forms instead. `types = gradual`
    never emits the offset forms, even with an annotation present (the
    annotation is unenforced there, so trusting it would be unsound).
  - **Runtime** (brink-runtime/brink-format): materializes the reserved
    `RecordGet`/`RecordSet` (`0xD1`/`0xD2`) opcodes — flat bounds-checked
    offset into the record's own field vector, no shape re-check (that's the
    performance payoff over the by-name forms); out-of-range is a
    turn-terminating `RuntimeError::RecordFieldOffsetOutOfRange`, never
    UB/panic. Same COW (take → `make_mut` → write-back) discipline as
    `RecordSetDyn`.
  - **Gradual construction faults** (value-model-spec §11c): a construction
    literal missing a declared field or supplying an undeclared one compiles
    under `types = gradual` (strict already rejects it at `E069`/`E070`) to a
    deterministic runtime fault via a reserved sentinel `ShapeId`
    (`RuntimeError::InvalidShapeId`) — no new opcode needed.

  Wasm-observable surface: `Opcode::RecordGet`/`RecordSet` are real variants
  now (disassembler text in `brink-web::program_model` and the `.inkt`
  writer both cover them); struct declarations, construction literals, field
  reads, and single-level field writes all compile to real bytecode reachable
  through `@brink-lang/web`'s compile/run surface for the first time.

  Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
  `STRUCT`/`Name#{…}`/field-access grammar.

- 0308aec: TM-5 (#621, docs/typed-mode-spec.md §9 step 5): hover and inlay hints now
  surface _inferred_ types, not just declared/annotated ones, through the
  FG-narrowed per-def seam (`ProjectDb::inferred_signature`/`infer_body` —
  never the whole-project `type_inference()`).

  Hover: a `temp` or parameter with no annotation now shows its inferred
  type (`` `x: int` ``) instead of nothing; an unannotated knot/stitch
  header falls back to its inferred signature for any param/return position
  a TM-2 inline annotation or doc-tag doesn't already cover. A declared
  annotation (TM-2 `name: type`, or a `///` doc-tag/host-manifest type for
  externals) always wins over inference — the firewall rule — and an
  `Unknown`/unresolvable inferred type shows nothing rather than noise.

  Inlay hints: a new `InferredType` kind renders an inferred-type ghost
  label (`: int`) right after an unannotated `~ temp name = …` declaration;
  an explicit `: type` ascription suppresses it (already visible in the
  source). Exposed through `@brink-lang/web`'s `inlay_hints`/`hover` JSON as
  `"inferred_type"` and the existing hover content string respectively; the
  LSP maps it to the standard `TYPE` inlay-hint kind (previously every hint
  defaulted to `PARAMETER`).

  `brink_ide::hover::hover` and `brink_ide::inlay_hints::inlay_hints` both
  gained a `&ProjectDb` (plus `FileId` for `inlay_hints`) parameter to reach
  the per-def queries — an internal API change to `brink-ide`, `brink-lsp`,
  and `brink-web`'s wasm bridge, not a `.inkb`/runtime change. Boundary-
  annotation quick-fix is explicitly out of scope (#657, parked).

## 0.10.0

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
- 973858f: Add the HIR structural projection to the editor session (#454 phase 2):
  `getHirSpansDoc(doc)` returns nested semantic spans (kind, depth, resolved
  `def_id`/`target_id` identity) plus a per-line container stack for rails, via
  the new wasm `hir_spans_doc` export. New `HirSpan` / `HirLineContainer` /
  `HirProjection` types.
- 54c37df: Extend the HIR projection's coverage (#463): new span kinds
  `divert_stmt` (whole divert/tunnel/thread statements, distinct from the
  `divert` target reference inside them; suppressed for statements inside
  inline logic in choice text), `divert_terminal` (`-> END` / `-> DONE` — no
  longer unprojected, and never flagged unresolved), `logic` (assignments
  and returns), and `conditional` / `sequence` (whole-construct extents,
  non-container). Container extents now include gather/labeled-block labels,
  so labeled gather lines (`- (g)`, `- (g) text`, nested labeled blocks) are
  covered by their containers and render their rails. Multi-line
  non-container spans that straddle a fragment view's start are dropped from
  `getHirSpansDoc` instead of being clamped to the view's top-left.
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
- 6289b0e: Weave structure is now foldable (#476): choice branches fold from their
  choice line (full-branch extent) and gather continuations fold from their
  gather line, derived from the HIR projection's container extents. Choice
  folds were previously dead code (single-line CST ranges), so story weave
  never folded at all. Conditional/sequence folds are unchanged. Known
  limitation: an unlabeled gather whose own line is prose gets no fold yet
  (ptr-less line content; upstream lowering gap).

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
