//! Content and body lowering phase.
//!
//! Defines rich output types for content lines and logic lines, the
//! [`LowerBody`] trait, the [`ContentAccumulator`], and the [`Integrate`]
//! trait that connects them.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{
    AssignOp, Assignment, Content, ContentPart, DiagnosticCode, Expr, Return, Stmt, Tag, TempDecl,
};

use super::context::{LowerScope, LowerSink, Lowered};
use super::divert::LowerDivert;
use super::expr::LowerExpr;
use super::helpers::{content_ends_with_glue, expr_contains_call, name_from_ident};

// ─── Output types ───────────────────────────────────────────────────

/// Structured output from lowering a [`ast::ContentLine`].
///
/// Each variant captures all the information the backbone needs to
/// integrate the result — the node impl decides *what* was produced,
/// the backbone decides *how* to emit it.
pub enum ContentLineOutput {
    /// A content statement with optional trailing divert.
    Content {
        content: Content,
        divert: Option<Stmt>,
        ends_with_glue: bool,
    },
    /// A bare divert with no content (e.g., `-> knot`).
    BareDivert(Stmt),
    /// The line had no content, no divert, no tags.
    Empty,
}

/// Structured output from lowering a [`ast::LogicLine`].
pub enum LogicLineOutput {
    Return(Return),
    TempDecl(TempDecl),
    Assignment(Assignment),
    ExprStmt(Expr),
}

impl LogicLineOutput {
    /// Whether this logic line contains a function call, which requires
    /// an `EndOfLine` after it to match inklecate's behavior.
    pub fn has_call(&self) -> bool {
        match self {
            Self::ExprStmt(expr) => expr_contains_call(expr),
            Self::TempDecl(td) => td.value.as_ref().is_some_and(expr_contains_call),
            Self::Assignment(a) => expr_contains_call(&a.value),
            Self::Return(_) => false,
        }
    }

    /// Convert into a [`Stmt`].
    pub fn into_stmt(self) -> Stmt {
        match self {
            Self::Return(r) => Stmt::Return(r),
            Self::TempDecl(td) => Stmt::TempDecl(td),
            Self::Assignment(a) => Stmt::Assignment(a),
            Self::ExprStmt(e) => Stmt::ExprStmt(e),
        }
    }
}

// ─── LowerBody trait ────────────────────────────────────────────────

/// Extension trait for AST nodes that contribute statements to a body.
///
/// Each impl returns a rich typed output. The backbone uses [`Integrate`]
/// to consume the output and produce the final `Vec<Stmt>`.
pub trait LowerBody {
    type Output;

    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Self::Output>;
}

// ─── ContentLine ────────────────────────────────────────────────────

impl LowerBody for ast::ContentLine {
    type Output = ContentLineOutput;

    fn lower_body(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<ContentLineOutput> {
        let parts = self
            .mixed_content()
            .map(|mc| lower_content_node_children(mc.syntax(), scope, sink))
            .unwrap_or_default();
        let tags = lower_tags(self.tags(), scope, sink);

        // If this line has only a divert (no content), emit a divert statement.
        if parts.is_empty() && tags.is_empty() {
            if let Some(dn) = self.divert()
                && let Ok(stmt) = dn.lower_divert(scope, sink)
            {
                return Ok(ContentLineOutput::BareDivert(stmt));
            }
            return Ok(ContentLineOutput::Empty);
        }

        let ends_with_glue = content_ends_with_glue(&parts);
        let divert = self
            .divert()
            .and_then(|dn| dn.lower_divert(scope, sink).ok());

        Ok(ContentLineOutput::Content {
            content: Content {
                ptr: Some(SyntaxNodePtr::from_node(self.syntax())),
                parts,
                tags,
            },
            divert,
            ends_with_glue,
        })
    }
}

// ─── LogicLine ──────────────────────────────────────────────────────

impl LowerBody for ast::LogicLine {
    type Output = LogicLineOutput;

    fn lower_body(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<LogicLineOutput> {
        let range = self.syntax().text_range();

        if let Some(ret) = self.return_stmt() {
            let value = ret.value().and_then(|e| e.lower_expr(scope, sink).ok());
            return Ok(LogicLineOutput::Return(Return {
                ptr: Some(ast::AstPtr::new(&ret)),
                value,
                onwards_args: Vec::new(),
            }));
        }

        if let Some(temp) = self.temp_decl() {
            let ident = temp
                .identifier()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
            let name = name_from_ident(&ident)
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
            let value = temp.value().and_then(|e| e.lower_expr(scope, sink).ok());
            // Register the local *after* lowering the initializer so
            // `~ temp x = x` doesn't accidentally self-reference.
            sink.add_local(crate::symbols::LocalSymbol {
                name: name.text.clone(),
                range: name.range,
                scope: scope.to_scope(),
                kind: crate::SymbolKind::Temp,
                param_detail: None,
            });
            return Ok(LogicLineOutput::TempDecl(TempDecl {
                ptr: ast::AstPtr::new(&temp),
                name,
                value,
            }));
        }

        if let Some(assign) = self.assignment() {
            let target = assign
                .target()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))
                .and_then(|e| e.lower_expr(scope, sink))?;
            let value = assign
                .value()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))
                .and_then(|e| e.lower_expr(scope, sink))?;
            let op = assign
                .op_token()
                .map_or(AssignOp::Set, |tok| match tok.kind() {
                    brink_syntax::SyntaxKind::PLUS_EQ => AssignOp::Add,
                    brink_syntax::SyntaxKind::MINUS_EQ => AssignOp::Sub,
                    _ => AssignOp::Set,
                });
            return Ok(LogicLineOutput::Assignment(Assignment {
                ptr: ast::AstPtr::new(&assign),
                target,
                op,
                value,
            }));
        }

        // Bare expression statement: ~ expr
        for child in self.syntax().children() {
            if let Some(expr) = ast::Expr::cast(child) {
                let e = expr.lower_expr(scope, sink)?;
                return Ok(LogicLineOutput::ExprStmt(e));
            }
        }

        Err(sink.diagnose(range, DiagnosticCode::E014))
    }
}

