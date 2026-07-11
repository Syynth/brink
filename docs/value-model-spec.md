# The value model — data, identity, and sharing in Tier-1 brink

Status: **DRAFT for the value-model design round** (epic #397; follows
phase 0). Nothing here is ruled yet. The lead candidate is presented
with its alternatives honestly costed; §11 (functions as values,
closures) is deliberately open pending the round's remaining
discussion. Rulings land in `docs/decision-log.md` and this header
flips to "ratified" section by section.

Context: `docs/scripting-substrate-spec.md` (the query-shaped compiler
this model compiles under), the #397 sizing ("the value model is the
single biggest chunk"), and the 2026-07-11/12 design exploration on
#397's thread.

## 1. Stakes

Brink's pitch against "just embed Lua" is: **every script is pausable,
saveable, replayable** — park/resume on externals, name-keyed
`SaveState`, deterministic journal replay, speculation. All of that is
downstream of one property: *script state is serializable data, always*.
Tier-1 growth (arrays, maps, richer logic) is where that property
either survives or quietly dies. This spec is the survival plan.

## 2. The model: data is values, identity is names

**Lead candidate.** Brink stays a value-semantics language:

- **Data is values.** Collections copy on assignment/passing like every
  existing brink value. No aliasing, no object identity, no cycles —
  by construction.
- **Identity is names.** Mutable identity lives in *named cells*
  (VARs/temps — already governed by scope policy, `#@local`, save,
  replay) and in *symbolic tokens* (divert targets today; host handles,
  §8). `ref` is the one aliasing construct, and it references cells,
  never heap objects — exactly ink's existing `ref`/`VariablePointer`
  design, extended rather than replaced.

Lineage: Clojure's semantics (values + explicit identity constructs),
Swift's implementation (COW collections in production games), Rust's
discipline (values + visible borrows). Ink LISTs already behave this
way; Tier-1 collections join the substrate's own model.

**Rejected alternative — reference semantics (Lua tables).** Costed and
declined (pending ruling): shared mutable heap objects require a
GC-or-refcount decision, cycle handling, identity-preserving
serialization, an object-graph protocol at the host/wasm boundary, and
re-earning every guarantee in §1. Aliasing would also invalidate most
of §9's compiler elisions and make §6's sharing optimizations
*semantic* changes instead of invisible ones. The trade is Lua
familiarity, not capability.

## 3. The load-bearing invariant

> **Sharing is unobservable.** Programs and hosts can never distinguish
> two structurally equal values — no pointer identity, no refcounts, no
> copy timing, no `is_same()`. Ever.

Every optimization in this spec (§5–§7, §9) is the compiler/VM cashing
this invariant in. Exposing identity once makes each of them a
breaking change forever. This is the first ruling to ratify.

## 4. Collections

- `Value::Array(Arc<Vec<Value>>)` and `Value::Map(Arc<OrderedMap>)`.
  `Value` stays small; clone is a refcount bump.
- **Map keys**: scalars only in v1 (int, string, bool, list items —
  exact domain TBD). Iteration order is **specified** — RULING NEEDED:
  insertion-order vs sorted. (House determinism religion satisfied
  either way; insertion-order matches author intuition, sorted matches
  BTreeMap habits.)
- **Equality is structural**, with an `Arc::ptr_eq` fast path (same
  snapshot → instant true). NaN-bearing collections never compare
  equal (IEEE semantics compose structurally); harmless, stated.
- Hashing is not author-visible.
- Constant literals intern into a **literal pool** in the format
  (generalizing the existing `list_literals` + `PushList(idx)` design),
  loaded as Arc bumps — zero runtime allocation; pool entries are
  unpoisonable because COW copies on any author mutation.

## 5. Performance model (Arc-COW mechanics)

- Assign/pass/stack ops: O(1). Reads: pointer deref. Mutation while
  unique: in place. Mutation while shared: one O(n) node copy, then
  unique again.
- **The one cliff** — mutate-while-shared in a loop — is neutralized in
  the VM we own: variable read-modify-write compiles to *take out of
  slot → `make_mut` → write back*, so loop-append is O(1) amortized;
  last-use analysis (§9) makes it static where the compiler can see it.
- Persistent structures (HAMT/RRB) are **not** used: at game-scale
  collection sizes their constants lose to flat COW vectors/maps, and
  they'd bloat the wasm build. Revisit only with profiling evidence.
- Costs that stay: deep equality on genuinely different-but-similar
  values; serialization duplicates shared substructure (dedupable at
  save time, §6).

## 6. Sharing optimizations (all optional, all invisible under §3)

- **v1 ships**: the `ptr_eq` equality fast path. Nothing else is
  required for correctness.
