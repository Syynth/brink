# Tier-1 roadmap — making the value model real

Status: logistics plan (2026-07-11), following the ratified
`docs/value-model-spec.md` and the phase-0 substrate
(`docs/scripting-substrate-spec.md`). Ordering ruling: **format schema
on paper → runtime value core → one format bump → compiler surface** —
every intermediate merge oracle-neutral by construction.

## Milestones

### T1a — value core (runtime-led)

The semantic foundation. No grammar changes; oracle byte-identical
throughout (new opcodes sit inert until T1b emits them).

1. **Format schema RFC** (paper, reviewed before any bytes): wire
   representation for `Value::Array`/`Value::Map`, the generalized
   literal pool (superseding `list_literals`' special case), and the
   reserved opcode surface — load/move/mutate-in-place variants,
   collection ops — sized for §9's one-bump rule.
2. **Runtime value core**: `Value::Array(Arc<Vec<Value>>)`,
   `Value::Map(Arc<OrderedMap>)` (insertion-order; keys int/string/
   bool), COW mechanics with the take→`make_mut`→write-back RMW
   discipline, structural equality with the `ptr_eq` fast path.
   Tested via hand-assembled bytecode/StoryData builders — no parser
   involvement.
3. **State plumbing**: SaveState/journal tree serialization,
   wasm JSON boundary, bindings `Value` conversions (snapshot-only
   contract enforced at the bevy-brink surface).
4. **The format bump** (VERSION 4) rides the runtime PR — schema
   validated by a working implementation before freezing.

### T1b — compiler surface: collections + logic growth

Grammar → HIR → LIR → codegen for what T1a can execute: collection
literals and indexing, multi-line `~` blocks, `for`/`while` loops,
block-scoped locals, stdlib slice 1 (len/push/insert/remove/keys/
values/contains). Includes the **strict-ink mode design note** (how
extension syntax is feature-gated so the oracle-anchored subset stays
checkable — the #397 open question, cheap under the query graph's
shared prefixes). First emission of T1a opcodes; brink-native test
corpus grows a Tier-1 wing (no oracle exists for new surface — spec +
property tests carry it, per the standing divergence discipline).

### TM — typed mode (inserted 2026-07-12, #605 ruling)

Strict types, inferred internally (mono-HM per SCC), declared at
boundaries; inline annotation syntax (brink dialect); structs
(`Value::Record`) land here. Spec: `docs/typed-mode-spec.md`. Slices
TM-1..TM-5; sequencing ruling: **types → T1c → effects**. T1c below is
held until TM's spine lands (its rulings are logged and stand).

### T1c — functions as values, partial application (formerly "closures")

Function tokens; closure values `{fn, env}` with capture-list grammar;
`val`/`ref` env rows (durable-cell restriction enforced by the
analyzer); creation-site effect binding recorded for §11b's later use;
host callback invocation surface in bevy-brink.

### T1d — handles & host boundary

`Value::Handle{kind, id}`; manifest handle kinds (extending the host
semantic-type vocabulary); bevy-brink rehydration hook
(`EntityMapper`-based) + dead-handle policy + `is_valid`; journal
records tokens; snapshot-retention dev metric.

### T1e — path projections

Symbolic (cell, path) projections per spec §7: grammar, the three
ratified semantics (index snapshot at creation, invalidation as
turn-terminating fault, immediate write-through), serialization.
Borrow-analysis spine-holding is an *optimization backlog item*, not
part of this milestone.

### T2 — effects round (design first)

§11b detailed design: effect-row inference queries, the #@ entry-point
firewall, manifest access-set join, bevy scheduling/prefetch/reactive-
sleep. Own design round with rulings before any implementation.

## Working method

- Spine items (format bump, runtime value core, T1b codegen) run as
  single reviewed agents, oracle-gated, sequenced.
- Mechanical items (stdlib functions, test-corpus growth, docs) run as
  pump waves once the spine of each milestone lands.
- Every milestone ends with scope reconciliation against this doc.
- The type checker is deliberately absent above: it arrives after T1b
  on the `signature` firewall, scoped by its own short round (gradual;
  annotation = firewall, absence = dynamic — already ruled).
