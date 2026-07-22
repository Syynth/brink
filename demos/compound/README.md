# The Compound — Phase 0 (gameplay v2)

A small, complete, **playable** top-down stealth game built in **pure Bevy**
(no brink APIs). It is the *control group* for the drive-app plan
(`docs/drive-app-plan.md`): every entity's behavior is written in plain,
legible Rust so that Phase 1 can port each archetype to ink **one module at a
time** and diff the result — and the measured cost — against this baseline.

**Gameplay v2** (plan §10) turns the sandbox into a game with opposing
dynamics:

- **A generated compound.** Every round draws a fresh **seeded BSP layout**
  (`layout_gen`) — rooms partitioned by a BSP tree, connected by a spanning
  tree plus loop doorways, with locked doors whose switches are always placed
  so the layout is **solvable by construction**. Room *recipes* place the
  encounter: Entry, Exit, Guard posts, Camera nests, Storage (gold), Switch
  rooms, Alarm panels, a timed high-value Vault, and Barracks (reinforcement
  entry).
- **Greed vs safety.** Gold is picked up in dangerous rooms but **banked only
  on exit** — get caught and you lose the unbanked haul. The HUD shows
  *banked* vs *carrying*.
- **Speed vs noise.** Walking is silent; holding **Shift** runs faster but
  emits noise that pulls guards toward the sound. Thrown **coins** are lures;
  a **smoke** bomb breaks a chase.
- **An MGS-lenient suspicion ladder.** Guards climb `Patrol → Curious →
  Investigate → Chase`; cones respect walls, and once you are spotted the only
  way to escape is to **break line of sight** (running never works). Alerted
  guards shout to recruit neighbours and head for an **alarm panel** to wake
  the whole compound — intercept them or stay hidden until they give up.
