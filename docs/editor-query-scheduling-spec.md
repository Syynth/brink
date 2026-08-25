# Per-segment IDE queries + editor scheduling (#3064)

Status: **RULED 2026-08-24** (decision log "Editor freshness model:
per-segment IDE queries, debounced project analysis"); implementation in
phases below, each landed and measured independently.

## 0. The freshness model (ruled)

| Surface | Freshness requirement | Mechanism |
|---|---|---|
| Text editing / cursor | synchronous | CM6 core (untouched) |
| Line classification (screenplay rendering) | fresh **within the knot being typed**, every keystroke | per-segment memoized query — in practice the whole file stays fresh, the edited knot is the only recompute |
| Semantic tokens | same | same |
| Folding ranges | same (falls out of the same substrate) | same |
| HIR overlay marks | same | same (per-segment projection) |
| Whole-project analysis (diagnostics, cross-file resolution refresh) | debounce-on-pause (~250–300 ms) | JS-side scheduling over a split update API |
| Compile | on save / player interaction (ruled this morning) | already specced |

## 1. Measured starting point (segment-road delta, typing-burst p50)

72 ms keydown-to-paint = `cm.elementType.computeLineInfos` 32.4
(`wasm.updateDocument` 9.7 + `wasm.getLineContextsDoc` 21.6 + ~1 JS) +
`cm.folding.computeRanges` 12.1 + `cm.hirOverlay.buildState` 10.7
(`getHirSpansDoc` 7.0) + `cm.highlight.decorations` 7.1
(`getSemanticTokensDoc` 5.4) + ~6 small passes. ~56 ms of it is wasm
whole-doc query walks; analysis itself is 3.9 ms native.

Recon facts the plan builds on (file:line anchors in #3064's thread):

- **Semantic tokens** (`brink-ide/src/semantic_tokens.rs`): stateless
  per-token classifier over the CST — trivially segmentable. Its two
  whole-file prologues are the real cost: `build_resolution_index`
  re-scans `analysis.resolutions` per call, and `LineIndex::new` rebuilds
  the whole line table per call.
- **Line contexts** (`brink-ide/src/line_context.rs`): composes fragment-
  safe facets. Dialect chains break on blank lines and structural lines,
  so no state crosses a knot header. The projection's identity ids are
  explicitly ignored — structural projection suffices.
- **Folding** (`brink-ide/src/folding.rs`): mostly knot-local with FOUR
  named cross-segment features — (a) a knot's decl-fold end is clamped
  before the NEXT knot's doc block (the segmenter's cut positions carry
  exactly this), (b) INCLUDE/IMPORT leading-run folds are header-segment
  facts, (c) doc-comment folds subtract decl-consumed lines (per-segment
  accumulable), (d) machinery/narrative run folds are a linear scan whose
  runs break on structural lines — knot headers are structural, so runs
  cannot cross segments (emergent today; the per-segment build makes it
  an enforced bound, equality-gated).
- **Projection** (`brink-ide/src/hir_projection.rs`): the visitor's
  handles are monotonic ids in document walk order — per-segment local
  handles + sequential renumbering at assembly reproduces them exactly.
  The `decl_ids`/`ref_targets` join reads `resolutions_index` (cheap,
  ~1 ms fresh after the segment road — NOT the diagnostics bundle).
  Today the session projection cache is wiped wholesale on every
  `update_source`, so every keystroke rebuilds every queried file's
  projection from scratch.
- **updateDocument** (`brink-web/editor/doc_handles.rs`): fused
  update+analyze; the split exists inside Rust (`update_source` /
  `refresh_analysis`) but is not JS-callable. No delta ingress — JS
  pushes full `doc.toString()`; two more per-keystroke `toString()`
  calls (folding.ts:45, highlight.ts:74) build strings their host
  callbacks then IGNORE.
- **Staleness dissolves**: because per-segment token/projection queries
  read `resolutions_index` (cheap) rather than the full analysis bundle,
  the debounce needs to cover only the diagnostics passes. No surface in
  the table above ever renders from stale positions.

## 2. Phases

**Phase A — free wins, no API change.**
A1. Delete the ignored `doc.toString()` arguments (folding.ts,
    highlight.ts) — two full-document serializations per keystroke for
    nothing.
A2. Memoize the per-file `LineIndex` in the wasm session keyed on
    document revision (three rebuilds per keystroke today).

**Phase B — per-segment queries (the substance).** Each step: salsa
query per segment over the #3084 substrate + assembly with
rebase/renumber + a corpus-equality gate against the whole-file
implementation (the segments gate pattern), then reroute the existing
`*Doc` wasm export — transparent to TS.
B1. `ResolvedDialect` becomes a db input (store the config; compile the
    regexes in a memoized `resolved_dialect_query` — the
    `AnalysisOptions` pattern).
B2. `segment_projection_query` + `assemble_projection` (handle
    renumbering, range rebase; identity join from `resolutions_index`).
    Retires the wipe-the-world session projection cache.
B3. `segment_line_contexts_query` (fragment trivia + segment projection
    + dialect) + line-number-rebased assembly.
B4. `segment_semantic_tokens_query` (fragment CST walk + a new memoized
    per-file resolution-kind map query) + assembly.
B5. `segment_folding_query` + assembly handling the four cross-segment
    features above.
B6. Reroute `getLineContextsDoc` / `getSemanticTokensDoc` /
    `getFoldingRangesDoc` / `getHirSpansDoc`; measure per pass
    (typing-burst + perf-compare).

**Phase C — update split + debounce (API + TS).**
C1. wasm: delta ingress `applyEdits(doc, edits)` (splice Rust-side;
    `update_source` only) + a separate exported analysis/diagnostics
    pull; `updateDocument` stays as the compat fusion.
C2. TS: the elementType push switches to the delta path; the
    diagnostics-bearing pull debounces on pause. CM ordering constraint
    preserved: the StateField still pushes before consumers query.
C3. Reassess: after B, per-keystroke recompute is memo-hit assembly
    (~1–2 ms per pass); JS-side async/lazy wiring (lazy folding, mapped
    decorations) only if the numbers still demand it.

## 3. Gates

Per-query corpus equality sweeps (tiers in CI, full corpus under the
sweep env), the acceptance gate, studio/editor vitest suites, and a
typing-burst + perf-compare run per landed phase. Success criterion:
keydown p50 under **16 ms** after B6, under **8 ms** (the ruled frame
budget) after C2 — judged on the same fixture and hardware as the
segment-road delta.
