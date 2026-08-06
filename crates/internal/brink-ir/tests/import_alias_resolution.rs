//! Import aliases must actually be honored — not just parsed and recorded
//! (issue #1590).
//!
//! `ImportItem.alias` used to be read only by the E089 duplicate-import
//! check: `resolve::import_coverage_for_file` keyed the bare-import
//! coverage set on `item.name` (the source spelling), and
//! `resolve::lookup_by_name` only ever looked candidates up by their own
//! definition name in `index.by_name` — which never contains an alias
//! spelling at all. So `IMPORT { haggle AS h } FROM mod` / `use
//! mod::haggle as h;` parsed and lowered cleanly, recorded the alias on
//! `ImportItem`, and then silently did nothing with it: a reference to `h`
//! was reported unresolved (`E024`), while a reference to the *original*
//! name `haggle` resolved fine.
//!
//! This is pre-existing on ink's `IMPORT … AS`, but #1581/#1588 newly accept
//! the native `use … as` spelling (previously `E129`), which is what makes
//! this reachable from a live native project for the first time — hence one
//! fixture per dialect below, both proving the same resolution machinery.
//!
//! **Ruling tested (issue #1590 — "is the original name still licensed?"):**
//! brink's `AS`/`as` is **additive**, not Rust's shadow-and-revoke — the
//! alias is a second local spelling for the same import, and the source
//! name stays resolvable through it too. See the doc comment on
//! `brink_analyzer::resolve::lookup_by_name` for the full justification.
//! Both directions are asserted below in the same fixture.
//!
//! **Why the defining module is an `.ink` file**, matching
//! `native_use_import_scope.rs`'s fixture: the referencing side is the
//! dialect under test; the defining side needs a *public* symbol in a
//! *declared* module. At the time this fixture was written native had no
//! visibility syntax of its own (`lower_native` left every declaration's
//! visibility at `None`, and a declared module defaults `Private`,
//! decision-log 2026-07-23), so `#@public` was the only spelling of
//! "public in a declared module" available. **That gap has since closed**
//! (issue #1582, RULED 2026-08-03: native gained its own `pub` keyword —
//! see `crates/internal/brink-ir/tests/native_pub_visibility.rs` for the
//! fully-native regression). The ink-defining-module shape here is kept
//! anyway, since it is also exercising the ink-side alias grammar
//! (`IMPORT … AS`), which a fully-native fixture cannot cover.

use brink_analyzer::{AnalysisOptions, Dialect, ModuleMap, ResolvedModule};
use brink_ir::{DiagnosticCode, FileId, HirFile, SymbolManifest};

/// `market.ink` — declared module `quest`, exporting `haggle`.
///
/// A single-segment module name, not native's `::`-hierarchical
/// `story::market::barter` (unlike `native_use_import_scope.rs`'s fixture):
/// ink's `IMPORT { … } FROM mod` grammar parses `mod` as exactly one
/// identifier token (`declaration.rs::import_module`), so it cannot name a
/// `::`-qualified module at all. `quest` matches both dialects' importer
/// fixtures below without hitting that ink-only grammar ceiling.
const MARKET: &str = "\
#@module(quest)
== haggle ==
#@public
You haggle at the market stall.
-> DONE
";

/// Ink importer: `IMPORT { haggle AS h } FROM quest`, then references both
/// the alias `h` and the original `haggle`.
const INK_IMPORTER: &str = "\
IMPORT { haggle AS h } FROM quest
== start ==
-> h
-> haggle
";

/// Native importer: `use quest::haggle as h;`, same two references.
const NATIVE_IMPORTER: &str = "\
use quest::haggle as h;

flow start() {
  -> h
  -> haggle
}
";

const MARKET_FILE: FileId = FileId(0);
const IMPORTER_FILE: FileId = FileId(1);

fn lower_ink(file: FileId, src: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "ink fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let (hir, manifest, diags) = brink_ir::hir::lower(file, &parsed.tree());
    assert!(diags.is_empty(), "ink fixture lowering: {diags:?}");
    (hir, manifest)
}

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

