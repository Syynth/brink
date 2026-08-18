//! Issue #2837: `lower_call`'s resolved-target match must refuse a
//! non-callable symbol kind (`ListItem`, `Label`, `Stitch`, `Param`,
//! `Temp`, `Struct`) with a diagnostic (`E183`) instead of silently
//! emitting `lir::Expr::Call` against whatever id happens to be resolved
//! there.
//!
//! `brink-analyzer::resolve::resolve_function` cannot legitimately produce
//! any of these kinds for a real call site today (see `DiagnosticCode::
//! E183`'s own doc for the full reachability argument), so this test
//! cannot be written as ordinary `.ink` source through the full pipeline
//! — there is no author-writable program that reaches the refusal via
//! real resolution. Instead it reproduces the *shape* of the bug PR #2836
//! exposed: take a real analysis result and simulate a resolution mistake
//! by pointing one call site's resolution at a genuinely non-callable
//! symbol already present in the index (a declared `LIST` item), the same
//! way a future resolver regression could. This is `lower_call`'s
//! defensive backstop, exercised directly rather than through the
//! analyzer that is not supposed to produce this input in the first place.

use std::collections::HashMap;

use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolvedRef, Severity, SymbolKind, SymbolManifest,
    hir, lir,
};

/// Find the callee `Path`'s range of the first bare-call statement
/// (`~ name()`, HIR `Stmt::ExprStmt(Expr::Call(path, _))`) in a file's root
/// content — exactly the range `ctx.resolve_path` keys its lookup on
/// (issue #1561, `ResolvedRef::range`'s own doc).
///
/// Returns `Option` rather than unwrapping here: `clippy::expect_used`'s
/// test carve-out (`clippy.toml`'s `allow-expect-in-tests`) only covers a
/// function clippy can see is itself a test, not an ordinary helper called
/// from one — the caller (a `#[test]` fn) is where the `.expect(..)` below
/// belongs.
fn first_call_path_range(file: &HirFile) -> Option<rowan::TextRange> {
    file.root_content.stmts.iter().find_map(|stmt| match stmt {
        hir::Stmt::ExprStmt(hir::Expr::Call(path, _)) => Some(path.range),
        _ => None,
    })
}

#[test]
fn lower_call_refuses_a_resolution_pointed_at_a_non_callable_symbol() {
    // `mystery` is not declared, not a builtin, and not a t1b stdlib name —
    // real analysis leaves its call site fully unresolved (an `E025`
    // diagnostic, no `ResolvedRef` pushed for its range). `LIST Verbs =
    // alpha, beta` gives the index a genuine `ListItem` symbol to point at.
    let source = "LIST Verbs = alpha, beta\n~ mystery()\n-> END\n";

    let parsed = brink_syntax::parse(source);
    let file_id = FileId(0);
    let (mut file, manifest, _diags) = brink_ir::hir::lower(file_id, &parsed.tree());
    brink_ir::hir::normalize_file(&mut file);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &file, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let list_item_id = result
        .index
        .symbols
        .iter()
        .find(|(_, info)| info.kind == SymbolKind::ListItem)
        .map(|(id, _)| *id)
        .expect("`LIST Verbs = alpha, beta` declares at least one ListItem symbol");

    let call_range = first_call_path_range(&file)
        .expect("expected a bare `~ name()` ExprStmt(Call) in root content");

    // Simulate the resolution mistake: point the otherwise-unresolved
    // `mystery()` call site at the list item's id, exactly as a future
    // `resolve_function` regression could.
    let mut resolutions = result.resolutions.clone();
    resolutions.push(ResolvedRef {
        file: file_id,
        range: call_range,
        target: list_item_id,
    });

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &file)];
    let (program, diags) =
        lir::lower_to_program(&files_for_lir, &result.index, &resolutions, &HashMap::new());

    let found = diags.iter().find(|d| d.code == DiagnosticCode::E183);
    assert!(
        found.is_some(),
        "expected an E183 diagnostic, got: {diags:?}"
    );
    let e183: &Diagnostic = found.expect("just asserted above");
    assert!(
        e183.message.contains("ListItem"),
        "E183's message should name the offending kind it actually found: {}",
        e183.message
    );
    assert_eq!(
        e183.code.severity(),
        Severity::Error,
        "a non-callable call target must be a compile error, not advisory"
    );

    // `brink-db`'s production `lir_query` gates `program: None` on any
    // Error-severity diagnostic here (`LowerCtx::diagnostics`'s own doc) —
    // this whole-program entry point still returns `Some` (it has no such
    // gate of its own), so the real proof this is a *compile* error rather
    // than a silent miscompile is the diagnostic assertion above, not
    // `program`'s shape. Lowering must not have emitted `lir::Expr::Call`
    // against the list item's id either way.
    let program = program.expect("lower_to_program still returns a program alongside E183");
    let has_bogus_call = program.root.body.iter().any(|stmt| {
        matches!(
            stmt,
            lir::Stmt::ExprStmt(lir::Expr::Call { target, .. }) if *target == list_item_id
        )
    });
    assert!(
        !has_bogus_call,
        "lowering must never emit Call against the non-callable resolved id"
    );
}
