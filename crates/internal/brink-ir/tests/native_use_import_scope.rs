//! A native `use` actually *resolves* through the qualified-import path
//! (issue #1581) — not through `lookup_by_name`'s bare-name fallback.
//!
//! `lower_native::import` lowers `use story::market::barter::haggle;` to
//! `Import { module: "story::market::barter", items: [haggle], bare: true }`.
//! That string is matched by **equality** against a real module name, so the
//! only way to prove the lowering is right is to run a whole-project
//! analysis in which two *different* modules export the same leaf name and
//! check that each importing file binds the one it named. A bare-name
//! fallback cannot satisfy both directions at once: it returns the same flat
//! first-winner to both importers. Before #1581 (`module` was dot-joined and
//! kept the leaf) the imports matched nothing and exactly that happened.
//!
//! **Why the two defining modules are `.ink` files.** The referencing side
//! is the real native `use` under test. The defining side needs *public*
//! symbols in *declared* modules, and native has no visibility syntax yet —
//! `lower_native` leaves every declaration's `visibility` at `None`, and a
//! declared module defaults `Private` (decision-log 2026-07-23, "Native
//! visibility: top-level flows default to Private"), which `E087` blocks
//! from crossing a module boundary at all. `#@public` is the only spelling
//! of "public in a declared module" the compiler currently has, so the
//! fixture uses it. See the issue #1581 thread: giving native its own
//! public-visibility marker is tracked separately.

use std::collections::BTreeMap;

use brink_analyzer::{AnalysisOptions, Dialect, ModuleMap, ResolvedModule};
use brink_ir::{FileId, HirFile, SymbolManifest};

/// `market/barter.ink` — module `story::market::barter`, exporting `haggle`.
const MARKET: &str = "\
#@module(story::market::barter)
== haggle ==
#@public
You haggle at the market stall.
-> DONE
";

/// `docks/barter.ink` — module `story::docks::barter`, exporting a *homonym*
/// `haggle`. The disambiguation this whole fixture exists to force.
const DOCKS: &str = "\
#@module(story::docks::barter)
== haggle ==
#@public
You haggle on the docks.
-> DONE
";

/// `main.brink` — native, importing the market's `haggle` by qualified path.
const MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  -> haggle
}
";

/// `alt.brink` — native, importing the docks' `haggle` instead.
const ALT: &str = "\
use story::docks::barter::haggle;

flow other() {
  -> haggle
}
";

const MARKET_FILE: FileId = FileId(0);
const DOCKS_FILE: FileId = FileId(1);
const MAIN_FILE: FileId = FileId(2);
const ALT_FILE: FileId = FileId(3);

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

/// The module map `brink-db` would build for this project: every file's
/// module is *declared* (an `#@module` for the ink files; a path-derived
/// `native_module_path` for the `.brink` ones — spelled out here, since
/// `brink-db` is downstream of this crate).
fn module_map() -> ModuleMap {
    [
        (MARKET_FILE, "story::market::barter"),
        (DOCKS_FILE, "story::docks::barter"),
        (MAIN_FILE, "story::main"),
        (ALT_FILE, "story::alt"),
    ]
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

/// Analyze the four-file project and report, per importing file, the module
/// its `-> haggle` divert resolved into.
fn resolved_haggle_modules() -> BTreeMap<FileId, Option<String>> {
    let files = [
        lower_ink(MARKET_FILE, MARKET),
        lower_ink(DOCKS_FILE, DOCKS),
        lower_brink(MAIN_FILE, MAIN),
        lower_brink(ALT_FILE, ALT),
    ];
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> =
        [MARKET_FILE, DOCKS_FILE, MAIN_FILE, ALT_FILE]
            .into_iter()
            .zip(files.iter())
            .map(|(id, (hir, manifest))| (id, hir, manifest))
            .collect();

    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&inputs, &module_map(), &opts, true);

    // Both `haggle` definitions must survive into the index — M-2d
    // coexistence. If one had been dropped as a duplicate there would be
    // nothing to disambiguate and this test would prove nothing.
    assert_eq!(
        result.index.by_name.get("haggle").map(Vec::len),
        Some(2),
        "both module-qualified `haggle` definitions must coexist in the index"
    );

    let mut out = BTreeMap::new();
    for r in &result.resolutions {
        let Some(info) = result.index.symbols.get(&r.target) else {
            continue;
        };
        if info.name != "haggle" {
            continue;
        }
        let previous = out.insert(r.file, info.module.clone());
        assert!(previous.is_none(), "one `haggle` reference per file");
    }
    out
}

/// The headline: each importer binds the `haggle` of the module it actually
/// named. Asserted in **both** directions on purpose — a bare-name fallback
/// (the only path a native `use` could take before #1581) returns the same
/// flat first-winner to both files, so it fails one of the two.
#[test]
fn each_importer_binds_the_module_its_use_named() {
    let resolved = resolved_haggle_modules();
    assert_eq!(
        resolved.get(&MAIN_FILE).and_then(Option::as_deref),
        Some("story::market::barter"),
        "`use story::market::barter::haggle` must bind the market's haggle"
    );
    assert_eq!(
        resolved.get(&ALT_FILE).and_then(Option::as_deref),
        Some("story::docks::barter"),
        "`use story::docks::barter::haggle` must bind the docks' haggle"
    );
}

/// The same lowered `Import` also has to satisfy `modules::import_covers`,
/// the `E025` import-required gate — otherwise a correctly-imported
/// cross-module reference is still reported as needing an import.
#[test]
fn a_qualified_use_licenses_the_reference_it_names() {
    let files = [
        lower_ink(MARKET_FILE, MARKET),
        lower_ink(DOCKS_FILE, DOCKS),
        lower_brink(MAIN_FILE, MAIN),
        lower_brink(ALT_FILE, ALT),
    ];
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> =
        [MARKET_FILE, DOCKS_FILE, MAIN_FILE, ALT_FILE]
            .into_iter()
            .zip(files.iter())
            .map(|(id, (hir, manifest))| (id, hir, manifest))
            .collect();
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&inputs, &module_map(), &opts, true);

    let offenders: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                brink_ir::DiagnosticCode::E025 | brink_ir::DiagnosticCode::E087
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "an imported public symbol must be licensed, not flagged: {offenders:?}"
    );
}
