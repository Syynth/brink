//! Issue #3532: list values carried stale or merged origins — `^` merged
//! both operands' origins, `LIST_MIN`/`MAX`/`ALL`/`INVERT`/`RANGE` cloned
//! their input's, and assignment retained old origins only when the new
//! empty value had none — so `LIST_ALL`/`LIST_INVERT`/`LIST_COUNT` over an
//! emptied list disagreed with ink. ink's rule (`InkList.originNames`,
//! `RetainListOriginsForAssignment`): a non-empty list's origins are its
//! items', an empty list's are whatever it was built with (none for a
//! fresh list, the left operand's for `+`/`-`, the input's for a
//! non-empty `LIST_RANGE`), and an empty list assigned over a list value
//! takes that value's origins. Reference outputs below are inkjs 2.4.0
//! via `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6); the corpus case with a C# golden
//! is owed (dotnet, maintainer-local).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it to the end, returning every
/// delivered line's text verbatim.
fn play(source: &str) -> Vec<String> {
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
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.clone()),
            Step::Choices(_) => panic!("unexpected choices in {source}"),
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

const PRELUDE: &str =
    "LIST l = (a), b\nLIST m = (c), d\nVAR g = (a)\nVAR h = ()\n-> k\n\n=== k ===\n";

/// Plays `PRELUDE` + `body` and returns each line with its `N:` label and
/// trailing `.\n` stripped, so the expectations read as ink prints them.
fn run(body: &str) -> Vec<String> {
    play(&format!("{PRELUDE}{body}-> END\n"))
        .into_iter()
        .map(|l| {
            let l = l.trim_end_matches('\n').trim_end_matches('.');
            l.split_once(':').expect("labelled line").1.to_owned()
        })
        .collect()
}

/// Operations that build a fresh list carry no origins when empty.
#[test]
fn fresh_lists_have_no_origins_when_empty() {
    let body = "1:{LIST_INVERT(LIST_INVERT((a,b)))}.\n\
                2:{LIST_INVERT(l ^ m)}.\n\
                3:{LIST_INVERT(LIST_MIN(l - l))}.\n\
                4:{LIST_COUNT(LIST_INVERT(l ^ m))}.\n\
                5:{LIST_ALL(LIST_INVERT(LIST_ALL(l)))}.\n\
                6:{LIST_ALL(LIST_MAX(l ^ m))}.\n\
                7:{LIST_ALL(LIST_INVERT((a) + 5))}.\n\
                8:{LIST_ALL((a) + 5)}.\n\
                9:{LIST_ALL(LIST_INVERT(LIST_ALL((a)+(c))))}.\n";
    assert_eq!(run(body), ["", "", "", "0", "", "", "", "", ""]);
}

/// `+` and `-` start from a copy of the left operand; a non-empty result
/// derives its origins from its items, never from an empty operand.
#[test]
fn union_and_without_keep_the_left_operands_origins() {
    let body = "1:{LIST_INVERT(l - l)}.\n\
                2:{LIST_ALL((a) + (m - m))}.\n\
                3:{LIST_ALL((m - m) + (a))}.\n\
                4:{LIST_ALL((l - l) + (m - m))}.\n\
                5:{LIST_ALL((l - l) - m)}.\n\
                6:{LIST_ALL(LIST_INVERT((a) + (c)))}.\n";
    assert_eq!(
        run(body),
        ["a, b", "a, b", "a, b", "a, b", "a, b", "a, c, b, d"]
    );
}

/// `LIST_RANGE` of an empty input is a fresh list; of a non-empty input
/// it keeps the input's origins even when the range empties it.
#[test]
fn range_keeps_origins_only_for_a_non_empty_input() {
    let body = "1:{LIST_ALL(LIST_RANGE(l, 5, 9))}.\n\
                2:{LIST_ALL(LIST_RANGE(l - l, 1, 9))}.\n\
                3:{LIST_ALL(LIST_RANGE((a)+(c), 2, 2))}.\n";
    assert_eq!(run(body), ["a, b", "", "a, c, b, d"]);
}

/// Assigning an empty list to a global or an existing temp replaces the
/// new value's origins with the old value's — whatever they were,
/// including none.
#[test]
fn empty_assignment_retains_the_old_values_origins() {
    let body = "~ g = m - m\n1:{LIST_ALL(g)}.\n\
                ~ g = (c)\n~ g = l - l\n2:{LIST_ALL(g)}.\n\
                ~ h = l - l\n3:{LIST_ALL(h)}.\n\
                ~ h = m - m\n4:{LIST_ALL(h)}.\n\
                ~ temp t = (a)\n~ t = m - m\n5:{LIST_ALL(t)}.\n\
                ~ temp u = ()\n~ u = m - m\n6:{LIST_ALL(u)}.\n\
                ~ u = ()\n7:{LIST_ALL(u)}.\n";
    assert_eq!(run(body), ["a, b", "c, d", "", "", "a, b", "", ""]);
}

/// Retention also applies to a write through a `ref` parameter.
#[test]
fn empty_assignment_through_ref_retains_origins() {
    let body = "~ clear(g)\n1:{LIST_ALL(g)}.\n";
    let src = format!("{PRELUDE}{body}-> END\n\n=== function clear(ref x) ===\n~ x = m - m\n");
    let lines: Vec<String> = play(&src)
        .into_iter()
        .map(|l| l.trim_end_matches('\n').trim_end_matches('.').to_owned())
        .collect();
    assert_eq!(lines, ["1:a, b"]);
}

/// The differential's original find: a function returning an emptied
/// list intersected with a fresh one-item list.
#[test]
fn differential_shape_prints_nothing() {
    let src = "LIST l0_a = (li0_0)\n-> k\n\n=== k ===\n{(f0_a() ^ LIST_MAX((li0_0)))}\n-> END\n\n\
               === function f0_a() ===\n~ return LIST_INVERT(LIST_INVERT((li0_0)))\n";
    assert_eq!(play(src), vec!["\n"]);
}
