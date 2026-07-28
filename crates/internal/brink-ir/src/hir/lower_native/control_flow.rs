//! B0.8 Wave B: the code-ground control-flow layer (`if`/`else`, `while`,
//! `for … in …`, `until`) plus the `let`/assignment/expression-statement
//! substrate Wave A's grammar (`brink-syntax-native::parser::stmt`) already
//! produces (`docs/decision-log.md` 2026-07-23 "Code-ground sitting",
//! issue #1177).
//!
//! Lowers to the **existing** `~ { … }` T1b closed statement set —
//! `BlockStmt`/`IfStmt`/`WhileStmt`/`ForStmt`/`AwaitStmt`
//! (`crate::hir::types`) — the NF-2 fence: no new HIR nodes. This module
//! mirrors `hir::lower::content::logic_block` structurally (same target
//! shape, same per-statement dispatch table) — that module is this one's
//! differential partner
//! (`crates/internal/brink-ir/tests/b08_native_control_flow.rs`,
//! `crates/internal/brink-ir/tests/b08_native_wave_b_tail.rs`).
//!
//! `until <cond>;` is native's condition-park spelling; it lowers to the
//! SAME `AwaitStmt` node the brink-dialect's `~ await <cond>` produces
//! (decision-log item 4: "`until` is written; suspend is inferred... the
//! runtime's `FlowSleep` reactive-wake already implements" it) — a
//! spelling change, not a new construct.
//!
//! B0.8 Wave B *tail* (issue #1322, "B0.8 Wave B TAIL: native code-body
//! statement forms #1177 didn't cover") fills in the rest of the ruled
//! surface: `return e` (→ `BlockStmt::Return`), `break`/`continue` (→
//! `BlockStmt::Break`/`Continue`), and compound/RMW assignment (`x += e`,
//! `x.field += e` → `Assignment { op: AssignOp::Add, .. }`) — see
//! `lower_return_stmt`/`lower_block_item`'s `BREAK_STMT`/`CONTINUE_STMT`
//! arms and `lower_assignment` below. Blocks-as-values (a `STMT_BLOCK`'s
//! own value when reached in expression position) and `#fn` remain out of
//! this slice — see `expr::lower_expr`'s `STMT_BLOCK` arm doc and `expr`'s
//! module doc, respectively. UFCS calls lower structurally here like any
//! other call; their resolution is `brink-analyzer::ufcs`' (issue #1482).

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::SyntaxNode;
use brink_syntax_native::ast::{self, AstNode as _};

use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{
    AssignOp, Assignment, AwaitStmt, BlockStmt, Diagnostic, DiagnosticCode, ElseBranch, ForStmt,
    IfStmt, Name, Return, ReturnKind, TempDecl, WhileStmt,
};

use super::expr::lower_expr;
use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

