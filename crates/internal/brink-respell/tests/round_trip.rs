//! Round-trip verification for the `.brink` native-surface emitter
//! (issue #1178): `story.brink` → HIR → emit → reparse+relower → run.
//!
//! Entirely off the ink pipeline (both sides of the comparison are native
//! `.brink` source, run through the same `explore_from_brink_native` path)
//! — the oracle cannot move because this test never touches it.
//!
//! Verification is **episode-identity through the actual runtime**, not
//! HIR struct equality: the emitter's output is allowed to restructure
//! text (canonical body-then-stitches ordering, normalized whitespace,
//! `Stmt::EndOfLine` markers that carry no textual weight) as long as the
//! *compiled, executed* behavior is unchanged — exactly the differential
//! method `docs/b0-findings.md` NF-5 and this program's own exit criterion
//! describe. `brink_test_harness::corpus::explore_from_brink_native`
//! already performs the honest minimal native pipeline composition (parse
//! → `lower_native` → analyze → LIR → codegen → link → explore); this test
//! only supplies both sides of the diff.
//!
//! Fixture corpus: `tests/tier1-brink-respell/*/story.brink` — the same
//! hand-curated corpus (weave/choice/tunnel/thread/alternation-adjacent
//! semantics) `tests/tier1-brink-respell/README.md` describes as the NF-5
//! (c) seed this issue's emitter now has real machinery to regenerate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use brink_ir::FileId;
use brink_ir::hir::emit_native::emit_file;
use brink_ir::hir::lower_native;
use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/tier1-brink-respell")
        .canonicalize()
        .expect("tests/tier1-brink-respell must exist at the repo root")
}

fn explore_config() -> ExploreConfig {
    ExploreConfig {
        max_depth: 20,
        max_episodes: 200,
    }
}

/// Lower `src` through the native frontend, emit it back out, and assert
/// the emitted `.brink` plays episode-identical to the original.
fn round_trip_case(name: &str) {
    let dir = fixtures_root().join(name);
    let src = fs::read_to_string(dir.join("story.brink"))
        .unwrap_or_else(|e| panic!("{name}: read story.brink: {e}"));

    let parsed = brink_syntax_native::parse(&src);
    assert!(
        parsed.errors().is_empty(),
        "{name}: story.brink itself has parse errors: {:?}",
        parsed.errors()
    );
    let tree = parsed.tree();
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &tree);
    assert!(
        diags.is_empty(),
        "{name}: story.brink itself has lowering diagnostics: {diags:?}"
    );

    let emitted =
        emit_file(&hir).unwrap_or_else(|e| panic!("{name}: emitter refused this fixture: {e}"));

    let config = explore_config();
    let original_episodes = explore_from_brink_native(&src, &config)
        .unwrap_or_else(|e| panic!("{name}: exploring the original fixture failed: {e}"));
    let emitted_episodes = explore_from_brink_native(&emitted, &config).unwrap_or_else(|e| {
        panic!("{name}: exploring the emitted respelling failed: {e}\n--- emitted ---\n{emitted}")
    });

    assert_eq!(
        original_episodes, emitted_episodes,
        "{name}: round-trip episode mismatch\n--- emitted .brink ---\n{emitted}"
    );
}

#[test]
fn basic_tunnel() {
    round_trip_case("basic-tunnel");
}

#[test]
fn complex_flow_v1() {
    round_trip_case("complex-flow-v1");
}

#[test]
fn const_vars() {
    round_trip_case("const-vars");
}

#[test]
fn exhibit_fogg_passage() {
    round_trip_case("exhibit-fogg-passage");
}

#[test]
fn gather_basic() {
    round_trip_case("gather-basic");
}

#[test]
fn labeled_mid_flow_gather() {
    round_trip_case("labeled-mid-flow-gather");
}

#[test]
fn manual_stitch_v1() {
    round_trip_case("manual-stitch-v1");
}

#[test]
fn simple_glue() {
    round_trip_case("simple-glue");
}

#[test]
fn sticky_choice() {
    round_trip_case("sticky-choice");
}

#[test]
fn weave_options() {
    round_trip_case("weave-options");
}
