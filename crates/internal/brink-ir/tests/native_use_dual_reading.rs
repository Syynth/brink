//! End-to-end proof that dual-reading `use` (issue #1592) actually
//! **resolves** references, not just avoids a diagnostic.
//!
//! `crates/internal/brink-analyzer/src/modules.rs`'s unit tests cover the
//! diagnostic side (`E088`/`E090`) precisely and cheaply against hand-built
//! `SymbolIndex` fixtures. This file proves the other half through the real
//! parse → lower → analyze pipeline: a native `use story::market::barter;`
//! whose trailing segment `barter` names a **module**, not an item, must
//! license **module-qualified** references to that submodule's public
//! exports inside the importing file (`-> barter::haggle`) — "a trailing
//! segment that resolves to a module licenses that module, exactly as
//! Rust's `use` does" (the #1592 ruling's own words,
//! `docs/decision-log.md` 2026-07-27).
//!
//! ⚠ **Corrected 2026-08-05 (issue #2287).** This file's own prose (and
//! `resolve::import_coverage_for_file`'s doc, and `docs/modules-spec.md`
//! §2) used to over-read that ruling as "the submodule's public exports
//! also become **bare**-referenceable" — i.e. that `use story::market::
//! barter;` alone would license bare `-> haggle`, with no `barter::`
//! qualifier at all. That over-reading was itself the bug #2287 reported:
//! Rust's `use a::b;` makes `b` nameable as `b::item`, never brings `item`
//! into bare scope — "licenses that module" always meant *qualified*
//! access, matching every other qualified-module-import case in this
//! codebase (`IMPORT quest_3` → `-> quest_3.ambush.start`,
//! `native-surface-charter.md` §13.2). Only a **symbol-level or glob**
//! import brings a name into *bare* scope. The fixtures below were fixed
//! to assert the corrected (and originally-intended) reading.
//!
//! **Why the defining side is `.ink`, again.** Same reason as
//! `native_use_import_scope.rs` (issue #1581): at the time this fixture was
//! written, native had no working visibility marker of its own — every
//! native declaration's `visibility` stayed `None`, and a *declared* native
//! module defaults `Private`, which blocks any cross-module reference
//! outright (`E087`) before dual-reading is even relevant. `#@public` on an
//! `.ink` file was the only "public in a declared module" spelling the
//! compiler had, so that is what proves the resolution mechanics. **That
//! gap has since closed** (issue #1582, RULED 2026-08-03: native gained its
//! own `pub` keyword — see
//! `crates/internal/brink-ir/tests/native_pub_visibility.rs` for the
//! fully-native regression). The ink-defining-module shape here is kept
//! anyway: it is also exercising dual-reading's module-vs-item resolution
//! (a `use` naming a *module*, not an item), which is orthogonal to which
//! dialect defines the module and not itself proven by the `#1582`
//! fixture.

use std::collections::BTreeMap;

use brink_analyzer::{AnalysisOptions, Dialect, ModuleMap, ResolvedModule};
use brink_ir::{DiagnosticCode, FileId, HirFile, SymbolManifest};

/// `market/barter.ink` — module `story::market::barter`, exporting `haggle`.
/// Note there is **no file at all** for `story::market` itself — it is a
/// pure directory, never any file's own module. That is the exact shape the
/// original silent no-op required: `story::market` never had a
/// `declared_exports` entry to check `barter` against.
const BARTER: &str = "\
#@module(story::market::barter)
== haggle ==
#@public
You haggle at the market stall.
-> DONE
";

/// `main.brink` — native, `use`s the **module**, not an item
/// (`use story::market::barter;`, no `{ }`, no trailing item beyond the
/// submodule's own name) and then references its export **module-qualified**
/// (issue #2287's corrected reading — see the module doc).
const MAIN: &str = "\
use story::market::barter;

flow start() {
  -> barter::haggle
}
";

