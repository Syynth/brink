//! Scenario harness skeleton + SERIAL-driver baselines (issue #900, BH-B-1).
//!
//! `#897`'s bevy host track needs a scenario harness before `BH-3` (the
//! parallel step phase) exists, so the eventual "parallel ≡ serial-in-
//! flow-id-order" determinism law has a real serial baseline to be judged
//! against — the 2026-07-16 BASELINE-FIRST ruling. This is that baseline:
//! a **headless `MinimalPlugins` runner** driving today's serial per-flow
//! driver ([`FlowInstance::advance`](bevy_brink::FlowInstance)/`choose`,
//! via [`BrinkFlow`](bevy_brink::BrinkFlow)) through a matrix of synthetic
//! scenarios, timed per-phase. The reusable core lives in
//! `benches/scenario/model.rs` (config, generator, frame-loop systems,
//! driver); `benches/scenario/report.rs` adds the CLI + CSV/markdown
//! output on top.
//!
//! # Honesty — what this harness can and cannot see
//!
//! `docs/effects-spec.md` §12.1 names the eventual frame loop: **Collect**
//! (spawned + woken) → Schedule → Prefetch → **Step** (parallel, pure VM
//! against borrowed reads) → **Apply** (buffered writes, flow-id order) →
//! Subscribe → Detect. None of Schedule/Prefetch/Subscribe/Detect exist yet
//! (`BH-1` rows, `BH-3` parallel step, `BH-4` wake contract are all
//! unbuilt) — this harness only instruments the three phases that already
//! have a real implementation to time:
//!
//! - **Collect** = an ECS query filter (`Without<ScenarioParked>`) building
//!   the active-flow list. Honest limitation: "parked" here is a **static**
//!   partition assigned once at scenario setup, not the real wake-dependency
//!   set `BH-4` will build — this measures whether Collect's cost tracks
//!   `active_count` or `flow_count`, nothing about *which* flows wake.
//! - **Step** = calls `BrinkFlow::advance_until_terminal` for every
//!   collected flow, one at a time, on the main thread. This **is** the
//!   entire "SERIAL driver" the ruling asks to baseline — no task pool, no
//!   `UnsafeWorldCell`, no row-based access proof. `BH-3`'s parallel step
//!   is judged against these numbers later.
//! - **Apply** = bookkeeping only (a frame counter bump). Ink writes today
//!   land immediately inside Step via `ContextView` mutation — there is no
//!   buffered-write commit to time yet (`BH-2`). This phase is a
//!   placeholder seam for when one exists, not a real cost today; reported
//!   as such rather than silently inflated or omitted.
//!
//! The **`world_size`** axis spawns inert background entities to stress
//! ECS storage/query overhead at scale; it does **not** yet interact with
//! per-flow row/access sets (`BH-1` isn't wired to real Bevy components),
//! so it cannot yet show anything about scheduling contention — it exists
//! so a future `BH-3` PR can extend this same harness instead of building a
//! second one. The **`active_fraction`** axis is a fixed active:parked
//! split assigned once at spawn (see Collect, above). **`turn_weight`**
//! varies the generated ink program's per-turn workload (text volume, one
//! var mutation, one inline conditional).
//!
//! Oracle untouched: this harness never touches `tests/tier{1,2,3}` or the
//! oracle snapshot corpus; the synthetic story is generated in memory.
//!
//! # Running
//!
//! ```sh
//! cargo bench -p bevy-brink --bench scenario_bench
//! # Fewer frames for a quick sanity pass:
//! cargo bench -p bevy-brink --bench scenario_bench -- --frames 5
//! # With the #821 Arc-clone/COW-copy debug counters:
//! cargo bench -p bevy-brink --features bench-counters --bench scenario_bench
//! # BH-2 batch-mode driver (advance_batch) over the same axis matrix:
//! cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode batch
//! # BH-3 parallel driver (advance_batch_parallel) over the same axis matrix:
//! cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode parallel
//! # Thread-curve exploration (one ComputeTaskPool size per process; prints
//! # only, never writes baseline files):
//! cargo bench -p bevy-brink --features bench-counters --bench scenario_bench -- --mode parallel --compute-threads 2
//! ```
//!
//! Writes `benches/baselines/serial-driver.csv` and
//! `benches/baselines/serial-driver.md` (relative to this crate's root) —
//! the in-repo SERIAL baselines at flow counts 1/100/1k/10k. `--mode batch`
//! runs BH-2's batch driver (`advance_batch`, #914) over the same matrix
//! instead and writes `benches/baselines/batch-serial-driver.{csv,md}`;
//! `--mode parallel` runs BH-3's parallel driver (`advance_batch_parallel`,
//! #927) and writes `benches/baselines/parallel-driver.{csv,md}` — each
//! mode owns its file pair and never touches the others'. See
//! `docs/bevy-bench.md` for the captured baselines and regeneration
//! instructions.
//!
//! This target is `test = false` in `Cargo.toml`: `cargo test` never runs
//! it (running `main()` would execute the whole baseline matrix and
//! rewrite the checked-in baseline files as a side effect of an ordinary
//! test run). Real `cargo test` coverage of the generator/driver logic
//! lives in `tests/scenario_bench_model.rs`, which includes
//! `scenario/model.rs`'s source directly.

#[path = "scenario/model.rs"]
mod model;
#[path = "scenario/report.rs"]
mod report;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    report::run()
}
