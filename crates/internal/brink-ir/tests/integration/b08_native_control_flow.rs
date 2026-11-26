//! B0.8 Wave B exit-criterion tests: native `.brink` code-dialect
//! control-flow **statement** lowering — `if`/`else`, `while`,
//! `for … in …`, `until` (`docs/decision-log.md` 2026-07-23 "Code-ground
//! sitting", issue #1177).
//!
//! Lives as an integration test for the same reason `b06_native_declarations.rs`/
//! `b07_native_body.rs` do (see those files' module docs): admission
//! checking needs `brink-analyzer`, a dev-dependency that itself depends on
//! `brink-ir`.
//!
//! # Reachability, honestly
//!
//! The code-ground statement layer (`STMT_BLOCK`/`LET_STMT`/…, B0.8 Wave A)
//! is reachable through the parser via an **expression position** —
//! `var x = { … }` — AND, since #1309 (body-dialect selectors, charter §4,
//! RULED 2026-07-23), as a `flow`/`fn` declaration's own body: plain
//! `{ }` on a `fn` (its per-keyword default) or `~{ }` on a `flow` (the
//! "Compound guard" override) both parse+lower through this same
//! `STMT_BLOCK` grammar (`parser/decl.rs::decl_body`). `var`/`const`
//! initializers were wired into the real `lower_native::lower` pipeline
//! first (`decl::lower_var_decl` → `expr::lower_expr`), and
//! `expr::lower_expr`'s `STMT_BLOCK` arm calls
//! `lower_native::control_flow::lower_stmt_block` for real — so
//! `admission_clean_for_a_var_initializer_exercising_every_construct`
//! below proves the four control-flow constructs are lowered by that
//! production entry point on a real `.brink` file, not just a differential
//! fixture. What that arm still can't do is give the `STMT_BLOCK` itself a
//! *value* (blocks-as-values has no HIR node — no `Expr::Block` exists,
//! and NF-2 forbids minting one this slice) — so the pipeline test below
//! asserts exactly one diagnostic (E129, "the block has no value yet"),
//! not zero, and that is the honest, expected shape.
//!
//! `fn_body_default_reaches_stmt_block_lowering_for_real` further down
//! proves the **declaration-body** call site: a `fn`'s default `{ }` body
//! lowers through `container::lower_body` → `body::lower_stmt_block_as_body`
//! → this same `control_flow::lower_stmt_block`, wrapped as the container's
//! sole `Stmt::LogicBlock` — the exact shape a brink-dialect container
//! whose entire body is one `~ { … }` block already produces (see
//! `ink_block_stmts` below).
//!
//! The **shape** differential tests further down call
//! `lower_native::control_flow::lower_stmt_block` directly (a small `pub`
//! entry point mirroring `hir::lower_single_knot`'s "lower one construct in
//! isolation" precedent) so they can inspect the resulting `Vec<BlockStmt>`
//! tree — something the pipeline test can't do, since the outer
//! `Expr::Null` result throws it away. This mirrors the b07 file's own
//! honest split between "prove real-pipeline reachability" and "prove
//! shape correctness via a differential test."

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{BlockStmt, ElseBranch, FileId, Stmt};
use brink_syntax_native::ast::{self as native_ast, AstNode as _};

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

