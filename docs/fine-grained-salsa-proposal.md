# Fine-grained salsa — proposal for the per-def query migration (#623)

Status: **draft for a design round** (deliverable of #623). Precedes TM-2
(#618, inline type syntax) deliberately, so TM-2/TM-3 wire their type-system
consumers against the per-def query shape rather than a whole-project one.
Written against: decision-log 2026-07-11 ("Phase-0 query engine: salsa,
adopted coarse-grained"), 2026-07-11 ("Fine-grained salsa + IR rework is the
eventual destination", **STATUS: tentative** — this round exists to firm it
up), and the #605 typed-mode rulings (2026-07-12) whose `signature`/
`infer_body`/`type_diagnostics` queries this shapes. Companion to
`docs/scripting-substrate-spec.md` (phase-0 substrate) and
`docs/typed-mode-spec.md` §2.

> **Two grounding facts the reader must know before ruling** (details in §1):
> 1. The TM-1 `crates/internal/brink-analyzer/src/infer/` module **does not
>    exist in this checkout.** `signature.rs` (the phase-0 `Sig` stub) is the
>    only type-adjacent analyzer code. There is no `infer_project`, no SCC
>    solver, no `infer_body`. So this is not a migration of existing inference
>    code — it is a decision about the *shape inference is born in*.
> 2. The 2026-07-13 decision log carries only the STRUCT-syntax amendment. The
>    "fine-grained commitment" is issue #623 on GitHub; no ratified log entry
>    yet. The standing log entry (2026-07-11) is explicitly tentative.

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
  one already-per-def query (`signature_query`).

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
| `diagnostics_query` | `(ProjectInput, SourceFile)` | per-file | filters `analysis_query.diagnostics` by file |
| `lir_query` | `ProjectInput` | **whole-project**, `no_eq` | never backdates (`lir::Program` has no `PartialEq`) |
| `story_data_query` | `ProjectInput` | **whole-project** | backdates on `CompileProduct`/`StoryData` `Eq` — the byte anchor |

**Where the coarse memos actually are — concretely:**

- **`analysis_query`** is *the* whole-project memo. It composes the layer-2
  pieces then calls `brink_analyzer::finish_analysis`, which runs `validate`,
  `dialect_gate::check`, and the four `external_check` passes over **all
  files' HIR**. Its only cutoff is `AnalysisResult: PartialEq` — and that
  struct carries `diagnostics: Vec<Diagnostic>` (with ranges) and a
  `ResolutionMap`, so almost any edit changes it and it rarely backdates.
  Every downstream query (`diagnostics_query`, `lir_query`) depends on it.
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
*dependency edges*.** `signature_query` already has the right key; the target
gives it (and the new inference queries) the right edges.

**Stays coarse deliberately:**
- `symbol_index_query` / `resolution_index_query` — a whole-project merge is
  correct (names are global) and cheap; `resolution_index`'s `Eq` already
  shields dependents. Splitting them buys nothing.
- `story_data_query` assembly stays whole-project as the final backdate anchor
  (§7 oracle neutrality depends on this single `StoryData`-`Eq` firebreak).

**Becomes finer:**

1. **`signature(def)` — declaring-file dependency only (FG-1).** Change
   `signature_query` to read only the declaring file's `lowered_query` (found
   via `resolution_index`'s `SymbolInfo.file`) instead of all files' HIR. `Sig`
   already carries no ranges (`signature.rs`: name/kind/params/value_type/
   is_local) so it backdates across body/whitespace edits. After FG-1 a body
   edit re-runs only the signatures *declared in that file*. This is the
   firewall unit TM-2 feeds (annotation = firewall) and the precondition for
   inference reading `signature(callee)` cheaply.

2. **`infer_body(def)` — new per-def query (FG-2).** Per typed-mode §2:
   `infer_body(A)` reads **only** `signature(B)` for its callees, never their
   bodies (call-site inference forbidden). Produces the body's local typing +
   an `InferredSig` (frozen param/return types; effect row later). Advisory
   only under gradual policy — no codegen effect (§7).

3. **SCC solving as queries.** Inference is monomorphic HM per call-graph SCC
   (typed-mode §2). The tension: a salsa query is per-key, but an SCC solves a
   whole binding group mutually. Two shapes — **this is Fork 1**:
   - **(a) Pre-computed SCC batching (recommended).** A derived
     `call_graph()` (edges from `resolve`/HIR), a derived `scc_of(def) ->
     SccId` (Tarjan condensation, a pure function — mirrors the internal-SCC
     decomposition the task expected `infer_project` to already do, but lifted
     to a query key), and `solve_scc(SccId)` that runs the fixpoint **inside
     one query** in plain Rust over the group's members. `signature(def)` /
     `infer_body(def)` read `solve_scc(scc_of(def))`. **This avoids salsa
     cycles entirely** — no query calls itself transitively; the only
     recursion is Rust-level inside `solve_scc`. Determinism and wasm behavior
     are unchanged from every other query.
   - **(b) Native salsa fixpoint cycles.** Let `infer_body(A) → signature(B) →
     infer_body(B) → …` form a real salsa cycle and use salsa 0.27's fixpoint
     recovery (`cycle_fn`/`cycle_initial`). *Grep confirms zero uses of any
     cycle API in `brink-db` today* — this would be the first, unproven in our
     wasm build, with convergence/`Eq`-fixpoint obligations we'd own.
   Batching also gives the natural cutoff grain: `scc_of` (the condensation) is
   a small, high-`Eq`-cutoff derived query — an intra-body edit that doesn't
   change the call graph leaves every `SccId` stable.

4. **`type_diagnostics(def)` — per-def (FG-2).** Keyed per def so one def's
   error set doesn't invalidate another's; aggregated for display the way
   `diagnostics_query` already aggregates.

5. **Tracked structs vs `Arc<plain>` — Fork 2.** The tentative destination
   names "tracked structs". The cheap incremental move is to keep returning
   `Arc<Sig>` / `Arc<InferredSig>` (plain, tight `Eq`) from per-def-keyed
   queries — the coarse pattern, but with the corrected edges from FG-1/2. A
   full tracked-struct rework of HIR bodies (field-level dependency tracking)
   is a separate, later effort that should be *measured-need-driven*, not done
   speculatively in this epic.

`signature(def)`/`infer_body(def)` from typed-mode §2 therefore map to:
`signature_query` (existing, de-coarsened in FG-1) and a new `infer_body_query`
reading `solve_scc(scc_of(def))` (FG-2).

---

## 3. Cutoff semantics — what gates backdating at each new boundary

Every new boundary must place a **range-free, span-free, ordering-stable**
`Eq` summary between an expensive producer and its consumers (the
`resolution_index` playbook).

- **`Sig` (`signature.rs`)** — already `PartialEq`/`Eq`, no ranges. Backdates
  across any edit that doesn't change name/kind/params/declared-or-inferred
  value type. After FG-1 this is the per-file firewall TM-2's annotations feed.
- **`InferredSig(def)`** — the load-bearing new boundary. Its `Eq` **must
  exclude** source spans, diagnostic text, and internal unification-var
  numbering; **include only** frozen param types, return type, and (later) the
  effect row. This is exactly the "inferred signatures ripple to callers on
  body edits, salsa early-cutoff contains no-change" cost the #605 ruling put
  on the record: a body edit that leaves inferred types unchanged backdates
  `InferredSig`, and every caller's `infer_body` memo survives.
- **`SccId` / `scc_of(def)`** — `Eq` on the condensation. An intra-body edit
  that adds/removes no call edge leaves the partition identical; only
  call-graph topology changes ripple.
- **`type_diagnostics(def)`** — per-def `Vec<Diagnostic>`; carries ranges, so
  it does *not* backdate on formatting, but its blast radius is one def.

**Where firebreaks matter most for TM-2/TM-3:**
- **TM-2** (annotation syntax): the annotation *is* the `Sig` firewall. The
  `Sig` `Eq` must be insensitive to everything except the annotation's typed
  content, or every edit under an annotated def re-checks its callers.
- **TM-3** (strict policy): `Unknown`-escape errors and the §4 lattice checks
  live in `type_diagnostics(def)`. They must be per-def so a strict error in
  one knot does not re-run project-wide checking — otherwise strict mode makes
  the editor quadratic.

---

## 4. Migration strategy — FORK (Fork 5)

- **Big-bang.** One epic PR train converts signatures + inference + analysis +
  LIR to per-def in lockstep. *Risk:* huge blast radius, hard to bisect, and it
  couples the (unproven) SCC-query design to the (proven) analysis decomposition.
  *Effort:* high, single long-lived branch fighting the merge train.
- **Query-family-by-family, inference-first (recommended).** Order by consumer
  pressure and blast radius:
  1. **Inference first** — it has *no consumers yet* (TM-1 hasn't landed), so
     getting `signature`/`infer_body`/`solve_scc` fine-grained is pure upside
     with zero regression surface. This is also the reframing forced by §1:
     since `infer_project` doesn't exist, **do not build it whole-project and
     migrate later** — build it per-def from day one *as* TM-1.
  2. **Analysis decomposition** — split `finish_analysis` into per-file
     diagnostic contributors (Fork 3), higher risk (LIR consumes
     `analysis.index`/`resolutions`/`diagnostics`).
  3. **LIR/codegen per-container** — this is the existing slice-C (#460) work
     (Fork 4).
  *Risk:* per-family; each family is independently oracle-gated. *Effort:*
  spread across FG-1…FG-5, each a normal reviewed slice.

Recommendation: **family-by-family, inference-first.** The inference family is
consumer-free right now; that window closes the moment TM-2 ships.

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

Per-def memos multiply the ingredient row count by the live-def count: today
`memory.rs` reports one `count` per *query*; after FG-2 the heavy tables
(`infer_body`, `type_diagnostics`, per-container `lir`) are keyed by `DefKey`,
so their `count` grows with definitions × edit history of deleted defs.

Facts from the current code:
- `memory.rs` (#529) reports `count` per ingredient but **`heap_bytes` is
  always `None`** — no query specifies a `heap_size` estimator, and every
  layer-2/3 output is `Arc`-wrapped so `fields_bytes` is only pointer-sized.
- Salsa's LRU (the spec §8 follow-up, per `memory.rs` docs) trims **by count,
  not bytes** — and is not wired up yet.
- `#536` durable path→`FileId` identity keeps *per-file* memo counts flat
  across remove/re-add (asserted by `db_memo_retention.rs`). Interned
  `DefKey`s are never freed by salsa, exactly like `SourceFile`.

**Proposed policy:**
1. **Durable `DefKey` identity**, mirroring #536: a re-added definition
   (rename-and-restore, churn) must reuse its `DefinitionId`-keyed interned
   `DefKey` so per-def memos overwrite in place rather than leaking a column
   per deleted def. (Content-hashed `DefinitionId`s already give this for
   unchanged defs; the concern is add/remove churn of the *same* named def.)
2. **Count-based LRU on the per-def heavy queries** (`infer_body`,
   `type_diagnostics`, per-container `lir`), capacity `k × live_def_count`
   (k ≈ 2–3, measured), following spec §8's "LRU on the heavy queries".
3. **Add `heap_size` estimators** to the `Arc`-wrapped heavy queries so the
   `memory-introspection` snapshot becomes byte-aware, and the LRU cap can be
   tuned against real bytes rather than counts alone.
4. **Extend `db_memo_retention.rs`** to per-def churn (add/remove a knot →
   assert per-def memo counts return flat), the direct analogue of its
   existing per-file assertion.

---

## 7. Test / verification plan

- **Oracle gate on every slice.** Ratchet `RATCHET_EPISODE_COUNT` (5,577) must
  not move; corpus report identical. TM-1 inference is advisory-only under
  gradual policy — it produces *no* codegen effect — so this holds by
  construction until TM-3.
- **Extend `incremental_fuzz.rs` (§6.2) to every new per-def family.** Today it
  asserts incremental `story_data()` == fresh compile (and byte-identical
  `.inkb` via `write_inkb`) after every seeded edit. Add per-query assertions:
  for a random sample of defs after each edit, `infer_body(def)` /
  `signature(def)` / `type_diagnostics(def)` on the long-lived db must equal
  the fresh-db value. This is the only detector for a cutoff/purity bug in the
  new queries (salsa owns dependency tracking; *we* own query purity and `Eq`
  correctness). The fuzz mutations already include `~ temp` churn and
  whitespace churn — extend with call-graph-altering edits (add/remove a
  divert/call) to exercise `scc_of` cutoff.
- **`db_memo_retention.rs`** extended per §6.4.
- **`compile_bench.rs` (#498)** re-run before/after each slice: cold
  full-corpus, cold synthetic (50×20), warm one-line-edit recompile. FG-1/FG-2
  should not regress cold; FG-2's win shows in warm once inference exists.
- **Oracle-neutrality argument.** The migration is byte-identical because
  (a) inference is advisory and never reaches `lir`/`codegen` before TM-3;
  (b) `signature`/`analysis`/`lir` decompositions are refactors gated by the
  existing `query_equivalence` construction (`analysis_query` composes the same
  `finish_analysis` back half; the fuzz harness's `StoryData`-`Eq` and `.inkb`
  byte compare is the anchor). Any per-def split that changed a byte would fail
  `incremental_fuzz` immediately.

---

## 8. Forks for ruling

1. **SCC solving shape — pre-computed batching vs native salsa fixpoint.**
   Options: (a) `scc_of(def)` condensation + `solve_scc(SccId)` fixpoint inside
   one Rust query (no salsa cycle); (b) native `cycle_fn`/`cycle_initial`
   fixpoint recovery. **Recommend (a):** avoids salsa cycles entirely, keeps
   determinism/wasm behavior identical to every existing query, and gives a
   clean `SccId` cutoff grain; (b) would be the first cycle API use in the repo,
   unproven in our wasm build.

2. **Tracked structs vs `Arc<plain>` for signatures/inferred sigs in FG-1/2.**
   Options: (a) per-def-keyed queries returning `Arc<Sig>`/`Arc<InferredSig>`
   (coarse pattern, corrected edges); (b) convert to salsa tracked structs now.
   **Recommend (a):** the tentative "tracked structs" destination is
   measured-need-driven; the corrected *edges* deliver the incrementality,
   tracked-struct HIR rework is a later slice not gated on TM-2.

3. **Decompose `analysis_query` now vs keep it coarse until inference lands.**
   Options: (a) split `finish_analysis` into per-file diagnostic contributors
   in this epic; (b) do only the cheap `signature` all-files-HIR fix (FG-1) now,
   defer the full split. **Recommend (b) then (a) as FG-3:** FG-1 is a small,
   safe, high-value edge fix; the full analysis split is higher-risk (LIR reads
   `analysis.index`/`resolutions`) and shouldn't block the inference family.

4. **Symbolic-ref codegen linking in the first slice vs deferred (slice C).**
   Options: (a) per-container LIR + link in FG's first slice; (b) defer to a
   late slice. **Recommend (b):** it's the #460 codegen-incrementality win,
   orthogonal to the type-system per-def shape TM-2/TM-3 need now.

5. **Migration order — big-bang vs family-by-family, inference-first.**
   **Recommend family-by-family, inference-first:** inference is consumer-free
   until TM-2 ships; converting it per-def now is zero-regression upside, and
   §1 means we'd otherwise build `infer_project` whole-project and pay a second
   migration.

---

## 9. Slice plan

Each slice is a single reviewed, oracle-gated, behavior-neutral PR train (the
spine-slice method), sized like FG-1…FG-5:

- **FG-1 — de-coarsen `signature(def)`.** `signature_query` reads only the
  declaring file's `lowered_query` (via `resolution_index`'s `SymbolInfo.file`),
  not all files' HIR. Adds a `signature`-incremental assertion to
  `incremental_fuzz`. Oracle-neutral (signatures don't reach codegen). Small.
- **FG-2 — inference, born per-def (= TM-1 #625, reshaped).** `call_graph()`,
  `scc_of(def)`, `solve_scc(SccId)` (Fork 1a), `infer_body(def)`,
  `type_diagnostics(def)` — advisory-only, gradual policy, both dialects.
  Extends `incremental_fuzz` with per-def inference equality + call-graph-edge
  mutations. Oracle byte-identical.
- **FG-3 — decompose `analysis_query`.** Split `finish_analysis`
  (validate/dialect_gate/external checks) into per-file diagnostic contributors
  behind an aggregating query, so a body edit re-runs only the edited file's
  contributors. Gated by the existing `query_equivalence` construction.
- **FG-4 — per-container LIR + symbolic-ref link (= slice C / #460).**
  `lir_query` splits into per-`DefinitionId` chunk memos; `story_data_query`
  runs the symbolic link/assembly. Measured against #498; the warm-recompile
  number this slice exists to crush.
- **FG-5 — memory bounding.** Durable `DefKey` identity (#536 analogue),
  count-based LRU on the per-def heavy queries, `heap_size` estimators, and the
  `db_memo_retention` per-def churn assertion (§6).

TM-2 (#618) and TM-3 land on top of FG-1/FG-2's per-def `signature`/
`InferredSig` boundaries — which is the whole reason this round runs first.