/// The bare-divert sibling of [`MAIN`] — same import, but the reference
/// omits the qualifier. Proves the other (dangerous) half of #2287's
/// correction: a module-only import must NOT also license this spelling.
const MAIN_BARE: &str = "\
use story::market::barter;

flow start() {
  -> haggle
}
";

/// A sibling fixture where the trailing segment names neither an item nor a
/// module — the case #1592 requires to newly diagnose.
const MAIN_TYPO: &str = "\
use story::market::nonexistent;

flow start() {
  -> DONE
}
";

const BARTER_FILE: FileId = FileId(0);
const MAIN_FILE: FileId = FileId(1);

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

/// `story::market` is deliberately **absent** — no file is that module, only
/// its submodule `story::market::barter` is declared.
fn module_map() -> ModuleMap {
    module_map_with_main("story::main")
}

/// Parameterized over `MAIN_FILE`'s own declared module — used by the #1686
/// review's regression test to make `MAIN_FILE` the **parent** module
/// (`story::market`) rather than an unrelated sibling (`story::main`), which
/// is exactly the shape that false-positived `E090` before the fix (see
/// `parent_importing_its_own_declared_child_submodule_licenses_with_no_e090`
/// below).
fn module_map_with_main(main_module: &str) -> ModuleMap {
    [
        (BARTER_FILE, "story::market::barter"),
        (MAIN_FILE, main_module),
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

/// `use story::market::barter;` — naming the module, not an item — must
/// license `-> barter::haggle` (module-qualified access to the submodule's
/// public export), with zero diagnostics: no `E025`/`E087` (unresolved/
/// private cross-module reference) and no `E088` (the retired silent
/// no-op's diagnostic).
#[test]
fn use_naming_a_module_licenses_qualified_access_to_its_exports() {
    let barter = lower_ink(BARTER_FILE, BARTER);
    let main = lower_brink(MAIN_FILE, MAIN);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (BARTER_FILE, &barter.0, &barter.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
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
                DiagnosticCode::E024
                    | DiagnosticCode::E025
                    | DiagnosticCode::E087
                    | DiagnosticCode::E088
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "`use story::market::barter;` must license `-> barter::haggle`, not flag it: \
         {offenders:?}"
    );

    // And the reference must have actually resolved into `barter`'s
    // `haggle`, not merely gone unflagged by coincidence.
    let haggle_module: BTreeMap<FileId, Option<String>> = result
        .resolutions
        .iter()
        .filter_map(|r| {
            let info = result.index.symbols.get(&r.target)?;
            (info.name == "haggle").then(|| (r.file, info.module.clone()))
        })
        .collect();
    assert_eq!(
        haggle_module.get(&MAIN_FILE).and_then(Option::as_deref),
        Some("story::market::barter"),
        "the qualified `-> barter::haggle` in main.brink must resolve into the licensed submodule"
    );
}

/// The other (dangerous) half of issue #2287's correction: the exact same
/// import that licenses `-> barter::haggle` above must NOT also license
/// the bare `-> haggle` spelling — a module-qualified import brings the
/// module's name into scope, never its individual exports' bare names.
#[test]
fn use_naming_a_module_does_not_license_bare_access_to_its_exports() {
    let barter = lower_ink(BARTER_FILE, BARTER);
    let main = lower_brink(MAIN_FILE, MAIN_BARE);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (BARTER_FILE, &barter.0, &barter.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&inputs, &module_map(), &opts, true);

    let e024: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E024)
        .collect();
    assert_eq!(
        e024.len(),
        1,
        "`use story::market::barter;` must leave bare `-> haggle` unresolved — a module import \
         licenses the module's name, not its exports' bare names: {:?}",
        result.diagnostics
    );
}

/// The retired silent no-op: `use story::market::nonexistent;` names
/// neither an item `story::market` exports (it exports nothing — no file is
/// that module) nor a declared submodule (`story::market::nonexistent` is
/// not `story::market::barter`). Before #1592 this diagnosed nothing at
/// all; it must now raise `E088`.
#[test]
fn use_naming_neither_an_item_nor_a_module_now_diagnoses() {
    let barter = lower_ink(BARTER_FILE, BARTER);
    let main = lower_brink(MAIN_FILE, MAIN_TYPO);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (BARTER_FILE, &barter.0, &barter.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&inputs, &module_map(), &opts, true);

    let e088: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E088)
        .collect();
    assert_eq!(
        e088.len(),
        1,
        "a trailing segment resolving to neither an item nor a module must now diagnose \
         (the retired silent no-op): {:?}",
        result.diagnostics
    );
}