/// A single `var` initializer exercising every B0.8 Wave B construct group
/// (`let`/assignment/expression statements from Wave A, plus `if`/`else`,
/// `while`, `for … in …`, `until`), compiled through the real
/// `lower_native::lower` entry point. The ONLY diagnostic is the known,
/// expected E129 on the outer `STMT_BLOCK` itself (blocks-as-values isn't
/// representable yet) — every construct *inside* it lowers with zero
/// further diagnostics, proving the control-flow lowering is exercised for
/// real, not just parsed.
#[test]
fn admission_clean_for_a_var_initializer_exercising_every_construct() {
    // `items`/`done` are left undeclared on purpose — structural HIR
    // lowering never resolves names (that's `brink-analyzer`'s job), so an
    // unresolved path is a perfectly ordinary `Expr::Path` here, and using
    // one keeps this fixture free of native's separate `#[...]` array-
    // sigil grammar (a different, unrelated construct). Every statement is
    // `;`-terminated — no bare trailing tail — so the block's own E129 is
    // the ONLY diagnostic (a bare tail would add its own E129 for the
    // still-unrepresentable "blocks-as-values" case, which is not this
    // test's concern; see `let_assign_expr_stmt_shape_matches_ink_temp_decl`
    // for that distinct gap).
    let src = "\
var x = {
  let a = 1;
  a = a + 1;
  log(a);
  if a > 0 {
    log(a);
  } else if a < 0 {
    log(a);
  } else {
    log(a);
  }
  while a < 10 {
    a = a + 1;
  }
  for item in items {
    log(item);
  }
  until done;
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

// ─── #1309: fn/flow declaration bodies reach STMT_BLOCK lowering ───────
//
// Distinct from the var-initializer test above: here `STMT_BLOCK` sits at
// **declaration-body** position, not expression position, so there is no
// "blocks-as-values" gap to work around — a well-formed body lowers with
// ZERO diagnostics, the honest proof that `container::lower_body` really
// dispatches through `body::lower_stmt_block_as_body` on a real `.brink`
// file, not just a differential fixture.

#[test]
fn fn_body_default_reaches_stmt_block_lowering_for_real() {
    // Plain `{ }` on a `fn` is code-ground by default (charter §4).
    let src = "\
fn heal(hp) {
  let bonus = 1;
  if hp > 0 {
    log(hp);
  }
  return hp + bonus;
}
";
    let (hir, _manifest, diags) = lower_native_fixture(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    let knot = &hir.knots[0];
    assert!(knot.is_function);
    let Stmt::LogicBlock(lb) = &knot.body.stmts[0] else {
        panic!(
            "a code-ground body lowers as a single wrapping LogicBlock, got {:?}",
            knot.body.stmts
        );
    };
    assert_eq!(
        block_shape(&lb.stmts),
        vec!["TempDecl", "If", "Return"],
        "shape: {:?}",
        lb.stmts
    );
}

#[test]
fn flow_compound_guard_override_reaches_stmt_block_lowering_for_real() {
    // `~{ }` forces code-ground on a `flow` — charter §3's "Compound
    // guard", the non-default combination #1309 also wires.
    let src = "\
flow guard() ~{
  let ok = true;
  while ok {
    ok = false;
  }
}
";
    let (hir, _manifest, diags) = lower_native_fixture(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.knots.len(), 1);
    let knot = &hir.knots[0];
    assert!(!knot.is_function);
    let Stmt::LogicBlock(lb) = &knot.body.stmts[0] else {
        panic!(
            "a code-ground body lowers as a single wrapping LogicBlock, got {:?}",
            knot.body.stmts
        );
    };
    assert_eq!(block_shape(&lb.stmts), vec!["TempDecl", "While"]);
}

#[test]
fn fn_body_default_shape_matches_ink_fn_body_of_one_logic_block() {
    // The differential-vs-brink-dialect exit criterion (#1309's own
    // correction comment): the same logic authored as a native `fn`'s
    // default code-ground body, and as an ink/brink-dialect container whose
    // entire body is one `~ { … }` block, lowers to the identical
    // `Knot.body` shape modulo provenance — one wrapping `Stmt::LogicBlock`
    // carrying the same `BlockStmt` sequence (`is_function` is orthogonal
    // to this shape, so the ink side stays a plain knot, same as the
    // `let_assign_expr_stmt_shape_matches_ink_temp_decl` precedent below).
    let native_src = "\
fn heal(hp) {
  let bonus = 1;
  hp = hp + bonus;
  log(hp);
}
";
    let ink_src = "\
== test ==
~ {
    temp bonus = 1
    hp = hp + bonus
    log(hp)
}
-> END
";
    let (hir, _manifest, diags) = lower_native_fixture(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Stmt::LogicBlock(native_lb) = &hir.knots[0].body.stmts[0] else {
        panic!(
            "expected a wrapping LogicBlock, got {:?}",
            hir.knots[0].body.stmts
        );
    };
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_lb.stmts), block_shape(&ink_stmts));
    assert_eq!(
        block_shape(&native_lb.stmts),
        vec!["TempDecl", "Assignment", "ExprStmt"]
    );
}

