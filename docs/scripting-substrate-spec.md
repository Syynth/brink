# Scripting substrate — the query-shaped compiler (phase 0 of #397)

Status: **draft for review** (deliverable of #499). Rulings this spec is
written against: decision-log 2026-07-10 ("Scripting-epic direction: #473
first, then query-substrate phase 0") and 2026-07-11 ("Phase-0 query
engine: salsa, adopted coarse-grained"). Architecture context: the
2026-07-10 comment on #397. Issues: #498 (benchmarks), #499 (this spec),
#500 (slice A), #501 (slice B), #460 (slice C).

## 1. Goal and non-goals

**Goal.** Restructure the compiler pipeline into memoized queries over a
salsa database so that (a) a second language frontend can join the same
query graph (the #397 growth path), (b) recompiles after an edit redo
only what the edit dirtied (#460), and (c) the IDE layer reads the same
queries instead of maintaining bespoke whole-project walks. Behavior is
**unchanged throughout phase 0** — every slice is oracle-gated (ratchet
5,577 must not move) and the corpus report must be identical.

**Non-goals of phase 0.** No language growth (that starts after slice C).
No type checking (`signature` lands as a stub). No IR rewrites — HIR,
LIR, and `StoryData` stay exactly the types they are. No runtime or
format changes of any kind.

## 2. The adoption pattern: coarse-grained salsa

Per the 2026-07-11 ruling:

- **Inputs** are file texts (plus the include-graph root/manifest). The
  editor's in-memory overlays become input overrides — the same
  mechanism, no separate overlay pathway.
- **Queries** are per-file/per-definition functions returning the
  **existing plain IR types behind `Arc`**. The IRs are *not* rewritten
  as salsa tracked structs; salsa's role is memoization + dependency
  tracking + early cutoff, mediated entirely at query boundaries.
  **Trajectory note (tentative ruling, 2026-07-11):** "IRs stay as-is"
  is a *phase-0 constraint, not a permanent principle* — the expected
  long-term direction is fine-grained integration (tracked structs,
  per-def/field-level granularity, IR rework where it pays) as live
  semantic tooling grows. Phase-0 query boundaries must therefore not
  foreclose finer granularity: prefer def-keyed queries where cheap,
  and keep per-def hashes in `hir`.
- **Early cutoff is the firewall.** Salsa backdates a dependent when a
  recomputed dependency is `Eq`-equal. The load-bearing instance: a body
  edit that doesn't change any *signature* re-lowers that body only —
  `signature(def)` recomputes equal, and every referencing body's memo
  survives. This subsumes the hand-designed signature-firewall from the
  earlier design round; our job reduces to making sure the query
  boundaries put cheap, `Eq`-comparable summaries (signatures, symbol
  tables) between expensive stages.
- **Purity discipline.** Every query must be a pure, deterministic
  function of its inputs — no clocks, no randomness, no interior
  mutation visible across calls. This is already house style
  (determinism rules, content-hashed `DefinitionId`s) and becomes a hard
  requirement. Any future compile-time evaluation of user code must
  satisfy it too.

## 3. Where the database lives: `brink-db`

`ProjectDb` (crates/internal/brink-db) is already the shared project
model for every long-lived consumer (`brink-ide`, `brink-lsp`,
`brink-web`) and the one-shot compiler. It is the natural owner of the
salsa database:

- `file_state` / the hand-rolled per-knot `knot_cache` dissolve into
  salsa inputs and the `parse`/`hir` queries (the knot cache's
  byte-offset invalidation heuristic is replaced by real dependency
  tracking).
- `ProjectDb`'s public API is preserved during phase 0 where practical —
  consumers migrate to query calls incrementally, not in one big bang.
- The salsa dependency is confined to `brink-db` plus query definitions;
  `brink-syntax`/`brink-ir`/`brink-analyzer` export plain functions that
  queries call. A crate that defines no queries does not see salsa.
- **All query APIs live in `brink-db`** — it is the one crate that knows
  salsa exists; stage crates own the *how*, brink-db owns the *when*.
  Consequence: brink-db moves up the dependency stack, gaining deps on
  `brink-analyzer` and `brink-codegen-inkb` (today it sees only
  syntax + ir), and every consumer — the `brink-compiler` driver, ide,
  lsp, web — sits above brink-db and stops hand-wiring pipeline stages.
  The wasm always-recompute fallback (§8) is likewise a brink-db feature
  flag, invisible to consumers.

## 4. Query inventory (phase 0)

Layer 0 — inputs:

| Query | Returns | Notes |
|---|---|---|
| `file_text(FileId)` | `Arc<str>` | set by CLI loader / editor overlay |
| `project_root()` | entry file + include resolution config | |

Layer 1 — per file:

| Query | Returns | Cutoff summary |
|---|---|---|
| `parse(FileId)` | `Arc<Parse>` (CST) | — |
| `hir(FileId)` | `Arc<HirFile>` + diagnostics | per-def hashes enable finer reuse later |
| `include_graph()` | file set, edges | small, high-cutoff |

Layer 2 — project-wide names (the symbol service, slice A):

| Query | Returns | Cutoff summary |
|---|---|---|
| `symbol_index()` | `Arc<SymbolIndex>` (decls only) | rebuilt cheaply; `Eq` cutoff shields dependents |
| `resolve(FileId)` | `Arc<ResolutionMap>` for that file's refs | depends on `symbol_index`, not other files' bodies |
| `signature(DefId)` | `Arc<Sig>` — **stub in phase 0**: name, kind, params, initializer-derived type (today's `infer_value_meta`), parsed-but-unchecked `#@` type annotations | the firewall unit for the future type checker |

Layer 3 — lowering/codegen (whole-project in slice B, split in slice C):

| Query | Returns | Notes |
|---|---|---|
| `lir(def or project)` | `Arc<lir::…>` | slice B: one project query; slice C: per-container |
| `story_data()` | `Arc<StoryData>` | batch compile = pull this one query |
| `diagnostics(FileId)` | `Arc<[Diagnostic]>` | aggregates hir/resolve/validate per file |

The analyzer dissolves along these lines: `resolve.rs` → layer-2
queries; `external_check::infer_value_meta` → `signature`;
`validate.rs` → per-file `diagnostics` contributors. `brink-analyzer`
as a crate may survive as the home of those functions — dissolution is
about the *pass structure*, not necessarily crate boundaries.

## 5. Consumers

- **`brink-compiler` (batch)**: `compile()` constructs a db, sets
  inputs, pulls `story_data()`. Cold cost must not regress beyond noise
  (#498 gate).
- **`brink-ide` / `brink-lsp` / `brink-web` sessions**: one db per
  session; edits are input writes; every feature (hover, rename
  analysis, line contexts, folding inputs) reads queries. Wasm is
  single-threaded — salsa's parallel/cancellation machinery is inert
  there, which is fine (the debounce model stays).
- **F5 speculation / fragment eval**: a fragment compile is a synthetic
  input file in a forked db; the unchanged project prefix of the query
  graph is reused, which is the cheap version of what the F5 ruling
  paid for with a bespoke cache.

## 6. Verification strategy

1. **Oracle gate** on every slice: ratchet 5,577 byte-identical, corpus
   report unchanged.
2. **Incremental == from-scratch**: a fuzz/property harness that applies
   random edit sequences to corpus projects and asserts the incremental
   db's `story_data()` is bit-identical to a fresh compile after every
   edit. This is the safety net for cutoff/purity mistakes (salsa owns
   dependency tracking, but *we* own query purity).
3. **#498 benchmarks** before slice A lands and after each slice: cold
   full-corpus compile (regression budget: single-digit %), warm
   one-line-edit recompile (the number slice C exists to crush), wasm
   bundle size delta for `brink-web` (measured, with a budget agreed at
   slice B review).

## 7. Slices (mapping to filed issues)

- **Slice A (#500)** — symbol service: extract `symbol_index` /
  `resolve` / `signature`-stub as functions with the query-shaped
  signatures above, still called eagerly by the existing driver. No
  salsa yet — this is the pure-refactor risk isolator.
- **Slice B (#501)** — the db: salsa into `brink-db`; layers 0–2 become
  real queries; `lir`/`story_data` wrapped as whole-project queries;
  knot_cache/file_state retired; consumers migrated. LIR
  logic/narrative module hygiene rides along.
- **Slice C (#460)** — split lowering/codegen per container behind
  `story_data()` assembly; measured against #498.

Each slice is a separate PR train, oracle-gated, with the fuzz harness
(§6.2) required from slice B on.

## 8. Risks and mitigations

- **Salsa API churn** — pin the version; upgrades are deliberate PRs.
  The coarse-grained pattern touches few files if signatures move.
- **Wasm size** — measured at slice B; if the delta is unacceptable,
  the fallback is feature-gating salsa out of the wasm build and running
  the db in always-recompute mode (queries still work, nothing is
  memoized) — the coarse pattern makes this a build-config decision,
  not a rewrite.
- **Memory growth of memo tables** — long editor sessions accumulate
  memos; use salsa LRU on the heavy queries (`parse`, `lir`) and
  measure in the session-length editor tests.
- **Purity leaks** — the §6.2 harness is the detector; house
  determinism rules are the prevention.

## 9. After phase 0 (non-normative)

The type checker arrives as layer-2/3 queries (`infer_body(def)`,
`type_diagnostics(def)`) with `signature` as its firewall — annotation =
firewall, absence = dynamic. A script frontend arrives as a new input
kind plus its own `parse`/`hir` queries joining the existing
`symbol_index`/`signature`/`lir` graph — cross-language incrementality
(edit a script body; ink callers recompute only if a signature changed)
falls out of the same cutoff mechanics. Neither requires revisiting this
spec's structure; both were design inputs to it.

Beyond that sits the tentative fine-grained destination (§2 trajectory
note): migrating hot queries to tracked structs and per-def granularity,
reworking the IRs where field-level dependencies pay for themselves.
That migration is expected to be incremental — query by query, driven by
measured needs of the live type checker — precisely because phase 0
keeps the boundaries def-shaped.
