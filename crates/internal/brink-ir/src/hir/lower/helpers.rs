//! Pure helper functions for HIR lowering.
//!
//! These are stateless transformations — no diagnostics, no side effects.

use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode};
use rowan::TextRange;

use crate::{ContentPart, Expr, InfixOp, Name, Path, PostfixOp, PrefixOp, StringExpr, StringPart};

use super::context::{LowerScope, LowerSink};
use super::expr::LowerExpr;

pub fn make_name(text: impl Into<String>, range: TextRange) -> Name {
    Name {
        text: text.into(),
        range,
    }
}

pub fn name_from_ident(ident: &ast::Identifier) -> Option<Name> {
    let text = ident.name()?;
    Some(make_name(text, ident.syntax().text_range()))
}

pub fn lower_path(path: &ast::Path) -> Path {
    let segments: Vec<Name> = path
        .segments()
        .map(|tok| make_name(tok.text().to_string(), tok.text_range()))
        .collect();
    Path {
        segments,
        range: path.syntax().text_range(),
    }
}

pub fn path_full_name(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

pub fn lower_prefix_op(pe: &ast::PrefixExpr) -> Option<PrefixOp> {
    let tok = pe.op_token()?;
    match tok.kind() {
        SyntaxKind::MINUS => Some(PrefixOp::Negate),
        SyntaxKind::BANG | SyntaxKind::KW_NOT => Some(PrefixOp::Not),
        _ => None,
    }
}

pub fn lower_infix_op(ie: &ast::InfixExpr) -> Option<InfixOp> {
    let tok = ie.op_token()?;
    match tok.kind() {
        SyntaxKind::PLUS => Some(InfixOp::Add),
        SyntaxKind::MINUS => Some(InfixOp::Sub),
        SyntaxKind::STAR => Some(InfixOp::Mul),
        SyntaxKind::SLASH => Some(InfixOp::Div),
        SyntaxKind::PERCENT | SyntaxKind::KW_MOD => Some(InfixOp::Mod),
        SyntaxKind::CARET => Some(InfixOp::Intersect),
        SyntaxKind::EQ_EQ => Some(InfixOp::Eq),
        SyntaxKind::BANG_EQ => Some(InfixOp::NotEq),
        SyntaxKind::LT => Some(InfixOp::Lt),
        SyntaxKind::GT => Some(InfixOp::Gt),
        SyntaxKind::LT_EQ => Some(InfixOp::LtEq),
        SyntaxKind::GT_EQ => Some(InfixOp::GtEq),
        SyntaxKind::KW_AND | SyntaxKind::AMP_AMP => Some(InfixOp::And),
        SyntaxKind::KW_OR | SyntaxKind::PIPE => Some(InfixOp::Or),
        SyntaxKind::KW_HAS | SyntaxKind::QUESTION => Some(InfixOp::Has),
        SyntaxKind::KW_HASNT | SyntaxKind::BANG_QUESTION => Some(InfixOp::HasNot),
        _ => None,
    }
}

pub fn lower_postfix_op(pe: &ast::PostfixExpr) -> Option<PostfixOp> {
    let tok = pe.op_token()?;
    match tok.kind() {
        SyntaxKind::PLUS => Some(PostfixOp::Increment),
        SyntaxKind::MINUS => Some(PostfixOp::Decrement),
        _ => None,
    }
}

pub fn content_ends_with_glue(parts: &[ContentPart]) -> bool {
    matches!(parts.last(), Some(ContentPart::Glue))
}

/// Returns true if the expression tree contains a function call.
pub fn expr_contains_call(expr: &Expr) -> bool {
    match expr {
        Expr::Call(..) => true,
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => expr_contains_call(inner),
        Expr::Infix(lhs, _, rhs) => expr_contains_call(lhs) || expr_contains_call(rhs),
        Expr::String(s) => s
            .parts
            .iter()
            .any(|p| matches!(p, StringPart::Interpolation(e) if expr_contains_call(e))),
        _ => false,
    }
}

/// Lower a string literal, handling interpolations via `LowerExpr`.
pub fn lower_string_lit(
    lit: &ast::StringLit,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> StringExpr {
    let mut parts = Vec::new();
    for child in lit.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) if tok.kind() != SyntaxKind::QUOTE => {
                let text = tok.text().to_string();
                if !text.is_empty() {
                    parts.push(StringPart::Literal(text));
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if let Some(inline) = ast::InlineLogic::cast(node)
                    && let Some(expr) = inline
                        .inner_expression()
                        .and_then(|inner| inner.expr())
                        .and_then(|e| e.lower_expr(scope, sink).ok())
                {
                    parts.push(StringPart::Interpolation(Box::new(expr)));
                }
            }
            rowan::NodeOrToken::Token(_) => {}
        }
    }
    StringExpr { parts }
}
