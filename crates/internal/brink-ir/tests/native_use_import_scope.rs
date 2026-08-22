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
//! symbols in *declared* modules; at the time this fixture was written,
//! native had no visibility syntax of its own — `lower_native` left every
//! declaration's `visibility` at `None`, and a declared module defaults
//! `Private` (decision-log 2026-07-23, "Native visibility: top-level flows
//! default to Private"), which `E087` blocks from crossing a module
//! boundary at all — so `#@public` (the brink-dialect tag directive) was
//! the only spelling of "public in a declared module" the compiler had,
//! and the fixture used it. **That gap has since closed** (issue #1582,
//! RULED 2026-08-03: native gained its own `pub` keyword —
//! `crates/internal/brink-ir/tests/native_pub_visibility.rs` is the
//! fully-native two-file regression this fixture's own doc used to flag as
//! missing). The ink-defining-module shape here is kept anyway: it is
//! ALSO exercising the cross-*dialect* case (an ink module referenced by a
//! native `use`), which a fully-native fixture cannot cover.

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

// ─── Issue #2287: module-qualified divert resolution ────────────────
//
// A self-contained two-file project, separate from the fixtures above —
// this exercises `use story::market::barter;` (the *module* form, no
// braces, no leaf item) rather than `use story::market::barter::haggle;`
// (the bare-item form the fixtures above already cover). Whole-project,
// end to end through `analyze_with_modules`, not just `resolve.rs`'s own
// unit tests: this is what actually proves the fix reaches a real native
// `use` + `-> divert` pair rather than only the hand-built `ImportScope`/
// `UnresolvedRef` fixtures `brink-analyzer::resolve`'s own tests use.

/// `market/barter.brink` — native, module `story::market::barter`,
/// exporting a public flow `haggle`. One *typed* parameter on purpose
/// (#2298 review round, finding 6): the call fixtures below invoke
/// `haggle(x)`, so a zero-param declaration made every "clean" control
/// silently tolerate an arity diagnostic its E024/E025/E087 filter never
/// saw — and an untyped `x` draws strict inference's E065 instead. With
/// `x: int` (and the divert fixtures passing an argument to match), each
/// control fixture analyzes to ZERO diagnostics, so the filters guard
/// exactly what they claim to.
const QUALIFIED_MARKET: &str = "\
pub flow haggle(x: int) {
  You haggle at the market stall.
  -> DONE
}
";

/// `story.brink` — `use story::market::barter;` (module-qualified import)
/// then the module-qualified divert the maintainer confirmed is the
/// intended spelling.
const QUALIFIED_MAIN_ACCEPTED: &str = "\
use story::market::barter;

flow start() {
  -> barter::haggle(1)
}
";

/// Same import, but the *bare* divert spelling — must stay rejected: a
/// module-qualified import licenses `barter::haggle`, never bare `haggle`
/// (issue #2287's bug (b), the more dangerous of the two).
const QUALIFIED_MAIN_BARE_REJECTED: &str = "\
use story::market::barter;

flow start() {
  -> haggle(1)
}
";

const QUALIFIED_MARKET_FILE: FileId = FileId(10);
const QUALIFIED_MAIN_FILE: FileId = FileId(11);

fn qualified_module_map() -> ModuleMap {
    [
        (QUALIFIED_MARKET_FILE, "story::market::barter"),
        (QUALIFIED_MAIN_FILE, "story::story"),
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

fn analyze_qualified_project(main_src: &str) -> brink_analyzer::AnalysisResult {
    let market = lower_brink(QUALIFIED_MARKET_FILE, QUALIFIED_MARKET);
    let main = lower_brink(QUALIFIED_MAIN_FILE, main_src);
    let inputs: Vec<(FileId, &HirFile, &SymbolManifest)> = vec![
        (QUALIFIED_MARKET_FILE, &market.0, &market.1),
        (QUALIFIED_MAIN_FILE, &main.0, &main.1),
    ];
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_analyzer::analyze_with_modules(&inputs, &qualified_module_map(), &opts, true)
}

/// Bug (a): `use story::market::barter;` must license the module-qualified
/// `-> barter::haggle` divert, end to end — it must resolve, and it must
/// not raise `E024` (unresolved), `E025` (import-required), or `E087`
/// (private-across-modules).
#[test]
fn qualified_use_licenses_the_module_qualified_divert() {
    let result = analyze_qualified_project(QUALIFIED_MAIN_ACCEPTED);
    let offenders: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                brink_ir::DiagnosticCode::E024
                    | brink_ir::DiagnosticCode::E025
                    | brink_ir::DiagnosticCode::E087
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "`use story::market::barter;` must license `-> barter::haggle` \
         with no unresolved/import-required/private diagnostic: {offenders:?}"
    );
}

/// Bug (b): the same import must NOT license the bare `-> haggle` spelling
/// — it must stay unresolved (`E024`), not silently accepted. This is the
/// dangerous over-permissive defect issue #2287 reported: reverting either
/// `lookup_divert`'s `lookup_knot_bare` step or `classify_bare` back to the
/// old flat `lookup_by_name` makes this fail (the bare name would resolve
/// and no diagnostic would fire at all).
#[test]
fn qualified_use_does_not_license_the_bare_divert() {
    let result = analyze_qualified_project(QUALIFIED_MAIN_BARE_REJECTED);
    let e024s: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == brink_ir::DiagnosticCode::E024)
        .collect();
    assert_eq!(
        e024s.len(),
        1,
        "a module-qualified-only import must leave bare `-> haggle` \
         unresolved, exactly one E024: {:?}",
        result.diagnostics
    );
}

