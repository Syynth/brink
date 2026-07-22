//! Lowering for the native surface's minimal expression grammar
//! (`brink-syntax-native`'s `expr.rs`: literals, paths, calls, paren
//! grouping, prefix/infix operators, string interpolation).
//!
//! This is declaration-*head* territory, not body-dialect lowering: `var`/
//! `const` initializers, struct field types, and annotation arguments are
//! expressions the B0.5 grammar already gives real internal shape (not a
//! balanced-token blob) specifically so B0.6 can lower them — see
//! `brink-syntax-native/src/parser/expr.rs`'s module doc ("this is the
//! expression *skeleton* B0.5 needs to give those constructs a real
//! internal shape instead of a balanced-token blob"). Real code-dialect
//! *statement* grammar (`let`/assign/if-stmt/while/for/UFCS-calls) stays
//! out of scope — B0.8.
//!
//! `LAMBDA_EXPR` is tokenized/parsed (B0.5) but explicitly unlowered until
//! the code sitting rules a real anonymous-body node (`docs/b0-sequencing.md`
//! §3: "B0.5 tokenizes pipes; B0.8 does not lower them") — encountering one
//! here is E129, not a silent `Expr::Null`.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use crate::hir::FileId;
use crate::{Diagnostic, DiagnosticCode, Expr, FloatBits, InfixOp, Name, Path, PrefixOp};
use crate::{StringExpr, StringPart};

/// Lower one expression-grammar node to an [`Expr`]. Never fails outright —
/// an unrecognized/unsupported node shape pushes E129 and returns a `Null`
/// placeholder so the caller (a `var`/`const` initializer, a struct field
/// default, …) still gets a well-formed HIR tree to hang the diagnostic off
/// of, rather than a lowering that can't produce a value at all.
pub(super) fn lower_expr(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    match node.kind() {
        N::INTEGER_LIT => {
            let lit = ast::IntegerLit::cast(node.clone()).and_then(|n| n.value());
            if let Some(v) = lit {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "brink integers are 32-bit, mirrors ink's IntegerLit lowering"
                )]
                Expr::Int(v as i32)
            } else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
                Expr::Int(0)
            }
        }
        N::FLOAT_LIT => {
            let lit = ast::FloatLit::cast(node.clone()).and_then(|n| n.value());
            if let Some(v) = lit {
                Expr::Float(FloatBits::from_f64(v))
            } else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
                Expr::Float(FloatBits::from_f64(0.0))
            }
        }
        N::BOOLEAN_LIT => {
            let lit = ast::BooleanLit::cast(node.clone()).and_then(|n| n.value());
            if let Some(v) = lit {
                Expr::Bool(v)
            } else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
                Expr::Bool(false)
            }
        }
        N::STRING_LIT => lower_string_lit(file_id, node, diags),
        N::PATH_EXPR => {
            let path = ast::PathExpr::cast(node.clone()).and_then(|n| n.path());
            if let Some(p) = path {
                Expr::Path(lower_path(&p))
            } else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
                Expr::Null
            }
        }
        N::PAREN_EXPR => {
            let inner = ast::ParenExpr::cast(node.clone()).and_then(|n| n.inner());
            if let Some(inner_node) = inner {
                lower_expr(file_id, &inner_node, diags)
            } else {
                diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
                Expr::Null
            }
        }
        N::PREFIX_EXPR => lower_prefix(file_id, node, diags),
        N::INFIX_EXPR => lower_infix(file_id, node, diags),
        N::CALL_EXPR => lower_call(file_id, node, diags),
        _ => {
            // LAMBDA_EXPR and anything else the expr grammar can produce
            // that this slice doesn't recognize (e.g. a malformed ERROR
            // node reaching here) — loud, not a silent Null with no trace.
            diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
            Expr::Null
        }
    }
}

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Lower a dotted native `Path` node to the HIR `Path` shape.
pub(super) fn lower_path(path: &ast::Path) -> Path {
    let range = path.syntax().text_range();
    let segments: Vec<Name> = path
        .segments()
        .map(|t| Name {
            text: t.text().to_string(),
            range: t.text_range(),
        })
        .collect();
    Path { segments, range }
}

