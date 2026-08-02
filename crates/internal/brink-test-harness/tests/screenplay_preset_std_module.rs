//! `std/conventions/screenplay.brink` mechanical coverage (issue #1720
//! review finding).
//!
//! Before this file, the built-in screenplay preset's shipped source
//! (`std/conventions/screenplay.brink`) had zero automated coverage: nothing
//! parsed, compiled, linted, or ran it directly. The only proof of its
//! correctness was a hand-duplicated declaration block in
//! `tests/tier1-native/conventions-screenplay-preset/story.brink`, kept in
//! sync with the real file only by `std/README.md`'s "keep the two in
//! sync" plea — while `docs/native-feature-status.md` promoted the row to
//! `Parses ✅` / `Runs ✅`, cells that table's own legend ties to actually
//! parsing/running the artifact (`❓` = "unverified"). A silent drift
//! between the two files would have gone undetected: the golden fixture
//! would keep passing even if the real `std/` file broke, was cut down, or
//! renamed a handler.
//!
//! This test reads the real, shipped file directly and asserts it lowers
//! with no diagnostics through the same native frontend
//! (`brink_ir::hir::lower_native::lower`) every `.brink` compile goes
//! through, and that it declares exactly the four claim handlers the
//! preset's own module doc promises: `heading`, `transition`, `cue`, and
//! `parenthetical`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_ir::FileId;

/// Repo-root-relative path to the shipped preset source, mirroring
/// `tier1_native.rs`'s own `corpus_dir()` shape (`CARGO_MANIFEST_DIR` for
/// this crate is `crates/internal/brink-test-harness`; three `..` reach the
/// repo root).
fn screenplay_preset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("std")
        .join("conventions")
        .join("screenplay.brink")
}

#[test]
fn screenplay_preset_std_module_lowers_with_no_diagnostics_and_four_claim_handlers() {
    let path = screenplay_preset_path();
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let parse = brink_syntax_native::parse(&src);
    assert!(
        parse.errors().is_empty(),
        "std/conventions/screenplay.brink must parse cleanly: {:?}",
        parse.errors()
    );

    let (hir, _manifest, diags) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    assert!(
        diags.is_empty(),
        "std/conventions/screenplay.brink must lower with no diagnostics: {diags:?}"
    );

    let mut handler_names: Vec<&str> = hir
        .claim_handlers
        .iter()
        .map(|h| h.name.text.as_str())
        .collect();
    handler_names.sort_unstable();

    assert_eq!(
        handler_names,
        vec!["cue", "heading", "parenthetical", "transition"],
        "the shipped preset must declare exactly the four handlers its own \
         module doc promises — got {handler_names:?}. If this fails, either \
         `std/conventions/screenplay.brink` drifted from \
         `tests/tier1-native/conventions-screenplay-preset/story.brink` (see \
         `std/README.md`'s \"keep the two in sync\" note), or a handler was \
         renamed without updating this test."
    );
}
