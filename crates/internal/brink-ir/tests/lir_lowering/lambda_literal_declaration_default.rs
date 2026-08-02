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
    let lir::ConstValue::FnRef(target) = g.default else {
        panic!(
            "a file-scope lambda has no enclosing frame to capture from, so it \
             must fold to a bare FnRef (no bound environment), got {:?}",
            g.default
        );
    };
    // Review finding on #1774: `assemble_program`'s
    // `root_children.extend(prelude.lifted.iter().cloned())` is the one hunk
    // that makes this feature more than a type-check relaxation — without
    // it, `twice`'s `FnRef` target would point at a container that was never
    // actually assembled into the program. Walk the assembled tree (not just
    // `PreludeDecls`/`GlobalDef::default`) to prove the lifted container is
    // really there, really a function, and really has `twice`'s one `x`
    // parameter.
    let lifted = find_any(&program.root, &|c| c.id == target).unwrap_or_else(|| {
        panic!(
            "no container with id {target:?} was assembled into program.root's \
             tree — the FnRef target does not resolve to a real container"
        )
    });
    assert!(
        lifted.is_function,
        "the lifted container for a decl-default lambda must be a function"
    );
    assert_eq!(
        lifted.params.len(),
        1,
        "expected `twice`'s one `x` param, got {} params",
        lifted.params.len()
    );
    assert_eq!(
        program.name_table[lifted.params[0].name.0 as usize], "x",
        "expected the lifted container's param to be named `x`"
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

/// Review finding on #1774: `GlobalLambdaCtx`'s `AnalyzerTables` used to be
/// unconditionally empty (`UfcsLookup::new()`/`CoalesceLookup::new()`),
/// regardless of what the caller actually computed — so a decl-default
/// lambda body's own `or`-coalescing chains always fell back to
/// `CoalesceShape::RuntimeCheck` (the default for "no verdict recorded"),
/// even though `coalesce::resolve` *does* record a real verdict for a
/// decl-default lambda's chains (it hand-recurses over
/// `hir.variables`/`hir.constants` specifically because their initializers
/// sit outside `visit::visit`'s walk — issue #1764). This test proves the
/// real table now reaches the lambda body: `some(1) or 2`'s left-hand side
/// is a statically-known `Option[int]`, so the chain collapses
/// (`CoalesceShape::Collapse`), not the runtime-check fallback a caller
/// still handing an empty table would be stuck with.
#[test]
fn coalesce_chain_in_lambda_decl_default_gets_its_real_recorded_shape() {
    let source = "const f = ||: int {\n  let x = some(1) or 2;\n  x\n}\n\n\
                   flow main() {\n  Result: {f()} -> END\n}\n";
    let (program, diagnostics) = lower_native_with_real_tables(source);
    assert!(
        diagnostics.is_empty(),
        "expected a clean lowering: {diagnostics:?}"
    );
    let program = program.expect("lowering stays total");
    let lambda = find_any(&program.root, &|c| c.is_function)
        .expect("the decl-default lambda must lift into its own function container");

    let shape = lambda.body.iter().find_map(|stmt| match stmt {
        lir::Stmt::DeclareTemp {
            value: Some(lir::Expr::Coalesce { shape, .. }),
            ..
        } => Some(*shape),
        _ => None,
    });
    assert_eq!(
        shape,
        Some(lir::CoalesceShape::Collapse),
        "expected the real recorded shape (Collapse — Option[int] or int), \
         got {shape:?} — either no Coalesce DeclareTemp was found or its \
         shape fell back to the empty-table default"
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
