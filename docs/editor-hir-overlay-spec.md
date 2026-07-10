# Editor HIR Overlay Spec (Track A)

Status: **design approved, unimplemented** (2026-07-07). Decision log: "HIR span overlay in
editor + source-compatible debugger direction". Sibling: the deferred instruction-level
sourcemapping epic (#452), which this overlay is *not* blocked on.

## 1. Goal

Project the **HIR tree** onto source ranges and expose those spans to the CodeMirror editor,
so the editor can style, interact with, and be inspected against the *semantic structure* of
the story — not just syntactic token color.

Today's highlighter (`brink-ide/semantic_tokens.rs` → `brink-web` → `ink-editor/highlight.ts`)
is only syntax-deep plus name-resolution coloring on use-site identifiers. It emits one
flat CSS class per token. This overlay is an **additional, structural layer**: nested spans
carrying semantic identity and `data-*` attributes, plus per-line block membership for
margin rails.

This is deliberately the substrate the future debugger (#452) builds on; where the two meet
(container identity ≡ runtime container identity) is called out as a #452 concern, not a
Track A requirement.

## 1a. The projection is the canonical HIR structural model (not just an editor overlay)

The editor overlay is the *first* consumer, but the projection is bigger than it: it is the
**one canonical structural model** for the HIR, and per-line/per-span structural features across
the IDE are **views over it**, with trivia and dialect layered on *separately* rather than fused
into the walk. The architecture (decision log, 2026-07-07 "Layered HIR structural architecture"):

```
shared HIR visitor  (traversal primitive, #457)
  ├─ reference/decl COLLECTION → SymbolManifest        ┐ upstream (feeds analysis; #458,
  ├─ structural validation (validate: E029–E034)       ┘ a mis-placed visitor pass today)
  └─ resolve → SymbolIndex + ResolutionMap
        └─ PROJECTION (HIR + analysis → spans + per-line container/weave stack)   ← this spec
              └─ views:  editor overlay/rails · line_context.element/weave · folding scaffold/nature

  + trivia facet   (comments/block-comments — CST scan)   ┐ layered on the views,
  + dialect facet  (regex over source)                    ┘ never fused into the projection
```

Why this matters here: `line_context.rs`'s `WeaveElement`
(`ChoiceLine`/`ChoiceBody`/`GatherContinuation`/`ConditionalBranch`/`SequenceBranch`) *is*
the projection's per-line container stack (R7), and `folding`'s scaffold/nature classification is
another per-line structural view. Consequently the projection's per-line weave/element output
(§8, R7) is a **first-class deliverable**, sized to serve `line_context` and `folding`, not a
rails-only detail.

> **Status (#463, shipped):** this re-expression landed. `line_context` owns no HIR walk — it
> composes the trivia facet (`brink-ide::trivia`), a span-replay view over
> `project_hir_structural`, source-text patch passes for HIR under-coverage, and the dialect
> facet; `folding`'s scaffold marks read the projection's `Conditional`/`Sequence`
> construct-extent spans (its bespoke `ScaffoldMarker` walk is gone). The producer grew
> view-serving coverage for this: `sticky`/`weave_depth` on Choice/Gather containers,
> `DivertStmt`/`DivertTerminal`/`Logic` statement spans, choice-tag spans, construct extents
> (Body-gated), and label-inclusive gather extents. Behavior was pinned byte-identical by
> `brink-ide/tests/line_context_snapshots.rs` before the refactor.

## 2. Consumers (the projection is designed from these)

| Consumer | Needs | Served by |
|---|---|---|
| Structural styling (dim inactive branch, highlight enclosing knot, bracket-match choice↔gather) | kind + containment at a position | nested spans + per-line block stack |
| Interactions (hover card, click-to-navigate, highlight-all-refs) | resolved symbol identity | `def_id` on decls, `target_id` on refs; queryable StateField |
| Tooling / inspectability (tests, devtools, binder/story-graph overlays) | stable, queryable DOM markers | rendered `data-*` attributes |
| Rails (concentric block bars in both margins) | per-line nested container stack | derived from container spans (kind + depth + handle) |
| **`line_context` element/weave** (view, shipped in #463) | per-line structural element + `WeaveElement`-equivalent + depth | the per-line weave/element output (§8) — trivia (comments) + dialect layered on separately |
| **`folding` scaffold/nature** (view, shipped in #463) | per-line container kind for machinery/narrative runs + scaffold | same per-line output; no bespoke scaffold walk |

## 3. Requirements

- **R1 — Coverage.** Every HIR node with a real source range becomes a span: structural
  containers (knot, stitch, choice, gather/continuation, conditional branch, sequence branch,
  thread, tunnel), named entities (labels, params, var/const/list decls + members), references
  (divert targets, var refs, calls), content regions (text, interpolation, glue, spring), tags,
  includes. **Skip synthesized nodes** (`ptr: None`). Expression *leaves* (bare literals /
  operators) are out — no own-range; deferred (would need CST walking).
- **R2 — Per-span semantics.** Each span carries `range`, `kind`, `depth`, and identity:
  `def_id` (named decls), `target_id` (resolved reference → its definition), and a stable
  `handle` for synthetic containers (see §6).
- **R3 — Nesting.** Preserve the tree: emit overlapping spans reflecting HIR containment
  (knot ⊃ choice ⊃ branch ⊃ content ⊃ ref), not a flattened one-class-per-token overlay.
- **R4 — Coexistence.** The existing `tok-*` syntactic/name-resolution coloring stays; this
  overlay is an independent decoration layer, not a replacement. CodeMirror composes multiple
  decoration facets.
- **R5 — Robustness.** Analysis lags the doc by a keystroke and can fail to parse mid-edit.
  Keep last-good spans and remap them through the transaction's changes (CodeMirror
  `RangeSet.map`) rather than clearing on every transient parse error.
- **R6 — Multi-file ready.** Spans carry `FileId`. The editor is single-document for now;
  cross-file projection is deferred but the contract must not preclude it.
- **R7 — Block rails.** Per-line block membership must support **nested concentric rails in
  both margins**, covering **every** container kind (knots/stitches, choice-blocks/gathers,
  conditional/sequence branches, threads/tunnels). The per-line container *stack*
  (outermost→innermost, each with kind + handle + depth) is derived on the frontend from the
  container spans; no separate per-line payload is emitted.

## 4. Architecture (mirrors the existing token pipeline)

```
HIR tree (retained per-file in ProjectDb; reachable via IdeSession::hir)
   + analysis SymbolIndex (named DefinitionIds) + ResolutionMap (resolved targets)
 → [brink-ide]  project_hir(hir, source, analysis, file) -> Vec<ProjectedSpan>
 → [brink-web]  hir_spans_doc(id) -> JSON (absolute line/UTF-16 positions)
 → [packages/wasm]  getHirSpans(id) -> HirSpan[]
 → [ink-editor]  StateField (canonical) -> inline marks + line decorations + gutter
```

### 4.1 Producer — `brink-ide`, new `hir_projection` module

New `crates/internal/brink-ide/src/hir_projection.rs`, exported from `lib.rs`. Signature
parallels `semantic_tokens`:

```rust
pub fn project_hir(
    hir: &HirFile,
    source: &str,
    analysis: &AnalysisResult,
    file: FileId,
) -> Vec<ProjectedSpan>;
```

Model the recursive walk on `story_graph::Builder::walk_file / walk_block / walk_content`
(`brink-ide/src/story_graph.rs`) — it already visits the HIR tree emitting `(node, range,
kind, file)` and cross-references `analysis`. Convert each node's `TextRange` with `LineIndex`
(`brink-ide/src/line_index.rs`: `line_col(offset) -> (line, utf16_col)`), exactly as
`semantic_tokens::push_token` does. Pull `def_id` from `analysis.index` (`by_name`/`symbols`)
and `target_id` from `analysis.resolutions` (`Vec<ResolvedRef { file, range, target }>`,
`brink-ir/src/symbols/index.rs`).

### 4.2 Bridge — `brink-web`

New `EditorSession::project_hir_impl(&self, path, view)` mirroring `semantic_tokens_impl`
(`brink-web/src/lib.rs`), reading `self.session.hir(file_id)`, `.source(file_id)`,
`.analysis()`. Exported as `hir_spans_doc(id)`. Returns **absolute** positions (not delta-encoded),
matching the `TokenJs` convention already used by `semantic_tokens_doc`.

### 4.3 TS bridge — `packages/wasm` + `packages/wasm-types`

`getHirSpans(id): HirSpan[]` (JSON.parse), `HirSpan` typed in `wasm-types`.

### 4.4 Editor — `ink-editor`

A **`StateField`** is the source of truth: it holds the parsed `HirSpan[]` (queryable by
interactions) and derives the decoration sets. Recomputed on doc change; on parse failure it
remaps last-good spans (R5). Two renderers consume it (see §5).

## 5. Rendering split (the rail requirement forces this — and it's the right shape)

Not everything is an inline mark. A knot spanning 200 lines as a character-range
`Decoration.mark` is wasteful and produces absurd nested DOM.

- **Inline `Decoration.mark`** → *fine-grained* spans only: names, references, content,
  interpolations, tags, small nodes. `class="hir-<kind>"` + `data-hir-kind` / `data-def-id` /
  `data-target-id` / `data-depth`. Nesting here is shallow and real.
- **`Decoration.line` + a custom `gutter`** → *block/container membership* and rails (R7).
  Per-line block attributes drive CSS rails; the gutter renders concentric margin bars. Line-
  granular (rails snap to whole lines), which is what a rail wants, and it avoids giant
  multi-line marks entirely. The frontend computes each line's container stack from the
  container spans covering it, ordered by `depth`.

Both renderers read the same StateField; interactions read the StateField regardless of which
renderer drew the visual.

### 5.1 Branch extent & edit-stability

A choice-branch rail must cover **the whole branch body**, not just the choice line. The HIR
already computes the branch boundary: weave-folding (`fold_weave`) folds the flat `*`/`-`
bullets into `Choice.body` + the gather `continuation`, so the CST `Choice` node covers only
the choice's own line(s), *not* the folded body. The branch's line extent is therefore the
**range-union of `Choice.body`'s node ranges** (min start .. max end over its statements,
skipping synthesized `ptr: None` nodes) — a min/max accumulated during the walk. We inherit the
boundary logic for free rather than re-deriving it.

This makes rails **stable while editing** without any persistent identity:
- Recomputed from the HIR on every successful parse → the rail always wraps the current branch,
  growing/shrinking with edits.
- On a transient mid-keystroke parse failure, last-good line decorations are remapped through
  the change (R5) → the rail follows the text until a fresh parse replaces it; no flicker.
- The range-derived handle's "changes when you edit above it" property is irrelevant here —
  nothing holds a handle across edits; rails are pure per-render output.
- Only rough edge: at a branch's exact boundary a remapped insertion is briefly ambiguous
  (belongs to the branch or after it?), resolved authoritatively by the next parse — at most a
  one-keystroke, imperceptible lag.

## 6. Identity model

- **Named symbols** (knot, stitch, var, const, list + members, external, param, label):
  real `DefinitionId` from the analysis `SymbolIndex` → `def_id`. References resolve to their
  definition via `ResolutionMap` → `target_id`. This powers go-to-def and highlight-all-refs.
  Available today with zero plumbing.
- **Synthetic containers** (choice, gather, conditional/sequence branch, thread, tunnel): the
  db HIR is **not** stamped with container `DefinitionId`s (stamping only happens in the
  codegen/LIR path, on a throwaway clone). Track A gives these a **range-derived `handle`**
  (e.g. the container's start offset), stable within a doc version — sufficient for rails,
  structural styling, and "highlight this whole block." **No stamping, no clone.**
- **B2 alignment (deferred, #452):** the debugger wants overlay container identity to *equal*
  runtime container identity. Reconciling the range-derived handle with codegen/runtime
  `DefinitionId`s (which depend on the normalized, stamped HIR) is tracked in #452, not here.
- **Explicitly not revisited now:** a structural/path-derived identity (path_hash-style) for
  synthetic containers is **deferred, not adopted**. It was a recurring source of pain during
  compiler development (synthetic-container addressing), and Track A gains nothing from it —
  rails and structural styling recompute per render and never need a persistent anchor. The
  range-derived handle is sufficient. The upgrade trigger is a *persistent* anchor on an
  anonymous block (edit-surviving breakpoint, comment pinned to a choice, or debugger
  "you are inside this gather"), all of which live in #452.

### 6.1 Why range-derived is the right call for A (the two identity needs)

The word "identity" hides two distinct needs, and synthetic containers force the choice:

- **Ephemeral render identity** — "in *this frame*, which spans are the same block?" Needed by
  rails, hover-highlight-block, structural styling. Recomputed every keystroke, never persisted.
  Range-derived serves this perfectly at zero cost.
- **Persistent / cross-layer identity** — "refer to *this* choice stably across edits **and**
  across overlay ↔ bytecode ↔ runtime." Needed by persistent breakpoints, pinned annotations,
  and the debugger correlating a runtime position to a source span.

Track A needs only the first; #452 needs the second. Named containers (knots/stitches) still
carry their real `DefinitionId` *in addition* to a range handle, so go-to-def / highlight-refs
are unaffected — the range-handle limitation touches only anonymous blocks, and only for
persistence/cross-layer, which nothing in A requires.

Implementation caveat: a bare start-offset can collide (a choice and its inner content block
may share a start byte), so the handle is the **full range** (or `(start, kind, depth)`), not
just the start offset, so "same block" comparisons don't false-match.

### 6.2 Attaching identity — the range-join (resolved)

Verified (2026-07-07): the analysis structures anchor their ranges to exactly the HIR node
ranges, so a **range-keyed join** is clean and needs no scope logic in the walk:

- `SymbolInfo.range == HIR Name.range`, verbatim, for every declaration kind (threaded
  lowering → manifest → `SymbolInfo` with no transformation). List items anchor to the whole
  `ListMember` node, which still equals `ListMember.name.range`.
- `ResolvedRef.range == HIR Path.range`, verbatim (both from the identical `ast::Path.text_range()`).

Mechanism — pre-build two inverse maps once (there is no `by_range`; `navigation.rs`'s proven
join is an O(symbols) linear scan, fine for a cursor but not per-node):
- `decl_ids: TextRange → (DefinitionId, kind)` from `index.symbols`
- `ref_targets: TextRange → DefinitionId` from `resolutions`

Then **route by node kind** during the walk: declaration nodes consult only `decl_ids`
(→ `def_id`); reference nodes (`Expr::Path` / `DivertTarget` / `Call` / list-literal items)
consult only `ref_targets` (→ `target_id`). A reference with no match **is** an unresolved
reference → mark it (`data-unresolved`, style as broken). We route by node, never by blind
position, so:

- The **def/ref precedence** problem `navigation.rs` must arbitrate (it starts from a raw offset)
  never arises — node kind selects the map.
- The **first-stitch auto-enter divert** edge case is inert: lowering synthesizes an
  `UnresolvedRef` whose range equals the stitch's own declaration identifier range (a collision
  in the flat resolution map), but that divert's HIR node is `ptr: None`, so the walk skips it —
  no phantom reference span is emitted at the declaration's range.

Coverage note: the file-level declaration vecs (`variables`, `constants`, `lists`, `externals`)
hang off `HirFile` **directly, not in the `Block` tree** — the walk (and the shared visitor,
#457) must iterate those flat vecs in addition to descending the block tree, or they are
silently omitted. Names/params/labels/temps/list-members live in the tree/structures and are
covered by the descent.

Frontend payoff — highlight-all-refs: given any span, key `K = def_id ?? target_id`; select every
span where `def_id == K || target_id == K`. Correct across shadowing for free (range-keying
already distinguished a shadowing temp from the global it shadows — distinct ranges, distinct ids).

## 7. Access model

Editor state is canonical (a queryable `StateField` / `RangeSet`); `data-*` attributes are
*rendered from it* for CSS hooks and external inspection. Interactions read state, not the DOM.
This best serves the interaction + debugger future while still giving tooling a queryable DOM.

## 8. Data contract (draft)

```ts
type HirSpan = {
  startLine: number; startChar: number;   // absolute, UTF-16
  endLine: number;   endChar: number;
  kind: HirSpanKind;                       // "knot" | "stitch" | "choice" | "gather"
                                           //  | "cond_branch" | "seq_branch" | "thread"
                                           //  | "tunnel" | "content" | "interpolation"
                                           //  | "glue" | "spring" | "tag" | "divert_target"
                                           //  | "var_ref" | "call" | "label" | "param"
                                           //  | "var_decl" | "const_decl" | "list_decl"
                                           //  | "list_member" | "include"
  container: boolean;                      // true → block renderer (line + gutter); false → inline mark
  depth: number;                           // nesting depth
  defId?: number;                          // named-symbol DefinitionId (decls)
  targetId?: number;                       // resolved target DefinitionId (references)
  handle: number;                          // stable-within-doc id (range-derived) for containers
  file: number;                            // FileId (R6)
};
```

## 9. Performance

Analysis already runs on every doc change (the token path depends on it); the HIR is already
retained. The projection is one additional tree walk (O(nodes)) reusing cached HIR + analysis.
The StateField holds full-doc span data (queryable everywhere for go-to-def); rendered
decorations can be narrowed to the viewport later if profiling warrants. No new debounce
expected — the projection is synchronous like the current token path.

## 10. Relationship to dialects

Custom **dialects** (`DialogueDialect` / `ResolvedDialect`, `brink-ir/src/dialect.rs`) are a
narrower thing than the name suggests, and the HIR overlay is **orthogonal** to them.

A dialect is a **line-level, regex-driven classification facet on narrative text lines** —
character cues, parentheticals, dialogue chains (the configurable `@Name:<>` screenplay
behavior). It adds **no** keywords, operators, statement types, or grammar constructs, and
changes **no** `SyntaxKind`, HIR node type, symbol kind, or resolution. It is **tooling-only,
query-time state** (registering one does not even re-analyze) and is absent from `brink-syntax`,
`brink-db`, and `brink-analyzer` entirely. Its only consumers are `line_contexts`
(`line_contexts_with_dialect` → `apply_dialect`, a source-text regex post-pass that attaches a
`DialectLineInfo` facet) and folding (which reads its `nature` field, #365).

Consequences for this overlay:

1. **`project_hir` is dialect-agnostic** — HIR structure is dialect-invariant, so the projection
   takes `(hir, source, analysis, file)` and **no `dialect` parameter** (mirrors
   `story_graph::Builder`). A custom dialect produces identical HIR nodes, symbol kinds, and
   resolutions; the projection never needs to know a dialect exists.
2. **They are complementary layers at different granularities.** Where the HIR overlay marks a
   dialogue line as generic **content**, the dialect facet subdivides *that same text* into
   cue / dialogue / parenthetical. The HIR has no notion of "this narrative line is a cue" — that
   lives only in `DialectLineInfo`. The two stack; they do not compete over the same
   classification.
3. **Three decoration layers now coexist** — existing `tok-*` (syntax + resolution), new `hir-*`
   (structural), and the dialect facet. CodeMirror composes them, but the studio needs a coherent
   visual story (z-order; don't let three layers style one span into mud). This is a UI-composition
   concern, not a data one.
4. **If dialect-aware sub-spans inside content are ever wanted** (e.g. `data-dialect-kind="cue"` on
   a content span), read the **already-computed `LineContext.dialect`** and layer it — do **not**
   make `project_hir` dialect-aware. This keeps the analysis-derived structural overlay and the
   query-time regex facet on their separate, correctly-cadenced data paths (dialect updates
   without reanalyze; HIR spans don't). Folding's consumption of dialect `nature` is the existing
   precedent for that pattern. This is an optional future nicety, not part of Track A.
5. **Rails are unaffected** — they follow HIR containers, which dialects never touch.

## 11. Non-goals

- Expression-leaf granularity (bare literals/operators) — deferred; no own-range.
- Instruction-level / bytecode provenance — that's #452 (B2), a `brink-format`-affecting effort.
- Cross-document (multi-file) projection in the editor UI — contract is FileId-ready, UI is single-doc.
- Replacing the existing `tok-*` semantic coloring.
- Making `project_hir` dialect-aware — dialect sub-classification stays a separate, line-contexts-sourced layer (§10).

## 12. Suggested phasing

1. **Producer + contract** — `project_hir` in brink-ide (walk + identity + LineIndex), unit-tested
   against fixtures. No frontend.
2. **Bridge** — `hir_spans_doc` in brink-web + `getHirSpans` + `wasm-types`.
3. **StateField + inline marks** — canonical model + `hir-<kind>` marks with `data-*`; CSS.
   Proves nesting + tooling/inspectability.
4. **Rails** — line decorations + gutter; concentric bars from the per-line container stack.
   Proves structural styling.
5. **Interactions** — hover card, click-to-navigate, highlight-all-refs off the StateField.
   Proves the resolved-identity payload.
