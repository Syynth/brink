//! BH follow-up (#911, deliverable 4): a baseline-diff tripwire comparing
//! the checked-in `benches/baselines/serial-driver.csv` against a fresh
//! in-test mini-run of the same scenario config — the seed of `BH-3`'s
//! future serial-vs-parallel comparator (`docs/bevy-bench.md`'s "Running"
//! section flagged this as not-yet-built, matching the #821 epic's
//! Workstream D precedent).
//!
//! **Design, given the "advisory/non-required if flaky" ask.** Two
//! different kinds of numbers live in the same CSV row, with very
//! different reproducibility:
//! - `turns_completed`/`flow_anomalies` are **deterministic**: the same
//!   fixed seed and config drive the exact same simulated outcome on any
//!   machine, any load — no wall-clock dependency at all. These are
//!   asserted **exactly**, hard-failing on drift, because a mismatch here
//!   can only mean the harness's *behavior* changed (a generator edit, a
//!   driver bug), not that the test machine is slower or faster today.
//! - `frame_p50_ms`/`turns_per_sec`/etc. are wall-clock timings — they will
//!   legitimately differ from the baseline's capture machine on every CI
//!   runner. These are **reported, not gated**: printed for visibility (the
//!   "report cost" the issue asks for) rather than asserted against a
//!   tolerance band that would eventually flake on a loaded CI box.
//!
//! Only the smallest baseline row (`serial-1`) is re-run here — replaying
//! the full 10k-flow row on every `cargo test` would slow the ordinary test
//! pass considerably for no extra coverage (the driver logic under test is
//! identical at every `flow_count`).
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test fixture parsing: fail loudly on a malformed baseline csv row or an \
              unrecognized turn_weight label rather than silently mis-comparing"
)]

// This test only exercises a slice of `model.rs`'s surface (the smallest
// baseline row, a handful of `ScenarioResult` fields) — the rest of its
// pub items are real, used by `benches/scenario_bench.rs` and
// `tests/scenario_bench_model.rs`, just not from this file. `dead_code`
// would otherwise fire per-binary, since each `#[path]` inclusion compiles
// its own copy of the module.
#[path = "../benches/scenario/model.rs"]
#[expect(
    dead_code,
    reason = "only a slice of model.rs's surface is exercised from this file"
)]
mod model;

use model::{ScenarioConfig, TurnWeight, run_scenario};

const BASELINE_CSV: &str = include_str!("../benches/baselines/serial-driver.csv");

const EXPECTED_CSV_HEADER: &str = "name,flow_count,active_fraction,world_size,turn_weight,frames,seed,\
frame_p50_ms,frame_p99_ms,collect_p50_us,step_p50_us,apply_p50_us,turns_per_sec,\
turns_completed,flow_anomalies,rss_before_kb,rss_after_kb,rss_delta_kb,cow_copies,arc_clones";

// Mirrors `benches/scenario/report.rs`'s baseline constants — kept in sync
// deliberately. A change to either file without the other is exactly the
// drift this test exists to surface (the row's own `active_fraction`/
// `world_size`/`seed` columns are cross-checked against these below, so a
// silent divergence fails loudly here rather than only showing up as an
// unexplained CSV diff at the next `cargo bench` regeneration).
const BASELINE_ACTIVE_FRACTION: f64 = 0.7;
const BASELINE_WORLD_SIZE: usize = 0;
const BASELINE_SEED: u64 = 0x5CEE_0900_BEEF_CAFE;

struct BaselineRow {
    name: String,
    flow_count: usize,
    active_fraction: f64,
    world_size: usize,
    turn_weight: String,
    frames: usize,
    seed: u64,
    frame_p50_ms: f64,
    turns_per_sec: f64,
    turns_completed: u64,
    flow_anomalies: u64,
}

