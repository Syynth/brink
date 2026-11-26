//! B0.8 Wave B **tail** exit-criterion tests: the native `.brink`
//! code-dialect statement forms issue #1177 (Wave B's control-flow slice)
//! didn't cover — `return e`/`break`/`continue` and compound/RMW
//! assignment (`x += e`, `x.field += e`) — plus an honest, explicitly-
//! documented gap note for UFCS resolution (`docs/decision-log.md`
//! 2026-07-23 "Code-ground sitting", issue #1322).
//!
//! Sibling file to `b08_native_control_flow.rs` (same wave, same
//! differential-testing approach — deliberately NOT merged into that file
//! since it's scoped to #1177's four control-flow constructs specifically;
//! this file is #1322's own tail slice). Lives as an integration test for
//! the same reason `b06_native_declarations.rs`/`b07_native_body.rs`/
//! `b08_native_control_flow.rs` do: admission checking needs
//! `brink-analyzer`, a dev-dependency that itself depends on `brink-ir`.
//!
//! # Reachability, honestly
//!
//! Same posture as `b08_native_control_flow.rs`: the code-ground statement
//! layer is reachable through the parser only via an expression position
//! (`var x = { … }`), since wiring it into `flow`/`fn` declaration bodies
//! is its own, explicitly deferred slice (issue #1309). The shape
//! differential tests below call
//! `lower_native::control_flow::lower_stmt_block` directly (the same `pub`
//! entry point `b08_native_control_flow.rs` uses) to inspect the resulting
//! `Vec<BlockStmt>` tree.
//!
//! # UFCS: the gap this file pinned, and where it was closed
//!
//! Issue #1322 also listed UFCS resolution (`x.foo(y)` → field-access-wins,
//! else free-fn `foo(x, y)`) as ruled surface, and this file originally
//! pinned it as an honest, unimplemented gap: ink's own grammar
//! structurally rejects the shape (`~ temp x = obj.field()` is `E104` —
//! "Direct-call syntax is RULED to a bare variable/temp/param callee only",
//! `brink-ir/src/hir/lower/expr/references.rs`'s `ast::CallExpr::lower_expr`
//! doc, t1c-spec §3/§10), so there was no differential partner to build
//! against, and no `brink-analyzer` pass resolved a multi-segment
//! `Expr::Call` path.
//!
//! **That gap is closed** (issue #1482, B3a; D1–D5 RULED 2026-07-26). The
//! resolution is type-directed and therefore lives in the analyzer, not in
//! this lowering: `brink-analyzer::ufcs` infers the receiver's type, lets a
//! matching field win outright (`E140` if it isn't callable), else desugars
//! onto a free function in ordinary lexical scope, and records the verdict
//! in a `node → verdict` side table. The *lowering* below is unchanged and
//! still produces the full dotted callee path — which is exactly what that
//! pass keys on, so `ufcs_call_shape_lowers_the_full_dotted_callee_path`
//! stays as the pin for the shape contract the two layers share. The
//! cross-frontend asymmetry it also records (ink cannot express this at
//! all) is what keeps the ink corpus out of the new pass by construction.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{AssignOp, BlockStmt, Expr, FileId, Stmt};
use brink_syntax_native::ast::{self as native_ast, AstNode as _};

// ─── Shared differential-test scaffolding (mirrors `b08_native_control_flow.rs`) ──

fn native_block_stmts(src: &str) -> (Vec<BlockStmt>, Vec<brink_ir::Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "native fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let file = parse.tree();
    let var_decl = file
        .syntax_children()
        .find_map(native_ast::VarDecl::cast)
        .expect("one var decl");
    let value = var_decl.value().expect("initializer node");
    let stmt_block = native_ast::StmtBlock::cast(value).expect("STMT_BLOCK");
    let mut diags = Vec::new();
    let stmts = lower_native::control_flow::lower_stmt_block(FileId(0), &stmt_block, &mut diags);
    (stmts, diags)
}

