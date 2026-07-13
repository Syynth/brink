# Fine-grained salsa — proposal for the per-def query migration (#623)

Status: **draft for a design round** (deliverable of #623). Precedes TM-2
(#618, inline type syntax) deliberately, so TM-2/TM-3 wire their type-system
consumers against the per-def query shape rather than the whole-project one
TM-1 shipped. Written against: decision-log 2026-07-11 ("Phase-0 query
engine: salsa, adopted coarse-grained"), **2026-07-13 ("Salsa fine-grained
migration promoted: tentative destination → committed before the epic
closes")**, the #605 typed-mode rulings (2026-07-12), and the landed TM-1
substrate (PR #625 / #617). Companion to `docs/scripting-substrate-spec.md`
(phase-0 substrate) and `docs/typed-mode-spec.md` §2.

Framing per the 2026-07-13 ruling: TM-1's whole-project `type_inference_query`
was ratified as **explicit debt against #623**. Measurement governs sequencing
only (trigger: TM-1's compile-bench vs the #498 baseline; hard deadline:
before T2 effects). This round decides the target shape so TM-2/TM-3 build on
it instead of deepening the debt.

---

## 1. Current state — every salsa query in `brink-db` today

Salsa **0.27.2**, `default-features = false, features = ["macros"]` (root
`Cargo.toml`). Inventory is **off**: ingredients are registered explicitly in
`BrinkDatabase::default` (`queries.rs`) — a missing one panics on first use.
`brink-db` is the only crate that sees salsa; `ProjectDb` (`db.rs`) is the
path-keyed shell, re-exported through `brink-driver`.

**Inputs (layer 0)** — `queries.rs`:
- `SourceFile { file_id, path, text }` — `#[salsa::input]`; `text` is the only
  mutable input (editor overlay = `set_text`).
- `ProjectInput { files: Vec<SourceFile>, entry, analysis_options }` — the
  project-level input.
- `DefKey<'db> { def: DefinitionId }` — `#[salsa::interned]`, the key for the
  per-def-keyed queries (`signature_query`, `infer_body_query`).

**Derived queries** — all `#[salsa::tracked]`, granularity noted:

| Query | Key | Granularity | Cutoff / notes |
|---|---|---|---|
| `parse_query` | `SourceFile` | per-file | returns `Parse` by ref |
| `lowered_query` | `SourceFile` | per-file | `LoweredFile{hir,manifest,diagnostics}`, `PartialEq` |
| `suppressions_query` | `SourceFile` | per-file | — |
| `include_graph_query` | `ProjectInput` | **whole-project** | reads every file's `lowered.hir.includes` |
| `symbol_index_query` | `ProjectInput` | **whole-project** | merges all manifests; `(Arc<SymbolIndex>, diags)` |
| `resolution_index_query` | `ProjectInput` | **whole-project** | the **cutoff seam** — decls-only, ranges zeroed (#517) |
| `resolve_query` | `(ProjectInput, SourceFile)` | per-file | reads `resolution_index` + the file's own `manifest.locals` |
| `signature_query` | `(ProjectInput, DefKey)` | **per-def key, whole-project deps** | see below |
| `analysis_query` | `ProjectInput` | **whole-project** | the big coarse memo — see below |
| `type_inference_query` | `ProjectInput` | **whole-project** (TM-1, #625) | wraps `infer_project`; **lazy** — see below |
| `infer_body_query` | `(ProjectInput, DefKey)` | per-def **view** over the project memo | `Option<Arc<BodyTypes>>`, value-`Eq` backdates |
| `type_diagnostics_query` | `(ProjectInput, SourceFile)` | per-file | **empty stub** — shape reserved for TM-3 |
| `diagnostics_query` | `(ProjectInput, SourceFile)` | per-file | filters `analysis_query.diagnostics` by file |
| `lir_query` | `ProjectInput` | **whole-project**, `no_eq` | never backdates (`lir::Program` has no `PartialEq`) |
| `story_data_query` | `ProjectInput` | **whole-project** | backdates on `CompileProduct` `Eq` — the byte anchor |

**The TM-1 inference family, as landed (#625):**

- `type_inference_query` wraps `brink_analyzer::infer_project(files, index,
  resolutions)` (`infer/mod.rs`), reading `analysis_query` for the
  `(index, resolutions)` pair plus every file's `lowered_query`. It is
  **lazy by construction** — not read by `analysis_query`, `lir_query`,
  `diagnostics_query`, or `story_data_query`; today nothing calls it (pure
  substrate, no measurable delta on existing paths per the #625 report).
- Internally `infer_project` already does SCC batching: pass 1
  (`build_call_graph`) runs `infer_def_body` per def with empty signatures
  purely to collect call edges (`BodyResult.calls`);
  `graph::strongly_connected_components` (deliberately reachability-sets,
  `O(V·(V+E))`, not Tarjan — documented smallness argument in `graph.rs`) +
  `topo_order` (condensation + Kahn's algorithm) produce dependency-ordered
  batches; `solve_batches` fixpoints each batch, capped at
  `MAX_SCC_ITERATIONS = 8` (termination with partial `Unknown` is legal, not
  a hang). The firewall (`infer_body(A)` reads only `signature(B)`) holds at
  batch boundaries; within a batch, "signature" means the SCC's current
  fixpoint estimate.
- Output: `InferenceResult { signatures: BTreeMap<DefinitionId, InferredSig>,
  bodies: BTreeMap<DefinitionId, BodyTypes> }` — all `Eq`, **no ranges**
  (`InferredSig{params: Vec<Ty>, return_ty}`; `BodyTypes` adds name-keyed
  locals). Already the right cutoff shape (§3).
- `infer_body_query` and `type_diagnostics_query` are thin per-def/per-file
  **views** over the one project-wide memo (mirroring `signature_query` /
  `diagnostics_query`). Notably `Arc<T>: PartialEq` compares by value, so
  `infer_body_query` *does* backdate per def even though its producer
  recomputes wholesale — consumer-facing firebreaks partly exist already;
  the debt is the **producer's O(project) recompute**, plus its dependency
  on `analysis_query` (below). There is **no per-def inferred-signature
  query** — only the whole `type_inference()` result and `infer_body(def)`
  (`db.rs`); TM-2's firewall consumer will need one.

**Where the coarse memos actually are — concretely:**

- **`analysis_query`** is *the* whole-project memo. It composes the layer-2
  pieces then calls `brink_analyzer::finish_analysis`, which runs `validate`,
  `dialect_gate::check`, and the four `external_check` passes over **all
  files' HIR**. Its only cutoff is `AnalysisResult: PartialEq` — and that
  struct carries `diagnostics: Vec<Diagnostic>` (with ranges) and a
  `ResolutionMap`, so almost any edit changes it and it rarely backdates.
  `diagnostics_query`, `lir_query` — **and now `type_inference_query`** —
  depend on it, so once a TM-2 consumer exists, nearly every edit will re-run
  whole-project inference through this edge.
- **`signature_query` is per-def by key but coarse by dependency.** It builds
  `hir_refs` over **every** file and passes them to `brink_analyzer::signature`
  — which only reads the *declaring* file's HIR (`files.iter().find(id ==
  info.file)`). Salsa records a read-edge on every file's `lowered_query`
  anyway, so a body edit in **any** file re-runs **every** signature memo. This
  is a live over-coarsening, not a design necessity (§2, FG-1).
- **`lir_query`** is whole-project and `no_eq` — it never backdates; the
  firebreak is one stage later at `story_data_query` (StoryData `Eq`).
- **`resolution_index_query`** is the one worked-out fine-grained-*style*
  cutoff: the full `SymbolIndex` carries a `TextRange` per symbol, so it never
  backdates; the projection drops locals (`Param`/`Temp`) and zeroes ranges so
  whitespace/`~ temp` churn anywhere leaves every `resolve`/`signature` memo
  intact (#517). It is the template for every new cutoff projection below.

**Consumers:** `brink-compiler` (batch: pull `story_data`), `brink-lsp` /
`brink-ide` (per-edit reads of `diagnostics`/`resolve`/`hir`/`symbol_index`),
`brink-web` (wasm, single-threaded). Memory introspection (`memory.rs`, #529)
is behind `memory-introspection`.

---

## 2. Target shape

Principle: **push the per-def boundary from the query *key* into the query
*dependency edges*.** `signature_query`/`infer_body_query` already have the
right keys; the target gives them the right edges — and decomposes the landed
whole-project `infer_project` into per-SCC queries.

**Stays coarse deliberately:**
- `symbol_index_query` / `resolution_index_query` — a whole-project merge is
  correct (names are global) and cheap; `resolution_index`'s `Eq` already
  shields dependents. Splitting them buys nothing.
- `story_data_query` assembly stays whole-project as the final backdate anchor
  (§7 oracle neutrality depends on this single `Eq` firebreak).

**Becomes finer:**

1. **`signature(def)` — declaring-file dependency only (FG-1).** Change
   `signature_query` to read only the declaring file's `lowered_query` (found
   via `resolution_index`'s `SymbolInfo.file`) instead of all files' HIR. `Sig`
   already carries no ranges (`signature.rs`) so it backdates across
   body/whitespace edits. After FG-1 a body edit re-runs only the signatures
   *declared in that file*. This edge fix flows straight into inference too:
   `infer/mod.rs::collect_globals` reads globals through `signature()`.

2. **Decompose `infer_project` into per-SCC queries (FG-2).** TM-1's internal
   structure maps onto query boundaries almost one-to-one — this is a
   **refactor of landed, tested code, not invention**:

   | TM-1 today (`infer/`) | Target query | What moves |
   |---|---|---|
   | pass 1: `build_call_graph` (per-def `infer_def_body` with empty sigs, edges from `BodyResult.calls`) | `call_edges(def)` per-def; merged by a derived `call_graph()` | extract a lean edge-collection walk (or keep reusing `infer_def_body` and discard types, as today); the per-def key gives call-edge cutoff |
   | `strongly_connected_components` + `topo_order` (condensation, Kahn's) | `scc_membership()` → `(def → SccId, condensation DAG)` | the algorithms move **unchanged** into the query body; output gains a stable `SccId` (component's minimum `DefinitionId` — already the sort key in `graph.rs`) |
   | `solve_batches` loop over all batches | `solve_scc(SccId)` — one batch's bounded fixpoint | the per-batch body of `solve_batches` becomes `brink_analyzer::solve_batch(batch, deps_sigs)`; `solve_scc(S)` reads `solve_scc(T)` for each condensation predecessor `T` — **acyclic by construction** (the condensation is a DAG), so no salsa cycles |
   | `InferenceResult` assembly | `infer_body_query(def)` / new `inferred_signature(def)` read `solve_scc(scc_of(def))` | the per-def views keep their existing keys and `None` contracts; the whole-project `type_inference()` accessor becomes an aggregation (or is retired) |

   What **stays in `brink-analyzer`** as plain pure functions: `Ty`/`unify`
   (`ty.rs`), `infer_def_body`/`BodyCtx` (`body.rs`), the SCC/condensation
   algorithms (`graph.rs`), the single-batch fixpoint with its
   `MAX_SCC_ITERATIONS` cap. What **moves to `brink-db`**: only the *when* —
   query keys, edges, and `SccId` interning. This preserves the substrate
   spec §3 rule (stage crates own the how, brink-db owns the when) and every
   TM-1 test.

   Salsa 0.27 does have fixpoint cycle support (`cycle_fn`/`cycle_initial`),
   but **pre-computed SCC batching avoids cycles entirely** — no query calls
   itself transitively; the only iteration is Rust-level inside `solve_scc`,
   exactly where TM-1 already put it. Fork 1 asks whether to keep that or
   switch to native cycles.

3. **`inferred_signature(def)` — new per-def view (FG-2).** The
   `InferredSig` half of the result, missing from the landed API. This is the
   boundary TM-2 wires annotations into (annotation overrides/checks the
   inferred sig) and the one callers' `infer_body` reads — it must exist
   per-def *before* TM-2, which is why this round runs now.

4. **`type_diagnostics` — fill per-file, blast-radius per-SCC (TM-3).** The
   landed stub is keyed `(project, file)`, matching `diagnostics_query`'s
   aggregation shape. Keep the per-file key; the fine-grained win comes from
   its future body reading `solve_scc` results, so one def's error recompute
   touches one SCC, not the project.

5. **Tracked structs vs `Arc<plain>` — Fork 2.** The committed destination
   names "tracked structs". The cheap move is to keep returning `Arc<plain>`
   (`InferredSig`, `BodyTypes` — tight `Eq`, already landed) from
   per-def/per-SCC-keyed queries. A tracked-struct rework of HIR bodies
   (field-level tracking) is a separate, later, measured-need effort.

`signature(def)`/`infer_body(def)` from typed-mode §2 therefore map to:
`signature_query` (existing, de-coarsened in FG-1), `inferred_signature(def)`
(new view), and `infer_body_query` (existing key, re-pointed at
`solve_scc(scc_of(def))` instead of the whole-project memo).

---

## 3. Cutoff semantics — what gates backdating at each new boundary

Every new boundary must place a **range-free, span-free, ordering-stable**
`Eq` summary between an expensive producer and its consumers (the
`resolution_index` playbook).

- **`Sig` (`signature.rs`)** — already `PartialEq`/`Eq`, no ranges. Backdates
  across any edit that doesn't change name/kind/params/declared-or-inferred
  value type. After FG-1 this is the per-file firewall TM-2's annotations feed.
- **`InferredSig` (`infer/mod.rs`)** — already the right shape: positional
  `Vec<Ty>` + `return_ty`, derived `Eq`, no spans, no unification-var
  numbering (types are fully resolved `Ty` values). This is the load-bearing
  boundary: a body edit that leaves inferred types unchanged backdates
  `inferred_signature(def)`, and every caller's `infer_body` memo survives —
  exactly the "ripple contained by early cutoff" cost the #605 ruling
  accepted on the record. The migration must keep spans/diagnostics out of it.
- **`BodyTypes`** — name-keyed locals, no ranges; per-def value-`Eq` already
  backdates `infer_body_query` today (Arc value equality). Correct as-is.
- **`SccId` / `scc_membership()`** — `Eq` on the condensation. An intra-body
  edit that adds/removes no call edge leaves the partition identical; only
  call-graph topology changes ripple into re-solves of downstream SCCs. The
  per-def `call_edges(def)` cutoff is what makes this cheap.
- **`type_diagnostics(file)`** — carries ranges, so it does *not* backdate on
  formatting, but its blast radius is one file's SCC memberships.

**Where firebreaks matter most for TM-2/TM-3:**
- **TM-2** (annotation syntax): the annotation feeds `Sig`; `Sig`'s and
  `InferredSig`'s `Eq` must be insensitive to everything except typed content,
  or every edit under an annotated def re-checks its callers.
- **TM-3** (strict policy): `Unknown`-escape errors and the §4 lattice checks
  fill `type_diagnostics`. They must read per-SCC solves so a strict error in
  one knot does not re-run project-wide checking — otherwise strict mode makes
  the editor quadratic.
- **The `analysis_query` edge**: `type_inference_query` currently takes
  `(index, resolutions)` from `analysis_query`, inheriting its almost-never-
  backdates `PartialEq`. The decomposition must re-source these from
  `resolution_index_query` + per-file `resolve_query` (the same data, without
  riding the diagnostics-laden `AnalysisResult`) — otherwise per-SCC
  granularity is defeated one edge upstream.

---

## 4. Migration strategy — FORK (Fork 5)

- **Big-bang.** One epic PR train converts signatures + inference + analysis +
  LIR to per-def in lockstep. *Risk:* huge blast radius, hard to bisect, and it
  couples the inference decomposition to the (riskier) analysis decomposition.
  *Effort:* high, single long-lived branch fighting the merge train.
- **Query-family-by-family, inference-first (recommended).** Order by consumer
  pressure and blast radius:
  1. **Inference first** — landed (#625) but **consumer-free until TM-2
     ships**, so decomposing it now is a migration of tested code with zero
     regression surface and TM-1's full test suite as the equivalence gate.
     The window closes the moment TM-2 wires hover/annotations against the
     whole-project shape — the precise debt the 2026-07-13 ruling flagged.
  2. **Analysis decomposition** — split `finish_analysis` into per-file
     diagnostic contributors (Fork 3); higher risk (LIR consumes
     `analysis.index`/`resolutions`/`diagnostics`).
  3. **LIR/codegen per-container** — the existing slice-C (#460) work (Fork 4).
  *Risk:* per-family; each family is independently oracle-gated. *Effort:*
  spread across FG-1…FG-5, each a normal reviewed slice.

Sequencing note: the 2026-07-13 ruling makes TM-1's compile-bench report the
scheduling trigger. Because `type_inference_query` is lazy and unconsumed, it
cannot move warm ide-reanalyze yet — so measurement alone does not force
immediate scheduling. The reason to run FG-1/FG-2 *now* anyway is
shape-locking for TM-2, not perf.

Recommendation: **family-by-family, inference-first.**

---

## 5. Symbolic-ref codegen linking — the slice-C deferral (Fork 4)

Slice C (#460, `scripting-substrate-spec.md` §7) splits `lir_query` into
per-container chunks keyed by `DefinitionId`, assembled by `story_data_query`.
The **link phase** is: each per-container LIR memo emits a chunk holding
**symbolic** `DefinitionId` references (never resolved indices — matching the
runtime's existing symbolic-ref/linker model, decision-log 2026-03-01); the
assembly query collects the changed chunks and re-runs `brink_codegen_inkb`'s
symbol resolution over the full set. A one-knot edit re-emits one chunk; the
link pass is the only whole-project step, and its output still backdates on
`StoryData` `Eq`.

**Fork:** include per-container LIR + symbolic linking in this epic's *first*
slice, or defer. **Recommendation: defer** (FG-4, late). It is orthogonal to
the type-system per-def shape that TM-2/TM-3 need *now*: linking is a codegen
incrementality win (#460), not a type-checker unblocker. Front-loading it
couples this epic to codegen-chunk churn with no TM payoff.

---

## 6. Memory bounding

Per-def/per-SCC memos multiply ingredient row counts by live-def/SCC counts:
today `memory.rs` reports one `count` per *query*; after FG-2 the heavy tables
(`solve_scc`, `infer_body`, per-container `lir`) grow with definitions × edit
history of deleted defs.

Facts from the current code:
- `memory.rs` (#529) reports `count` per ingredient but **`heap_bytes` is
  always `None`** — no query specifies a `heap_size` estimator, and every
  layer-2/3 output is `Arc`-wrapped so `fields_bytes` is only pointer-sized.
- Salsa's LRU (the spec §8 follow-up, per `memory.rs` docs) trims **by count,
  not bytes** — and is not wired up yet.
- `#536` durable path→`FileId` identity keeps *per-file* memo counts flat
  across remove/re-add (asserted by `db_memo_retention.rs`). Interned
  `DefKey`s (and any future `SccId` interning) are never freed by salsa,
  exactly like `SourceFile`.

**Proposed policy:**
1. **Durable `DefKey`/`SccId` identity**, mirroring #536: content-hashed
   `DefinitionId`s already give stable keys for unchanged defs; SCC ids keyed
   on the component's minimum member (the existing `graph.rs` sort key) reuse
   rows across unrelated edits. Add/remove churn of the same named def must
   overwrite in place, not leak a memo column per deletion.
2. **Count-based LRU on the per-def heavy queries** (`solve_scc`,
   `infer_body`, per-container `lir`), capacity `k × live_def_count`
   (k ≈ 2–3, measured), following spec §8's "LRU on the heavy queries".
3. **Add `heap_size` estimators** to the `Arc`-wrapped heavy queries so the
   `memory-introspection` snapshot becomes byte-aware and the LRU cap can be
   tuned against real bytes rather than counts alone.
4. **Extend `db_memo_retention.rs`** to per-def churn (add/remove a knot →
   assert per-def/per-SCC memo counts return flat), the direct analogue of its
   existing per-file assertion.

---

## 7. Test / verification plan

- **Oracle gate on every slice.** Ratchet `RATCHET_EPISODE_COUNT` (5,577) must
  not move; corpus report identical. The inference family is advisory-only
  under gradual policy — `type_inference_query` is not read by `lir_query` or
  `story_data_query` — so this holds by construction until TM-3.
- **TM-1's own tests as the decomposition equivalence gate.** `infer/mod.rs`
  carries firewall, fixpoint-convergence, and determinism tests
  (`mutual_recursion_*`, `determinism_same_input_same_output`); `db.rs` has
  the end-to-end `type_inference_tests`. FG-2 must keep all of them green on
  the decomposed shape, plus a direct assertion that per-SCC-composed results
  equal a single `infer_project` call over the same inputs.
- **Extend `incremental_fuzz.rs` (§6.2) to every new per-def family** — the
  2026-07-13 ruling explicitly sweeps this in. Today it asserts incremental
  `story_data()` == fresh compile (byte-identical `.inkb` via `write_inkb`)
  after every seeded edit. Add per-query assertions: for a sample of defs
  after each edit, `inferred_signature(def)` / `infer_body(def)` /
  `signature(def)` on the long-lived db must equal the fresh-db value. The
  fuzz mutations already include `~ temp` and whitespace churn; extend with
  call-graph-altering edits (add/remove a call/divert-with-args) to exercise
  `scc_membership` cutoff.
- **`db_memo_retention.rs`** extended per §6.4.
- **`compile_bench.rs` (#498)** re-run before/after each slice: cold
  full-corpus, cold synthetic (50×20), warm one-line-edit recompile — the
  ruling's own scheduling trigger. FG-1/FG-2 must not regress cold; FG-2's
  win shows in warm once TM-2 gives inference a per-edit consumer.
- **Oracle-neutrality argument.** The migration is byte-identical because
  (a) inference is lazy and never reaches `lir`/`codegen` before TM-3;
  (b) `signature`/`analysis`/`lir` decompositions are refactors gated by the
  existing `query_equivalence` construction and the fuzz harness's
  `StoryData`-`Eq` + `.inkb` byte compare. Any per-def split that changed a
  byte would fail `incremental_fuzz` immediately.

---

## 8. Forks for ruling

1. **SCC solving shape — lift TM-1's batching into queries vs native salsa
   fixpoint cycles.** Options: (a) `call_edges(def)` + `scc_membership()` +
   `solve_scc(SccId)` — the condensation DAG makes query recursion acyclic,
   no salsa cycles anywhere; (b) native `cycle_fn`/`cycle_initial` fixpoint
   recovery, letting `infer_body(A) → inferred_signature(B) → infer_body(B)`
   cycle. **Recommend (a), strengthened by the landed code:** TM-1 already
   implements exactly this batching internally (`graph.rs` SCC + Kahn,
   `solve_batches` bounded fixpoint, `MAX_SCC_ITERATIONS=8`) — (a) is a
   refactor that keeps every algorithm and test; (b) discards working code
   for the repo's first cycle-API use, unproven in our wasm build, with
   convergence obligations we'd newly own.

2. **Tracked structs vs `Arc<plain>` for signatures/inferred sigs in FG-1/2.**
   Options: (a) per-def/per-SCC-keyed queries returning the landed
   `Arc<Sig>`/`Arc<InferredSig>`/`Arc<BodyTypes>` (tight `Eq`, corrected
   edges); (b) convert to salsa tracked structs now. **Recommend (a):** the
   corrected *edges* deliver the incrementality; tracked-struct HIR rework is
   a later, measured-need slice not gated on TM-2.

3. **Decompose `analysis_query` now vs keep it coarse until after FG-2.**
   Options: (a) split `finish_analysis` into per-file diagnostic contributors
   in this epic's first slices; (b) do FG-1 plus re-sourcing inference's
   inputs off `analysis_query` (§3, the inherited-cutoff edge) now, defer the
   full split to FG-3. **Recommend (b) then (a):** the input re-sourcing is
   small and necessary for FG-2 to pay off at all; the full split is
   higher-risk (LIR reads `analysis.index`/`resolutions`) and shouldn't block
   the inference family.

4. **Symbolic-ref codegen linking in the first slice vs deferred (slice C).**
   Options: (a) per-container LIR + link in FG's first slice; (b) defer to a
   late slice. **Recommend (b):** it's the #460 codegen-incrementality win,
   orthogonal to the type-system per-def shape TM-2/TM-3 need now.

5. **Migration order — big-bang vs family-by-family, inference-first.**
   **Recommend family-by-family, inference-first:** the inference family is
   landed but consumer-free until TM-2, so decomposing it is a
   zero-regression migration of tested code with TM-1's suite as the gate —
   and the whole-project shape was ratified as explicit debt against this
   epic. The window closes when TM-2 ships its first consumer.

---

## 9. Slice plan

Each slice is a single reviewed, oracle-gated, behavior-neutral PR train (the
spine-slice method):

- **FG-1 — de-coarsen `signature(def)` + re-source inference inputs.**
  `signature_query` reads only the declaring file's `lowered_query` (via
  `resolution_index`'s `SymbolInfo.file`); `type_inference_query` takes
  `(index, resolutions)` from `resolution_index_query` + per-file
  `resolve_query` instead of `analysis_query`. Adds `signature`-incremental
  assertions to `incremental_fuzz`. Oracle-neutral. Small.
- **FG-2 — decompose `infer_project` per-SCC.** `call_edges(def)`,
  `scc_membership()`, `solve_scc(SccId)` (Fork 1a), `inferred_signature(def)`
  (the missing per-def view), `infer_body_query` re-pointed at its SCC.
  Analyzer keeps `ty.rs`/`body.rs`/`graph.rs` and the single-batch fixpoint
  as pure functions; brink-db owns keys/edges. Gated by TM-1's test suite +
  a composed-equals-monolithic assertion + fuzz extension with
  call-graph-edge mutations. Oracle byte-identical. **Must land before TM-2.**
- **FG-3 — decompose `analysis_query`.** Split `finish_analysis`
  (validate/dialect_gate/external checks) into per-file diagnostic
  contributors behind an aggregating query, so a body edit re-runs only the
  edited file's contributors. Gated by the `query_equivalence` construction.
- **FG-4 — per-container LIR + symbolic-ref link (= slice C / #460).**
  `lir_query` splits into per-`DefinitionId` chunk memos; `story_data_query`
  runs the symbolic link/assembly. Measured against #498; the warm-recompile
  number this slice exists to crush.
- **FG-5 — memory bounding.** Durable `DefKey`/`SccId` identity (#536
  analogue), count-based LRU on the per-def heavy queries, `heap_size`
  estimators, and the `db_memo_retention` per-def churn assertion (§6).

TM-2 (#618) and TM-3 land on top of FG-1/FG-2's `Sig`/`InferredSig` per-def
boundaries — which is the whole reason this round runs before TM-2.
