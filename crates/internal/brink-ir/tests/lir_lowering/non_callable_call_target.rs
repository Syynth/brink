//! Issue #2837: `lower_call`'s resolved-target match must refuse a
//! non-callable symbol kind (`ListItem`, `Label`, `Stitch`, `Param`,
//! `Temp`, `Struct`) with a diagnostic (`E183`) instead of silently
//! emitting `lir::ExprKind::Call` against whatever id happens to be resolved
//! there.
//!
//! `brink-analyzer::resolve::resolve_function` cannot legitimately produce
//! `Stitch`/`ListItem`/`Label`/`Struct` for a real call site (see
//! `DiagnosticCode::E183`'s own doc for the full reachability argument),
//! so the first test below simulates that shape by hand — there is no
//! author-writable program that reaches *those four kinds'* refusal via
//! real resolution. `Param`/`Temp` are different: a genuine forward
//! reference (calling a name before its declaring binding) reaches this
//! same match on ordinary `.ink` source with no simulation at all, and a
//! block-scoped-temp-after-close call must land on `E082` instead — both
//! covered by real-pipeline tests further down this file.

use std::collections::HashMap;

use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolvedRef, Severity, SymbolKind, SymbolManifest,
    hir, lir,
};

use crate::support::{find_diag, lower_ink_with_warnings};

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
    // `program`'s shape. Lowering must not have emitted `lir::ExprKind::Call`
    // against the list item's id either way.
    let program = program.expect("lower_to_program still returns a program alongside E183");
    let has_bogus_call = program.root.body.iter().any(|stmt| match &stmt.kind {
        lir::StmtKind::ExprStmt(e) => matches!(
            &e.kind,
            lir::ExprKind::Call { target, .. } if *target == list_item_id
        ),
        _ => false,
    });
    assert!(
        !has_bogus_call,
        "lowering must never emit Call against the non-callable resolved id"
    );
}

// ── Real-source coverage: the two shapes an author actually hits ─────────
//
// Unlike the hand-constructed-resolution test above, both shapes below
// compile through the full `.ink` pipeline with no simulated resolution —
// review of #2848 found real, author-writable source reaches this match's
// `Temp`/`Param` arms today (a genuine forward reference), and that the
// block-scoped-temp-after-close shape must land on E082, not E183, to
// match `lower_path`'s own guard for the identical mistake.

#[test]
fn calling_a_block_scoped_temp_after_its_block_closes_is_e082_not_e183() {
    // Same defect `block_scoped_temp_read_after_block_closes.rs` proves for
    // a value-read and a `ref`-argument call — here the block-scoped temp
    // itself is the call target. Must land on E082 (mirroring
    // `lower_path`'s own guard), never on the generic E183 refusal.
    let src = "VAR gold = 100\n~ {\n    temp f = 1\n}\n~ f()\n-> END\n";
    let (_program, diags) = lower_ink_with_warnings(src);

    assert!(
        find_diag(&diags, DiagnosticCode::E082).is_some(),
        "expected E082 for a block-scoped temp called after its block closed: {diags:?}"
    );
    assert!(
        find_diag(&diags, DiagnosticCode::E183).is_none(),
        "block-scoped-temp-after-close must not also/instead report E183: {diags:?}"
    );
}

#[test]
fn calling_a_temp_before_its_declaration_is_e183_with_a_forward_reference_message() {
    // A genuine forward reference — `f` is called before its own `temp`
    // declaration. `ctx.temp_slot` has nothing open for `f` at the call
    // site, so this reaches `lower_call`'s non-callable-kind match on real
    // source, no hand-constructed `ResolvedRef` needed. This reproduces on
    // the plain ink surface with no `--dialect brink`.
    let src = "~ f()\n~ temp f = 1\n-> END\n";
    let (_program, diags) = lower_ink_with_warnings(src);

    let e183 = find_diag(&diags, DiagnosticCode::E183)
        .expect("expected E183 for a temp called before its own declaration");
    assert_eq!(e183.code.severity(), Severity::Error);
    assert!(
        e183.message.contains("before its declaration")
            && !e183.message.contains("resolves to a Temp"),
        "forward-reference message must name the real defect, not the misleading \
         'resolves to a Temp, which cannot be called' wording: {}",
        e183.message
    );
}
