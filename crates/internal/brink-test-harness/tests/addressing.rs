//! Integration tests for qualified path addressing
//! ([`Program::find_address`]).
//!
//! Compiles small ink stories with the brink compiler, links them, and
//! checks that knot, qualified stitch, and author-label paths
//! (`knot.label`, `knot.stitch.label`) resolve via the compiler-emitted
//! `address_paths` table.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use brink_runtime::{FlowInstance, Program};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

fn compile(src: &str) -> (Program, LineTables) {
    let out = brink_compiler::compile("t.ink", |p| {
        if p == "t.ink" {
            Ok(src.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such include",
            ))
        }
    })
    .expect("compile");
    brink_runtime::link(&out.data).expect("link")
}

/// Knots, qualified stitches, and author labels under both a knot and a
/// stitch all resolve; unqualified label names and unknown paths do not.
#[test]
fn resolves_knot_stitch_and_label_paths() {
    let (program, _lt) = compile(
        "-> knot1\n\
         === knot1 ===\n\
         intro\n\
         - (klabel) gathered at knot\n\
         = stitch1\n\
         stuff\n\
         - (slabel) gathered at stitch\n\
         -> END\n",
    );

    // Scope paths (already worked before this feature).
    assert!(program.find_address("knot1").is_some(), "knot");
    assert!(program.find_address("knot1.stitch1").is_some(), "stitch");

    // Author labels — the new capability.
    assert!(
        program.find_address("knot1.klabel").is_some(),
        "label directly under a knot"
    );
    assert!(
        program.find_address("knot1.stitch1.slabel").is_some(),
        "label under a stitch"
    );

    // Unqualified label names and unknown paths must not resolve.
    assert!(program.find_address("klabel").is_none(), "bare label");
    assert!(program.find_address("slabel").is_none(), "bare label");
    assert!(program.find_address("knot1.nope").is_none(), "unknown");
}

/// A label path resolves to the start (offset 0) of a real container, so a
/// flow can be spawned there.
#[test]
fn label_path_is_a_spawnable_position() {
    let (program, _lt) = compile(
        "-> start\n\
         === start ===\n\
         first\n\
         - (here) second\n\
         -> END\n",
    );

    let (idx, offset) = program
        .find_address("start.here")
        .expect("start.here should resolve");
    assert_eq!(offset, 0, "label resolves to the start of its container");
    // The resolved position is a valid spawn point.
    let (_flow, _ctx) = FlowInstance::new_at(&program, idx);
}
