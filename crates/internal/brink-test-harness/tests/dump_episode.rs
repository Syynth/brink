//! Dump recorded episodes to see what the harness captures.
#![expect(clippy::unwrap_used, clippy::print_stderr)]

use brink_test_harness::{ExploreConfig, RunConfig, explore, record};

fn load(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// Compile a `.ink` fixture, link, and record an episode with the given inputs.
fn record_from_ink(ink_path: &str, inputs: &[usize]) -> brink_test_harness::Episode {
    let data = brink_compiler::compile_path(std::path::Path::new(ink_path))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let config = RunConfig {
        inputs: inputs.to_vec(),
        max_steps: 10_000,
    };
    record(&std::sync::Arc::new(program), line_tables, &config)
}

fn print_episode(ep: &brink_test_harness::Episode, label: &str) {
    eprintln!("══════════════════════════════════════════════════");
    eprintln!("Episode: {label}");
    eprintln!("  choice_path: {:?}", ep.choice_path);
    eprintln!("  outcome: {:?}", ep.outcome);
    eprintln!(
        "  initial globals: {} values",
        ep.initial_state.globals.len()
    );
    eprintln!("  steps: {}", ep.steps.len());

    for (i, step) in ep.steps.iter().enumerate() {
        eprintln!("  ── step {i} ──");
        let text_preview = if step.text.len() > 120 {
            format!("{}…", &step.text[..120])
        } else {
            step.text.clone()
        };
        eprintln!("    text: {text_preview:?}");

        let non_empty_tags: Vec<_> = step.tags.iter().filter(|t| !t.is_empty()).collect();
        if !non_empty_tags.is_empty() {
            eprintln!("    tags: {non_empty_tags:?}");
        }

        eprintln!("    outcome: {:?}", step.outcome);
        eprintln!("    writes: {} mutations", step.writes.len());

        // Show first few writes
        for (j, w) in step.writes.iter().take(8).enumerate() {
            eprintln!("      [{j}] {w:?}");
        }
        if step.writes.len() > 8 {
            eprintln!("      ... and {} more", step.writes.len() - 8);
        }
    }
    eprintln!();
}

#[test]
fn dump_minimal() {
    let ep = record_from_ink(
        "../../../tests/tier1/basics/I001-minimal-story/story.ink",
        &[],
    );
    print_episode(&ep, "I001 — minimal story (no choices)");

    // Also dump as JSON to verify serde roundtrip
    let serialized = serde_json::to_string_pretty(&ep).unwrap();
    eprintln!("── JSON ──");
    eprintln!("{serialized}");

    // Roundtrip
    let deserialized: brink_test_harness::Episode = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.choice_path, ep.choice_path);
    assert_eq!(deserialized.steps.len(), ep.steps.len());
    assert_eq!(deserialized.steps[0].text, ep.steps[0].text);
}

#[test]
fn dump_once_only_json() {
    let ep = record_from_ink(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink",
        &[0, 0],
    );
    let serialized = serde_json::to_string_pretty(&ep).unwrap();
    eprintln!("── I079 JSON ──");
    eprintln!("{serialized}");

    let deserialized: brink_test_harness::Episode = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.choice_path, ep.choice_path);
    assert_eq!(deserialized.steps.len(), ep.steps.len());
}

#[test]
fn dump_once_only_choices() {
    // Choose first, then first again → fallback fires → end
    let ep = record_from_ink(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink",
        &[0, 0],
    );
    print_episode(&ep, "I079 — once-only choices [0, 0]");
}

#[test]
fn dump_tower_of_hanoi_3() {
    let input_str = load("../../../tests/tier3/lists/tower-of-hanoi/input.txt");
    let inputs: Vec<usize> = input_str
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let ep = record_from_ink(
        "../../../tests/tier3/lists/tower-of-hanoi/story.ink",
        &inputs,
    );
    print_episode(&ep, "Tower of Hanoi (3 discs)");
}

/// `runner::record`'s `Line::Done` arm now probes `classify_done`, matching
/// `explorer::explore` (see `termination.rs`). Pins the one observable
/// behavior change: a `Done` reached without an explicit `-> DONE` now
/// surfaces as `Outcome::Error("...ran out of content...")` instead of
/// `Outcome::Done`, for `record` as well as `explore`.
#[test]
fn record_divert_choice_flags_missing_terminal_as_error() {
    let ep = record_from_ink(
        "../../../tests/tier1/choices/divert-choice/story.ink",
        &[0, 0],
    );
    assert!(
        matches!(
            &ep.outcome,
            brink_test_harness::Outcome::Error(msg) if msg.contains("ran out of content")
        ),
        "expected Outcome::Error containing 'ran out of content', got {:?}",
        ep.outcome
    );
}

/// Counterpart to the above: a fixture that reaches an explicit `-> END`
/// is unaffected by the `classify_done` probe (its terminal `Line` is
/// `Line::End`, never `Line::Done`).
#[test]
fn record_once_only_choices_reaches_explicit_end() {
    let ep = record_from_ink(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink",
        &[0, 0],
    );
    assert_eq!(ep.outcome, brink_test_harness::Outcome::Ended);
}

/// `record` and `explore` must agree on outcome for the same choice path —
/// exactly the divergence this PR removes (`runner.rs` previously never
/// probed `classify_done` on a `Done` terminal; `explorer.rs` always did).
#[test]
fn record_and_explore_agree_on_divert_choice_outcome() {
    let recorded = record_from_ink(
        "../../../tests/tier1/choices/divert-choice/story.ink",
        &[0, 0],
    );

    let data = brink_compiler::compile_path(std::path::Path::new(
        "../../../tests/tier1/choices/divert-choice/story.ink",
    ))
    .unwrap()
    .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let config = ExploreConfig {
        max_depth: 5,
        max_episodes: 50,
    };
    let episodes = explore(std::sync::Arc::new(program), line_tables, &config);
    let explored = episodes
        .iter()
        .find(|ep| ep.choice_path == recorded.choice_path)
        .expect("explore should produce an episode for choice path [0, 0]");

    assert_eq!(explored.outcome, recorded.outcome);
}

#[test]
fn dump_explore_once_only() {
    let data = brink_compiler::compile_path(std::path::Path::new(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink",
    ))
    .unwrap()
    .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();

    let config = ExploreConfig {
        max_depth: 5,
        max_episodes: 20,
    };
    let episodes = explore(std::sync::Arc::new(program), line_tables, &config);

    eprintln!("\n══════════════════════════════════════════════════");
    eprintln!("Explore I079: {} episodes found", episodes.len());
    for (i, ep) in episodes.iter().enumerate() {
        print_episode(ep, &format!("branch {i}"));
    }
}
