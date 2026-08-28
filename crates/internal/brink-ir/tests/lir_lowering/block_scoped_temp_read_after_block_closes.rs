use crate::support::*;
use brink_ir::lir;

// ── #680 RCA: block-scoped temp read after its block closes (E082) ───────
//
// #680 was filed as "a `ref`-argument call co-occurring with a `temp` decl
// in the same `~ { }` block resolves to the wrong global slot
// (UnresolvedGlobal)". Root-causing the reporter's minimal repro against
// current `main` shows the `ref`-argument call is a red herring: it
// reproduces with *no* ref call at all, and does *not* reproduce for the
// literal "ref call + temp decl in the same block" shape alone. The actual
// trigger is reading a T1b block-scoped `temp` (`~ { … }`) from *outside*
// its own block — LIR lowering's fallback for "temp not currently visible"
// (`lower_path`'s `SymbolKind::Temp` arm, kept for inklecate-compat
// classic-temp forward-reference emulation) previously caught this case
// too, silently emitting a phantom hashed `GetGlobal`/`RefGlobal` id that
// was never registered as a real global — exactly the reported
// `UnresolvedGlobal` fault, with zero compile diagnostic. This matches the
// `tests/tier1-brink/annotations-mixed` fixture's own account of the bug
// (docs comment there: rewritten from a `~ { … }` block to standalone `~`
// logic lines "specifically to avoid tripping this bug").

#[test]
fn block_scoped_temp_read_after_block_closes_is_e082_not_a_silent_phantom_global() {
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n{name}\n-> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e082 = find_diag(&diags, brink_ir::DiagnosticCode::E082)
        .expect("expected E082 for a block-scoped temp read after its block closed");
    assert_eq!(e082.code.severity(), brink_ir::Severity::Error);
    // `lower_to_program` is total — it still returns `Some` even with an
    // Error-severity diagnostic recorded (the `brink-db` `lir_query` layer
    // is what refuses to hand back a `Program`, not `lower_to_program`
    // itself, matching the E057/E059 precedent above).
    let program = program.expect("lower_to_program is total — it still returns Some");
    // The pre-fix behavior emitted `Expr::GetGlobal(<hash of "name">)` here
    // — a `DefinitionId` never present in `program.globals` (only `gold`
    // is). Confirm that phantom id is gone: `gold` is still the only
    // global, and it never masquerades as the temp's storage.
    assert_eq!(program.globals.len(), 1);
    assert_eq!(program.globals[0].id, find_global(&program, "gold").id);
}

#[test]
fn block_scoped_temp_passed_by_ref_after_block_closes_is_e082() {
    // Same defect, reached through `lower_call_args`'s `ref`-argument path
    // instead of `lower_path` — `name` is passed by `ref` to `heal` after
    // its declaring block has closed.
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n~ heal(name)\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e082 = find_diag(&diags, brink_ir::DiagnosticCode::E082)
        .expect("expected E082 for an out-of-scope block temp passed by ref");
    assert_eq!(e082.code.severity(), brink_ir::Severity::Error);
    assert!(
        program.is_some(),
        "lower_to_program is total — it still returns Some"
    );
}

#[test]
fn ref_argument_call_with_temp_decl_in_the_same_block_compiles_clean() {
    // The issue's literal minimal repro shape — a `ref`-argument call
    // co-occurring with a `temp` decl in the same `~ { … }` block — with the
    // temp used only inside its own block (never read after the block
    // closes). This must compile with no E082 and resolve `gold`'s `ref`
    // argument to the real global slot, proving the fix doesn't regress the
    // shape #680 was originally filed against.
    let src = "VAR gold = 100\n~ {\n    temp x = 1\n    heal(gold)\n}\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E082).is_none(),
        "temp used only within its own block must never trigger E082: {diags:?}"
    );
    let program = program.expect("well-formed program lowers cleanly");
    let gold_id = find_global(&program, "gold").id;
    let heal = find_child(&program.root, "heal");
    let call = program
        .root
        .body
        .iter()
        .find_map(|s| match &s.kind {
            lir::StmtKind::ExprStmt(e) if matches!(&e.kind, lir::ExprKind::Call { .. }) => Some(e),
            _ => None,
        })
        .expect("heal(gold) should lower to an ExprStmt(Call)");
    match &call.kind {
        lir::ExprKind::Call { target, args } => {
            assert_eq!(*target, heal.id);
            assert_eq!(args.len(), 1);
            match &args[0] {
                lir::CallArg::RefGlobal(id) => assert_eq!(*id, gold_id),
                lir::CallArg::RefTemp(..) => panic!("expected RefGlobal(gold), got RefTemp"),
                lir::CallArg::Value(_) => panic!("expected RefGlobal(gold), got Value"),
                lir::CallArg::RefProjection { .. } => {
                    panic!("expected RefGlobal(gold), got RefProjection")
                }
            }
        }
        _ => unreachable!(),
    }
}
