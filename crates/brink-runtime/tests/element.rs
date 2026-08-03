//! Regression tests for [`OutputLine::element`] (`Element`, issue #1683) —
//! the per-line classification field the `Step`/`OutputLine` redesign
//! (#1684) reserved but never populated.
//!
//! Scoped narrowly and honestly (see `Element`'s own doc): today every line
//! reports the degenerate [`Element::narrative`] case, regardless of
//! source. These tests pin that the field exists, is always the narrative
//! default, and survives both a plain line and a line inside a choice-driven
//! run — the two shapes `brink-web`'s marshal layer and any other host
//! consumes today.

use brink_runtime::{Element, FastRng, Step, Story};

/// Compile ink source and link it into a runnable story.
#[expect(clippy::unwrap_used)]
fn story_from_source(src: &str) -> Story<FastRng> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Every `Step::Line` carries the degenerate narrative element — no
/// `@[element]` dispatch exists in this fixture, so there is nothing to
/// classify beyond the always-correct default.
#[test]
fn plain_lines_carry_the_narrative_default() {
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
    for line in &lines {
        assert_eq!(line.element, Element::narrative(), "{steps:?}");
        assert_eq!(line.element.kind, "narrative");
        assert!(line.element.data.is_empty());
    }
}

/// The narrative default survives a choice-driven branch too — `element`
/// is stamped independently of `block_id`, so a fresh run after a choice
/// still reports the same degenerate classification.
#[test]
fn lines_after_a_choice_still_carry_the_narrative_default() {
    let mut story = story_from_source(
        "-> start\n\
         === start ===\n\
         Before.\n\
         * [left] Went left.\n-> END\n\
         * [right] Went right.\n-> END\n",
    );
    let _ = story.continue_maximally().expect("drive to choices");
    story.choose(0).expect("choose left");
    let after_steps = story.continue_maximally().expect("drive to END");

    let after_lines: Vec<_> = after_steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert!(!after_lines.is_empty(), "{after_steps:?}");
    for line in &after_lines {
        assert_eq!(line.element, Element::narrative(), "{after_steps:?}");
    }
}
