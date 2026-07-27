# Compile-time profile findings (issue #460)

Issue #460 — "incremental codegen (LIR + codegen + cross-file analysis)" —
is explicitly conditional: the scripting-substrate spec's §6.3 and the F5
ruling both say invest **only if measurement shows recompile latency is a
real problem**, and #498 exists to produce the numbers. This is the
measurement, and its conclusion.

Instrument: `compile_bench`'s `projectdb_cold.*` / `projectdb_warm.*` rows
(`cargo run --release -p brink-test-harness --bin compile_bench`). They pull
the `ProjectDb` query graph in dependency order through public accessors —
each pull happens after its dependencies are already memoized, so each row
is that layer's **marginal** cost, not a cumulative total.

## The numbers

`compile_bench`'s synthetic studio-scale project: 51 files, 1,000 knots,
6,765 lines, 188 KB. Release build, medians as printed by the harness.
"Warm" is a one-line body edit to a single knot of a single file on a
long-lived db — the editor/`brink-lsp` shape.

| Stage | Cold (ms) | % cold | Warm (ms) | % warm |
|---|---:|---:|---:|---:|
| `resolutions_index` — parse + HIR + symbol index + resolve | 27.4 | 8.8% | 0.9 | 8.4% |
| `prelude_decls` — FG-4e globals/lists/externals/shapes | 0.9 | 0.3% | 0.0 | 0.2% |
| `knot_chunks` — FG-4d per-knot LIR chunk memos | 4.0 | 1.3% | 0.2 | 2.2% |
| `diagnostics` — analysis diagnostics + suppression partition | 1.0 | 0.3% | 0.2 | 2.0% |
| `link` — chunks → one `lir::Program` | 2.4 | 0.8% | 2.2 | 20.6% |
| `effect_inference` — T2-3 `effects(def)` fixpoint | 272.5 | 88.0% | 5.7 | 53.2% |
| `story_data` rest — codegen emit + effect-row assembly | 1.6 | 0.5% | 1.5 | 13.7% |
| **total** | **309.8** | | **10.7** | |

Cold rows are single-sample by construction (a db is cold exactly once);
warm rows are medians of 10.

## What this says about #460

**1. Cross-file analysis is already incremental.** 27.4 ms cold → 0.9 ms
warm is a 30× reuse factor. The FG-3 decomposition (#632) did the job
#460's "cross-file analysis" bullet described; nothing is left to do there.

**2. LIR lowering and codegen are not the bottleneck.** Everything #460's
title names — chunks + link + codegen emit — is **8.0 ms of a 309.8 ms cold
compile (2.6%)** and **3.9 ms of a 10.7 ms warm recompile (37%)**. The
remaining structural work #633 tracks (symbolic-ref codegen chunks so a
per-container artifact cache becomes possible) can only attack that slice.
Cold, its entire addressable surface is 2.6% of compile time. Warm, a
*perfect* result — link and codegen reduced to zero — would take the
recompile from 10.7 ms to 6.8 ms, which is not a latency a person can
perceive in an editor.

**3. The actual cost centre is T2-3 advisory effect inference.** The
`effects(def)` fixpoint (#860/#862 — rows the runtime does not read yet) is
**88% of cold compile time** and the largest single warm component. It is
also 3× the *entire* rest of the pipeline combined, cold. Any further
compile-latency investment should start here, not in LIR/codegen.

The diagnostic-heavy variant (`diag_cold.compile`, same project shape with
brink-extension content and annotations) lands at ~1,042 ms cold, which
does not change the ranking — it adds analysis cost on top of the same
effect-inference floor.

## Ruling implication

The per-container / symbolic-ref codegen chunk redesign (#633, the
slice-C deferral) is **not justified by measurement today**. It carries a
real correctness cost — container indices and global slots are positional,
assigned by DFS/declaration order in the linker, and the VM's hot path uses
`container_idx` — so it is an oracle-sensitive change to buy at most 2.6%
of cold compile. It stays deferred until either (a) effect inference is
addressed and the ranking changes, or (b) a project shape appears where the
link phase's whole-project cost actually dominates.

What *was* worth doing, and landed with this document, is the O(K × project
size) defect the profile exposed inside the existing per-knot layer: every
one of the K chunk memos rebuilt the whole knot-invariant lowering
environment (the flattened resolution lookup over every project resolution,
the reconstructed struct-shape tables, the `FileId`→path map). It is now
built once per project revision in `chunk_lowering_ctx_query` and shared:

| Row | Before | After |
|---|---:|---:|
| `projectdb_cold.knot_chunks` | 34.3 ms | 4.0 ms |
| `projectdb_warm.edited_file_knot_chunks` | 0.8 ms | 0.2 ms |
| `synthetic_cold.compile` (end-to-end) | 341 ms | 307 ms |

Byte-identical output is the non-negotiable bar (CLAUDE.md determinism
rule): `crates/internal/brink-db/tests/issue_460_shared_chunk_ctx.rs` pins
cold-vs-warm `.inkb` identity across an edit sequence, and
`incremental_fuzz.rs` is the broad randomized version of the same contract.
