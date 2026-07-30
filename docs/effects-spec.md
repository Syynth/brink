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

**Parity test (issue #1736).** `crates/internal/brink-db/tests/
query_equivalence.rs` now carries
`analysis_matches_with_fn_value_creation_and_effects_assertion` and its
`…_exceedance` companion: fixtures where a def creates (but never
directly calls) two fn values pointing at different globals, asserting
that `db.analysis()` (the salsa path) and `brink_analyzer::
analyze_with_options` (the monolithic `effects_project` path) produce
byte-identical `AnalysisResult`s, including the `@[effects(…)]`
exceedance diagnostic. Tracing why a divergence would (or currently
cannot) surface here matters more than the test itself: `EffectAtoms.
direct_calls`/`creates_fn_values` are computed by the *one* shared
`infer_def_body` walk both paths call into — `call_edges_query`
(salsa) and `def_effect_atoms_query`/`effects_project` (both paths)
each invoke it fresh on the same inputs and get the same `.calls` set,
so `direct_calls` itself can never differ between the two paths.
The only place `creates_fn_values` is actually read at all is the
`.chain(&a.creates_fn_values)` in `effects_project`'s local graph
builder (`infer/mod.rs`), and that graph feeds `topo_order`/SCC
batching *only* — `solve_scc_effects`'s row-join loop (`infer/
effects.rs`) iterates `member_atoms.direct_calls` exclusively and never
reads `creates_fn_values`. A `creates_fn_values` entry that is *not*
also a `direct_calls` entry (the shape #1727's lambda-literal plan
would introduce — a lambda has no named target for `record_call_edge`
to route through) therefore cannot currently become load-bearing for
any computed `EffectRow` on **either** path, batching differences
included: nothing ever asks `solve_scc_effects` to join over it. Confirmed
empirically by temporarily disabling `record_call_edge` at the `#fn`
creation site alone (simulating exactly that superset shape) and
re-running the parity tests above — both paths stayed identical, because
`resolve_pending_value_calls`'s call-through-a-local narrowing
(`body.rs`) independently calls `record_call_edge` for every traced
`#fn`/`bind` origin at the *call* site, so a target that is ever actually
invoked through the local still lands in `direct_calls` regardless of
the creation site. **Consequence for #1727:** when lambda literals gain
index symbols and a body creates one without ever calling it,
`call_graph_query` reading `creates_fn_values` (closing this issue's
literal ask) will not by itself make the lambda's row reach its
creator — `solve_scc_effects`'s own join loop will *also* need to fold
in `creates_fn_values`, not just `direct_calls`, or the created-but-
uncalled case (exactly what `creating_a_fn_value_joins_the_targets_row_
even_without_a_call` in `infer/mod.rs` pins today only because the
creation site's `record_call_edge` happens to also fire) will silently
under-report once that dual-recording coincidence is gone. Filed as a
scope note on #1736 for #1727 to pick up, not fixed here — no code
today constructs a `creates_fn_values` superset for this analysis to be
tested against.

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

Still outstanding on #1680: a row on `Ty::Fn` itself (with the
unifier's row join, and the row-insensitive equality at the two
`ValueCallKind::ArgMismatch` sites that would otherwise fire a spurious
`E063` under `types = strict`), which is what §6 mechanism 3 — the heap
— needs. A call through a fn value loaded out of a VAR/CONST cell is
still pessimal.

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
  through a value loaded out of the **heap** (§6 mechanism 3), which
  needs a row on `Ty::Fn`, and a call through a **lambda**, which awaits
  #1727's index symbols.
- The shipped core (set-based row, per-SCC fixpoint, `EffectRows` wire
  format, `@[effects(…)]` assertions) is reused unchanged; this amendment
  ADDS the two dimensions and the "one canonical signature" framing, and
  wires row inference to the native-lowered HIR.
