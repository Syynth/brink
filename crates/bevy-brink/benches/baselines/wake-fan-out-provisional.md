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