fn name_from(tok: Option<brink_syntax_native::SyntaxToken>) -> Option<Name> {
    tok.map(|t| Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// Lower an `as NAME` binding (B1b, issue #1475) and enforce the v1
/// whole-condition restriction on the expression it binds.
///
/// The parser already refuses an operator *after* the binding
/// (`brink-syntax-native::parser::binding::reject_composition`); what only
/// this layer can see is a binding sitting on top of a `&&`/`||`
/// composition (`if a && find(x) as s { … }`), because the head expression
/// parses fine and only its lowered *shape* gives the intent away. Both
/// halves of the ruling's "composition with `&&`/`||` is a parse/analysis
/// error" are therefore covered, each where it is detectable.
///
/// A rejected composition yields `None` — the construct lowers as an
/// ordinary unbound `if`/`while`, which the F27 condition check then judges
/// on its own terms. `E145` is Error-severity, so nothing reaches codegen
/// either way.
pub(super) fn lower_as_binding(
    file_id: FileId,
    binding: Option<&ast::AsBinding>,
    condition: &crate::Expr,
    diags: &mut Vec<Diagnostic>,
) -> Option<Name> {
    let binding = binding?;
    if let crate::Expr::Infix(ie) = condition
        && matches!(ie.op, crate::InfixOp::And | crate::InfixOp::Or)
    {
        diags.push(diag(
            file_id,
            binding.syntax().text_range(),
            DiagnosticCode::E145,
        ));
        return None;
    }
    name_from(binding.name_token())
}

/// Lower a `STMT_BLOCK`'s statements to the T1b closed set. Any child that
/// isn't one of the seven recognized statement kinds — the block's own
/// blocks-as-values tail, or a malformed item the parser already
/// error-recovered — is skipped with an `E129` diagnostic, matching
/// `expr::lower_expr`'s "loud, not a silent drop" convention (CLAUDE.md:
/// "flag silent data drops").
///
/// `pub`, mirroring [`crate::hir::lower_single_knot`]'s precedent: a small,
/// self-contained lowering entry point for callers (differential tests,
/// tooling) that need to lower one code-ground block in isolation, without
/// a full source file or a value-producing home for the block itself
/// (blocks-as-values isn't representable yet — see `expr::lower_expr`'s
/// `STMT_BLOCK` arm). The production call site is that same arm, reached
/// from any real `.brink` file via a `var`/`const` initializer.
pub fn lower_stmt_block(
    file_id: FileId,
    block: &ast::StmtBlock,
    diags: &mut Vec<Diagnostic>,
) -> Vec<BlockStmt> {
    block
        .items()
        .filter_map(|item| lower_block_item(file_id, &item, diags))
        .collect()
}

/// Lower only a `STMT_BLOCK`'s **statements**, leaving its blocks-as-values
/// tail expression to the caller (issue #1685).
///
/// The one caller is [`super::lambda::lower_lambda`]: a lambda's braced body
/// is the one place in the grammar where the tail has a real home — it is
/// the lambda's value ("last expression is the value", RULED 2026-07-19),
/// lowered into `LambdaBody::Block { stmts, tail }`. Everywhere else a tail
/// still has nowhere to live, which is why [`lower_stmt_block`] keeps
/// routing it through the `E129` "loud, not a silent drop" arm.
pub(super) fn lower_stmt_block_stmts(
    file_id: FileId,
    block: &ast::StmtBlock,
    diags: &mut Vec<Diagnostic>,
) -> Vec<BlockStmt> {
    let tail = block.tail();
    block
        .items()
        .filter(|item| Some(item) != tail.as_ref())
        .filter_map(|item| lower_block_item(file_id, &item, diags))
        .collect()
}

fn lower_block_item(
    file_id: FileId,
    item: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<BlockStmt> {
    match item.kind() {
        N::LET_STMT => {
            ast::LetStmt::cast(item.clone()).and_then(|n| lower_temp_decl(file_id, &n, diags))
        }
        N::ASSIGN_STMT => {
            ast::AssignStmt::cast(item.clone()).and_then(|n| lower_assignment(file_id, &n, diags))
        }
        N::EXPR_STMT => {
            ast::ExprStmt::cast(item.clone()).and_then(|n| lower_expr_stmt(file_id, &n, diags))
        }
        N::IF_STMT => ast::IfStmt::cast(item.clone())
            .and_then(|n| lower_if_stmt(file_id, &n, diags).map(BlockStmt::If)),
        N::WHILE_STMT => ast::WhileStmt::cast(item.clone())
            .and_then(|n| lower_while_stmt(file_id, &n, diags).map(BlockStmt::While)),
        N::FOR_STMT => ast::ForStmt::cast(item.clone())
            .and_then(|n| lower_for_stmt(file_id, &n, diags).map(BlockStmt::For)),
        N::UNTIL_STMT => ast::UntilStmt::cast(item.clone())
            .map(|n| BlockStmt::Await(lower_until_stmt(file_id, &n, diags))),
        N::RETURN_STMT => ast::ReturnStmt::cast(item.clone())
            .map(|n| BlockStmt::Return(lower_return_stmt(file_id, &n, diags))),
        N::BREAK_STMT => ast::BreakStmt::cast(item.clone())
            .map(|n| BlockStmt::Break(native_provenance(file_id, NodeClass::Break, n.syntax()))),
        N::CONTINUE_STMT => ast::ContinueStmt::cast(item.clone()).map(|n| {
            BlockStmt::Continue(native_provenance(file_id, NodeClass::Continue, n.syntax()))
        }),
        _ => {
            diags.push(diag(file_id, item.text_range(), DiagnosticCode::E129));
            None
        }
    }
}

/// `let name (: type)? (= expr)?;` — mirrors
/// `logic_block::lower_block_temp_decl`'s diagnostic choice (E014) for the
/// missing-name case, for differential symmetry on malformed input too, not
/// just well-formed shapes.
///
/// The annotation (NG-B, issue #1488) lowers to the same `hir::TypeExpr`
/// the ink dialect's `~ temp x: int = …` ascription produces, so
/// `brink-analyzer::strict`'s temp firewall (`collect_temps` →
/// `annotations::resolve`) exempts an annotated native `let` from `E065`
/// with no analyzer change at all.
fn lower_temp_decl(
    file_id: FileId,
    temp: &ast::LetStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<BlockStmt> {
    let range = temp.syntax().text_range();
    let Some(name) = name_from(temp.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E014));
        return None;
    };
    let value = temp.value().map(|v| lower_expr(file_id, &v, diags));
    Some(BlockStmt::TempDecl(TempDecl {
        ptr: native_provenance(file_id, NodeClass::TempDecl, temp.syntax()),
        name,
        value,
        annotation: temp
            .type_annotation()
            .as_ref()
            .and_then(super::types::lower_type_annotation),
    }))
}

/// `place = expr;` / `place += expr;` / `place -= expr;` — the place is
/// always a dotted `PATH` (no `::`, `AssignStmt::place`'s doc), lowered to
/// `Expr::Path` as the assignment target (mirrors ink's
/// `Assignment.target: Expr` shape). `op` mirrors the brink-dialect's own
/// `logic_block::lower_block_assignment` token-to-`AssignOp` mapping
/// exactly (B0.8 Wave B tail, issue #1322: "compound/RMW assignment") —
/// `PLUS_EQ`/`MINUS_EQ` map to `Add`/`Sub`, anything else (a bare `=`, or a
/// malformed parse `assign_stmt`'s `expect(EQ)` fallback already
/// diagnosed) falls back to `Set`.
fn lower_assignment(
    file_id: FileId,
    assign: &ast::AssignStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<BlockStmt> {
    let range = assign.syntax().text_range();
    let Some(place) = assign.place() else {
        diags.push(diag(file_id, range, DiagnosticCode::E014));
        return None;
    };
    let Some(value_node) = assign.value() else {
        diags.push(diag(file_id, range, DiagnosticCode::E014));
        return None;
    };
    let target = crate::Expr::Path(super::expr::lower_path(&place));
    let value = lower_expr(file_id, &value_node, diags);
    let op = assign
        .op_token()
        .map_or(AssignOp::Set, |tok| match tok.kind() {
            N::PLUS_EQ => AssignOp::Add,
            N::MINUS_EQ => AssignOp::Sub,
            _ => AssignOp::Set,
        });
    Some(BlockStmt::Assignment(Assignment {
        ptr: native_provenance(file_id, NodeClass::Assignment, assign.syntax()),
        target,
        op,
        value,
    }))
}

fn lower_expr_stmt(
    file_id: FileId,
    stmt: &ast::ExprStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<BlockStmt> {
    let range = stmt.syntax().text_range();
    let Some(expr_node) = stmt.expr() else {
        diags.push(diag(file_id, range, DiagnosticCode::E015));
        return None;
    };
    Some(BlockStmt::ExprStmt(lower_expr(file_id, &expr_node, diags)))
}

/// Mirrors `logic_block::lower_if_stmt`'s error posture exactly: a missing
/// condition (E015) drops the whole `IfStmt` (propagated via `?` through
/// `lower_else_clause`/`lower_block_item`'s `.and_then`), not a
/// half-lowered placeholder — same "whole malformed statement, not a
/// patched-up one" policy the differential partner uses.
fn lower_if_stmt(
    file_id: FileId,
    if_stmt: &ast::IfStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<IfStmt> {
    let range = if_stmt.syntax().text_range();
    let Some(cond_node) = if_stmt.condition() else {
        diags.push(diag(file_id, range, DiagnosticCode::E015));
        return None;
    };
    let condition = lower_expr(file_id, &cond_node, diags);
    let binding = lower_as_binding(file_id, if_stmt.as_binding().as_ref(), &condition, diags);
    let body = if_stmt
        .body()
        .map(|b| lower_stmt_block(file_id, &b, diags))
        .unwrap_or_default();
    let else_branch = match if_stmt.else_clause() {
        None => None,
        Some(clause) => Some(lower_else_clause(file_id, &clause, diags)?),
    };
    Some(IfStmt {
        ptr: native_provenance(file_id, NodeClass::If, if_stmt.syntax()),
        condition,
        binding,
        body,
        else_branch,
    })
}

fn lower_else_clause(
    file_id: FileId,
    clause: &ast::ElseClause,
    diags: &mut Vec<Diagnostic>,
) -> Option<ElseBranch> {
    if let Some(nested_if) = clause.if_stmt() {
        Some(ElseBranch::ElseIf(Box::new(lower_if_stmt(
            file_id, &nested_if, diags,
        )?)))
    } else {
        let body = clause
            .body()
            .map(|b| lower_stmt_block(file_id, &b, diags))
            .unwrap_or_default();
        Some(ElseBranch::Else(body))
    }
}

fn lower_while_stmt(
    file_id: FileId,
    w: &ast::WhileStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<WhileStmt> {
    let range = w.syntax().text_range();
    let Some(cond_node) = w.condition() else {
        diags.push(diag(file_id, range, DiagnosticCode::E015));
        return None;
    };
    let condition = lower_expr(file_id, &cond_node, diags);
    let binding = lower_as_binding(file_id, w.as_binding().as_ref(), &condition, diags);
    let body = w
        .body()
        .map(|b| lower_stmt_block(file_id, &b, diags))
        .unwrap_or_default();
    Some(WhileStmt {
        ptr: native_provenance(file_id, NodeClass::While, w.syntax()),
        condition,
        binding,
        body,
        // Native has no `await` keyword to spell a persistent-await
        // variant with (retired, decision-log item 4) — always a plain
        // loop.
        is_await: false,
    })
}

#[expect(
    clippy::similar_names,
    reason = "var_name/val_name are the ForStmt field names (k/v's HIR spelling, B2 #1461) — \
              not a pair a rename would clarify"
)]
fn lower_for_stmt(
    file_id: FileId,
    f: &ast::ForStmt,
    diags: &mut Vec<Diagnostic>,
) -> Option<ForStmt> {
    let range = f.syntax().text_range();
    let Some(var_name) = name_from(f.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E014));
        return None;
    };
    // Two-binding map iteration (`for k, v in m`, B2 #1461): the second
    // `IDENT` after the comma, when the grammar admitted one. `None` for
    // the ordinary single-binding form.
    let val_name = name_from(f.val_name_token());
    let Some(iterable_node) = f.iterable() else {
        diags.push(diag(file_id, range, DiagnosticCode::E015));
        return None;
    };
    let iterable = lower_expr(file_id, &iterable_node, diags);
    let body = f
        .body()
        .map(|b| lower_stmt_block(file_id, &b, diags))
        .unwrap_or_default();
    Some(ForStmt {
        ptr: native_provenance(file_id, NodeClass::For, f.syntax()),
        var_name,
        val_name,
        iterable,
        body,
    })
}

/// `until <cond>;` → `AwaitStmt` (see module doc). No HIR-level diagnostic
/// for a missing condition — same posture as the brink-dialect's own
/// `AwaitStmt.condition: Option<Expr>` (`hir/lower/content/logic_block.rs::
/// lower_await_stmt`): the parser's own `p.error` on `until_stmt` already
/// covers a malformed `until` with no condition expression.
fn lower_until_stmt(file_id: FileId, u: &ast::UntilStmt, diags: &mut Vec<Diagnostic>) -> AwaitStmt {
    let condition = u.condition().map(|n| lower_expr(file_id, &n, diags));
    AwaitStmt {
        ptr: native_provenance(file_id, NodeClass::Await, u.syntax()),
        condition,
    }
}

/// `return expr?;` → `Return` (B0.8 Wave B tail, issue #1322). Mirrors the
/// brink-dialect's own `logic_block::lower_block_return` exactly: always
/// `ReturnKind::Explicit` (native's code-ground `return` has no
/// tunnel-redirect counterpart — `parser/stmt.rs::return_stmt`'s doc), no
/// `onwards_args` (that field is tunnel-redirect-only). No HIR-level
/// diagnostic for a missing value — same posture as [`lower_until_stmt`]:
/// the value is genuinely optional (`return;` is legal, mirrors ink's own
/// `Return.value: Option<Expr>`), not a malformed-parse signal.
fn lower_return_stmt(
    file_id: FileId,
    ret: &ast::ReturnStmt,
    diags: &mut Vec<Diagnostic>,
) -> Return {
    let value = ret.value().map(|n| lower_expr(file_id, &n, diags));
    Return {
        ptr: Some(native_provenance(file_id, NodeClass::Return, ret.syntax())),
        kind: ReturnKind::Explicit,
        value,
        onwards_args: Vec::new(),
    }
}
