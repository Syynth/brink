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

// ─── symbol_index() ─────────────────────────────────────────────────

const INDEX_SRC_A: &str = "\
VAR gold = 10
LIST mood = happy, (sad)
=== hub ===
Hello.
= plaza
-> DONE
";

const INDEX_SRC_B: &str = "\
EXTERNAL beep(times)
=== market(stock) ===
~ temp haggle = 2
{haggle} {stock}
-> hub
";

#[test]
fn symbol_index_is_deterministic_across_runs() {
    let (_hir_a, man_a) = lower(FileId(0), INDEX_SRC_A);
    let (_hir_b, man_b) = lower(FileId(1), INDEX_SRC_B);
    let inputs = [(FileId(0), &man_a), (FileId(1), &man_b)];

    let (index1, diags1) = brink_analyzer::symbol_index(&inputs);
    let (index2, diags2) = brink_analyzer::symbol_index(&inputs);

    // Content equality: same symbols under the same ids, same name buckets
    // in the same order, same diagnostics in the same order.
    let symbols1: std::collections::BTreeMap<_, _> = index1.symbols.iter().collect();
    let symbols2: std::collections::BTreeMap<_, _> = index2.symbols.iter().collect();
    assert_eq!(symbols1, symbols2);
    let by_name1: std::collections::BTreeMap<_, _> = index1.by_name.iter().collect();
    let by_name2: std::collections::BTreeMap<_, _> = index2.by_name.iter().collect();
    assert_eq!(by_name1, by_name2);
    assert_eq!(diags1, diags2);
    assert!(!index1.symbols.is_empty(), "index has content");
}

#[test]
fn symbol_index_matches_analyze_output() {
    let (hir_a, man_a) = lower(FileId(0), INDEX_SRC_A);
    let (hir_b, man_b) = lower(FileId(1), INDEX_SRC_B);

    let (index, _diags) = brink_analyzer::symbol_index(&[(FileId(0), &man_a), (FileId(1), &man_b)]);
    let result =
        brink_analyzer::analyze(&[(FileId(0), &hir_a, &man_a), (FileId(1), &hir_b, &man_b)]);

    let direct: std::collections::BTreeMap<_, _> = index.symbols.iter().collect();
    let via_analyze: std::collections::BTreeMap<_, _> = result.index.symbols.iter().collect();
    assert_eq!(direct, via_analyze);
}

// ─── resolve(file) ──────────────────────────────────────────────────

#[test]
fn per_file_resolve_concatenation_matches_analyze() {
    let (hir_a, man_a) = lower(FileId(0), INDEX_SRC_A);
    let (hir_b, man_b) = lower(FileId(1), INDEX_SRC_B);
    let (index, _) = brink_analyzer::symbol_index(&[(FileId(0), &man_a), (FileId(1), &man_b)]);

    let (res_a, diags_a) = brink_analyzer::resolve(
        FileId(0),
        &man_a,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    let (res_b, diags_b) = brink_analyzer::resolve(
        FileId(1),
        &man_b,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    let mut concat = (*res_a).clone();
    concat.extend((*res_b).clone());

    let full = brink_analyzer::analyze(&[(FileId(0), &hir_a, &man_a), (FileId(1), &hir_b, &man_b)]);
    assert_eq!(concat, full.resolutions, "per-file concat == whole-project");
    assert!(diags_a.is_empty() && diags_b.is_empty(), "all refs resolve");
}

#[test]
fn resolving_a_file_does_not_observe_other_files_bodies() {
    // File A resolves against the index; file B's *body-only* content (its
    // unresolved refs) must be invisible to A's resolution.
    let (_hir_a, man_a) = lower(FileId(0), INDEX_SRC_A);
    let (_hir_b, man_b) = lower(FileId(1), INDEX_SRC_B);

    // Same declarations in B, different body references.
    let (_hir_b2, man_b2) = lower(
        FileId(1),
        "EXTERNAL beep(times)\n=== market(stock) ===\n~ temp haggle = 2\n{gold} {mood}\n-> hub.plaza\n",
    );

    let (index1, _) = brink_analyzer::symbol_index(&[(FileId(0), &man_a), (FileId(1), &man_b)]);
    let (index2, _) = brink_analyzer::symbol_index(&[(FileId(0), &man_a), (FileId(1), &man_b2)]);

    let (res1, diags1) = brink_analyzer::resolve(
        FileId(0),
        &man_a,
        &index1,
        &brink_analyzer::ImportScope::default(),
    );
    let (res2, diags2) = brink_analyzer::resolve(
        FileId(0),
        &man_a,
        &index2,
        &brink_analyzer::ImportScope::default(),
    );
    assert_eq!(*res1, *res2, "file A's resolutions changed with B's body");
    assert_eq!(diags1, diags2);
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
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
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
    assert!(signature(bogus, &result.index, &[(FileId(0), &hir)], None).is_none());
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
        None,
    )
    .expect("camp in a");
    let sig_b = signature(
        def_id(&res_b, SymbolKind::Knot, "camp"),
        &res_b.index,
        &[(FileId(0), &hir_b)],
        None,
    )
    .expect("camp in b");
    assert_eq!(*sig_a, *sig_b, "body edit changed a declaration signature");
}

// ─── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────────
//
// `signature()` as the annotation firewall: an annotated param/return/`VAR`
// carries the annotation's resolved type in `Sig`, independent of anything
// body inference would derive (annotation wins, per spec).

#[test]
fn signature_knot_carries_param_and_return_type_annotations() {
    let src = "=== function heal(hp: int, amount: float): bool ===\n~ return true\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Knot, "heal");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(
        sig.param_annotations,
        vec![
            Some(brink_analyzer::Ty::Int),
            Some(brink_analyzer::Ty::Float)
        ]
    );
    assert_eq!(sig.return_annotation, Some(brink_analyzer::Ty::Bool));
}

