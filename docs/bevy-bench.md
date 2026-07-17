# bevy-brink scenario harness — SERIAL-driver baselines

BH-B-1 (issue #900): the scenario harness skeleton + SERIAL-driver
baselines for `#897`'s bevy host track, landed **before `BH-3`** (the
parallel step phase) exists — the 2026-07-16 BASELINE-FIRST ruling. `BH-3`
ships a determinism law ("parallel ≡ serial-in-flow-id-order, byte-identical
over randomized workloads"); that law needs a real serial baseline to be
judged against, so the baseline lands first with a denominator already in
the repo.

## What this is

A **headless `MinimalPlugins` runner** (`crates/bevy-brink/benches/scenario_bench.rs`)
that spawns synthetic ink flows as real `BrinkFlow`/`BrinkContext`/
`BrinkGlobals` components/resources (the same production types
`bevy-brink` consumers use — no parallel test-only context type), drives a
fixed number of `App::update()` frames, and times three phases per frame:
**Collect**, **Step**, **Apply** — the three phases of today's serial
driver, per `docs/effects-spec.md` §12.1's eventual frame loop (Collect →
Schedule → Prefetch → Step → Apply → Subscribe → Detect). Only
Collect/Step/Apply have a real implementation today; the rest (`BH-1` rows,
`BH-3` parallel step, `BH-4` wake contract) are unbuilt.

## Honesty — what each number can and can't see

This is the load-bearing section; read it before trusting any number here.

- **Collect** = an ECS query filter (`Without<ScenarioParked>`) building
  the active-flow list for the frame. "Parked" is a **static** partition
  assigned once at scenario setup — `active_fraction` of flows are marked
  active and advanced every frame; the rest are spawned parked and never
  touched again. This measures whether Collect's cost tracks `active_count`
  or the full `flow_count`, **not** the real wake-dependency machinery
  `BH-4` will build (parked flows there wake on a condition; here they
  never wake at all).
- **Step** = `BrinkFlow::advance_until_terminal` called once per collected
  flow, serially, on the main thread — no task pool, no
  `UnsafeWorldCell`, no row-based access proof. This **is** the "SERIAL
  driver" the ruling asks to baseline; `BH-3`'s parallel step is judged
  against these numbers, not against wall-clock intuition.
- **Apply** = bookkeeping only (a frame-index bump). Ink writes today land
  immediately inside Step, via `ContextView` mutation of the shared
  `BrinkGlobals` `World` — there is no buffered-write commit to time yet
  (`BH-2`). This phase is a placeholder seam for when one exists; its near-
  zero cost here is not a real measurement of anything, and is reported as
  such rather than silently inflated or omitted.
- **`world_size`** spawns inert background entities to stress ECS
  storage/query overhead at scale. It does **not** yet interact with
  per-flow row/access sets — `BH-1` isn't wired to real Bevy components —
  so it can't yet demonstrate anything about scheduling contention. It
  exists in `ScenarioConfig` so a future `BH-3` PR extends this harness
  instead of building a second one. The checked-in baselines hold it at 0.
- **`turn_weight`** (`Light`/`Medium`/`Heavy`) varies the generated ink
  program's per-turn workload (sentence count, one var mutation, one
  inline conditional). The checked-in baselines fix it at `Medium` and vary
  `flow_count` only.
