//! `pub` (issue #1582, RULED 2026-08-03) is the native visibility marker
//! that makes a cross-file native reference legal at all.
//!
//! `check_cross_module_refs` (`brink-analyzer/src/modules.rs`) gates in two
//! INDEPENDENT stages: a private target is `E087` unconditionally, and only
//! a *public* target in another declared module reaches the second gate
//! (`E025`, unless the referrer imported it). Native already had `USE_DECL`
//! in its grammar (issue #1581), so the import half of that machinery
//! existed — it never got a chance to matter, because every native
//! declaration lowered with `visibility: None`, which a declared module
//! (native modules are always declared — they derive identity from the
//! file path) treats as `Private`. So **every** cross-file native
//! reference raised `E087`, regardless of `use`.
//!
//! This is the fully-native two-file proof the issue's ruling asked for:
//! the SAME two-file project, differing only in whether the definition
//! carries `pub` — `E087` before, clean after. Reverting the production
//! diff (the grammar + `visibility_mark`/`is_pub` wiring) turns
//! `market_flow_is_public_and_licensed_by_use` back into a parse error (no
//! `pub` token exists) or an `E087`, proving this is a real regression
//! guard, not a vacuous one.

use brink_analyzer::{AnalysisOptions, Dialect, ModuleMap, ResolvedModule};
use brink_ir::{DiagnosticCode, FileId, HirFile, SymbolManifest, VisibilityMark};

const MARKET_FILE: FileId = FileId(0);
const MAIN_FILE: FileId = FileId(1);

fn lower_brink(file: FileId, src: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax_native::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "native fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let (hir, manifest, diags) = brink_ir::hir::lower_native::lower(file, &parsed.tree());
    assert!(diags.is_empty(), "native fixture lowering: {diags:?}");
    (hir, manifest)
}

fn module_map() -> ModuleMap {
    [(MARKET_FILE, "story::market"), (MAIN_FILE, "story::main")]
        .into_iter()
        .map(|(file, name)| {
            (
                file,
                ResolvedModule {
                    name: name.to_string(),
                    declared: true,
                    was: None,
                },
            )
        })
        .collect()
}

/// `market.brink` — module `story::market`, exporting a private (default)
/// `haggle` flow.
const MARKET_PRIVATE: &str = "\
flow haggle() {
  You haggle at the market stall.
  -> DONE
}
";

/// `market.brink` — the same flow, marked `pub` (issue #1582).
const MARKET_PUBLIC: &str = "\
pub flow haggle() {
  You haggle at the market stall.
  -> DONE
}
";

/// `main.brink` — native, importing and referencing the market's `haggle`.
const MAIN: &str = "\
use story::market::haggle;

flow start() {
  -> haggle
}
";

fn diagnostics_for(market_src: &str, main_src: &str) -> Vec<brink_ir::Diagnostic> {
    let market = lower_brink(MARKET_FILE, market_src);
    let main = lower_brink(MAIN_FILE, main_src);
    let files: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (MARKET_FILE, &market.0, &market.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&files, &module_map(), &opts, true);
    result.diagnostics
}

/// **Before**: without `pub`, a `use`-imported cross-file native reference
/// still raises `E087` unconditionally — the private-default gate never
/// even reaches the import-coverage check. Proves the bug report in
/// #1582's body end to end on the native surface (previously only shown
/// against a mixed ink/native pair — `native_use_import_scope.rs`).
#[test]
fn private_by_default_native_flow_raises_e087_across_files_even_with_use() {
    let diags = diagnostics_for(MARKET_PRIVATE, MAIN);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E087),
        "expected E087 for a private-by-default cross-file reference: {diags:?}"
    );
}

/// **After**: `pub` on the definition licenses the same reference — `use`
/// now gets to matter, since visibility no longer rejects it first.
#[test]
fn pub_flow_resolves_the_same_cross_file_reference_licensed_by_use() {
    let diags = diagnostics_for(MARKET_PUBLIC, MAIN);
    let offenders: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::E087 | DiagnosticCode::E025))
        .collect();
    assert!(
        offenders.is_empty(),
        "a `pub` definition imported via `use` must not be flagged: {offenders:?}"
    );
}

/// Sanity: both fixtures parse and lower to exactly one `Knot` each (so the
/// diagnostics above are really about the cross-module gate, not a missing
/// definition), and the public fixture's `Knot::visibility` is exactly
/// `Some(VisibilityMark::Public)` — the field `check_cross_module_refs`
/// actually reads.
#[test]
fn both_fixtures_lower_to_one_knot_each_and_the_public_one_carries_the_mark() {
    let (market_hir, _) = lower_brink(MARKET_FILE, MARKET_PUBLIC);
    let (main_hir, _) = lower_brink(MAIN_FILE, MAIN);
    assert_eq!(market_hir.knots.len(), 1);
    assert_eq!(main_hir.knots.len(), 1);
    assert_eq!(market_hir.knots[0].visibility, Some(VisibilityMark::Public));
}
