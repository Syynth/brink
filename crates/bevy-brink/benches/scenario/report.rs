//! CLI parsing, the checked-in baseline matrix, and CSV/markdown report
//! writers for the scenario harness (issue #900, BH-B-1). See
//! `benches/scenario_bench.rs`'s module docs for the full write-up.
#![expect(
    clippy::print_stdout,
    reason = "benchmark harness: the printed matrix is the product (same stance as compile_bench/runtime-bench)"
)]

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::model::{
    ScenarioConfig, ScenarioResult, TurnWeight, WakeConfig, WakeKind, compute_pool_threads,
    run_batch_scenario, run_parallel_scenario, run_scenario, run_wake_scenario,
};

// ── CSV + markdown matrix output ────────────────────────────────────────

const CSV_HEADER: &str = "name,flow_count,active_fraction,world_size,turn_weight,frames,seed,\
frame_p50_ms,frame_p99_ms,collect_p50_us,step_p50_us,apply_p50_us,turns_per_sec,\
turns_completed,flow_anomalies,rss_before_kb,rss_after_kb,rss_delta_kb,cow_copies,arc_clones";

fn opt_u64(v: Option<u64>) -> String {
    v.map_or_else(|| "n/a".to_string(), |x| x.to_string())
}
fn opt_i64(v: Option<i64>) -> String {
    v.map_or_else(|| "n/a".to_string(), |x| x.to_string())
}

fn csv_row(r: &ScenarioResult) -> String {
    format!(
        "{},{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{},{},{},{},{},{},{}",
        r.name,
        r.flow_count,
        r.active_fraction,
        r.world_size,
        r.turn_weight.label(),
        r.frames,
        r.seed,
        r.frame_p50_ms,
        r.frame_p99_ms,
        r.collect_p50_us,
        r.step_p50_us,
        r.apply_p50_us,
        r.turns_per_sec,
        r.turns_completed,
        r.flow_anomalies,
        opt_u64(r.rss_before_kb),
        opt_u64(r.rss_after_kb),
        opt_i64(r.rss_delta_kb),
        opt_u64(r.cow_copies),
        opt_u64(r.arc_clones),
    )
}

fn write_csv(results: &[ScenarioResult], path: &std::path::Path) -> io::Result<()> {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for r in results {
        out.push_str(&csv_row(r));
        out.push('\n');
    }
    fs::write(path, out)
}

fn write_markdown(
    results: &[ScenarioResult],
    path: &std::path::Path,
    title: &str,
    preamble: &str,
) -> io::Result<()> {
    let mut out = String::new();
    let _ = writeln!(out, "# {title}\n");
    out.push_str(preamble);
    out.push_str("\n\n");
    out.push_str(
        "| scenario | flows | active | world | turn | frames | frame p50 (ms) | frame p99 (ms) | collect p50 (µs) | step p50 (µs) | apply p50 (µs) | turns/sec | turns | anomalies | rss Δ (KB) | cow_copies | arc_clones |\n",
    );
    out.push_str(
        "|---|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in results {
        let _ = writeln!(
            out,
            "| {} | {} | {:.0}% | {} | {} | {} | {:.3} | {:.3} | {:.2} | {:.2} | {:.2} | {:.1} | {} | {} | {} | {} | {} |",
            r.name,
            r.flow_count,
            r.active_fraction * 100.0,
            r.world_size,
            r.turn_weight.label(),
            r.frames,
            r.frame_p50_ms,
            r.frame_p99_ms,
            r.collect_p50_us,
            r.step_p50_us,
            r.apply_p50_us,
            r.turns_per_sec,
            r.turns_completed,
            r.flow_anomalies,
            opt_i64(r.rss_delta_kb),
            opt_u64(r.cow_copies),
            opt_u64(r.arc_clones),
        );
    }
    fs::write(path, out)
}

// ── Baseline matrix + CLI ───────────────────────────────────────────────