/// Issue #2287's "verify before scoping" directive, row 4 of the corrected
/// table: `use story::market::barter::*;` (a glob import). This is not a
/// resolution question at all — the native grammar has no glob-`use`
/// production to lower in the first place. `parser::decl::use_tree` only
/// recognizes an `IDENT` segment, a `{ … }` group, or a trailing `as`
/// alias after each `::`; a bare `*` is none of those, so parsing produces
/// an error rather than a `USE_DECL` with any glob shape. Pinned here so
/// this finding survives as a structural fact, not just a PR-body claim:
/// if a future change teaches the grammar a real glob production, this
/// test's failure is the signal to add the accepting counterpart.
#[test]
fn glob_use_is_not_reachable_in_the_native_grammar() {
    let src = "use story::market::barter::*;\n\nflow start() {\n  -> haggle\n}\n";
    let parsed = brink_syntax_native::parse(src);
    assert!(
        !parsed.errors().is_empty(),
        "a glob `use ...::*;` must fail to parse cleanly — if this starts \
         passing, the native grammar has gained glob-`use` support and \
         issue #2287's row 4 needs a real resolution test, not this pin"
    );
}

// ─── Issue #2298: the remainder #2287/#2296 deliberately left ───────
//
// Item 1, the live gap: `resolve_function`'s "try knots" step (ink allows
// a knot as a function via tunnels) is bug (b)'s call-site twin — a bare
// `haggle()` after only a module-qualified import must be rejected
// exactly like the bare divert `-> haggle` already is. The fixture below
// also probes #2948's composition concern (decl-initializer visitation):
// the call sits inside a decl-default lambda literal (`|x| haggle(x)`
// as a top-level `const`'s value), the shape #2948 taught other analyzer
// passes to walk — `project_manifest`'s own `Expr::Lambda` handling
// already reaches inside one unconditionally (see `symbols::project`'s
// `walk_expr`/`walk_lambda`), so this fixture is the end-to-end proof
// that the reference actually gets recorded and resolved there too, not
// just a unit-tested claim about the walker.

/// `story.brink` — module-qualified import only, then a bare tunnel-
/// function call to `haggle` nested inside a decl-default lambda.
const QUALIFIED_MAIN_BARE_CALL_REJECTED: &str = "\
use story::market::barter;

const probe = |x| haggle(x)

flow start() {
  -> DONE
}
";

/// The same call site, but with a genuine symbol-level bare import — must
/// resolve cleanly, the should-not-fire control for the rejection above.
const QUALIFIED_MAIN_BARE_CALL_ACCEPTED: &str = "\
use story::market::barter::haggle;

const probe = |x| haggle(x)

flow start() {
  -> DONE
}
";

/// Item 1's RED case, end to end: before this fix, `resolve_function`'s
/// flat `lookup_by_name`/`classify` let the dual-reading phantom
/// `story::market::barter` qualified-module entry license the bare call,
/// reproducing #2287 bug (b) for a call instead of a divert. Item 3: the
/// resulting `E025` must name the qualified-import-only candidate it
/// skipped, mirroring `modules::check`'s own E025 "import it from"
/// framing.
#[test]
fn qualified_use_does_not_license_the_bare_call() {
    let result = analyze_qualified_project(QUALIFIED_MAIN_BARE_CALL_REJECTED);
    let e025s: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == brink_ir::DiagnosticCode::E025)
        .collect();
    assert_eq!(
        e025s.len(),
        1,
        "a module-qualified-only import must leave the bare call `haggle()` \
         unresolved, exactly one E025: {:?}",
        result.diagnostics
    );
    assert!(
        e025s[0]
            .message
            .contains("import it from `story::market::barter`"),
        "the E025 for a module-imported-but-bare call must name the \
         qualified-import-only candidate it skipped: {}",
        e025s[0].message
    );
}

/// The should-not-fire control: a genuine symbol-level bare import must
/// still license the bare call, inside the same decl-default-lambda
/// nesting — the exclusion must not overcorrect into rejecting a
/// legitimately bare-imported knot-as-tunnel-function.
#[test]
fn symbol_level_use_licenses_the_bare_call() {
    let result = analyze_qualified_project(QUALIFIED_MAIN_BARE_CALL_ACCEPTED);
    let offenders: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                brink_ir::DiagnosticCode::E024
                    | brink_ir::DiagnosticCode::E025
                    | brink_ir::DiagnosticCode::E087
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "`use story::market::barter::haggle;` must license the bare call \
         `haggle(x)` inside the decl-default lambda: {offenders:?}"
    );
}
