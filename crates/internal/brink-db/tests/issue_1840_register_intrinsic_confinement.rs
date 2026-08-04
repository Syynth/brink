//! Issue #1840 Q5 — the *legality* half of the 2026-08-02 ruling
//! (`docs/decision-log.md` "`register` is a comptime-only intrinsic;
//! calling it elsewhere is a diagnostic"): `register` is legal only inside
//! the project's configured conventions module's well-known `fn
//! conventions()`.
//!
//! Exercised end-to-end through `ProjectDb`'s salsa query layer — the same
//! `db.analysis()` path `brink compile`/`brink check` and `@brink-lang/web`
//! run, matching `issue_1844_conventions_module_fence.rs`'s own precedent
//! for why this is the right test level for a project-config-gated
//! diagnostic. This file deliberately mirrors that suite's shape so the two
//! sibling confinement checks (`E169` for *declaring* a claiming handler,
//! `E175` for *calling* `register`) read side by side — and deliberately
//! diverges on the "unconfigured project" cases, since `register`'s
//! legality is a language-level restriction, not a project-configuration-
//! dependent one (see `register_intrinsic_diagnostics`'s own module doc).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn opts_with_conventions(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        conventions: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

/// [`opts_with_conventions`], plus `dialect: Brink` — every effects-assertion
/// check (`effects_assertions_diagnostics_query` and its siblings) gates on
/// `AnalysisOptions.dialect == Brink` regardless of the file's own native
/// `.brink` syntax (`docs/effects-spec.md` §10's "Callers only run this
/// under `dialect = brink`" posture, mirrored by `t2_2_effects_assertions.
/// rs`'s `analyze_native` helper) — `opts_with_conventions` alone (as the
/// `E175`-only tests above use it) never triggers `E103`.
fn opts_with_conventions_and_brink_dialect(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..opts_with_conventions(pointer)
    }
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

/// The ruled shape from `docs/decision-log.md`'s 2026-08-02 entry, minus
/// the `use std::conventions::screenplay` import (no stdlib modules exist
/// in the tree yet — `docs/conventions-comptime-sizing.md` §5 item 2).
const CONVENTIONS_FN: &str = "fn scene(place: string) {\n  return place;\n}\n\
    fn conventions() {\n  register(scene);\n}\n";

#[test]
fn a_legal_register_call_in_the_configured_module_is_silent() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

#[test]
fn a_register_call_outside_fn_conventions_in_the_configured_module_is_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "fn scene(place: string) {\n  return place;\n}\n\
         fn setup() {\n  register(scene);\n}\n\
         fn conventions() {\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E175], "{diags:?}");
    assert!(diags[0].message.contains("register"), "{diags:?}");
}

/// The same `fn conventions() { register(scene); }` shape as the silent
/// case above, but declared in a file that is NOT the project's configured
/// conventions module — illegal regardless of the function it sits in.
#[test]
fn a_register_call_in_the_right_function_but_wrong_file_is_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E175], "{diags:?}");
}

/// Diverges from `E169`'s `unset_conventions_never_fires`: with no conventions
/// module configured at all, there is no possible legal placement for
/// `register`, so every call is still illegal — unlike declaring a claiming
/// handler (nothing being confined *to* yet), `register`'s legality is not
/// project-configuration-dependent.
#[test]
fn unset_conventions_still_flags_every_register_call() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "scenes/heading.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E175], "{diags:?}");
}

/// Diverges from `E169`'s `a_preset_name_pointer_never_fires`: a bare
/// preset name (`conventions = "screenplay"`) names no project file, so —
/// same reasoning as the unset case — every `register` call is still
/// illegal.
#[test]
fn a_preset_name_pointer_still_flags_every_register_call() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("screenplay"));
    db.set_file(
        "scenes/heading.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E175], "{diags:?}");
}

/// Diverges from `E169`'s `an_unresolvable_conventions_pointer_never_fires_
/// even_against_the_real_module`: a typo'd/moved `conventions` pointer means
/// there is no file the compiler can confirm is "the" conventions module,
/// so even the file the author most likely intended still gets flagged —
/// there is no module for a `register` call to be legally "inside".
#[test]
fn an_unresolvable_conventions_pointer_still_flags_the_real_module() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("typo.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E175], "{diags:?}");
}

/// A same-file `register`-named user function shadows the intrinsic, so
/// calling it is an ordinary, legal call anywhere — never `E175` — even
/// with a conventions module configured elsewhere.
#[test]
fn a_shadowing_register_function_is_never_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        "fn register(x: string) {\n  return x;\n}\n\
         fn setup() {\n  register(\"x\");\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