const DEFAULT_FRAMES: usize = 30;
/// Fixed active:parked ratio and turn weight for the checked-in baselines
/// — only `flow_count` varies, per the issue's "baselines at 1/100/1k/10k
/// flows" ask. `world_size` is held at 0 so these baselines isolate
/// flow-count scaling cleanly; non-zero `world_size` is an exploration
/// axis for later `BH-3` work, not this seed's baseline matrix.
const BASELINE_ACTIVE_FRACTION: f64 = 0.7;
const BASELINE_WORLD_SIZE: usize = 0;
const BASELINE_TURN_WEIGHT: TurnWeight = TurnWeight::Medium;
const BASELINE_SEED: u64 = 0x5CEE_0900_BEEF_CAFE;

/// Which scenario driver the run exercises — the serial per-flow loop
/// (issue #900's baselines), BH-2's batch driver (`advance_batch`, issue
/// #914), or BH-3's parallel driver (`advance_batch_parallel`, issue #927).
/// Selected with `--mode serial|batch|parallel`; each mode writes its own
/// baseline file pair so regenerating one never clobbers the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverMode {
    Serial,
    Batch,
    Parallel,
    Wake,
}

impl DriverMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::Batch => "batch",
            Self::Parallel => "parallel",
            Self::Wake => "wake",
        }
    }

    /// Baseline file stem under `benches/baselines/`.
    const fn file_stem(self) -> &'static str {
        match self {
            Self::Serial => "serial-driver",
            Self::Batch => "batch-serial-driver",
            Self::Parallel => "parallel-driver",
            Self::Wake => "wake-driver",
        }
    }
}

/// Parsed CLI: frame count, driver mode, and the parallel mode's optional
/// `ComputeTaskPool` size override (`--compute-threads N`, thread-curve
/// exploration runs — see `run()` for why those never write baseline files).
struct Cli {
    frames: usize,
    mode: DriverMode,
    compute_threads: Option<usize>,
}

fn parse_cli() -> Result<Cli, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut frames = DEFAULT_FRAMES;
    let mut mode = DriverMode::Serial;
    let mut compute_threads = None;
    for w in args.windows(2) {
        if w[0] == "--frames" {
            frames = w[1].parse()?;
        }
        if w[0] == "--mode" {
            mode = match w[1].as_str() {
                "serial" => DriverMode::Serial,
                "batch" => DriverMode::Batch,
                "parallel" => DriverMode::Parallel,
                "wake" => DriverMode::Wake,
                other => {
                    return Err(format!(
                        "unknown --mode `{other}` (expected serial|batch|parallel|wake)"
                    )
                    .into());
                }
            };
        }
        if w[0] == "--compute-threads" {
            compute_threads = Some(w[1].parse()?);
        }
    }
    if compute_threads.is_some() && mode != DriverMode::Parallel {
        return Err("--compute-threads only applies to --mode parallel \
             (the serial/batch drivers never touch ComputeTaskPool)"
            .into());
    }
    Ok(Cli {
        frames,
        mode,
        compute_threads,
    })
}

