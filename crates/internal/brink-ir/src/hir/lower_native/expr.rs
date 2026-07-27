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
//! internal shape instead of a balanced-token blob"). The code-ground
//! *statement* grammar itself (`let`/assign/if/while/for/until/return/
//! break/continue) lives in `super::control_flow`, B0.8 Waves A/B/B-tail —
//! this module's only seam with it is the `STMT_BLOCK` atom case below
//! (blocks-as-values, still unrepresentable as a value — see that arm's
//! doc).
//!
//! **UFCS (issues #1322, #1482):** `lower_call` below lowers a
//! multi-segment dotted callee (`x.foo(y)`) to `Expr::Call(Path, args)`
//! unmodified, keeping every segment. That is deliberate and load-bearing:
//! the ruled field-access-wins/free-fn resolution is **type-directed**, so
//! it cannot be decided here — it lives in `brink-analyzer::ufcs` (B3a,
//! D1–D5 RULED 2026-07-26), which splits this path into a receiver (every
//! segment but the last) and a method name, infers the receiver's type, and
//! records its verdict in a side table for LIR lowering and IDE hover. This
//! lowering's only job is to preserve the shape; see
//! `crates/internal/brink-ir/tests/b08_native_wave_b_tail.rs`'s
//! `ufcs_call_shape_lowers_the_full_dotted_callee_path` for the pin on that
//! contract. ink's own grammar cannot express the shape at all (E104,
//! `hir/lower/expr/references.rs::CallExpr`), which is what keeps the ink
//! corpus out of the analyzer pass by construction.
//!
//! **Construction (B5, issue #1464)**: `CONSTRUCT_LITERAL` — the
//! `TypeName { … }` initializer — lowers through [`lower_construct`] below,
//! which dispatches on the `construct` protocol registry
//! (`crate::hir::construct`) rather than on a closed set of names baked
//! into this match, per the #1103 ruling. Every registered target desugars
//! to an HIR shape that already existed.
//!
//! **Lambdas (issue #1685)**: `LAMBDA_EXPR` lowers for real through
//! [`super::lambda::lower_lambda`] → [`Expr::Lambda`]. The E129 fence that
//! stood here — "unlowered until the code sitting rules a real
//! anonymous-body node" — is gone: the 2026-07-19 airport sitting ruled the
//! whole surface (Rust pipes with colon returns, by-value capture as the
//! only mode, single-expression or braced-block bodies), so the
//! anonymous-body node the fence was waiting on now exists. See
//! [`super::lambda`]'s module doc for how each half of that ruling lands.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use super::provenance::native_provenance;
use crate::hir::FileId;
use crate::hir::construct::{ConstructForm, ConstructTarget};
use crate::provenance::NodeClass;
use crate::{
    Diagnostic, DiagnosticCode, Expr, FloatBits, InfixExpr, InfixOp, Name, Path, PrefixOp,
};
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
        N::CONSTRUCT_LITERAL => lower_construct(file_id, node, diags),
        N::LAMBDA_EXPR => super::lambda::lower_lambda(file_id, node, diags),
        N::STMT_BLOCK => {
            // Blocks-as-values (decision-log 2026-07-23 item 2) has no HIR
            // representation yet — no `Expr::Block` variant exists, and
            // NF-2 forbids minting one in this slice. But the block's own
            // *statements* (B0.8 Wave B's `let`/assign/expr/if/while/for/
            // until control flow, issue #1177) DO have a real lowering
            // target (the existing `~ { … }` T1b closed set) and are
            // lowered here for their diagnostics and reachability — this
            // is the one production call site that exercises
            // `control_flow::lower_stmt_block` from a real `.brink` file
            // (`var x = { if a { … } };`), not just a differential test
            // fixture. The block's own *value* still can't be produced, so
            // this arm still ends in E129 — "loud, not silent", and an
            // honest one: the statements inside genuinely have nowhere to
            // live as an `Expr`. (A lambda's braced body is *not* this
            // case — it is a function body with a real home, lowered by
            // `super::lambda` into `LambdaBody::Block`, tail included.)
            if let Some(sb) = ast::StmtBlock::cast(node.clone()) {
                let _ = super::control_flow::lower_stmt_block(file_id, &sb, diags);
            }
            diags.push(diag(file_id, node.text_range(), DiagnosticCode::E129));
            Expr::Null
        }
        _ => {
            // Anything else the expr grammar can produce that this slice
            // doesn't recognize (e.g. a malformed ERROR node reaching
            // here) — loud, not a silent Null with no trace.
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
        // The whole `lhs op rhs` node's own range — the identity key a side
        // table addresses this node by (issue #1517).
        let ptr = native_provenance(file_id, NodeClass::Infix, node);
        Expr::Infix(InfixExpr::new(ptr, lhs, op, rhs))
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
        // B1 `or`-coalescing (`docs/stdlib-spec.md` §1.6a, issue #1460):
        // the single `KW_OR` token, distinct from `is_double_pipe()`'s
        // `||` above — `InfixOp::Coalesce`, never `InfixOp::Or` (ink's
        // boolean disjunction, oracle-frozen and unreachable from this
        // lowering path).
        N::KW_OR => Some(InfixOp::Coalesce),
        _ => None,
    }
}

