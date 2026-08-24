# Per-knot incremental lowering — spec draft

Status: **IMPLEMENTED for ink, 2026-08-24** (steps 1–3; v1 stopped at
step 3 per the measurement gate). The segmenter, tracked-struct
segments, per-segment lowering, and range-rebasing assembly are live —
`raw_lowered_query`'s ink arm rides the segment road; the retired
whole-file composition stays in-tree as the corpus-equality oracle.
Byte-identity bar met for HIR/manifest/admission with ONE declared
adjustment: the assembled diagnostics vector is multiset-identical in a
deterministic segment-major order (reproducing the whole-file road's
kind-grouped interleaving would thread per-kind streams through every
segment product for zero semantic value). One design addition forced by
the whole-file parse's trivia absorption: segments carry a
`lowered_range` that OVERLAPS the next segment's doc block (a knot's
node range absorbs trailing trivia up to the next header, so its
fragment must too). Native (`.brink`) remains whole-file — its
segmenter is the ruled follow-up.
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

**Gate result (2026-08-24, `ide_bench` staged-attribution rows —
`ide_large.stage.*`, 7 runs, idle machine, stages sum 29.4 ms vs the
unsplit 29.8 ms with a 0.0 ms control row):**

| pass | median | share |
|---|---|---|
| HIR-lower the edited file (parse warm) | **24.0 ms** | **~80%** |
| reparse the edited file | 3.8 ms | ~13% |
| cross-file analysis diagnostics | 0.5 ms | ~2% |
| symbol-index rebuild | 0.5 ms | ~2% |
| module map + resolve + per-file diags + resolutions bundle + assembly | ~0.6 ms | ~2% |

Verdict: **parse+lower own ~93% of the cost — §3 step 4 resolves to "v1
stops at step 3"** (segmentation + per-segment parse/lower + assembly);
resolve/diagnostics/index need no segment decomposition. Secondary
observation, worth a look during implementation but not a scope change:
HIR lowering costs ~6× the parse on the same bytes — if a constant-factor
inefficiency (allocation churn, quadratic append) hides in
`lowered_query`, fixing it is upside on top of, not instead of, the
firewall.

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
  boundaries today must be enumerated and either carried in the
  file-header/assembly layer or force adjacent-segment coupling in the
  key. **Done — see §4a.**
- **Diagnostics ranges** rebase with their segment; a diagnostic's
  message must never bake in absolute positions.
- **Oracle ratchet + tier goldens are the behavioral floor**, as always.


## 4a. Cross-knot construct enumeration (step zero — done 2026-08-24)

**Headline: the production ink path already lowers per-knot.** The db
road's `lower_file` (`brink-db/src/queries/mod.rs`) composes
`lower_single_knot` per knot — each with a **fresh** `LowerScope` and
`EffectSink` — plus `lower_top_level`, then assembles. Since today's
output is the correctness baseline, the byte-identity of that
composition *proves* knot lowering carries no cross-knot lowering state:
`LowerScope` is `{file_id, current_knot, current_stitch}` (no counters),
the sink is a plain `Vec<Diagnostic>`, and `DefinitionId` is a 56-bit
hash of the qualified *name* (`brink-format/src/id.rs`) — offset-free,
so no renumbering ever enters the picture. The spec's original worry
list (gather fall-through, weave scope, trailing-content attachment)
dissolves: those are knot-internal or symbolic at HIR level, and the
firewall's step 2 wraps *existing* entry points in per-segment memos
rather than teasing apart a whole-file recursion. What remains is the
enumeration below — boundary rules, hoisted collections, assembly
passes, and the parse seam.