#[test]
fn signature_unannotated_param_is_none_in_param_annotations() {
    let src = "=== heal(hp: int, amount) ===\n~ return\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Knot, "heal");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(
        sig.param_annotations,
        vec![Some(brink_analyzer::Ty::Int), None]
    );
    assert_eq!(sig.return_annotation, None, "no return annotation declared");
}

#[test]
fn signature_stitch_carries_param_annotations_and_no_return_annotation_when_unannotated() {
    let src = "=== camp ===\nText.\n= fire\n~ temp x: string = who\n-> DONE\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Stitch, "camp.fire");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert!(sig.param_annotations.is_empty(), "fire has no params");
    assert_eq!(sig.return_annotation, None, "no return annotation declared");
}

#[test]
fn signature_stitch_carries_a_declared_return_annotation() {
    // #1509: a *nested* stitch's `: type` return clause widens NG-C's
    // `Knot`-only grammar — `signature()`'s `Sig::return_annotation` (the
    // same field `brink-ide::hover` reads for both knots and stitches)
    // must pick it up.
    let src = "=== camp ===\nText.\n= fire(logs: int): bool\n~ return true\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Stitch, "camp.fire");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(sig.param_annotations, vec![Some(brink_analyzer::Ty::Int)]);
    assert_eq!(sig.return_annotation, Some(brink_analyzer::Ty::Bool));
}

#[test]
fn signature_var_annotation_wins_over_the_literal_inferred_type() {
    // The initializer literal alone would infer `Int` (via
    // `infer_literal_type`); the explicit `float` annotation overrides it —
    // annotation wins over inference (spec §3/TM-1 firewall rule).
    let src = "VAR gold: float = 100\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Variable, "gold");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Float));
}

#[test]
fn signature_unannotated_var_keeps_the_literal_inferred_type() {
    let src = "VAR gold = 100\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Variable, "gold");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Int));
}

#[test]
fn signature_const_annotation_wins_over_the_literal_inferred_type() {
    // #641: CONST mirrors VAR's firewall rule — the initializer literal
    // alone would infer `Int`; the explicit `float` annotation overrides
    // it (same pattern as `signature_var_annotation_wins_over_the_literal_inferred_type`).
    let src = "CONST speed: float = 100\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Constant, "speed");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Float));
}

#[test]
fn signature_unannotated_const_keeps_the_literal_inferred_type() {
    let src = "CONST speed = 100\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Constant, "speed");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(sig.value_type, Some(brink_analyzer::InferredType::Int));
}

#[test]
fn signature_list_generic_annotation_resolves_against_declared_list_names() {
    let src = "LIST Weathers = sunny, (rainy)\n=== function pick(w: list<Weathers>): void ===\n~ return\n";
    let (hir, _manifest, result) = analyzed(src);
    let def = def_id(&result, SymbolKind::Knot, "pick");
    let sig = signature(def, &result.index, &[(FileId(0), &hir)], None).expect("known def");
    assert_eq!(
        sig.param_annotations,
        vec![Some(brink_analyzer::Ty::List("Weathers".to_string()))]
    );
    // `void` has no `Ty` representation in this slice (return-position-only
    // sentinel) — `resolve_annotation` correctly reports it as unresolved.
    assert_eq!(sig.return_annotation, None);
}

#[test]
fn strict_ink_suppresses_annotation_content_checks() {
    // Maintainer ruling 2026-07-13: under `strict-ink` a bad annotation is
    // rejected whole by the dialect gate (E051) — the content checks
    // (E061 unknown name / E062 fn-reserved) must NOT stack a second
    // diagnostic on the same span. Under `brink` the content checks fire.
    // #641: CONST is covered by the same gated `annotations::check` call
    // as VAR — exercised here alongside it, not re-gated separately.
    let src = "VAR cb: fn(int): bool = 0\nVAR p: Frobnicator = 0\nCONST bad: Bogus = 0\n";
    let (hir, manifest) = lower(FileId(0), src);
    let files = [(FileId(0), &hir, &manifest)];

    let strict = brink_analyzer::analyze_with_options(
        &files,
        &brink_analyzer::AnalysisOptions::default(), // dialect = StrictInk
    );
    assert!(
        strict
            .diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E051),
        "strict-ink still rejects the annotation as extension syntax"
    );
    assert!(
        !strict.diagnostics.iter().any(|d| matches!(
            d.code,
            brink_ir::DiagnosticCode::E061 | brink_ir::DiagnosticCode::E062
        )),
        "strict-ink must not critique the content of rejected syntax: {:?}",
        strict.diagnostics
    );

    let brink = brink_analyzer::analyze_with_options(
        &files,
        &brink_analyzer::AnalysisOptions {
            dialect: brink_analyzer::Dialect::Brink,
            ..Default::default()
        },
    );
    assert!(
        brink
            .diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E061),
        "brink dialect flags the unknown type name"
    );
    // T1c-1 (#699): `fn(T…): R` is a legal type form now — E062 is retired
    // and must not fire under either dialect.
    assert!(
        !brink
            .diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E062),
        "fn(...) types are legal since T1c-1 — E062 is retired: {:?}",
        brink.diagnostics
    );
}
