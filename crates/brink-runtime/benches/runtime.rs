use std::fmt;

use brink_converter::convert;
use brink_format::StoryData;
use brink_json::InkJson;
use brink_runtime::{DotNetRng, Program, Stats, StepResult, Story};

// ── Scenarios ────────────────────────────────────────────────────────────────

struct Scenario {
    name: &'static str,
    json: &'static str,
    inputs: Vec<usize>,
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const MINIMAL_JSON: &str =
    include_str!("../../../tests/tier1/basics/I001-minimal-story/story.ink.json");

const HANOI_3_JSON: &str = include_str!("../../../tests/tier3/lists/tower-of-hanoi/story.ink.json");
const HANOI_3_INPUT: &str = include_str!("../../../tests/tier3/lists/tower-of-hanoi/input.txt");

const HANOI_10_JSON: &str = include_str!("../../../benchmarks/stories/hanoi-10/story.ink.json");
const HANOI_10_INPUT: &str = include_str!("../../../benchmarks/stories/hanoi-10/input.txt");

const CRUCIBLE_8_JSON: &str = include_str!("../../../benchmarks/stories/crucible-8/story.ink.json");
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
                    json: MINIMAL_JSON,
                    inputs: vec![],
                },
                Scenario {
                    name: "hanoi-3",
                    json: HANOI_3_JSON,
                    inputs: parse_inputs(HANOI_3_INPUT),
                },
                Scenario {
                    name: "hanoi-10",
                    json: HANOI_10_JSON,
                    inputs: parse_inputs(HANOI_10_INPUT),
                },
                Scenario {
                    name: "crucible-8",
                    json: CRUCIBLE_8_JSON,
                    inputs: parse_inputs(CRUCIBLE_8_INPUT),
                },
            ]
        })
        .as_slice()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[expect(clippy::unwrap_used)]
fn parse_and_convert(json: &str) -> StoryData {
    let ink: InkJson = serde_json::from_str(json).unwrap();
    convert(&ink).unwrap()
}

#[expect(clippy::unwrap_used)]
fn run_to_completion(program: &Program, inputs: &[usize]) -> Stats {
    let mut story = Story::<DotNetRng>::new(program);
    let mut input_idx = 0;

    loop {
        match story.continue_maximally().unwrap() {
            StepResult::Done { .. } | StepResult::Ended { .. } => break,
            StepResult::Choices { choices, .. } => {
                if input_idx >= inputs.len() {
                    break;
                }
                let idx = inputs[input_idx];
                input_idx += 1;
                assert!(idx < choices.len());
                story.choose(idx).unwrap();
            }
        }
    }

    story.stats().clone()
}

// ── Benchmark groups ─────────────────────────────────────────────────────────

mod converter_bench {
    use super::{InkJson, Scenario, convert, scenarios};

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn convert_json(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| {
            let ink: InkJson = serde_json::from_str(scenario.json).unwrap();
            convert(&ink).unwrap()
        });
    }
}

mod linker_bench {
    use super::{Scenario, parse_and_convert, scenarios};

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn link(bencher: divan::Bencher, scenario: &Scenario) {
        let data = parse_and_convert(scenario.json);
        bencher.bench_local(|| brink_runtime::link(&data).unwrap());
    }
}

mod runtime_step {
    use super::{Scenario, parse_and_convert, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn run(bencher: divan::Bencher, scenario: &Scenario) {
        let data = parse_and_convert(scenario.json);
        #[expect(clippy::unwrap_used)]
        let program = brink_runtime::link(&data).unwrap();
        bencher.bench_local(|| run_to_completion(&program, &scenario.inputs));
    }
}

mod end_to_end {
    use super::{Scenario, parse_and_convert, run_to_completion, scenarios};

    #[divan::bench(args = scenarios())]
    fn full_pipeline(bencher: divan::Bencher, scenario: &Scenario) {
        bencher.bench_local(|| {
            let data = parse_and_convert(scenario.json);
            #[expect(clippy::unwrap_used)]
            let program = brink_runtime::link(&data).unwrap();
            run_to_completion(&program, &scenario.inputs);
        });
    }

    #[divan::bench(args = scenarios())]
    #[expect(clippy::unwrap_used)]
    fn preconverted(bencher: divan::Bencher, scenario: &Scenario) {
        let data = parse_and_convert(scenario.json);
        bencher.bench_local(|| {
            let program = brink_runtime::link(&data).unwrap();
            run_to_completion(&program, &scenario.inputs);
        });
    }
}

#[expect(clippy::unwrap_used, clippy::print_stderr)]
fn print_hanoi_10_stats() {
    let data = parse_and_convert(HANOI_10_JSON);
    let program = brink_runtime::link(&data).unwrap();
    let inputs = parse_inputs(HANOI_10_INPUT);
    let stats = run_to_completion(&program, &inputs);

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
    let data = parse_and_convert(CRUCIBLE_8_JSON);
    let program = brink_runtime::link(&data).unwrap();
    let inputs = parse_inputs(CRUCIBLE_8_INPUT);
    let stats = run_to_completion(&program, &inputs);

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
