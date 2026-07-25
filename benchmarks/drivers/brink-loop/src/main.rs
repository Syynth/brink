// brink internal-loop benchmark driver.
//
// Usage: brink-loop <story.ink|story.inkb> <input.txt> [--iterations N]
//
// Runs the story N times in a single process, reporting total and average time.
// Input file is 0-indexed (one choice index per line).
// Stops when input is exhausted.

use std::time::Instant;

use brink_runtime::{DotNetRng, Line, Story};

fn run_once(
    program: std::sync::Arc<brink_runtime::Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) {
    let mut story = Story::<DotNetRng>::new(program, line_tables);
    let mut input_idx = 0;

    loop {
        let lines = match story.continue_maximally() {
            Ok(l) => l,
            Err(e) => panic!("runtime error: {e}"),
        };
        let last = lines.last();
        match last {
            // `Suspended` is runtime-unreachable today (FS-3r not yet
            // landed, see brink_runtime::Line docs) but is matched here so
            // this tool keeps compiling once it becomes reachable.
            Some(
                Line::Text { .. } | Line::Done { .. } | Line::End { .. } | Line::Suspended { .. },
            )
            | None => break,
            Some(Line::Choices { choices, .. }) => {
                if input_idx >= inputs.len() {
                    break;
                }
                let idx = inputs[input_idx];
                input_idx += 1;
                assert!(idx < choices.len());
                story
                    .choose(idx)
                    .unwrap_or_else(|e| panic!("choose failed: {e}"));
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: brink-loop <story.ink|story.inkb> <input.txt> [--iterations N]");
        std::process::exit(1);
    }

    let story_path = &args[1];
    let input_path = &args[2];
    let iterations: usize = args
        .iter()
        .position(|a| a == "--iterations")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let input_str = std::fs::read_to_string(input_path)
        .unwrap_or_else(|e| panic!("failed to read {input_path}: {e}"));

    let inputs: Vec<usize> = input_str
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.trim()
                .parse()
                .unwrap_or_else(|e| panic!("bad input line {l:?}: {e}"))
        })
        .collect();

    let data = if story_path.ends_with(".inkb") {
        let bytes = std::fs::read(story_path)
            .unwrap_or_else(|e| panic!("failed to read {story_path}: {e}"));
        brink_format::read_inkb(&bytes).unwrap_or_else(|e| panic!("failed to read inkb: {e}"))
    } else {
        brink_compiler::compile_path(std::path::Path::new(story_path))
            .unwrap_or_else(|e| panic!("failed to compile {story_path}: {e}"))
            .data
    };
    let (program, line_tables) =
        brink_runtime::link(&data).unwrap_or_else(|e| panic!("failed to link: {e}"));
    let program = std::sync::Arc::new(program);

    let start = Instant::now();
    for _ in 0..iterations {
        run_once(program.clone(), line_tables.clone(), &inputs);
    }
    let elapsed = start.elapsed();

    eprintln!(
        "brink-loop: {} iterations in {:.3}s ({:.3}ms avg)",
        iterations,
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / iterations as f64
    );
}
