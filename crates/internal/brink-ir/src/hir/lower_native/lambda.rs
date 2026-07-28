//! `LAMBDA_EXPR` → [`Expr::Lambda`] — the native surface's anonymous fn
//! value (issue #1685).
//!
//! The surface is Rust pipes with colon returns, RULED 2026-07-19
//! (`docs/decision-log.md`, "Lambdas ruled: Rust pipes under the `RustScript`
//! north star"): `|g| g.awake` · `|g: Guest|: bool { … }` · `||`. What that
//! ruling fixes, and where each half lands:
//!
//! - **Params optionally annotated** — the annotation is lowered when
//!   written and left `None` otherwise (mono-HM infers at concrete call
//!   sites); the parser reuses the declaration grammar's `PARAM` node, so
//!   [`lower_lambda_params`] is [`super::container::lower_params`]'s twin
//!   over a `LAMBDA_PARAMS` row.
//! - **Colon return spelling** — `|g|: bool { … }`, never `->` (one arrow,
//!   one meaning: a divert). The parser already places it; here it is just
//!   the one `TYPE_ANNOTATION` directly under the `LAMBDA_EXPR`.
//! - **Single-expression or braced-block bodies; last expression is the
//!   value** — the grammar's own blocks-as-values tail
//!   (`ast::StmtBlock::tail`) *is* that last expression, so
//!   [`crate::LambdaBody::Block`] records statements and tail separately
//!   rather than leaving the value implicit. Note the block body does **not**
//!   go through `expr::lower_expr`'s `STMT_BLOCK` arm (which is still the
//!   honest E129 blocks-as-values fence): a lambda's braced body is a
//!   *function body*, not a block used as a value, and it has a real home.
//! - **`return` leaves the lambda** — a `RETURN_STMT` inside the body
//!   lowers to the same `BlockStmt::Return` any code-ground block produces;
//!   *which* frame it returns from is a resolution fact for the layer that
//!   gives the lambda a frame, not a shape this lowering can express.
//! - **Capture is by value, always** (Rust `move` as the only mode, no
//!   keyword, no ref captures in v1), so there is no capture *mode* to
//!   record. The one capture rule that is decidable lexically —
//!   **assignment to a captured binding is a compile error**, because a
//!   snapshot write is always a lost write — is [`check_capture_writes`]
//!   below (`E156`).
//! - **Lambdas are fn-colored always** and rows compose through captures
//!   (#872), but `Ty::Fn` carries no effect rows at all today (#1680) — so
//!   nothing here records one. See issue #1685's coordination note.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use crate::hir::FileId;
use crate::provenance::NodeClass;
use crate::{Diagnostic, DiagnosticCode, Expr, LambdaBody, LambdaExpr, Name, Param};

use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

