# The Compound — migration friction journal

The drive-app plan (`docs/drive-app-plan.md` §1, §9) ports each entity
archetype from pure Rust to ink one module at a time. This file is the
**evidence machine**: every port gets an entry recording what was awkward, the
lines-of-code delta, the measured per-frame cost, and any brink API gaps hit.
Awkwardness is the deliverable — a precise gap report is a success, not a
failure (§1's "friction journal" job).

Format per entry: **what moved · what was awkward · LOC · cost · API gaps**.

---

## Phase 1a — the alarm system (`alarm.rs` → `alarm.ink`)

**Issue:** #1045. **Port order:** first (README §"Suggested migration order") —
a single global value, one writer, many readers: the smallest possible seam,
chosen to establish the integration pattern every later port reuses.

### What moved

The escalation **state** (`alarm_level`, `alarm_global`) and **logic**
(soft-cap on spotting, panel jump to 3.0, 0.25/s decay, tier = floor(level))
now live in `assets/alarm.ink` as two globals and four functions
(`escalate_spotting`, `trigger_global`, `decay`, `alarm_reset`). `src/alarm.rs`
stays **verbatim** as the Rust baseline — the mixed-world end state (§9): a
`--alarm-impl rust|ink` launch flag picks exactly one writer per round, and
everything downstream still reads the same `Alarm` ECS resource, none the wiser.

The seam that connects the two lives in `src/ink_alarm.rs`.

### The two seams, and the API grain chosen for each

**Write seam — engine → ink via `call_ink_function`.** Guards/cameras still
emit `SpottedEvent` / `GlobalAlarm` messages. Each frame the (exclusive)
`ink_alarm_system` drains them and folds them into ink by *calling the ink
functions*: `decay(dt)`, then `escalate_spotting(amount)` per sighting, then
`trigger_global()` on a panel hit.

Why this grain and not a binding or `set_global`:

- A `bind_brink_*` binding is **ink → engine** (ink calls out to Rust) — the
  wrong direction. The alarm needs the *engine* to push events *into* ink.
- A raw `set_global("alarm_level", …)` would work mechanically but **bypasses
  the soft-cap / decay logic** — the exact logic the port exists to move into
  ink. Writing the level directly would leave ink holding state it doesn't
  govern. `call_ink_function` keeps ink authoritative over its own rules.

**Read seam — mirror ink globals → the `Alarm` resource, once per frame.**
After driving ink, the system reads `alarm_level` / `alarm_global` back out of
`BrinkGlobals<AlarmStory>` (resolve the name to a slot via
`Program::global_index`, read the slot via `ContextAccess::global`) and writes
them into the shared `Alarm` resource. Every reader (guards, cameras, HUD)
stays an ordinary cheap `Res<Alarm>` — unchanged from Phase 0.

Why mirror instead of letting each reader query ink: re-entering the VM needs
`&mut World` (exclusive) and costs far more than a resource read, so a
per-reader ink query would both serialize every reader and multiply the VM
cost by the reader count. Reading ink state **once** into an ECS resource at
the single-writer point is the natural shape — and it happens to be exactly
the "World-policy writes, frame-start consistency" seam the plan already
described (§4), just with ink as the writer.

### What was awkward

1. **The seam dwarfs the logic (expected, and the whole point).** Moving ~24
   lines of Rust escalation logic into ~15 lines of ink cost ~210 lines of
   Rust glue in `ink_alarm.rs`. Most of that is one-time, amortizing
   boilerplate — the launch toggle, the asset load, the exclusive-system event
   plumbing, the read-seam name→slot lookup — that the next port reuses. But it
   is an honest signal: for the *smallest* entity, the integration surface is
   an order of magnitude larger than the behavior it wraps. The interesting
   number is how much of this 210 shrinks (becomes shared scaffolding) by the
   second and third ports.

2. **Driving ink is one `call_ink_function` per event, from an exclusive
   system.** There is no "apply N events to a flow" batch entry point, and
   because `call_ink_function` needs `&mut World`, the whole alarm writer had
   to become an exclusive system (a `Local<SystemState<…>>` to still read
   `Time` + the message queues). Ergonomic, but every VM re-entry pays the
   function-eval setup cost; a frame with several sightings makes several
   separate calls. See gap G1.

3. **No high-level "read a global by name" helper.** Reading ink-owned state
   back is a three-step reach: `entity → BrinkProgram.handle →
   Assets<ProgramAsset> → Program::global_index(name) → BrinkGlobals::inner ::
   global(slot)`, plus importing the `ContextAccess` trait for the last hop.
   Correct and not slow, but verbose for what is conceptually
   `globals.get_f32("alarm_level")`. See gap G2.

4. **`.ink` source loads fine, but inline compile has no one-liner.** The live
   app just `asset_server.load("alarm.ink")`s — the default `dev` feature's
   `InkLoader` compiles the source at load time and even hot-reloads it (nice).
   But the *parity test* wants a deterministic, non-async story, so it compiles
   inline — and that is four manual steps (`brink_compiler::compile` →
   `brink_runtime::link` → `FlowInstance::new_at_root` for the initial context
   → hand-insert `ProgramAsset` + `LineTablesAsset` + `BrinkStoryAsset`),
   copied from the `engine_bindings` example. A "compile this source string
   into a `BrinkStoryAsset`" helper would remove a chunk of test boilerplate.
   See gap G3. (Hot-reload also needs bevy's `file_watcher` feature enabled by
   the host; the demo doesn't, so edits reload on next launch, not live.)

None of these **blocked** the port — the alarm is fully ported and the parity
test is green. They are ergonomics findings for the charter.

### LOC

| Piece | Rust | ink |
|---|---:|---:|
| Escalation logic (the semantics) | 24 (`Alarm` methods) | 15 (`alarm.ink` fns) |
| State declaration | ~6 (`struct Alarm` + fields) | 2 (`VAR` × 2) + 3 `CONST` |
| Per-frame writer | 19 (`alarm_system`) | 210 (`ink_alarm.rs`, non-test) |
| **Total behavior** | **~43** (alarm.rs logic + system) | **~22 ink + ~210 Rust seam** |

The logic itself is *more* concise in ink (15 vs 24 lines). The cost of the
port is entirely in the seam, which is mostly reusable scaffolding.

### Measured per-frame cost

Measured by `ink_alarm::tests::measure_ink_alarm_cost` (10k-frame average,
debug-opt profile, Apple M2 — the same build the HUD numbers come from; run
`cargo test measure_ink_alarm_cost -- --nocapture` to reproduce):

| Frame shape | Rust (`Alarm` methods) | ink (`call_ink_function`) | ratio |
|---|---:|---:|---:|
| Calm (decay only) | ~4–5 ns | **~2.5–3.3 µs** | ~600× |
| Hot (decay + 1 spotting) | ~8–11 ns | **~4.9–5.3 µs** | ~500× |

(The in-game full-system Rust number, including message-reader plumbing, is
the ~42 ns/frame HUD baseline.) The cost is dominated by per-call VM-eval
setup, not the ink arithmetic — each additional `call_ink_function` adds
~2.5 µs, which is what gap G1 is about. In absolute terms ~3 µs/frame is
irrelevant for one alarm (0.02% of a 60 fps frame); the number matters as the
*per-entity, per-event* unit cost for the later ports that have dozens of
entities.

The HUD's `alarm` line prints the live cost at nanosecond resolution (42 ns
rounds to 0 µs) beside the Rust baseline, labelled with the active impl.

### Semantics parity

`ink_alarm::tests::ink_alarm_matches_rust_frame_by_frame` drives **both**
implementations through one scripted event sequence (ramp to soft-cap, decay,
panel jump, spotting-must-not-lower-a-global, full bleed-out clearing the
latch, round-start reset, re-escalate) and asserts identical **tier** (the
correctness bar), identical **global latch**, and level within 1e-4 every
frame. brink stores floats as `f32` — the same width as `alarm.rs` — so the
arithmetic is directly comparable. **Status: green.**

### API gaps filed

- **G1 (#1058)** — no batch "apply these events / call this fn N times" entry
  point; every event is a separate `call_ink_function` + ~2.5 µs VM-eval setup
  from an exclusive system.
- **G2 (#1059)** — **resolved.** `BrinkGlobals::get(&self, program, name)`
  collapses the manual `Program::global_index` + `ContextAccess::global`
  reach (and its trait import) into one call; `read_alarm_state` in
  `src/ink_alarm.rs` now uses it.
- **G3 (#1060)** — **resolved.** `bevy_brink::compile_story_inline(app, name,
  source)` wraps the compile→link→context→insert dance in one call;
  `build_alarm_story` in `src/ink_alarm.rs`'s test module now uses it instead
  of hand-rolling the four steps.

---

## Phase 1b — doors/switches (`doors.rs` → `doors.ink` + `src/ink_doors.rs`)

**Issue:** #1068. **Port order:** second (README §"Suggested migration
order") — "await on a value," the minimal REACTIVE entity: a door does
nothing until its switch flips. Chosen specifically to prove the BH-4 host
wake surface (`FlowSleep`/wake_when, `docs/effects-spec.md` §13.1) end to
end — ink-level `await` stays fenced until FS-3r, so this port is the
**first real consumer of BH-4 outside its own test suite**, and *that* is
the interesting datapoint, not the (trivial) door logic itself.

### What moved

Each locked door becomes its own ink flow instance under one shared
`DoorsStory` marker, running `assets/doors.ink`. The flow entity **is** the
door entity — `src/ink_doors.rs::attach_ink_door_flows` attaches
`BrinkFlowRequest<DoorsStory>` + a dormant, one-shot `FlowSleep<DoorsStory>`
straight onto the same `Door` sprite entity `doors::spawn_doors_from_layout`
already spawns (unmodified — it's still the source of both the `Door`
sprites and the `Switch` entities, for **both** implementations; only its
`accent_color`/`ACCENT_COLORS` helpers got a `pub(crate)` bump so the ink
read-seam can reuse them instead of forking a second copy).

`--doors-impl rust|ink` picks exactly one writer per round, same shape as
Phase 1a's `--alarm-impl`.

### The wake seam, and why it inverts Phase 1a's shape

Phase 1a's write seam pushes engine events *into* ink every frame
(`call_ink_function`). A door's seam runs the other way: `should_open`
(`assets/doors.ink`) calls a `bind_brink_query` world-access binding
(`is_switch_on`) that reads the live `Switch` component straight out of the
ECS **at evaluation time** — there is no per-frame mirror system at all.
`run_flow_sleep` (BH-4's own exclusive re-evaluation driver) resolves that
binding inline, exactly the way `call_ink_function` resolves one from a
normal engine→ink call (`bevy-brink/examples/engine_bindings.rs`'s
`can_advance()` / `enemy_count()` pattern). This is a genuine ergonomics win
over Phase 1a: **no G1/G2-style mirroring boilerplate was needed to feed the
condition** — the direction (engine state → ink read) is exactly what
`bind_brink_query` is for.

### What was awkward — the wake_when authoring ergonomics (the actual finding)

1. **A component-backed condition always must-polls, with no way to avoid
   it — now confirmed by a real consumer, not just a unit test.**
   `should_open`'s dependency is the `Switch` component, not a
   `BrinkGlobals` variable, so `mark_wake_dirty` has no change-tick hook on
   it (`docs/effects-spec.md` §12.5 is not wired — the `sleep` module's own
   doc comments anticipate exactly this case, "e.g. `is_player_nearby`
   reading `Transform`"). The only way to get a *sound* wake is
   `.with_detect(...)` with a non-empty (any-value) bit, which forces
   **every** parked door to re-evaluate its condition **every frame** while
   locked — not "on switch change." For one door this is invisible; for a
   compound with dozens of locked doors it is dozens of
   `call_ink_function`+`bind_brink_query` round trips a frame, paid purely
   because there is no cheaper option today. Same interim #996 already
   tracks — commented there with this port's concrete cost data point
   rather than filing a duplicate: **G4 (#996)**.
2. **`WakeArming` has no "toggle" shape, forcing a real design compromise.**
   `Once` fires exactly one wake and is done forever; `Persistent` re-arms
   and re-*steps* every single turn boundary while the condition stays true
   — for a door that means continuously running "the door is open" turns
   every other frame for as long as the switch stays on, purely to keep
   re-asserting a fact that hasn't changed. Neither shape expresses "wake on
   a *transition*, then go quiet until the *opposite* transition" — the
   natural shape for a boolean-latch reactive entity (a door, a light
   switch, a pressure plate). We chose `Once`: the door opens and **never
   re-locks**, even if the Rust baseline's switch is later flipped back off
   (`doors::door_sync_system` is fully bidirectional — it does re-lock).
   That divergence is deliberate and directly tested (see below), not a
   bug — but it is a real behavior gap versus the Rust baseline, and
   modeling the reversible case properly would need per-flow `Local`-scoped
   ink state (`docs/scoped-flow-state-spec.md`) to give each door instance
   its own private "am I open" bit, which is out of scope for this minimal
   port. Filed as **G5 (#1081)**.
3. **Many instances of one program, one marker: works, but undocumented.**
   Spawning N flows of the same `assets/doors.ink` program under a single
   `DoorsStory` marker relies on a fact that isn't obvious from the
   `bevy-brink` docs: `BrinkGlobals<M>` is **one shared World per marker**,
   so any `VAR` the story declares would be shared across every door
   instance, not private per-instance. `doors.ink` sidesteps this by
   declaring **no globals at all** — every door's own state lives entirely
   in its flow entity's own `BrinkFlow`/`FlowSleep` components, read via
   `bind_brink_query`'s `(Entity, Vec<Value>)` input instead. This worked
   out cleanly here, but the "N instances, one marker, zero globals" pattern
   is worth a doc callout rather than something the next port has to
   rediscover by reading `globals.rs`'s source.

None of these **blocked** the port — every locked door correctly stays shut
until its switch flips, and the divergence from the Rust baseline (no
re-lock) is explicit and tested, not silent. They are ergonomics findings
for the charter, same as Phase 1a's G1–G3.

### LOC

| Piece | Rust | ink |
|---|---:|---:|
| Reactive logic (the semantics) | 4 (`door_sync_system`'s body) | 3 (`doors.ink`'s `should_open` + root turn) |
| Per-frame writer / seam | — (Rust needs none; it's a plain query) | ~90 (`ink_doors.rs`, non-test): marker, impl toggle, attach system, binding, read seam |

Unlike Phase 1a, there is no per-frame *write* seam at all (no
`call_ink_function` mirror loop) — the entire seam is the one-time `attach`
(spawn-time) plus the `bind_brink_query` binding the wake condition calls.
The logic itself is trivially small on both sides; the seam is still the
whole cost, exactly as Phase 1a predicted ("the interesting number is how
much of this shrinks... by the second... port" — here it shrank by
eliminating the write-seam category entirely, at the cost of the must-poll
finding above).

### Measured cost

Measured by `ink_doors::tests::measure_ink_door_wake_cost` (10k-iteration
average, debug-opt profile — the same build the HUD numbers come from; run
`cargo test measure_ink_door_wake_cost -- --nocapture` to reproduce). See the
test output for the exact numbers on your machine; the shape to expect is
the same order of magnitude as Phase 1a's `call_ink_function` cost
(µs-scale per call, dominated by VM-eval setup) for `should_open`, against a
sub-microsecond Rust `Vec`/query scan. Because gap G4 forces every locked
door to pay this cost **every frame** while parked (not just on a switch
change), this number is the one that matters most for a compound with many
simultaneously-locked doors — it is the direct multiplier on "how many
locked doors can this scene afford."

The HUD's `doors` line prints the live cost (µs resolution) beside the
active impl label, the same shape as the alarm's ns-resolution line.

### Semantics parity

`ink_doors::tests` drives the reactive contract end to end (through the
plugin's own `mark_wake_dirty`/`run_flow_sleep` + a host-registered
`advance_batch::<DoorsStory>` — no direct calls into BH-4 internals):

- a door stays locked (zero-cost, dormant/parked) while its switch is off,
  and opens once the switch flips on, then stays open;
- two doors sharing one switch id open together; a door watching a
  *different* id is unaffected;
- frame-by-frame parity against the Rust baseline for the common
  (monotonic) case — every frame before the first flip, both
  implementations agree the door is closed;
- the **documented divergence**: a scripted flip sequence that ends with the
  switch back off proves the ink door stays open while the Rust baseline
  re-locks — asserted explicitly (`assert_ne!`), not hidden.

**Status: green** (including the intentional, tested divergence).

### API gaps filed

- **G4 (#996)** — a `FlowSleep` condition backed by an ECS component (not a
  `BrinkGlobals` variable) has no way to avoid must-polling every wake pass;
  `docs/effects-spec.md` §12.5's component-tick wiring doesn't exist yet.
  Already anticipated in the `sleep` module's own doc comments, and already
  tracked by #996; this port is the first real (non-test) consumer to
  actually pay the cost, so it's a comment on #996 with a concrete data
  point, not a new issue.
- **G5 (#1081)** — `WakeArming` offers only `Once` (permanent) or
  `Persistent` (re-steps every turn boundary while true); there is no "wake
  on transition, park until the opposite transition" toggle shape for a
  boolean-latch reactive entity. Forced a documented behavioral
  simplification (doors never re-lock) rather than a faithful port.

---

## Phase 1c — security cameras (`cameras.rs` → `cameras.ink` + `src/ink_cameras.rs`)

**Issue:** #1083. **Port order:** third (README §"Suggested migration
order") — "the pure loop archetype": sweep an angle, detect via cone +
raycast, no per-entity memory to marshal beyond the sweep phase itself.
Chosen as the first port with **N flow instances sharing one program AND
needing private per-instance state** — doors (Phase 1b) also has N
instances of one program, but declares no ink state at all, so it never had
to answer "how does each instance keep its own private cell."

### What moved

Every `SecurityCamera` a round spawns gets its own flow instance under one
shared `CamerasStory` marker, running `assets/cameras.ink` — the flow entity
**is** the camera entity, `src/ink_cameras.rs::attach_ink_camera_flows`
attaching `BrinkFlowRequest<CamerasStory>` straight onto each
`spawn_cameras_from_layout` entity, same pattern as the doors port.
`src/cameras.rs` stays untouched as the Rust baseline (only
`CAMERA_RANGE`/`CAMERA_HALF_ANGLE` got a `pub(crate)` bump so the ink port's
`sees_player` binding can reuse the exact geometry constants instead of
forking copies). `--cameras-impl rust|ink` picks exactly one writer per
round, same shape as `--alarm-impl`/`--doors-impl`.

Because all flows under one marker share one `BrinkGlobals<CamerasStory>`
World, a plain `VAR` for `phase`/`facing` would be one sweep state shared by
every camera in the compound — wrong the moment a round has more than one.
`assets/cameras.ink` uses `#@local` (`docs/directive-annotations-spec.md`
§3) for both cells: flow-private storage, invisible to every other camera's
flow. `center_angle` and the loadout/stealth-adjusted `range` are deliberately
**not** ink state at all — `ink_camera_system` passes them as fresh
arguments to `sweep_and_detect` every call, computed host-side exactly like
`camera_ai_system` does, so the only thing ink actually remembers across
frames is the two `#@local` cells.

### The seams

**Write seam — one function call per camera per frame, exclusive, serial —
not `advance_batch`.** `ink_camera_system` calls `sweep_and_detect(dt,
center_angle, range)` (advances `phase`/`facing`, returns whether the camera
currently sees the player), then `camera_facing()` (the read seam for the
debug gizmo / parity test) — two `call_ink_function` re-entries per live
camera per frame, the same exclusive-system shape Phase 1a's alarm uses.
**Not** Phase 1b's `advance_batch`: BH-3's `homes_any_local` guard (#925)
skips (with a `warn!`) any flow whose program compiles `#@local` defaults
rather than silently double-counting per-instance state across the batch's
shared-World assumption — a story built on `#@local` state has to stay on
the serial driver. This is a direct, load-bearing consequence of choosing
`#@local` over the alternative (no private state, thus no faithful sweep at
all), not a workaround.

**Detection — a world-access binding, not ink vector math.** `sees_player`
(`bind_brink_query`) does the actual cone-and-raycast test
(`world::point_in_cone` + `world::raycast_clear`) against the live
`Transform`s/`Collider`s, reading the calling flow entity's own `Transform`
as the cone apex — ink never does vector math (icebox #827), and this is
exactly the doors' `is_switch_on` shape (a query binding reading state off
the calling flow entity) applied to geometry instead of a component flag.

**Read seam — the alarm write happens in Rust, not ink.** This is the
finding, not just a seam choice — see "What was awkward" point 1 below.

### What was awkward

1. **The plan's command-based design (`docs/drive-app-plan.md` §3) is
   unreachable from this port's drive mechanism — discovered, not assumed.**
   The plan sketched cameras raising the alarm via a
   `#[derive(BrinkCommand)]` "spotted" command fired from ink. But
   `call_ink_function`'s evaluation handler (`EvalHandler` in
   `crates/bevy-brink/src/bindings.rs`) only resolves `bind_brink_fn` (pure,
   inline) and `bind_brink_query` (world-access, paused/resumed) bindings —
   a `bind_brink_command`-bound `EXTERNAL` isn't in either bucket and falls
   through to `ExternalResult::Fallback`, the same path an *unbound*
   external takes: the call silently runs the in-story fallback instead of
   firing the event, with no error and no log. `BrinkHandler` (the serial
   story-stepping handler) buffers and flushes commands correctly;
   `EvalHandler` (the one-pass engine→ink driver behind
   `call_ink_function`) has no equivalent buffer at all. Worked around by
   having `sweep_and_detect` return a plain boolean and `ink_camera_system`
   write `SpottedEvent` itself — the same seam `ink_alarm_system` already
   uses to *read* `SpottedEvent`, just one step earlier in the chain. Filed
   as a new issue (checked for a dupe first, per the #996 precedent — none
   existed): **G6 (#1096)**.
2. **`#@local` is the right tool, but this is the first port to need it —
   the flywheel didn't cover it.** Neither the alarm (one shared flow, plain
   `VAR`s) nor doors (N flows, zero `VAR`s) needed a storage-class
   annotation at all. Discovering `#@local` (and that BH-3's batch driver
   deliberately excludes it) was net-new spelunking this port had to do that
   the first two didn't leave behind as reusable knowledge — worth a doc
   callout the same way doors' "N instances, one marker, zero globals"
   pattern was.
3. **Two VM re-entries per camera per frame, not one — the batch-call gap
   (G1/#1058) compounds exactly as Phase 1a predicted.** `sweep_and_detect`
   and `camera_facing` are two separate `call_ink_function` calls because
   the read-back (`camera_facing`) has no batch-with-the-write-call entry
   point either. For a compound with a dozen cameras that is 2×12 = 24 VM
   re-entries a frame, on top of doors' and the alarm's own calls. This is
   the same #1058 gap Phase 1a filed, now with a second multiplier (2×
   per-entity, not just N-per-entity) — noted here as more evidence for
   #1058/#1062's dispatch-order priority, not a new issue.

None of these **blocked** the port — every camera sweeps and detects
identically to the Rust baseline (see Semantics parity below), and the
command-binding gap is a documented, tested workaround, not a silent bug.
They are ergonomics findings for the charter, same as Phase 1a's G1–G3 and
Phase 1b's G4–G5.

### The flywheel check — did the accumulated helpers suffice?

The issue asked this port to explicitly note whether the ergonomics helpers
the first two ports built (`compile_story_inline`, `BrinkGlobals::get`) held
up for a third, structurally different entity:

- **`compile_story_inline` (#1060) — sufficed as-is.**
  `build_cameras_story` in `ink_cameras.rs`'s test module calls it exactly
  the way `ink_alarm`/`ink_doors` do, no new wrapping needed. Three ports in,
  this one is fully load-bearing shared scaffolding.
- **`BrinkGlobals::get` (#1059) — not exercised, and that is itself a
  finding.** This port declares no `BrinkGlobals`-visible `VAR`s at all (see
  "What moved" above) — `center_angle`/`range` are call arguments, and the
  `#@local` cells are per-flow, not per-marker-global, so there is nothing
  for a globals-by-name reader to reach. The flywheel's second helper simply
  doesn't apply to this port's shape, which is a useful data point on its
  own: the ports so far split evenly between "needs the globals reader"
  (alarm) and "doesn't" (doors, cameras) — not because the helper is
  deficient, but because only one of three entity archetypes so far
  authoritatively owns engine-visible state as ink globals.
- **What the flywheel did *not* yet cover: per-instance private ink state.**
  Neither prior helper (nor any prior port) anticipated `#@local` — this
  port had to discover the annotation and its `advance_batch` exclusion
  (BH-3's #925 guard) from source, not from an existing pattern. That gap is
  now closed for the *next* port by this entry existing (see point 2 above).
- **What the batch call surface (#1058, "first consumer if merged") turned
  out to mean: not applicable here, for a real reason.** The issue flagged
  cameras as the potential first consumer of #1058's batch engine→ink call
  surface if it landed before this port did. It hadn't, and — more
  importantly — it wouldn't have helped even if it had: #1058's batch
  surface is scoped to `advance_batch`-shaped driving, and this port is
  excluded from `advance_batch` entirely by the `#@local` guard (#925). The
  serial, per-call cost (point 3 above) is a *different* axis than the one
  #1058 amortizes, and is worth flagging back onto #1058/#1062 as scope
  evidence rather than assuming the existing ticket already covers it.

### LOC

| Piece | Rust | ink |
|---|---:|---:|
| Sweep + detect logic (the semantics) | 2 (`camera_ai_system`'s phase/facing update) | 3 (`sweep_and_detect`'s body) |
| State declaration | ~6 (`SecurityCamera` fields) | 4 (`#@local VAR` × 2) + 2 `CONST` |
| Per-frame writer / seam | ~45 (`camera_ai_system`, non-test) | ~323 (`ink_cameras.rs`, non-test) |

Structurally identical to Phase 1a's finding (the seam dwarfs the logic), not
Phase 1b's (no per-frame write seam at all) — cameras need both a per-frame
*write* seam (Phase 1a's shape, doubled per call in point 3 above) **and** a
`bind_brink_query` *read* seam (Phase 1b's shape), because the reactive
detection math is engine-side while the sweep bookkeeping is ink-side.

### Measured cost

Measured by the parity test's scripted frame loop against the from-scratch
Rust reference (`ink_cameras::tests::ink_camera_matches_rust_sweep_and_detection_over_a_scripted_path`
— see the test for the exact per-call shape). The expected order of
magnitude is the same as Phase 1a's `call_ink_function` cost (µs-scale per
call, dominated by VM-eval setup), **doubled** per camera per frame (two
calls, not one — point 3 above), against a sub-microsecond Rust sweep+cone+
raycast. The HUD's `cameras` line prints the live cost (µs resolution)
beside the active impl label, the same shape as doors'.

### Semantics parity

`ink_cameras::tests` drives the reactive contract with a from-scratch Rust
reference kept independent of `ink_camera_system` (so the test doesn't just
check ink against itself):

- `ink_camera_matches_rust_sweep_and_detection_over_a_scripted_path` — a
  30-frame scripted player path (approach through the cone, pass behind a
  wall, stand in the open close-up) asserts identical `facing` (within 1e-4)
  and identical detection outcome every frame, exercising both the
  cone-angle and the raycast-blocked branches;
- `ink_camera_system_writes_spotted_event_and_skips_disabled` —
  reachability through the real system (not just the test-only `sweep_camera`
  helper): a live camera facing the player writes `SpottedEvent`; a disabled
  one is skipped entirely, matching `camera_ai_system`'s own
  `if cam.disabled { continue }`.

**Status: green.**

### API gaps filed

- **G6 (#1096)** — `call_ink_function`'s evaluation handler silently falls
  back to the in-story body for a `bind_brink_command`-bound `EXTERNAL`
  instead of firing the event or erroring — no diagnostic surfaces the
  mismatch. Discovered because this port's original design (a command-fired
  alarm) hit it directly; worked around by returning a boolean and having
  the host write the event itself. New issue, checked for a dupe first.
- **G1 (#1058)** — additional evidence, not a new issue: this port pays the
  batch-call gap **twice** per camera per frame (`sweep_and_detect` +
  `camera_facing`, no batched read-back), and is *excluded* from the
  #1058 batch surface's `advance_batch` scope entirely by the `#@local`
  guard (#925) — see "The flywheel check" above.
