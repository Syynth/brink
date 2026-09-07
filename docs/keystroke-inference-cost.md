# The keystroke cost of type inference — quadratic, root-caused, fixed

**Status:** fixed on #3585, **awaiting ruling.** Everything below is
measured on that branch; the numbers labelled *before* are `main` at
`c6b50034c`. The fix touches how `brink-db`'s per-def inference queries
depend on the segment road, which is the maintainer's call — see
"Design notes for the ruling".

**Measured by:** `crates/brink-web/src/editor/perf_probe.rs` (`#[ignore]`d;
`cargo test --release -p brink-web --lib -- --ignored --nocapture
--test-threads=1 perf_probe`), one callgrind pass, and — the instrument
that settled it — a per-query **execution counter** off salsa's
`WillExecute` event (`brink_db::set_execution_counting` /
`take_execution_counts`, off by default, one relaxed atomic load per
execution while off).

## The finding

A one-character insert at the **end** of a file — an edit that cannot change
a single call edge — re-ran whole-project type inference, and the cost was
**quadratic in that file's knot count**:

| knots in one file | before | after |
|------------------:|-------:|------:|
| 15 | 0.79 ms | 0.26 ms |
| 30 | 1.45 ms | 0.39 ms |
| 60 | 5.29 ms | 0.71 ms |
| 120 | 21.88 ms | 1.40 ms |
| 240 | 104.18 ms | 2.94 ms |
| 480 | **504.86 ms** | **5.80 ms** |

Before: doubling the knots quadrupled the cost. After: **0.012 ms per knot,
flat.**

## Why "the per-file firewall holds" was the wrong frame

The first reading noted that spreading the same 240 knots over more files
divided the cost — "the FG-2.1 per-file firewall holds; the cost within a
file is quadratic." True, and beside the point: ink's `INCLUDE` puts every
file into one namespace, so a file is not a semantic unit at all. The
firewall was a **salsa dependency boundary** — the per-def queries read
`lowered_query(file).hir`, the *assembled* whole-file HIR, so their memos
were keyed on the file. "Every knot is a virtual file" is what #3084's
segment road already built for *lowering*; the per-def inference family
simply did not consume it. The spec anticipated exactly this
(`per-knot-incremental-lowering-spec.md` §3 step 4: *"If resolve /
per-file-diagnostics turn out to own a large share… they decompose along
the same segments"*). This is that step.

Same 240 knots, one file versus sixteen, editing one file:

| files | knots/file | before | after |
|------:|-----------:|-------:|------:|
| 1 | 240 | 104.83 ms | 2.76 ms |
| 2 | 120 | 39.34 ms | 2.53 ms |
| 4 | 60 | 15.30 ms | 2.36 ms |
| 8 | 30 | 7.17 ms | 2.31 ms |
| 16 | 15 | 3.72 ms | 2.35 ms |

The file boundary no longer exists as a cost unit. Edit position does not
matter either (240 knots, one file: append 2.62 ms, mid-knot with every later
range shifting 2.63 ms; before: 111 / 129 ms).

## What the counter showed, and why reading the code did not

After one append edit to a 240-knot file, executions per query:

| query | before | after |
|---|------:|------:|
| `call_edges_query` | 240 | **1** |
| `def_body_query` | 240 | **1** |
| `referenced_globals_query` | 240 | **1** |
| `solve_scc_query` | 240 | **1** |
| `segment_lowered_query` | 1 | 1 |
| `file_segment_at_query` | — | 240 (a `Vec` index each; the O(n) that remains) |

The first per-segment prototype re-pointed the per-def queries at the
segment road and **did not move the number** — every per-def query still
executed 240 times. Nothing in the code reading explained it. The counter
did in one screen: `def_segment` reached its segment by indexing
`file_segments_query(file)`'s **whole `Vec`**, so every def depended on
every segment identity in the file, and re-minting the one edited segment
invalidated all 240 readers. A per-index accessor (`file_segment_at_query`)
makes a def depend on its **own** segment's identity only; an unchanged
segment backdates (content-seeded id) and every other def's memo validates
without executing. That one seam is the entire difference between the two
"after" columns and the "before" ones.

## The fix, in four pieces (`crates/internal/brink-db`)

1. **`file_def_segments_query(project, file)`** — `def id → segment index`,
   built from the range-stripped inference index and each segment's own
   lowered knots. Ids are name-hashed (`alloc_address(path)`), so a
   fragment yields the same ids as the assembled file.
2. **`file_segment_at_query(file, idx)`** — the seam above.
3. **`def_body_query` / `call_edges_query` / `referenced_globals_query`**
   read the def's own segment (`segment_lowered_query` → one-knot
   `HirFile`, segment-relative ranges) plus a per-segment resolve
   (`segment_resolutions`: `resolve_file` over the fragment's own
   manifest — same resolver, same index, same import scope; a strict slice
   of `resolve_query(file)`). No `collect_defs` on this road: the def is
   found by id in a one-knot fragment.
