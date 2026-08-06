# FG-2.1 — lazy per-reference `BodyCtx.globals` (design round, #638)

Status: **RULED — maintainer ruling round 2026-07-13** (deliverable of #638;
all four forks resolved, see §6). Ordered and ruled 2026-07-13. Follows FG-2 (#631 / PR #637, merged `5e0901a3`) which delivered
per-def/per-SCC query *structure* but explicitly fenced out dependency
*narrowing* ("Known limitation" section of #637). Written against the ratified
`docs/fine-grained-salsa-proposal.md`, the 2026-07-13 FG rulings
(decision-log: SCC batching / Arc<plain> / inference-first / codegen-link
deferred), the TM-2 rulings (#640, E063 opt-in — *no* warm-path inference
consumer landed yet), and the in-flight #626 / #627 (ruled) work.

Checkout verified fresh: `HEAD == origin/main == d7d5859f`, tree clean. Every
`file:line` below was read at that SHA.

---

## 1. Current state — the eager whole-project footprint

The inference family's per-def/per-SCC queries all gather inputs through one
helper:

- `inference_inputs(db, project)` — `crates/internal/brink-db/src/queries.rs:427-443`.
  Builds `hir_refs` over **every** `project.files(db)` (line 438-441), reads
  **every** file's `resolve_query` (434-437), and clones `inference_index_query`
  (432). Returned by value to `call_edges_query` (467), `call_graph_query`
  (482), and `solve_scc_query` (547). Because `hir_refs` spans all files, salsa
  records a read-edge on **every** file's `lowered_query` — so a body edit
  anywhere re-*executes* every per-def/per-SCC inference query. Value-`Eq`
  cutoff (`SolvedScc: Eq`, queries.rs:508) shields only *downstream* consumers.

Inside brink-analyzer the same whole-project read repeats one layer down:

- `collect_globals(files, index)` — `crates/internal/brink-analyzer/src/infer/mod.rs:152-166`.
  Iterates **all** `index.symbols`, and for each `Variable`/`Constant` calls
  `crate::signature::signature(id, index, files)` passing **all** `files`
  (line 159). It builds the eager `BTreeMap<DefinitionId, Ty>` that becomes
  `BodyCtx.globals`.
- `call_edges` (mod.rs:398-420) and `solve_scc` (mod.rs:440-468) each rebuild
  `collect_globals` + `collect_defs` over all files on every call
  (405-407 / 450-453).

### 1.1 Reference-class enumeration — what a body actually looks up

Walking `infer/body.rs`, every cross-def lookup a body performs, and its
source:

| Reference class in body | Where | Source today | Data needed | Per-def query that can already serve it |
|---|---|---|---|---|
| **Param / Temp** (this body's own locals) | `ty_of_def` body.rs:126-128; `observe` 150-167 | `self.locals` (in-body) + `index.symbols[def].kind/name` | in-body accumulator; symbol kind+name | none needed — never leaves the body; only reads `index` for kind/name |
| **VAR / CONST value type** | `ty_of_def` body.rs:129-131 | **`ctx.globals.get(&def)`** ← the villain | that global's declaration-derived `Ty` | **`signature_query(def).value_ty`** (FG-1, per-declaring-file, queries.rs:303-319; the field was `value_type` until issue #1540 widened it to full `Ty` fidelity) |
| **LIST decl** | `ty_of_def` body.rs:132 | `Ty::List(index.symbols[def].name)` | symbol name only | none — pure `index` read, no HIR, no globals |
| **LIST item** | `ty_of_def` body.rs:133-138; `infer_list_literal` 252-263 | `index.symbols[def].name` split on `.` | symbol name only | none — pure `index` read |
| **Knot / Stitch / External / Label as a *value*** | `ty_of_def` body.rs:139-141 | `Ty::Unknown` (T1c fence) | nothing | none — constant `Unknown` |
| **Callable signature** (call / divert-with-args) | `infer_call` body.rs:297-308; `infer_target` 375-391 | **`ctx.known_sigs.get(&def)`** | callee's current `InferredSig` | **NOT lazy — must stay in the fixpoint working set** (see §3) |
| **Call-edge membership** | `record_call_edge` body.rs:174-178 | `ctx.inferable.contains(&def)` | project-wide inferable id-set | index-derivable (`inferable_defs`, mod.rs:385-387) |

**The finding that scopes this whole round:** `BodyCtx.globals`
(body.rs:50) is read in **exactly one place** — `ty_of_def`'s
`Variable | Constant` arm (body.rs:129-131). It carries **only VAR/CONST
declaration-derived value types**. LIST/LIST-item come from `index`; callables
come from `known_sigs`; everything else is `Unknown`. So "reworking
`BodyCtx.globals` into lazy per-reference lookup" is precisely: **replace the
eager VAR/CONST map with per-reference `signature_query(var).value_ty`**, and
nothing else in the reference taxonomy is touched.

---

## 2. Target shape (5-8 sentences)

Replace `BodyCtx.globals: &BTreeMap<DefinitionId, Ty>` (body.rs:50) with a
lazy resolver the walk consults only when it hits a resolved VAR/CONST
reference, backed in brink-db by `signature_query(def)` — whose dependency
edge, post-FG-1, is the *declaring file's* `lowered_query` alone, not all
files. Because VAR/CONST value types are **declaration-derived** and orthogonal
to the SCC call graph, this lookup can never re-enter `solve_scc`, so the
condensation-DAG acyclicity argument is untouched and no new cycle surface
appears. The callable-signature path (`known_sigs`) is **deliberately left
eager and threaded through the fixpoint working set** — making *it* lazy is a
separate, cycle-dangerous change (§3) this round should not undertake. Pass 1
(`call_edges`) discards all types and needs neither globals nor `known_sigs`,
so its input can shrink to the declaring file's HIR + resolution + the
index-derived `inferable` set (§4). To make the narrowing actually pay off,
`solve_scc`'s HIR input must shrink from all-files to the SCC-members'
declaring files, which needs a small per-def HIR projection query (the
`inference_index_query` precedent) — RULED IN, see §6 Ruling 2 (full
narrowing). After the rework, a body edit re-executes only the
call_edges of defs in that file and the solve_scc of SCCs whose members (or
whose referenced globals) live in that file — the same class of win FG-1 gave
`signature_query`.

---

## 3. The cycle question — why lazy globals stays acyclic

SCCs are components of the **call graph over knots/stitches only**
(`inferable`, mod.rs:352). VAR/CONST symbols have no body, are never in
`inferable`, and are never call-graph nodes. A lazy global lookup therefore
resolves to `signature_query(var)`, which reads **only the declaring file's
HIR** (queries.rs:303-319, `signature.rs:82-194` — declaration-derived
`value_ty`, no body analysis). It never calls `solve_scc_query` /
`inferred_signature_query`. So:

- `solve_scc_query(S)` → (lazy) `signature_query(var)` → `lowered_query(file)`.
  No path back to any `solve_scc_query`. The condensation-DAG recursion
  (`solve_scc_query` on predecessors, queries.rs:540-545) is unchanged and
  still acyclic by construction (Fork 1 ruling).

The cycle risk lives **only** in the callable path. An in-SCC callee reference
must read the SCC's *current fixpoint estimate*, not
`inferred_signature_query(callee)` (which would call `solve_scc(same S)` →
salsa cycle). Today that estimate lives in the single `known_sigs` map inside
one `solve_scc` execution, seeded `Unknown` and updated per round
(mod.rs:267-299) — cross-SCC finalized sigs and in-SCC working sigs coexist in
one map, all Rust-level inside the query body. **FG-2.1 must not touch
`known_sigs`.** This is the load-bearing scope boundary: *globals become lazy;
callables stay in the working set.* Recording it explicitly is half the point
of the round.

---

## 4. The firewall question — declared/derived ≠ call-site inference

The 2026-07-12 firewall ruling forbids **call-site-driven inference**: a
caller's argument types flowing backward into a callee's params. `signature(B)`
for a callable reads B's own already-computed signature forward; it never reads
B's body from A (infer/mod.rs:1-45 doc; the
`signature_is_declaration_derived_only` guard test named at mod.rs:14-15).

Lazy globals cannot breach this because **VAR/CONST are not callables**. A
global's `value_ty` comes from its own initializer literal / TM-2 annotation
(`signature.rs:103-120`), never from any use site. `signature_query(var)`
returns that declaration-derived value unchanged whether read eagerly (today)
or lazily (proposed) — same bytes, same firewall unit. And because the
callable `known_sigs` path is untouched (§3), the param-inference firewall it
governs is definitionally unaffected. The issue's third open question resolves:
**lazy lookup of a declared/derived signature is not call-site inference.**

---

## 5. Pass 1 (`call_edges`) needs no types at all

`build_call_graph`/`call_edges` run `infer_def_body` with `known_sigs` **empty**
and keep only `.calls` (mod.rs:234-246, 417-419). `record_call_edge`
(body.rs:174-178) fires on any `resolve(path.range)` landing in `inferable` —
independent of every type computed during the walk. So the types pass 1
computes (including every `ctx.globals` read) are **pure waste**: `call_edges`
calls `collect_globals` over all files (mod.rs:405) only to feed a walk whose
output ignores it.

Edge discovery needs exactly: the declaring file's HIR (to get params+body),
that file's resolution map, and the `inferable` id-set (index-derivable). It
needs neither `globals` nor `known_sigs`. Dropping the `collect_globals` call
from `call_edges` and narrowing its HIR input to the declaring file is a strict
improvement that alone makes `call_edges_query` per-declaring-file. (This does
require a lean edge-only walk or a documented "globals resolver returns
`Unknown` in pass 1" — see Fork 1, since the shared `BodyCtx` carries the
resolver.)

---

## 6. Rulings (maintainer, 2026-07-13)

### Ruling 1 — pre-scan + narrow map, specified as a reusable per-def body-facts projection

`infer_def_body`/`BodyCtx` are pure brink-analyzer code that must not know
about salsa. Options for wiring the per-reference lookup:

- **(a) Resolver hook.** `BodyCtx.globals: &dyn Fn(DefinitionId) -> Ty` (or a
  one-method trait). brink-db's impl calls `signature_query`; brink-analyzer's
  own tests pass a closure over a map. The body walk calls the resolver
  on-demand when it hits a VAR/CONST, so salsa records the narrow edge as a
  side effect of the walk (standard query-within-query).
  - *Tradeoff:* true laziness, minimal new code, but puts a salsa re-entrant
    callback inside the hot body-walk loop, and brink-analyzer's `BodyCtx`
    grows a `dyn` seam (CLAUDE.md "instrumentation doesn't belong in the
    production path" tension — arguably this is production data access, not
    instrumentation).
- **(b) Pre-scan + narrow map.** brink-analyzer exposes a cheap "collect
  VAR/CONST ids referenced by this SCC's member bodies" scan; brink-db resolves
  each via `signature_query`, builds a small `BTreeMap`, and passes it into the
  unchanged `BodyCtx.globals: &BTreeMap`. Two walks (scan, then solve) but the
  pure functions stay salsa-agnostic with no re-entrancy.
  - *Tradeoff:* keeps `BodyCtx` a plain map and the walk pure; costs a second
    body traversal per solve and a new public scan fn. The scan is the exact
    reference set we need anyway.

**RULED: (b) pre-scan + narrow map.** brink-analyzer stays pure ("db owns
the when, analyzer owns the how" holds); the dependency edges are identical to
true laziness. Decisive consideration (maintainer question at the round): the
pre-scan **is** a per-def global read-set — the exact fact a T2 effect row
carries (global reads feed purity checking, replay invalidation, and
host-boundary freezing; writes are unambiguous effects). Specify the scan as a
reusable per-def body-facts projection — `referenced_globals(def)`, the same
family as `call_edges(def)` — so T2 extends an existing query instead of
adding another walk. The resolver-callback option would leave no reusable
artifact for effects and blurs the analyzer-purity boundary.

### Ruling 2 — full narrowing: lazy globals AND per-def HIR projection

- **(a) Globals-only (issue's literal scope).** Make `globals` lazy but keep
  `inference_inputs`' all-files `hir_refs` for `collect_defs`/`inferable`.
  *Tradeoff:* smallest PR, but the all-files `lowered_query` edge in
  `inference_inputs` (queries.rs:438) still fires on every body edit — the
  per-def narrowing is **defeated one edge upstream**, exactly the failure mode
  the proposal §3 warns about for `analysis_query`. Buys almost nothing warm.
- **(b) Narrow HIR too.** Replace `collect_defs`(all files) inside `solve_scc`
  with a per-def HIR projection query (`def_body_query(def) → (file, params,
  body)`, keyed per-def, dep = declaring file's `lowered_query`; the
  `inference_index_query` precedent, queries.rs:406-417). `solve_scc(S)` then
  reads HIR only for S's members' files. `inferable` comes from an
  index-sourced `inferable_defs_query` (dep = `inference_index_query`, not
  HIR).
  *Tradeoff:* this is the real warm-path win, but it's a larger change and adds
  1-2 new projection queries.

**RULED: (b) narrow HIR too.** Globals-only would leave the all-files
`lowered_query` edge firing one step upstream, defeating the round's purpose.
FG-2.1 delivers the version where a body edit re-executes only the edited
file's `call_edges` and the SCCs that touch it.

### Ruling 3 — lands BEFORE TM-3 (wave 20; TM-3 slips one wave)

TM-2 merged (#640) with E063 **opt-in, not auto-invoked** — so no warm-path
inference consumer exists yet; the #638 gap is not hurting production today.
TM-3 (#619) is the first slice that runs inference in production (strict
`Unknown`/`Conflicted`-escape errors → `type_diagnostics`).

- **(a) Number FG-2.1 now, before FG-3, land before TM-3** — mirrors the
  FG-1/FG-2-before-TM-2 shape-locking logic: decompose while inference is still
  consumer-free (zero regression surface, TM-1 suite as the gate).
- **(b) Defer until TM-3 actually needs the warm win** — measurement-governed,
  matching the 2026-07-13 "measurement governs sequencing only" ruling; don't
  pay design cost until a consumer proves the bottleneck.
- **(c) Fold into a later FG slice** (e.g. bundle with FG-3's `analysis_query`
  split, since both are dependency-narrowing).

**RULED: (a) before TM-3.** Mirrors the ratified FG-1/FG-2-before-TM-2
principle: shape-lock the substrate while inference is consumer-free (the
window TM-2's E063-opt-in ruling kept open), so TM-3's strict-mode consumer
wires against the final dependency graph and its compile-bench report measures
the real thing. Sequence within wave order: after #626 lands (§7).

### Ruling 4 — one behavior-neutral PR

- **(a) Single FG-2.1 PR.** Behavior-neutral (advisory-only, oracle
  byte-identical), gated by composed-equals-monolithic + fuzz. Feasible if
  Fork 2(a) or a contained Fork 2(b).
- **(b) Mini-spine.** (i) drop pass-1 globals + narrow `call_edges` (§5);
  (ii) lazy globals resolver for `solve_scc` (Fork 1); (iii) HIR narrowing +
  new projection queries (Fork 2b). Each independently oracle-gated.

**RULED: (a) single PR** — one coherent, behavior-neutral, oracle-gated diff
(the FG-2 precedent), with the pointer-identity narrowing test landing in the
same change that makes the claim.

---

## 7. Interaction with in-flight work (design against POST-landing shape)

- **#626 (floating-stitch defs reaching `collect_defs`).** FG-2.1's def→body
  access (Fork 2b's `def_body_query`, or the pre-scan's def enumeration) must
  use the **post-#626** `collect_defs` that finds floating `= stitch` defs
  lowered into `hir.knots` by bare name. **Ordering dependency:** land #626
  first (or rebase FG-2.1 onto it) so the new per-def projection doesn't harden
  the bug. Both are pre-TM-3 (#626 is a TM-3 entry criterion), so sequence
  #626 → FG-2.1.
- **#627 (`Ty::Conflicted`, ruled 2026-07-13).** The resolver signature
  `DefinitionId → Ty` is variant-agnostic, so a new absorbing lattice point is
  transparent to it. A VAR/CONST `value_ty` is single-sourced (one
  initializer/annotation), so a global itself won't be `Conflicted`; and
  `unify` staying monotone (the ruling) means the SCC fixpoint and the
  composed-equals-monolithic gate are unaffected. FG-2.1's cutoff `Eq`
  (`InferredSig`/`BodyTypes`, derived) already covers `Conflicted`. Design
  against the post-#627 `Ty` enum but expect **no structural interaction** —
  just include a Conflicted-global case in tests if #627 lands first.

---

## 8. Cutoff / perf — the dependency graph after

One body edit in file X (`set_text(X)` → `lowered_query(X)` changes):

| Query | Before FG-2.1 | After (Fork 2b) |
|---|---|---|
| `call_edges(def)` | re-executes for **every** def (all-files `hir_refs`) | re-executes only for defs **declared in X** |
| `solve_scc(S)` | re-executes for **every** SCC | re-executes only for SCCs with a member in X **or** referencing a VAR/CONST declared in X |
| `signature(var)` | (already per-file, FG-1) | unchanged — the edge FG-2.1 reuses |

New narrow projection queries needed (the `inference_index_query` precedent):
`def_body_query(def)` (per-def HIR, Fork 2b), `inferable_defs_query(project)`
(index-sourced). No new query for globals — FG-2.1 **reuses** the existing
`signature_query`.

**Is it worth it (the TM-3 frame).** Before: a TM-3 strict-mode consumer forces
O(project) `solve_scc` re-execution on every keystroke (warm
`reanalyze_ide` scales with project size). After: O(SCCs touching the edited
file + SCCs referencing its globals) — for a 50-file project with localized
SCCs, one component instead of all of them. This is the exact FG-1 win
(`signature_query` per-file) extended to the solver. The measurable gate is
`compile_bench.rs` `synthetic_warm.reanalyze_ide` **once TM-3 gives inference a
per-edit consumer** — today (no consumer) both numbers are noise (PR #637's
own bench confirms zero delta), which is precisely why Fork 3 is a real
scheduling question, not an obvious "do it now."

---

## 9. Slice plan

Slots between FG-2 (landed) and FG-3 in the proposal §9 ladder:

- **FG-2.1 — lazy per-reference `BodyCtx.globals`** (one PR, Ruling 4).
  Pre-scan `referenced_globals(def)` projection + narrow map (Ruling 1); drop
  pass-1 globals + narrow `call_edges` (§5); narrow `solve_scc` HIR via
  `def_body_query`/`inferable_defs_query` (Ruling 2). `known_sigs` untouched
  (§3, ratified). Oracle byte-identical (advisory-only). Lands after #626,
  before TM-3 (Ruling 3).

Gate (extends the proposal §7 plan):
- **TM-1 suite unchanged** — `infer/mod.rs::tests` firewall / mutual-recursion
  / determinism (mod.rs:496-737) and `composed_per_scc_solve_equals_monolithic_infer_project`
  (mod.rs:699-737) must stay green: the equivalence anchor.
- **`incremental_fuzz.rs`** — already asserts `inferred_signature`/`infer_body`/
  `signature` incremental-equals-fresh (PR #637). Add a VAR/CONST-initializer
  edit mutation to exercise the lazy-globals edge under equals-fresh.
- **`fg2_scc_dependency_edges.rs`** — add a **narrowing** (not just
  equals-fresh) test, the analogue of `fg1_dependency_edges.rs`'s
  pointer-identity guarantee: edit a body/global in file X, assert
  `solve_scc(S)` for an SCC with no member in X and no reference to X's globals
  is **not re-executed** (Arc ptr identity / salsa did-execute). This is the
  assertion that proves the whole round paid off.
- **`compile_bench.rs`** — cold must not regress; warm `reanalyze_ide` is the
  number FG-2.1 exists to move, measurable only once TM-3 consumes inference.
- **`db_memo_retention.rs`** — if Fork 2b adds `def_body_query`, extend per-def
  churn (add/remove a knot → memo count flat), per proposal §6.4.

---

## 10. Ratified at the round (issue #638's second open question)

- **CONFIRMED:** FG-2 as merged is **structure + value-cutoff only** (not
  execution narrowing) — as PR #637 disclosed; TM-2's opt-in consumer wiring
  against it stands.
- **CONFIRMED (scope boundary):** FG-2.1 narrows **globals (VAR/CONST) only**;
  callable `known_sigs` stays eager in the fixpoint working set; making
  callables lazy is a distinct, cycle-requiring change out of scope (would
  need the native-cycle path declined at the #623 round).
