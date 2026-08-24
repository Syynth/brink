//! The file-level `#@module`/`#@was` arbitration diagnostics must reach the
//! db road (issue #3088).
//!
//! Before #3088, `lower_file` harvested declarations from a discarded
//! whole-file `lower()` call — and the module arbitration pair emitted in
//! that call's sink (`E095` self-alias, `E049` was-without-module) was
//! silently dropped with it, while `brink-analyzer`'s manifest pass
//! explicitly skips re-diagnosing E095 on the assumption lowering surfaced
//! it. These pins hold the fixed behavior: the arbitration diagnostics are
//! kept, and exactly once (a regression re-adding a duplicate declaration
//! walk would double them).

use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn diag_count(diags: &[brink_ir::Diagnostic], code: DiagnosticCode) -> usize {
    diags.iter().filter(|d| d.code == code).count()
}

/// A self-alias `#@was` (old name equals the current module name) is `E095`
/// ("nothing to migrate") — emitted by the file-level module arbitration,
/// exactly once.
#[test]
fn self_alias_was_reaches_the_db_road_as_e095() {
    let mut db = ProjectDb::new();
    let file = db.update_file(
        "alpha.ink",
        "#@module(alpha)\n#@was(alpha)\n=== greet ===\nHello.\n-> END\n".to_owned(),
    );
    let diags = db.diagnostics(file).expect("file is loaded");
    assert_eq!(
        diag_count(diags, DiagnosticCode::E095),
        1,
        "the module self-alias E095 must reach the db road exactly once: {diags:?}"
    );
}

/// A file-level `#@was` with no `#@module` to attach to is `E049`
/// ("directive not supported on this target") — Error severity, so the
/// malformed directive now fails loudly instead of silently compiling.
#[test]
fn was_without_module_reaches_the_db_road_as_e049() {
    let mut db = ProjectDb::new();
    let file = db.update_file(
        "story.ink",
        "#@was(ghost)\n=== greet ===\nHello.\n-> END\n".to_owned(),
    );
    let diags = db.diagnostics(file).expect("file is loaded");
    assert_eq!(
        diag_count(diags, DiagnosticCode::E049),
        1,
        "the orphaned #@was E049 must reach the db road exactly once: {diags:?}"
    );
}

/// Control: a well-formed `#@module` + `#@was(different_old_name)` pair
/// emits neither arbitration diagnostic.
#[test]
fn well_formed_module_rename_emits_no_arbitration_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.update_file(
        "beta.ink",
        "#@module(beta)\n#@was(old_beta)\n=== greet ===\nHello.\n-> END\n".to_owned(),
    );
    let diags = db.diagnostics(file).expect("file is loaded");
    assert_eq!(diag_count(diags, DiagnosticCode::E095), 0);
    assert_eq!(diag_count(diags, DiagnosticCode::E049), 0);
}

/// A `#@was` whose line attaches to a following declaration is the #1672
/// rename flow's stamp (decl-owned), not an orphaned module directive —
/// even at the top of the file, where the placement is byte-identical to
/// the file-level one. No `E049`. (This exact shape broke `brink ide
/// rename --write` when the orphan diagnostic first surfaced: the CLI
/// stamps `#@was(old)` directly above a line-1 `VAR`.)
#[test]
fn decl_attached_was_on_line_one_is_not_an_orphan() {
    let mut db = ProjectDb::new();
    let file = db.update_file(
        "story.ink",
        "#@was(gold)\nVAR coins = 1\n=== greet ===\nYou have {coins}.\n-> END\n".to_owned(),
    );
    let diags = db.diagnostics(file).expect("file is loaded");
    assert_eq!(
        diag_count(diags, DiagnosticCode::E049),
        0,
        "a decl-attached #@was must not be diagnosed as an orphaned module directive: {diags:?}"
    );
}
