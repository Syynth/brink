// brink internal-loop benchmark driver.
//
// Usage: brink-loop <story.ink.json> <input.txt> [--iterations N]
//
// Runs the story N times in a single process, reporting total and average time.
// Input file is 0-indexed (one choice index per line).
// Stops when input is exhausted.

use std::time::Instant;

use brink_converter::convert;
use brink_json::InkJson;
use brink_runtime::{DotNetRng, Line, Story};

fn run_once(
    program: &brink_runtime::Program,
    inputs: &[usize],
) {
    let mut story = Story::<DotNetRng>::new(program);
    let mut input_idx = 0;

    loop {
        let lines = match story.continue_maximally() {
            Ok(l) => l,
            Err(e) => panic!("runtime error: {e}"),
        };
        let last = lines.last();
        match last {
            Some(Line::Text { .. }) | Some(Line::End { .. }) | None => break,
            Some(Line::Choices { choices, .. }) => {
                if input_idx >= inputs.len() {
                    break;
                }
                let idx = inputs[input_idx];
                input_idx += 1;
                assert!(idx < choices.len());
                story.choose(idx).unwrap_or_else(|e| panic!("choose failed: {e}"));
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: brink-loop <story.ink.json> <input.txt> [--iterations N]");
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

    let json_str = std::fs::read_to_string(story_path)
        .unwrap_or_else(|e| panic!("failed to read {story_path}: {e}"));
    let input_str = std::fs::read_to_string(input_path)
        .unwrap_or_else(|e| panic!("failed to read {input_path}: {e}"));

    let inputs: Vec<usize> = input_str
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().parse().unwrap_or_else(|e| panic!("bad input line {l:?}: {e}")))
        .collect();

    let ink: InkJson = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse JSON: {e}"));
    let data = convert(&ink)
        .unwrap_or_else(|e| panic!("failed to convert: {e}"));
    let program = brink_runtime::link(&data)
        .unwrap_or_else(|e| panic!("failed to link: {e}"));

    let start = Instant::now();
    for _ in 0..iterations {
        run_once(&program, &inputs);
    }
    let elapsed = start.elapsed();

    eprintln!(
        "brink-loop: {} iterations in {:.3}s ({:.3}ms avg)",
        iterations,
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / iterations as f64
    );
}
