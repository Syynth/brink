//! T1d-2b (issue #774, docs/t1d-spec.md §3): threading the registered
//! `HostManifest`'s handle-kind vocabulary through the FG-2 salsa substrate
//! into inference (`solve_scc_query`/`signature_query`, and their
//! `brink_analyzer` counterparts `solve_scc`/`signature`) — making strict
//! `handle<K>` kind-rejection reachable end-to-end through the *production*
//! `db.diagnostics(file)` seam (`diagnostics_query` -> `analysis_query` ->
//! `finish_analysis`), the same path CLI/LSP/IDE consumers read. PR #769
//! (T1d-2) landed the manifest-vs-annotation resolution and the
//! annotation-firewall exemption but explicitly disclosed this gap as
//! deferred: "a genuine cross-kind handle mismatch detected purely from
//! body-usage inference … is not yet caught". This file is the #767
//! acceptance-criterion regression guard for that gap, mirroring
//! `tm3_strict.rs`'s existing production-seam pattern.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::{BaseType, DiagnosticCode, HostManifest, SemanticTypeDef};

/// `get_audio`/`get_timer` are leaf functions whose return type is
/// annotated with a distinct handle kind each; their body-derived return
/// stays `Unknown` (`id` is never otherwise constrained), so the annotation
/// firewall overlay supplies the concrete `Ty::Handle(K)`. `main`'s temps
/// `a`/`b` pick those types up purely through call-site inference — never an
/// annotation of their own — then get compared, a genuine cross-kind handle
/// mismatch detectable only from body-usage inference.
const CROSS_KIND_SRC: &str = "\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== function get_timer(id: int): handle<Timer> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ temp b = get_timer(1)\n{a == b:\n  ok\n}\n-> DONE\n";

const SAME_KIND_SRC: &str = "\
=== function get_audio(id: int): handle<AudioInstance> ===\n~ return id\n\
=== function get_audio2(id: int): handle<AudioInstance> ===\n~ return id\n\
=== main ===\n~ temp a = get_audio(1)\n~ temp c = get_audio2(1)\n{a == c:\n  ok\n}\n-> DONE\n";

fn two_kind_manifest() -> HostManifest {
    HostManifest {
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
        ..Default::default()
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

/// Positive case, through the real salsa pipeline: `binding declared
/// handle<AudioInstance> rejects handle<Timer> at compile time` (the #767
/// acceptance criterion) — `db.diagnostics(file)` must carry `E066` for the
/// cross-kind comparison, reached via `solve_scc_query` (not the pure
/// `infer_project` fallback — `db.diagnostics` always goes through
/// `analysis_query`'s FG-narrowed `type_inference_query`).
#[test]
fn cross_kind_handle_rejection_reaches_production_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(two_kind_manifest()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `a`")),
        "{diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `b`")),
        "{diags:?}"
    );
}

/// Negative case: two locals of the *same* declared handle kind compared
/// against each other unify cleanly — no escape, through the same
/// production seam.
#[test]
fn same_kind_handle_comparison_reaches_no_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", SAME_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(two_kind_manifest()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.code, DiagnosticCode::E065 | DiagnosticCode::E066)),
        "same-kind comparison must not escape: {diags:?}"
    );
}

/// Gradual (the default) never reaches any TM-3 escape diagnostic through
/// the same production seam, regardless of the registered manifest —
/// TM-1 inference stays advisory-only under gradual, byte-identical
/// forever. Proves T1d-2b's manifest threading is strict-mode-gated, not a
/// behavior change to gradual mode.
#[test]
fn cross_kind_handle_mismatch_stays_advisory_only_under_gradual() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        host_manifest: Some(two_kind_manifest()),
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

/// The salsa-memoized path (`solve_scc_query`) must actually get exercised,
/// not skipped — `db.type_inference()` after a strict analyze shows
/// `get_audio`/`get_timer`'s distinct `Ty::Handle` return kinds, proving the
/// manifest reached the per-SCC solve (not just `signature()`/the
/// annotation-firewall exemption PR #769 already wired).
#[test]
fn strict_analyze_populates_memoized_inference_with_distinct_handle_kinds() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(two_kind_manifest()));

    // Force strict analysis to run (reads `type_inference_query` internally).
    let _ = db.analysis();

    let index = db.symbol_index();
    let get_audio_id = *index
        .by_name
        .get("get_audio")
        .and_then(|ids| ids.first())
        .expect("get_audio is indexed");
    let get_timer_id = *index
        .by_name
        .get("get_timer")
        .and_then(|ids| ids.first())
        .expect("get_timer is indexed");

    let inference = db.type_inference();
    assert_eq!(
        inference
            .signatures
            .get(&get_audio_id)
            .expect("get_audio has an inferred signature")
            .return_ty,
        brink_analyzer::Ty::Handle("AudioInstance".to_string())
    );
    assert_eq!(
        inference
            .signatures
            .get(&get_timer_id)
            .expect("get_timer has an inferred signature")
            .return_ty,
        brink_analyzer::Ty::Handle("Timer".to_string())
    );
}
