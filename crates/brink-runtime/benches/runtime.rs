use std::fmt;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::StoryData;
use brink_runtime::{DotNetRng, Line, Program, Stats, Story};

// ── Scenarios ────────────────────────────────────────────────────────────────

struct Scenario {
    name: &'static str,
    /// `.ink` entry point, relative to this crate's manifest dir.
    ink: &'static str,
    inputs: Vec<usize>,
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const MINIMAL_INK: &str = "../../tests/tier1/basics/I001-minimal-story/story.ink";

const HANOI_3_INK: &str = "../../tests/tier3/lists/tower-of-hanoi/story.ink";
const HANOI_3_INPUT: &str = include_str!("../../../tests/tier3/lists/tower-of-hanoi/input.txt");

const HANOI_10_INK: &str = "../../benchmarks/stories/hanoi-10/story.ink";
const HANOI_10_INPUT: &str = include_str!("../../../benchmarks/stories/hanoi-10/input.txt");

const CRUCIBLE_8_INK: &str = "../../benchmarks/stories/crucible-8/story.ink";
const CRUCIBLE_8_INPUT: &str = include_str!("../../../benchmarks/stories/crucible-8/input.txt");

/// Loop-append (issue #576, `docs/value-model-spec.md` §5's "one cliff")
/// benchmark: 10k sequential `push`es onto a freshly-created array in one
/// `~ { … }` block — brink-dialect only (no strict-ink/oracle equivalent;
/// see the `.ink` file's header comment for the before/after cliff this
/// isolates). Not part of `scenarios()`/`Scenario` (those all compile under
/// the default strict-ink dialect via `compile_story`) — `loop_append_bench`
/// below is a standalone `#[divan::bench]` using `compile_story_brink`.
const LOOP_APPEND_10K_INK: &str = "../../benchmarks/stories/loop-append-10k/story.ink";

#[expect(clippy::unwrap_used)]
fn parse_inputs(s: &str) -> Vec<usize> {
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.trim().parse().unwrap())
        .collect()
}

fn scenarios() -> &'static [Scenario] {
    static SCENARIOS: std::sync::OnceLock<Vec<Scenario>> = std::sync::OnceLock::new();
    SCENARIOS
        .get_or_init(|| {
            vec![
                Scenario {
                    name: "minimal",
                    ink: MINIMAL_INK,
                    inputs: vec![],
                },
                Scenario {
                    name: "hanoi-3",
                    ink: HANOI_3_INK,
                    inputs: parse_inputs(HANOI_3_INPUT),
                },
                Scenario {
                    name: "hanoi-10",
                    ink: HANOI_10_INK,
                    inputs: parse_inputs(HANOI_10_INPUT),
                },
                Scenario {
                    name: "crucible-8",
                    ink: CRUCIBLE_8_INK,
                    inputs: parse_inputs(CRUCIBLE_8_INPUT),
                },
            ]
        })
        .as_slice()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[expect(clippy::unwrap_used)]
fn compile_story(ink_rel: &str) -> StoryData {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ink_rel);
    brink_compiler::compile_path(&path).unwrap().data
}

/// Like [`compile_story`] but under the brink dialect (`push`/`~ { … }`
/// blocks are T1b extensions, invisible to the default strict-ink
/// compile) — used only by [`LOOP_APPEND_10K_INK`].
#[expect(clippy::unwrap_used)]
fn compile_story_brink(ink_rel: &str) -> StoryData {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ink_rel);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_path_with_options(&path, options)
        .unwrap()
        .data
}

#[expect(clippy::unwrap_used)]
fn run_to_completion(
    program: &Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) -> Stats {
    let mut story = Story::<DotNetRng>::new(Arc::clone(program), line_tables);
    let mut input_idx = 0;

    loop {
        let mut done = false;
        for line in story.continue_maximally().unwrap() {
            match line {
                Line::Text { .. } => {}
                Line::Done { .. } | Line::End { .. } => {
                    done = true;
                }
                Line::Choices { choices, .. } => {
                    if input_idx >= inputs.len() {
                        done = true;
                        break;
                    }
                    let idx = inputs[input_idx];
                    input_idx += 1;
                    assert!(idx < choices.len());
                    story.choose(idx).unwrap();
                }
            }
        }
        if done {
            break;
        }
    }

    story.stats().clone()
}

// ── Benchmark groups ─────────────────────────────────────────────────────────

mod compiler_bench {
    use super::{Scenario, compile_story, scenarios};

    #[divan::bench(args = scenarios())]
    fn compile(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| compile_story(scenario.ink));
    }
}

mod linker_bench {
    use super::{Scenario, compile_story, scenarios};

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn link(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        bencher.bench_local(|| brink_runtime::link(&data).unwrap());
    }
}

