//! Optimizer measurement over the whole oracle corpus — a report, not a gate.
//!
//! For every oracle-backed case (tier1–3): compile once, optimize a copy with
//! the resident pass set, link both, and replay **every checked-in oracle
//! episode** on both artifacts, summing the VM's `Stats::opcodes`. The
//! episodes are real play paths the C# runtime took, so this is "how many
//! fewer instructions does the optimized story execute on the same play",
//! not a synthetic estimate. Static reductions (bytecode bytes, fusions per
//! pass) are reported alongside.
//!
//! Ignored by default — it is the triage/measurement tool, not a correctness
//! check (the fence, `opt_corpus_fence.rs`, is that):
//!
//! ```sh
//! cargo test -p brink-test-harness --test opt_corpus_stats -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use brink_opt::OptConfig;
use brink_runtime::{DotNetRng, Program, Stats, Step, Story};
use brink_test_harness::corpus::{collect_oracle_cases, has_empty_source, is_compile_error_case};
use brink_test_harness::oracle;

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Same headroom as `runner::RunConfig::default()`.
const MAX_STEPS: usize = 10_000;

/// Replay one choice path and return the VM counters at the end. Errors and
/// exhausted inputs end the replay where they occur — both artifacts hit
/// them at the same point (the fence proves that), so the counters stay
/// comparable.
fn replay(
    program: &Arc<Program>,
    line_tables: &[Vec<brink_format::LineEntry>],
    choice_path: &[usize],
) -> Stats {
    let mut story = Story::<DotNetRng>::new(Arc::clone(program), line_tables.to_vec());
    let mut next_input = 0;
    for _ in 0..MAX_STEPS {
        match story.continue_single() {
            Ok(Step::Line(_)) => {}
            Ok(Step::Choices(_)) => {
                let Some(&pick) = choice_path.get(next_input) else {
                    break;
                };
                next_input += 1;
                if story.choose(pick).is_err() {
                    break;
                }
            }
            Ok(Step::Done | Step::End | Step::Suspended) | Err(_) => break,
        }
    }
    story.stats().clone()
}

#[derive(Default, Clone)]
struct Tally {
    cases: usize,
    episodes: usize,
    opcodes_before: u64,
    opcodes_after: u64,
    bytes_before: usize,
    bytes_after: usize,
    fusions: BTreeMap<&'static str, usize>,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.cases += other.cases;
        self.episodes += other.episodes;
        self.opcodes_before += other.opcodes_before;
        self.opcodes_after += other.opcodes_after;
        self.bytes_before += other.bytes_before;
        self.bytes_after += other.bytes_after;
        for (k, v) in &other.fusions {
            *self.fusions.entry(k).or_default() += v;
        }
    }

    fn reduction(&self) -> f64 {
        if self.opcodes_before == 0 {
            return 0.0;
        }
        #[expect(clippy::cast_precision_loss)]
        let r =
            100.0 * (self.opcodes_before - self.opcodes_after) as f64 / self.opcodes_before as f64;
        r
    }
}

fn measure_case(case_dir: &Path) -> Result<Tally, String> {
    let ink_path = case_dir.join("story.ink");
    let output = brink_compiler::compile_path(&ink_path).map_err(|e| format!("compile: {e}"))?;
    let base = output.data;
    let mut optimized = base.clone();
    let report = brink_opt::optimize(&mut optimized, &OptConfig::defaults());

    let (program_a, tables_a) = brink_runtime::link(&base).map_err(|e| format!("link: {e}"))?;
    let (program_b, tables_b) =
        brink_runtime::link(&optimized).map_err(|e| format!("link: {e}"))?;
    let (program_a, program_b) = (Arc::new(program_a), Arc::new(program_b));

    let episodes = oracle::load_oracle_episodes(case_dir)?;
    let mut tally = Tally {
        cases: 1,
        episodes: episodes.len(),
        bytes_before: base.containers.iter().map(|c| c.bytecode.len()).sum(),
        bytes_after: optimized.containers.iter().map(|c| c.bytecode.len()).sum(),
        ..Tally::default()
    };
    for pass in &report.passes {
        for (unit, count) in &pass.notes {
            let _ = unit;
            *tally.fusions.entry(pass.name).or_default() += count;
        }
    }
    for ep in &episodes {
        tally.opcodes_before += replay(&program_a, &tables_a, &ep.choice_path).opcodes;
        tally.opcodes_after += replay(&program_b, &tables_b, &ep.choice_path).opcodes;
    }
    Ok(tally)
}

