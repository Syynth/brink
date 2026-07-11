#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Tests for the query-shaped symbol-service seams (#500, substrate spec §4
//! layer 2): `symbol_index`, per-file `resolve`, and the `signature` stub.

use brink_analyzer::{Sig, signature};
use brink_format::DefinitionId;
use brink_ir::{FileId, HirFile, SymbolKind, SymbolManifest};

/// Parse + lower one source file.
fn lower(file: FileId, source: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax::parse(source);
    let (hir, manifest, diags) = brink_ir::lower(file, &parsed.tree());
    assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
    (hir, manifest)
}

/// Full single-file analysis.
fn analyzed(source: &str) -> (HirFile, SymbolManifest, brink_analyzer::AnalysisResult) {
    let (hir, manifest) = lower(FileId(0), source);
    let result = brink_analyzer::analyze(&[(FileId(0), &hir, &manifest)]);
    (hir, manifest, result)
}

/// Find a definition id by kind + name in an analysis result.
fn def_id(result: &brink_analyzer::AnalysisResult, kind: SymbolKind, name: &str) -> DefinitionId {
    result
        .index
        .symbols
        .values()
        .find(|s| s.kind == kind && s.name == name)
        .unwrap_or_else(|| panic!("no {kind:?} named {name}"))
        .id
}

// ─── signature(def) stub ────────────────────────────────────────────

const SIG_SRC: &str = "\
#@local
VAR mood = 5
CONST SPEED = 0.5
VAR title = \"Ada\"
=== function damage(weapon, ref hits) ===
~ return 1
=== camp ===
#@local
Text.
= fire
#@local
More.
-> DONE
";

fn sig_for(kind: SymbolKind, name: &str) -> Sig {
    let (hir, _manifest, result) = analyzed(SIG_SRC);
    let def = def_id(&result, kind, name);
    let sig = signature(def, &result.index, &[(FileId(0), &hir)]).expect("known def");
    (*sig).clone()
}

#[test]
fn signature_var_carries_inferred_type_and_local_bit() {
    let sig = sig_for(SymbolKind::Variable, "mood");
    assert_eq!(sig.name, "mood");
    assert_eq!(sig.kind, SymbolKind::Variable);
    assert!(sig.params.is_empty());
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Int));
    assert!(sig.is_local, "#@local VAR must set is_local");

    let title = sig_for(SymbolKind::Variable, "title");
    assert_eq!(title.value_type, Some(brink_analyzer::InferredType::String));
    assert!(!title.is_local);
}

#[test]
fn signature_const_carries_inferred_type() {
    let sig = sig_for(SymbolKind::Constant, "SPEED");
    assert_eq!(sig.kind, SymbolKind::Constant);
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Float));
    assert!(!sig.is_local);
}

#[test]
fn signature_function_knot_carries_params() {
    let sig = sig_for(SymbolKind::Knot, "damage");
    assert_eq!(sig.kind, SymbolKind::Knot);
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.params[0].name, "weapon");
    assert!(!sig.params[0].is_ref);
    assert_eq!(sig.params[1].name, "hits");
    assert!(sig.params[1].is_ref);
    assert_eq!(sig.value_type, None);
    assert!(!sig.is_local);
}

#[test]
fn signature_local_knot_and_stitch_carry_local_bit() {
    let knot = sig_for(SymbolKind::Knot, "camp");
    assert!(knot.is_local, "#@local knot must set is_local");

    let stitch = sig_for(SymbolKind::Stitch, "camp.fire");
    assert_eq!(stitch.name, "camp.fire");
    assert!(stitch.is_local, "#@local stitch must set is_local");
}

#[test]
fn signature_unknown_def_returns_none() {
    let (hir, _manifest, result) = analyzed(SIG_SRC);
    let bogus = DefinitionId::new(brink_format::DefinitionTag::Address, 0xDEAD_BEEF);
    assert!(signature(bogus, &result.index, &[(FileId(0), &hir)]).is_none());
}

#[test]
fn signature_is_declaration_derived_only() {
    // Editing a knot's body (including its temps) must not change its Sig.
    let a = "=== camp ===\nText.\n-> DONE\n";
    let b = "=== camp ===\n~ temp extra = 3\nOther words entirely {extra}.\n-> DONE\n";

    let (hir_a, _m, res_a) = analyzed(a);
    let (hir_b, _m, res_b) = analyzed(b);
    let sig_a = signature(
        def_id(&res_a, SymbolKind::Knot, "camp"),
        &res_a.index,
        &[(FileId(0), &hir_a)],
    )
    .expect("camp in a");
    let sig_b = signature(
        def_id(&res_b, SymbolKind::Knot, "camp"),
        &res_b.index,
        &[(FileId(0), &hir_b)],
    )
    .expect("camp in b");
    assert_eq!(*sig_a, *sig_b, "body edit changed a declaration signature");
}
