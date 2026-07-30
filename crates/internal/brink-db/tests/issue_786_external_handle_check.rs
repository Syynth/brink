//! Issue #786 (docs/t1d-spec.md §3): `EXTERNAL` call-site argument checking
//! against a manifest-declared `Handle<K>` param, exercised through the
//! *production* `db.diagnostics(file)` seam (`diagnostics_query` ->
//! `analysis_query` -> `finish_analysis`, which reads `solve_scc_query`),
//! not just the pure `crate::infer_project` + `crate::strict::check` path
//! `brink-analyzer`'s own `infer/mod.rs` and `strict.rs` tests already
//! cover. Mirrors `t1d_2b_handle_kind_inference.rs`'s production-seam
//! pattern for the sibling T1d-2b slice — the narrowed/projected FG-2
//! substrate (range-zeroed `inference_index`, per-file-narrowed resolutions,
//! per-def body projection) is exactly what can diverge from the pure path,
//! so `collect_external_sigs`'s seeding into `solve_scc`'s `known_sigs`
//! must be exercised end-to-end through `solve_scc_query`, not assumed from
//! the pure-path tests alone.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::{
    BaseType, DiagnosticCode, ExternalKind, HostManifest, ManifestExternal, ManifestParam,
    SemanticTypeDef, TypeRef,
};

/// `play_sound` is a manifest-registered `EXTERNAL` declared to take
/// `Handle<AudioInstance>`. `get_timer`'s return is annotated
/// `Handle<Timer>`, so `t`'s declared kind flows into `main` purely through
/// call-site inference — never an annotation of its own — then gets passed
/// as `play_sound`'s argument, a cross-kind mismatch detectable only once
/// `collect_external_sigs`'s declaration-derived signature is seeded into
/// `known_sigs` ahead of `main`'s SCC solve.
const CROSS_KIND_SRC: &str = "\
EXTERNAL play_sound(inst)\n\
=== function get_timer(id: int): Handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp t = get_timer(1)\n~ play_sound(t)\n-> DONE\n";

/// Same shape, but the argument is already the binding's own declared kind
/// (`AudioInstance`) — must unify cleanly, no escape.
const SAME_KIND_SRC: &str = "\
EXTERNAL play_sound(inst)\n\
=== function get_audio(id: int): Handle<AudioInstance> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ play_sound(a)\n-> DONE\n";

fn two_kind_manifest_with_play_sound() -> HostManifest {
    HostManifest {
        markup: Vec::new(),
        types: vec![
            SemanticTypeDef {
                name: "AudioInstance".to_string(),
                base: BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            },
            SemanticTypeDef {
                name: "Timer".to_string(),
                base: BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            },
        ],
        externals: vec![ManifestExternal {
            name: "play_sound".to_string(),
            params: vec![ManifestParam {
                name: "inst".to_string(),
                ty: TypeRef("AudioInstance".to_string()),
            }],
            returns: TypeRef::default(),
            kind: ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        }],
    }
}

fn strict_opts(manifest: HostManifest) -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        host_manifest: Some(manifest),
        ..AnalysisOptions::default()
    }
}

/// Positive case, through the real salsa pipeline: a `Handle<Timer>`
/// argument to a binding whose manifest entry declares `Handle<AudioInstance>`
/// must escape with `E066` for the caller's `temp t`, reached via
/// `solve_scc_query` (not the pure `infer_project` fallback —
/// `db.diagnostics` always goes through `analysis_query`'s FG-narrowed
/// `type_inference_query`).
#[test]
fn external_call_cross_kind_handle_rejection_reaches_production_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(two_kind_manifest_with_play_sound()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `t`")),
        "{diags:?}"
    );
}

/// Negative case: the argument's own declared kind matches the binding's
/// declared param kind — unifies cleanly, no escape, through the same
/// production seam.
#[test]
fn external_call_same_kind_handle_argument_reaches_no_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", SAME_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(two_kind_manifest_with_play_sound()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.code, DiagnosticCode::E065 | DiagnosticCode::E066)),
        "same-kind binding argument must not escape: {diags:?}"
    );
}

/// Gradual (the default) never reaches any TM-3 escape diagnostic through
/// the same production seam, regardless of the registered manifest — proves
/// the `EXTERNAL` call-site checking `collect_external_sigs` wires in is
/// strict-mode-gated, not a behavior change to gradual mode.
#[test]
fn external_call_cross_kind_handle_mismatch_stays_advisory_only_under_gradual() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        host_manifest: Some(two_kind_manifest_with_play_sound()),
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
