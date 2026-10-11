//! Issue #3413: a `shuffle once` with text around it lost that text once
//! its alternatives ran out. ink prints the prefix and suffix on every
//! later visit. The inline lift gives a plain `once` an extra "exhausted"
//! branch holding just the prefix and suffix, and makes it `stopping`. It
//! skipped `shuffle once`, on the worry that the extra branch would be
//! shuffled into the pool. It is not: `shuffle stopping` shuffles all but
//! its last branch and then sticks on it, so with the extra branch it
//! draws exactly as `shuffle once` does and then repeats the prefix and
//! suffix. (ink compiles `once` the same way: an extra empty element, and
//! `stopping`.) Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6). brink's shuffle order differs
//! from ink's (#3538), so with several alternatives only the set drawn is
//! compared.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Run `body` `visits` times (a knot that counts its visits in `n` and
/// diverts back to itself), returning every delivered line's text.
fn play(body: &str, visits: u32) -> Vec<String> {
    let source =
        format!("VAR n = 0\n-> k\n\n=== k ===\n~ n++\n{body}\n{{n < {visits}: -> k}}\n-> END\n");
    let output = brink_compiler::compile("story.ink", |_| Ok(source.clone()));
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
            Step::End => return lines,
            other => panic!("unexpected step {other:?} in {source}"),
        }
    }
    panic!("story did not end in 200 steps");
}

/// One alternative, so the order cannot differ: the issue's shape.
#[test]
fn the_text_around_a_shuffle_once_survives_exhaustion() {
    assert_eq!(
        play("A {shuffle once:c} Z", 3),
        ["A c Z\n", "A Z\n", "A Z\n"]
    );
    assert_eq!(play("{shuffle once:c} Z", 3), ["c Z\n", "Z\n", "Z\n"]);
    assert_eq!(play("A {shuffle once:c}", 3), ["A c\n", "A\n", "A\n"]);
}

/// Several alternatives: each is drawn exactly once, then the text around
/// them repeats.
#[test]
fn every_alternative_is_drawn_once_before_the_text_repeats() {
    let lines = play("A {shuffle once:c|d|e} Z", 5);
    assert_eq!(lines.len(), 5, "{lines:?}");
    let mut drawn: Vec<&str> = lines[..3].iter().map(String::as_str).collect();
    drawn.sort_unstable();
    assert_eq!(drawn, ["A c Z\n", "A d Z\n", "A e Z\n"]);
    assert_eq!(lines[3..], ["A Z\n", "A Z\n"]);

    let lines = play("{shuffle once:c|d} Z", 4);
    let mut drawn: Vec<&str> = lines[..2].iter().map(String::as_str).collect();
    drawn.sort_unstable();
    assert_eq!(drawn, ["c Z\n", "d Z\n"]);
    assert_eq!(lines[2..], ["Z\n", "Z\n"]);
}

/// Without text around it, an exhausted `shuffle once` still prints
/// nothing at all.
#[test]
fn a_bare_shuffle_once_prints_nothing_after_exhaustion() {
    let mut lines = play("{shuffle once:c|d}", 4);
    lines.sort_unstable();
    assert_eq!(lines, ["c\n", "d\n"]);
}
