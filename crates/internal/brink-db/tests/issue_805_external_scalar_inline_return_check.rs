//! Issue #805 (PR #794 / issue #786 lineage, docs/t1d-spec.md §3): extends
//! `EXTERNAL` call-site checking to (1) scalar semantic types from the
//! manifest vocabulary (not just `handle<K>` kinds), (2) inline-doc-only
//! externals (no matching `ManifestExternal` entry), and (3) return-position
//! kind checking. Exercised through the *production* `db.diagnostics(file)`
//! seam (`diagnostics_query` -> `analysis_query` -> `whole_project_diagnostics_query`,
//! which reads `solve_scc_query`), not just the pure `crate::infer_project` +
//! `crate::strict::check` path `brink-analyzer`'s own `infer/mod.rs` tests
//! already cover — the wave-10 lesson: the narrowed/projected FG-2 substrate
//! (range-zeroed `inference_index`, per-file-narrowed resolutions, per-def
//! body projection) is exactly what can diverge from the pure path, and for
//! #805 specifically `solve_scc_query` must also read the new
//! `inline_docs_query` dependency `collect_external_sigs` now needs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_ir::{
    BaseType, DiagnosticCode, ExternalKind, HostManifest, ManifestExternal, ManifestParam,
    SemanticTypeDef, TypeRef,
};

fn strict_opts(manifest: HostManifest) -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        host_manifest: Some(manifest),
        ..AnalysisOptions::default()
    }
}

// ─── (1) Scalar semantic types ──────────────────────────────────────────

fn manifest_with_scalar_type_and_toggle() -> HostManifest {
    HostManifest {
        markup: Vec::new(),
        types: vec![SemanticTypeDef {
            name: "switch_id".to_string(),
            base: BaseType::Int,
            constraint: None,
            values: None,
            widget: None,
        }],
        externals: vec![ManifestExternal {
            name: "toggle".to_string(),
            params: vec![ManifestParam {
                name: "id".to_string(),
                ty: TypeRef("switch_id".to_string()),
            }],
            returns: TypeRef::default(),
            kind: ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        }],
    }
}

/// `toggle` declares `id: switch_id` (a scalar semantic type, `base: int`,
/// not a `handle<K>` kind). A string-literal-derived local passed as the
/// argument is a genuine `int` vs `string` conflict, reachable only once
/// the binding's declared *scalar* type is seeded into `known_sigs`.
#[test]
fn external_call_scalar_semantic_type_mismatch_reaches_production_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let src = "EXTERNAL toggle(id)\n\
               === main ===\n~ temp s = \"harbor\"\n~ toggle(s)\n-> DONE\n";
    let file = db.set_file("main.ink", src.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest_with_scalar_type_and_toggle()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `s`")),
        "{diags:?}"
    );
}

/// Negative case: an int-derived local matches `toggle`'s declared
/// `switch_id` (base int) cleanly — no escape.
#[test]
fn external_call_scalar_semantic_type_match_reaches_no_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let src = "EXTERNAL toggle(id)\n\
               === main ===\n~ temp s = 5\n~ toggle(s)\n-> DONE\n";
    let file = db.set_file("main.ink", src.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest_with_scalar_type_and_toggle()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.code, DiagnosticCode::E065 | DiagnosticCode::E066)),
        "scalar-type-matching argument must not escape: {diags:?}"
    );
}

// ─── (2) Inline-doc-only externals ──────────────────────────────────────

/// The registered manifest declares only the handle-kind *vocabulary*
/// (`types`) — no `ManifestExternal` entry for `play_sound` at all.
/// `play_sound`'s only declared signature source is its own inline `///
/// @param` doc comment.
fn manifest_with_only_handle_vocabulary() -> HostManifest {
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
        externals: Vec::new(),
    }
}

const INLINE_ONLY_CROSS_KIND_SRC: &str = "\
/// @param inst {AudioInstance}
EXTERNAL play_sound(inst)
=== function get_timer(id: int): handle<Timer> ===
~ return id
=== main ===
~ temp t = get_timer(1)
~ play_sound(t)
-> DONE
";

/// `play_sound` is documented purely via an inline `///` doc comment — no
/// matching `ManifestExternal` entry anywhere in the registered manifest.
/// A cross-kind (`Timer`) argument to its inline-declared `AudioInstance`
/// param must still escape with `E066`, reached via `solve_scc_query`'s new
/// `inline_docs_query` dependency.
#[test]
fn inline_only_external_cross_kind_argument_reaches_production_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", INLINE_ONLY_CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest_with_only_handle_vocabulary()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `t`")),
        "{diags:?}"
    );
}

const INLINE_ONLY_SAME_KIND_SRC: &str = "\
/// @param inst {AudioInstance}
EXTERNAL play_sound(inst)
=== function get_audio(id: int): handle<AudioInstance> ===
~ return id
=== main ===
~ temp a = get_audio(1)
~ play_sound(a)
-> DONE
";

/// Negative case: the argument's own declared kind matches the inline
/// doc's declared param kind — unifies cleanly, no escape.
#[test]
fn inline_only_external_same_kind_argument_reaches_no_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", INLINE_ONLY_SAME_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest_with_only_handle_vocabulary()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.code, DiagnosticCode::E065 | DiagnosticCode::E066)),
        "same-kind inline-doc-declared argument must not escape: {diags:?}"
    );
}

// ─── (3) Return-position kind checking ──────────────────────────────────

fn manifest_with_play_sound_and_spawn_timer() -> HostManifest {
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
            ManifestExternal {
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
            },
            ManifestExternal {
                name: "spawn_timer".to_string(),
                params: Vec::new(),
                returns: TypeRef("Timer".to_string()),
                kind: ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            },
        ],
    }
}

/// `spawn_timer` is a manifest-registered `EXTERNAL` with no params and a
/// *declared return type* of `handle<Timer>`. Its return value, assigned
/// straight into a temp then passed to `play_sound` (declared
/// `handle<AudioInstance>`), is a cross-kind mismatch detectable only once
/// an `EXTERNAL`'s own declared *return* kind — not just its params — is
/// seeded into `known_sigs`.
const RETURN_POSITION_CROSS_KIND_SRC: &str = "\
EXTERNAL play_sound(inst)
EXTERNAL spawn_timer()
=== main ===
~ temp x = spawn_timer()
~ play_sound(x)
-> DONE
";

#[test]
fn external_return_position_cross_kind_reaches_production_diagnostics_under_strict() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", RETURN_POSITION_CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(strict_opts(manifest_with_play_sound_and_spawn_timer()));

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E066 && d.message.contains("temp `x`")),
        "{diags:?}"
    );
}

/// Gradual (the default) never reaches any TM-3 escape diagnostic through
/// the same production seam, regardless of the registered manifest — proves
/// all three #805 widenings are strict-mode-gated, not a behavior change to
/// gradual mode (mirrors PR #794's own gradual-mode negative).
#[test]
fn external_call_checking_widenings_stay_advisory_only_under_gradual() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.ink", RETURN_POSITION_CROSS_KIND_SRC.to_owned());
    db.set_entry("main.ink");
    db.set_analysis_options(AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        host_manifest: Some(manifest_with_play_sound_and_spawn_timer()),
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