/// Lower a `TypeName { … }` construction literal (B5, issue #1464; #1103
/// RULED 2026-07-23, `docs/stdlib-spec.md` §9.6).
///
/// This is the **dispatch** half of the ruling: the parser gave us one
/// grammar (a type path plus element / pair entries), and the `construct`
/// registry ([`crate::hir::construct::ConstructTarget`]) decides what it
/// means. Every registered std target desugars into an HIR shape that
/// already exists — the ruling's whole point is that construction is a
/// protocol over the existing value model, not a new node kind:
///
/// | written | registry entry | HIR |
/// |---|---|---|
/// | `Map { "a": 1 }` | [`ConstructTarget::Map`] | [`Expr::MapLiteral`] |
/// | `Flags { Red, Blue }` | [`ConstructTarget::Flags`] | [`Expr::ListLiteral`] |
/// | `Weighted { 3: "gold" }` | [`ConstructTarget::Weighted`] | `weighted(3, "gold")` ([`Expr::Call`]) |
/// | `Point { x: 1, y: 2 }` | *(unregistered)* | [`Expr::StructLiteral`] |
///
/// The unregistered fall-through is deliberate and is what keeps the
/// std-only fence honest: user types do not *register* anything this round
/// (the `impl` spelling is still deferred), they simply keep the declared-
/// struct reading the compiler already had.
fn lower_construct(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) -> Expr {
    let Some(lit) = ast::ConstructLiteral::cast(node.clone()) else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let Some(type_path) = lit.type_path() else {
        diags.push(diag(file_id, node.text_range(), DiagnosticCode::E015));
        return Expr::Null;
    };
    let path = lower_path(&type_path);
    let segments: Vec<String> = path.segments.iter().map(|s| s.text.clone()).collect();
    let entries: Vec<ast::ConstructEntry> = lit.entries().collect();

    let target = ConstructTarget::lookup(&segments);
    let expected = target.map_or(ConstructForm::Pair, ConstructTarget::form);
    if !entries_match_form(&entries, expected) {
        diags.push(form_mismatch(file_id, node, &segments, expected));
        return Expr::Null;
    }

    match target {
        Some(ConstructTarget::Map) => Expr::MapLiteral(crate::MapLiteral {
            ptr: native_provenance(file_id, NodeClass::MapLiteral, node),
            entries: entries
                .iter()
                .map(|e| {
                    let at = e.syntax().text_range();
                    (
                        lower_entry_part(file_id, e.key().as_ref(), at, diags),
                        lower_entry_part(file_id, e.value().as_ref(), at, diags),
                    )
                })
                .collect(),
        }),
        Some(ConstructTarget::Flags) => {
            // A flags value names declared members, so each element must be
            // a bare name — the same shape ink's `(A, B)` list literal has.
            let mut items = Vec::with_capacity(entries.len());
            for entry in &entries {
                let member = entry
                    .value()
                    .and_then(ast::PathExpr::cast)
                    .and_then(|p| p.path());
                let Some(p) = member else {
                    diags.push(diag(
                        file_id,
                        entry.syntax().text_range(),
                        DiagnosticCode::E139,
                    ));
                    return Expr::Null;
                };
                items.push(lower_path(&p));
            }
            Expr::ListLiteral(items)
        }
        Some(ConstructTarget::Weighted) => {
            // The **total** literal (#1103 cascade ruling B): desugars to
            // the existing `weighted(w, v, …)` flattened-pair intrinsic,
            // which already faults on an invalid table (`E120` statically,
            // `WeightedBadWeight` at runtime). The validating
            // `construct → Option` member is ratified but unspelled, so
            // there is deliberately no second form here.
            let mut args = Vec::with_capacity(entries.len() * 2);
            for entry in &entries {
                let at = entry.syntax().text_range();
                args.push(lower_entry_part(file_id, entry.key().as_ref(), at, diags));
                args.push(lower_entry_part(file_id, entry.value().as_ref(), at, diags));
            }
            Expr::Call(
                Path {
                    segments: vec![Name {
                        text: "weighted".to_string(),
                        range: type_path.syntax().text_range(),
                    }],
                    range: type_path.syntax().text_range(),
                },
                args,
            )
        }
        None => {
            let mut fields = Vec::with_capacity(entries.len());
            for entry in &entries {
                let Some(name) = entry.key().and_then(|k| bare_field_name(&k)) else {
                    diags.push(diag(
                        file_id,
                        entry.syntax().text_range(),
                        DiagnosticCode::E139,
                    ));
                    return Expr::Null;
                };
                let at = entry.syntax().text_range();
                fields.push((
                    name,
                    lower_entry_part(file_id, entry.value().as_ref(), at, diags),
                ));
            }
            Expr::StructLiteral(crate::StructLiteral {
                ptr: native_provenance(file_id, NodeClass::StructLiteral, node),
                shape: Name {
                    text: segments.last().cloned().unwrap_or_default(),
                    range: type_path.syntax().text_range(),
                },
                fields,
            })
        }
    }
}