- **Guards navigate the compound, they don't phase through it.** Every guard
  movement target — a patrol post, a last-known player position, an alarm
  panel, a search point, a return-to-post — is routed by **room-graph
  pathfinding** (`nav`): A* over the generator's rooms-and-doorways connectivity
  produces a sequence of door-center waypoints, and the guard walks straight
  lines only *within* a room. As defense in depth guards also run the **same
  wall collision the player does**, so a guard can never end a frame inside a
  wall even if a path were wrong. **Guards traverse doors regardless of lock
  state** — staff carry keys, and it keeps the player's switch mechanic from
  ever stranding the compound's guards; the player-only lock mechanic is
  unchanged (#1044).
- **Sidegrades, not upgrades.** The shop sells trade-offs (boots: +speed
  −quiet; cloak: −enemy vision −run speed; muffled soles: −noise −top speed)
  plus coin/smoke restocks. Rounds escalate the *compound* (more guards,
  richer layouts), never the player.

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

**Phase 1a is underway**: the alarm system is ported to ink
(`assets/alarm.ink` + `src/ink_alarm.rs`), side-by-side with the Rust
baseline per the mixed-world end state. Pick the writer at launch:

```sh
cargo run -- --alarm-impl rust   # the Phase-0 Rust alarm (default)
cargo run -- --alarm-impl ink    # the ink port; HUD reports its µs/frame
```

**Phase 1b is underway too**: doors/switches — the minimal REACTIVE entity —
are ported to ink (`assets/doors.ink` + `src/ink_doors.rs`) via the host BH-4
wake surface (`FlowSleep`/wake_when), side-by-side with the Rust baseline.
Pick the writer at launch, independently of `--alarm-impl`:

```sh
cargo run -- --doors-impl rust   # the Phase-0 Rust doors (default)
cargo run -- --doors-impl ink    # the ink port; HUD reports its µs/frame
```

**Phase 1c is underway too**: security cameras — the pure sweep-and-detect
loop — are ported to ink (`assets/cameras.ink` + `src/ink_cameras.rs`), one
flow instance per camera (`#@local` sweep state, since every camera shares
one `CamerasStory` program). Pick the writer at launch, independently of the
other two:

```sh
cargo run -- --cameras-impl rust   # the Phase-0 Rust cameras (default)
cargo run -- --cameras-impl ink    # the ink port; HUD reports its µs/frame
```

The friction journal for each port lives in [`MIGRATION.md`](MIGRATION.md).
As of Phase 1a the crate takes brink *path* dependencies (`bevy-brink`, plus
`brink-compiler` as a dev-dep) — still workspace-excluded, still never built
by root CI.

## Controls

| Key        | Action                                             |
|------------|----------------------------------------------------|
| `WASD` / arrows | Move the player                               |
| `Shift` (hold) | Run — faster, but noisy (draws guards)         |
| `E`        | Interact — disable a nearby camera, flip a switch  |
| `LMB`      | Throw a coin toward the cursor (a lure)            |
| `Q`        | Drop a smoke bomb (breaks a chase)                 |
| `+`        | Spawn 500 rats (the scale spectacle)               |
| `F1`       | Toggle vision-cone gizmos                           |
| `R`        | Reset the current round (new seed = new layout)    |

In the shop, click the item buttons to buy (affordability-gated) and
**Continue** to start the next, richer round.

## HUD

Top-left shows: round, **banked** vs **carrying** gold, cameras disabled, coin
and smoke counts, alarm level + tier + state (`calm` / `alerted` / `sweeping`
/ `GLOBAL`), FPS, live guard/rat counts, and a **per-frame behavior-system
timing readout in µs** (guards / cameras / doors / alarm / rats / total).
Those microsecond figures are the Rust-side cost baseline the Phase 1 ink port
is measured against, so the behavior systems are deliberately run serially
(each writes the shared `BehaviorTimings` resource) to keep the numbers
comparable and summable.

## Modules — one entity archetype per file

| Module         | Owns                                                                   |
|----------------|------------------------------------------------------------------------|
| `layout_gen.rs`| **Seeded BSP layout generation** — the pure `generate(seed) → LayoutData` encounter designer: BSP rooms, spanning-tree + loop doorways, solvable-by-construction locked-door/switch DAG, and room recipes. A **new migration specimen** (the future systems-logic ink port); pure + heavily unit-tested. |
| `guards.rs`    | Guard suspicion ladder `Patrol → Curious → Investigate → Chase`, wall-respecting vision cones, LOS-mandatory escape, shout recruitment, alarm-panel seeking, reinforcements. Every movement target is routed through `nav` and every step runs the player's wall collision (never phase, #1044). The transition rule + suspicion integrator are pure functions. |
| `nav.rs`       | **Guard room-graph pathfinding** (#1044): a pure `RoomGraph` built from the layout's rooms-and-doorways connectivity, with A* over door-center waypoints. Guards traverse every door regardless of lock (staff keys). Pure + unit-tested across generated seeds. |
| `cameras.rs`   | Sweeping security cameras; wall-respecting LOS; raise the alarm on sight; `E` to disable for bounty. |
| `doors.rs`     | Doors + switches; a locked door stays solid until its switch is flipped (reactive await-on-a-value). |
| `alarm.rs`     | Global escalation `0..3` with slow decay; single-writer world policy fed by `SpottedEvent`; spotting soft-caps at tier 1 — only a guard reaching an alarm panel (`GlobalAlarm`) wakes the compound. |
| `loot.rs`      | Gold pickups; the carried haul (banked only on exit). |
| `noise.rs`     | The speed-vs-noise axis: run noise, thrown coins, smoke clouds. |
| `rats.rs`      | Hundreds–thousands of wandering rats; the cheap throughput spectacle. |
| `rounds.rs`    | Top-level `Playing ⇄ Shop` FSM; per-round seed; banking + **edge-triggered** round end (one reward per exit); difficulty scaling; arena (re)build from the layout. Pure functions. |
| `shop.rs`      | bevy_ui intermission menu; affordability-gated buy buttons; STRUCT-shaped items. |
| `stats.rs`     | Player stats + persistent loadout; the sidegrade/consumable catalogue and pure purchase logic. |
| `world.rs`     | Arena constants, the player entity, layout instantiation, and shared geometry helpers (`point_in_cone`, `raycast_clear`, `resolve_collision`, `draw_cone`). |
| `hud.rs`       | The heads-up display + timing readout. |
| `timing.rs`    | The `BehaviorTimings` resource shared by every behavior system. |
| `main.rs`      | App wiring, player movement (walk/run) + collision, coin/smoke input, debug keybinds. |

## Suggested Phase-1 migration order

Port the archetypes to ink cheapest-seam-first, so each step is a clean diff
against the Rust behavior it replaces:

1. **`alarm.rs`** — a single global value with one writer. The smallest,
   safest first port; establishes the world-policy write seam.
2. **`doors.rs`** — a minimal reactive entity (await on a value). Proves the
   suspend-on-change path with almost no logic.
3. **`cameras.rs`** — a pure sweep + detect loop. No per-entity memory to
   marshal beyond the sweep phase itself (`#@local`); disabling stays
   Rust-only in both modes, and the port ended up returning a plain boolean
   rather than firing a `#[derive(BrinkCommand)]` command — see
   `MIGRATION.md`'s Phase 1c entry for the gap that forced that call.
4. **`layout_gen.rs`** — the **systems-logic specimen**: a pure, seeded
   `generate(seed) → LayoutData` with no ECS. Ports to an ink function/`STRUCT`
   computation whose output is diffed structurally against the Rust generator
   (determinism + solvability make the diff exact), independent of the entity
   ports.
5. **`guards.rs`** — the statechart archetype and the whole reason for the
   demo. Knots-as-states, `#@local` suspicion, tunnel-as-async chase. Port
   last of the AI entities, once the seams above are proven.
6. **`rounds.rs` / `shop.rs`** — the outer FSM and weave-as-menu; the economy
   becomes a `STRUCT` value table.
7. **`rats.rs`** — kept in Rust as the batch/parallel throughput control, or
   ported last purely to measure the scale ceiling.

Each port should keep this Rust module intact for a side-by-side timing diff
before the Rust version is retired.
