# Scoped Flow State — runtime core restructuring spec

**Status:** Design (0.9.0). Load-bearing foundation; **implementation does not
begin until this spec is approved.** This supersedes the "speculative execution"
framing of [speculative-eval-spec.md](speculative-eval-spec.md) (#411): watch/eval
becomes a *light feature* riding on the core defined here, not a standalone
mechanism.

> **Trust / oracle note.** This restructures the runtime's core state and
> orchestration. The hard invariant is that **the single-flow path stays
> byte-identical** — the oracle corpus (harness drives `Story`) must not move.
> The whole change lands as a behavior-preserving decomposition *first*, with
> the new scoping engaged only by a non-default policy. If we get that wrong
> it's an oracle regeneration, which the project rules class as a major
> operation. Designed additively, it stays clear of that.

## Why this exists

Three problems, one root cause.

1. **State is global-only.** All mutable story state lives in a single
   `Context` (globals, visit/turn counts, turn index, RNG). A flow either shares
   the *entire* context (`spawn_flow_shared`) or forks the *entire* thing
   (`spawn_flow`, isolated). Binary, whole-context, at the endpoints — no way to
   say "this variable is world-global, that one is per-entity."

2. **Orchestration is duplicated.** `Story` is the batteries-included
   orchestrator (web `StoryRunner`, cli, ide, and the **oracle harness** all
   drive it). But **bevy-brink bypasses it** — it composes `FlowInstance` +
   `Context` directly and re-implements the drive-to-terminal loop, replay
   recording (`ReplayRecorder`), and locale application (`apply_locale`) that
   `Story` already contains, because `Story`'s shape doesn't fit ECS. Two
   orchestrations kept in lockstep by hand is the long-standing tax.

3. **Watch/eval / per-entity flows have nowhere clean to live.** The
   celeris State panel wants side-effect-proof watches; games want per-NPC
   flows with their own local state. Both were being designed as bolt-ons.

The root cause is the same: **there is no clean primitive layer between raw
`FlowInstance`/`Context` and the batteries-included `Story`.** bevy reaches past
`Story` into raw internals; watch/eval had to invent isolation machinery. This
spec defines that missing layer.

## The model

### Two intrinsic categories

- **Execution-local** — callstack/threads, output buffer, pending choices,
  current PC. A flow *is* these; nothing ever shares them. Already on
  `FlowInstance`. Not a policy.
- **Story-state** — globals, visit counts, turn counts, turn index, RNG. **All
  of it is uniformly scopable** world-vs-local, per addressable unit.

### Uniform scoping

Every unit of story-state is homed to **World** or **Local** by one policy:

| State | Scoping granularity |
|---|---|
| globals | per variable (`DefinitionId` / slot) |
| visit_counts | per knot/stitch (`DefinitionId`) |
| turn_counts | per knot (`DefinitionId`) |
| turn_index | per field (scalar) |
| rng_seed / previous_random | per field (scalar stream) |

Visit-count sharing is therefore **not a special case** — declaring knot
`shrine` as World gives it a shared visit count (any flow entering bumps the one
count, any `{shrine: …}` reads it) via the *same* routing as a shared variable.

The **single-flow story is the degenerate policy** — every unit homed to World,
no local layer — which is byte-for-byte today's `Context`. That is the
oracle-safety anchor.

**Caveat (documented, not hidden):** a World-scoped RNG stream interleaves draws
from all flows by execution order, so its determinism depends on flow
*scheduling*. Flow-local RNG has no such dependency. World-RNG is *expressible*
(a shared "deck" mechanic wants it); the consumer takes on the scheduling-order
semantics when they scope it that way. Same for `turn_index`.

### Layered representation (copy-on-write)

The flow-local layer is **always** a sparse override over a frozen base — spawn
and discard are O(1), nothing is eagerly copied. Reads walk a chain:

```
flow-local overrides → (parent snapshot, if spawned from a flow) → World → Program defaults
```

- **Read** a unit: first hit in the chain wins (ultimately the Program's
  immutable default).
- **Write** a *Local* unit → own override map (CoW; base untouched).
- **Write** a *World* unit → the shared World layer.
- **Spawn** from a flow: new empty override map based on the parent's current
  local — an O(1) structural-sharing snapshot, so the child sees state *as of
  spawn* and the parent's later writes don't leak in.
- **Discard**: drop the map. (watch/eval; also a rejected side-conversation.)
- **Commit**: fold the override map into the base.

Because the base is a frozen snapshot, CoW here is observably a snapshot copy —
the persistent map just makes the snapshot free. Watch/eval needs no bespoke
"isolation": it is a flow-local layer read-through to the live flow, discarded.

### Write-back is determined by scope, not a separate knob

- **World-scoped write** → shared World layer, visible to every flow
  immediately. No "make it back" step — it was never forked. This is the
  coordination path.
- **Flow-local write** → private to the flow. Persists as that flow's own state
  for its lifetime; folds upward **only via an explicit `commit`**.

**Commit is a fork-only operation, and it is a documented seam — deferred, not
implemented in 0.9.0.** Designing the spawn API (below) made this precise:

- A **root flow** (the primary story flow, a per-entity NPC) has nothing above
  it but the world, and world-scoped writes *already* escape live — so a root
  flow never commits. Its flow-local state (an NPC's private `mood`) simply
  persists for the flow's lifetime.
- A **fork** is the only thing with a parent to resolve back into, so it's the
  only thing that commits (fold up) or discards (drop). watch/eval always
  discards; a sub-conversation whose private outcome should stick would commit.

Nothing in 0.9.0 needs fork-commit — per-entity NPCs persist, watch/eval
discards — so `commit(child, &mut parent)` is **defined as a seam and left
unimplemented**, which keeps the **commit-conflict policy entirely out of the
release**. (When implemented: default fold-into-current-base, last-write-wins,
merge hook later; world-scoped units never hit it, being written live.)

## The policy

```rust
WorldPolicy {
    default: Scope,               // World | Local — what unlisted units are
    overrides: Map<Name, Scope>,  // the exceptions (variable names, knot paths)
    // + scalar flags for turn_index and rng
}
```

The `default` field is the whole local-by-default vs global-by-default dial:

| Use case | policy |
|---|---|
| single-flow story | `default: World, overrides: {}` → **identical to today** |
| per-entity NPC | `default: Local, overrides: {gold: World, shrine: World}` |
| fully isolated fork | `default: Local, overrides: {}` |
| ink concurrent flows | `default: World, overrides: {}` |

**Where it lives / comes from / is used:**

- **Comes from the host**, at world creation — *not* baked into ink. The same
  `.ink` file is instanced single-flow by one host, per-entity by another; the
  compiled artifact carries no policy. (Consistent with the dialect ruling and
  the host-capability-manifest precedent. An author-intent annotation that
  *produces* a default policy is a possible later convenience; the runtime always
  consumes a resolved policy, never ink annotations.)
- **Lives in the `World`**, resolved. `WorldPolicy` (names) is resolved **once at
  creation** against the `Program` symbol table into a fast `ResolvedPolicy`
  (per-slot flag / knot set / scalar flags). Unknown names fail *there*, once —
  not at runtime. **Immutable for the world's lifetime** (changing the split
  would mean migrating live state between layers).
- **Used behind the existing `ContextAccess` seam.** The VM already accesses all
  story-state through `context: &mut impl ContextAccess` — it never touches
  `Context` fields directly. The policy lives *behind* that trait (exactly as
  `ProgramLike` sits behind the VM's program access). A **routing view**
  implements `ContextAccess` over `(&mut World, &mut FlowLocal)` and consults
  `ResolvedPolicy` on each `get_global`/`set_global`/`visit_count`/
  `increment_visit`/`turn_*`/`draw_random`. The VM code is **unchanged**; routing
  is an O(1) lookup; the all-World default routes everything to the shared store
  — today's behavior byte-for-byte.

### Sandbox mode (orthogonal)

watch/eval is **not** a different `WorldPolicy` — it inherits the world's split
like every flow. What sandboxes it is a per-spawn **mode**: treat the World layer
as a read-only snapshot (even World-scoped writes divert to a throwaway local
layer) and discard on drop. It is one more `ContextAccess` wrapper over the same
routing view — no policy change, no new state path.

## Core primitives and placement

The layer both `Story` and bevy-brink compose — **not** owned by `Story`:

| Primitive | What it is | bevy today → after |
|---|---|---|
| `Program` | immutable bytecode (shared) | `BrinkProgram<M>` |
| **`World`** | shared layer + `ResolvedPolicy` | `BrinkGlobals<M>` (raw `Context`) → wraps `World` |
| `FlowInstance` | execution state | `BrinkFlow<M>` |
| **`FlowLocal`** | per-flow override layer (from per-flow `Context`) | `BrinkContext<M>` (raw `Context`) → wraps `FlowLocal` |
| routing view | `ContextAccess` over `(&mut World, &mut FlowLocal)` | built transiently in the advance system |
| `step(program, flow, view)` | the VM | called by bevy's advance system |
| shared drive ops | drive-to-terminal, `Line` assembly, replay, locale | replaces bevy's duplicated copies |

`Story` and bevy-brink are **peer orchestrators** over this core — `Story` the
single-story batteries-included one, bevy the ECS/many-entity one. Neither is
privileged; neither owns the state model.

## Ownership and lifetimes

The current `Story<'p, R>` borrows `program: &'p Program`. That borrow is a
primary source of the mess: web already works around it with **three `unsafe`
raw-pointer transmutes to `&'static Program`** over a pinned `Box` (brink-web
`lib.rs` `new`/`reset`/`reload`), and bevy's program is a shared *asset*
referenced by flow *components* that can't carry `'p`. `Program` is **not**
`Arc` or `Clone` today (`Story::clone` works only because `&Program` is `Copy`);
F1.1 wraps it in `Arc` and deletes both the `<'p>` parameter and web's unsafe.

The rule: **immutable-and-shared → `Arc`; mutable → single-owner with a
step-scoped `&mut`; structural sharing (`Arc`) quarantined inside the override
map.**

| Thing | Ownership | Lifetime |
|---|---|---|
| `Program`, line tables | `Arc<_>` — immutable, shared | none — the `<'p>` parameter is **deleted** |
| `World` | single owner (`Story` field / bevy `Resource`) | `&mut` per step |
| `FlowInstance`, `FlowLocal` | owned per flow | `&mut` per step |
| parent snapshot (fork base) | **owned** frozen persistent-map clone | none — *not* a borrow |
| routing view | transient, borrows `(&mut World, &mut FlowLocal)` | step-scoped only |

Consequences:

- **No `<'p>` anywhere.** `Story` becomes owned/`'static`; web stops being
  self-referential; bevy's asset *is* the `Arc<Program>`.
- **No locks.** The VM steps one flow at a time, so `&mut World` +
  `&mut FlowLocal` for the step is exclusive access without `Mutex`/`RwLock`.
  Flows sharing a world **serialize by construction** (bevy: one advance system
  holding `ResMut<BrinkWorld>` iterating entities) — which is *correct*, since
  concurrent writes to a shared world var would race. Flows that want
  parallelism use an isolated policy (`default: Local`, no overrides) → the
  world layer is **empty** → no shared mutable state → parallel advance.
- **Fork snapshots are owned, not borrowed.** A child's base is a cloned
  persistent-map handle of the parent *as of spawn* (O(1), structural sharing),
  so a sandboxed watch flow holds **no borrow** on the live flow — the live flow
  is unaffected, and discard is just a drop. The only `Arc` touching mutable-ish
  state is inside the persistent map, where it buys the O(1) snapshot; the
  single-flow and fully-shared paths keep the local layer empty and never engage
  it.

## Spawn and drive API

The one surface every consumer touches. The **core hands back owned
`(FlowInstance, FlowLocal)` pairs and does not own flows** — the orchestrator
chooses storage (Story a collection, bevy components). That is what lets today's
`Story.instances`/`shared_instances` become *Story's* choice, not the runtime's.

```rust
// Create a world (resolves policy → ResolvedPolicy against program symbols; bad names fail here)
World::new(program: Arc<Program>, policy: WorldPolicy) -> Result<World, PolicyError>

// Spawn a ROOT flow over the world (primary story flow; a per-entity NPC)
World::spawn(&self, entry: FlowStart) -> Result<(FlowInstance, FlowLocal)>

// FORK a flow from an existing one (sub-conversation; watch/eval). Mode baked here, stored on the local.
World::fork(&self, parent: &FlowLocal, entry: FlowStart, mode: Mode) -> Result<(FlowInstance, FlowLocal)>

// Drive — transient bundle built per advance; constructs the ContextAccess routing view internally
FlowStep::new(&program, &tables, &mut world, &mut flow, &mut local)
    .advance()          -> Result<Line>        // one visible line
    .advance_to_end()   -> Result<Vec<Line>>   // to terminal (owns the line limit)
    .choose(i)          -> Result<()>
    .eval(target, args) -> Result<Value>       // function eval → value (the value-shaped case)

// Terminal ops on a FORK (root flows do neither — they persist):
commit(child: FlowLocal, parent: &mut FlowLocal)   // DEFERRED SEAM — see write-back
// discard = drop the (FlowInstance, FlowLocal)

enum FlowStart { Root, Address(Path) }   // "run from this knot/stitch"
enum Mode { Normal, Sandbox }            // Sandbox: world read-only (writes divert to local), discard-only
```

Design points:

- **Root vs fork are distinct constructors** because they take different inputs
  — a root needs only an entry (base = world); a fork needs the parent's
  `&FlowLocal` for the O(1) snapshot plus a `Mode`.
- **`Mode` is baked at fork time**, stored on the `FlowLocal`; the `FlowStep`
  routing-view constructor reads it. A sandboxed flow is sandboxed for life.
- **Value-shaped vs transcript-shaped** watches split here: `FlowStart::Address`
  → `advance*` → `Line`s; `FlowStep::eval` → `Value`. This is the Tier-0
  "invoke existing" surface (a function/knot with **literal** args); a computed
  arg (`damage(gold + 1)`) falls to Tier-1 fragment compilation.
- **A fork is a COMPLETE snapshot** — all flows *and* the resolver, nothing
  silently dropped. Today's `Story::clone` already copies all flows
  (`default`/`instances`/`shared_instances` are all cloned) but drops the
  **resolver** (`resolver: None`) — only because `Box<dyn PluralResolver>` isn't
  `Clone`. The new-model fix is to carry it as `Arc<dyn PluralResolver>` so a
  fork keeps localization. (In the new model `shared_instances` dissolves into
  empty-local-over-world flows; completeness is the contract, which keeps
  `Mode::Sandbox` sound.)

Each consumer, same three operations:

```rust
// Story facade — owns World + a flow collection
fn continue_single(&mut self) -> Result<Line> {
    FlowStep::new(&self.program, &self.tables, &mut self.world,
                  &mut self.primary.0, &mut self.primary.1).advance()
}

// bevy advance system — World is a Resource, flow is components
for (mut flow, mut local) in &mut query {
    let lines = FlowStep::new(&prog, &tables, &mut world, &mut flow, &mut local).advance_to_end()?;
    // trigger observer events per line
}

// watch/eval — fork sandboxed, run, discard
let (mut f, mut l) = world.fork(&live.local, FlowStart::Address("cellar"), Mode::Sandbox)?;
let preview = FlowStep::new(&prog, &tables, &mut world, &mut f, &mut l).advance_to_end()?;
// drop((f, l))
```

## `Story` decomposition

`Story` is on the oracle path (harness drives `Story::new` +
`continue_single*`), so it is **not** removed — removing it just forces web,
cli, and the harness to each reinvent the drive loop (which *is* `Story`-the-
facade). Instead:

- **Extract** the shared orchestration — drive-to-terminal, `Line` assembly,
  replay recording, locale application — into primitive-level ops **both `Story`
  and bevy call**, killing the two-implementations-in-lockstep duplication.
- `Story` **slims to a facade** over the core for web/cli/oracle. **Shared**
  flows become `FlowLocal`s over the default `World` (`default: World` policy).
  **Correction (2026-07-07, from F2):** the collapse is *partial*, not total.
  Because **policy is per-`World`** and `World` is single-owner with no locks, a
  *fully-isolated* flow (independent state) genuinely needs its **own** `World` —
  it cannot be a `FlowLocal` over the shared world, since one per-world policy
  can't make one flow private while another is shared. So isolated flows stay
  own-`World` (a `Story` with truly-independent flows holds multiple `World`s, or
  the consumer uses separate `Story`s). The original "isolated-vs-shared is *just*
  policy / everything dissolves into one `World`" over-claimed. The genuine open
  fork — **per-`World` vs per-flow policy** (the latter would let isolated flows
  share a world, at the cost of flows disagreeing on a unit's scope) — is deferred
  until a concrete consumer (F3 fork/spawn or bevy multi-entity) forces it; the
  shipped model is **per-`World`**.
- bevy **stops duplicating** and thins to ECS glue over the same ops; its
  fork-mode + commit (currently "designed, no API surface") come from the core.

## Oracle-safety strategy

The change lands in this order, each stage provable:

1. **Extract primitives behind existing seams, zero behavior change.**
   `ContextAccess` (already there) and a `ProgramLike`-style discipline let us
   pull `World`/`FlowLocal`/routing out with the VM untouched. `Story` rebuilt on
   them as a **pure refactor** — oracle byte-identical, **zero snapshot churn** —
   before anything new rides on it.
2. **Add scoping**, engaged only by a non-default `WorldPolicy`. The single-flow
   degenerate case (`default: World`) is the same code path as stage 1.
3. **Add sandbox mode + watch/eval** (the light feature) and **thin bevy** onto
   the shared ops.

At no stage does default single-flow behavior change; the ratchet holds
throughout.

## Watch/eval as a light feature

Once the core exists, watch/eval is a thin policy + entry-point layer, and it
tiers by whether the target already exists in the program:

- **Tier 0 — invoke existing (no compiler).** Start a sandboxed flow at an
  existing knot/stitch, or call an existing function/knot with **literal** args,
  over a cloned/COW local layer; collect value + transcript; discard. The invoke
  machinery already exists (`callFunction`, `goToPath`) — it just runs live
  today; the sandbox is the delta. Covers most real watches.
- **Tier 1 — arbitrary expression/content (compiler + overlay).** For genuinely
  novel fragments, the fragment-compile + program-overlay work from
  [speculative-eval-spec.md](speculative-eval-spec.md) §4–5, now running *on this core*
  (the sandbox flow is a `FlowLocal` in sandbox mode; the overlay is the program
  view). Everything in that spec still applies — it becomes the Tier-1 section.

The **externals `@kind` policy** (query live; effect/presentation
fallback-or-armed; async pending) from speculative-eval-spec §7 attaches here — it's
about how externals behave inside a sandboxed flow, orthogonal to what runs.

## Open questions (the sections we finish together)

*Resolved so far: uniform scoping; CoW layers; scope-determined write-back;
engine-supplied `WorldPolicy`; `ContextAccess` routing; sandbox mode;
ownership/lifetimes (Arc immutable, single-owner mutable, owned fork snapshots,
no `<'p>`); the spawn/drive API; `commit` as a deferred fork-only seam.*

1. **RNG seeding for spawned local streams** — where a flow-local RNG's seed
   comes from (derived from parent + entity id? fresh? host-supplied?) for
   determinism across spawns. The one remaining semantic question.
2. **The extracted-orchestration boundary** — the precise list of ops that move
   from `Story` into shared Layer-2 primitives (drive-to-terminal, `Line`
   assembly, replay, locale, function-eval, save/load, debug-snapshot) and their
   signatures, so both `Story` and bevy call them. Mostly an F1 implementation-
   planning task.
3. **Persistent-map choice** — *resolved:* start with **`alloc::BTreeMap` +
   `alloc::sync::Arc`** (frozen-base chain; core+alloc only, deterministic,
   cache-friendly at small sizes, zero external crate). Never engaged on the
   single-flow / fully-shared paths. Escalate to a **verified no_std+alloc**
   persistent map (`rpds`/`archery`, *not* `im`/`imbl` which are believed
   std-required) behind the `FlowLocal` API only if F3/F4 profiling shows
   fork-heavy watch churn matters. Chosen deliberately to keep the runtime's
   **no_std goal** (#434) open — the core must not take a std-only dep here.

## Phasing (milestones re-derived from the foundation)

The previously-filed #411 milestones (#429–#433) were the watch-first framing
and will be **re-derived** from this foundation once the spec is approved.
Expected shape:

- **F1** ✅ **DONE** — core-primitive extraction + `Story` migration (3 PRs
  #442/#444/#445, oracle byte-identical). `World`/`FlowLocal`/`ContextView`
  exist; `Story` rides on them. (Planned F1.4 dissolved: the
  `instances`/`shared_instances` collapse moved to F2, the Layer-2 drive-op
  extraction to F6.)
- **F2** ✅ **functionally DONE** (F2.1 #446, F2.2 #448) — `WorldPolicy` +
  `ResolvedPolicy` + policy-aware routing + **flat `FlowLocal` storage**
  (boundary decision **A**, 2026-07-07: F2 owns a *flat* per-flow override map so
  scoping is functional and testable on its own; F3 upgrades it to CoW).
  Single-flow (all-World default) stays byte-identical; scoping works end-to-end.
  The **`instances`/`shared_instances` collapse (planned F2.3) is deferred** — it
  hit the per-`World`-policy limit above (isolated flows need own `World`s, so
  they don't collapse into one); the current structure is correct and nothing is
  blocked. Revisit when a consumer forces the per-`World`-vs-per-flow decision.
- **F3** ✅ **DONE** (F3.1 #450, F3.2 #451) — CoW `FlowLocal` (frozen-base
  snapshot chain) + fork/sandbox/discard; `commit` shipped as a deferred
  `NotImplemented` seam.
- **F4** ✅ **DONE** (F4.1 #453, F4.2 #455, F4.3 #456) — `Speculation` +
  `KindTieredHandler` + web binding (shipped in npm 0.9.0).
- **F5** ✅ **DONE** (#461) — mechanism **B** per the speculative-eval-spec
  amendment (synthetic-symbol recompile + cache, not the overlay).
- **F6** bevy-brink thinning onto the shared ops — **see the F6 AMENDMENT
  below** for the final rulings and F6.1–F6.3 slicing.

F1 is the gate: nothing else starts until the decomposition is proven
behavior-identical.

## F6 AMENDMENT (2026-07-10) — final rulings and slicing

Design round with Ben (decision-log 2026-07-10). Four rulings supersede or
sharpen the corresponding sections above; #441's original text is stale in
places (corrections below).

**1. Policy stays per-World; scope is a property of the variable/knot.**
Per-flow policy (policy supplied at spawn) was examined and **rejected**:
per-entity privacy comes from each flow having its own `FlowLocal`, not its
own policy. Spawning a flow is just `(entry) → FlowInstance + fresh
FlowLocal` — no policy parameter. Exceptional per-entity behavior (a private
counter raising a global flag) is written **in ink**, where it's visible.

**2. Default is `World`; private is the marked, enumerated case.** This
supersedes the "local-by-default per-entity NPC" row in §The policy as the
*recommended* configuration: plain `VAR` must keep meaning shared (ink
compatibility, oracle anchoring), so the language feature will mark the
*private* case — and default-World is the only default that survives that
transition without flipping. Hosts enumerate private names over a World
default. Policy construction is shaped **base ⊕ host-overrides** from day
one: base is empty today, compiler-emitted later (the language epic — a
flow-private storage class for VARs + knot marking, scope bits carried in
the compiled `Program`; the *only* place `brink-format` eventually changes).

**3. Knot scope is subtree-inclusive.** `ResolvedPolicy::resolve` today maps
an override name to exactly one `DefinitionId` — but sequence/cycle/stopping
counters key **interior container ids**, and idiomatic per-entity memory is
carried by visit counts (`{ Halt! | Back again? }`, `* {not intro} …`). A
knot override must cover its whole definition subtree or the motivating
use case silently half-breaks. Fixed in F6.1. The all-World fast path
short-circuits before any of this — oracle-safe.

**4. Entity durability is state-only via per-flow `SaveState`.** Each flow's
durable state round-trips as an ordinary name-keyed `SaveState`
(`save_state`/`load_state` lifted off `Story` to any flow's context); a save
= one `SaveState` for the shared World + one per entity, composed host-side.
No format change; a paused flow resumes from its knot entry, not mid-line.
bevy's `commit_from`/`commit_progress`/`commit_globals_only` are **deleted,
not promoted** — they compensated for the full-`World`-clone-per-flow model,
which F6 removes; no absorb/merge machinery exists or is needed.

**Slicing** (each PR oracle-gated where it touches `brink-runtime`):

- **F6.1** (`brink-runtime`, oracle-gated): extract one shared
  drive-to-terminal op replacing `Story::continue_maximally_impl`'s loop and
  bevy's four duplicates; reconcile bevy's misnamed `STEP_LIMIT` (10_000
  *lines*, `StepLimitExceeded`) onto core `LINE_LIMIT` semantics; lift
  `save_state`/`load_state` off `Story`; subtree-inclusive knot-scope
  resolution.
- **F6.2** (bevy-only): `BrinkContext` holds `FlowLocal` (today a full
  `World` clone); advance system builds `ContextView` over shared
  `BrinkGlobals` + entity `FlowLocal`; policy installed at plugin setup
  (base ⊕ overrides); delete the drive loops, `ReplayRecorder` drive
  wiring duplicates, and `commit_*` helpers.
- **F6.3** (bevy-only): per-entity `SaveState` save/load via the F6.1 lift.

**#441 corrections:** `apply_locale` calls **stay** — the locale overlay is a
pure line-table transform unrelated to scoped state, and the core offers no
replacement; deleting them would delete localization. `BrinkGlobals` already
wraps `World` — no change needed there.

## Decisions (to log on approval)

- Uniform scoping of all story-state; execution-state intrinsically flow-local.
- CoW layered local state; scope-determined write-back; commit is the single
  explicit opt-in for local write-back.
- Engine-supplied `WorldPolicy`, resolved into the `World`, ink untouched.
- Core-primitive layer beneath both `Story` and bevy; `Story` decomposed to a
  behavior-preserved facade; bevy thinned onto shared ops.
- watch/eval demoted to a light feature (sandbox policy + entry points) on the
  core; speculative-eval-spec becomes its Tier-1 section.
- All of it ships in 0.9.0; behavior-preserving-refactor-first keeps the oracle
  pinned.