/// Whether every entry is in `expected`'s form. An empty literal
/// (`Map { }`, `Flags { }`) vacuously matches either form — the ruled
/// grammar accepts it and it constructs the empty value.
fn entries_match_form(entries: &[ast::ConstructEntry], expected: ConstructForm) -> bool {
    entries
        .iter()
        .all(|e| e.is_pair() == (expected == ConstructForm::Pair))
}

/// The `E139` diagnostic for a form mismatch, naming both the target and
/// the form it does construct from.
fn form_mismatch(
    file: FileId,
    node: &SyntaxNode,
    segments: &[String],
    expected: ConstructForm,
) -> Diagnostic {
    let name = segments.last().map_or("<unnamed>", String::as_str);
    Diagnostic {
        file,
        range: node.text_range(),
        message: format!(
            "`{name} {{ … }}` constructs from {} entries",
            expected.label()
        ),
        code: DiagnosticCode::E139,
    }
}

/// Lower one side of a construction entry, pushing `E015` at `fallback`
/// (and returning `Null`) if the parser left the slot empty — never a
/// silent drop.
fn lower_entry_part(
    file_id: FileId,
    part: Option<&SyntaxNode>,
    fallback: rowan::TextRange,
    diags: &mut Vec<Diagnostic>,
) -> Expr {
    let Some(n) = part else {
        // Only reachable from a malformed parse (`Map { : 1 }`); the caller
        // has already checked the entry's *form*.
        diags.push(diag(file_id, fallback, DiagnosticCode::E015));
        return Expr::Null;
    };
    lower_expr(file_id, n, diags)
}

/// A struct-literal field key must be a bare, single-segment name — the
/// field form. `Point { 1: 2 }` or `Point { a.b: 2 }` is `E139`.
fn bare_field_name(key: &SyntaxNode) -> Option<Name> {
    let path = ast::PathExpr::cast(key.clone())?.path()?;
    let lowered = lower_path(&path);
    match lowered.segments.as_slice() {
        [only] => Some(only.clone()),
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
                N::STRING_ESCAPE => literal.push_str(unescape_string_token(t.text())),
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

/// Decode one `STRING_ESCAPE` token (`\n \t \\ \"`) to its literal text.
/// Shared with [`super::module`]'s `@[was("…")]` path extraction — the escape
/// set is fixed by the lexer (`lexer/mod.rs::lex_string_token`), so anything
/// else is unreachable and maps to the empty string.
pub(super) fn unescape_string_token(raw: &str) -> &'static str {
    match raw {
        "\\n" => "\n",
        "\\t" => "\t",
        "\\\\" => "\\",
        "\\\"" => "\"",
        _ => "",
    }
}
