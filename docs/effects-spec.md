# Effects & ECS scheduling — the T2 round

Status: **language surface fully RULED** (sitting 1, 2026-07-13:
foundations; sitting 2, 2026-07-14: author-facing surface — see the
decision-log entries of those dates). Remaining OPEN items are
host-side only (§10 tail). Prereqs: value-model spec
(ratified), typed-mode spec (shipped), T1c spec (ratified; T1c-2+ in
flight). Implementation waits for the fine-grained salsa substrate
(#623 ruling: per-def effect rows must not ship on coarse
memoization).

## 1. What this buys, in one sentence

The host must make decisions **before** running ink — schedule,
prefetch, subscribe — and effect rows are the only way it can know
anything before running. Consumers, ascending: parallel flow
scheduling (access-disjoint flows advance concurrently), prefetch
(world queries known before entry resolve ahead, collapsing
park/resume round-trips), reactive sleep (a parked flow's dependency
set drives change-detection subscriptions — flows cost zero until an
input moves).

## 2. The layer model — RULED

Three layers, never conflated:

- **Atomic effects** are emitted by *expressions* when they run:
  `read cell`, `write cell`, `call external-kind`. Data never has
  effects; code emits them.
- **Rows** are static summaries of possible atoms:
  `{reads: CellSet, writes: CellSet, calls: KindSet}` — **unordered
  sets** (ordering is the journal's contract, not the row's) — plus,
  since NS-A2 (#1108, from #1087/#1097), three boolean **dimensions**:
  `emits` (may produce content — narration/dialogue fragments; glue-only
  output counts; tag-only lines do NOT), `tags` (may touch the tag
  channel — the independent metadata sibling, per the 2026-07-18 ruling
  refinement), and `faults` (may raise a turn-terminating domain fault —
  E078-lineage conversions, OOB indexing, missing-key reads, division by
  zero, the NS-A1 stdlib faults, projection invalidation, value-call
  dispatch faults; per-fault-kind granularity is the reserved
  refinement). All three are conservative-total and inferred by the same
  per-SCC fixpoint as the sets. Every atom is absorbed into the
  *enclosing definition's* row.
- **Types**: `Ty::Fn` is the **only** row-carrying type constructor —
  a function value is the only data that encapsulates pending
  computation. `fn(int): int ⟨reads: gold⟩` means *calling it* reads
  gold; the returned int is just an int. Collections holding fn
  values carry rows via element types (containment, not
  effectfulness — reading the map is pure).

**Shipped entry rows contain what the host can act on**: World cells
per-cell + external call kinds. Temps die inside the frame and
`#@local` cells are flow-private by construction — neither can matter
to the host; both are excluded from shipped rows. Internal inference
keeps full per-cell precision regardless.

## 3. Soundness direction — RULED (ratified 2026-07-11, restated)

Rows may over-report, never under-report. Over-report = lost
parallelism or a spurious wakeup; under-report = an engine-level
race. Effects are **conservative-total**: the pessimal
touches-everything row is always available and always sound; "no
answer" is never an option (the asymmetry with gradual types, on the
record since the typed-mode round).

## 4. Inference — RULED

A definition's row coalesces exactly like its type: walk the body,
collect atoms, union; direct calls pull in the callee's row with
recursion handled by the **same per-SCC fixpoint as TM-1** (monotone
join, finite lattice — cells + kinds — terminates, no widening);
indirect calls pull in the callee *type's* row (§6). The row lives in
the signature object beside param/return types, so signature-firewall
/ Eq-cutoff economics apply unchanged. FG-2.1's `referenced_globals`
per-def pre-scan (landed) is the atom-collection pass.

Gradual mode: an `Unknown`-typed callee slot has no row to read →
pessimal row. Corollary on the record: **strict mode buys scheduler
precision**, not just error-catching.

### 4.1 `InferPass`'s lambda-body frame boundary — implementation contract (issue #1762)