- **Ref collapsing** (specified, optional): after a deep equality
  returns *true*, collapse the operands' sources to one Arc —
  union-find path compression for values. Sites: a fused
  `EqVars(a, b)` opcode (peephole over `LoadVar a; LoadVar b; Eq`);
  store-time keep-old-Arc when writing an equal value; save-time
  content-hash dedup (recursive, catches equal subtrees). Survivor
  choice is deterministic (keep left/older). Precedent: hash-consing,
  V8 string internalization, salsa backdating. Tension: collapsing
  increases sharing and can re-trigger COW copies on later writes —
  net win for compare/read-heavy values, a wash for soon-mutated ones;
  documented, not feared. Note this is *only sound under value
  semantics* — with reference semantics it would create aliasing.
- **Quiescent-point sweeps**: `-> DONE` is a language-defined
  safepoint (stack empty, frames dead, only the cell forest + transcript
  live) and the least latency-sensitive moment in the loop. Optional
  work there: recursive collapse/dedup pass, locality rebuilds
  (`shrink_to_fit`, re-interning), cache eviction (salsa LRU,
  transcript pages). Granularity: per-flow at each flow's `Done`
  (its `FlowLocal` is quiescent), World-wide only at host-declared
  all-flows-terminal. Host-held snapshots need zero coordination:
  refcounts keep old allocations alive until the host drops them —
  no stop-the-world exists in this design. `-> END` is the same
  safepoint with a bigger license.

## 7. `ref` and path projections