4. **`solve_scc_query`'s coordinate join.** The first prototype passed the
   five brink-ide `value_call_fix` tests *wrong*: it handed the solver
   segment-relative bodies but the whole-file, absolute-range resolutions,
   so every range lookup missed and `pong(n-1)` never resolved (the FG-2
   mutual-recursion test caught it). Now each SCC member's body clone and
   its per-segment resolutions are rebased by a **synthetic per-member
   stride** (`SCC_MEMBER_STRIDE`, disjoint 4 MiB windows) — no collision
   between members, and **no read of the segment's `offset`**, which would
   make every later SCC's solve shift-sensitive. Outputs are un-strided
   back to segment-relative, so the memo is offset-free; the real offset is
   added at the two consumer-facing seams, `infer_body_query` (per def —
   reads only its own segment's offset) and `type_inference_query` (the
   aggregate). `BodyTypes` has exactly two range-bearing fields
   (`value_calls[].range`, `array_remove_calls`); `DefBody` has exactly one
   consumer. This is the same deferred-rebase shape `projection_query`
   uses.

Everything else keeps the old road: native (`.brink`) files, the
root-content synthetic def, lambdas (not in `hir.knots`), any def the map
does not hold.

## On the real story

TheIntercept, 100 KB, 30 knots, one file — the user-facing bill:

| | before | after |
|---|------:|------:|
| `db.type_inference()` after one edit | 16.3 ms | **8.4 ms** |
| per keystroke, host write path, appending | 30.0 ms | **17.8 ms** |
| per keystroke, mid-document | 30.8 ms | 19.0 ms |
| `ide.inlayHints` per keystroke | 11.1 ms | **2.4 ms** |
| `inlayHints`, whole doc, after edit | 15.2 ms | 8.1 ms |
| `inlayHints`, 50-line viewport | 4.2 ms | 4.0 ms |

What remains is outside inference: `argumentWidgets` (~5 ms — the host
passes `(0, doc.length)`, a whole-document walk), and the ~4 ms
range-independent floor under hints, which is the `syntax_root` whole-file
parse (~3.7 ms) that `inlay_hints` and `argument_widgets` pull as "IDE
consumers that want the whole-file tree" — the next candidate for the
segment road. `codeActions` (16.6 ms, on demand, 76% a whole-document
reformat) is unchanged and separate.

## Gates

`cargo test --workspace --exclude bevy-brink --no-fail-fast`: 320 suites
green. `cargo clippy --workspace --all-targets -- -D warnings`, and with
`--all-features`: clean. `cargo fmt --all -- --check`: clean. Specifically
exercised: `db_memo_retention` (flat memo counts across remove/re-add
churn), `fg2_scc_dependency_edges` (mutual recursion converges; a new
call edge re-solves downstream), the brink-web acceptance gate, the oracle
ratchet, `tier1_native`, brink-ide's fixer obligations.

## Design notes for the ruling

- **This narrows FG-2.1 (#638) one level.** That ruling shrank
  `solve_scc`'s HIR input "to SCC-members' declaring files via per-def
  projection queries". The declaring unit is now the segment. The
  `call_graph_query` doc's guarantee — "an edit in file X only pays for
  X's own defs" — becomes "only for the edited knot's own defs".
- **`segment_resolutions` is deliberately not a tracked query.** A
  `(project, file, segment)` memo interns a fresh key per re-minted
  segment; at one key per edit its interned-key LRU turns over too slowly
  to plateau inside `db_memo_retention`'s two warmup cycles (2 → 4 over
  ten cycles while every neighbour stayed flat; salsa reuses a slot only
  when it is `LOW`-durability *and* a new intern lands on that ingredient).
  The three callers carry flat `DefKey` memos and one knot's resolve is
  cheap, so the dedup a memo would buy is not worth a key. Anyone adding a
  tracked-struct-keyed query with low traffic will meet the same test.
- **The stride saturates.** `SCC_MEMBER_STRIDE.saturating_mul(i + 1)` —
  an SCC with more than 1023 members in one file would collide. Real ink
  does not put a thousand mutually-recursive knots in one cycle, but a
  merge-quality version should size the stride from the members' maximum
  body span, or key resolutions per def instead of per file.
- **`type_inference_query` now reads every segment-road def's offset**, so
  it re-executes on every shift edit doing O(defs) range adds. That is
  inside the measured totals (~2.3–2.9 ms at 240 knots includes it). The
  alternative — diagnostics consumers going per-def through
  `infer_body_query` — would remove even that.
- **`file_def_segments_query` maps to a segment *index*.** Inserting a knot
  above others shifts later indices, so those defs re-execute once (cheap,
  and correct). Mapping to the `FileSegment` directly needs a
  `salsa::Update`-bearing value type; a tuple does not qualify.
- **The counter stays.** It is the instrument this class of bug needs and
  the code reading provably lacked; off by default, gated by one relaxed
  atomic load. It should be documented in `CLAUDE.md`'s perf section once
  ruled.

## Superseded: the costed options

The earlier revision of this document ranked (A) hoisting `collect_defs`'
per-def preamble, (B) dropping the `String` from the def-lookup key, and
(C) narrowing the firewall per-segment, recommending A first. That ranking
assumed the re-execution *count* was fixed and only the *cost per
execution* could be cut — a partial prototype of A gave 505 → 331 ms with
the quadratic surviving. C was the asymptotic fix, and with the seam it is
the whole fix. A and B are now cold-build costs only; still worth doing,
no longer on the keystroke path.

## The dead ends, kept

- Not the inlay-hint walk: three identical calls after one edit cost
  18.5 / 0.96 / 0.90 ms — 95% invalidated memo; the hints cost ~1 ms.
- Not the whole-file parse: ~3.7 ms, real, separate.
- Not `solve_scc_query`'s per-SCC linear component scan: indexing it
  changed nothing measurable.
- Not the eager `refresh_analysis` in the probe's original write path: the
  work moves to whoever pulls the lowering first; the total is the same.
