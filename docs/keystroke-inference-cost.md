# The keystroke cost of type inference is quadratic in knots-per-file

**Status:** findings + options. **No ruling yet** — this document exists to
get one. Nothing here is implemented; the two prototypes described were
measured and reverted.

**Measured by:** `crates/brink-web/src/editor/perf_probe.rs`
(`#[ignore]`d; `cargo test --release -p brink-web --lib perf_probe --
--ignored --nocapture`), plus one callgrind pass over the 480-knot case.

## The finding

A **one-character insert at the end of a file** — an edit that cannot change
a single call edge — re-runs whole-project type inference, and the cost is
quadratic in the number of knots in that file.

One file, N knots, `db.type_inference()` priced after each edit (release,
native; a lower bound on wasm):

| knots | type_inference | per knot |
|------:|---------------:|---------:|
| 15 | 0.79 ms | 0.053 ms |
| 30 | 1.45 ms | 0.048 ms |
| 60 | 5.29 ms | 0.088 ms |
| 120 | 21.88 ms | 0.182 ms |
| 240 | 104.18 ms | 0.434 ms |
| 480 | **504.86 ms** | 1.052 ms |

From 60 knots up, doubling the knots roughly **quadruples** the cost. A
480-knot file is half a second of inference per keystroke.

For scale: `tests/tier3/misc/TheIntercept/story.ink` is 30 knots (60 with
stitches) in 100 KB, and measures ~16 ms — consistent with the curve once
its knots are larger than the synthetic ones.

## Why this is surprising

The per-knot incremental lowering road (#3084) works. `raw_lowered_query`
rides the segment road, `segment_lowered_query` parses only its own
segment's text, and a knot-interior edit re-lowers one knot. The
incrementality is real — it is simply defeated one layer up.

## Root cause

`brink_analyzer::infer::collect_defs` is the shared preamble of the whole
**per-def** query family. It is called once per def, from at least
`call_edges` (mod.rs:1279), `referenced_globals` (:1318) and `def_body`
(:1231) — and `call_graph_query` calls `call_edges_query` once for every
inferable def in the project.

Each call:

1. Iterates **every symbol in the project index**, building
   `def_of: BTreeMap<(FileId, SymbolKind, String), DefinitionId>` — cloning
   the name `String` for every symbol.
2. Walks the whole declaring file's HIR to collect its defs.
3. Linearly scans that vector to find the one def it was asked about.

So `n` defs × O(project symbols + file HIR) = the quadratic.

Callgrind over the 480-knot case, 58.9 G instructions total:

| share | symbol |
|------:|--------|
| 17.67% | `BTreeMap<(FileId, SymbolKind, String), DefinitionId>::insert` |
| 12.81% | `__memcmp_avx2_movbe` (String key comparison) |
| 11.24% | `brink_analyzer::infer::collect_defs` |
| 5.86% | `referenced_globals` |
| 5.86% | `call_edges` |
| 5.25% | `scc_graph` |

**`collect_defs` and the map it builds account for ~41.7% of all
instructions.**

## What the invalidation boundary actually is

The FG-2.1 per-file firewall **holds**. `call_edges_query` depends on
`lowered_query(file)`, so an edit to file X invalidates X's defs and no
others. Same 240 knots, spread over N files, editing one file:

| files | knots/file | type_inference |
|------:|-----------:|---------------:|
| 1 | 240 | 104.83 ms |
| 2 | 120 | 39.34 ms |
| 4 | 60 | 15.30 ms |
| 8 | 30 | 7.17 ms |
| 16 | 15 | 3.72 ms |

The firewall is per **file**, and the cost within a file is quadratic. That
is the whole problem in one sentence.

## Options

### A. Hoist `collect_defs` out of the per-def path

Make the file's def list (and its `def_of` map) a query computed **once per
file per revision** instead of once per def, consumed by `call_edges`,
`referenced_globals` and `def_body` alike.

- **Ceiling:** the 41.7% callgrind share, plus the `find` scans.
- **Measured (partial prototype):** hoisting it for `call_edges` only —
  leaving `referenced_globals` and `def_body` untouched — took the 480-knot
  case **505 ms → 331 ms (−35%)**. The quadratic survived, as expected:
  two of its three callers were still paying it.
- **Effort:** small. Pure factoring; the computation is unchanged, so
  correctness rests on existing tests rather than new semantics.
- **Risk:** low. No ruling needed — this is not a design change.
- **Caveat:** this alone probably does **not** make the curve linear. It
  removes a large constant and one of the quadratic's factors; the remaining
  per-def work still scales with file size. Re-measure before assuming.

### B. Drop the `String` from the def-lookup key

`def_of` keys on `(FileId, SymbolKind, String)` and clones the name for every
symbol on every build; `memcmp` alone is 12.81% of instructions. Key on an
interned id instead.

- **Ceiling:** most of the 12.81% memcmp share and a large share of the
  17.67% insert cost (allocation + comparison).
- **Effort:** small-to-moderate. Touches symbol identity, so it wants care.
- **Risk:** low-moderate. Behaviour-preserving if the interning is total.
- **Note:** largely subsumed by A if A eliminates most rebuilds — do A
  first, re-measure, then decide whether B still pays.

### C. Narrow the firewall from per-file to per-segment

Make the per-def queries depend on `segment_lowered_query` rather than
`lowered_query`, so a knot-interior edit invalidates that knot's defs only.

- **Ceiling:** O(defs in the edited knot) instead of O(defs in the file) —
  the asymptotic fix rather than a constant-factor one.
- **Effort:** large.
- **Risk:** high, and it needs a **ruling**. Cross-knot resolution reads the
  assembled file HIR today; def identity would have to be stable across
  re-segmentation (the tracked-struct identity model in
  `docs/per-knot-incremental-lowering-spec.md` §3 is the groundwork, but
  `call_edges` needs the *resolution* map too, which is file-scoped).
- **Recommendation:** do not start here. A and B are cheap and may make the
  curve acceptable; C is only justified if they do not.

### D. Do nothing; rely on authors splitting files

The firewall already rewards it — 240 knots over 16 files is 3.7 ms versus
104.8 ms in one. Stated for completeness, and rejected: file layout is an
authoring decision, and "your story is too long for one file" is not a
constraint this project should impose.

## Recommendation

**A, then re-measure, then decide between B and C.** A is a contained
refactor with a measured 41.7% target and no design question attached; the
partial prototype already shows −35% from a third of it. Whether the curve
is still quadratic afterwards is the fact that should choose between B (more
constant-factor work) and C (the architectural fix).

## Two things this is not

- **Not the inlay-hint walk.** That was the first suspect and it is wrong:
  three identical `inlayHints` calls after one edit cost 18.5 / 0.96 / 0.90 ms
  — 95% of the first call is invalidated memo. The hints cost ~1 ms.
- **Not the whole-file ink parse.** Real at ~3.7 ms on a 100 KB file, and
  worth fixing separately, but with `syntax_root`, `analysis()` and
  `host_values()` all pulled warm beforehand the inference cost is still
  there.
