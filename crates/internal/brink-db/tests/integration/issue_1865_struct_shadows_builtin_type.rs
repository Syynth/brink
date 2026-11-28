//! Issue #1865, db-direct road: a `STRUCT` declared with the same name as a
//! reserved builtin leaf (`content`) or NS-A8 tower kind (`vec3`) must raise
//! `E188` through the production `db.diagnostics(file)` seam — the same
//! seam `brink-compiler`'s `compile_path`/`compile_with_options` and the
//! LSP's `Driver::diagnostics` both read.
//!
//! The fix lives entirely inside `brink_analyzer::annotations::
//! check_reserved_type_names`, called from `per_file_diagnostics` — the one
//! function both `ProjectDb`'s `diagnostics_query` and the off-db
//! `IdeSnapshot::analyze` road reach identically (`brink-ide`'s
//! `live_typing_db_divergence.rs` proves the off-db road directly). Same
//! "fixed at the shared brink-analyzer layer, so both roads share the fix
//! by construction" posture as
//! `issue_2083_fn_valued_const_global_call_site.rs`'s own module doc.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

/// `STRUCT` is a brink-dialect extension on the ink surface (`E051` under
/// `strict-ink`) — every fixture in this file needs `dialect = brink` to
/// even reach the declaration itself, let alone `E188`.
fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// The exact repro from issue #1865's report: a `STRUCT` named after the
/// `content` capture-contract leaf (issue #1846).
#[test]
fn struct_named_content_raises_e188_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "STRUCT content = #{x: int}\nVAR v: content = 0\n-> DONE\n".to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(brink_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E188),
        "a STRUCT shadowing the `content` builtin leaf must be flagged through the \
         db-backed path: {diags:?}"
    );
}

/// The NS-A8 tower-kind sibling: `resolve`'s tower-kind arm runs before the
/// struct lookup too, so a `STRUCT vec3` collides exactly like `content`
/// does.
#[test]
fn struct_named_vec3_raises_e188_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "STRUCT vec3 = #{x: float, y: float, z: float}\n-> DONE\n".to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(brink_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E188),
        "a STRUCT shadowing the `vec3` tower kind must be flagged through the db-backed \
         path: {diags:?}"
    );
}

/// Negative case: an ordinary struct name must not raise `E188` through the
/// same production seam.
#[test]
fn ordinary_struct_name_raises_no_e188_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "STRUCT Point = #{x: float, y: float}\n-> DONE\n".to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(brink_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E188),
        "an ordinary struct name must not raise E188: {diags:?}"
    );
}

/// Negative case, verified truthfully rather than assumed: a struct named
/// after a generic head (`Array`) has no actual collision — `Array<T>`'s
/// special-casing only applies inside `TypeExpr::Generic`, never to a bare
/// `Named` reference — so it must not raise `E188` either.
#[test]
fn struct_named_array_raises_no_e188_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.ink",
        "STRUCT Array = #{x: int}\nVAR v: Array = 0\n-> DONE\n".to_owned(),
    );
    db.set_entry("main.ink");
    db.set_analysis_options(brink_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E188),
        "a STRUCT named `Array` has no real collision (only TypeExpr::Generic special-cases \
         the name) and must not raise E188: {diags:?}"
    );
}

/// Native-surface sibling: `struct content { … }` (`.brink` grammar) must
/// raise `E188` exactly like the ink `STRUCT content = #{…}` spelling does
/// — both frontends lower to the same `HirFile::structs`, and
/// `check_reserved_type_names` walks that shared representation, not either
/// surface's own CST. `is_native` is derived from the `.brink` path itself
/// (`ProjectDb::is_native`), so no explicit dialect option is needed here,
/// unlike the ink fixtures above.
#[test]
fn native_struct_named_content_raises_e188_through_production_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.brink",
        "struct content {\n  x: int\n}\n\nflow main() {\n  Hello. -> END\n}\n".to_owned(),
    );
    db.set_entry("main.brink");

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E188),
        "a native `struct content` must raise E188 through the db-backed path: {diags:?}"
    );
}
