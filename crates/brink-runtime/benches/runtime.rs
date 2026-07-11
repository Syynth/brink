use std::fmt;
use std::sync::Arc;

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