/// The four checked-in flow-count baselines (`turn_weight` fixed at
/// [`BASELINE_TURN_WEIGHT`]), plus two small supplementary rows that
/// exercise the `turn_weight` axis itself (`Light`/`Heavy`) at a fixed,
/// modest flow count — proving the axis is actually wired end to end
/// (generator → compiled program → driven turns), not merely declared in
/// `ScenarioConfig` and never reached. Row names carry the mode's label
/// prefix (`serial-*` / `batch-*` / `parallel-*`) so each mode's baseline
/// file stays self-describing.
fn baseline_configs(frames: usize, prefix: &str) -> Vec<ScenarioConfig> {
    let flow_count_rows = [("1", 1), ("100", 100), ("1k", 1_000), ("10k", 10_000)]
        .into_iter()
        .map(|(suffix, flow_count)| ScenarioConfig {
            name: format!("{prefix}-{suffix}"),
            flow_count,
            active_fraction: BASELINE_ACTIVE_FRACTION,
            world_size: BASELINE_WORLD_SIZE,
            turn_weight: BASELINE_TURN_WEIGHT,
            frames,
            seed: BASELINE_SEED,
            // The checked-in baselines stay scalar-only (docs/bevy-bench.md's
            // honesty note on cow_copies/arc_clones reading 0 here) — the
            // collection-typed axis is a separate exploration path (#911),
            // proven to move the counters in tests/scenario_bench_model.rs
            // instead of in this checked-in matrix.
            collection_global: false,
        });
    let turn_weight_rows = [
        ("100-light", TurnWeight::Light),
        ("100-heavy", TurnWeight::Heavy),
    ]
    .into_iter()
    .map(|(suffix, turn_weight)| ScenarioConfig {
        name: format!("{prefix}-{suffix}"),
        flow_count: 100,
        active_fraction: BASELINE_ACTIVE_FRACTION,
        world_size: BASELINE_WORLD_SIZE,
        turn_weight,
        frames,
        seed: BASELINE_SEED,
        collection_global: false,
    });
    flow_count_rows.chain(turn_weight_rows).collect()
}

/// The wake-driver matrix (BH-4, #973). Four groups:
///
/// - **idle-detect** (`active=0`, sleeping detect-capable, no batch turn): the
///   zero-cost-parked residual — the per-frame cost of N purely-sleeping
///   detect-capable flows on a quiet World (after the one-time bootstrap
///   evaluation, `mark_wake_dirty` flags nothing → the wake systems are a bare
///   query scan). Sweeps 0/100/1k/10k so the per-sleeper slope is readable, with
///   the `0` row as the "N absent" zero baseline.
/// - **idle-poll** (same, must-poll): the contrast — a non-empty detect bit
///   forces `run_flow_sleep` to re-evaluate every sleeper's condition every
///   frame (the `#996` interim). idle-poll − idle-detect at a fixed N is the
///   price of the missing component-tick wiring.
/// - **ratio** (mixed, batch turn running): the headline. A mostly-sleeping
///   population under `advance_batch` costs like its **active** subset, not its
///   total — parked flows are skipped by Collect. 90:10 and 99:1 at 100/1k/10k,
///   to be read against the `batch-serial-driver` baselines at the same *total*.
/// - **storm** (all sleeping, persistent + always-true + must-poll, batch turn):
///   the thundering herd — the population re-wakes and steps under the driver.
fn wake_configs(frames: usize) -> Vec<WakeConfig> {
    let mk = |name: &str, active: usize, sleeping: usize, kind: WakeKind, drive_batch: bool| {
        WakeConfig {
            name: name.to_string(),
            active,
            sleeping,
            kind,
            drive_batch,
            turn_weight: BASELINE_TURN_WEIGHT,
            frames,
            seed: BASELINE_SEED,
        }
    };
    vec![
        // ── idle-detect: zero-cost-parked residual (no batch turn) ──
        mk("wake-idle-detect-0", 0, 0, WakeKind::DetectSkip, false),
        mk("wake-idle-detect-100", 0, 100, WakeKind::DetectSkip, false),
        mk("wake-idle-detect-1k", 0, 1_000, WakeKind::DetectSkip, false),
        mk(
            "wake-idle-detect-10k",
            0,
            10_000,
            WakeKind::DetectSkip,
            false,
        ),
        // ── idle-poll: must-poll contrast (no batch turn) ──
        mk("wake-idle-poll-100", 0, 100, WakeKind::MustPoll, false),
        mk("wake-idle-poll-1k", 0, 1_000, WakeKind::MustPoll, false),
        mk("wake-idle-poll-10k", 0, 10_000, WakeKind::MustPoll, false),
        // ── ratio: the headline (batch turn; parked detect-capable) ──
        mk("wake-100-90to10", 10, 90, WakeKind::DetectSkip, true),
        mk("wake-100-99to1", 1, 99, WakeKind::DetectSkip, true),
        mk("wake-1k-90to10", 100, 900, WakeKind::DetectSkip, true),
        mk("wake-1k-99to1", 10, 990, WakeKind::DetectSkip, true),
        mk("wake-10k-90to10", 1_000, 9_000, WakeKind::DetectSkip, true),
        mk("wake-10k-99to1", 100, 9_900, WakeKind::DetectSkip, true),
        // ── storm: thundering herd (batch turn; all always-wake) ──
        // Capped at 1k: a woken flow reaching `-> DONE` fires
        // `gc_on_turn_done`, whose handle-registry sweep is O(total flows), so
        // a storm is O(n²); at 10k that is ~27 s/frame (see the header's gc
        // note). 100/1k keep the canonical run tractable while still exposing
        // the quadratic — the 10k point is reported by extrapolation.
        mk("wake-storm-100", 0, 100, WakeKind::Storm, true),
        mk("wake-storm-1k", 0, 1_000, WakeKind::Storm, true),
    ]
}