fn name_from(tok: Option<SyntaxToken>) -> Option<Name> {
    tok.map(|t| Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// Lower one `LAMBDA_EXPR` node.
///
/// Never fails outright: a malformed node (no params row, no body — both
/// only reachable through parser error recovery) reports `E015` and yields
/// `Expr::Null`, the same "well-formed HIR tree to hang the diagnostic off
/// of" contract [`super::expr::lower_expr`] has for every other shape.
pub(super) fn lower_lambda(
    file_id: FileId,
    node: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Expr {
    let Some(lambda) = ast::LambdaExpr::cast(node.clone()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let Some(body_node) = lambda.body() else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };

    let params = lower_lambda_params(lambda.params().as_ref());
    let return_type = lambda
        .return_annotation()
        .as_ref()
        .and_then(super::types::lower_type_annotation);

    let body = if let Some(block) = ast::StmtBlock::cast(body_node.clone()) {
        let tail = block.tail();
        let stmts = super::control_flow::lower_stmt_block_stmts(file_id, &block, diags);
        LambdaBody::Block {
            stmts,
            tail: tail.map(|t| Box::new(super::expr::lower_expr(file_id, &t, diags))),
        }
    } else {
        LambdaBody::Expr(Box::new(super::expr::lower_expr(
            file_id, &body_node, diags,
        )))
    };

    check_capture_writes(file_id, &lambda, diags);

    Expr::Lambda(Box::new(LambdaExpr {
        ptr: native_provenance(file_id, NodeClass::Lambda, node),
        params,
        return_type,
        body,
    }))
}

/// Lower a `LAMBDA_PARAMS` row to the HIR [`Param`] shape.
///
/// `is_ref`/`is_divert` are always `false`: `parser/expr.rs::lambda_param`
/// accepts neither `ref` (no ref captures, RULED 2026-07-19) nor a
/// divert-typed parameter.
fn lower_lambda_params(params: Option<&ast::LambdaParams>) -> Vec<Param> {
    params
        .into_iter()
        .flat_map(|row| row.params().collect::<Vec<_>>())
        .filter_map(|p| {
            name_from(p.name_token()).map(|name| Param {
                name,
                is_ref: false,
                is_divert: false,
                annotation: p
                    .type_annotation()
                    .as_ref()
                    .and_then(super::types::lower_type_annotation),
            })
        })
        .collect()
}

/// Enforce "assignment to a captured binding is a compile error" (`E156`,
/// RULED 2026-07-19) over one lambda's body.
///
/// The rule is decided **lexically**, which is exactly as far as this layer
/// can see and — usefully — exactly as far as it needs to:
///
/// - names bound *inside* the lambda (its own params, and any `let`/`for`/
///   `as` binding in its body) are ordinary locals; writing to them is
///   fine;
/// - names bound *outside* the lambda but still inside some enclosing
///   binder (an enclosing `fn`/`flow` param, an enclosing `let`/`for`/`as`
///   binding, an enclosing lambda's param) are **captured**; writing to
///   them is `E156`;
/// - every other name — most importantly a module-level `var` global — is
///   not a capture at all and is left alone. Globals are durable cells
///   reached by name, not snapshotted bindings, so a write to one is a real
///   write. That is also why this check never needs cross-file resolution:
///   anything it cannot see lexically is, by construction, not a capture.
///
/// Assignments nested inside a *deeper* lambda are skipped here — that
/// lambda's own lowering checks them, with its own (correctly nested) view
/// of what is inner and what is captured.
fn check_capture_writes(file_id: FileId, lambda: &ast::LambdaExpr, diags: &mut Vec<Diagnostic>) {
    let node = lambda.syntax();
    let inner = inner_binders(lambda);
    let outer = outer_binders(node);

    for assign in node
        .descendants()
        .filter(|n| n.kind() == N::ASSIGN_STMT)
        .filter(|n| nearest_lambda(n).as_ref() == Some(node))
    {
        let Some(root) = ast::AssignStmt::cast(assign.clone())
            .and_then(|a| a.place())
            .and_then(|p| p.segments().next())
        else {
            continue;
        };
        let text = root.text().to_string();
        if inner.contains(&text) {
            continue;
        }
        if outer.contains(&text) {
            diags.push(diag(file_id, assign.text_range(), DiagnosticCode::E156));
        }
    }
}

/// The nearest enclosing `LAMBDA_EXPR` of `node` (itself excluded).
fn nearest_lambda(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors()
        .skip(1)
        .find(|a| a.kind() == N::LAMBDA_EXPR)
}

/// Every name bound anywhere inside `lambda` — its params plus every
/// `let`/`for`/`as` binding in its body, including ones inside nested
/// lambdas (over-inclusive on purpose: a false *negative* on this rule
/// costs a missed diagnostic, a false positive rejects legal code).
fn inner_binders(lambda: &ast::LambdaExpr) -> Vec<String> {
    let mut names: Vec<String> = lambda
        .params()
        .into_iter()
        .flat_map(|row| row.params().collect::<Vec<_>>())
        .filter_map(|p| p.name_token().map(|t| t.text().to_string()))
        .collect();
    for node in lambda.syntax().descendants() {
        collect_binder_names(&node, &mut names);
    }
    names
}

/// Every name bound by an *ancestor* of the lambda — the enclosing
/// function's params, and every `let`/`for`/`as`/enclosing-lambda binding
/// in scope around it. These are the names a write inside the lambda would
/// lose.
fn outer_binders(lambda_node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    for ancestor in lambda_node.ancestors().skip(1) {
        match ancestor.kind() {
            N::FN_DECL | N::FLOW_DECL => {
                names.extend(
                    ancestor
                        .children()
                        .filter(|c| c.kind() == N::PARAM_LIST)
                        .flat_map(|pl| pl.children().collect::<Vec<_>>())
                        .filter_map(ast::Param::cast)
                        .filter_map(|p| p.name_token().map(|t| t.text().to_string())),
                );
            }
            N::LAMBDA_EXPR => {
                names.extend(
                    ast::LambdaExpr::cast(ancestor.clone())
                        .and_then(|l| l.params())
                        .into_iter()
                        .flat_map(|row| row.params().collect::<Vec<_>>())
                        .filter_map(|p| p.name_token().map(|t| t.text().to_string())),
                );
            }
            N::STMT_BLOCK
            | N::IF_STMT
            | N::WHILE_STMT
            | N::UNTIL_STMT
            | N::CONDITIONAL_BLOCK
            | N::CHOICE_GUARD => {
                // `AS_BINDING` is parsed as a trailing sibling of the
                // condition head, inside the construct that binds it
                // (`parser::binding::as_binding`'s doc comment) — never an
                // ancestor of its own, so it only surfaces by scanning
                // these constructs' direct children rather than by
                // matching on `ancestor.kind()` itself.
                for child in ancestor.children() {
                    collect_binder_names(&child, &mut names);
                }
            }
            _ => collect_binder_names(&ancestor, &mut names),
        }
    }
    names
}

/// Push the names `node` itself binds (`let x`, `for k, v in …`,
/// `… as name`), if any.
fn collect_binder_names(node: &SyntaxNode, out: &mut Vec<String>) {
    match node.kind() {
        N::LET_STMT => out.extend(
            ast::LetStmt::cast(node.clone())
                .and_then(|s| s.name_token())
                .map(|t| t.text().to_string()),
        ),
        N::FOR_STMT => {
            if let Some(f) = ast::ForStmt::cast(node.clone()) {
                out.extend(f.name_token().map(|t| t.text().to_string()));
                out.extend(f.val_name_token().map(|t| t.text().to_string()));
            }
        }
        N::AS_BINDING => out.extend(
            ast::AsBinding::cast(node.clone())
                .and_then(|b| b.name_token())
                .map(|t| t.text().to_string()),
        ),
        _ => {}
    }
}
