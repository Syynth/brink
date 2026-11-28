//! Regression tests for [`OutputLine::block_id`] (`BlockId`, §3.7/§8d.2) —
//! the run of adjacent content a `Step::Line` belongs to.
//!
//! Before this file, `block_id` had zero test coverage anywhere in the
//! diff that introduced it: every occurrence was production code, docs, or
//! the changeset (rule 20a). Deleting all three `next_block_id += 1` call
//! sites (`FlowInstance::advance_with_limit`'s Done-resume arm,
//! `choose_path_string_with_args`, and `select_choice`) would leave the
//! suite green. These tests pin the two load-bearing properties: adjacent
//! lines produced by one uninterrupted run share an id, and each of the
//! three "fresh run begins here" boundaries bumps it.

use brink_runtime::{FastRng, Step, Story};

/// Compile ink source and link it into a runnable story.
#[expect(clippy::unwrap_used)]
fn story_from_source(src: &str) -> Story<FastRng> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Adjacent lines produced by one uninterrupted run (no choice, no jump,
/// no Done in between) share the same `block_id`.
#[test]
fn adjacent_lines_in_one_run_share_a_block_id() {
    let mut story = story_from_source("One.\nTwo.\n-> END\n");
    let steps = story.continue_maximally().expect("drive to END");

    let lines: Vec<_> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "{steps:?}");
    assert_eq!(lines[0].text, "One.\n");
    assert_eq!(lines[1].text, "Two.\n");
    assert_eq!(
        lines[0].block_id, lines[1].block_id,
        "lines from one uninterrupted run must share a block_id: {steps:?}"
    );
}

/// The block id changes after a choice selection — the chosen branch is a
/// fresh run.
#[test]
fn block_id_changes_after_a_choice() {
    let mut story = story_from_source(
        "-> start\n\
         === start ===\n\
         Before.\n\
         * [left] Went left.\n-> END\n\
         * [right] Went right.\n-> END\n",
    );

    let before_steps = story.continue_maximally().expect("drive to choices");
    let before_id = before_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line before the choices");
    assert!(
        matches!(before_steps.last(), Some(Step::Choices(_))),
        "{before_steps:?}"
    );

    story.choose(0).expect("choose left");
    let after_steps = story.continue_maximally().expect("drive from the choice");
    let after_id = after_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line after the choice");

    assert_ne!(
        before_id, after_id,
        "the chosen branch must start a fresh block_id: before={before_steps:?} after={after_steps:?}"
    );
}

/// The block id changes after resuming from `Step::Done` — the next turn
/// is a fresh run, even though the flow isn't over.
#[test]
fn block_id_changes_after_a_done_resume() {
    let mut story = story_from_source("One.\n-> DONE\nTwo.\n-> END\n");

    let first_steps = story.continue_maximally().expect("drive to Done");
    assert!(
        matches!(first_steps.last(), Some(Step::Done)),
        "{first_steps:?}"
    );
    let first_id = first_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line before Done");

    let second_steps = story.continue_maximally().expect("drive past Done");
    let second_id = second_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line after resuming from Done");

    assert_ne!(
        first_id, second_id,
        "resuming past Done must start a fresh block_id: first={first_steps:?} second={second_steps:?}"
    );
}

/// The block id changes after a host-directed `choose_path_string` jump —
/// the jump target is a fresh run.
#[test]
fn block_id_changes_after_choose_path_string() {
    let mut story = story_from_source(
        "Hello.\n-> END\n\
         === elsewhere ===\n\
         Elsewhere.\n-> DONE\n",
    );

    let before_steps = story.continue_maximally().expect("drive to END");
    let before_id = before_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line before the jump");

    story.choose_path_string("elsewhere").expect("jump");
    let after_steps = story.continue_maximally().expect("drive from the jump");
    let after_id = after_steps
        .iter()
        .find_map(|s| match s {
            Step::Line(line) => Some(line.block_id),
            _ => None,
        })
        .expect("a line after the jump");

    assert_ne!(
        before_id, after_id,
        "the jump target must start a fresh block_id: before={before_steps:?} after={after_steps:?}"
    );
}