`InferPass` (`crates/internal/brink-analyzer/src/infer/body.rs`) is one
struct that walks **one enclosing definition's** body. A lambda literal
has no `BodyResult`/`BodyTypes` of its own (no `DefinitionId` exists for
it at inference time — see `infer_lambda`'s doc comment) and a lambda's
own `stmts` still need visiting for their side effects (§2's atom
absorption; issue #1749), so `infer_lambda` re-enters the *same*
`InferPass` via `infer_block_stmt`/`infer_expr` rather than spinning up a
nested pass. That reuse is exactly where PR #1750's first attempt leaked:
`infer_block_stmt` mutates several fields that are conceptually the
**lambda's own frame**, not the enclosing def's — the module doc's own
words are "`return` leaves the lambda"
(`crates/internal/brink-ir/src/hir/lower_native/lambda.rs`), and the very
next clause is the reason a lambda-body walk needs a frame boundary at
all: "*which* frame it returns from is a resolution fact for the layer
that gives the lambda a frame, not a shape this lowering can express".

**The rule.** Every `InferPass` field falls into exactly one of the
buckets below. Get a lambda-body walk on the wrong side of this split and
either a lambda's own `return`/locals corrupt the enclosing def's state,
or (the #1763-shaped mirror problem) atoms that are supposed to be
absorbed into the enclosing row get lost because they were wrongly
snapshotted away.

There is a third hazard the two-bucket split does not, by itself, make
safe: a field can be **cumulative** (never snapshotted) yet still refer
to *frame-scoped* state **by name** rather than by `DefinitionId`. When
that happens, the cumulative field's post-walk resolution can read a
name that meant something different inside the lambda than it means
once the lambda's own frame is gone, because what survives the restore
is the enclosing def's local of that same name. `pending_value_calls` is
exactly this: cumulative (never snapshotted), but it stores
`ValueCallOrigin::Local(name)` entries resolved only after the whole
walk finishes (`InferPass::resolve_pending_value_calls`) against
`local_fn_origins` — which *is* frame-scoped, keyed by that same flat
name, and by then restored to the enclosing def's own summary. Lambda
params are indexed as `SymbolKind::Temp` in the same project-wide name
keyspace (`walk_lambda`,
`crates/internal/brink-ir/src/symbols/project.rs`), so a lambda param
can collide by name with an unrelated enclosing Temp, and a value call
made through the lambda's own param can resolve — silently and
incorrectly — against whatever the enclosing def's same-named Temp
traced to instead, an under-report of the direction §3 forbids.

**Fixed (issue #1779).** A snapshot/restore of `local_fn_origins` around
the lambda's `stmts` does not, by itself, close this — a lambda param
that is only ever *read* (never itself written inside the lambda) never
touches `local_fn_origins` at all, so the map an early, in-lambda
resolution would consult is byte-identical to the one the deferred,
post-restore resolution consults today; timing was never the load-bearing
variable. The actual defect is that a lambda's own param is classified
`ValueCallOrigin::Local` in the first place — the same mistake
`local_call_origin`'s `SymbolKind::Param` arm already refuses to make for
one of *this* definition's own declared params, for the identical reason
("carries an implicit caller-provided initial value [the local write
summary] never sees"). The fix is a third, classification-time guard,
`InferPass::lambda_param_names` — a by-name reference count of every
param belonging to a lambda currently being walked (any nesting depth),
live for the lambda's `stmts` **and** its tail/expr value alike (an
expression-bodied lambda has no `stmts` to bracket at all).
`local_call_origin` classifies a `SymbolKind::Temp` def as `Unknown`
instead of `Local` whenever its name is shadowed this way — regardless of
what `local_fn_origins[name]` currently holds, since that entry (if any)
can only ever be an unrelated enclosing local's own summary or a join
corrupted by this lambda's own same-named write, and neither bounds what
the lambda's param can actually hold. This is strictly more conservative
than the pre-fix behavior (`Local` → `Unknown` only, never the reverse),
so it cannot introduce a new under-report of its own — see the pinning
tests in `crates/internal/brink-analyzer/src/infer/body.rs`:
`lambda_param_shadow_forces_local_call_origin_to_unknown`,
`lambda_param_collision_with_a_traced_enclosing_local_stays_pessimal`, and
the positive control
`lambda_capturing_a_non_colliding_enclosing_local_still_narrows`.

- **Frame-scoped — snapshot before the walk, restore after it.** These
  describe *this one lambda body's own, unmodeled frame*; nothing about
  them is meant to be visible to the enclosing def once the lambda
  returns:
  - `return_ty`, `has_value_return` — a lambda's `return` resolves the
    lambda's own (unmodeled) return type, never the enclosing def's.
  - `locals` — a lambda's `TempDecl`s and param bindings are scoped to
    the lambda's own block, not hoisted into the enclosing def's locals.
  - `annotated` — the lambda's own ascription fallbacks; same scoping
    argument as `locals`.
  - `local_fn_origins` — write-narrowing summaries keyed by local name;
    a lambda-local write must not narrow a same-named enclosing local
    (or vice versa).

  As of this writing that is exactly the five fields PR #1750 identified
  and `infer_lambda` snapshots/restores (`body.rs`, `infer_lambda`) — if
  a future change adds a sixth field to `InferPass` that holds state
  restored to a prior *value* on exit from the lambda (a snapshot taken
  before the walk and written back after it, the way `locals` or
  `local_fn_origins` are), it joins this bucket and this list must grow
  with it. A field that instead needs a push/pop shadow that must not
  outlive the lambda — nothing meaningful precedes it, and it is removed
  rather than restored to some prior value — belongs in the
  **Name-shadowed** bucket below instead; `lambda_param_names` is exactly
  that shape, which is why issue #1779 added a third bucket rather than
  extending this one.

  **Scope of the restore, precisely.** `infer_lambda` snapshots before,
  and restores after, the **whole** of a block body — both its `stmts`
  *and* its trailing tail expression (`LambdaBody::Block { stmts, tail
  }`). The `stmts` walk is where `TempDecl`s, assignments and nested
  `return`s mutate these fields; the tail is the block's *value*
  position, and it both reads and (through `observe`) writes the very
  locals those statements just bound. Both therefore belong to the same
  frame, and the restore point is after the tail, not between the two.
  (A single-expression body, `LambdaBody::Expr`, has no `stmts` to
  snapshot around in the first place — its one expression walks under
  whatever frame was already live when `infer_lambda` was entered.)

  This ordering was itself a bug, fixed by issue #1789: until then the
  restore landed *between* the `stmts` and the tail, because the tail was
  reached through `LambdaBody::value_exprs()` (which, for a block body,
  yields only the tail —
  `crates/internal/brink-ir/src/hir/types.rs`) in a loop that sat after
  the restore. A block-bodied lambda's tail was consequently inferred
  against the **enclosing** def's restored `locals`, and since `locals`
  is keyed by *bare name* the failure was two-directional on a shadowed
  name, plus one further direction on a merely *captured* (not shadowed)
  name:

  - **Read** — a temp the lambda's own `stmts` declared was invisible to
    the lambda's own tail (`ty_of_def` found no entry → `Unknown`), so the
    `E063` arity check (which requires a known callee type) was skipped
    entirely there; an over-applied call through a lambda-local `fn` temp
    in tail position was never checked for arity. A spurious `E065`
    Unknown-escape fired in its place — the wrong diagnostic, not silence.
  - **Write (shadowed)** — `observe` from the tail unified the lambda's
    own type into whatever *enclosing* local carried that bare name, e.g.
    making an enclosing `int` temp `Conflicted` and firing a spurious
    `E066` on a temp the enclosing body never misuses. A false positive
    on user code, and the same class of leak PR #1750 closed for `stmts`.
  - **Write (captured, incidental)** — the fix's frame window has the
    inverse effect on a *captured*, unshadowed enclosing temp: a
    tail-only capturing use's `observe` is now confined to the lambda's
    own frame and discarded by the restore, so it no longer narrows the
    enclosing temp the way it used to. The enclosing temp is left exactly
    as unannotated as the identical use already was from statement
    position under #1750 — a consistency fix, not a new failure mode,
    but reaching `types = strict` diagnostics like the other two.

  All three are pinned by `native_lambda_tail_sees_its_own_block_locals`,
  `native_lambda_tail_does_not_corrupt_a_shadowed_enclosing_local`, and
  `native_lambda_tail_capture_use_no_longer_narrows_enclosing_capture`
  (`crates/internal/brink-analyzer/src/strict.rs`), each of which pairs
  the tail-position fixture with the identical shape written as a
  statement — the statement half was already correct under #1750, so it
  is the tail, not the check, that the fixtures discriminate on.

- **Cumulative — never snapshotted; left to accumulate straight through
  the walk.** These are the **effect-atom accumulators**, and letting a
  lambda body's contribution to them survive into the enclosing frame is
  the entire *point* of walking the lambda's `stmts` in the first place
  (§2/§3: every atom is absorbed into the enclosing definition's row,
  and a lambda has no row of its own to absorb into instead — see
  `infer_lambda`'s doc comment on the keyspace gap). This bucket holds
  the remaining effect-atom accumulators: `calls`, `referenced_globals`,
  `effect_writes`, `external_calls`, `effect_opaque`, `effect_emits`,
  `effect_tags`, `effect_faults`, `effect_faults_refined`, `value_calls`,
  `array_remove_calls`, `created_fn_values`, `pending_value_calls`,
  `param_index`, `param_writes`, `param_holes`, `pending_call_fn_args`,
  `call_fn_args`. (`param_index` is built once from the enclosing def's
  own signature before the walk starts and is only ever *read* during
  it — never mutated (`InferPass::new`; every other use is a `.get`).
  `param_writes`, `param_holes`, `pending_call_fn_args`, and
  `call_fn_args` are the enclosing def's own §6.1 caller/callee-side
  row-variable bookkeeping — cumulative for the same reason as the rest
  of this bucket: a call or a `ref`-argument write made from inside a
  lambda body is absorbed into it exactly like any other atom, per the
  absorption rule stated above.)

- **Name-shadowed — push before the walk, pop after it, by reference
  count.** `lambda_param_names` (issue #1779) is neither of the above: it
  is not restored to a prior *value* (nothing meaningful precedes it —
  entering a lambda only ever adds shadows, never removes one that was
  already there) and it is not left to accumulate forever either — a
  shadow must not outlive the lambda that introduced it, or an unrelated,
  later, non-lambda reference to the same bare name would be wrongly
  de-narrowed too. So each of a lambda's own param names is
  incremented on entry and decremented (removed at zero) after that
  lambda's whole body — `stmts` and the tail/expr value both — has been
  walked. Consulted only by `local_call_origin`, purely as a classification
  guard; it names no targets and joins nothing.

**Why the fix is a wholesale snapshot/restore, not a diff-based undo.**
The tempting shortcut — record which keys `locals`/`annotated`/
`local_fn_origins` gained during the lambda walk and remove just those
afterward — is unsound, because `bind_local` **unifies into a
pre-existing entry of the same name** rather than only ever inserting a
fresh key (`Self::bind_local`, `body.rs`), and `local_fn_origins` has
the same shape of hazard through `bump_local_write`'s
`entry(name).or_default()`: a lambda-local write folds into a
*pre-existing* same-named enclosing Temp's summary rather than minting
a new key. If the lambda shadows an enclosing local this way, the "new
key" diff is empty — nothing was *added* — yet the existing entry's
*value* was still mutated in place by the unify/fold. A diff-based undo
would see no key to remove and leave that corrupted value behind (it
misses this case exactly as it misses an added key). Only a snapshot of
the whole map's prior value, restored wholesale, undoes a same-key
mutation as well as an added key.

**Cross-references.** The worked example lives in code:
`InferPass::infer_lambda`'s doc comment
(`crates/internal/brink-analyzer/src/infer/body.rs`) states the five
frame-scoped fields and points back to this section; keep both in sync
if the field list changes. The adjacent hazard from the other side —
`strict::collect_temps` deliberately *not* descending into
`Expr::Lambda`, because a lambda-local temp can never survive into the
enclosing def's `BodyTypes.locals` for this same frame-scoping reason —
is documented at its own call site in
`crates/internal/brink-analyzer/src/strict.rs` (issue #1763, PR #1766).

## 5. Rows ride the unifier (the heap answer) — RULED

Rows are joined by the same unifier that joins types: a cell or
collection's element type accumulates the join of every fn value
assigned into it, through copies, parameters, returns, and nesting —
because *typing already follows values*. No separate points-to
machinery exists or is planned. The residual imprecision is
flow-insensitivity (the join covers the cell's whole lifetime), which
is the exact and only home of §8's refinements.

## 6. Flow through function values: the four mechanisms — RULED

1. **Higher-order params** (SHIPS): fn-typed params carry **row
   variables**, instantiated at call sites with concrete rows —
   shallow polymorphism, since every value's row is fixed at its
   creation site (`#fn` on a named target; `bind` copies rows;
   rows never grow after birth).
2. **Host-provided callbacks** (SHIPS): inference can't see native
   bodies; the manifest declares each binding's (and host-passed
   callback's) capability row. Symmetric with layer 2 of §9.
3. **The heap** (SHIPS): calls through stored values read the
   cell/element *type's* row — §5. Sound, coarse, improvable (§8).
   The row substrate landed with §6.1c; the *reading* half is still
   open on the stratum question named there.
4. **Runtime narrowing** (SPECIFIED; optional host optimization) —
   §7.

### 6.1a How the call graph learns a fn-value edge — RULED 2026-07-28

*(Fork A of #1680; `docs/decision-log.md` "Fork A — fn-value call-graph
edges are harvested STRUCTURALLY"; implemented by issue #1726.)*

Fn-value callees become call-graph edges through a **structural atom**,
never a row-derived one. `EffectAtoms.creates_fn_values` is the per-def
set of targets whose fn values a body **creates**, harvested by the same
body walk that already produces `direct_calls`/`referenced_globals` —
run with **empty globals and empty sigs**, keeping only structural id
sets. In the monolithic `effects_project` path it is fed into that
function's call graph explicitly, alongside `direct_calls`. The salsa
path (`call_graph_query` → `call_edges_query`/`direct_calls`, what the
IDE, `brink check`, and `@brink-lang/web` actually run) does **not**
read `creates_fn_values` at all; it relies instead on the fact that
`creates_fn_values` is a subset of `direct_calls` by construction (the
same walk records a `#fn` target as both), a property pinned by the
`every_fn_value_creation_target_is_also_a_call_graph_edge` test. Either
way, SCC batching and the `solve_scc_effects` fixpoint are unchanged.

**Parity test (issue #1736).**
`crates/internal/brink-db/tests/query_equivalence.rs` now carries
`analysis_matches_with_fn_value_creation_and_effects_assertion` and its
`…_exceedance` companion: fixtures where a def creates (but never
directly calls) two fn values pointing at different globals, asserting
that `db.analysis()` (the salsa path) and
`brink_analyzer::analyze_with_options` (the monolithic
`effects_project` path) produce byte-identical `AnalysisResult`s,
including the `@[effects(…)]` exceedance diagnostic. Tracing why a
divergence would (or currently cannot) surface here matters more than
the test itself: `EffectAtoms.direct_calls`/`creates_fn_values` are
computed by the *one* shared `infer_def_body` walk both paths call
into — `call_edges_query` (salsa) and
`def_effect_atoms_query`/`effects_project` (both paths) each invoke it
fresh on the same inputs and get the same `.calls` set, so
`direct_calls` itself can never differ between the two paths.
The only place `creates_fn_values` is actually read at all is the
`.chain(&a.creates_fn_values)` in `effects_project`'s local graph
builder (`infer/mod.rs`), and that graph feeds `topo_order`/SCC
batching *only* — `solve_scc_effects`'s row-join loop
(`infer/effects.rs`) iterates `member_atoms.direct_calls` exclusively
and never reads `creates_fn_values`. But that same join loop's
hole-instantiation step (`instantiate_hole`, same file) resolves each
traced fn-value target against `rows`/`known_rows`, falling back to
`next.opaque = true` when a target has no row there — and row
*availability* is decided by topo/batch order, i.e. by the call-graph
edge set: on the salsa side, `effects_scc_query`
(`brink-db/src/queries/mod.rs`) builds `known_rows` only from
`scc_membership_query`'s `depends_on`, itself derived from
`call_graph_query`. So a `creates_fn_values` entry that is *not* also a
`direct_calls` entry (the shape #1727's lambda-literal plan would
introduce — a lambda has no named target for `record_call_edge` to
route through) is correctly invisible to the join loop's own
`direct_calls` iteration, but is *not* thereby proven inert on the
hole-instantiation path: whether a traced target's row is available
there still depends on the graph edge set that decides SCC batching.
**Consequence for #1727:** when lambda literals gain index symbols and
a body creates one without ever calling it, `call_graph_query` reading
`creates_fn_values` (closing this issue's literal ask) will not by
itself make the lambda's row reach its creator — `solve_scc_effects`'s
own join loop will *also* need to fold in `creates_fn_values`, not just
`direct_calls`, or the created-but-uncalled case (exactly what
`creating_a_fn_value_joins_the_targets_row_even_without_a_call` in
`infer/mod.rs` pins today only because the creation site's
`record_call_edge` happens to also fire) will silently under-report
once that dual-recording coincidence is gone. Filed as a scope note on
#1736 for #1727 to pick up, not fixed here — no code today constructs a
`creates_fn_values` superset for this analysis to be tested against.

**No inferred row or signature may ever be consulted to decide an edge.**
That is what keeps `call_graph → scc_membership → solve_scc →
call_graph` acyclic, and it costs nothing, because §6.1 fixes every fn
value's row *at its creation site* and creation sites are **syntactic**:
`#fn(g)` names `g` literally, and `bind` copies from an already-known
value rather than naming a new target. Salsa's native `cycle_fn`/
`cycle_initial` therefore stays **declined** (#623, upheld 2026-07-28):
keeping the dependency graph acyclic is a better property than making a
cycle tractable.

**Consequence for the opaque floor.** `EffectAtoms.opaque` is no longer
a flat *"calls through a function value → pessimal"*. A call through a
local whose **every** write traced to an in-project creation site
narrows to the **join over those targets** — joining over-reports at
worst, so conservative-total (§3) is preserved without having to pick
one origin. The floor survives exactly where the source is genuinely
unknown: a single untraced write (a param, a call's return, a heap
load, or a `ref`-parameter call-site rebind — passing the local into a
`ref` slot lets the callee reassign it to whatever the caller passed
in, so that write is recorded as untraced too) poisons the name, and
those cases belong to §6.2 (manifest-declared host callbacks) and §6.3
/ §5 (the heap's type-row join).

**Known gap — lambda literals.** A lambda's `DefinitionId` is minted
during LIR lowering (`lir::lower::lambda`), downstream of HIR
inference, so it has **no index symbol** to harvest and is out of scope
for this mechanism. The obstacle is keyspace (no index symbol → no
`DefKey`/SCC membership), not timing. Tracked separately (#1727).

**Aliasing channel enumeration — DRAFTED 2026-07-29, PENDING
RATIFICATION (issue #1735).** Filed from the #1726/PR #1731 retro:
does `local_fn_origins` need to learn any more aliasing channels, or
does the untraced-write guard already cover them? #1735 carries
`needs-design`; this enumeration is traced against the existing code
(not new architecture), but per the repo's `needs-design` posture it
is recorded here as a draft awaiting maintainer ratification, not
folded into `docs/decision-log.md` as a settled ruling. Enumerated:

1. **A bare Temp write** (`TempDecl` initializer or `Assignment` to a
   single-segment `Path` resolving to `Temp`) — the one channel this
   rung actually narrows, joined over every write (Fork A above).
2. **A Param write or read** — never narrowed (never classified as
   `Local`, see the soundness argument on `local_call_origin`); a
   fn-typed param instead gets its own row-variable treatment (§6.1b).
3. **A `ref`-parameter call-site *rebind*** (`poke(f, cb)` where
   `poke`'s first param is `ref` — the callee can reassign `f` to
   whatever the caller passed for `cb`) — explicitly folded in as
   untraced by `record_ref_param_writes`, covered above and pinned by
   `a_ref_param_rebind_through_a_call_site_stays_pessimal`.
4. **A `ref` *projection* onto a field/index** (`ref npc.hp`, T1e) as a
   `ref`-slot argument at a **call** (not creation) site — unwrapped to
   its root path first (mutating a projection writes through the
   root's own cell); if the root is a Temp this is channel 3 again, if
   the root is a global it is channel 6 and out of
   `local_fn_origins`'s keyspace entirely (a documented no-op in
   `record_fn_write`, pinned by
   `a_ref_param_write_to_an_unrelated_global_root_does_not_poison_a_traced_local`
   — see that test's own doc comment for what it does and does not
   pin). `ref` also binds at a **second, distinct** grammar position —
   channel 5 below — so this channel's own root-unwrapping is the only
   reduction it performs.
5. **A `#fn`-creation-site `ref`-binding** (`#fn(heal, player_hp)` —
   binding `heal`'s `ref hp` param at creation, docs/t1c-spec.md §1:
   `~ temp heal_player = #fn(heal, player_hp)   // binds ref hp := cell
   player_hp`) — a grammar position distinct from channel 4's
   *call*-site projection argument. A Temp/Param bound here is refused
   outright by `fn_values.rs`'s E080 check (`check_ref_arg`) — the
   creation itself is a diagnostic, so there is no fn-value alias left
   to trace (accepted-VAR / refused-temp fixtures at
   `fn_values.rs::ref_param_bound_to_var_is_clean`/
   `val_params_never_require_binding` and
   `ref_param_bound_to_temp_is_e080`). docs/t1c-spec.md §2 and
   `docs/decision-log.md`'s t1e ratification entry ("projections exist
   only in ref-argument position (`heal(ref npc.hp, 5)`, `#fn`
   binding)") both name this position explicitly — it is not absent
   from the grammar. **Known gap (not modelled, not sound-pessimal):**
   when the bound cell is a valid `VAR` (accepted, not refused),
   nothing in this pass records the write that happens when the
   created value is later called — `infer_fn_literal` never calls
   `record_ref_param_writes`, and the callee's own body write (e.g.
   `heal`'s `hp = hp + amount`) resolves `hp` as a `Param` and can't
   see which caller cell it is bound to. This is a genuine
   conservative-total (§3) under-report, tracked as #1755 (parent:
   #1680, the row-polymorphism gap this most naturally belongs to) —
   not fixed by this pass.
6. **The heap** (a fn value read out of a `VAR`/`CONST` cell, or a
   collection element) — never classified as `Local` in the first
   place (`local_call_origin` only recognizes `Temp`/`Param`), so it is
   not merely untraced, it is outside this mechanism's keyspace by
   construction. §5 rules the answer for this channel: the cell's
   element *type* accumulates the join of every fn value assigned into
   it, because *typing already follows values* — "no separate
   points-to machinery exists or is planned." Pinned by
   `a_call_through_a_heap_stored_fn_value_stays_pessimal`.
7. **A call's return value, or any other non-`#fn`/non-`bind` RHS** —
   `fn_literal_write_origin` returns `None`, folded in as untraced by
   `bump_local_write` (channel 1's own fallback).

**Draft ruling:** channels 1–3 are **modelled** (1 narrows, 2 defers to
§6.1b's separate mechanism, 3 floors); channels 4, 6, and 7 are
**permanently opaque at this rung by design**, not a gap — channel 4
reduces to 3 or 6, and channel 6 is §5's job, a type-level join that is
deliberately *not* a second points-to system. Channel 5 is **mixed**:
its Temp/Param case is refused at the checker (no alias survives to
trace), but its VAR case is a real, currently-unmodelled under-report
(#1755) — not a deliberate pessimal fallback like every other channel
here. A future precision rung may narrow channel 6 further (§8 style —
e.g. reachability-sliced per-cell join), but that is a §8 refinement to
the heap answer, not a new `local_fn_origins` aliasing channel. This
enumeration is otherwise complete against the traced code, but it does
**not** support a claim that nothing is left unmodelled — channel 5's
VAR case is a known, tracked exception, and this whole enumeration
awaits maintainer ratification before it can be treated as settled.

### 6.1b Row variables on fn-typed params — SHIPPED (issue #1680)

Mechanism 1 above, as built. A definition whose body calls through one
of its own `fn`-typed params does **not** take the opaque floor for
that call: its row carries a **hole** at the param's declaration index
(`EffectRow::holes`), which is Fork C's *"row with a hole"* in the
inference layer. The row is therefore parametric — its true effects are
the listed atoms ⊔ the row of whatever fn value the caller passes.

- **The definition read on its own is unchanged.** `EffectRow::opaque`
  now means *intrinsic* opacity; `EffectRow::is_pessimal()` — `opaque ||
  !holes.is_empty()` — is the effective floor, and every consumer of a
  row reads it (assertions, protocol contracts, await purity, IDE
  hover, the `.inkb` writer). An uninstantiated row variable tops the
  lattice exactly like `opaque`, so §3's conservative-total direction is
  untouched.
- **The caller instantiates.** The body walk records, per `(inferable
  callee, argument position)`, the creation targets that position's
  arguments traced to (`EffectAtoms::call_fn_args`) — joined over every
  call site *and* every argument-bearing divert to that callee, so one
  untraceable site poisons the position for all of them. The per-SCC
  fixpoint folds a callee's row in with `join_atoms` (which leaves the
  callee's holes behind — they index the *callee's* param space) and
  discharges each hole from that summary.
- **Structural, so still acyclic.** Both halves are decided
  syntactically, exactly as §6.1a requires: a `#fn` target is a name, a
  local's origin summary is a write set. No inferred row or signature
  is consulted.
- **Fallbacks, all to the floor.** A `ref` param (it aliases the
  caller's storage); a param the body assigns to or hands to a `ref`
  slot (it no longer holds the caller's argument); an argument that did
  not trace; a fill target with no row; and a fill target whose own row
  still holes — §6.1's shallow polymorphism fixes every value's row at
  its creation site, so a hole is filled with **ground** rows and never
  chained into another hole.
- **The wire is unchanged.** The `EffectRows` section stays one ground
  row per def: a row still carrying a hole is *closed* to opaque on the
  way out. Fork C's ruled encoding — an explicit hole slot filled by
  §7's token lookup — is the remaining half and lands with runtime
  narrowing (#1723); the section is section-locally versioned so it can
  grow without a format bump.

### 6.1c The row on `Ty::Fn` — SHIPPED substrate (issue #1680)

Steps 2 and 3 of #1680's ruled build order, as built. `Ty::Fn` now
carries a third component, `FnRow` — the effect row of §5 — and `unify`
joins it alongside the params and the return.

- **What the type carries is the creation-target set, not a computed
  row.** §7 rules that a row is a `DefinitionId → row` **table lookup**,
  so the thing a *type* has to carry is the lookup keys; and §6.1a
  requires that evidence be structural, since `#fn(g)` names `g`
  syntactically and `bind` copies from an already-known value. `FnRow`
  is therefore a set of in-project targets with an `unknown` **top**
  element, and its join is set union with `unknown` absorbing — exactly
  §5's *"a cell or collection's element type accumulates the join of
  every fn value assigned into it, through copies, parameters, returns,
  and nesting"*, and §3's conservative direction (one untraceable
  *typed* source poisons the slot for good). The absorption is on
  `unknown` specifically, not on any write that merely fails to resolve
  a target: a write typed `Ty::Unknown` (an unresolved reference, or an
  unregistered `EXTERNAL`'s return) unifies against `Ty::Unknown` — the
  join *identity*, not `unknown`'s top — so it leaves the other operand's
  row untouched rather than poisoning it. `unify_joins_the_effect_row_alongside_params_and_return`
  (`infer/ty.rs`) enshrines exactly this: an unobserved slot must not
  poison a traced one.
- **Minted at creation sites only, and fixed there.** A `#fn(target, …)`
  literal in a body (`infer::body::infer_fn_literal`) and a global cell's
  `#fn` initializer (`signature::declared_fn_type`, declaration-derived)
  mint a concrete row; `bind` carries the callee's row through unchanged,
  since partial application never changes which def eventually runs.
  Everything else — a written `fn(T…): R` annotation, a lambda (no
  `DefinitionId` before LIR, #1727) — is the top element. A global cell's
  row is minted **once**, at `collect_globals` (`infer/mod.rs`), from
  `declared_fn_type`'s declaration-derived read alone; `BodyCtx::globals`
  (`infer/body.rs`) is a read-only map that body inference only ever
  reads from (`ty_of_def`) — a later `~ cell = #fn(other)` write is
  folded into the effect walk's *write set*, never back into the cell's
  type. So a global's row under-approximates any cell that is ever
  reassigned to a different fn value after its declaration; it is not
  widened by later assignments the way a local's row is. This is the
  same under-approximation the heap rung (below) is meant to read.
- **Rows never decide assignability.** `unify(param, arg) == param` was a
  *structural* test at both `ValueCallKind::ArgMismatch` sites, and
  `strict.rs` promotes that mismatch to an `E063` **error** under
  `types = strict`. Two `fn(int): int` values born at different targets
  join to a third row, so the structural test reports a mismatch whose
  own message is self-refuting ("expected `fn(int): int`, found
  `fn(int): int`" — rows are not part of the written type language and
  never render). `infer::assignable` erases rows on both sides and is
  now the single predicate behind all four assignability checks: the two
  value-call sites, `annotations`' `E063`, and `structs`' `E071`.

**What this does NOT yet do — the open stratum question.** §6 mechanism
3 (the heap) is still pessimal. `def_effect_atoms` deliberately runs the
body walk with **empty globals and empty signatures**, which is
load-bearing for §6.1a's acyclicity, and a `#fn` literal types as
`Unknown` under empty signatures — so the type-carried row is invisible
to effect inference *as currently constructed*. Consuming it means
deciding **which stratum reads the type-carried row**, which the
2026-07-28 sitting did not settle. It is a smaller question than Fork A
was (types depending on already-computed effects is acyclic in the
current query graph, since effects never read types), but §6.1a's
prohibition is written as *"no inferred row **or signature** may ever be
consulted to decide an edge"*, and a global cell's declaration-derived
`Ty::Fn` is reached through `signature()`. That decision is what the
heap rung needs next, and it is the whole of what remains on #1680.

## 7. Runtime narrowing: selection, not inference — RULED

The runtime never computes a row. Every row that can exist is
computed at compile time (no runtime code synthesis, §11) and ships
in a `DefinitionId → row` table; a live fn value is a token; its row
is a **table lookup**. Narrowing, mechanically: at schedule-commit
the host may ask the VM for the fn tokens currently reachable from a
dispatch cell and substitute their looked-up rows for that dispatch's
static fallback.

Soundness gate (static, computed from flow-insensitive info we
already have): a dispatch is **narrowable only if its cell is not in
the entry's own write set** — a turn that can reassign the cell keeps
the static join. Cross-flow interference is the scheduler's existing
job: the dispatch cell is in the reads set, so concurrent writers
were already conflicts.

## 8. Refinement ladder — RULED (optimizer-not-gatekeeper doctrine)

Refinements only ever **narrow** rows; nothing downstream can break
when precision improves (tighter rows = more parallelism, fewer
wakeups, never new conflicts). Per the borrow-analysis precedent:
soundness never depends on any rung; each is adoptable when profiling
justifies it.

1. **Reachability slicing** — per entry point, join only `#fn` sites
   reachable from that entry's call/divert graph (call-graph pruning;
   candidate for v1 if cheap).
2. **Flow-sensitive per-def** — kill/gen over assignments within a
   body (backlog-with-doctrine).
3. **Runtime narrowing** — §7 (host-optional).

## 9. The ECS join and the trust tiers — RULED (direction)

**Two vocabularies, one join.** Brink-side rows speak cells + call
kinds (all the compiler can see; ships in `.inkb`). The host manifest
declares each binding's capability signature in engine vocabulary
(`get_position: reads Transform`). At load: entry row → look up call
kinds → union capability sets with world cells → a scheduler-native
access description. Absorbs/supersedes
`docs/host-capability-manifest.md` (Track B finally has its
consumer).

Trust tiers:

- **First-party `.inkb`**: rows inferred, trusted, zero checks (v1).
- **Foreign ink** (mods/DLC/host-generated): declared capability rows
  **verified against bytecode at load** (JVM-verifier style — cell
  and kind references are statically scannable; reject on
  exceedance). Zero runtime cost; the VM cap-mask (per-op check in
  foreign frames, fault on violation) is the named *fallback*, not
  the default. Direction only — not v1 implementation.
- **Host natives**: manifest declarations are an honesty contract
  with the host's own scheduler, not a security boundary (the host is
  the TCB).

The row table extends **only at load boundaries**, with
verified-or-vouched entries — this is the invariant that keeps
scheduling sound as content loads.

## 10. The author-facing surface — RULED (sitting 2, 2026-07-14)

- **No lockfile.** Inference is deterministic from source — there is
  no reproducibility problem for a pin artifact to solve. The shipped
  `.inkb` rows are the frozen record; a checked-in generated row file
  is rejected as compiler output cosplaying as input.
- **The only contract is the optional inline assertion.**
  **SUPERSEDED SPELLING (NS-A2, #1108; stdlib-spec §9.2, ruled
  2026-07-18; clause grammar AMENDED 2026-07-19 to the Rust-meta-item
  paren shape, issue #1120):** the assertion's final form is the
  **annotation line** `@[effects(…)]`, with args from `{pure, silent,
  total}` (any subset, comma-joined) plus **parenthesized**
  `reads(…)`/`writes(…)`/`calls(…)` clauses — bare top-level idents are
  always flags, so a flag can never be swallowed into an open clause.
  `@[effects(reads(gold), calls(audio))]` declares an upper bound on
  the state row; `@[effects(pure)]` asserts the empty state row (the
  tooling-trust case); `@[effects(silent)]` asserts no `emits` (tags
  are NOT bounded by `silent` — the no-tags arg has no ruled spelling
  v1); `@[effects(total)]` asserts no `faults`. All exceedance-only
  (`E103` state, `E108` silent, `E109` total) — asserting less than
  reality is legal. The old tag-channel spelling `#@effects(…)` shipped
  in released surface (`@brink-lang/web@0.11.1`) and stays a
  **deprecation alias**: its legacy `reads:`-colon clause grammar is
  FROZEN as-is, same checks, plus an `E110` warning. Nothing else errors or warns — there is no drift
  policy because there is nothing to drift against. Drift *visibility*
  is tooling: a `brink ide` effects-diff subcommand (CI-surfaceable as a
  PR comment) and IDE hover (both show the emits/tags/faults dimensions
  alongside the sets since NS-A2).
- **The RNG cell (NS-A6, #1112; stdlib-spec §7, ruled 2026-07-18).**
  RNG state is a named runtime state cell owned by `std::rand`
  (`DefinitionId::RNG_CELL` — the `rng_seed`/`previous_random` pair
  stories have always saved); every draw is an ordinary **write** to it
  in the row, on both surfaces (the frozen ink
  `RANDOM`/`SEED_RANDOM`/`LIST_RANDOM` and the brink draw verbs). No new
  row dimension. In `reads(…)`/`writes(…)` clauses the cell is spelled
  **`rng`** (`@[effects(writes(rng))]` covers a draw-bearing def); a
  user-declared `VAR`/`CONST` named `rng` shadows the spelling, per the
  general stdlib shadowing rule. Consequences fall out of existing
  machinery: `@[effects(pure)]` asserts rng-freedom (E103 names `rng`),
  and the wake-condition purity gate (E105) rejects draw-bearing
  conditions. Ink **shuffle sequences** (`{~a|b}`) are unchanged: they
  derive from the seed + visit index without advancing the cell (a cell
  *read*, which rows do not model — the pre-existing posture).
- **Default-public entry set.** Every knot/stitch ships its row — no
  `#@entry` marker exists (play-from-here already makes any knot a
  host entry). `#@private` opts out: not an entry point, row stays
  internal, host lookup fails at load. Its full visibility semantics
  belong to the **modules round** (the host is "outside every
  module" — one visibility concept, both axes). #657's effects half
  is closed by this; its types half stands (manifest-listed
  host-callable functions require annotations).

**Host-side mechanics — SETTLED (sitting 3, 2026-07-15); author
surface — SETTLED (sitting 4, 2026-07-16)**: see §12–§13. The T2
design round is CLOSED; everything remaining is implementation.

## 11. Invariants and non-goals

- **No runtime code synthesis** — the load-bearing invariant (not "no
  lambdas": lambdas-later get synthesized DefinitionIds, the same
  creation-freeze, and capture inherits the durable-cell rule; their
  save-stable anonymous identity is the lambda round's problem, not
  this spec's).
- Rows fixed at creation; refinements only narrow; the table extends
  only at load boundaries.
- **Format**: `EffectRows` (VERSION 4 reserved, section-locally
  versioned) ships **factored rows** — direct part + per-dispatch
  entries `{cell, narrowable, static fallback}` — plus the
  `DefinitionId → row` table. NS-A2 (#1108) bumped the section-local
  version 2 → 3: every `DirectEffects` block carries a trailing
  extension-flags byte (bit 0 `emits`, bit 1 `tags`, bit 2 `faults`;
  bits 3–7 reserved and strict-rejected — per-fault-kind granularity is
  the named future occupant, graduating via the next section-version
  bump, the same reservation discipline as the capability/handle
  slots). Flat rows are rejected: they
  structurally foreclose §7. **Capability atoms carry a reserved
  parameter slot** (ruled 2026-07-14, t1d-spec §7): an atom may be
  handle-parameterized (`Transform(@argN)`); v1 populates every atom
  as `(any)` (component-granular); instance resolution is a later
  narrowing rung (token comparison at schedule-commit — §7's
  machinery, second client). Possession-bounded capabilities are the
  tier-2 security model: handles are true ocap tokens (only bindings
  mint them). Exact byte layout at implementation, inside the
  section version.
- Non-goals on record: tier-2 (foreign-ink) implementation,
  entity-granular capabilities, dynamic linking (#717, icebox), any
  author-facing effect syntax beyond the entry-point freeze (no
  monads, no handlers, no function coloring — interior effects are
  inferred, always).


## 12. The bevy host runtime — settled mechanics (sitting 3, 2026-07-15)

Recorded so implementation can proceed later without re-deriving.
T2's implementation splits: the **compiler half** (row inference,
`#@effects`, EffectRows emission — buildable now on the delivered FG
substrate) lands first; the host half builds against real shipped
rows.

### 12.1 The frame loop

bevy-brink's flow-runner advances pending flows each frame through:
**Collect** (spawned + woken) → **Schedule** (per-flow next-turn
access from the row of the container the flow is parked in) →
**Prefetch** (§12.3) → **Step** (parallel, pure VM against borrowed
reads, writes/commands buffered) → **Apply** (buffered writes in
deterministic flow-id order) → **Subscribe** (newly-parked flows
register wake-dependency sets) → **Detect** (cell writes + component
change ticks wake intersecting parked flows).

Mechanism notes banked from the walk-through:
- **Per-container rows are the resume-scheduling estimate** — a flow
  resumes from wherever it parked, which is the concrete reason every
  knot/stitch ships a row (the earlier "host can start anywhere"
  rationale was the weaker half of the truth).
- **Reactive sleep needs no new row granularity**: a parked flow's
  wake set = the live park-condition's direct reads (walkable at
  runtime) ∪ the transitive rows of functions the condition calls
  (the shipped `DefinitionId → row` table).

### 12.2 Borrow, don't copy — RULED

The host **never copies world data** to parallelize. The row-join
output is the same currency as bevy's `FilteredAccessSet`; the step
phase runs flows on the task pool inside an `UnsafeWorldCell` scope
with access proven by rows — shared read borrows, buffered writes
(bevy's own executor pattern, one storey down). The §8 snapshot-only
contract applies **at the ink boundary only**: what crosses into
script is a value (a binding returning a `Transform` copies one small
struct — inherent to value semantics); it never required host-side
world copying.

### 12.3 Prefetch, honestly

Three cheap things, zero data copying:
1. resolve dynamic `QueryState`s per access set once per batch
   (`QueryBuilder`, `ComponentId`-driven, `FilteredEntityRef`);
2. prevalidate entity liveness for handle-targeted reads (feeds the
   dead-handle path pre-step);
3. **eliminate park/resume round-trips**: with the borrow held during
   step, world-read bindings become synchronous reads —
   `AwaitingExternal` suspension for world queries stops existing on
   the batch path.

### 12.4 Frame-start consistency — RULED (the concurrency semantics)

Batch-scheduled flows read the **frame-start world** (reads pinned to
the frame-start change tick); writes buffer and apply in
deterministic flow-id order at Apply. Consequences: everything
parallelizes (no conflict partitioning needed — even write-write is
deterministic by apply order); a peer's same-frame write is visible
**next** frame (double-buffered, simulation-tick semantics). The
**serial host API** (stepping one flow at a time) keeps today's
immediate-visibility semantics — documented as the serial mode, not a
policy switch. The write-write wire bit considered earlier is NOT
needed under this ruling; it stays a reserved-slot question if a
conflict-partitioned policy is ever wanted.

### 12.5 Two-level bevy integration — RULED direction, staged

- **Level 1 (v2 optimization)**: build the flow-runner via
  `SystemParamBuilder`/`QueryParamBuilder` with the **aggregate
  access of all loaded stories' rows**, so bevy's own scheduler runs
  narrative concurrently with unrelated game systems (no `&mut World`
  serialization). A system's access is fixed at build — which aligns
  exactly with the ruled **load-boundary** invariant: story
  load/unload is when the params rebuild.
- **Level 2 (v1)**: exclusive-system driver; inside it, dynamic
  queries + `Access` math + `UnsafeWorldCell` + `ComputeTaskPool`
  scope per §12.2–12.4. Fully correct and parallel across flows on
  day one; level 1 is pure scheduling throughput, zero semantic
  change.
- Change detection for sleep rides per-entity `ComponentTicks`
  through untyped access — no typed `Changed<T>` filters needed.

### 12.6 What remains for the final host sitting

Manifest capability grammar (author-facing spelling; the
change-detectable-vs-opaque read bit per binding); reactive-sleep
author-facing API surface; serial-mode documentation. Everything else
in the host half is settled above.


## 13. The host author surface — RULED (sitting 4, 2026-07-16; the round's final sitting)

### 13.1 Reactive sleep is host-driven (no language change)

Sleeping is a bevy-brink API, not an ink construct. The game sets a
flow's **standing wake policy**; ink authors write ordinary knots. A
language-level `await {cond}` primitive **is planned, not deferred** —
subsequently ruled as statement-position syntax in
`flow-suspension-spec.md` §3 (2026-07-16). (The earlier "future
direction, not v1" wording here is **superseded spec drift**, corrected
2026-07-21.) Host-driven reactive sleep (this section) and in-language
`await` are **complementary**, not competing: the host sets a standing
policy on ordinary knots; `await` is the author-written park form.

**The wake contract** (ruled precisely):

1. `wake_when(cond)` does not park — flows park at their natural
   yield points (turn end, `-> DONE`). The policy governs *waking*:
   a parked flow under a policy is skipped by Collect entirely; its
   condition's dependency set (computable from effect rows) sits in
   the wake map. Parked cost: zero.
2. A dependency changing triggers **re-evaluation, not waking** —
   the condition (a pure fn per its effect row; purity is checkable
   now) is evaluated only when a dependency moved, and the flow
   wakes only on condition-true.
3. A woken flow runs a normal turn; the condition has no mid-turn
   influence.
4. Policies are **persistent by default** (re-arm when the flow
   re-parks); `wake_once` covers one-shots; the host may clear or
   replace a policy anytime.
5. The policy applies to **turn-boundary parks only**. Choice-blocked
   and external-blocked flows have their own resume paths (`choose`,
   `resolve_external`); `-> END` flows are dead and the policy is
   inert.
6. `spawn_flow(...).wake_when(cond)` spawns **dormant**: parked at
   entry, first turn runs on first condition-true.

**Bevy shape**: a `FlowSleep` component on the flow entity (condition
+ dependency set) — ECS-idiomatic, inspector-visible; the wake system
is a query over `FlowSleep`. (The callback-registry alternative is
rejected.)

**What a row can and cannot say about a dependency** (issue #1146,
the #1101 fix): point 2's "only when a dependency moved" is decided by
intersecting the changed cells of a batch turn's Apply with the
condition's inferred **read row**. That row bounds **global cells**
(§2's `reads`) and nothing else — story *bookkeeping* (visit counts,
turn counts, the turn index, RNG state) has no read atom, so a
condition reading `TURNS_SINCE(-> knot)` or a visit count is
indistinguishable from a constant one. Because every real turn writes
bookkeeping, treating it as a dependency of everything would collapse
the whole mechanism back to "re-check on any change", so v1 treats a
bookkeeping write as inert for a condition with a known, non-opaque row
and asks the host to declare the exception (bevy-brink:
`FlowSleep::reads_bookkeeping()`). A missing or **opaque** row still
re-evaluates on any change — the conservative floor of §3 applies here
exactly as everywhere else. Graduating a bookkeeping dimension on the
row (so the declaration is inferred, like `emits`/`tags`/`faults` in
NS-A2) is the recorded follow-up.

### 13.2 Manifest capability grammar

Capabilities extend the existing JSON manifest per external:

```json
{ "name": "get_position",
  "params": [{"name": "npc", "ty": "handle<Npc>"}],
  "effects": { "reads": ["Transform"],
               "detect": {"Transform": true} } }
```

- Capability names are **engine-vocabulary strings, compiler-opaque**
  — the compiler validates structure only; bevy-brink maps
  name → `ComponentId` at registration (the `HandleKind` pattern).
  This is what keeps the format host-agnostic (ruled value).
- The **`detect` bit** marks a read as change-detection-backed
  (bevy ticks) vs opaque (must poll for sleep purposes).
- Entity-granular parameter syntax: **reserved, not designed**
  (matches the wire slot).

### 13.3 Serial mode

The ruled semantics (serial host stepping = immediate write
visibility; batch scheduling = frame-start consistency, §12.4) get a
book section with the host implementation. Documentation deliverable
only.


## 14. Native-surface amendment — the row is the unified effect signature (AMENDED 2026-07-21)

Ruled in the block/effect/coroutine design sitting (see
`docs/block-effect-model.md` and the decision-log entry of this date).
This **extends §2's row model** — it does not replace it. It deliberately
reopens the "row dimensions" question that the §10 tail marked closed;
the maintainer ruled the reopening. Everything in §1–§13 stands.

### 14.1 The row is THE canonical effect signature — RULED

Every consumer queries **one** row: the host scheduler (§12), the wake-map
(§13), the type/coloring checker (`block-effect-model.md`), iterator
fusion, and consumers **not yet identified**. The row is expected to
**grow dimensions** as consumers appear — one home, not parallel
per-consumer effect systems. This is an explicit reversal of the working
assumption (held briefly during the sitting) that suspension "color" and
control-flow "tail" were sibling systems alongside the row: **color folds
into the row as a dimension; tail stays structural** (below).

### 14.2 Two new dimensions

- **`suspend(rung)`** — whether a definition can park, and at which rung
  (await / choice / turn), per the `flow-suspension-spec.md` ladder. This
  **folds the suspension "color" into the row**: the coloring rule "a
  definition may not call one whose outermost suspension rung exceeds its
  own" is an **inferred check over this dimension**, not an author
  annotation. (See the §11 reconciliation note, 14.5.)
- **`terminates`** — *PROVISIONAL* — whether control can leave via
  `-> END` / `-> DONE`. Sibling to `faults` (abnormal exit); the
  identified consumer is structured-concurrency lifetime (a supervisor
  knowing a child flow can finish). Marked provisional pending a confirmed
  consumer beyond lifetime; include-by-default is cheap and reversible.

Both are **inferred** by the same per-SCC fixpoint as every other
dimension, and both ride the `Ty::Fn` row and the shipped
`DefinitionId → row` table. Format: new extension-flag bits in the
`DirectEffects` block (§11 reserved bits 3–7; these graduate two of them
via the next section-version bump, same reservation discipline).

### 14.3 What stays OUT of the row — on the merits, not by omission

- **General control-transfer (a plain divert)** is **structural — the
  block's `tail`** (`block-effect-model.md`), enforced by the
  no-lateral-divert-from-a-value-flow rule. A divert's *data* effects are
  already absorbed transitively (the row walks through diverts); a plain
  divert adds nothing a scheduler needs. Only the terminal `-> END/DONE`
  case (lifetime) is a candidate dimension (`terminates`), never general
  divert.
- **Sequence-impurity** (the implicit cycle/once/shuffle cursor) stays out
  per the §10 / NS-A6 ruled posture: visit-index is a *read* the rows do
  not model; it is flow-local (no cross-flow scheduling relevance) and
  never appears in a fusion callback. No consumer is served by a distinct
  label.

### 14.4 Reads are the dependency axis, not an effect — RESTATED

Reaffirming what §1 and §13 already rely on: `reads` is the wake-map's
**dependency set** (a coeffect / input); it does **not** make a
definition impure. The two ruled purity predicates both stand and are for
different jobs — strong `@[effects(pure)]` (reads-free, the tooling-trust
bound, §10) and the weak **E105 wake-gate** (reads-OK, §13). **Iterator
fusion uses the weak predicate** (a fold may read a stable global; only
writes / calls / emits / suspend defeat fusion).

### 14.5 Reconciliation notes — FOR MAINTAINER REVIEW

1. **§11 "no function coloring" — RESOLVED 2026-07-21.** The `suspend(rung)`
   dimension + no-call-up-the-ladder check are coloring-*shaped* but do not
   reintroduce coloring: the dimension is **inferred like every other row
   dimension** (never author-written), and the check is a purity-style
   inferred constraint — no author-facing coloring syntax, no viral
   annotation. `flow-suspension-spec.md` §4 already ruled the compatible
   position: *"no colored-function virality can exist — the 'color' is a
   distinction ink authors have always had"* (the existing fn vs knot/tunnel
   boundary). The suspend dimension merely makes that existing structural
   distinction inferred and queryable off the row. §11's intent (interior
   effects inferred always; no author coloring surface) is preserved.
2. **`await` posture gap — RESOLVED 2026-07-21 (spec drift, not a
   conflict).** §13.1 formerly called a language-level `await` a "future
   direction, not v1," which predated `flow-suspension-spec.md` §3 ruling
   `await` as statement syntax. Corrected in §13.1: `await` is **planned**;
   host-driven reactive sleep and in-language `await` are complementary.
   Folding `suspend(rung)` into the row rests on that (planned) suspension
   model, with no spec conflict.

### 14.6 Build posture

- **§6.1 shallow row-polymorphism is IN-SCOPE, not deferred.** Without it,
  a call through a fn-value is opaque/pessimal, which makes the row
  useless across the native code dialect's higher-order core (lambdas,
  fn-value iteration) — precise only for first-order code. It builds as
  part of the effect-system work (tractable: substitution over
  creation-fixed rows, riding the SCC fixpoint), off the "author a scene"
  critical path but not deferred. **Landed in two steps.** §6.1a's
  structural creation-site atom (#1726) narrows a call through a *local*
  whose every write traced to an in-project `#fn`/`bind` creation site to
  the join over those targets; §6.1b's row variables (#1680) cover the
  case that atom cannot reach — a call through a fn-typed **param**,
  whose value the caller supplies. What remains pessimal is a call
  through a value loaded out of the **heap** (§6 mechanism 3) and a call
  through a **lambda**, which awaits #1727's index symbols. The heap's
  `Ty::Fn` row substrate landed with §6.1c; what it still needs is the
  stratum decision named there, not more type machinery.
- The shipped core (set-based row, per-SCC fixpoint, `EffectRows` wire
  format, `@[effects(…)]` assertions) is reused unchanged; this amendment
  ADDS the two dimensions and the "one canonical signature" framing, and
  wires row inference to the native-lowered HIR.

## 15. Testing Guidance — Effect-Row Semantics

Any PR touching effect-row inference, inference soundness, or effect-row
narrowing / dimensions must run the independent ground-truth harness
**before** merging:

```sh
cargo test -p brink-test-harness --test t2_ground_truth_effects --features effect-trace -- --nocapture
```

This harness (`crates/internal/brink-test-harness/tests/t2_ground_truth_effects.rs`)
traces actual bytecode execution through the instrumented runtime and
asserts that the **statically-inferred** `effects(def)` row covers every
atom actually observed at runtime. It is the independent oracle for
effect-row completeness (§2/§3) — complementary to the snapshot-based
oracle-episode tests but testing a different invariant (static precision,
not behavioral conformance). The oracle-episode snapshots check
behavioral conformance against the C# ink runtime; the structural
`conservative_total_*` tests below check only inter-row ⊇-consistency,
which a shared under-report (#866) satisfies. This harness is what
isolates effect-inference bugs in callees even when the caller's row
accidentally masks the under-report through structural over-reporting.

The test is feature-gated `required-features = ["effect-trace"]` so that
`cargo test --workspace` (the default gate) never builds or runs it. CI
covers it through the `Test (all features)` job (`cargo test --workspace
--all-features --exclude bevy-brink`), which builds and runs this
harness on every PR; the explicit command above gives the same signal
locally, before pushing. A similar pattern to `bench-counters` (issue
#821), this keeps effect-row correctness in scope without slowing the
default/critical-path gate.

Structural tests (`brink-analyzer::infer::effects::conservative_total_*`)
check inter-row consistency (a caller's row ⊇ its callees' rows); do not
mistake that for completeness (the #866 ref-param-write bug passed every
structural test while *both* rows silently under-reported the same real
write). Always run the ground-truth harness before shipping effect changes.