// ─── Shape differential: native vs. brink-dialect T1b `~ { … }` ────────
//
// `lower_native::control_flow::lower_stmt_block` is `pub` (mirrors
// `hir::lower_single_knot`'s "lower one construct in isolation" precedent)
// specifically so tests like these can inspect the `Vec<BlockStmt>` shape
// the pipeline test above can't reach (its result is thrown away, since
// `STMT_BLOCK` has no `Expr` to hold it).

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

#[test]
fn let_assign_expr_stmt_shape_matches_ink_temp_decl() {
    let native_src = "\
var x = {
  let a = 1;
  a = a + 1;
  log(a);
}
";
    let ink_src = "\
== test ==
~ {
    temp a = 1
    a = a + 1
    log(a)
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));
    assert_eq!(native_stmts.len(), 3, "TempDecl, Assignment, ExprStmt");
}

/// A genuine, known cross-frontend asymmetry (not a bug in this PR's
/// scope): a `STMT_BLOCK`'s bare, unterminated trailing expression is its
/// blocks-as-values *tail* — which has no HIR representation yet (no
/// `Expr::Block` exists) — so `lower_stmt_block` skips it with its own
/// E129, dropping it entirely. The brink-dialect's `~ { … }` has no
/// tail/value concept at all (every line is a flat statement), so the
/// identical trailing bare expression there becomes an ordinary
/// `BlockStmt::ExprStmt`. Pinned here so a future blocks-as-values slice
/// has a failing-test signpost to flip, rather than silent drift.
#[test]
fn bare_trailing_tail_is_dropped_on_native_but_kept_as_exprstmt_on_ink() {
    let native_src = "\
var x = {
  let a = 1;
  a
}
";
    let ink_src = "\
== test ==
~ {
    temp a = 1
    a
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert_eq!(diags.len(), 1, "the dropped tail's own E129: {diags:?}");
    assert_eq!(diags[0].code, brink_ir::DiagnosticCode::E129);
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), vec!["TempDecl"]);
    assert_eq!(block_shape(&ink_stmts), vec!["TempDecl", "ExprStmt"]);
}

#[test]
fn if_else_shape_matches_ink_if_stmt() {
    let native_src = "\
var x = {
  if a > 0 {
    log(1);
  } else {
    log(2);
  }
}
";
    let ink_src = "\
== test ==
~ {
    if a > 0 {
        log(1)
    } else {
        log(2)
    }
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::If(native_if) = &native_stmts[0] else {
        panic!("expected If");
    };
    let BlockStmt::If(ink_if) = &ink_stmts[0] else {
        panic!("expected If");
    };
    assert_eq!(native_if.body.len(), ink_if.body.len());
    assert!(matches!(native_if.else_branch, Some(ElseBranch::Else(_))));
    assert!(matches!(ink_if.else_branch, Some(ElseBranch::Else(_))));
    let (Some(ElseBranch::Else(native_else)), Some(ElseBranch::Else(ink_else))) =
        (&native_if.else_branch, &ink_if.else_branch)
    else {
        panic!("expected Else on both sides");
    };
    assert_eq!(native_else.len(), ink_else.len());
}

#[test]
fn else_if_chain_shape_matches_ink_nested_if_stmt() {
    let native_src = "\
var x = {
  if a > 0 {
    log(1);
  } else if a < 0 {
    log(2);
  } else {
    log(3);
  }
}
";
    let ink_src = "\
== test ==
~ {
    if a > 0 {
        log(1)
    } else if a < 0 {
        log(2)
    } else {
        log(3)
    }
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);

    let BlockStmt::If(native_if) = &native_stmts[0] else {
        panic!("expected If");
    };
    let BlockStmt::If(ink_if) = &ink_stmts[0] else {
        panic!("expected If");
    };
    assert!(matches!(native_if.else_branch, Some(ElseBranch::ElseIf(_))));
    assert!(matches!(ink_if.else_branch, Some(ElseBranch::ElseIf(_))));
    let (Some(ElseBranch::ElseIf(native_nested)), Some(ElseBranch::ElseIf(ink_nested))) =
        (&native_if.else_branch, &ink_if.else_branch)
    else {
        panic!("expected ElseIf on both sides");
    };
    assert!(matches!(
        native_nested.else_branch,
        Some(ElseBranch::Else(_))
    ));
    assert!(matches!(ink_nested.else_branch, Some(ElseBranch::Else(_))));
}