fn module_map(importer_module: &str) -> ModuleMap {
    [(MARKET_FILE, "quest"), (IMPORTER_FILE, importer_module)]
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

/// Run the two-file project and return (diagnostics, resolved modules for
/// every reference in `IMPORTER_FILE`, in source order).
fn analyze(
    importer_hir: &HirFile,
    importer_manifest: &SymbolManifest,
    importer_module: &str,
) -> (Vec<brink_ir::Diagnostic>, Vec<Option<String>>) {
    let (market_hir, market_manifest) = lower_ink(MARKET_FILE, MARKET);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (MARKET_FILE, &market_hir, &market_manifest),
        (IMPORTER_FILE, importer_hir, importer_manifest),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result =
        brink_analyzer::analyze_with_modules(&inputs, &module_map(importer_module), &opts, true);

    let mut targets: Vec<(rowan::TextRange, Option<String>)> = result
        .resolutions
        .iter()
        .filter(|r| r.file == IMPORTER_FILE)
        .filter_map(|r| {
            let info = result.index.symbols.get(&r.target)?;
            (info.name == "haggle").then(|| (r.range, info.module.clone()))
        })
        .collect();
    targets.sort_by_key(|(range, _)| range.start());

    (
        result.diagnostics,
        targets.into_iter().map(|(_, module)| module).collect(),
    )
}

/// Ink: `IMPORT { haggle AS h } FROM quest` — the alias `h`
/// resolves to the market's `haggle`, and (additive ruling) so does the bare
/// original name `haggle` referenced right after it.
#[test]
fn ink_import_alias_resolves_both_alias_and_original_name() {
    let (hir, manifest) = lower_ink(IMPORTER_FILE, INK_IMPORTER);
    let (diagnostics, resolved) = analyze(&hir, &manifest, "story::town");

    let offenders: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                DiagnosticCode::E024 | DiagnosticCode::E025 | DiagnosticCode::E087
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "both `-> h` and `-> haggle` must resolve cleanly: {offenders:?}"
    );
    assert_eq!(
        resolved,
        vec![Some("quest".to_string()), Some("quest".to_string())],
        "`h` (the alias) and `haggle` (the source name) must both bind the market's haggle"
    );
}

/// Native: `use quest::haggle as h;` — same two assertions,
/// proving the fix in the dialect #1581/#1588 newly exposed to this bug.
#[test]
fn native_use_alias_resolves_both_alias_and_original_name() {
    let (hir, manifest) = lower_brink(IMPORTER_FILE, NATIVE_IMPORTER);
    let (diagnostics, resolved) = analyze(&hir, &manifest, "story::town");

    let offenders: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                DiagnosticCode::E024 | DiagnosticCode::E025 | DiagnosticCode::E087
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "both `-> h` and `-> haggle` must resolve cleanly: {offenders:?}"
    );
    assert_eq!(
        resolved,
        vec![Some("quest".to_string()), Some("quest".to_string())],
        "`h` (the alias) and `haggle` (the source name) must both bind the market's haggle"
    );
}

/// Negative case: an alias is scoped to the file whose import declared it —
/// a *different* file that never imported `quest` at all
/// must not resolve a bare `h` (there is nothing in scope named `h`, aliased
/// or otherwise — it is simply an unresolved divert target, `E024`), and a
/// bare `haggle` reference is rightly flagged `E025` (import-required — a
/// public definition in a declared module the file never imported).
#[test]
fn a_file_that_never_imported_the_module_gets_neither_alias_nor_bare_access() {
    const OUTSIDER: &str = "\
== start ==
-> h
-> haggle
";
    let (hir, manifest) = lower_ink(IMPORTER_FILE, OUTSIDER);
    let (market_hir, market_manifest) = lower_ink(MARKET_FILE, MARKET);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (MARKET_FILE, &market_hir, &market_manifest),
        (IMPORTER_FILE, &hir, &manifest),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result =
        brink_analyzer::analyze_with_modules(&inputs, &module_map("story::town"), &opts, true);

    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E024),
        "`h` names nothing in this file's scope — no import, no alias — so \
         `-> h` must be an unresolved divert target: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E025),
        "a bare reference to a public definition in an un-imported declared \
         module must still be gated, alias or not: {:?}",
        result.diagnostics
    );
}