- **`frame p50/p99`** are wall-clock `App::update()` durations (the whole
  bevy schedule for that frame — `tick_world_filler` +
  Collect+Step+Apply + bevy's own command-flush overhead), not just the
  sum of the three phases; the gap between `frame_p50_ms` and
  `collect+step+apply` is bevy scheduling overhead, not attributable to
  the driver phases under test.
- **`turns/sec`** = total completed turns (a flow reaching `Choices` and
  being auto-chosen) divided by total wall time across all frames.
- **RSS** is a coarse, single-process, best-effort proxy (`ps -o rss=`,
  same idiom as `docs/runtime-bench.md`'s #821 Workstream C) — not exact
  per-flow byte accounting (`#538`'s `heap_size` estimators aren't landed).
  Read as "does flow_count scaling roughly track memory linearly, not
  something worse," not a byte-exact tripwire.
- **`cow_copies`/`arc_clones`** are the #821 Arc-clone/COW-copy debug
  counters (`brink-runtime`'s `bench-counters` feature), forwarded through
  `bevy-brink`'s own `bench-counters` feature. `n/a` in every row unless
  that feature is enabled — see "Running," below. **Measured: both are 0
  in every baseline row**, and that is the honest, expected result, not a
  broken counter — the checked-in `turn_weight=medium` template's only
  global is a scalar `Int` (`turn_count`); it never touches an
  `Array`/`Map`/`Record` global, so it never reaches the
  `array_make_mut`/`map_make_mut`/`record_make_mut`/`GetGlobal`-collection
  call sites these counters instrument. **A collection-typed scenario axis
  now exists** (issue #911, `ScenarioConfig::collection_global`): a
  `live`/`history` array pair shared then mutated every turn (the
  `snapshot-retention-g10-m10` bench story's share-then-mutate shape,
  scaled to one generation per turn) — proven to move both counters off
  zero in `crates/bevy-brink/tests/scenario_bench_model.rs`'s
  `collection_global_axis_forwards_nonzero_counters` (feature-gated on
  `bench-counters`, since without it the fields are always `None`). The
  checked-in `serial-driver.csv` baselines below deliberately still hold
  this axis `false` — this is a proof the plumbing carries a real value,
  not a new baseline dimension.
- **`flow_anomalies`** counts any Step outcome other than reaching
  `Choices` cleanly (an error, or an unexpected `Done`/`End`/
  `AwaitingQuery`) — always 0 for a correct run of the generated,
  always-looping template; a nonzero count is a real bug signal, not
  filtered out.

Oracle untouched: the synthetic story is generated in memory; this harness
never reads or writes `tests/tier{1,2,3}` or the oracle snapshot corpus.

## Running

```sh
# Full baseline matrix (1/100/1k/10k flows), default 30 frames each.
cargo bench -p bevy-brink --bench scenario_bench

# Fewer frames for a quick sanity pass.
cargo bench -p bevy-brink --bench scenario_bench -- --frames 5

# With the #821 Arc-clone/COW-copy debug counters.
cargo bench -p bevy-brink --features bench-counters --bench scenario_bench

# BH-2 batch-mode driver (advance_batch, #914) over the same axis matrix.
cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode batch

# BH-3 parallel driver (advance_batch_parallel, #927) over the same axis matrix.
cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode parallel

# Thread-curve exploration: pin the ComputeTaskPool size (one size per
# process — bevy's pools are process-global). Prints results only; never
# writes baseline files, so a non-default pool size can't clobber the
# checked-in pair.
cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode parallel --compute-threads 2
```

Writes `crates/bevy-brink/benches/baselines/serial-driver.csv` and
`serial-driver.md` (the full axis matrix, machine-readable + human-
readable) — these are the in-repo SERIAL baselines this issue asks for.
Update the "Baseline" section below by hand after regenerating.

`--mode batch` runs the same matrix through BH-2's batch driver
(`advance_batch`: frame-start read pinning, per-flow buffered
writes/commands, flow-id-ordered Apply — `docs/effects-spec.md` §12.4) and
writes `batch-serial-driver.{csv,md}` instead, never touching the serial
pair. Batch-mode column mapping (the driver fuses Collect/Step/Apply
inside `advance_batch`, so the harness can't time them separately):
`step p50` is the whole batch turn including its command flush, `apply p50`
is the harness's host-side auto-choose pass, and `collect p50` reads 0.
The canonical batch capture, its machine context, and the batch-vs-serial
comparison live in `batch-serial-driver.md` itself (hand-maintained header
over the generated matrix — re-add after regenerating).

`--mode parallel` runs the matrix through BH-3's parallel driver
(`advance_batch_parallel`, #927: the Step phase on `ComputeTaskPool`
through an `UnsafeWorldCell`; Collect/Step/Apply shared verbatim with
`advance_batch`, so the two are byte-identical — the determinism law) and
writes `parallel-driver.{csv,md}`. Column mapping is identical to batch
mode. The run prints `compute_task_pool_threads=` — bevy's task pools are
process-global, so record that number with any capture; `--compute-threads
N` pins the pool size for thread-curve exploration (print-only, one size
per process). The canonical parallel capture, its machine context, thread
count, and the parallel-vs-serial/-vs-batch comparisons live in
`parallel-driver.md` itself (hand-maintained header over the generated
matrix — re-add after regenerating).

**Baseline-diff tripwire (issue #911, the seed of `BH-3`'s serial-vs-
parallel comparator):**
`crates/bevy-brink/tests/scenario_baseline_tripwire.rs`'s
`checked_in_baseline_matches_a_fresh_mini_run` re-runs the checked-in
`serial-1` row's exact config in an ordinary `cargo test` pass and
compares it against the checked-in CSV. Deterministic fields
(`turns_completed`, `flow_anomalies`) are asserted exactly — same seed,
same config, no wall-clock dependency, so a mismatch means the harness's
*behavior* changed, not that the machine is slower. Timing fields
(`frame_p50_ms`, `turns_per_sec`) are reported (`eprintln!`) for
visibility, never gated — advisory by design, since they will legitimately
differ from the baseline's capture machine on every runner.

## Baseline

Captured 2026-07-16, Apple Silicon dev machine, `cargo bench --features
bench-counters` (the `bench` profile, optimized), default 30 frames,
`active_fraction=0.7`, `world_size=0`. The four `serial-*` rows fix
`turn_weight=medium` and vary `flow_count` only (the issue's ask); the two
`serial-100-{light,heavy}` rows hold `flow_count=100` and vary
`turn_weight` instead, proving that axis is actually wired end to end
rather than merely declared in `ScenarioConfig`. Regenerate via
`crates/bevy-brink/benches/baselines/serial-driver.{csv,md}` (this table
is copied from the generated `.md`, not hand-maintained — regenerate
rather than hand-edit if it drifts).

| scenario | flows | active | world | turn | frames | frame p50 (ms) | frame p99 (ms) | collect p50 (µs) | step p50 (µs) | apply p50 (µs) | turns/sec | turns | anomalies | rss Δ (KB) | cow_copies | arc_clones |
|---|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| serial-1 | 1 | 70% | 0 | medium | 30 | 0.076 | 30.178 | 0.04 | 8.12 | 0.04 | 381.2 | 30 | 0 | 160 | 0 | 0 |
| serial-100 | 100 | 70% | 0 | medium | 30 | 0.594 | 0.861 | 0.29 | 491.71 | 0.04 | 114790.2 | 2100 | 0 | 1600 | 0 | 0 |
| serial-1k | 1000 | 70% | 0 | medium | 30 | 5.353 | 6.770 | 2.12 | 5075.12 | 0.04 | 128468.8 | 21000 | 0 | 14592 | 0 | 0 |
| serial-10k | 10000 | 70% | 0 | medium | 30 | 37.504 | 62.137 | 19.29 | 35102.50 | 0.08 | 181820.8 | 210000 | 0 | 145936 | 0 | 0 |
| serial-100-light | 100 | 70% | 0 | light | 30 | 0.165 | 0.342 | 0.17 | 108.38 | 0.04 | 396806.4 | 2100 | 0 | 16 | 0 | 0 |
| serial-100-heavy | 100 | 70% | 0 | heavy | 30 | 0.502 | 0.747 | 0.12 | 418.75 | 0.04 | 134386.8 | 2100 | 0 | 1088 | 0 | 0 |

Reading these honestly: `step_p50_us` scales roughly linearly with
`flow_count` (≈5 µs/flow at 100, ≈5.1 µs/flow at 1k, ≈3.5 µs/flow at 10k —
the serial per-flow loop's expected shape, no batching to amortize), while
`collect_p50_us` stays tiny relative to `step` at every size (the query
filter itself is cheap; almost all frame time is in Step, as expected for
a driver with no parallelism yet). `frame_p99_ms` at `serial-1` (30.178ms)
is a clear outlier against its own `p50` (0.076ms) — a single slow first
frame (JIT/cache warmup, `MinimalPlugins`' first-frame setup) dominating a
30-frame sample at `flow_count=1`; visible proof that `p99` over a small
frame count is noisy at the smallest scenario size, not a real per-flow
cost. `cow_copies`/`arc_clones` are 0 throughout — see the honesty note
above (the template's only global is a scalar `Int`, never a collection).

**Absolute numbers are machine-specific and will drift — this is `BH-3`'s
future comparison point, not a strict pass/fail gate.**