fn ink_block_stmts(src: &str) -> Vec<BlockStmt> {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(src);
    assert!(
        parse.errors().is_empty(),
        "ink fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    let knot_ast = tree
        .syntax()
        .children()
        .find_map(brink_syntax::ast::KnotDef::cast)
        .expect("ink fixture must contain one knot");
    let (knot, diags) = brink_ir::hir::lower::lower_single_knot(FileId(0), &knot_ast);
    assert!(
        diags.is_empty(),
        "unexpected ink lowering diagnostics: {diags:?}"
    );
    let knot = knot.expect("knot must lower");
    knot.body
        .stmts
        .into_iter()
        .find_map(|s| match s {
            Stmt::LogicBlock(lb) => Some(lb.stmts),
            _ => None,
        })
        .expect("knot body must contain a LogicBlock")
}

fn block_stmt_kind(s: &BlockStmt) -> &'static str {
    match s {
        BlockStmt::TempDecl(_) => "TempDecl",
        BlockStmt::Assignment(_) => "Assignment",
        BlockStmt::Return(_) => "Return",
        BlockStmt::If(_) => "If",
        BlockStmt::While(_) => "While",
        BlockStmt::For(_) => "For",
        BlockStmt::Break(_) => "Break",
        BlockStmt::Continue(_) => "Continue",
        BlockStmt::ExprStmt(_) => "ExprStmt",
        BlockStmt::Await(_) => "Await",
    }
}

fn block_shape(stmts: &[BlockStmt]) -> Vec<&'static str> {
    stmts.iter().map(block_stmt_kind).collect()
}

// ─── Reachability: the real `.brink` pipeline ──────────────────────────

fn lower_native_fixture(
    src: &str,
) -> (
    brink_ir::HirFile,
    brink_ir::SymbolManifest,
    Vec<brink_ir::Diagnostic>,
) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    lower_native::lower(FileId(0), &tree)
}

/// A single `var` initializer exercising every B0.8 Wave B *tail* addition
/// (`return e`, `break`, `continue`, plain and compound assignment),
/// compiled through the real `lower_native::lower` entry point — same
/// "the ONLY diagnostic is the outer block's own known E129" posture as
/// `b08_native_control_flow.rs`'s
/// `admission_clean_for_a_var_initializer_exercising_every_construct`.
#[test]
fn admission_clean_for_a_var_initializer_exercising_every_tail_construct() {
    let src = "\
var x = {
  let a = 1;
  a += 1;
  a -= 1;
  player.gold += 10;
  while a < 10 {
    if a > 5 {
      break;
    }
    continue;
  }
  return a;
}
";
    let (_hir, _manifest, diags) = lower_native_fixture(src);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one diagnostic (the outer block's own unrepresentable \
         value), got: {diags:?}"
    );
    assert_eq!(diags[0].code, brink_ir::DiagnosticCode::E129);
}

// ─── `return e` / `break` / `continue` (issue #1322) ───────────────────

#[test]
fn return_with_value_shape_matches_ink_return_stmt() {
    let native_src = "\
var x = {
  return a + 1;
}
";
    let ink_src = "\
== test ==
~ {
    return a + 1
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Return(native_ret) = &native_stmts[0] else {
        panic!("expected Return");
    };
    let BlockStmt::Return(ink_ret) = &ink_stmts[0] else {
        panic!("expected Return");
    };
    assert!(native_ret.value.is_some());
    assert!(ink_ret.value.is_some());
    assert_eq!(native_ret.kind, brink_ir::ReturnKind::Explicit);
    assert_eq!(ink_ret.kind, brink_ir::ReturnKind::Explicit);
}

#[test]
fn bare_return_shape_matches_ink_return_stmt() {
    let native_src = "\
var x = {
  return;
}
";
    let ink_src = "\
== test ==
~ {
    return
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Return(native_ret) = &native_stmts[0] else {
        panic!("expected Return");
    };
    let BlockStmt::Return(ink_ret) = &ink_stmts[0] else {
        panic!("expected Return");
    };
    assert!(native_ret.value.is_none());
    assert!(ink_ret.value.is_none());
}

#[test]
fn break_continue_shape_matches_ink() {
    let native_src = "\
var x = {
  while a < 10 {
    break;
    continue;
  }
}
";
    let ink_src = "\
== test ==
~ {
    while a < 10 {
        break
        continue
    }
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::While(native_while) = &native_stmts[0] else {
        panic!("expected While");
    };
    let BlockStmt::While(ink_while) = &ink_stmts[0] else {
        panic!("expected While");
    };
    assert_eq!(block_shape(&native_while.body), vec!["Break", "Continue"]);
    assert_eq!(block_shape(&ink_while.body), vec!["Break", "Continue"]);
}

// ─── Compound / RMW assignment (issue #1322) ────────────────────────────

#[test]
fn compound_add_assign_shape_matches_ink() {
    let native_src = "\
var x = {
  a += 1;
}
";
    let ink_src = "\
== test ==
~ {
    a += 1
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Assignment(native_assign) = &native_stmts[0] else {
        panic!("expected Assignment");
    };
    let BlockStmt::Assignment(ink_assign) = &ink_stmts[0] else {
        panic!("expected Assignment");
    };
    assert_eq!(native_assign.op, AssignOp::Add);
    assert_eq!(ink_assign.op, AssignOp::Add);
}

