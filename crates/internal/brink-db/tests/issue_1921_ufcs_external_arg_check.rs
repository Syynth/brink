//! Issue #1921: a UFCS-shaped call into an `EXTERNAL`/`extern` target must be
//! argument-checked exactly as strictly through the *production*,
//! db-backed `db.diagnostics(file)` seam (`diagnostics_query` ->
//! `analysis_query` -> `type_inference_query` -> `solve_scc_query` ->
//! `brink_analyzer::solve_scc`) as it already was through the pure
//! `crate::infer_project` + `ufcs::resolve` path.
//!
//! `brink_analyzer::solve_scc`'s `known_sigs` is seeded with
//! `collect_external_sigs`'s declaration-derived signatures (issue #786),
//! but externals are never members of any SCC `batch` — before this fix,
//! the `signatures` map it *returned* was filtered to exactly `batch`'s own
//! members, so an external's seeded signature never survived past that one
//! call. `type_inference_query` aggregates every SCC's `SolvedScc::signatures`
//! into the project-wide `InferenceResult::signatures` `ufcs::
//! check_ufcs_arg_types` reads (`self.signatures.get(&target)`) — so on the
//! db-backed path that lookup always missed for an `EXTERNAL` target, and a
//! UFCS call's argument types went completely unchecked there, even though
//! the identical mismatch through the pure `infer_project` path (whose
//! `solve_batches` sibling returns `known_sigs` wholesale, no batch filter)
//! was already caught. This mirrors `issue_786_external_handle_check.rs`'s
//! own production-seam pattern for the direct-call sibling gap.
//!
//! UFCS is native-only (a multi-segment `Expr::Call` path can only
//! originate in the native frontend — see `ufcs`'s own module doc), so
//! every fixture here is a `.brink` file under `types = strict` (the native
//! surface's B0.9 strict-only ruling).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::{
    DiagnosticCode, ExternalKind, HostManifest, ManifestExternal, ManifestParam, TypeRef,
};

/// `set_volume` is a manifest-registered `EXTERNAL`/`extern` declared to
/// take a bare `int`. `get_name`'s inferred return type is `string` (from
/// its own `return "loud";`, no annotation needed), so `n`'s declared type
/// flows into `total` purely through call-site inference. The UFCS spelling
/// `n.set_volume()` desugars to `set_volume(n)` (D5, issue #1482): the
/// receiver counts as the external's one declared param (`level: int`), so
/// a `string` argument there is a genuine cross-type mismatch, detectable
/// only once `collect_external_sigs`'s declaration-derived signature has
/// actually reached `InferenceResult::signatures` for this `EXTERNAL`.
const MISMATCH_SRC: &str = "\
extern set_volume(level)

fn get_name() {
  return \"loud\";
}

fn total() {
  let n = get_name();
  n.set_volume();
}

flow main() {
  {total()}
}
";

/// Same shape, but the argument already matches the declared `int` param —
/// must unify cleanly, no escape.
const MATCH_SRC: &str = "\
extern set_volume(level)

fn get_level() {
  return 3;
}

fn total() {
  let n = get_level();
  n.set_volume();
}

flow main() {
  {total()}
}
";

fn set_volume_manifest() -> HostManifest {
    HostManifest {
        markup: Vec::new(),
        types: Vec::new(),
        externals: vec![ManifestExternal {
            name: "set_volume".to_string(),
            params: vec![ManifestParam {
                name: "level".to_string(),
                ty: TypeRef("int".to_string()),
            }],
            returns: TypeRef::default(),
            kind: ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        }],
    }
}

fn strict_native_opts(manifest: HostManifest) -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        host_manifest: Some(manifest),
        ..AnalysisOptions::default()
    }
}

/// The regression itself: a UFCS call into an `EXTERNAL` whose declared
/// param type disagrees with the receiver's inferred type must raise
/// `E063`, reached via the production `type_inference_query` ->
/// `solve_scc_query` seam (not the pure `infer_project` fallback —
/// `db.diagnostics` always goes through `analysis_query`'s FG-narrowed
/// `type_inference_query` under strict).
#[test]
fn ufcs_call_into_external_with_mismatched_arg_raises_e063_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", MISMATCH_SRC.to_owned());
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts(set_volume_manifest()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "a UFCS call's `string` receiver into `set_volume`'s declared `int` \
         param must be argument-checked through the db-backed path: {diags:?}"
    );
}

/// Negative case: the argument's own inferred type already matches the
/// external's declared param type — unifies cleanly, no escape, through
/// the same production seam.
#[test]
fn ufcs_call_into_external_with_matching_arg_raises_no_e063_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", MATCH_SRC.to_owned());
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts(set_volume_manifest()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "a matching-type argument must not escape: {diags:?}"
    );
}
