//! T1d-2b (issue #774, docs/t1d-spec.md §3): threading the registered
//! `HostManifest`'s handle-kind vocabulary through the FG-2 salsa substrate
//! into inference (`solve_scc_query`/`signature_query`, and their
//! `brink_analyzer` counterparts `solve_scc`/`signature`) — making strict
//! `Handle<K>` kind-rejection reachable end-to-end through the *production*
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
use brink_ir::{
    BaseType, DiagnosticCode, HostManifest, ManifestExternal, SemanticTypeDef, TypeRef,
};

/// `get_audio`/`get_timer` are leaf functions whose return type is
/// annotated with a distinct handle kind each. `spawn_audio`/`spawn_timer`
/// are genuinely-registered `EXTERNAL` producers (issue #1942's Scope
/// section proposes "a natively-registered producer" as one construction
/// path) — each declares a fixed `returns` naming its own `Handle`-based
/// `SemanticTypeDef`, so `collect_external_sigs` resolves the *body's own*
/// return expression to the concrete `Ty::Handle(K)` directly; the
/// annotation only has to confirm it, never manufacture it from `Unknown`.
/// `main`'s temps `a`/`b` pick those types up purely through call-site
/// inference — never an annotation of their own — then get compared, a
/// genuine cross-kind handle mismatch detectable only from body-usage
/// inference.
///
/// A registered producer replaces a plain `~ return id` (issue #1912): a
/// handle is an opaque `{kind, id}` scalar (docs/t1d-spec.md §1), not an
/// `int`, so handing an `int`-annotated param back out of a
/// `Handle<K>`-returning function was a real type error that only passed
/// while reading an annotated param as a value typed `Unknown`. It also
/// replaces the earlier *unregistered* `opaque_handle` workaround (PR
/// #1938): that made the return type-check pass only because an
/// unregistered external's result is untyped, papered over by the
/// annotation-firewall overlay — scaffolding issue #1942 asked not to let
/// calcify. `spawn_audio`/`spawn_timer` are real, checked signatures
/// instead.
const CROSS_KIND_SRC: &str = "EXTERNAL spawn_audio()\nEXTERNAL spawn_timer()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== function get_timer(): Handle<Timer> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp a = get_audio()\n~ temp b = get_timer()\n{a == b:\n  ok\n}\n-> DONE\n";

const SAME_KIND_SRC: &str = "EXTERNAL spawn_audio()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== function get_audio2(): Handle<AudioInstance> ===\n~ return spawn_audio()\n\
=== main ===\n~ temp a = get_audio()\n~ temp c = get_audio2()\n{a == c:\n  ok\n}\n-> DONE\n";

/// A no-arg, manifest-registered producer whose declared `returns` names a
/// `Handle`-based semantic type — the genuine construction path issue
/// #1942's Scope section calls out ("a natively-registered producer ... that
/// legitimately returns `Handle<K>`"), not a language literal (T1d spec §7
/// rules out handle literal syntax entirely: "no literal syntax exists;
/// only bindings mint them").
fn handle_producer(name: &str, kind: &str) -> ManifestExternal {
    ManifestExternal {
        name: name.to_string(),
        params: Vec::new(),
        returns: TypeRef(kind.to_string()),
        kind: brink_ir::ExternalKind::default(),
        doc: None,
        widgets: Vec::new(),
        path: Vec::new(),
    }
}

fn two_kind_manifest() -> HostManifest {
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
        externals: vec![
            handle_producer("spawn_audio", "AudioInstance"),
            handle_producer("spawn_timer", "Timer"),
        ],
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
/// Handle<AudioInstance> rejects Handle<Timer> at compile time` (the #767
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

/// This PR's body describes a manual, uncommitted experiment (temporarily
/// mis-registering `spawn_timer`'s declared return) to argue the new
/// registered-producer mechanism is genuinely load-bearing rather than
/// decorative — a review finding pointed out that every assertion in this
/// file's other fixtures passes identically with the `externals` vectors
/// deleted entirely, because with no manifest entry
/// `collect_external_sigs` skips the external
/// (`brink-analyzer/src/infer/mod.rs`), the body-derived return stays
/// `Ty::Unknown`, and the annotation-firewall overlay re-supplies
/// `Handle<K>` exactly as before (the pre-#1942 `opaque_handle` behavior).
/// This test commits that experiment as an automated regression instead:
/// `spawn_timer` is registered with a genuinely *wrong* declared `returns`
/// (`"Timer"`) for a function annotated `Handle<AudioInstance>`. It can only
/// go green when the producer's declared return is genuinely consulted,
/// because `annotations::report_if_mismatched`
/// (`crates/internal/brink-analyzer/src/annotations.rs:534`) returns early
/// whenever `body_ty.is_unresolved()` — i.e. it stays silent whenever the
/// type came from the annotation overlay rather than the body. Drop the
/// registration (or the `externals` entry entirely) and this guard goes
/// red, which is the property #1942 actually asked for.
#[test]
fn mis_kinded_registered_producer_is_caught_through_production_diagnostics_under_strict() {
    let src = "EXTERNAL spawn_timer()\n\
=== function get_audio(): Handle<AudioInstance> ===\n~ return spawn_timer()\n\
=== main ===\n~ temp a = get_audio()\n-> DONE\n";

    let mut manifest = two_kind_manifest();
    // Only register the mis-kinded producer under test: `spawn_timer`
    // genuinely declares `returns: Timer`, called from a function annotated
    // `Handle<AudioInstance>`.
    manifest.externals = vec![handle_producer("spawn_timer", "Timer")];

    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", src.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E063),
        "{diags:?}"
    );
}