// ─── Content accumulator ────────────────────────────────────────────

/// Accumulates statements from body-level outputs.
///
/// The backbone dispatches to node trait impls, which return typed outputs.
/// The accumulator consumes those outputs via [`Integrate`] and builds
/// the final `Vec<Stmt>`.
#[derive(Default)]
pub struct ContentAccumulator {
    stmts: Vec<Stmt>,
}

impl ContentAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_stmt(&mut self, stmt: Stmt) {
        self.stmts.push(stmt);
    }

    pub fn finish(self) -> Vec<Stmt> {
        self.stmts
    }
}

// ─── Integrate trait ────────────────────────────────────────────────

/// Tells the [`ContentAccumulator`] how to consume a typed output.
///
/// Each output type from `LowerBody` has a corresponding `Integrate` impl
/// that converts it into statement pushes. Missing an impl is a compile error.
pub trait Integrate<T> {
    fn integrate(&mut self, output: T);
}

impl Integrate<ContentLineOutput> for ContentAccumulator {
    fn integrate(&mut self, output: ContentLineOutput) {
        match output {
            ContentLineOutput::Content {
                content,
                divert,
                ends_with_glue,
            } => {
                self.stmts.push(Stmt::Content(content));
                if let Some(d) = divert {
                    self.stmts.push(d);
                } else if !ends_with_glue {
                    self.stmts.push(Stmt::EndOfLine);
                }
            }
            ContentLineOutput::BareDivert(stmt) => {
                self.stmts.push(stmt);
            }
            ContentLineOutput::Empty => {}
        }
    }
}

impl Integrate<LogicLineOutput> for ContentAccumulator {
    fn integrate(&mut self, output: LogicLineOutput) {
        let needs_eol = output.has_call();
        self.stmts.push(output.into_stmt());
        if needs_eol {
            self.stmts.push(Stmt::EndOfLine);
        }
    }
}

// ─── Content child helpers ──────────────────────────────────────────

/// Lower the inline content children of a syntax node (`TEXT`, `GLUE`, `ESCAPE`,
/// `INLINE_LOGIC`) into a `Vec` of `ContentPart`s.
pub fn lower_content_node_children(
    node: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<ContentPart> {
    use brink_syntax::SyntaxKind;

    let mut parts = Vec::new();
    for child in node.children_with_tokens() {
        if let rowan::NodeOrToken::Node(child_node) = child {
            match child_node.kind() {
                SyntaxKind::TEXT => {
                    let text = child_node.text().to_string();
                    if !text.is_empty() {
                        parts.push(ContentPart::Text(text));
                    }
                }
                SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
                SyntaxKind::ESCAPE => {
                    let text = child_node.text().to_string();
                    if text.len() > 1 {
                        parts.push(ContentPart::Text(text[1..].to_string()));
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(inline) = ast::InlineLogic::cast(child_node) {
                        lower_inline_logic(&inline, &mut parts, scope, sink);
                    }
                }
                // DIVERT_NODE and TAGS appear as siblings in
                // MIXED_CONTENT — handled by the caller.
                SyntaxKind::DIVERT_NODE | SyntaxKind::TAGS => {}
                other => {
                    debug_assert!(
                        other.is_token(),
                        "unexpected node SyntaxKind in lower_content_node_children: {other:?}"
                    );
                }
            }
        }
    }
    parts
}

/// Lower an inline logic node into content parts.
///
/// Delegates to [`conditional::lower_inline_logic_into_parts`] which handles
/// value interpolation, inline conditionals, and inline sequences.
fn lower_inline_logic(
    inline: &ast::InlineLogic,
    parts: &mut Vec<ContentPart>,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) {
    super::conditional::lower_inline_logic_into_parts(inline, parts, scope, sink);
}

/// Lower optional tags into a Vec of Tag.
pub fn lower_tags(
    tags: Option<ast::Tags>,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<Tag> {
    tags.map_or_else(Vec::new, |t| {
        t.tags().map(|tag| lower_tag(&tag, scope, sink)).collect()
    })
}

fn lower_tag(tag: &ast::Tag, scope: &LowerScope, sink: &mut impl LowerSink) -> Tag {
    use brink_syntax::SyntaxKind::HASH;

    let mut parts = Vec::new();
    let mut text_buf = String::new();
    let mut first = true;

    for child in tag.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                if first && tok.kind() == HASH {
                    first = false;
                    continue;
                }
                first = false;
                text_buf.push_str(tok.text());
            }
            rowan::NodeOrToken::Node(node) => {
                first = false;
                if node.kind() == brink_syntax::SyntaxKind::INLINE_LOGIC {
                    if !text_buf.is_empty() {
                        parts.push(ContentPart::Text(std::mem::take(&mut text_buf)));
                    }
                    if let Some(inline) = ast::InlineLogic::cast(node) {
                        lower_inline_logic(&inline, &mut parts, scope, sink);
                    }
                }
            }
        }
    }
    let remaining = text_buf.trim_end().to_string();
    if !remaining.is_empty() {
        parts.push(ContentPart::Text(remaining));
    }
    if let Some(ContentPart::Text(t)) = parts.first_mut() {
        *t = t.trim_start().to_string();
        if t.is_empty() {
            parts.remove(0);
        }
    }

    Tag {
        parts,
        ptr: ast::AstPtr::new(tag),
    }
}
