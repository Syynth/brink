//! Coverage for `assemble_analyzer_tables` (issue #1528): the one path a
//! caller with no salsa layer of its own (`brink-test-harness`'s
//! `corpus.rs`) assembles every analyzer side-table LIR lowering needs
//! through.
//!
//! The fixture below deliberately exercises **both** known side-tables
//! (B3a UFCS and B1 `or`-coalescing) in one project, so
//! [`neither_table_is_silently_empty`] would fail if either table's
//! gate+translate step were dropped from `assemble_analyzer_tables` — the
//! exact failure mode issue #1528 flags: a table forgotten in a salsa-free
//! caller's assembly compiles cleanly and passes every test that doesn't
//! specifically require that table, but silently stays at the empty
//! default (`UfcsLookup`/`CoalesceLookup`'s own `is_empty()` — see their
//! doc). **When a third side-table joins these two, extend this fixture
//! (and this test's assertions) to cover it too** — this test only proves
//! today's two tables are wired, not any future one; see
//! `assemble_analyzer_tables`'s own doc for why a future table's
//! *computation* only needs adding in one place, but coverage of it still
//! needs a human to extend a fixture like this one.
//!
//! `analyze_native` below deliberately mirrors `brink-test-harness`'s
//! `corpus.rs::compile_and_explore_from_brink_native`'s own analysis call
//! (`is_native = true`, `dialect` left at its `StrictInk` default) rather
//! than reaching for this crate's `analyze`/`analyze_with_options`
//! convenience wrappers — both of those hardcode `is_native = false`, which
//! spuriously diagnoses every native-only syntax form this fixture needs
//! (`STRUCT`, `some`, struct construction literals) as brink-extension-gated
//! (`E051`) even though it never reaches an ink frontend. Using the real
//! native configuration here is what makes this test a faithful proof of
//! `assemble_analyzer_tables`'s actual (only) caller's inputs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, ModuleMap, ResolutionMap, assemble_analyzer_tables};
use brink_ir::hir::lower_native;
use brink_ir::{Diagnostic, FileId, HirFile, SymbolIndex, SymbolManifest};

/// A `.brink` fixture with a resolvable UFCS `FreeFnDesugar` call
/// (`g.greet(3)`, mirroring `brink-test-harness/tests/b3a_ufcs_e2e.rs`'s
/// minimal shape) *and* a coalescing chain with a statically pinned
/// left-hand side (`some(1) or 2`, mirroring
/// `brink-analyzer/tests/coalesce_types.rs`'s minimal shape) — one project
/// that needs both tables non-empty at once.
const SRC: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

flow main() {
  Value is {some(1) or 2}.
  Greeting is {shout()}.
  -> END
}

fn shout() {
  let g = Guest { name: \"ada\" };
  return g.greet(3);
}
";

fn lower(src: &str) -> (HirFile, SymbolManifest) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
    (hir, manifest)
}

/// `analyze_with_modules(is_native = true)` with a module-blind map — the
/// exact call `compile_and_explore_from_brink_native` makes, see the module
/// doc for why.
fn analyze_native(
    file_id: FileId,
    hir: &HirFile,
    manifest: &SymbolManifest,
) -> (Arc<SymbolIndex>, ResolutionMap, Vec<Diagnostic>) {
    let result = brink_analyzer::analyze_with_modules(
        &[(file_id, hir, manifest)],
        &ModuleMap::new(),
        &AnalysisOptions::default(),
        true,
    );
    (result.index, result.resolutions, result.diagnostics)
}

#[test]
fn neither_table_is_silently_empty() {
    let file_id = FileId(0);
    let (hir, manifest) = lower(SRC);
    let (index, resolutions, diagnostics) = analyze_native(file_id, &hir, &manifest);
    assert!(
        diagnostics.is_empty(),
        "fixture must analyze cleanly: {diagnostics:?}"
    );

    let hir_inputs = vec![(file_id, &hir)];
    let manifest_inputs = vec![(file_id, &manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);

    let tables = assemble_analyzer_tables(&hir_inputs, &index, &resolutions, None, &inline_docs);

    assert!(
        !tables.ufcs.is_empty(),
        "the UFCS table must record the fixture's g.greet(3) verdict — an \
         empty table here means assemble_analyzer_tables silently dropped \
         the UFCS side-table"
    );
    assert!(
        !tables.coalesce.is_empty(),
        "the coalesce table must record the fixture's `some(1) or 2` chain \
         — an empty table here means assemble_analyzer_tables silently \
         dropped the coalesce side-table"
    );
}

/// A project using neither feature stays at the all-empty default. This
/// pins the *result* only — it does not observe whether `infer_project` ran,
/// so it does not by itself prove the laziness gate (`needs_ufcs` /
/// `needs_coalesce` short-circuiting whole-project inference) is still
/// wired: an unconditional `infer_project` call would produce the same
/// all-empty tables for this feature-free fixture and still pass here. See
/// `assemble_analyzer_tables`'s own doc for the laziness claim itself.
#[test]
fn a_project_using_neither_feature_stays_empty() {
    let file_id = FileId(0);
    let src = "\
flow main() {
  Value is 1.
  -> END
}
";
    let (hir, manifest) = lower(src);
    let (index, resolutions, diagnostics) = analyze_native(file_id, &hir, &manifest);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let hir_inputs = vec![(file_id, &hir)];
    let manifest_inputs = vec![(file_id, &manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);

    let tables = assemble_analyzer_tables(&hir_inputs, &index, &resolutions, None, &inline_docs);

    assert!(tables.ufcs.is_empty());
    assert!(tables.coalesce.is_empty());
}
