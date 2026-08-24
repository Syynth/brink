# Per-knot incremental lowering — spec draft

Status: **RULED 2026-08-24** (§5's three questions answered — see the
decision log entry "Per-knot incremental lowering: keys, frontend order,
sequencing"); implementation may begin at the §1 measurement gate.
Follow-up to option A (decision log 2026-08-24); the measured motivation
lives in `docs/desktop-perf-baseline.md` §"Option A delta".

## 1. Motivation, with numbers

After option A, one keystroke in a 5,863-line file costs **~30 ms**
(native and wasm alike), and the salsa event probe shows why with no
ambiguity: incrementality at the FILE granularity is already perfect —
each query executes exactly once, and the once is the problem. The edited
file's entire pipeline re-runs per keystroke: whole-file parse →
whole-file HIR lower → whole-file resolve → whole-file per-file
diagnostics → index rebuild over its whole manifest. A 20-knot file pays
~1 ms; a 900-knot file pays ~30 ms; the cost is linear in file size and
lands on every keystroke.

The 8 ms frame budget (ruled 2026-08-24) cannot be met while any
whole-file pass sits on the keystroke path. The two ways out are taking
analysis off the synchronous keystroke (the async-architecture phase) or
making the file-granular passes knot-granular — this spec is the latter,
and the two compose: even async analysis wants per-knot cost so results
stay fresh.

**Measurement gate (do this FIRST, before any design ruling):** extend
`ide_bench` (or a salsa-event probe row) to split the ~30 ms across
parse / raw-lower / claim-injected lower / resolve / per-file-diagnostics
/ symbol-index rebuild / bundle assembly on the large fixture. The
phase split decides how far §3's scope must reach — committing to scope
before that measurement repeats the #3063 half-hypothesis (the clone was
the vehicle, not the cargo).

## 2. Precedents

- **In-repo:** FG-4d's `knot_chunks` — per-knot **LIR** chunk memos with a
  shared `chunk_lowering_ctx_query` (the O(K × project) defect and its
  fix are written up in `docs/compile-time-profile-findings.md`). The
  HIR side has no equivalent: `raw_lowered_query`/`lowered_query` are
  per-file.
- **External:** rust-analyzer's item-tree / body-query firewall — edits
  inside one item's body invalidate only that body's queries; the item
  *tree* (names, signatures) changes only when declarations do. That is
  exactly the knot/declaration split ink and `.brink` both have.

## 3. Design sketch

The firewall shape, adapted:

1. **Segmentation.** A cheap `file_segments_query(file)` splits source
   into a file-header segment (VAR/CONST/INCLUDE/`use`/module directives)
   plus one segment per knot/flow/fn. Segment boundaries come from the
   LEXER, not regex — ink knots are line-anchored (`=== name ===`), but
   native blocks nest braces, and stray `===` inside strings must not
   split. Output: `Vec<Segment { kind, byte_range, content_hash }>`.
2. **Per-segment lowering.** `segment_lowered_query(file, segment_key)`
   parses and lowers ONE segment, emitting HIR + manifest contribution +
   diagnostics with **segment-relative ranges**. Key by content hash (or
   an interned segment-content struct) so an edit that leaves a knot's
   bytes unchanged backdates even as its file offset shifts — the same
   reason `knot_chunks` went symbolic-ref-shaped. The conventions
   claim-handler decls (`external_claim_handlers_query`) join the key:
   claiming reaches inside lowering (#2289), so a conventions change must
   invalidate every segment memo, exactly as it invalidates
   `lowered_query` today.
3. **Assembly.** `lowered_query` becomes an assembler: concatenate
   segment HIR/manifests, rebasing ranges by each segment's current file
   offset (a per-item add, not a re-lower). One keystroke inside knot K
   re-lowers K and re-runs assembly (O(file) but shallow — pointer/range
   arithmetic, no parsing); every other segment's memo backdates.
4. **Downstream, as the measurement demands.** If resolve /
   per-file-diagnostics turn out to own a large share of the 30 ms, they
   decompose along the same segments (both already iterate knot-shaped
   units internally). If parse+lower dominate, v1 stops at step 3.

**Alternative considered — rowan incremental reparse** (green-tree node
reuse on edit): solves only the parse share, leaves lowering and every
downstream pass whole-file, and both frontends would need edit-range
plumbing through the wasm boundary. The firewall shape solves all layers
with one mechanism and has the in-repo LIR precedent. Rowan reuse can
still be layered inside step 2 later if segment-parse itself ever shows
up in a profile.

## 4. Correctness obligations

- **Byte-identical output.** Assembled `HirFile`/`SymbolManifest` must be
  byte-identical to today's whole-file lowering across the corpus — the
  same non-negotiable bar as #460's shared-ctx fix, pinned the same way
  (cold-vs-warm identity across an edit sequence + the incremental fuzz
  suite extended to knot-interior edits).
- **Cross-knot constructs.** Anything lowering resolves ACROSS knot
  boundaries today (gather chains falling through knot ends, weave scope,
  trailing-content attachment, doc-comment attachment to the next knot)
  must be enumerated and either carried in the file-header/assembly layer
  or force adjacent-segment coupling in the key. This enumeration is
  implementation step zero after the measurement gate.
- **Diagnostics ranges** rebase with their segment; a diagnostic's
  message must never bake in absolute positions.
- **Oracle ratchet + tier goldens are the behavioral floor**, as always.

## 5. Ruled (2026-08-24)

1. **Segment keys: salsa TRACKED STRUCTS**, minted by the segmentation
   query with identity seeded by content hash — survives offset shifts
   AND knot insertion/reorder, garbage-collects properly (no immortal
   interned memos; the MemberSet caveat deliberately avoided), and aligns
   with the salsa-native workspace epic's tracked-partition direction.
2. **Ink first.** The measured symptom is an ink project and the ink
   segmenter is line-anchored; native follows once the assembly/rebasing
   machinery is proven, with its lexer-driven brace-aware segmenter as
   its own step.
3. **Before the async phase.** Optimization-wave shaped (no API or
   threading change, byte-identical bar); async later inherits cheap
   analysis as freshness — the same "don't wall off slow code" logic as
   the wave-first ruling.

## 6. Success criteria

`ide_large.phase.db_analysis` ~30 ms → within ~2× of the small-file cost
(~1–3 ms); wasm `updateDocument` p95 on the perf fixture from ~35 ms to
<10 ms; judged by `ide_bench` + the `typing-burst` scenario vs the
recorded runs, per the standing perf-compare protocol. Zero movement on
the oracle ratchet, tier goldens, or the acceptance gate.