/// An unannotated param named `register` (native allows unannotated params —
/// `brink-syntax-native/src/parser/declaration.rs`'s `fn heal(hp)`) shadows
/// the intrinsic when used as a call target — `resolve_query`'s locals
/// lookup (`resolve.rs`'s "temps/params used as function names" branch)
/// resolves it before the T1b-intrinsic fallback is ever consulted, so this
/// must never raise `E175`.
#[test]
fn a_param_named_register_used_as_a_call_target_is_never_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        "fn apply(register) {\n  return register(1);\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

/// The same shadow, via a local `temp` instead of a param — same resolution
/// path, same expected silence.
#[test]
fn a_temp_named_register_used_as_a_call_target_is_never_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        "fn scene(place: string) {\n  return place;\n}\n\
         fn setup() {\n  let register = scene;\n  register(\"x\");\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

/// A project with no `register` calls anywhere never raises `E175`,
/// configured conventions module or not — the ordinary, unconfigured-
/// project case stays completely silent.
#[test]
fn a_project_with_no_register_calls_is_always_silent() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "fn conventions() {\n}\nflow main() {\n  hi\n}\n".to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

/// Full-pipeline reachability, not just diagnostics: a legally-placed
/// `register` call compiles all the way through `db.story_data()` — LIR
/// lowering and `brink-codegen-inkb` codegen included, the same query
/// `brink compile`/`brink check` pull — with no errors and a real
/// `StoryData` produced. Proves the interim `lower_t1b_stdlib_call`
/// `"register"` arm doesn't crash or ICE the real pipeline.
#[test]
fn a_legal_register_call_compiles_all_the_way_to_story_data() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CONVENTIONS_FN}flow main() {{\n  hi\n}}\n"),
    );
    db.set_entry("conventions.brink");
    let product = db.story_data().expect("entry is set").clone();
    assert!(product.errors.is_empty(), "{:?}", product.errors);
    assert!(product.story.is_some(), "expected a compiled story");
}

/// The mirror case: an illegally-placed `register` call reaches
/// `db.story_data()` as a real compile error (`E175`, `story: None`) —
/// the same gate `brink_compiler::compile_with_options` reads
/// (`Some(story) = product.story else return Err(...)`) — not merely a
/// diagnostic that a caller could ignore.
#[test]
fn an_illegal_register_call_fails_story_data_with_e175() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "fn scene(place: string) {\n  return place;\n}\n\
         fn setup() {\n  register(scene);\n}\n\
         fn conventions() {\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    db.set_entry("conventions.brink");
    let product = db.story_data().expect("entry is set").clone();
    assert!(product.story.is_none(), "expected compilation to fail");
    assert_eq!(codes(&product.errors), vec![DiagnosticCode::E175]);
}

// ─── Issue #1840 Q4 — the effect-row gap this pass closes ──────────────
//
// Before this fix, `register` had no arm in `brink_analyzer::infer::
// intrinsics`, so it was, in practice, a row-exempt intrinsic (an empty
// row) — the exact shape the Q4 ruling rejected in favor of the RNG-cell
// precedent. `register` now writes `DefinitionId::CONVENTIONS_REGISTRY_
// CELL`, unconditionally, on every call.

/// The ruled example's original (superseded) spelling —
/// `@[effects(pure)] fn conventions() { register(...) }` — now genuinely
/// fails its own `E103` fence, exactly as the Q4 ruling's analysis found,
/// instead of compiling clean the way it did before this pass.
#[test]
fn a_pure_conventions_fn_now_exceeds_on_the_registry_write() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions_and_brink_dialect("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "fn scene(place: string) {\n  return place;\n}\n\
         @[effects(pure)]\nfn conventions() {\n  register(scene);\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E103 && d.message.contains("conventions_registry")),
        "{diags:?}"
    );
    // The legal-placement check (E175) is a separate, unaffected pass — the
    // call sits inside `fn conventions()` in the configured module either
    // way.
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E175),
        "{diags:?}"
    );
}

/// The Q4-corrected spelling: declaring the write (rather than claiming
/// purity) satisfies the assertion — no `E103`.
#[test]
fn declaring_the_registry_write_satisfies_the_effects_assertion() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions_and_brink_dialect("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "fn scene(place: string) {\n  return place;\n}\n\
         @[effects(writes(conventions_registry))]\nfn conventions() {\n  register(scene);\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(diags.is_empty(), "{diags:?}");
}
