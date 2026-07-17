# Flow suspension & `await` — the FlowFrame model

Status: **DESIGNED 2026-07-16, implementation parked** (post-T2; its
own milestone when scheduled — format + compiler + runtime + host
work). Rulings in `docs/decision-log.md` ("FlowFrame suspension +
await"). Companions: `docs/effects-spec.md` §12–§13 (the wake
contract this plugs into), `docs/t1c-spec.md` (fn-value conditions),
`docs/modules-spec.md` §5 (the rehydration machinery frame drift
rides). Tracked at #889.

## 1. Why this exists

Two needs converge on one model:

- **Yielding flows**: ink that sleeps and wakes —
  `~ while await alarm_raised { … }` — ambient NPC logic written as
  ink, parked at zero cost between wakes (the §13.1 wake contract).
- **Durable save-at-any-park**: today's `SaveState` carries world
  state only (globals/visits/RNG — verified: no position or callstack
  fields). A flow parked at a *choice* mid-tunnel has no durable
  representation. Games need one regardless of awaits.

The FlowFrame is the single suspended-state representation serving
both.

## 2. The FlowFrame — RULED

A parked flow serializes as:

1. **current container id** (name-stable),
2. **return stack**: `Vec<DefinitionId>` of tunnel-return containers
   (name-stable; depth-capped — a park-depth limit, sibling of the
   step limit),
3. **frame record**: a compiler-synthesized, **name-keyed** record
   holding every local that crosses a yield — a plain `Value`,
   serialized by the existing encoders,
4. **wake policy**: await-site id + condition fn token (+ host wake
   source), both name-stable.

**No instruction offsets, ever.** Recompile-stability rides
container/DefinitionId identity — the same contract as saves,
`#@was`, and fn tokens. This is the Rust-async insight (suspended
computation = a state-machine *value*) at ink's granularity, built
from save-grade materials.

## 3. `await` — RULED syntax

Statement position, logic blocks only (the seam rule keeps narrative
out of hot loops). The facility doctrine demands syntax: an intrinsic
call that split the turn would lie about its call shape.

```ink
~ await gold > 100            // one-shot (wake_once semantics)
~ await some_fn_value         // dynamic condition: fn(): bool value
~ while await alarm_raised {  // persistent; exits when the host
    // bounded work            //   cancels the policy (false arm)
}
```

- **Direct-expression conditions** capture as compiler-synthesized
  pure fns. Their identity = the await site's synthesized
  resume-container path — site-stable, so the general anonymous-fn
  identity problem does not apply here. Purity is enforced by the
  effect row (read-only or compile error via the exceedance
  machinery); the row IS the wake map's dependency set.
- **`while await` desugar**: yield-with-policy → (waking IS
  condition-true, per the wake contract — the bool never
  materializes) → body → loop. Host cancels the policy → the false
  arm → clean loop exit.
- Host wake *sources* (next-frame etc.) remain host-side API
  (`wake_when`, §13.1); an ink spelling for them is PROPOSED-only.
- **Mid-expression `await` is permanently out** (statement only).

## 4. Composition: tunnels await, functions never — RULED

Ink's existing two-level structure is the color boundary:

- **Functions** (expression-level): permanently synchronous. Aligns
  with purity, effects, and expression semantics. No
  colored-function virality can exist — the "color" is a distinction
  ink authors have always had.
- **Tunnels** (statement-level, name-stable returns): the awaiting
  composition unit. `-> patrol_until_alarm ->` is a reusable awaiting
  helper; tunnels call awaiting tunnels; the FlowFrame return stack
  is exactly the tunnel-return chain.

## 5. Locals: auto-promotion with spill-on-park — RULED

- Locals that cross a yield are compiled into the frame record —
  `for` iterators included; awaits inside loops just work. The model
  in one sentence: **locals in awaiting scopes live in the flow's
  frame.**
- **Spill-on-park**: while running, promoted locals occupy ordinary
  VM temp slots — **zero hot-path overhead**. At a park, the k
  crossing locals (statically known per site, typically 1–5) move
  into the frame record (Arc bumps — a large collection costs one
  refcount, COW handles the rest); on wake, the symmetric restore.

## 6. Cost model — RULED acceptable

| Event | Cost |
|---|---|
| Running (any flow) | zero |
| Park / wake | O(k) value moves + one bounded pure-fn eval on wake |
| Parked, dependencies quiet | zero (skipped by Collect) |
| Non-awaiting flows | zero everywhere; no frame is synthesized |

Memory: a parked flow ≈ low hundreds of bytes (return stack tens of
bytes; frame record k Arc'd values; policy tens of bytes). A
thousand parked ambients ≈ sub-MB; saves grow by tens of KB per
hundred parked flows. Compile: per-site liveness pass + one struct
shape + one resume container (µs per def). Format: one new
suspended-flow section, section-locally versioned, absent for flows
parked at plain boundaries.

## 7. Drift and pathology handling — RULED

- **Frame-shape drift** (author edits a tunnel; crossing-locals set
  changes vs a save): frame records are name-keyed and ride the
  standard rehydration machinery — missing field → default with
  report, extra field → dropped with report, renamed local → treated
  as missing. Tolerant-of-patches, never silent, never UB.
- **Author hoards** (large distinct values in crossing locals ×
  many flows): real memory, authoring concern; the #821
  snapshot-retention metric is the detector.
- **Recursive awaiting tunnels**: park-depth cap (guard-against-
  unbounded-growth rule).
- Synthesized resume containers are invisible to visit-count queries
  (compiler-internal structure).

## 8. What this buys

Serializable, recompile-stable coroutines — Unity coroutines aren't
serializable, Lua coroutines definitively aren't, Rust futures aren't
build-stable. Sleeping NPCs at hundreds of bytes each, saveable
mid-sleep, surviving patches. This is the §1 value-model pitch
("every script is pausable, saveable, replayable") extended to its
logical end, and the sharpest differentiator versus "just embed Lua."

## 9. Implementation sketch (when scheduled — its own milestone)

1. **FS-1 format**: the suspended-flow section (FlowFrame encoding,
   writer+reader+round-trips per the standing rule).
2. **FS-2 compiler**: per-site liveness, frame-shape + resume-
   container synthesis, `await` grammar/HIR/lowering, the purity
   gate on conditions.
3. **FS-3 runtime**: spill/restore, park-depth cap, save/load of
   FlowFrames, rehydration drift handling.
4. **FS-4 host**: FlowSleep integration (the policy IS §13.1's),
   dormant spawn, `wake_once`, cancellation → false.
5. **FS-5 tail**: book chapter, corpus (an ambient-NPC example
   program), IDE (await-site hover: frame shape + dependency set).