fn lower_prefix(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    let Some(prefix) = ast::PrefixExpr::cast(node.clone()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let op = prefix.op_token().as_ref().and_then(prefix_op);
    let Some(operand) = prefix.operand() else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let inner = lower_expr(file_id, &operand, diags);
    if let Some(op) = op {
        Expr::Prefix(op, Box::new(inner))
    } else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E016));
        inner
    }
}

fn prefix_op(tok: &SyntaxToken) -> Option<PrefixOp> {
    match tok.kind() {
        N::MINUS => Some(PrefixOp::Negate),
        N::BANG => Some(PrefixOp::Not),
        _ => None,
    }
}

fn lower_infix(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    let Some(infix) = ast::InfixExpr::cast(node.clone()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let (Some(lhs_node), Some(rhs_node)) = (infix.lhs(), infix.rhs()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let lhs = lower_expr(file_id, &lhs_node, diags);
    let rhs = lower_expr(file_id, &rhs_node, diags);

    let op = if infix.is_double_pipe() {
        Some(InfixOp::Or)
    } else {
        infix.op_token().as_ref().and_then(infix_op)
    };

    if let Some(op) = op {
        Expr::Infix(Box::new(lhs), op, Box::new(rhs))
    } else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E016));
        lhs
    }
}

fn infix_op(tok: &SyntaxToken) -> Option<InfixOp> {
    match tok.kind() {
        N::PLUS => Some(InfixOp::Add),
        N::MINUS => Some(InfixOp::Sub),
        N::STAR => Some(InfixOp::Mul),
        N::SLASH => Some(InfixOp::Div),
        N::PERCENT => Some(InfixOp::Mod),
        N::EQ_EQ => Some(InfixOp::Eq),
        N::BANG_EQ => Some(InfixOp::NotEq),
        N::LT => Some(InfixOp::Lt),
        N::GT => Some(InfixOp::Gt),
        N::LT_EQ => Some(InfixOp::LtEq),
        N::GT_EQ => Some(InfixOp::GtEq),
        N::AMP_AMP => Some(InfixOp::And),
        _ => None,
    }
}

fn lower_call(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    let Some(call) = ast::CallExpr::cast(node.clone()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E017));
        return Expr::Null;
    };
    let Some(callee) = call.callee() else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E017));
        return Expr::Null;
    };
    let path = lower_path(&callee);
    let args: Vec<Expr> = call
        .arg_list()
        .into_iter()
        .flat_map(|al| al.syntax().children().collect::<Vec<_>>())
        .map(|arg_node| lower_expr(file_id, &arg_node, diags))
        .collect();
    Expr::Call(path, args)
}

/// Lower a `STRING_LIT` node (`"…" ` with optional `{expr}` interpolation)
/// to `Expr::String`. Escape decoding covers exactly the four sequences the
/// lexer recognizes (`\n \t \\ \"` — `lexer/mod.rs::lex_string_token`);
/// nothing else can reach `STRING_ESCAPE` by construction.
fn lower_string_lit(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    let mut parts: Vec<StringPart> = Vec::new();
    let mut literal = String::new();

    for el in node.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(t) => match t.kind() {
                N::STRING_TEXT => literal.push_str(t.text()),
                N::STRING_ESCAPE => literal.push_str(unescape(t.text())),
                _ => {}
            },
            rowan::NodeOrToken::Node(n) if n.kind() == N::INTERPOLATION => {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
                // INTERPOLATION := L_BRACE expression R_BRACE — the
                // expression is the node's only child node.
                if let Some(inner) = n.children().next() {
                    let inner_expr = lower_expr(file_id, &inner, diags);
                    parts.push(StringPart::Interpolation(Box::new(inner_expr)));
                } else {
                    diags.push(diag(file_id, n.text_range(), DiagnosticCode::E015));
                }
            }
            rowan::NodeOrToken::Node(_) => {}
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(StringPart::Literal(literal));
    }
    Expr::String(StringExpr { parts })
}

fn unescape(raw: &str) -> &'static str {
    match raw {
        "\\n" => "\n",
        "\\t" => "\t",
        "\\\\" => "\\",
        "\\\"" => "\"",
        _ => "",
    }
}