#[test]
fn compound_sub_assign_shape_matches_ink() {
    let native_src = "\
var x = {
  a -= 1;
}
";
    let ink_src = "\
== test ==
~ {
    a -= 1
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Assignment(native_assign) = &native_stmts[0] else {
        panic!("expected Assignment");
    };
    let BlockStmt::Assignment(ink_assign) = &ink_stmts[0] else {
        panic!("expected Assignment");
    };
    assert_eq!(native_assign.op, AssignOp::Sub);
    assert_eq!(ink_assign.op, AssignOp::Sub);
}

/// `x.field += e` — the RMW-path form (decision-log's own example),
/// distinct from the bare-name case above: the target is a dotted `Path`
/// with two segments, exercising `AssignStmt::place`'s `x.field` shape
/// together with the compound operator on the same statement.
#[test]
fn compound_add_assign_on_field_path_shape_matches_ink() {
    let native_src = "\
var x = {
  player.gold += 10;
}
";
    let ink_src = "\
== test ==
~ {
    player.gold += 10
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Assignment(native_assign) = &native_stmts[0] else {
        panic!("expected Assignment");
    };
    let BlockStmt::Assignment(ink_assign) = &ink_stmts[0] else {
        panic!("expected Assignment");
    };
    assert_eq!(native_assign.op, AssignOp::Add);
    assert_eq!(ink_assign.op, AssignOp::Add);
    let Expr::Path(native_path) = &native_assign.target else {
        panic!("expected Path target");
    };
    let Expr::Path(ink_path) = &ink_assign.target else {
        panic!("expected Path target");
    };
    assert_eq!(native_path.segments.len(), 2);
    assert_eq!(ink_path.segments.len(), 2);
}

/// A plain `=` assignment must still lower as `AssignOp::Set` — the
/// pre-existing #1177 behavior, unchanged by this PR's `+=`/`-=` addition.
#[test]
fn plain_assign_still_lowers_as_set() {
    let (native_stmts, diags) = native_block_stmts(
        "\
var x = {
  a = 1;
}
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let BlockStmt::Assignment(native_assign) = &native_stmts[0] else {
        panic!("expected Assignment");
    };
    assert_eq!(native_assign.op, AssignOp::Set);
}

// ─── UFCS: the shape contract the analyzer's pass keys on (module doc) ──

/// Native's `CALL_EXPR` lowering (`lower_native::expr::lower_call`) produces
/// `Expr::Call` for a multi-segment dotted callee path — `x.foo(y)` lowers
/// cleanly, keeping every segment, with no rejection.
///
/// That full dotted path is the **input contract** of the UFCS resolution
/// pass (`brink-analyzer::ufcs`, issue #1482): the pass splits it into a
/// receiver (every segment but the last) and a method name, so collapsing
/// or rewriting it here would silently break resolution. Pinned for that
/// reason — it was originally pinned as a gap signpost (see the module
/// doc's UFCS section) and is now the shared-shape guard between the two
/// layers.
///
/// The second half of the test keeps the cross-frontend asymmetry on the
/// record: ink cannot express this shape at all, which is what keeps the
/// ink corpus out of the type-directed pass by construction.
#[test]
fn ufcs_call_shape_lowers_the_full_dotted_callee_path() {
    let (native_stmts, diags) = native_block_stmts(
        "\
var x = {
  x.foo(y);
}
",
    );
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let BlockStmt::ExprStmt(Expr::Call(path, args)) = &native_stmts[0] else {
        panic!("expected ExprStmt(Call(..))");
    };
    assert_eq!(
        path.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>(),
        vec!["x", "foo"],
        "UFCS-shaped call must keep the full dotted callee path — \
         `brink-analyzer::ufcs` splits it into receiver + method name"
    );
    assert_eq!(args.len(), 1);

    // ink's own grammar has no equivalent: the identical call shape
    // (a non-bare-name callee) is a structural E104 in `brink-ir`'s ink
    // lowering (`hir/lower/expr/references.rs::CallExpr::lower_expr`),
    // not a differential-comparable construct.
    let parse = brink_syntax::parse("== test ==\n~ temp v = x.foo(y)\n-> END\n");
    assert!(parse.errors().is_empty(), "ink fixture must parse cleanly");
    let (_hir, _manifest, ink_diags) = brink_ir::hir::lower(FileId(0), &parse.tree());
    assert!(
        ink_diags
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E104),
        "expected ink to reject the same call shape with E104, got: {ink_diags:?}"
    );
}