#[test]
fn while_shape_matches_ink_while_stmt() {
    let native_src = "\
var x = {
  while a < 10 {
    a = a + 1;
  }
}
";
    let ink_src = "\
== test ==
~ {
    while a < 10 {
        a = a + 1
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
    assert_eq!(native_while.body.len(), ink_while.body.len());
    assert!(
        !native_while.is_await,
        "native has no `while await` form (until replaces await, decision-log item 4)"
    );
    assert!(!ink_while.is_await);
}

#[test]
fn for_shape_matches_ink_for_stmt() {
    let native_src = "\
var x = {
  for item in items {
    log(item);
  }
}
";
    let ink_src = "\
== test ==
~ {
    for item in items {
        log(item)
    }
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::For(native_for) = &native_stmts[0] else {
        panic!("expected For");
    };
    let BlockStmt::For(ink_for) = &ink_stmts[0] else {
        panic!("expected For");
    };
    assert_eq!(native_for.var_name.text, ink_for.var_name.text);
    assert_eq!(native_for.body.len(), ink_for.body.len());
    assert!(
        native_for.val_name.is_none(),
        "single-binding `for` has no second binding"
    );
    assert!(
        ink_for.val_name.is_none(),
        "the ink `~ {{ for … }}` grammar has no two-binding syntax at all — \
         always None, not just unset for this fixture"
    );
}

/// `for k, v in m` — two-binding map iteration (B2, issue #1461,
/// docs/stdlib-spec.md §5/§9's F10 ruling). Native-only: the ink `~ { for
/// … }` T1b grammar has no two-binding spelling, so there is no
/// differential partner here — this pins the native shape directly, the
/// one additive HIR field the B0 fence reserved (`val_name`,
/// docs/b0-sequencing.md:356).
#[test]
fn for_stmt_two_binding_populates_val_name() {
    let native_src = "\
var x = {
  for k, v in m {
    log(k, v);
  }
}
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let BlockStmt::For(native_for) = &native_stmts[0] else {
        panic!("expected For");
    };
    assert_eq!(native_for.var_name.text, "k");
    let val_name = native_for
        .val_name
        .as_ref()
        .expect("two-binding `for` populates val_name");
    assert_eq!(val_name.text, "v");
}

/// `until <cond>;` (native's sole condition-park spelling) lowers to the
/// exact same `AwaitStmt` HIR node ink's `~ await <cond>` produces
/// (decision-log item 4) — the flagship "spelling change, not a new
/// construct" claim, pinned by a real cross-frontend shape comparison.
#[test]
fn until_shape_matches_ink_await_stmt() {
    let native_src = "\
var x = {
  until done;
}
";
    let ink_src = "\
== test ==
~ {
    await done
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::Await(native_await) = &native_stmts[0] else {
        panic!("expected Await");
    };
    let BlockStmt::Await(ink_await) = &ink_stmts[0] else {
        panic!("expected Await");
    };
    assert!(native_await.condition.is_some());
    assert!(ink_await.condition.is_some());
}

#[test]
fn nested_control_flow_shape_matches_ink() {
    let native_src = "\
var x = {
  for i in xs {
    while a {
      if b {
        c = 1;
      } else {
        c = 2;
      }
    }
  }
}
";
    let ink_src = "\
== test ==
~ {
    for i in xs {
        while a {
            if b {
                c = 1
            } else {
                c = 2
            }
        }
    }
}
-> END
";
    let (native_stmts, diags) = native_block_stmts(native_src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let ink_stmts = ink_block_stmts(ink_src);
    assert_eq!(block_shape(&native_stmts), block_shape(&ink_stmts));

    let BlockStmt::For(native_for) = &native_stmts[0] else {
        panic!("expected For");
    };
    let native_while = native_for
        .body
        .iter()
        .find_map(|s| match s {
            BlockStmt::While(w) => Some(w),
            _ => None,
        })
        .expect("nested While");
    let native_if = native_while
        .body
        .iter()
        .find_map(|s| match s {
            BlockStmt::If(i) => Some(i),
            _ => None,
        })
        .expect("nested If");
    assert!(matches!(native_if.else_branch, Some(ElseBranch::Else(_))));
}
