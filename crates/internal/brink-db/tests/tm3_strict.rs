//! TM-3 (#619) salsa wiring: `types = strict` diagnostics must be reachable
//! through the production `diagnostics_query` path (`db.diagnostics(file)`)
//! — the same seam CLI/LSP/IDE consumers already read — and must reuse the
//! already-memoized, FG-narrowed `type_inference` query rather than forcing
//! `finish_analysis` to recompute inference from scratch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn strict_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

/// The production diagnostics seam (`db.diagnostics(file)`, backed by
/// `diagnostics_query` -> `analysis_query` -> `finish_analysis`) surfaces
/// TM-3's Unknown-escape error for an unannotated, unused param under
/// strict — the exact path CLI's `compile_path_with_options` and the LSP's
/// `Driver::diagnostics` both read.
#[test]
fn strict_unknown_escape_reaches_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E065),
        "{diags:?}"
    );
}

/// `types = strict` + `dialect = strict-ink` is a config error (`E064`),
/// reachable the same way.
#[test]
fn strict_with_strict_ink_dialect_reaches_production_diagnostics_as_config_error() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::StrictInk,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    });

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E064),
        "{diags:?}"
    );
}

/// Explicit gradual never reaches any TM-3 diagnostic through the same
/// production seam, regardless of dialect. (NS-A9: gradual is no longer
/// the Brink-dialect default — it must be opted into; the unset-`types`
/// default is dialect-keyed, covered by the companion test below.)
#[test]
fn explicit_gradual_reaches_no_strict_diagnostics_through_production_path() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        ..AnalysisOptions::default()
    });

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| matches!(
            d.code,
            DiagnosticCode::E064 | DiagnosticCode::E065 | DiagnosticCode::E066
        )),
        "{diags:?}"
    );
}

/// NS-A9: the Brink dialect with `types` unset resolves to strict — the
/// Unknown-escape reaches the production seam with no explicit `types`
/// setting at all.
#[test]
fn brink_dialect_unset_types_defaults_strict_through_production_path() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    });

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E065),
        "{diags:?}"
    );
}

/// `analysis_query`'s strict wiring reuses `type_inference_query` (FG-2/
/// FG-2.1's per-SCC-memoized whole-project inference) rather than calling
/// `finish_analysis`'s own internal `infer_project` fallback — reading
/// `db.type_inference()` after a strict analyze must show the def actually
/// got solved (a non-`Unknown` signature is present for the SCC touched),
/// proving the salsa-memoized inference path was exercised, not skipped.
#[test]
fn strict_analyze_populates_the_memoized_whole_project_inference() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n".to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts());

    // Force strict analysis to run (reads `type_inference_query` internally).
    let _ = db.analysis();

    let index = db.symbol_index();
    let heal_id = *index
        .by_name
        .get("heal")
        .and_then(|ids| ids.first())
        .expect("heal is indexed");
    let inference = db.type_inference();
    let sig = inference
        .signatures
        .get(&heal_id)
        .expect("heal has an inferred signature");
    assert_eq!(
        sig.params,
        vec![brink_analyzer::Ty::Int],
        "heal's param is inferred int from `hp + 1`"
    );
}

/// T1c follow-up (issue #712), through the *real* incremental salsa path:
/// `db.diagnostics(file)` -> `analysis_query` -> `strict::check`, backed by
/// `type_inference_query` -> `solve_scc_query`'s FG-2.1-narrowed globals map
/// (`brink-db/src/queries.rs`) — a separate implementation from the pure-
/// function `brink_analyzer::infer_project`'s `collect_globals`, which
/// `brink-analyzer`'s own unit tests exercise directly. A global `VAR`
/// holding a `#fn(...)` value, called with a wrong-typed argument, must
/// surface `E063` here too — proving the narrowed globals map picked up the
/// same `Sig::value_ty` fix (`Sig::fn_type` before issue #1540 generalized
/// it), not just the whole-project one.
#[test]
fn strict_global_fn_value_mismatch_reaches_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "=== function heal(ref hp: int, amount: int): int ===\n\
         ~ hp = hp + amount\n~ return hp\n\
         VAR player_hp = 10\n\
         VAR heal_player = #fn(heal, player_hp)\n\
         === main ===\n~ temp r: int = heal_player(\"x\")\n-> DONE\n"
            .to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E063 && d.message.contains("argument 1")),
        "{diags:?}"
    );
}