fn row(label: &str, t: &Tally) -> String {
    format!(
        "{label:<28} cases {:>4}  episodes {:>5}  opcodes {:>9} -> {:>9}  ({:>5.1}%)  bytes {:>7} -> {:>7}",
        t.cases,
        t.episodes,
        t.opcodes_before,
        t.opcodes_after,
        t.reduction(),
        t.bytes_before,
        t.bytes_after,
    )
}

#[test]
#[ignore = "measurement report, run with --ignored --nocapture"]
fn opt_corpus_stats() {
    let root = tests_dir();
    let mut per_case: Vec<(String, Tally)> = Vec::new();
    let mut per_tier: BTreeMap<String, Tally> = BTreeMap::new();
    let mut skipped = Vec::new();
    for case_dir in collect_oracle_cases(&root) {
        if has_empty_source(&case_dir) || is_compile_error_case(&case_dir) {
            continue;
        }
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(&case_dir)
            .display()
            .to_string();
        match measure_case(&case_dir) {
            Ok(t) => {
                let tier = rel.split('/').next().unwrap_or("?").to_string();
                per_tier.entry(tier).or_default().add(&t);
                per_case.push((rel, t));
            }
            Err(e) => skipped.push(format!("{rel}: {e}")),
        }
    }

    let mut total = Tally::default();
    for t in per_tier.values() {
        total.add(t);
    }

    println!(
        "\n=== optimizer over the oracle corpus (resident passes, every oracle episode replayed) ==="
    );
    for (tier, t) in &per_tier {
        println!("{}", row(tier, t));
    }
    println!("{}", row("TOTAL", &total));
    println!(
        "fusions: {}",
        total
            .fusions
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("  ")
    );

    // Distribution of per-case reductions (episode-weighted opcodes).
    let mut buckets: BTreeMap<u32, usize> = BTreeMap::new();
    for (_, t) in &per_case {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b = (t.reduction() / 5.0).floor().max(0.0) as u32 * 5;
        *buckets.entry(b).or_default() += 1;
    }
    println!("\nper-case reduction distribution (5% buckets):");
    for (b, n) in &buckets {
        println!(
            "  {b:>3}%..{:>3}%  {n:>4} case(s)  {}",
            b + 5,
            "#".repeat(*n / 2)
        );
    }

    per_case.sort_by(|a, b| {
        b.1.reduction()
            .partial_cmp(&a.1.reduction())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("\nlargest reductions:");
    for (rel, t) in per_case.iter().take(12) {
        println!(
            "  {:>5.1}%  {:>8} -> {:>8}  {rel}",
            t.reduction(),
            t.opcodes_before,
            t.opcodes_after
        );
    }
    println!("\nsmallest reductions (cases with >= 200 opcodes):");
    for (rel, t) in per_case
        .iter()
        .rev()
        .filter(|(_, t)| t.opcodes_before >= 200)
        .take(12)
    {
        println!(
            "  {:>5.1}%  {:>8} -> {:>8}  {rel}",
            t.reduction(),
            t.opcodes_before,
            t.opcodes_after
        );
    }
    if !skipped.is_empty() {
        println!("\nskipped {} case(s):", skipped.len());
        for s in &skipped {
            println!("  {s}");
        }
    }
    assert!(total.cases > 0, "no cases measured");
}
