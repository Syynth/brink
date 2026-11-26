//! Regression tests for issue #530: `signature_query`/`db.signature`
//! returns `None` for a local (`Param`/`Temp`) `DefinitionId` — by design
//! (`signature_is_none_for_locals` in `query_equivalence.rs` pins that) —
//! but a caller that already knows the local's declaring file must have a
//! working alternative instead of a silent "no signature" dead end. These
//! tests exercise `db.local_signature`, the per-file locals path that fills
//! that gap.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_ir::SymbolKind;

fn local_def(
    db: &ProjectDb,
    name: &str,
    kind: SymbolKind,
) -> (brink_ir::FileId, brink_format::DefinitionId) {
    let index = db.symbol_index();
    let (id, info) = index
        .symbols
        .iter()
        .find(|(_, info)| info.name == name && info.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} symbol named {name:?} in the index"));
    (info.file, *id)
}

#[test]
fn local_signature_resolves_a_params_own_annotation() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "=== function heal(hp: int) ===\n~ return hp\n".to_owned(),
    );

    let (decl_file, def) = local_def(&db, "hp", SymbolKind::Param);
    assert_eq!(decl_file, file);

    // `signature`/`db.signature` stays `None` for a local id — #517/#531's
    // decls-only contract is untouched by this per-file path.
    assert_eq!(db.signature(def), None);

    let sig = db
        .local_signature(file, def)
        .expect("local_signature resolves a declared param");
    assert_eq!(sig.name, "hp");
    assert_eq!(sig.kind, SymbolKind::Param);
    assert_eq!(sig.value_ty, Some(brink_analyzer::Ty::Int));
    assert!(!sig.is_local, "Param/Temp never carry #@local");
}

#[test]
fn local_signature_resolves_a_temps_own_annotation() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "=== quest ===\n~ temp step: float = 1.0\nOnward.\n-> END\n".to_owned(),
    );

    let (decl_file, def) = local_def(&db, "step", SymbolKind::Temp);
    assert_eq!(decl_file, file);

    let sig = db
        .local_signature(file, def)
        .expect("local_signature resolves a declared temp");
    assert_eq!(sig.name, "step");
    assert_eq!(sig.kind, SymbolKind::Temp);
    assert_eq!(sig.value_ty, Some(brink_analyzer::Ty::Float));
}

#[test]
fn local_signature_is_some_but_untyped_for_an_unannotated_local() {
    // No `: type` ascription on either — `local_signature` still finds the
    // local (unlike `signature`'s hard `None`), it just carries no
    // annotation-derived type, matching `signature`'s own "declaration-
    // derived only, no body inference" contract.
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "=== quest(hero) ===\n~ temp step = 1\nOnward.\n-> END\n".to_owned(),
    );

    let (_, param_def) = local_def(&db, "hero", SymbolKind::Param);
    let (_, temp_def) = local_def(&db, "step", SymbolKind::Temp);

    let param_sig = db
        .local_signature(file, param_def)
        .expect("local_signature still finds an unannotated param");
    assert_eq!(param_sig.value_ty, None);

    let temp_sig = db
        .local_signature(file, temp_def)
        .expect("local_signature still finds an unannotated temp");
    assert_eq!(temp_sig.value_ty, None);
}

#[test]
fn local_signature_is_none_for_the_wrong_file_or_an_unknown_def() {
    let mut db = ProjectDb::new();
    let file_a = db.set_file(
        "a.ink",
        "=== quest(hero: int) ===\nOnward.\n-> END\n".to_owned(),
    );
    let file_b = db.set_file("b.ink", "-> END\n".to_owned());

    let (_, def) = local_def(&db, "hero", SymbolKind::Param);

    // A local never resolves across files (#517) — asking `b.ink` for
    // `a.ink`'s own `hero` param must come back empty, not silently find it.
    assert_eq!(db.local_signature(file_b, def), None);

    // A declaration id (not a local) is `signature`'s job, not this one's.
    let index = db.symbol_index();
    let (quest_id, _) = index
        .symbols
        .iter()
        .find(|(_, info)| info.name == "quest")
        .expect("quest knot indexed");
    assert_eq!(db.local_signature(file_a, *quest_id), None);
}

/// FG-1-style dependency-edge pin (mirrors `fg1_dependency_edges.rs`):
/// `local_signature_query` must depend only on `file`'s own `lowered_query`,
/// not on every project file's — a body edit in an *unrelated* file must
/// leave the memo fully validated (same `Arc`), not re-executed.
#[test]
fn local_signature_memo_survives_unrelated_file_body_edit() {
    let mut db = ProjectDb::new();
    let file_a = db.set_file(
        "a.ink",
        "=== quest(hero: int) ===\nOnward.\n-> END\n".to_owned(),
    );
    db.set_file(
        "b.ink",
        "=== filler ===\nOriginal filler line.\n-> END\n".to_owned(),
    );

    let (_, def) = local_def(&db, "hero", SymbolKind::Param);

    let before = db.local_signature(file_a, def).expect("first read");
    // Edit b.ink's body only (no new/removed declaration) — same shape as
    // `fg1_dependency_edges.rs`'s `signature_memo_survives_unrelated_file_body_edit`.
    db.update_file(
        "b.ink",
        "=== filler ===\nA new line before.\nOriginal filler line, revised.\n-> END\n".to_owned(),
    );
    let after = db.local_signature(file_a, def).expect("second read");

    assert!(
        std::sync::Arc::ptr_eq(&before, &after),
        "an edit to a different file must not re-execute a's local_signature memo"
    );
}