const WAKE_MD_TITLE: &str = "Scenario harness — WAKE-driver baselines (BH-4, issue #973)";
const WAKE_MD_PREAMBLE: &str = "Generated by `cargo bench -p bevy-brink --bench scenario_bench -- --mode wake`. \
     The reactive-sleep wake contract (`docs/effects-spec.md` §13.1): a `FlowSleep` policy \
     parks a flow at its natural `-> DONE` yield; a **parked** flow is skipped by Collect (it \
     steps zero times, §13.1 point 1), and the plugin's `mark_wake_dirty` → `run_flow_sleep` \
     systems re-evaluate its pure condition — only when a dependency moved (`#913` detect verdict) \
     — and wake it only on condition-true. Column mapping: `frame p50/p99` is the whole reactive \
     frame (the wake systems over the sleeping population plus, on a `drive_batch` row, the batch \
     turn that steps only the active/woken flows); `step p50` is the timed `advance_batch` turn \
     alone (0 on an idle-wake row that runs no batch turn); `collect`/`apply` read 0. \
     \
     **Two regimes — read them differently.** (1) The `wake-idle-*` rows run **no** batch turn: \
     they isolate the pure per-frame cost of the wake systems over a purely-sleeping population on \
     a quiet World, and are the clean **zero-cost-parked** measurement. `wake-idle-detect-*` are \
     detect-capable (re-evaluated only on a World change — none here, so after one bootstrap \
     evaluation they cost only a query scan); `wake-idle-poll-*` force the must-poll cadence \
     (`#996` interim: a component-backed condition re-evaluates every frame). These are linear in \
     the sleeping count and are the headline. (2) The `wake-*-{90to10,99to1}` and `wake-storm-*` \
     rows **do** run the batch turn: a woken/active flow steps to `-> DONE`, which fires the \
     plugin's `gc_on_turn_done` handle-registry observer — and that observer sweeps **every flow \
     under M** (T1d reachable-handle GC), i.e. it is **O(total flows) per `-> DONE`**. So a \
     `drive_batch` row's cost is O(active × total) and a wake-storm is O(n²); at these sizes the \
     gc sweep, **not** the wake systems, dominates the frame. This is a T1d handle-GC interaction \
     orthogonal to the wake contract (the batch-serial baselines avoid it only because their \
     synthetic story parks at a *choice*, never `-> DONE`); it is called out in the hand-maintained \
     header and is the reason `wake-storm` is capped at 1k. See `docs/bevy-bench.md` for the \
     shared honesty caveats.";

