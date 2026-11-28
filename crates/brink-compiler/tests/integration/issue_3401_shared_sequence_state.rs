//! Issue #3401: a stateful alternative cloned into a lifted branch that
//! could not claim as a variant line used to get its own visit counter per
//! branch ("sharing revoked", the #3275 corner), so the sequence drifted a
//! view behind ink. Every clone now counts on the ORIGINAL's container
//! (`Sequence::counter_id`, codegen `TouchVisit`) while keeping its own
//! whole-line renderings, so the line table is unchanged.
//! Each case replays the line across views and asserts the text the C#
//! reference produces; the last one pins that the fix kept the #3275
//! ruling's whole-line renderings in the line table.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it, always choosing the first
/// choice, until `choices` choices have been made or the story ends;
/// return the emitted text lines.
fn play(source: &str, choices: usize) -> Vec<String> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    let mut made = 0;
    for _ in 0..400 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.trim().to_owned()),
            Step::Choices(_) => {
                if made == choices {
                    return lines;
                }
                made += 1;
                story.choose(0).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not settle in 400 steps");
}

#[test]
fn sequence_leading_a_conditional_line_advances_once_per_view() {
    let src = "-> k\n=== k ===\n+ [again]\n    {a|b}{true:p}{c|d|e}\n    -> k\n";
    assert_eq!(play(src, 4), vec!["apc", "bpd", "bpe", "bpe"]);
}

#[test]
fn sequences_on_a_glued_line_advance_once_per_view() {
    let src = "-> k\n=== k ===\n+ [again]\n    {a|b}{c|d|e} <>\n    \n    -> k\n";
    assert_eq!(play(src, 4), vec!["ac", "bd", "be", "be"]);
}

#[test]
fn mixed_claiming_branches_share_one_counter_stub_first() {
    // Mirror of the case below with the glue on the ELSE branch: the
    // claimable then-branch lowers first and stubs the stamped id; the
    // unclaimable else-branch lifts second as a clone counting on it.
    let src = "VAR n = 0\n-> k\n=== k ===\n+ [again]\n    ~ n = n + 1\n    {n mod 2 == 1:x|y<>}{c|d|e}\n    -> k\n";
    assert_eq!(play(src, 4), vec!["xc", "yd", "xe", "ye"]);
}

#[test]
fn mixed_claiming_branches_share_one_counter() {
    // The then-branch carries glue (unclaimable → bodied inline wrapper),
    // the else-branch is a plain line (claimable → variant stub); both
    // must touch ONE `{c|d|e}` count, alternating by `n`.
    let src = "VAR n = 0\n-> k\n=== k ===\n+ [again]\n    ~ n = n + 1\n    {n mod 2 == 1:x<>|y}{c|d|e}\n    -> k\n";
    assert_eq!(play(src, 4), vec!["xc", "yd", "xe", "ye"]);
}

/// Every whole-line rendering a lifted line produces, as plain line-table
/// text across all scopes.
fn plain_lines(source: &str) -> Vec<String> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned())).expect("compile");
    output
        .data
        .line_tables
        .iter()
        .flat_map(|t| t.lines.iter())
        .filter_map(|l| match &l.content {
            brink_format::LineContent::Plain(text) => Some(text.trim().to_owned()),
            brink_format::LineContent::Template(_) => None,
        })
        .collect()
}

#[test]
fn cloned_lines_keep_whole_line_renderings_in_the_line_table() {
    // #3275 ruling (1), reaffirmed for #3401: a cloned line's renderings
    // stay whole lines (translation units, VO slots) — the fix shares the
    // COUNTER, never the body, so no `a` / `c` fragments appear.
    let lines = plain_lines("-> k\n=== k ===\n+ [again]\n    {a|b}{c|d|e} <>\n    \n    -> k\n");
    for expected in ["ac", "ad", "ae", "bc", "bd", "be"] {
        assert!(
            lines.contains(&expected.to_owned()),
            "missing {expected} in {lines:?}"
        );
    }
    for fragment in ["a", "b", "c", "d", "e"] {
        assert!(
            !lines.contains(&fragment.to_owned()),
            "fragment {fragment} in {lines:?}"
        );
    }
    let lines = plain_lines("-> k\n=== k ===\n+ [again]\n    {a|b}{true:p}{c|d|e}\n    -> k\n");
    for expected in ["apc", "apd", "ape", "bpc", "bpd", "bpe"] {
        assert!(
            lines.contains(&expected.to_owned()),
            "missing {expected} in {lines:?}"
        );
    }
}