| # | Construct | Today | Per-knot classification |
|---|---|---|---|
| 1 | Knot/stitch bodies, weave scope, gathers, labels, diverts | `lower_single_knot`, fresh scope; diverts stay symbolic names | **Segment-local** (proven by the production composition) |
| 2 | `///` doc blocks preceding a knot/stitch header | `collect_doc_lines` walks `prev_token()` **across the node boundary** (`lower/doc_comment.rs`) | **Segmentation boundary rule** — the one true adjacent coupling: a segment starts at the first line of the contiguous preceding `///` block (broken by a blank line, a plain `//`, or content — exactly `collect_doc_lines`' rules). Precedent: the ruled editor invariant that a doc block is structurally part of the declaration it precedes |
| 3 | Knot directives (`#@local` / `#@private` / `#@effects` / `#@was`) | Leading tag-line run **inside** `KNOT_BODY` (`leading_body_directives`) | Segment-local |
| 4 | Decl directives before `VAR`/`CONST`/`LIST`/`EXTERNAL` | `directives_before` backward walk (blank-line-tolerant) | Segment-local — a declaration and its directive lines always share a segment |
| 5 | `VAR`/`CONST`/`LIST` declarations | Whole-tree `descendants()` walk — global regardless of nesting, legal inside knot bodies | **Hoisted contributions**: each segment emits its own decls; assembly concatenates in document order (segment order ⊃ document order, so ordering-sensitive diagnostics stay byte-identical) |
| 6 | `STRUCT` / `EXTERNAL` / `INCLUDE` / `IMPORT` / top-level stitches | Direct-children scans — can only appear before the first knot header | **Header segment** (top-level stitches, promoted to knots today, get their own segments) |
| 7 | Root content before the first knot | `lower_weave_body(file.syntax())` | Header segment |
| 8 | Module identity (`#@module` / `#@was`) + file-wide visibility/was collections | Whole-tree walks (`directive.rs`) | Hoisted contributions + assembly merge; the `E049`/`E095` arbitration stays in assembly |
| 9 | `TODO:` author notes (`E189`) | Whole-tree walk already dedup-partitioned by `SkipInsideKnots` | Already segment-partitioned |
| 10 | Parse errors → `E037` | One whole-file `parse.errors()` vector | Per-segment errors, rebased at assembly |
| 11 | `project_manifest` (B0.4), `check_anonymous_stateful` (`E157`), `validate_admission` (+ `file_len`) | Whole-file passes over the assembled `HirFile` | **Stay in assembly**, O(file) but shallow; their true share of the 24 ms is currently masked by the double-lowering defect below |
| 12 | `Provenance { file, range }` on every HIR node; `Diagnostic.range`; `DocBlock` ranges | Absolute offsets from the whole-file tree | **The rebasing surface**: segment memos emit segment-relative ranges; assembly adds each segment's current offset (one add per range) |
| 13 | Suppressions / `allow_scopes` | Separate `raw`-road CST scan (`suppressions_query`) | Unchanged whole-file scan (cheap); decomposable later if it ever shows in a profile |

**The parse seam is where the byte-identity risk actually concentrates.**
Today one whole-file parse feeds every knot; per-segment parsing must
produce the same subtrees. Two verified lexer facts shape the segmenter:
multi-line `/* … */` block comments exist, and an unterminated `/*`
lexes to EOF as one token — so a `=== header ===` inside a comment (or
string) must not split, and an unterminated construct must swallow the
rest of the file in both worlds. Both hold **by construction** if
segmentation slices the SAME lexer's token stream (one whole-file lex
per edit — cheap, inside today's 3.8 ms parse share) at header-token
boundaries. Rowan recovery differences at segment edges are what the §4
cold-vs-warm identity gate and the knot-interior fuzz extension exist to
pin.

**Found in passing — the double-lowering defect (independent of this
spec, fix first):** `lower_file` builds its knots via `lower_single_knot`
and its top-level diagnostics via `lower_top_level`, then ALSO calls the
whole-file `lower()` just to harvest declarations — discarding that
call's knots, root content, manifest, and diagnostics. Net effect per
keystroke: every knot is lowered **twice**, every declaration twice, the
manifest projected twice, the author-warning walk run twice. A
restructured `lower_file` that computes declarations once (no full
`lower()` call) should cut a large slice of the 24 ms lower stage before
any segmentation work begins, and shrinks the firewall's remaining
target. Tracked as issue #3088; the byte-identity bar (goldens + ratchet)
applies to it the same way.

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

## 6. Success criteria — MET (measured 2026-08-24, wired)

`ide_bench` on the 5,863-line large fixture, idle machine, 7-run medians,
segment road live end-to-end (control row 0.0 — full coverage):

| row | morning baseline | post-#3088 | segment road |
|---|---|---|---|
| `ide_large.update_and_analyze` | 30.6 ms | 20.1 ms | **4.3 ms** |
| `ide_large.phase.db_analysis` | 29.8 ms | 19.2 ms | **3.9 ms** |
| `stage.0_segments` (scan + minting toll) | — | — | 0.7 ms |
| `stage.2_lower` (one knot's fragment parse+lower + assembly) | 24.0 ms | 14.0 ms | **1.9 ms** |
| whole-file parse | 3.8 ms (on path) | 3.8 ms (on path) | 3.8 ms — **off the analysis path** (IDE-only) |
| small-project keystroke | 1.0 ms | 1.0 ms | 0.7 ms |

The maintainer's memory-traffic concern (stated 2026-08-24) measured in
place: the segmentation toll is 0.7 ms against ~15 ms saved. The 2×
text-residency posture stays tentative pending the memory bench.

## 6a. Original success criteria

`ide_large.phase.db_analysis` ~30 ms → within ~2× of the small-file cost
(~1–3 ms); wasm `updateDocument` p95 on the perf fixture from ~35 ms to
<10 ms; judged by `ide_bench` + the `typing-burst` scenario vs the
recorded runs, per the standing perf-compare protocol. Zero movement on
the oracle ratchet, tier goldens, or the acceptance gate.