const SERIAL_MD_TITLE: &str = "Scenario harness — SERIAL-driver baselines (issue #900)";
const SERIAL_MD_PREAMBLE: &str = "Generated by `cargo bench -p bevy-brink --bench scenario_bench`. See \
     `docs/bevy-bench.md` for the honesty caveats (what each column can and \
     can't see) and regeneration instructions. Absolute numbers are \
     machine-specific and will drift — this is a baseline for `BH-3`'s \
     later parallel-vs-serial comparison, not a strict pass/fail gate.";

const PARALLEL_MD_TITLE: &str = "Scenario harness — PARALLEL-driver baselines (BH-3, issue #927)";
const PARALLEL_MD_PREAMBLE: &str = "Generated by `cargo bench -p bevy-brink --bench scenario_bench -- --mode parallel`. \
     Same axes, story, seed, and setup as `batch-serial-driver.md`, but the batch \
     turn runs through `advance_batch_parallel` (BH-3, #927): the Step phase on \
     `ComputeTaskPool` through an `UnsafeWorldCell`, with Collect, per-flow Step, \
     and the flow-id-ordered Apply shared verbatim with the serial `advance_batch` \
     (the determinism law: parallel ≡ serial-in-flow-id-order, byte-identical). \
     Column mapping matches batch mode: `step p50` is the whole batch turn \
     (command flush included), `apply p50` is the harness's host-side auto-choose \
     pass, `collect p50` reads 0. The per-flow frame-start snapshot clone is still \
     paid — per task instead of per loop iteration (§12.2 \"borrow, don't copy\" \
     remains the follow-up, #937) — so expect parity-or-worse at low flow counts \
     (task-spawn overhead) and the win to appear as Step outgrows the fixed \
     per-turn cost. The `ComputeTaskPool` thread count is process-global and \
     printed by the run (`compute_task_pool_threads=`); record it with any \
     capture. See `docs/bevy-bench.md` for the shared honesty caveats.";