/// `main.brink` declared as **`story::market`** itself — the parent of the
/// declared submodule `story::market::barter` it `use`s — reusing the same
/// `MAIN` source (`use story::market::barter;`) as the module-licensing
/// test above.
const MAIN_MODULE: &str = "story::market";

/// Review finding #1686 (BLOCKING E090 false positive): a **parent** module
/// (`story::market`) importing its own declared **child** submodule
/// (`story::market::barter`) via the leaf-item shape must diagnose nothing —
/// in particular no `E090` — and must still license `-> barter::haggle`
/// (module-qualified, issue #2287's corrected reading — see the module
/// doc), end to end through the real pipeline (not just the
/// diagnostics-only unit test in `brink-analyzer/src/modules.rs`). This is
/// the exact repro the review gave: changing `MAIN_FILE`'s module from
/// `story::main` to `story::market`.
#[test]
fn parent_importing_its_own_declared_child_submodule_licenses_with_no_e090() {
    let barter = lower_ink(BARTER_FILE, BARTER);
    let main = lower_brink(MAIN_FILE, MAIN);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (BARTER_FILE, &barter.0, &barter.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(
        &inputs,
        &module_map_with_main(MAIN_MODULE),
        &opts,
        true,
    );

    assert!(
        result.diagnostics.is_empty(),
        "a parent module importing its own declared child submodule must diagnose nothing \
         (no E090 false positive, no E025/E087/E088): {:?}",
        result.diagnostics
    );

    let haggle_module: BTreeMap<FileId, Option<String>> = result
        .resolutions
        .iter()
        .filter_map(|r| {
            let info = result.index.symbols.get(&r.target)?;
            (info.name == "haggle").then(|| (r.file, info.module.clone()))
        })
        .collect();
    assert_eq!(
        haggle_module.get(&MAIN_FILE).and_then(Option::as_deref),
        Some("story::market::barter"),
        "licensing must still apply — the qualified `-> barter::haggle` must resolve into the \
         child submodule even when the importer is the parent module"
    );
}

/// `use story::market::barter as b;` — the trailing segment `barter`
/// resolves as a declared submodule, and the item is also aliased.
const MAIN_ALIASED: &str = "\
use story::market::barter as b;

flow start() {
  -> DONE
}
";

/// Review finding #1686 (BLOCKING aliased trailing module segment): aliasing
/// a trailing segment that resolves as a **module** has no sound `Import`
/// representation (aliasing a whole export set, not one name) and must
/// diagnose `E129` end to end through the real pipeline — mirroring
/// `lower_native::import::lower_use_decl`'s `E129` for the single-segment
/// `use a as m;` module-alias shape, just decided one pipeline stage later
/// once whole-project module data resolves the dual-reading.
#[test]
fn aliased_trailing_segment_resolving_to_a_module_diagnoses_e129() {
    let barter = lower_ink(BARTER_FILE, BARTER);
    let main = lower_brink(MAIN_FILE, MAIN_ALIASED);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (BARTER_FILE, &barter.0, &barter.1),
        (MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_modules(&inputs, &module_map(), &opts, true);

    let e129: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::E129)
        .collect();
    assert_eq!(
        e129.len(),
        1,
        "aliasing a trailing segment that resolves as a module must diagnose E129, not \
         silently drop the alias: {:?}",
        result.diagnostics
    );
}
