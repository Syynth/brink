//! Lambda lowering on the native `.brink` surface (issue #1685) — the
//! orphaned-ruling slice.
//!
//! The 2026-07-19 airport sitting ruled the whole lambda surface
//! (`docs/decision-log.md`, "Lambdas ruled: Rust pipes under the `RustScript`
//! north star"), but `hir::lower_native::expr` kept an `E129` fence saying
//! it was waiting for that sitting. These tests pin each half of the ruling
//! against the real lowering:
//!
//! - Rust pipes with **colon** returns (`|g| g.awake`, `|g: Guest|: bool
//!   { … }`, `||`), params optionally annotated;
//! - single-expression **or** braced-block bodies, with the block's
//!   trailing expression as the value ("last expression is the value");
//! - `return` inside a body lowering to an ordinary `BlockStmt::Return`;
//! - **assignment to a captured binding is a compile error** (`E156`) —
//!   including the three shapes that must *not* fire it (own param, own
//!   `let`, a module-level global cell);
//! - and, negatively, that `E129` no longer fires anywhere near a lambda.
//!
//! Reachability: every fixture is lowered through the production
//! `lower_native::lower` entry point over a whole `.brink` source file —
//! the same call the `.brink` compile path makes (`brink-compiler`'s
//! `driver::prepare_driver` → `brink-db`'s lowering query), never a
//! hand-built node. Most fixtures put their lambda where an author would:
//! in a `let` inside a `fn` body.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{
    BlockStmt, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, LambdaBody, LambdaExpr, Stmt,
    TypeExpr,
};

fn lower(src: &str) -> (HirFile, Vec<Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "native fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    (hir, diags)
}

/// Wrap a lambda in the shape an author writes: a `let` inside a `fn` body.
fn in_fn_body(lambda_src: &str) -> String {
    format!("fn make() {{\n  let f = {lambda_src};\n}}\n")
}

/// The first `Expr::Lambda` anywhere in the file's lowered HIR.
fn first_lambda(hir: &HirFile) -> LambdaExpr {
    struct Find(Option<LambdaExpr>);
    impl brink_ir::HirVisitor for Find {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, e: &Expr) {
            if let Expr::Lambda(l) = e
                && self.0.is_none()
            {
                self.0 = Some((**l).clone());
            }
        }
    }
    let mut find = Find(None);
    brink_ir::hir::visit::visit_with_decl_initializers(hir, &mut find);
    find.0.expect("a lambda in the lowered HIR")
}

fn lower_lambda(lambda_src: &str) -> (LambdaExpr, Vec<Diagnostic>) {
    let (hir, diags) = lower(&in_fn_body(lambda_src));
    (first_lambda(&hir), diags)
}

fn codes(diags: &[Diagnostic]) -> Vec<&'static str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

// ─── The surface: pipes, annotations, the colon return ───────────────

#[test]
fn single_expression_body_lowers_to_a_lambda() {
    let (lambda, diags) = lower_lambda("|g| g.awake");
    assert_eq!(codes(&diags), Vec::<&str>::new(), "no diagnostics expected");
    assert_eq!(lambda.params.len(), 1);
    assert_eq!(lambda.params[0].name.text, "g");
    assert!(
        lambda.params[0].annotation.is_none(),
        "param is unannotated"
    );
    assert!(!lambda.params[0].is_ref, "lambda params are never `ref`");
    assert!(lambda.return_type.is_none(), "no return annotation written");
    assert!(matches!(lambda.body, LambdaBody::Expr(_)));
}

#[test]
fn zero_arg_lambda_lowers_with_an_empty_param_row() {
    let (lambda, diags) = lower_lambda("|| 1");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    assert!(lambda.params.is_empty());
    assert!(matches!(lambda.body, LambdaBody::Expr(_)));
}

#[test]
fn annotated_params_and_the_colon_return_lower() {
    let (lambda, diags) = lower_lambda("|g: Guest, n: int|: bool { true }");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    assert_eq!(lambda.params.len(), 2);
    assert!(matches!(
        lambda.params[0].annotation,
        Some(TypeExpr::Named { ref name, .. }) if name == "Guest"
    ));
    assert!(matches!(
        lambda.params[1].annotation,
        Some(TypeExpr::Named { ref name, .. }) if name == "int"
    ));
    assert!(matches!(
        lambda.return_type,
        Some(TypeExpr::Named { ref name, .. }) if name == "bool"
    ));
}

