//! B5 end-to-end: `TypeName { … }` construction on the native surface,
//! through the real `.brink` pipeline (issue #1464; #1103 RULED
//! 2026-07-23 — `docs/stdlib-spec.md` §9.6).
//!
//! The unit tests in `brink-ir/tests/b5_native_construction.rs` prove the
//! *dispatch* (one CST shape → four HIR shapes). This file proves the
//! **user path**: a `.brink` source with construction literals in it
//! parses, lowers, analyzes, codegens, links, and *plays* —
//! `brink_test_harness::corpus::compile_and_explore_from_brink_native` is
//! the same honest minimal native pipeline `first_light.rs` runs the
//! respell corpus through.
//!
//! The negative case matters just as much: cascade ruling (A) says a
//! duplicate map key is a **compile error**, so the last test asserts the
//! compile is *refused* rather than silently last-wins.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

/// The concatenated text of the single episode a straight-line native
/// fixture produces.
fn play(src: &str) -> String {
    let episodes = explore_from_brink_native(src, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("native fixture must compile and play: {e}"));
    let episode = episodes.first().expect("one episode");
    episode
        .steps
        .iter()
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join("")
}

fn refuse(src: &str) -> String {
    match explore_from_brink_native(src, &ExploreConfig::default()) {
        Ok(_) => panic!("fixture must be refused, but it compiled"),
        Err(e) => e,
    }
}

/// `Map { … }` builds a real map that the map verbs then operate on.
#[test]
fn map_construction_plays() {
    let out = play(
        "\
fn count() {
  let m = Map { \"a\": 1, \"b\": 2, \"c\": 3 };
  return len(m);
}

flow main() {
  Size is {count()}.
}
",
    );
    assert_eq!(out, "Size is 3.\n");
}

/// `Flags { … }` — the element form — builds a flags value, here as a
/// declaration default so it also crosses the const-fold path.
#[test]
fn flags_construction_plays() {
    let out = play(
        "\
flags Mood = calm, wary, hostile
var mood = Flags { calm }

flow main() {
  Mood is {mood}.
}
",
    );
    assert_eq!(out, "Mood is calm.\n");
}

/// The unregistered fall-through: `P { x: 3 }` is the declared struct's
/// construction literal, with no `#` sigil anywhere on the native surface.
#[test]
fn struct_construction_plays() {
    let out = play(
        "\
struct P {
  x: int
}

fn getx() {
  let p = P { x: 3 };
  return p.x;
}

flow main() {
  X is {getx()}.
}
",
    );
    assert_eq!(out, "X is 3.\n");
}

/// Cascade ruling (B): the **total** `Weighted { … }` literal. It compiles
/// and links (the desugar reaches codegen)…
#[test]
fn weighted_construction_compiles_and_plays() {
    let out = play(
        "\
fn table() {
  let w = Weighted { 3: \"gold\", 1: \"iron\" };
  return 7;
}

flow main() {
  W is {table()}.
}
",
    );
    assert_eq!(out, "W is 7.\n");
}

/// …and it is **total**: an invalid table (a non-positive weight) is
/// refused at compile time, which is exactly the evidence-by-construction
/// the ruling cites as the reason `construct` is a protocol at all.
#[test]
fn an_invalid_weighted_table_is_refused() {
    let err = refuse(
        "\
fn table() {
  let w = Weighted { 0: \"gold\" };
  return 7;
}

flow main() {
  W is {table()}.
}
",
    );
    assert!(
        err.contains("E120"),
        "expected the weighted-table diagnostic, got: {err}"
    );
}

/// Cascade ruling (A): a duplicate key is a compile error (`E138`), not a
/// silent last-wins overwrite.
#[test]
fn a_duplicate_map_key_refuses_the_compile() {
    let err = refuse(
        "\
var m = Map { \"a\": 1, \"a\": 2 }

flow main() {
  Hi.
}
",
    );
    assert!(
        err.contains("E138"),
        "expected the duplicate-key diagnostic, got: {err}"
    );
}
