use crate::support::*;
use brink_ir::lir;

// Issue #1774 (RULED 2026-08-01, `docs/decision-log.md`): a native `var`/
// `const` may hold a fn value — both a bare-name function reference
// (already legal, #1862) and a lambda literal, which is this issue's own
// scope. `is_const_foldable_decl_default`'s `Lambda` arm used to raise
// `E083` here; `decls::collect_globals`'s new lambda path
// (`eval_const_lambda`) lifts a file-scope lambda literal into a real
// synthesized function value, reusing `lower::lambda::lower_lambda`
// verbatim — just handed an *empty* enclosing frame, since file scope has
// none. Native-only feature (lambdas and bare-name fn references are both
// native-surface constructs), hence `lower_native` rather than the sibling
// files' `lower_ink*` helpers. Top-level `var`/`const` decls take no
// trailing `;` on the native surface (unlike a `let` inside a body).

#[test]
fn const_lambda_literal_decl_default_folds_without_e083() {
    let source = "const twice = |x| x * 2\n\nflow main() {\n  Result: {twice(21)} -> END\n}\n";
    let (program, diagnostics) = lower_native(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E083),
        "a lambda literal is now a legal whole-default value, expected no E083, got {diagnostics:?}"
    );
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "twice");
    assert!(
        !g.mutable,
        "a `const` global stays immutable regardless of its default's kind"
    );
    assert!(
        matches!(g.default, lir::ConstValue::FnRef(_)),
        "a file-scope lambda has no enclosing frame to capture from, so it \
         must fold to a bare FnRef (no bound environment), got {:?}",
        g.default
    );
}

#[test]
fn var_lambda_literal_decl_default_also_folds() {
    let source = "var scale = |x| x + 1\n\nflow main() {\n  Result: {scale(1)} -> END\n}\n";
    let (program, diagnostics) = lower_native(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E083),
        "expected no E083 for a VAR lambda literal default, got {diagnostics:?}"
    );
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "scale");
    assert!(g.mutable, "a `var` global stays mutable");
    assert!(matches!(g.default, lir::ConstValue::FnRef(_)));
}

/// The ruling's own owed pin (`docs/decision-log.md` #1774's "Owed by the
/// implementation"): "a file-scope lambda cannot capture flow-local state",
/// made mechanical rather than merely argued. `adder`'s body reads another
/// global (`base`) — the same Path-to-`Variable` mechanism a flow-local
/// override would be read through if one existed at file scope (there is no
/// separate "capture" spelling for a flow-local cell; see the ruling's WHY).
/// Reading `base` must resolve as an ordinary global reference, not a
/// capture: `lir::Expr::MakeFnValue`'s `bound` row must stay empty
/// (`ConstValue::FnRef`, never `Closure`). If a future change ever gave file
/// scope a real temp/param frame — the only thing
/// `lower::lambda::captured_locals` treats as capturable — this assertion
/// is what would catch a captured-environment leak turning this into a
/// `Closure` carrying a snapshotted `base`, instead of the privacy hole
/// opening silently.
#[test]
fn lambda_literal_decl_default_reads_other_globals_without_capturing_them() {
    let source = "var base = 10\nconst adder = |x| x + base\n\n\
                   flow main() {\n  Result: {adder(5)} -> END\n}\n";
    let (program, diagnostics) = lower_native(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E083),
        "expected no E083, got {diagnostics:?}"
    );
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "adder");
    assert!(
        matches!(g.default, lir::ConstValue::FnRef(_)),
        "reading another global from a file-scope lambda body must resolve \
         as an ordinary global reference, never a captured/snapshotted \
         environment — got {:?}",
        g.default
    );
}

/// Two files' lambda-literal decl defaults at the same source offset must
/// not collide on `DefinitionId` (#1504) — `collect_globals` qualifies the
/// `IdAllocator`'s path prefix per file (`hir::root_content_scope_path`),
/// the same qualifier `lower_root_content_chunks` gives every other
/// per-file anonymous container.
#[test]
fn two_files_lambda_literals_at_the_same_offset_get_distinct_targets() {
    let a = "const f = |x| x\n";
    let b = "const g = |x| x\n";
    let program = lower_native_files_with_paths(&[("a.brink", a), ("b.brink", b)]);
    let f = find_global(&program, "f");
    let g = find_global(&program, "g");
    let (lir::ConstValue::FnRef(f_target), lir::ConstValue::FnRef(g_target)) =
        (&f.default, &g.default)
    else {
        panic!(
            "both defaults must be FnRefs, got {:?} / {:?}",
            f.default, g.default
        );
    };
    assert_ne!(
        f_target, g_target,
        "two files' same-offset lambda literals must not mint the same DefinitionId"
    );
}
