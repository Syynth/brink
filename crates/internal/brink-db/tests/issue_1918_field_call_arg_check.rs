//! Issue #1918: a UFCS call through a struct's own fn-typed field
//! (`UfcsVerdict::FieldCall`) must be argument-checked exactly as strictly
//! through the *production*, db-backed `db.diagnostics(file)` seam as
//! through the pure `whole_project_diagnostics`/`brink_analyzer::strict_diagnostics`
//! path (`crates/internal/brink-analyzer/tests/ufcs_resolution.rs`).
//!
//! Unlike issue #1921's own db-direct regression (`EXTERNAL` signatures
//! dropped by `solve_scc_query`'s batch filter — a genuine db-specific
//! divergence), this fix lives entirely inside `brink-analyzer::ufcs`'s
//! `strict_verdict_diagnostics` — the one function both `ProjectDb`'s
//! `diagnostics_query` (via `analysis_query` -> `whole_project_diagnostics`
//! -> `strict::check` -> `ufcs::check_strict`) and the off-db
//! `IdeSnapshot::analyze` (via `analyze_with_modules`, which calls the same
//! shared `whole_project_diagnostics`) reach identically. There is no
//! db-specific code to test beyond proving the production seam actually
//! reaches it — the same "fixed at the brink-analyzer layer, so both roads
//! share the fix by construction" posture
//! `issue_2083_fn_valued_const_global_call_site.rs` documents for its own
//! fix.
//!
//! UFCS is native-only (a multi-segment `Expr::Call` path can only
//! originate in the native frontend — see `ufcs`'s own module doc), so
//! every fixture here is a `.brink` file under `types = strict` (the native
//! surface's B0.9 strict-only ruling) — mirrors
//! `issue_1921_ufcs_external_arg_check.rs`'s own setup.
//!
//! Every fixture here also carries an unrelated `E071` ("mistyped field")
//! on the struct literal's own `greet: "hi"` initializer: the native
//! surface has no first-class function-value literal yet (`#fn(target,
//! args…)` is `brink-syntax`-only, T1c §2), so a real callable value can
//! never be assigned to a fn-typed field on this surface — see
//! `crates/internal/brink-test-harness/tests/b3a_ufcs_e2e.rs`'s own module
//! doc, which tracks this as a follow-up on #1505. Assertions below check
//! `E063` specifically, matching
//! `brink-analyzer/tests/ufcs_resolution.rs`'s own `FieldCall` fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn strict_native_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

/// `Guest` declares `greet: fn(int): int`; the call site under test is
/// substituted into `CALL_EXPR`.
fn field_call_src(call_expr: &str) -> String {
    format!(
        "\
struct Guest {{
  greet: fn(int): int
}}

fn shout() {{
  let g = Guest {{ greet: \"hi\" }};
  let n = {call_expr};
}}

flow main() {{
  {{shout()}}
}}
"
    )
}

/// The regression itself: a `FieldCall`'s written argument disagreeing with
/// the field's declared `fn(int): int` param type must raise `E063`,
/// reached via the production `db.diagnostics` seam.
#[test]
fn field_call_with_mistyped_argument_raises_e063_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", field_call_src("g.greet(\"nope\")"));
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "a field call's mistyped argument must be checked through the db-backed path: {diags:?}"
    );
}

/// The arity sibling: `g.greet()` supplies zero arguments against the
/// field's one declared parameter.
#[test]
fn field_call_with_wrong_arity_raises_e063_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", field_call_src("g.greet()"));
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "a field call's wrong arity must be checked through the db-backed path: {diags:?}"
    );
}

/// Negative case: a correctly-typed, correct-arity field call must not
/// raise `E063` through the same production seam.
#[test]
fn field_call_with_matching_argument_raises_no_e063_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", field_call_src("g.greet(3)"));
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "a matching-type, matching-arity field call must not escape: {diags:?}"
    );
}