const BATCH_MD_TITLE: &str = "Scenario harness — BATCH-serial-driver baselines (BH-2, issue #914)";
const BATCH_MD_PREAMBLE: &str = "Generated by `cargo bench -p bevy-brink --bench scenario_bench -- --mode batch`. \
     Same axes, story, and seed as `serial-driver.md`, but flows advance through \
     `advance_batch` (frame-start read pinning, per-flow buffered writes/commands, \
     flow-id-ordered Apply) instead of the serial per-flow loop. Column mapping for \
     this mode: `step p50` is the **whole batch turn** (Collect/Step/Apply are fused \
     inside `advance_batch`, command flush included), `apply p50` is the harness's \
     host-side auto-choose pass, and `collect p50` reads 0 (not separately \
     measurable from outside the driver). The per-flow frame-start snapshot clone is \
     BH-2's documented serial cost — §12.2's \"borrow, don't copy\" is the BH-3 \
     optimization — so batch is expected to sit above serial until BH-3 lands. See \
     `docs/bevy-bench.md` for the shared honesty caveats.";

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        frames,
        mode,
        compute_threads,
    } = parse_cli()?;
    let label = mode.label();
    println!(
        "scenario_bench | frames={frames} | mode={label} | headless MinimalPlugins runner (issue #900 / #914 / #927 / #973)"
    );
    println!(
        "scenario_bench | scenario | flow_count | frame_p50_ms | frame_p99_ms | turns_per_sec | rss_delta_kb"
    );

    if mode == DriverMode::Wake {
        return run_wake(frames);
    }

    let mut results = Vec::new();
    for config in baseline_configs(frames, label) {
        let r = match mode {
            DriverMode::Serial => run_scenario(&config)?,
            DriverMode::Batch => run_batch_scenario(&config)?,
            DriverMode::Parallel => run_parallel_scenario(&config, compute_threads)?,
            // Wake mode returns early via `run_wake` before this loop; it never
            // reaches the baseline-matrix path.
            DriverMode::Wake => {
                return Err("wake mode is dispatched by run_wake, not the baseline loop".into());
            }
        };
        println!(
            "scenario_bench | {:<11} | {:>10} | {:>12.3} | {:>12.3} | {:>13.1} | {:>12}",
            r.name,
            r.flow_count,
            r.frame_p50_ms,
            r.frame_p99_ms,
            r.turns_per_sec,
            opt_i64(r.rss_delta_kb),
        );
        if r.flow_anomalies > 0 {
            println!(
                "scenario_bench | WARNING: {} flow anomalies in {} (unexpected Step outcomes — see module docs)",
                r.flow_anomalies, r.name
            );
        }
        results.push(r);
    }

    if mode == DriverMode::Parallel {
        // Process-global pool, initialized by the first scenario's App —
        // report what the parallel Step actually ran with (see the
        // machine-context requirement on any canonical capture).
        let threads =
            compute_pool_threads().map_or_else(|| "unknown".to_string(), |n| n.to_string());
        println!("scenario_bench | compute_task_pool_threads={threads}");
    }

    if compute_threads.is_some() {
        // Thread-curve exploration run: the numbers above are the product;
        // never let a non-default pool size overwrite the checked-in
        // baseline pair (which is captured at bevy's default pool size).
        println!(
            "scenario_bench | --compute-threads override active: exploration run, baseline files NOT written"
        );
        return Ok(());
    }

    let (title, preamble) = match mode {
        DriverMode::Serial => (SERIAL_MD_TITLE, SERIAL_MD_PREAMBLE),
        DriverMode::Batch => (BATCH_MD_TITLE, BATCH_MD_PREAMBLE),
        DriverMode::Parallel => (PARALLEL_MD_TITLE, PARALLEL_MD_PREAMBLE),
        // Wake mode returns early (`run_wake`); this arm only keeps the match
        // exhaustive.
        DriverMode::Wake => (WAKE_MD_TITLE, WAKE_MD_PREAMBLE),
    };
    let stem = mode.file_stem();
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/baselines");
    fs::create_dir_all(&out_dir)?;
    write_csv(&results, &out_dir.join(format!("{stem}.csv")))?;
    write_markdown(
        &results,
        &out_dir.join(format!("{stem}.md")),
        title,
        preamble,
    )?;
    println!(
        "scenario_bench | wrote {}",
        out_dir.join(format!("{stem}.{{csv,md}}")).display()
    );

    Ok(())
}

/// Run the wake-driver matrix (BH-4, #973) and write `wake-driver.{csv,md}`.
/// Separate from [`run`]'s baseline-matrix loop because the wake axis has its
/// own config type ([`WakeConfig`]) and runner ([`run_wake_scenario`]).
fn run_wake(frames: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    for config in wake_configs(frames) {
        let r = run_wake_scenario(&config)?;
        println!(
            "scenario_bench | {:<20} | {:>10} | {:>12.3} | {:>12.3} | {:>13.1} | {:>12}",
            r.name,
            r.flow_count,
            r.frame_p50_ms,
            r.frame_p99_ms,
            r.turns_per_sec,
            opt_i64(r.rss_delta_kb),
        );
        if r.flow_anomalies > 0 {
            println!(
                "scenario_bench | WARNING: {} flow anomalies in {} (unexpected batch outcomes — see module docs)",
                r.flow_anomalies, r.name
            );
        }
        results.push(r);
    }

    let stem = DriverMode::Wake.file_stem();
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/baselines");
    fs::create_dir_all(&out_dir)?;
    write_csv(&results, &out_dir.join(format!("{stem}.csv")))?;
    write_markdown(
        &results,
        &out_dir.join(format!("{stem}.md")),
        WAKE_MD_TITLE,
        WAKE_MD_PREAMBLE,
    )?;
    println!(
        "scenario_bench | wrote {}",
        out_dir.join(format!("{stem}.{{csv,md}}")).display()
    );

    Ok(())
}
