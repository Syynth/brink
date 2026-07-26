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

## 10. Host surface & rehydration — RULED 2026-07-18 (FS-3 design round)

Ruled with the maintainer against two real consumers (a SpacetimeDB
server module + React/Three client, and an RPG Maker MZ plugin), both
manual (non-bevy) hosts.

### 10.1 Flow-addressed consumption

- **`continue` lives on the flow, not the story.** Story-level drive
  methods are sugar for the primary flow. Spawned/ambient flows are
  addressable handles; each has its own `Line` stream.
- **`Line::Suspended { text, tags }`** is a first-class per-flow
  terminal variant. Text accumulated before the park **flushes with
  it** into that flow's stream (parks are turn boundaries; pre-await
  text describes the pre-wait state and must not be held hostage).
- **Waking never auto-continues.** `wakeCheck()` reports runnable
  flows; the host drives them when it wants output (a reducer decides
  which transaction produces story output).

### 10.2 wakeCheck & dirty-tracking

- `wakeCheck()` (FlowInstance + web wrapper) re-evaluates **dirty**
  parked conditions and returns the woken flow ids. Empty = ~free.
- Dirtiness comes from the condition's read-set (the effect rows):
  host `setGlobal` and flow writes mark readers dirty. A write to a
  flow's `#@local` dirties only that flow's conditions.
- **Conditions are always evaluated in-context** — against the owning
  flow's environment (world + its locals), never a bare world view.
- **Pure manifest externals are legal conditions** (the purity gate
  judges their declared row). Conditions reading a host external are
  **always-dirty** (the host's world is opaque); finer invalidation
  hints are a compatible later addition, not v1. This is the manual-
  host analogue of §13's host-source wake — the FS-1 wire
  discriminant already covers it.

### 10.3 Save/load of parked flows

- A save captures **all** flows (primary + spawned): FlowFrame + wake
  policy + `#@local` store each, beside the shared world state.
- **Never-fail-load** (T1d posture): drift (vanished await site,
  missing condition token, frame name lost without `#@was`) never
  aborts a load. Each parked flow rehydrates **rebound** or
  **unresumable**; load yields a **rehydration report**; policy knob
  `Lenient` (production default) vs `Strict` (dev/CI: unresumable
  parked flows fail the load loudly).
- **Missing frame name without `#@was` ⇒ unresumable**, never
  resume-with-default (silent state change mid-flight is the banned
  laundering pattern). Unresumable flows remain handles in a terminal
  state; kill/restart is host policy.
- `wake_once` needs no cross-save ceremony — wake is condition truth
  at `wakeCheck()`, not a queued event.
- **Decomposed persistence**: the per-flow unit (`SuspendedFlow`) is
  independently encodable/decodable, version-stamped per unit — the
  row-per-flow shape (one world row + N flow rows) is a supported
  opt-in; the single-blob SaveState stays the default.

### 10.4 Caps and acceptance

- **Park-depth cap = 8**; at-cap is a turn-terminating runtime fault
  (parks nest only through tunnel chains; real stories sit at 1–2).
- **Oracle bar**: FS-3's opcodes are vanilla-unreachable; the ratchet
  stays byte-identical at `RATCHET_EPISODE_COUNT` with no corpus
  regeneration. That is the acceptance criterion.

### 10.5 Recorded future directions (icebox, not designed)

- **Journal-replay resume** (rung between rebind and unresumable):
  per-flow opt-in journaling; on drift, re-run from knot entry under
  the new story feeding recorded externals against a visit/RNG
  snapshot; any divergence aborts to unresumable. Best-effort tier
  for hosts that ship story changes continuously.
- **Story-version & migration facility**: author-declared story
  version stamped into StoryData + save units; load-time migration
  ladder = name-keyed rebind → `#@was` → author migration hooks →
  journal replay → unresumable-with-report. Rungs 1/2/5 exist today.

## 11. Implementation design — RULED 2026-07-18 (FS-3 implementation round)

### 11.1 Continuation-splitting (the center)

No instruction offsets (§3) is honored by **splitting containers at
await sites**: everything after an `await` becomes a synthesized
**continuation container**; parking = evaluate condition → false →
spill live locals per the FS-2 frame shape → record
`FlowFrame { container: <continuation id>, return_stack, frame,
wake }` → unwind. Resuming = restore the frame into a fresh
environment and **enter the continuation container from its top** —
an ordinary divert, no program-counter archaeology. Continuation
containers take **stable identities from their await site**
(module, enclosing def, site index), so frames survive recompiles via
the same name-keyed rehydration as everything else. The VM learns
one park outcome and one resume path; the cleverness lives in
codegen.

### 11.2 Continuation containers are INVISIBLE — RULED

No visit counts (they would pollute `shuffle`/`once` semantics in
behavior loops), not valid divert targets, absent from IDE
navigation/completion (debug views may show them). They are compiler
plumbing, not story structure — a new "hidden" container category in
the format, marked as such.

### 11.3 Reuse

- `wakeCheck()` condition evaluation reuses the isolated
  function-eval machinery (`begin_function_eval` lineage:
  output-isolated, transcript-untouched), evaluated in the owning
  flow's context (§10.2).
- Frame-shape tables ride a new StoryData section with `.inkt` dump
  parity (atoms land with the reader).
- Persistence is FS-1's SuspendedFlow section, already on main.

### 11.4 Slicing — RULED (maintainer approval between slices)

1. **FS-3w — web surface first**: flow-addressed API (flow handles,
   per-flow Line streams, `Line::Suspended` variant surface,
   `wakeCheck()` exported, returning empty until parks exist). Ships
   against today's runtime: consumers migrate API shape early;
   FS-3r later changes behavior, not interface.
2. **FS-3c — compiler**: liveness (#928's remainder), frame-shape
   emission, continuation-splitting, invisible-container category,
   `.inkt` parity. E052 fence STAYS UP. *(Landed: the `FrameShapes`
   StoryData section — `.inkb` tag `0x10` + `.inkt` `(frame_shapes …)`,
   optional/omitted-when-empty, writer+reader+per-codec round-trips — the
   `CountingFlags::INVISIBLE` continuation-container marker, and the
   per-await-site liveness → name-keyed frame-shape analysis
   (`brink_ir::hir::compute_frame_shapes`, computing crossing locals + the
   `module + enclosing def + site index` continuation identity). Behind the
   fence the section stays empty from compilation; wiring the analysis into
   the continuation-splitting codegen that populates it is FS-3r.)*
3. **FS-3r — runtime**: park/spill/resume, real wakeCheck +
   dirty-tracking, save/load integration + rehydration report +
   Lenient/Strict, and the E052 fence finally drops.

Nothing half-exists on main: the fence falls only in FS-3r.
