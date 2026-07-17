# The Compound — Phase 0

A small, complete, **playable** top-down stealth game built in **pure Bevy**
(no brink APIs). It is the *control group* for the drive-app plan
(`docs/drive-app-plan.md`): every entity's behavior is written in plain,
legible Rust so that Phase 1 can port each archetype to ink **one module at a
time** and diff the result — and the measured cost — against this baseline.

Sneak through a guarded compound to the green exit without getting caught.
Disable cameras for bounty, flip switches to open doors, survive the round,
then spend your gold in the intermission shop. Each round adds more guards.

## Running it

This crate lives **outside** the brink workspace (see the root `Cargo.toml`
`exclude = ["demos/*"]` and the empty `[workspace]` table in this crate's
`Cargo.toml`). It is never built by `cargo build --workspace` and cannot
perturb the oracle or CI. Build and run it on its own:

```sh
cd demos/compound
cargo run          # play it (needs a windowing environment)
cargo test         # unit tests for the FSM / alarm / shop / geometry logic
cargo clippy --all-targets -- -D warnings
```

## Controls

| Key        | Action                                             |
|------------|----------------------------------------------------|
| `WASD` / arrows | Move the player                               |
| `E`        | Interact — disable a nearby camera, flip a switch  |
| `+`        | Spawn 500 rats (the scale spectacle)               |
| `F1`       | Toggle vision-cone gizmos                           |
| `R`        | Reset the current round                            |

In the shop, click the item buttons to buy (affordability-gated) and
**Continue** to start the next, slightly harder round.

## HUD

Top-left shows: round, gold, cameras disabled, alarm level + tier, FPS, live
guard/rat counts, and a **per-frame behavior-system timing readout in µs**
(guards / cameras / doors / alarm / rats / total). Those microsecond figures
are the Rust-side cost baseline the Phase 1 ink port is measured against, so
the behavior systems are deliberately run serially (each writes the shared
`BehaviorTimings` resource) to keep the numbers comparable and summable.

## Modules — one entity archetype per file

| Module      | Owns                                                                   |
|-------------|-----------------------------------------------------------------------|
| `guards.rs` | Guard FSM `Patrol → Suspicious → Alert → Search → ReturnToPost`, vision cones, suspicion meter, patrol routes, reinforcement waves. The transition rule + suspicion integrator are pure functions. |
| `cameras.rs`| Sweeping security cameras; raise the alarm on sight; `E` to disable for bounty. |
| `doors.rs`  | Doors + switches; a door stays solid until its switch is flipped (reactive await-on-a-value). |
| `alarm.rs`  | Global escalation `0..3` with slow decay; single-writer world policy fed by `SpottedEvent` messages; tier ≥ 2 calls reinforcements. |
| `rats.rs`   | Hundreds–thousands of wandering rats; the cheap throughput spectacle. |
| `rounds.rs` | Top-level `Playing ⇄ Shop` FSM, round outcomes (escaped/caught), gold reward + difficulty scaling (pure functions), arena (re)build. |
| `shop.rs`   | bevy_ui intermission menu; affordability-gated buy buttons; STRUCT-shaped items. |
| `stats.rs`  | Player stats + persistent loadout; the shop item catalogue and pure purchase logic. |
| `world.rs`  | Fixed arena geometry, the player entity, and shared geometry helpers (`point_in_cone`, `resolve_collision`, `draw_cone`). |
| `hud.rs`    | The heads-up display + timing readout.                                |
| `timing.rs` | The `BehaviorTimings` resource shared by every behavior system.       |
| `main.rs`   | App wiring, player movement + collision, debug keybinds.              |

## Suggested Phase-1 migration order

Port the archetypes to ink cheapest-seam-first, so each step is a clean diff
against the Rust behavior it replaces:

1. **`alarm.rs`** — a single global value with one writer. The smallest,
   safest first port; establishes the world-policy write seam.
2. **`doors.rs`** — a minimal reactive entity (await on a value). Proves the
   suspend-on-change path with almost no logic.
3. **`cameras.rs`** — a pure sweep + detect loop plus one command
   (`camera disabled`). No per-entity memory to marshal.
4. **`guards.rs`** — the statechart archetype and the whole reason for the
   demo. Knots-as-states, `#@local` suspicion, tunnel-as-async chase. Port
   last of the AI entities, once the seams above are proven.
5. **`rounds.rs` / `shop.rs`** — the outer FSM and weave-as-menu; the economy
   becomes a `STRUCT` value table.
6. **`rats.rs`** — kept in Rust as the batch/parallel throughput control, or
   ported last purely to measure the scale ceiling.

Each port should keep this Rust module intact for a side-by-side timing diff
before the Rust version is retired.
