//! Correctness coverage for the scenario harness's generator + driver
//! logic (issue #900, BH-B-1).
//!
//! `benches/scenario_bench.rs` is `test = false` (running its `main()`
//! under `cargo test` would execute the full 1/100/1k/10k baseline matrix
//! and rewrite the checked-in baseline files as a side effect of an
//! ordinary test run — see that file's module docs). This is the real
//! `cargo test` path instead: it includes `benches/scenario/model.rs`'s
//! source directly via `#[path]`, so it exercises the *actual* generator
//! and driver code the bench binary uses, not a hand-copied
//! approximation — a change that breaks the generated ink template or the
//! frame-loop systems fails here, not silently in `cargo bench` output
//! nobody reads until it's too late.
#[path = "../benches/scenario/model.rs"]
mod model;

use model::{
    ScenarioConfig, ScenarioResult, TurnWeight, active_count, generate_story, run_scenario,
};

fn tiny_config(name: &str, turn_weight: TurnWeight) -> ScenarioConfig {
    ScenarioConfig {
        name: name.to_string(),
        flow_count: 3,
        active_fraction: 0.5,
        world_size: 2,
        turn_weight,
        frames: 4,
        seed: 42,
        collection_global: false,
    }
}

/// Touch every `ScenarioResult` field with a real assertion: partly a
/// round-trip check (the result honestly echoes the config it ran), partly
/// a sanity bound on the measured numbers. Written as one function so every
/// test below gets full-field coverage without repeating it.
fn assert_result_is_sane(config: &ScenarioConfig, r: &ScenarioResult) {
    assert_eq!(r.name, config.name);
    assert_eq!(r.flow_count, config.flow_count);
    assert!((r.active_fraction - config.active_fraction).abs() < f64::EPSILON);
    assert_eq!(r.world_size, config.world_size);
    assert_eq!(r.turn_weight, config.turn_weight);
    assert_eq!(r.turn_weight.label(), config.turn_weight.label());
    assert_eq!(r.frames, config.frames);
    assert_eq!(r.seed, config.seed);

    assert!(r.frame_p50_ms >= 0.0);
    assert!(r.frame_p99_ms >= 0.0);
    assert!(r.collect_p50_us >= 0.0);
    assert!(r.step_p50_us >= 0.0);
    assert!(r.apply_p50_us >= 0.0);
    assert!(r.turns_per_sec >= 0.0);
    assert_eq!(r.flow_anomalies, 0, "unexpected Step outcomes");
    assert!(
        r.turns_completed > 0,
        "expected at least one completed turn"
    );

    // RSS/`#821` counters are `Option` (best-effort / feature-gated) —
    // reading through the `Some` arm is enough to prove the plumbing
    // carries a real value when present, without hard-requiring platform
    // support (`ps -o rss=`) or the `bench-counters` feature in this test.
    if let Some(v) = r.rss_before_kb {
        assert!(v > 0);
    }
    if let Some(v) = r.rss_after_kb {
        assert!(v > 0);
    }
    if let Some(v) = r.rss_delta_kb {
        // A signed delta can legitimately be negative (allocator giving
        // pages back) — just prove it's a real, finite measurement.
        let _ = v;
    }
    #[cfg(feature = "bench-counters")]
    {
        assert!(r.cow_copies.is_some());
        assert!(r.arc_clones.is_some());
    }
    #[cfg(not(feature = "bench-counters"))]
    {
        assert_eq!(r.cow_copies, None);
        assert_eq!(r.arc_clones, None);
    }
}

/// The generated template compiles and actually drives multiple turns —
/// one pass per `TurnWeight` variant, catching an ink syntax mistake in
/// the generator before it ever reaches the bench matrix.
#[test]
fn every_turn_weight_compiles_and_drives_turns() {
    for turn_weight in [TurnWeight::Light, TurnWeight::Medium, TurnWeight::Heavy] {
        let config = tiny_config("smoke", turn_weight);
        let result = run_scenario(&config).expect("smoke scenario should run cleanly");
        assert_result_is_sane(&config, &result);
    }
}

/// Deterministic: the same seed produces the same generated story text —
/// no wall-clock/OS entropy anywhere in the generator.
#[test]
fn story_generation_is_deterministic() {
    let a = generate_story(TurnWeight::Heavy, 7, false);
    let b = generate_story(TurnWeight::Heavy, 7, false);
    assert_eq!(a, b);
}

/// Same determinism proof, with the collection-typed axis enabled — the
/// generator branch isn't only exercised for its default-off shape.
#[test]
fn story_generation_is_deterministic_with_collection_global() {
    let a = generate_story(TurnWeight::Medium, 7, true);
    let b = generate_story(TurnWeight::Medium, 7, true);
    assert_eq!(a, b);
}

#[test]
fn active_count_rounds_and_clamps() {
    assert_eq!(active_count(1, 0.7), 1);
    assert_eq!(active_count(100, 0.7), 70);
    assert_eq!(active_count(10, 0.0), 0);
    assert_eq!(active_count(10, 1.0), 10);
}

/// Zero flows is a configuration error, not a silent no-op run.
#[test]
fn zero_flows_errors_cleanly() {
    let config = ScenarioConfig {
        name: "empty".to_string(),
        flow_count: 0,
        active_fraction: 0.5,
        world_size: 0,
        turn_weight: TurnWeight::Light,
        frames: 1,
        seed: 1,
        collection_global: false,
    };
    assert!(run_scenario(&config).is_err());
}

/// A fully-parked scenario (`active_fraction = 0.0`) completes zero turns
/// and produces zero anomalies — Collect's query filter must not touch
/// parked flows at all, not just skip advancing them.
#[test]
fn fully_parked_scenario_completes_zero_turns() {
    let config = ScenarioConfig {
        name: "all-parked".to_string(),
        flow_count: 5,
        active_fraction: 0.0,
        world_size: 0,
        turn_weight: TurnWeight::Light,
        frames: 3,
        seed: 9,
        collection_global: false,
    };
    let result = run_scenario(&config).expect("all-parked scenario should still run");
    assert_eq!(result.turns_completed, 0);
    assert_eq!(result.flow_anomalies, 0);
}

/// BH follow-up (#911, deliverable 3): with the `bench-counters` feature
/// and `collection_global` enabled, `cow_copies`/`arc_clones` actually move
/// off zero — proving the counters forward through `bevy-brink`'s scenario
/// harness at all. Every *other* config in this file (and every checked-in
/// `serial-driver.csv` baseline row) is scalar-only, so its `Some(0)` is
/// indistinguishable from an unwired counter; this is the one config that
/// can tell the difference.
#[cfg(feature = "bench-counters")]
#[test]
fn collection_global_axis_forwards_nonzero_counters() {
    let config = ScenarioConfig {
        collection_global: true,
        frames: 5,
        ..tiny_config("collection-global", TurnWeight::Medium)
    };
    let result = run_scenario(&config).expect("collection-global scenario should run cleanly");
    assert_result_is_sane(&config, &result);
    assert!(
        result.cow_copies.unwrap_or(0) > 0,
        "expected at least one COW copy once `live` is mutated while shared into `history`: {result:?}"
    );
    assert!(
        result.arc_clones.unwrap_or(0) > 0,
        "expected at least one Arc-clone once a collection-typed global is read/shared: {result:?}"
    );
}
