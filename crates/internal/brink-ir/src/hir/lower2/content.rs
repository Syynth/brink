//! Content and body lowering phase.
//!
//! Defines rich output types for content lines and logic lines, the
//! [`LowerBody`] trait, the [`BodyBackend`] trait, and the
//! [`ContentAccumulator`] that ties everything together.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{
    AssignOp, Assignment, Block, Content, ContentPart, DiagnosticCode, Expr, Return, Stmt, Tag,
    TempDecl,
};

use super::conditional::{lower_inline_logic_into_parts, lower_multiline_block_from_inline};
use super::context::{LowerScope, LowerSink, Lowered};
use super::divert::LowerDivert;
use super::expr::LowerExpr;
use super::helpers::{content_ends_with_glue, expr_contains_call, name_from_ident};

// ─── Output types ───────────────────────────────────────────────────

/// Structured output from lowering a [`ast::ContentLine`].
pub enum ContentLineOutput {
    /// A content statement with optional trailing divert.
    Content {
        content: Content,
        divert: Option<Stmt>,
        ends_with_glue: bool,
    },
    /// A bare divert with no content (e.g., `-> knot`).
    BareDivert(Stmt),
    /// The content line wraps a promoted multiline block.
    /// All trailing content and divert are pre-lowered.
    PromotedBlock {
        stmt: Stmt,
        trailing_content: Option<Content>,
        divert: Option<Stmt>,
        needs_eol: bool,
    },
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
        // Check if this content line wraps a multiline block-level construct.
        if let Some(mc) = self.mixed_content()
            && let Some(il) = mc.inline_logics().next()
            && let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink)
        {
            let il_syntax = il.syntax().clone();
            let mut past_promoted = false;
            let mut trailing_parts = Vec::new();
            for child in mc.syntax().children_with_tokens() {
                if let rowan::NodeOrToken::Node(ref child_node) = child
                    && *child_node == il_syntax
                {
                    past_promoted = true;
                    continue;
                }
                if !past_promoted {
                    continue;
                }
                if let rowan::NodeOrToken::Node(child_node) = child {
                    match child_node.kind() {
                        brink_syntax::SyntaxKind::TEXT => {
                            let text = child_node.text().to_string();
                            if !text.is_empty() {
                                trailing_parts.push(ContentPart::Text(text));
                            }
                        }
                        brink_syntax::SyntaxKind::GLUE_NODE => {
                            trailing_parts.push(ContentPart::Glue);
                        }
                        brink_syntax::SyntaxKind::ESCAPE => {
                            let text = child_node.text().to_string();
                            if text.len() > 1 {
                                trailing_parts.push(ContentPart::Text(text[1..].to_string()));
                            }
                        }
                        brink_syntax::SyntaxKind::INLINE_LOGIC => {
                            if let Some(inline) = ast::InlineLogic::cast(child_node) {
                                lower_inline_logic_into_parts(
                                    &inline,
                                    &mut trailing_parts,
                                    scope,
                                    sink,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }

            let trailing_content = if trailing_parts.is_empty() {
                None
            } else {
                Some(Content {
                    ptr: None,
                    parts: trailing_parts,
                    tags: Vec::new(),
                })
            };
            let divert = self
                .divert()
                .and_then(|dn| dn.lower_divert(scope, sink).ok());
            let ends_glue = trailing_content
                .as_ref()
                .is_some_and(|c| content_ends_with_glue(&c.parts));
            let needs_eol = (trailing_content.is_some() && !ends_glue && divert.is_none())
                || (trailing_content.is_none() && divert.is_none());

            return Ok(ContentLineOutput::PromotedBlock {
                stmt,
                trailing_content,
                divert,
                needs_eol,
            });
        }

        let parts = self
            .mixed_content()
            .map(|mc| lower_content_node_children(mc.syntax(), scope, sink))
            .unwrap_or_default();
        let tags = lower_tags(self.tags(), scope, sink);

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

        for child in self.syntax().children() {
            if let Some(expr) = ast::Expr::cast(child) {
                let e = expr.lower_expr(scope, sink)?;
                return Ok(LogicLineOutput::ExprStmt(e));
            }
        }

        Err(sink.diagnose(range, DiagnosticCode::E014))
    }
}

// ─── BodyBackend trait ──────────────────────────────────────────────

/// Backend for the [`ContentAccumulator`]. Determines where flushed
/// statements go — directly into a `Vec<Stmt>`, or into weave items.
pub trait BodyBackend {
    fn push_stmt(&mut self, stmt: Stmt);
    fn finish(self) -> Block;
}

/// Direct backend: collects statements into a `Block`. For branch bodies.
#[derive(Default)]
pub struct DirectBackend {
    stmts: Vec<Stmt>,
}

impl DirectBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BodyBackend for DirectBackend {
    fn push_stmt(&mut self, stmt: Stmt) {
        self.stmts.push(stmt);
    }

    fn finish(self) -> Block {
        Block {
            label: None,
            stmts: self.stmts,
        }
    }
}

// ─── ContentAccumulator ─────────────────────────────────────────────

/// Accumulates content parts and block-level statements, flushing
/// buffered parts when block-level nodes appear.
///
/// Generic over [`BodyBackend`] — the backend determines where results go.
pub struct ContentAccumulator<B: BodyBackend> {
    backend: B,
    parts: Vec<ContentPart>,
    last_pushed_was_content: bool,
}

impl<B: BodyBackend> ContentAccumulator<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            parts: Vec::new(),
            last_pushed_was_content: false,
        }
    }

    // ── Content part buffering ──────────────────────────────────

    pub fn push_text(&mut self, text: String) {
        if !text.is_empty() {
            self.parts.push(ContentPart::Text(text));
        }
    }

    pub fn push_glue(&mut self) {
        self.parts.push(ContentPart::Glue);
    }

    pub fn push_escape(&mut self, text: &str) {
        if text.len() > 1 {
            self.parts.push(ContentPart::Text(text[1..].to_string()));
        }
    }

    pub fn has_buffered_parts(&self) -> bool {
        !self.parts.is_empty()
    }

    pub fn ends_with_glue(&self) -> bool {
        content_ends_with_glue(&self.parts)
    }

    // ── Flushing ────────────────────────────────────────────────

    /// Flush buffered content parts as a `Stmt::Content`.
    pub fn flush(&mut self) {
        if !self.parts.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr: None,
                parts: std::mem::take(&mut self.parts),
                tags: Vec::new(),
            }));
            self.last_pushed_was_content = true;
        }
    }

    /// Flush with tags.
    pub fn flush_with_tags(&mut self, tags: Vec<Tag>) {
        if !self.parts.is_empty() || !tags.is_empty() {
            self.backend.push_stmt(Stmt::Content(Content {
                ptr: None,
                parts: std::mem::take(&mut self.parts),
                tags,
            }));
            self.last_pushed_was_content = true;
        }
    }

    pub fn push_eol(&mut self) {
        self.backend.push_stmt(Stmt::EndOfLine);
        self.last_pushed_was_content = false;
    }

    pub fn last_was_content(&self) -> bool {
        self.last_pushed_was_content
    }

    // ── Block-level dispatch via traits ─────────────────────────

    /// Flush, lower a node via [`LowerBody`], and integrate its output.
    pub fn handle_content_line(
        &mut self,
        cl: &ast::ContentLine,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) {
        if let Ok(output) = cl.lower_body(scope, sink) {
            self.integrate_content_line(output);
        }
    }

    /// Flush, lower a logic line, and integrate.
    pub fn handle_logic_line(
        &mut self,
        ll: &ast::LogicLine,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) {
        self.flush();
        if let Ok(output) = ll.lower_body(scope, sink) {
            let needs_eol = output.has_call();
            self.backend.push_stmt(output.into_stmt());
            self.last_pushed_was_content = false;
            if needs_eol {
                self.push_eol();
            }
        }
    }

    /// Flush and lower a divert node.
    pub fn handle_divert(
        &mut self,
        dn: &ast::DivertNode,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) {
        self.flush();
        if let Ok(stmt) = dn.lower_divert(scope, sink) {
            self.backend.push_stmt(stmt);
            self.last_pushed_was_content = false;
        }
    }

    /// Try to promote inline logic to a block-level statement,
    /// or fall back to buffering inline content parts.
    ///
    /// Returns `true` if the inline logic was promoted to a block-level
    /// statement, `false` if it was inlined as content parts.
    pub fn handle_inline_logic(
        &mut self,
        il: &ast::InlineLogic,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> bool {
        if let Some(stmt) = lower_multiline_block_from_inline(il, scope, sink) {
            self.flush();
            self.backend.push_stmt(stmt);
            self.last_pushed_was_content = false;
            true
        } else {
            lower_inline_logic_into_parts(il, &mut self.parts, scope, sink);
            false
        }
    }

    /// Push a raw statement (bypasses part buffering).
    pub fn push_stmt(&mut self, stmt: Stmt) {
        self.last_pushed_was_content = matches!(&stmt, Stmt::Content(_));
        self.backend.push_stmt(stmt);
    }

    // ── Backend access ───────────────────────────────────────────

    /// Access the backend directly for backend-specific operations
    /// (e.g., `WeaveBackend::push_choice`).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    // ── Finish ──────────────────────────────────────────────────

    pub fn finish(mut self) -> Block {
        self.flush();
        self.backend.finish()
    }

    // ── Private: integrate outputs ──────────────────────────────

    fn integrate_content_line(&mut self, output: ContentLineOutput) {
        match output {
            ContentLineOutput::Content {
                content,
                divert,
                ends_with_glue,
            } => {
                self.backend.push_stmt(Stmt::Content(content));
                self.last_pushed_was_content = true;
                if let Some(d) = divert {
                    self.backend.push_stmt(d);
                    self.last_pushed_was_content = false;
                } else if !ends_with_glue {
                    self.push_eol();
                }
            }
            ContentLineOutput::BareDivert(stmt) => {
                self.backend.push_stmt(stmt);
                self.last_pushed_was_content = false;
            }
            ContentLineOutput::PromotedBlock {
                stmt,
                trailing_content,
                divert,
                needs_eol,
            } => {
                self.backend.push_stmt(stmt);
                self.last_pushed_was_content = false;
                if let Some(tc) = trailing_content {
                    self.backend.push_stmt(Stmt::Content(tc));
                    self.last_pushed_was_content = true;
                }
                if let Some(d) = divert {
                    self.backend.push_stmt(d);
                    self.last_pushed_was_content = false;
                } else if needs_eol {
                    self.push_eol();
                }
            }
            ContentLineOutput::Empty => {}
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
                        lower_inline_logic_into_parts(&inline, &mut parts, scope, sink);
                    }
                }
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

/// Lower optional tags into a `Vec<Tag>`.
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
                        lower_inline_logic_into_parts(&inline, &mut parts, scope, sink);
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