#[test]
fn a_lambda_lowers_in_a_top_level_var_initializer_too() {
    // The other reachable expression position: a hoisted declaration
    // initializer (`decl::lower_var_decl` → `expr::lower_expr`).
    let (hir, diags) = lower("var awake = |g| g.awake\n");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    assert!(matches!(
        hir.variables.first().expect("one var").value,
        Expr::Lambda(_)
    ));
}

// ─── Bodies: the block form, its tail value, and `return` ────────────

#[test]
fn braced_body_keeps_statements_and_the_trailing_value_expression() {
    let (lambda, diags) = lower_lambda("|x| { let base = 2; base + x }");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    let LambdaBody::Block { stmts, tail } = &lambda.body else {
        panic!("expected a braced body, got {:?}", lambda.body);
    };
    assert_eq!(stmts.len(), 1, "the `let` is the only statement");
    assert!(
        matches!(stmts[0], BlockStmt::TempDecl(_)),
        "the `let` lowers as a TempDecl, got {:?}",
        stmts[0]
    );
    let tail = tail.as_ref().expect("`base + x` is the lambda's value");
    assert!(
        matches!(**tail, Expr::Infix(_)),
        "tail is the `base + x` expression, got {tail:?}"
    );
}

#[test]
fn braced_body_without_a_tail_has_no_value_expression() {
    let (lambda, diags) = lower_lambda("|x| { let seen = x; }");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    let LambdaBody::Block { stmts, tail } = &lambda.body else {
        panic!("expected a braced body");
    };
    assert_eq!(stmts.len(), 1);
    assert!(tail.is_none(), "no trailing expression, so no value");
}

#[test]
fn return_inside_a_lambda_lowers_to_an_ordinary_return_statement() {
    let (lambda, diags) = lower_lambda("|x| { return x; }");
    assert_eq!(codes(&diags), Vec::<&str>::new());
    let LambdaBody::Block { stmts, .. } = &lambda.body else {
        panic!("expected a braced body");
    };
    assert!(
        matches!(stmts[0], BlockStmt::Return(_)),
        "got {:?}",
        stmts[0]
    );
}

// ─── The fence that used to stand here ───────────────────────────────

#[test]
fn a_lambda_no_longer_reports_the_unsupported_construct_fence() {
    // The regression this whole issue is about: `E129` ("construct not
    // supported by this lowering") used to be the *only* thing a lambda
    // produced. Every body form must now be clean of it.
    for lambda_src in [
        "|g| g.awake",
        "|| 1",
        "|g: Guest|: bool { true }",
        "|x| { let y = x; y }",
    ] {
        let (_, diags) = lower(&in_fn_body(lambda_src));
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E129),
            "E129 must not fire for `{lambda_src}`: {:?}",
            codes(&diags)
        );
    }
}

// ─── Capture discipline: E156 ────────────────────────────────────────

fn has_e156(src: &str) -> bool {
    let (_, diags) = lower(src);
    diags.iter().any(|d| d.code == DiagnosticCode::E156)
}

#[test]
fn assigning_to_a_captured_let_is_an_error() {
    assert!(
        has_e156("fn tally() {\n  let total = 0;\n  let add = |x| { total = x; };\n}\n"),
        "writing to the captured `total` must be E156"
    );
}

#[test]
fn compound_assignment_to_a_captured_let_is_an_error() {
    assert!(
        has_e156("fn tally() {\n  let total = 0;\n  let add = |x| { total += x; };\n}\n"),
        "`+=` is a write too"
    );
}

#[test]
fn assigning_to_a_field_of_a_captured_binding_is_an_error() {
    assert!(
        has_e156("fn tally() {\n  let acc = 0;\n  let add = |x| { acc.count = x; };\n}\n"),
        "a field write through a captured binding is still a lost write"
    );
}

#[test]
fn assigning_to_an_enclosing_fn_param_is_an_error() {
    assert!(
        has_e156("fn tally(n: int) {\n  let add = |x| { n = x; };\n}\n"),
        "an enclosing function's param is captured by value like any binding"
    );
}

#[test]
fn assigning_to_an_outer_lambdas_param_is_an_error() {
    assert!(
        has_e156("fn nest() {\n  let f = |outer| |inner| { outer = inner; };\n}\n"),
        "the inner lambda captures the outer lambda's param"
    );
}