/// Minimal positional CSV parse — the checked-in file has no quoted or
/// comma-bearing fields, so a plain `split(',')` is honest and doesn't need
/// a CSV crate dependency for one test.
fn parse_baseline_csv(text: &str) -> Vec<BaselineRow> {
    let mut lines = text.lines();
    let header = lines.next().expect("baseline csv has a header line");
    assert_eq!(
        header, EXPECTED_CSV_HEADER,
        "benches/scenario/report.rs's CSV_HEADER changed shape without this test's \
         EXPECTED_CSV_HEADER following — update both together"
    );
    lines
        .filter(|l| !l.is_empty())
        .map(|line| {
            let cols: Vec<&str> = line.split(',').collect();
            assert_eq!(
                cols.len(),
                20,
                "baseline csv row has an unexpected column count: {line}"
            );
            BaselineRow {
                name: cols[0].to_string(),
                flow_count: cols[1].parse().expect("flow_count"),
                active_fraction: cols[2].parse().expect("active_fraction"),
                world_size: cols[3].parse().expect("world_size"),
                turn_weight: cols[4].to_string(),
                frames: cols[5].parse().expect("frames"),
                seed: cols[6].parse().expect("seed"),
                frame_p50_ms: cols[7].parse().expect("frame_p50_ms"),
                turns_per_sec: cols[12].parse().expect("turns_per_sec"),
                turns_completed: cols[13].parse().expect("turns_completed"),
                flow_anomalies: cols[14].parse().expect("flow_anomalies"),
            }
        })
        .collect()
}

fn turn_weight_from_label(label: &str) -> TurnWeight {
    match label {
        "light" => TurnWeight::Light,
        "medium" => TurnWeight::Medium,
        "heavy" => TurnWeight::Heavy,
        other => panic!("unknown turn_weight label in baseline csv: {other}"),
    }
}

#[test]
fn checked_in_baseline_matches_a_fresh_mini_run() {
    let rows = parse_baseline_csv(BASELINE_CSV);
    let row = rows
        .iter()
        .find(|r| r.name == "serial-1")
        .expect("serial-1 row present in the checked-in baseline");

    // The row's own axis columns should still match this file's mirrored
    // baseline constants — catching drift between report.rs's
    // `baseline_configs` and this test even before the mini-run below runs.
    assert!(
        (row.active_fraction - BASELINE_ACTIVE_FRACTION).abs() < f64::EPSILON,
        "serial-1's active_fraction column drifted from the mirrored baseline constant"
    );
    assert_eq!(
        row.world_size, BASELINE_WORLD_SIZE,
        "serial-1's world_size column drifted from the mirrored baseline constant"
    );
    assert_eq!(
        row.seed, BASELINE_SEED,
        "serial-1's seed column drifted from the mirrored baseline constant"
    );

    let config = ScenarioConfig {
        name: row.name.clone(),
        flow_count: row.flow_count,
        active_fraction: row.active_fraction,
        world_size: row.world_size,
        story_globals: 0,
        turn_weight: turn_weight_from_label(&row.turn_weight),
        frames: row.frames,
        seed: row.seed,
        collection_global: false,
    };

    let fresh = run_scenario(&config).expect("mini-run should complete cleanly");

    // Deterministic outputs: fixed seed + fixed config -> exact
    // reproduction, no tolerance band needed or wanted.
    assert_eq!(
        fresh.turns_completed, row.turns_completed,
        "turns_completed drifted from the checked-in baseline for {} — if this is an \
         intentional behavior change, regenerate via `cargo bench -p bevy-brink --bench \
         scenario_bench` and check in the refreshed baseline",
        row.name
    );
    assert_eq!(
        fresh.flow_anomalies, row.flow_anomalies,
        "flow_anomalies drifted from the checked-in baseline for {} — an unexpected Step \
         outcome the baseline didn't have",
        row.name
    );

    // Advisory timing: reported for visibility, never gated — see this
    // file's module doc for why (machine-dependent wall-clock, not part of
    // the deterministic-outcome contract this test enforces).
    eprintln!(
        "scenario_baseline_tripwire | {} | baseline frame_p50_ms={:.4} turns_per_sec={:.2} | \
         fresh frame_p50_ms={:.4} turns_per_sec={:.2} (informational only — not gated)",
        row.name, row.frame_p50_ms, row.turns_per_sec, fresh.frame_p50_ms, fresh.turns_per_sec
    );
}