- v1: `ref` stays **cell-level** (ink's current rule).
- **Path refs** (`ref npc.inventory[3]`) are specified as **symbolic
  projections** — (cell, path segments), never interior pointers.
  Reads walk the path; writes desugar to read-modify-write on the root
  cell (Swift `inout` copy-in/copy-out lineage). Consequences: COW,
  scope routing, speculation isolation, and collapsing all survive
  untouched; projections serialize like `VariablePointer` (a save
  mid-call with a live projection works); exclusivity checking is
  local and syntactic (both paths visible at the call site).
- **Borrow analysis is an optimizer, not a gatekeeper**: soundness
  never depends on it (fallback is per-write path-walking RMW); when
  the compiler proves exclusivity it may hold the `make_mut` spine
  across a region. It can be incomplete and arrive later.
- RULINGS NEEDED: index expressions snapshot at `ref` creation
  (proposed: yes); path invalidation under a live projection is a
  defined runtime error (proposed: error, not clamp); overlapping
  projections policy (proposed: immediate write-through order —
  deterministic without any check).

## 8. The host boundary: snapshots and handles

- **Snapshot-only contract**: externals receive and return *values*
  (O(1) Arc snapshots); the host may never retain a mutable handle
  into script state. Locked into the bevy-brink binding contract and
  wasm boundary before Tier-1 ships.
- **Host resources** (entities, audio instances, assets, timers) enter
  the script world as **`Handle` tokens** — opaque `{kind, id}`
  scalars with value semantics, serializable, compared by token.
  **No live pointer ever lives in a `Value`.** Dereferencing happens
  only host-side against the host's registry.
  - *Save/load*: tokens serialize; a **rehydration hook** at load maps
    saved tokens → live resources or dead (bevy's `EntityMapper` is
    the native implementation). Dead handles are never UB — a binding
    dereferencing one returns its declared failure value; optional
    `is_valid(h)` world-query binding.
  - *Replay*: the journal records returned tokens; replay returns the
    recorded token — determinism at the token level, rebinding at the
    boundary.
  - *Kinds* live in the external manifest (the existing host
    semantic-type vocabulary the analyzer already polices), giving the
    future type checker `handle<AudioInstance>`-level checking, and
    giving the Track-B capability manifest its nouns.

Summary dogma: the script world holds only **values and names**; every
name (cell, divert target, handle) is symbolic and serializable, and
every name is re-bound at a defined seam (scope policy; linking;
rehydration).

## 9. What the compiler may assume (the guarantees contract)

The elisions this model legalizes — literal pooling + compile-time
hash-consing of constants, move elision via last-use analysis,
uniqueness-proven in-place mutation opcodes, CSE and constant folding
with syntactically visible effects, salsa-interned pools — are sound
only while ALL of the following hold. Breaking any of them later is a
breaking change to the compiled format and the optimizer:

1. **No identity leaks** (§3), to authors or hosts.
2. **`ref` is cell-level; paths are projections** — no interior
   pointers, ever (§7).
3. **Host boundary is snapshot-only** (§8).
4. **Builtins may fold; host externals never fold** (a `pure` binding
   is pure within a run, not deterministic across runs).
5. **Deterministic iteration + bit-exact folding** — compile-time
   evaluation must equal runtime evaluation, including float semantics
   (the C#-compat arithmetic lessons apply to the folder).
6. **Opcode/format surface reserved once**: load/move/mutate-in-place
   variants + the literal-pool section land in one planned
   `brink-format` version bump.

## 10. Resumability audit

Each #397 superpower, checked against this model: park/resume state is
values (serializable trees, including parked stacks); state-only saves
stay name-keyed trees; journal replay stays bit-deterministic
(specified iteration order, no identity to diverge, recorded external
results including handle tokens); live path projections serialize
symbolically; speculation cannot mutate live state by construction;
collapse/compaction are invisible to all of the above. The only place
resumability could have leaked — foreign identity inside values — is
closed by §8's token design.

## 11. Functions as values, closures — RULED 2026-07-11 (cross-flow ref-capture binding still open)

- **Function references**: symbolic tokens (`DefinitionId`, the divert-
  target move) — serializable, replayable, compared by token.
- **Closures are values**: `{fn: token, env: row}`. The environment is
  a row of entries, each **`val` (snapshot, the default) or `ref`
  (explicit opt-in per name, capture-list style)** — brink already
  spells aliasing `ref`, so this is the house accent.
- **`ref` captures are restricted to durable cells** (VARs/flow state;
  compile error on temps — the analyzer knows storage classes). This
  deletes the upvalue problem outright: no heap-promoted cells, no
  closure-outlives-frame machinery, no identity objects.
- **The row is analyzer-transparent, author-opaque**: the type/effect
  system sees `{gold: ref<int>, cfg: val<map>}`; the program can never
  reflect on, index, or compare environments (else the env becomes an
  identity-bearing table).
- **Effects**: `ref` generally needs effect polymorphism (a row
  variable bound at the call site — ink's existing `ref` params need
  this anyway); closures bind their row variables at **creation site**,
  so every closure value has a fully concrete effect row. A closure
  handed to the host as a callback carries a knowable access set
  (§11b).
- **Serialization**: env rows serialize symbolically (values as trees,
  refs as cell names); closures live in saves and journals like any
  value.
- RULING NEEDED: a `ref`-captured `#@local` cell crossing flows —
  resolve through the *executing* flow's scope view (late binding;
  consistent with names-not-identities; proposed) vs pinned to the
  creating flow (requires flow identity inside a value; disfavored).

## 11b. Effects & ECS scheduling — OPEN (sketch)

After §2, effects are syntactically total: state changes only via cell
writes, declared `ref`s, and declared externals. So the compiler can
infer a per-definition **effect row** — `{cell reads, cell writes,
external calls by kind}` — by transitive closure over the call/divert
graph: an `effects(def)` salsa query with Eq cutoff, next to
`signature(def)`. Function values stay analyzable because they are
tokens (§11); closure env refs bind at creation site.

The ECS join: the Track-B capability manifest declares what each
external *touches* in ECS terms (component/resource access); bevy-brink
joins effect rows × manifest into a per-entry-point Bevy access set,
known before the flow runs. Payoffs in ascending ambition: (1) parallel
flow scheduling from access-disjoint batches; (2) prefetch/batched
world-query resolution collapsing park/resume round-trips; (3)
**reactive sleep** — a parked flow's condition dependencies tell the
host exactly which change-detection wakes it; ambient NPC flows become
near-free.

Stability tension (same shape as the type firewall): inferred rows
drift with bodies; hosts want stable contracts. Proposed: inferred
everywhere internally, **declared/frozen at flow entry points** via the
`#@` channel, checked against inference, error on drift. Annotation =
firewall, absence = inferred.

## 11c. Error handling — RULED 2026-07-11

v1: the script side is **infallible** — no exceptions, no unwinding, no
in-language error values. Runtime faults (bad index, dead handle,
invalid projection) are defined, deterministic outcomes: total
operations with specified failure values where defined, otherwise
turn-terminating diagnostic events surfaced to the host and recorded in
the journal (replays fail identically). Result-shaped recoverable
errors are a later, demand-driven addition that joins the effects
system (§11b) without grammar changes.

## 12. Rulings needed (round checklist)

1. Ratify the model (§2) and the invariant (§3).
2. Map iteration order + key domain (§4).
3. Path-projection trio: index snapshot, invalidation, overlap (§7).
4. Host boundary contract + Handle design (§8) — including the
   rehydration hook's shape in bevy-brink.
5. The compiler guarantees contract (§9) as standing law.
6. ~~Functions/closures (§11)~~ RULED 2026-07-11 — except the
   cross-flow `ref`-capture binding question (late binding proposed).
7. Effects (§11b): effect rows, the manifest join, and the
   declared-at-entry firewall.
8. ~~Error handling (§11c)~~ RULED 2026-07-11 (ink-side infallible;
   host events).
9. v1 scope line: which of §6's optimizations ship v1 (proposed:
   `ptr_eq` only) vs specified-for-later.

## 13. Non-goals

GC of any kind; persistent/HAMT structures; interior pointers;
author-visible hashing or identity; non-scalar map keys (v1); exposing
refcounts or pool indices; any semantics that varies between the
collapsed and uncollapsed states of the same program.