#[test]
fn assigning_to_the_lambdas_own_param_is_fine() {
    assert!(
        !has_e156("fn f() {\n  let g = |x| { x = 1; };\n}\n"),
        "a lambda's own param is a local, not a capture"
    );
}

#[test]
fn assigning_to_a_let_declared_inside_the_lambda_is_fine() {
    assert!(
        !has_e156("fn f() {\n  let g = |x| { let seen = x; seen = 2; };\n}\n"),
        "an inner `let` is a local"
    );
}

#[test]
fn assigning_to_an_if_as_binding_is_an_error() {
    // `as NAME` is a trailing sibling of the condition head, parsed as a
    // child of `IF_STMT`/`WHILE_STMT`/`CONDITIONAL_BLOCK`/`CHOICE_GUARD`
    // (`parser::binding::as_binding`) — never an ancestor of the lambda
    // itself, so `outer_binders` must scan the `IF_STMT`'s children to see
    // it, the same way it already scans a `STMT_BLOCK`'s children for a
    // sibling `let`.
    assert!(
        has_e156(
            "fn f(o: option<int>) {\n  if find(o) as i {\n    let g = |x| { i = x; };\n  }\n}\n"
        ),
        "writing to the `if ... as i` binding from an inner lambda must be E156"
    );
}

#[test]
fn assigning_to_a_global_is_not_a_capture() {
    // A module-level `var` is a durable cell reached by name, not a
    // snapshotted binding — writing to one from a lambda is a real write.
    assert!(
        !has_e156("var gold = 0\n\nfn spend() {\n  let pay = |n| { gold = n; };\n}\n"),
        "a global write is not a lost write"
    );
}

// ─── Provenance + shared-walk plumbing ───────────────────────────────

#[test]
fn a_lambda_carries_its_own_provenance_span() {
    let src = "var awake = |g| g.awake\n";
    let (hir, _) = lower(src);
    let lambda = match &hir.variables[0].value {
        Expr::Lambda(l) => (**l).clone(),
        other => panic!("expected a lambda, got {other:?}"),
    };
    let span =
        brink_ir::expr_span(&Expr::Lambda(Box::new(lambda.clone()))).expect("lambda has a span");
    assert_eq!(
        &src[usize::from(span.start())..usize::from(span.end())],
        "|g| g.awake",
        "the span covers the whole lambda, not just its body"
    );
    assert_eq!(lambda.ptr.class(), brink_ir::NodeClass::Lambda);
}

#[test]
fn the_shared_hir_walk_descends_into_a_lambda_body() {
    // `hir::visit` is what IDE queries and analyzer passes share; a name
    // read inside a lambda body must be visible to them.
    struct Names(Vec<String>);
    impl brink_ir::HirVisitor for Names {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, e: &Expr) {
            if let Expr::Path(p) = e {
                self.0.push(p.segments[0].text.clone());
            }
        }
    }
    let (hir, _) = lower("fn f() {\n  let g = |x| { let y = seen; y + x };\n}\n");
    let mut v = Names(Vec::new());
    brink_ir::hir::visit::visit_with_decl_initializers(&hir, &mut v);
    assert!(
        v.0.iter().any(|n| n == "seen"),
        "the walk must reach the lambda body's reads, saw {:?}",
        v.0
    );
}

#[test]
fn lambda_params_are_recorded_as_locals_in_the_symbol_manifest() {
    // Without this, a reference to `g` inside `|g| g.awake` would resolve
    // to nothing and the analyzer would report it as unknown.
    let parse = brink_syntax_native::parse("fn f() {\n  let h = |g| g.awake;\n}\n");
    let (_hir, manifest, _diags) = lower_native::lower(FileId(0), &parse.tree());
    assert!(
        manifest.locals.iter().any(|l| l.name == "g"),
        "lambda param `g` must be a recorded local: {:?}",
        manifest.locals.iter().map(|l| &l.name).collect::<Vec<_>>()
    );
}

/// A knot body is `Stmt`-shaped; this is the sanity check that the fixture
/// shape used above really does place the lambda inside a lowered body
/// (rather than silently producing an empty stub body).
#[test]
fn the_fixture_shape_really_lowers_a_body() {
    let (hir, _) = lower(&in_fn_body("|g| g.awake"));
    let knot = hir.knots.first().expect("one knot");
    assert!(
        knot.body
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::LogicBlock(_))),
        "the `fn` body must lower to a logic block, got {:?}",
        knot.body.stmts
    );
}