mod runtime_step {
    use super::{Scenario, compile_story, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn run(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &scenario.inputs));
    }
}

/// Loop-append (issue #576) benchmark: isolates the RMW-mutate cost alone
/// (link once, run repeatedly), matching `runtime_step`'s granularity —
/// the compile step is brink-dialect-specific setup, not part of what this
/// benchmark measures. Before #576, this scenario is O(n^2) in the push
/// count (10k re-COWs of an up-to-10k-element array); after #576, O(n)
/// amortized. See the PR description for measured before/after numbers
/// (`docs/value-model-spec.md` §5 predicts, this benchmark verifies).
mod loop_append_bench {
    use super::{LOOP_APPEND_10K_INK, compile_story_brink, run_to_completion};

    #[divan::bench]
    fn push_10k(bencher: divan::Bencher) {
        let data = compile_story_brink(LOOP_APPEND_10K_INK);
        #[expect(clippy::unwrap_used)]
        let (program, line_tables) = brink_runtime::link(&data).unwrap();
        let program = std::sync::Arc::new(program);
        bencher.bench_local(|| run_to_completion(&program, line_tables.clone(), &[]));
    }
}

mod end_to_end {
    use super::{Scenario, compile_story, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn full_pipeline(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| {
            let data = compile_story(scenario.ink);
            #[expect(clippy::unwrap_used)]
            let (program, line_tables) = brink_runtime::link(&data).unwrap();
            let program = std::sync::Arc::new(program);
            run_to_completion(&program, line_tables, &scenario.inputs);
        });
    }

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn precompiled(bencher: divan::Bencher, scenario: &Scenario) {
        let data = compile_story(scenario.ink);
        bencher.bench_local(|| {
            let (program, line_tables) = brink_runtime::link(&data).unwrap();
            let program = std::sync::Arc::new(program);
            run_to_completion(&program, line_tables, &scenario.inputs);
        });
    }
}

#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_hanoi_10_stats() {
    let data = compile_story(HANOI_10_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    let inputs = parse_inputs(HANOI_10_INPUT);
    let stats = run_to_completion(&program, line_tables, &inputs);

    eprintln!("\n── hanoi-10 VM stats ──────────────────────────");
    eprintln!("  opcodes:              {:>10}", stats.opcodes);
    eprintln!("  steps:                {:>10}", stats.steps);
    eprintln!("  threads_created:      {:>10}", stats.threads_created);
    eprintln!("  threads_completed:    {:>10}", stats.threads_completed);
    eprintln!("  frames_pushed:        {:>10}", stats.frames_pushed);
    eprintln!("  frames_popped:        {:>10}", stats.frames_popped);
    eprintln!("  choices_presented:    {:>10}", stats.choices_presented);
    eprintln!("  choices_selected:     {:>10}", stats.choices_selected);
    eprintln!("  snapshot_cache_hits:  {:>10}", stats.snapshot_cache_hits);
    eprintln!(
        "  snapshot_cache_misses:{:>10}",
        stats.snapshot_cache_misses
    );
    eprintln!("  materializations:     {:>10}", stats.materializations);
    eprintln!("───────────────────────────────────────────────\n");
}

#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_crucible_8_stats() {
    let data = compile_story(CRUCIBLE_8_INK);
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let program = std::sync::Arc::new(program);
    let inputs = parse_inputs(CRUCIBLE_8_INPUT);
    let stats = run_to_completion(&program, line_tables, &inputs);

    eprintln!("\n── crucible-8 VM stats ────────────────────────");
    eprintln!("  opcodes:              {:>10}", stats.opcodes);
    eprintln!("  steps:                {:>10}", stats.steps);
    eprintln!("  threads_created:      {:>10}", stats.threads_created);
    eprintln!("  threads_completed:    {:>10}", stats.threads_completed);
    eprintln!("  frames_pushed:        {:>10}", stats.frames_pushed);
    eprintln!("  frames_popped:        {:>10}", stats.frames_popped);
    eprintln!("  choices_presented:    {:>10}", stats.choices_presented);
    eprintln!("  choices_selected:     {:>10}", stats.choices_selected);
    eprintln!("  snapshot_cache_hits:  {:>10}", stats.snapshot_cache_hits);
    eprintln!(
        "  snapshot_cache_misses:{:>10}",
        stats.snapshot_cache_misses
    );
    eprintln!("  materializations:     {:>10}", stats.materializations);
    eprintln!("───────────────────────────────────────────────\n");
}

fn main() {
    // Force scenario initialization before benchmarks run.
    let _ = scenarios();
    print_hanoi_10_stats();
    print_crucible_8_stats();
    divan::main();
}
