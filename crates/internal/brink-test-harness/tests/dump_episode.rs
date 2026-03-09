//! Dump recorded episodes to see what the harness captures.
#![expect(clippy::unwrap_used, clippy::print_stderr)]

use brink_test_harness::{ExploreConfig, explore, record_from_ink_json};

fn load(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
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
    let json = load("../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let ep = record_from_ink_json(&json, &[]);
    print_episode(&ep, "I001 — minimal story (no choices)");
}

#[test]
fn dump_once_only_choices() {
    let json = load(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink.json",
    );
    // Choose first, then first again → fallback fires → end
    let ep = record_from_ink_json(&json, &[0, 0]);
    print_episode(&ep, "I079 — once-only choices [0, 0]");
}

#[test]
fn dump_tower_of_hanoi_3() {
    let json = load("../../../tests/tier3/lists/tower-of-hanoi/story.ink.json");
    let input_str = load("../../../tests/tier3/lists/tower-of-hanoi/input.txt");
    let inputs: Vec<usize> = input_str
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let ep = record_from_ink_json(&json, &inputs);
    print_episode(&ep, "Tower of Hanoi (3 discs)");
}

#[test]
fn dump_explore_once_only() {
    let json = load(
        "../../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink.json",
    );
    let ink: brink_json::InkJson = serde_json::from_str(&json).unwrap();
    let data = brink_converter::convert(&ink).unwrap();
    let program = brink_runtime::link(&data).unwrap();

    let config = ExploreConfig {
        max_depth: 5,
        max_episodes: 20,
    };
    let episodes = explore(&program, &config);

    eprintln!("\n══════════════════════════════════════════════════");
    eprintln!("Explore I079: {} episodes found", episodes.len());
    for (i, ep) in episodes.iter().enumerate() {
        print_episode(ep, &format!("branch {i}"));
    }
}
