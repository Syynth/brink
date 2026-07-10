# The State Model

A running story has two kinds of mutable state, and they have different owners.

- **Execution-local state** — the call stack, threads, output buffer, pending
  choices, and program counter. A flow *is* these; nothing ever shares them.
  They live on `FlowInstance`.
- **Story-state** — global variables, visit counts, turn counts, the turn
  index, and the RNG stream. This is the state ink lets content read and write:
  `VAR gold = 10`, `{shrine}` (a visit count), `RANDOM(1, 6)`.

The interesting design is in the second kind. Story-state lives in a `World`,
and **every unit of it can be homed to the shared world or made private to one
flow**, under a single policy. That one idea explains named flows, per-entity
NPC dialogue, and the side-effect-proof "what would happen if…" evaluation the
[Speculation](../embedding/speculation.md) chapter covers.

> This is a deeper layer than [The Execution Model](./execution-model.md), which
> is about *stepping* a story. Here we're concerned with *where the state lives*
> while it steps. If you only ever drive a single `Story`, the model collapses
> to "one bag of globals" and you can skip ahead — but the moment you run more
> than one flow, or want to preview a branch without committing to it, this is
> the chapter that makes those features make sense.

## World and FlowLocal

Story-state is split across two types:

- **`World`** — the shared layer: `globals`, `visit_counts`, `turn_counts`,
  `turn_index`, and the RNG (`rng_seed` + `previous_random`). A write to a
  world-scoped unit is visible to every flow sharing that world immediately.
- **`FlowLocal`** — a per-flow override layer. It holds this flow's private
  values for the units the policy homes to `Local`, and falls through to the
  `World` for everything else.

A read walks a chain — the flow's own overrides first, then (for a forked flow)
a frozen snapshot of its parent, then the `World`, and ultimately the program's
declared defaults. The first hit wins. A `FlowLocal` that overrides nothing (the
common case) contributes no reads, so every access falls straight through to the
`World`.

The runtime's step functions take `&mut impl ContextAccess` and never touch
these fields directly. A `ContextView` — built transiently for each step over
`(&mut World, &mut FlowLocal)` — implements that trait and does the routing: on
every `get_global` / `set_global` / `visit_count` / `increment_visit` /
`draw_random`, it consults the policy and sends the access to the right layer.

## The policy

Which units are shared and which are private is declared by a `WorldPolicy`:

```rust
# extern crate brink_runtime;
use std::collections::BTreeMap;
use brink_runtime::{Scope, WorldPolicy};

// A per-entity NPC: everything private by default, but `gold` and the
// `shrine` knot's visit count are shared world state.
let policy = WorldPolicy {
    default: Scope::Local,
    overrides: BTreeMap::from([
        ("gold".to_string(), Scope::World),
        ("shrine".to_string(), Scope::World),
    ]),
    turn_index: Scope::Local,
    rng: Scope::Local,
};
# let _ = policy;
```

`Scope` is just `World` or `Local`. The `default` field is the whole
global-by-default vs local-by-default dial; `overrides` names the exceptions —
each name is matched first as a global variable, then as a knot/stitch path.
`turn_index` and `rng` are scoped as single scalar units of their own.

Four policies cover the useful cases:

| Use case | Policy |
|----------|--------|
| Single-flow story | `default: World`, no overrides — **identical to a classic single `Context`** |
| ink concurrent flows | `default: World`, no overrides — flows share everything, writes visible across |
| Per-entity NPC | `default: Local`, a few `World` overrides for shared save data |
| Fully isolated branch | `default: Local`, no overrides — nothing shared |

The policy **comes from the host, not from ink**. The same `.ink` file can be
instanced single-flow by one program and per-entity by another; the compiled
artifact carries no policy. It is resolved **once**, when the `World` is created,
against the program's symbol table — an override naming a variable or knot the
program doesn't declare fails right there, as a `PolicyError`, not at runtime:

```rust
# extern crate brink_runtime;
# use brink_runtime::{Program, World, WorldPolicy, PolicyError};
# fn demo(program: &Program) -> Result<(), PolicyError> {
// Resolves the policy against `program`'s symbols; bad names fail here, once.
let world = World::new(program, &WorldPolicy::default())?;
# let _ = world;
# Ok(())
# }
```

`WorldPolicy::default()` is all-`World` — the degenerate policy where every unit
is shared and there is no local layer. That is byte-for-byte the behavior of the
old single `Context`, and it is the one the oracle corpus runs against, so it is
the anchor the whole model is built not to disturb. `Story::new` uses it, which
is why an ordinary single-story consumer never sees any of this machinery.

> **A determinism caveat, stated plainly.** A `World`-scoped RNG stream
> interleaves draws from every flow sharing the world in execution order, so its
> output depends on how flows are *scheduled*. Flow-local RNG has no such
> dependency. World-scoped RNG is expressible — a shared "deck" mechanic wants
> it — but scoping it that way is opting into scheduling-order semantics.

## Sharing a world, or not

Multiple `FlowInstance`s can run against one `World` — this is how ink's
concurrent flows work, where one flow's writes are immediately visible to the
others. Or each flow can hold its own `World`, for independent playthroughs or
branch-and-rollback. The runtime doesn't prescribe which; the step functions
take `&mut World` wherever it lives.

This is exactly the difference between the two named-flow spawners in
[Named Flows](../embedding/named-flows.md): a shared flow is a `FlowLocal` over
the story's default `World`; an isolated flow owns a separate `World`.

Because the VM steps one flow at a time, `&mut World` + `&mut FlowLocal` for the
step is exclusive access with no locks. Flows sharing a world serialize by
construction — which is correct, since concurrent writes to a shared world
variable would race. Flows that want real parallelism use an isolated policy, so
there is no shared mutable state to contend over.

## Fork and sandbox

A `FlowLocal` can be **forked**: the child gets an empty override map over a
frozen, structurally-shared snapshot of the parent as of the fork. The snapshot
is O(1) — nothing is eagerly copied — so the child sees the parent's state at
fork time, the parent's later writes don't leak in, and discarding the child is
just a drop.

Forking takes a `Mode`:

- **`Mode::Normal`** routes by policy, exactly as above.
- **`Mode::Sandbox`** treats *every* unit as local: the shared `World` becomes a
  read-only base. Reads still fall through to the world's live values, but writes
  — even to world-scoped units — land only in this flow's own overrides. Nothing
  outside the flow is touched, so you can run it against current state, observe
  what it produces, and throw it away.

`Mode::Sandbox` is the side-effect-proof primitive behind
[Speculation](../embedding/speculation.md) — evaluating "what would this choice
do?" without the doing. Every construction path that predates forking produces
`Mode::Normal`, which is what keeps the default single-flow story unchanged.

## What's implemented

The split, the policy, policy-aware routing, copy-on-write flow-local storage,
fork, and sandbox mode are all live. One piece is a deliberate seam: `commit`
(folding a fork's private writes back into its parent) is **defined but not yet
implemented** — it returns a `CommitError`. Nothing in the current release needs
it: root flows persist their local state for their lifetime and never commit,
and sandboxed evaluation always discards. See the
[scoped-flow-state spec](https://github.com/Syynth/brink/blob/main/docs/scoped-flow-state-spec.md)
for the full design, including the commit-conflict policy that a future release
will settle.
