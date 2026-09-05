// brink internal-loop benchmark driver.
//
// Usage: brink-loop <story.ink|story.inkb> <input.txt> [--iterations N]
//
// Runs the story N times in a single process, reporting total and average time
// plus the VM's own counters (`brink_runtime::Stats`).
// Input file is 0-indexed (one choice index per line).
// Stops when input is exhausted.
//
// # Why the counters matter more than the timing
//
// For a deterministic VM, executed-opcode count is an EXACT metric: the same
// story and the same inputs dispatch the same opcodes on every machine, every
// run. Wall clock is noisy and machine-dependent. So when comparing an
// artifact against an optimized copy, `opcodes` is the primary number and
// timing is the confirmation — and the two answer different questions:
//
//   - `opcodes`     — did the transform remove executed instructions at all?
//                     The right metric for anything that removes CHEAP ops
//                     (jump threading), where a timing delta would be noise.
//   - `snapshot_cache_misses`, `materializations`, `frames_pushed`
//                   — did it remove EXPENSIVE work? `EnterContainer` nulls
//                     the call-stack snapshot cache and can force a
//                     `materialize()`; container splicing is aimed at exactly
//                     these, and they move where `opcodes` alone understates.
//   - elapsed       — did any of it reach the clock?
//
// The run is also its own determinism check: every iteration must produce
// identical counters, and a mismatch is reported rather than averaged away.

use std::time::Instant;

use brink_runtime::{DotNetRng, Step, Story};

/// Play the story once, returning the run's counters **and the line tables**.
///
/// Handing the tables back matters: they are a `Vec<Vec<LineEntry>>` that
/// `Story::new` takes by value, so a naive `line_tables.clone()` per
/// iteration deep-clones every `LineContent`, `audio_ref` and `slot_info` in
/// the story. On TheIntercept that measured ~1,007 allocations per
/// iteration — about a quarter of all allocations in a profile of this tool
/// (#3565), i.e. the instrument built to measure allocation pressure was
/// itself a leading source of it. `Story::into_snapshot` gives the tables
/// back on the way out, so they are *moved* through every iteration and
/// cloned exactly zero times.
fn run_once(
    program: std::sync::Arc<brink_runtime::Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) -> (brink_runtime::Stats, Vec<Vec<brink_format::LineEntry>>) {
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
            // landed, see brink_runtime::Step docs) but is matched here so
            // this tool keeps compiling once it becomes reachable.
            Some(Step::Line(_) | Step::Done | Step::End | Step::Suspended) | None => break,
            Some(Step::Choices(choices)) => {
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

    let stats = story.stats().clone();
    let (_, line_tables) = story.into_snapshot();
    (stats, line_tables)
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
    let (program, mut line_tables) =
        brink_runtime::link(&data).unwrap_or_else(|e| panic!("failed to link: {e}"));
    let program = std::sync::Arc::new(program);

    let start = Instant::now();
    let mut first: Option<brink_runtime::Stats> = None;
    let mut drift = 0usize;
    for _ in 0..iterations {
        let (stats, returned) = run_once(program.clone(), line_tables, &inputs);
        line_tables = returned;
        match &first {
            None => first = Some(stats),
            // Identical inputs must dispatch identical opcodes. Anything else
            // is nondeterminism, and averaging it away would hide it.
            Some(f) if f.opcodes != stats.opcodes || f.steps != stats.steps => drift += 1,
            Some(_) => {}
        }
    }
    let elapsed = start.elapsed();
    let s = first.unwrap_or_default();

    eprintln!(
        "brink-loop: {} iterations in {:.3}s ({:.3}ms avg)",
        iterations,
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / iterations as f64
    );
    // One line per counter, `key=value`, so two runs diff cleanly.
    eprintln!(
        "brink-loop-counters: opcodes={} steps={} frames_pushed={} frames_popped={} \
threads_created={} threads_completed={} choices_presented={} choices_selected={} \
snapshot_cache_hits={} snapshot_cache_misses={} materializations={}",
        s.opcodes,
        s.steps,
        s.frames_pushed,
        s.frames_popped,
        s.threads_created,
        s.threads_completed,
        s.choices_presented,
        s.choices_selected,
        s.snapshot_cache_hits,
        s.snapshot_cache_misses,
        s.materializations,
    );
    #[cfg(feature = "histogram")]
    if args.iter().any(|a| a == "--histogram") {
        print_histogram(&data, &s);
    }

    assert!(
        drift == 0,
        "brink-loop: {drift} of {iterations} iterations dispatched a different \
         opcode/step count from the first — the VM is nondeterministic on this \
         story, and every counter above is meaningless until that is fixed"
    );
}

/// Print the executed-opcode and opcode-pair histograms, naming each
/// discriminant by decoding the artifact's symbolic bytecode once.
#[cfg(feature = "histogram")]
fn print_histogram(data: &brink_format::StoryData, stats: &brink_runtime::Stats) {
    use brink_format::Opcode;
    use std::collections::BTreeMap;

    let mut names: BTreeMap<u8, String> = BTreeMap::new();
    for container in &data.containers {
        let code = &container.bytecode;
        let mut off = 0;
        while off < code.len() {
            let disc = code[off];
            match Opcode::decode(code, &mut off) {
                Ok(op) => {
                    // The variant name alone: `Debug` prints `Goto(...)` /
                    // `MakeClosure { .. }` / `Nop`.
                    let dbg = format!("{op:?}");
                    let variant = dbg.split(['(', '{', ' ']).next().unwrap_or("?").to_owned();
                    names.entry(disc).or_insert(variant);
                }
                Err(_) => break,
            }
        }
    }
    let name = |d: u8| names.get(&d).map_or("?", String::as_str);

    let total: u64 = stats.opcode_hist.iter().sum();
    let mut ops: Vec<(u64, u8)> = stats
        .opcode_hist
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(d, &c)| (c, d as u8))
        .collect();
    ops.sort_by(|a, b| b.cmp(a));
    eprintln!(
        "brink-loop-histogram: {total} opcodes, {} distinct",
        ops.len()
    );
    let mut cum = 0u64;
    for (c, d) in ops.iter().take(30) {
        cum += c;
        eprintln!(
            "  {:>9}  {:>5.1}%  cum {:>5.1}%  {}",
            c,
            *c as f64 * 100.0 / total as f64,
            cum as f64 * 100.0 / total as f64,
            name(*d)
        );
    }
    let mut pairs: Vec<(u64, u8, u8)> = stats
        .bigram_hist
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(i, &c)| (c, (i >> 8) as u8, (i & 0xff) as u8))
        .collect();
    pairs.sort_by(|a, b| b.cmp(a));
    eprintln!("brink-loop-histogram: top pairs");
    for (c, a, b) in pairs.iter().take(30) {
        eprintln!(
            "  {:>9}  {:>5.1}%  {} -> {}",
            c,
            *c as f64 * 100.0 / total as f64,
            name(*a),
            name(*b)
        );
    }
}
