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
//! `corpus.rs::compile_and_explore_from_brink_native`'s own hand-assembled
//! analysis pipeline (`is_native = true`, `dialect` left at its `StrictInk`
//! default) rather than reaching for this crate's `analyze`/
//! `analyze_with_options` convenience wrappers — both of those hardcode
//! `is_native = false` (see `finish_analysis`'s own doc), which spuriously
//! diagnoses every native-only syntax form this fixture needs (`STRUCT`,
//! `some`, struct construction literals) as brink-extension-gated (`E051`)
//! even though it never reaches an ink frontend. Using the real native
//! configuration here is what makes this test a faithful proof of
//! `assemble_analyzer_tables`'s actual (only) caller's inputs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, ImportScope, ResolutionMap, assemble_analyzer_tables};
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

/// `symbol_index` -> `resolve` -> `per_file_diagnostics(is_native = true)`
/// -> `native_strict_only_error` -> `whole_project_diagnostics` — the exact
/// sequence `compile_and_explore_from_brink_native` runs, see the module
/// doc for why.
fn analyze_native(
    file_id: FileId,
    hir: &HirFile,
    manifest: &SymbolManifest,
) -> (Arc<SymbolIndex>, ResolutionMap, Vec<Diagnostic>) {
    let opts = AnalysisOptions::default();
    let files_for_analysis = vec![(file_id, hir, manifest)];

    let (index, mut diagnostics) = brink_analyzer::symbol_index(&[(file_id, manifest)]);
    let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let (file_resolutions, resolve_diags) =
        brink_analyzer::resolve(file_id, manifest, &index, &scope);
    diagnostics.extend(resolve_diags);
    let mut resolutions = ResolutionMap::new();
    resolutions.extend(std::sync::Arc::unwrap_or_clone(file_resolutions));

    diagnostics.extend(brink_analyzer::per_file_diagnostics(
        file_id,
        hir,
        &resolutions,
        &index,
        opts.dialect,
        true,
        opts.host_manifest.as_ref(),
    ));
    diagnostics.extend(brink_analyzer::native_strict_only_error(
        file_id, opts.types,
    ));

    let (whole_diagnostics, _symbol_meta) = brink_analyzer::whole_project_diagnostics(
        &files_for_analysis,
        &index,
        &resolutions,
        &opts,
        None,
    );
    diagnostics.extend(whole_diagnostics);

    (index, resolutions, diagnostics)
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

/// A project using neither feature stays at the all-empty default — the
/// laziness gate this function's doc claims, pinned so a future change
/// can't accidentally make every project pay for whole-project inference.
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
