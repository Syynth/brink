# BH-B wake-fan-out — PROVISIONAL night-data (issue #973)

**Status: provisional, in-wave.** These are the *structural* step ratios of
the BH-4 reactive-sleep wake contract (`docs/effects-spec.md` §13.1), captured
alongside the BH-4 landing per the night-shift data rule. They are **not**
wall-clock numbers and **not** a canonical baseline — the canonical
throughput/latency curve on this axis is regenerated at the next quiet window
(as the BH-3 `parallel-driver` / `serial-driver` baselines were), against the
same scenario-harness machinery in `benches/scenario/`. Compare against those
canonical driver baselines, not against absolute wall-clock here (there is
none).

## What the axis measures

The wake-fan-out axis varies the **parked : active** ratio of a flow
population under `advance_batch`:

- A **parked** flow carries a `FlowSleep` policy whose condition is false. It
  is **skipped by Collect** entirely — it steps zero times and contributes no
  per-flow batch record. This is the "parked cost = zero" contract (§13.1
  point 1): a sleeping flow is free until a dependency moves its condition
  true.
- An **active** flow (no policy, or a woken one) steps normally.
- The **wake-storm** case stresses the opposite extreme: every flow carries a
  persistent, always-true, must-poll policy, so every flow wakes and steps
  **together** in each batch turn — the worst case for the wake systems
  (`mark_wake_dirty` + `run_flow_sleep`), where re-evaluation happens every
  pass for every flow.

The `step_ratio_active_to_total` column is `steps / (active-flow expected
steps)`; it is `1.00` in every row because parked flows contribute exactly
zero steps and active/woken flows step exactly once per turn — the invariant
this data exists to pin.

## Numbers

| scenario | flows | parked | active | condition | batch turns | steps (parked) | steps (active/woken) |
|---|---:|---:|---:|---|---:|---:|---:|
| fanout-6park-2active | 8 | 6 | 2 | `gate == 0` | 1 | 0 | 2 |
| fanout-4park-4active | 8 | 4 | 4 | `gate == 0` | 1 | 0 | 4 |
| fanout-7park-1active | 8 | 7 | 1 | `gate == 0` | 1 | 0 | 1 |
| wakestorm-5 | 5 | 0 | 5 | `gate != 0` (always) | 1 | — | 5 |

(Raw rows in `wake-fan-out-provisional.csv`.)

## How it was captured

The ratios are asserted by
`crate::sleep::tests::wake_fan_out_scenario_ratios`, which drives the **real**
`advance_batch::<M>` entry point + the plugin's registered
`mark_wake_dirty`/`run_flow_sleep` systems over a parked/active flow population
and reads `BrinkBatchReport` — the same report the canonical scenario harness
consumes. Because parked-flow skipping and one-step-per-turn are deterministic
(no wall-clock dependence), these structural ratios are exact and are asserted
in-test; only the *throughput* numbers (deferred to the quiet-window canonical
run) are machine-specific.

## §12.5 component-tick cheap path — before/after (issue #996, PROVISIONAL)

**Status: provisional, in-wave.** #996 wired ECS component change ticks into
`mark_wake_dirty` per capability (`docs/effects-spec.md` §12.5), lifting the
must-poll interim PR #991 shipped for component-backed detect-capable wake
conditions. This is the wake-driver before/after data point the issue asks for.

### What changed structurally

The axis here is a **parked, component-backed, detect-capable** flow — the
`is_player_nearby`-reading-`Transform` / door-`should_open`-reading-`Switch`
shape (the first real consumer is `demos/compound`'s ink doors port, #1080).

- **Before (must-poll interim):** because `mark_wake_dirty` had no hook on a
  component's change ticks, any policy whose `DetectSummary::bits` named a
  capability was flagged **every** wake pass. `run_flow_sleep` then re-evaluated
  its condition — a full `bind_brink_query` round trip — **once per parked flow
  per frame**, purely to (almost always) learn the condition is still false.
  For N locked doors that is N condition re-evaluations every frame.
- **After (this change):** a `detect_capability_changes::<M, C>` tracker (wired
  per registered component by `register_capability`) runs one
  `Query<(), Changed<C>>::is_empty()` check per frame — **shared across every
  flow watching that component**, and near-free on a quiet frame (bevy skips
  tables whose change tick didn't advance). While the component is unchanged,
  `mark_wake_dirty` flags **zero** of the N parked flows, so `run_flow_sleep`
  performs **zero** condition re-evaluations. The moment the component changes,
  the tracker's verdict flips and every watching flow is re-evaluated that frame
  (no missed wake). Marginal per-parked-flow cost while unchanged: 1 re-eval →
  0.

### The per-evaluation cost avoided

`demos/compound`'s `ink_doors::tests::measure_ink_door_wake_cost` prints the
cost of a single `should_open` re-evaluation (the `bind_brink_query` round trip
that was paid per parked door per frame under the must-poll interim) against a
trivial Rust bool-scan baseline — run
`cargo test -p compound measure_ink_door_wake_cost -- --nocapture` for the
machine-specific figure (the absolute wall-clock number is provisional and
deliberately not pinned here, exactly like the BH-3 driver baselines are
regenerated at the next quiet-window canonical run).

The load-bearing before/after is **structural**, not that per-call µs figure:
under the interim that per-call cost was paid `N_locked_doors × frames` (one
condition re-eval per parked door every frame); under the §12.5 cheap path it
is paid `0 × frames` while switches are idle, replaced by the single shared
`Query<(), Changed<Switch>>::is_empty()` tick check per frame. N re-evals/frame
→ 0 while unchanged, plus exactly one missed-wake-free wake on change — that is
what #996 delivers, and it is asserted by
`crate::sleep::tests::detect_capable_component_policy_is_not_reevaluated_while_unchanged`
(no re-eval when unchanged) and
`detect_capable_component_condition_wakes_on_component_change` (wakes on change).
