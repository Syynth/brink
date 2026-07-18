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
- **G2 (#1059)** — no ergonomic host-side "read an ink global by name"
  accessor on `BrinkGlobals`; today it is a manual `Program::global_index` +
  `ContextAccess::global` reach requiring a trait import.
- **G3 (#1060)** — no one-liner to compile an in-memory `.ink` source string
  into a `BrinkStoryAsset` for tests/tools; the four-step
  compile→link→context→insert dance is copy-pasted from the `engine_bindings`
  example.
