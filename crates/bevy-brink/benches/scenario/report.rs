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
    ScenarioConfig, ScenarioResult, TurnWeight, compute_pool_threads, run_batch_scenario,
    run_parallel_scenario, run_scenario,
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
}

impl DriverMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::Batch => "batch",
            Self::Parallel => "parallel",
        }
    }

    /// Baseline file stem under `benches/baselines/`.
    const fn file_stem(self) -> &'static str {
        match self {
            Self::Serial => "serial-driver",
            Self::Batch => "batch-serial-driver",
            Self::Parallel => "parallel-driver",
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
                other => {
                    return Err(format!(
                        "unknown --mode `{other}` (expected serial|batch|parallel)"
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
     `ComputeTaskPool` through an `UnsafeWorldCell`. Per-flow Step and the \
     flow-id-ordered Apply are literally the same functions shared with the \
     serial `advance_batch`; Collect is a hand-duplicated query kept \
     filter-identical by hand (#1633) \
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
        "scenario_bench | frames={frames} | mode={label} | headless MinimalPlugins runner (issue #900 / #914 / #927)"
    );
    println!(
        "scenario_bench | scenario | flow_count | frame_p50_ms | frame_p99_ms | turns_per_sec | rss_delta_kb"
    );

    let mut results = Vec::new();
    for config in baseline_configs(frames, label) {
        let r = match mode {
            DriverMode::Serial => run_scenario(&config)?,
            DriverMode::Batch => run_batch_scenario(&config)?,
            DriverMode::Parallel => run_parallel_scenario(&config, compute_threads)?,
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
